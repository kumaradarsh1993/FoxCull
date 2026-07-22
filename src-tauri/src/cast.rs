//! Chromecast ("cast to TV") backend.
//!
//! FoxCull runs on a laptop; the user wants to throw the photo/video they're
//! reviewing onto a Chromecast-built-in TV (their Sony Bravia Google TV) at the
//! best quality the Cast protocol allows. Three moving parts live in this file:
//!
//! 1. **Discovery** — an mDNS browse for `_googlecast._tcp.local.` (the
//!    `mdns-sd` crate, pure Rust).
//! 2. **A tiny local media HTTP server** — the TV can't read the laptop's disk,
//!    so it fetches the file over the LAN from us. It serves ONLY files we've
//!    explicitly registered (an allowlist keyed by an unguessable token — no
//!    arbitrary paths, no directory listing) and speaks HTTP Range so the TV
//!    can seek within a video.
//! 3. **Cast control** — we speak the CASTV2 protocol directly (TLS on 8009,
//!    length-prefixed protobuf frames carrying JSON payloads): CONNECT, launch
//!    the Default Media Receiver, LOAD the media URL, heartbeat, STOP.
//!
//! ## Why we hand-roll the protocol instead of using `rust_cast`
//! `rust_cast` (the obvious crate) depends on `rustls` with default features,
//! whose crypto backend is `aws-lc-rs` — that needs cmake + a C compiler +
//! NASM and is flatly broken on the `x86_64-pc-windows-gnu` toolchain this
//! machine builds with. The alternative rustls backend (`ring`) also needs a C
//! compiler. Our hard constraint is "no C toolchain," so we instead use
//! `native-tls`, which on Windows is the OS SChannel (pure-Rust `schannel`
//! FFI, no C build) and on macOS is Security.framework — both C-free and
//! cross-platform for the user's Windows-now / Mac-next trajectory.
//!
//! ## Why we hand-encode the protobuf instead of `prost`
//! The CASTV2 wire message (`CastMessage`) is a proto2 message with several
//! `required` scalar fields. `prost` encodes with proto3 semantics, which OMITS
//! scalar fields equal to their default — so `protocol_version = 0` and
//! `payload_type = 0` (STRING) would never be written, and the receiver's
//! proto2 parser rejects the frame for "missing required field." Hand-encoding
//! the handful of fields (below) is a few dozen lines, always emits the
//! required fields, needs no `protoc`, and keeps the dependency tree C-free.
//!
//! ## Quality
//! Video is served byte-for-byte from the original file — NO transcoding. The
//! Sony TV's Default Media Receiver hardware-decodes H.264/HEVC to 4K60, so the
//! untouched original is the highest-quality path. Photos are served full-res
//! too, though note the Default Media Receiver caps image RENDERING to roughly
//! 720p–1080p (it downscales); serving a custom receiver app for true full-res
//! stills is a later work item.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use native_tls::{TlsConnector, TlsStream};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::State;

// ---------------------------------------------------------------------------
// CASTV2 constants
// ---------------------------------------------------------------------------

/// Virtual sender/receiver ids used on every frame. `receiver-0` is the TV's
/// platform receiver; individual launched apps get their own transport id.
const SENDER_ID: &str = "sender-0";
const PLATFORM_RECEIVER_ID: &str = "receiver-0";

/// The Google-published Default Media Receiver. Handles bare audio/video/image
/// URLs with no custom receiver app to publish.
const DEFAULT_MEDIA_RECEIVER: &str = "CC1AD845";

const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

// ===========================================================================
// Public data shapes (serialized to the frontend)
// ===========================================================================

/// A discovered Cast device. `addr` is an IPv4 dotted string; `port` is almost
/// always 8009.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CastDevice {
    /// Stable-ish id from the TXT `id` record (device UUID) or the fullname.
    pub id: String,
    /// Friendly name from the TXT `fn` record (e.g. "Living Room TV").
    pub name: String,
    pub addr: String,
    pub port: u16,
}

/// Current cast state for the UI's button.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CastStatus {
    pub connected: bool,
    pub device_name: Option<String>,
    pub playing_path: Option<String>,
}

impl CastStatus {
    fn disconnected() -> Self {
        CastStatus { connected: false, device_name: None, playing_path: None }
    }
}

// ===========================================================================
// Managed Tauri state
// ===========================================================================

/// Holds the (lazily started) media server and the (optional) live connection
/// to whichever device we're currently casting to.
#[derive(Default)]
pub struct CastState {
    server: Mutex<Option<MediaServer>>,
    conn: Mutex<Option<CastConn>>,
}

// ===========================================================================
// 1. Discovery
// ===========================================================================

/// Browse mDNS for Cast devices for up to `timeout_ms` and return what
/// resolved. Deduplicated by fullname so a device advertising over several
/// interfaces appears once.
#[tauri::command]
pub fn cast_discover(timeout_ms: u64) -> Result<Vec<CastDevice>, String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let mdns = ServiceDaemon::new().map_err(|e| format!("mdns init failed: {e}"))?;
    let rx = mdns
        .browse("_googlecast._tcp.local.")
        .map_err(|e| format!("mdns browse failed: {e}"))?;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms.clamp(200, 15_000));
    let mut found: HashMap<String, CastDevice> = HashMap::new();

    // Drain resolve events until the deadline. `recv_timeout` lets us keep the
    // window tight rather than blocking the whole timeout on a single event.
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                // Prefer IPv4 — Cast control + our media URLs use v4 on the LAN.
                let addr = match info.get_addresses_v4().into_iter().next() {
                    Some(ip) => ip.to_string(),
                    None => continue, // v6-only advert; skip, we can't build a v4 URL
                };
                let name = info
                    .get_property_val_str("fn")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| friendly_from_fullname(info.get_fullname()));
                let id = info
                    .get_property_val_str("id")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| info.get_fullname().to_string());
                found.insert(
                    info.get_fullname().to_string(),
                    CastDevice { id, name, addr, port: info.get_port() },
                );
            }
            Ok(_) => {} // SearchStarted / ServiceFound / etc. — nothing to collect
            Err(_) => break, // timed out (deadline reached) or channel closed
        }
    }

    let _ = mdns.shutdown();
    let mut list: Vec<CastDevice> = found.into_values().collect();
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    crate::log::line(&format!(
        "cast: discovery found {} device(s): {}",
        list.len(),
        list.iter().map(|d| format!("{} @{}", d.name, d.addr)).collect::<Vec<_>>().join(", "),
    ));
    Ok(list)
}

/// Turn `My-TV-abcdef._googlecast._tcp.local.` into a rough display name when
/// the friendly-name TXT record is missing.
fn friendly_from_fullname(full: &str) -> String {
    full.split("._googlecast").next().unwrap_or(full).replace('-', " ")
}

// ===========================================================================
// 2. Local media HTTP server
// ===========================================================================

/// A running HTTP server plus the allowlist of files it will serve. Kept alive
/// for the whole app lifetime once started (cheap: a few idle worker threads).
struct MediaServer {
    /// `http://<lan-ip>:<port>` — the base the TV fetches from.
    base_url: String,
    /// token -> absolute file path. The ONLY files this server will serve. A
    /// request for any token not in this map is a 404; there is no path-based
    /// routing at all, so directory traversal is structurally impossible.
    allow: Arc<Mutex<HashMap<String, PathBuf>>>,
    /// Held only to keep the listening socket (and thus the worker loops) alive.
    _server: Arc<tiny_http::Server>,
}

impl MediaServer {
    fn start() -> Result<Self, String> {
        // Bind an ephemeral port on all interfaces so the TV can reach us.
        let server = tiny_http::Server::http("0.0.0.0:0")
            .map_err(|e| format!("media server bind failed: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|s| s.port())
            .ok_or_else(|| "media server has no IP address".to_string())?;

        // The TV needs our LAN IP, not 0.0.0.0 / 127.0.0.1. The classic trick:
        // "connect" a UDP socket toward a public address (no packet is sent)
        // and read back which local interface the OS routing table chose. Pure
        // std, works cross-platform, needs no network round-trip.
        let ip = local_lan_ip()
            .ok_or_else(|| "could not determine this machine's LAN IP".to_string())?;
        let base_url = format!("http://{ip}:{port}");

        let allow: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        let server = Arc::new(server);

        // A small worker pool: casting is low-QPS but the TV opens a couple of
        // parallel connections (e.g. probe + playback), and a Range/streaming
        // response occupies its worker for the whole transfer.
        for _ in 0..4 {
            let srv = server.clone();
            let allow = allow.clone();
            thread::spawn(move || serve_loop(srv, allow));
        }

        Ok(MediaServer { base_url, allow, _server: server })
    }

    /// Register `path` for casting and return the URL the TV should load. A new
    /// unguessable token is minted each time so URLs aren't predictable.
    fn register(&self, path: &Path) -> String {
        let token = gen_token();
        self.allow.lock().insert(token.clone(), path.to_path_buf());
        format!("{}/media/{}", self.base_url, token)
    }
}

/// Discover the LAN IP the OS would use to reach the internet (and therefore
/// the TV on the same subnet). No packet is actually transmitted.
fn local_lan_ip() -> Option<std::net::Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    match sock.local_addr().ok()? {
        SocketAddr::V4(a) => Some(*a.ip()),
        SocketAddr::V6(_) => None,
    }
}

/// Mint a 32-hex-char token from time + a monotonic counter, mixed through
/// splitmix64 so consecutive tokens don't share obvious bits. Not a CSPRNG —
/// it doesn't need to be: the allowlist is the real security boundary (an
/// attacker who can't guess the token can't reach any file, and even a guessed
/// token only maps to files the user chose to cast). It just needs to be
/// non-trivially guessable on a trusted LAN.
fn gen_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ c.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    format!("{:016x}{:016x}", x, nanos.rotate_left(17) ^ c)
}

/// One worker thread: pull requests off the shared server and serve them.
fn serve_loop(server: Arc<tiny_http::Server>, allow: Arc<Mutex<HashMap<String, PathBuf>>>) {
    loop {
        match server.recv() {
            Ok(req) => handle_request(req, &allow),
            Err(_) => break, // server dropped -> exit the worker
        }
    }
}

fn header(key: &str, value: &str) -> tiny_http::Header {
    // Both inputs are ASCII (fixed keys / mime strings / numeric ranges), so
    // this never fails in practice; a failed parse just yields no header.
    tiny_http::Header::from_bytes(key.as_bytes(), value.as_bytes())
        .unwrap_or_else(|_| tiny_http::Header::from_bytes(&b"X-Skip"[..], &b"1"[..]).unwrap())
}

fn handle_request(request: tiny_http::Request, allow: &Arc<Mutex<HashMap<String, PathBuf>>>) {
    use tiny_http::{Method, Response, StatusCode};

    // CORS preflight: the Default Media Receiver's page issues cross-origin
    // fetches, so answer OPTIONS with permissive headers and no body.
    if *request.method() == Method::Options {
        let resp = Response::empty(StatusCode(204))
            .with_header(header("Access-Control-Allow-Origin", "*"))
            .with_header(header("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"))
            .with_header(header("Access-Control-Allow-Headers", "*"));
        let _ = request.respond(resp);
        return;
    }

    // Only GET/HEAD reach media. (HEAD is treated like GET here — the Default
    // Media Receiver fetches with GET+Range, so the rare HEAD sending a body is
    // harmless on the LAN and keeps the code single-path.)
    let is_read = matches!(*request.method(), Method::Get | Method::Head);
    if !is_read {
        let _ = request.respond(Response::empty(StatusCode(405)));
        return;
    }

    // Route: strictly `/media/<token>`. Nothing else exists.
    let raw = request.url().to_string();
    let path_only = raw.split('?').next().unwrap_or("");
    let token = match path_only.strip_prefix("/media/") {
        Some(t) if !t.is_empty() && !t.contains('/') => t,
        _ => {
            let _ = request.respond(Response::empty(StatusCode(404)));
            return;
        }
    };

    let file_path = match allow.lock().get(token) {
        Some(p) => p.clone(),
        None => {
            let _ = request.respond(Response::empty(StatusCode(404)));
            return;
        }
    };

    // Pull the Range header (if any) before we consume `request`.
    let range_val = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_string());

    let mut file = match File::open(&file_path) {
        Ok(f) => f,
        Err(_) => {
            let _ = request.respond(Response::empty(StatusCode(404)));
            return;
        }
    };
    let total = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => {
            let _ = request.respond(Response::empty(StatusCode(500)));
            return;
        }
    };
    let mime = mime_for(&file_path).0;

    // Common headers on every media response.
    let base_headers = |status: StatusCode| {
        Response::empty(status)
            .with_header(header("Content-Type", &mime))
            .with_header(header("Accept-Ranges", "bytes"))
            .with_header(header("Access-Control-Allow-Origin", "*"))
            .with_header(header("Cache-Control", "no-store"))
    };

    if let Some(rv) = range_val {
        match parse_range(&rv, total) {
            Some((start, end)) => {
                let len = end - start + 1;
                if file.seek(SeekFrom::Start(start)).is_err() {
                    let _ = request.respond(Response::empty(StatusCode(500)));
                    return;
                }
                let reader = file.take(len);
                let resp = base_headers(StatusCode(206))
                    .with_header(header(
                        "Content-Range",
                        &format!("bytes {start}-{end}/{total}"),
                    ))
                    .with_data(reader, Some(len as usize));
                let _ = request.respond(resp);
            }
            None => {
                // Unsatisfiable range -> 416 with the full size advertised.
                let resp = Response::empty(StatusCode(416))
                    .with_header(header("Content-Range", &format!("bytes */{total}")))
                    .with_header(header("Access-Control-Allow-Origin", "*"));
                let _ = request.respond(resp);
            }
        }
        return;
    }

    // No Range: whole file.
    let resp = base_headers(StatusCode(200)).with_data(file, Some(total as usize));
    let _ = request.respond(resp);
}

/// Parse a single-range `Range: bytes=start-end` header against `total`.
/// Supports `start-`, `start-end`, and suffix `-N` (last N bytes). Returns the
/// inclusive `(start, end)` byte offsets, or `None` if unsatisfiable/multipart.
fn parse_range(raw: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = raw.trim().strip_prefix("bytes=")?;
    // We serve a single range only; ignore anything with a comma.
    if spec.contains(',') {
        return None;
    }
    let (s, e) = spec.split_once('-')?;
    if s.is_empty() {
        // Suffix range: last N bytes.
        let n: u64 = e.trim().parse().ok()?;
        if n == 0 {
            return None;
        }
        let start = total.saturating_sub(n);
        return Some((start, total - 1));
    }
    let start: u64 = s.trim().parse().ok()?;
    if start >= total {
        return None;
    }
    let end: u64 = if e.trim().is_empty() {
        total - 1
    } else {
        e.trim().parse::<u64>().ok()?.min(total - 1)
    };
    if start > end {
        return None;
    }
    Some((start, end))
}

/// Map a file extension to `(mime, is_video)`. `.mov` is reported as
/// `video/mp4` on purpose — the Default Media Receiver is happy with the H.264
/// elementary stream inside a QuickTime container under that type.
fn mime_for(path: &Path) -> (String, bool) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let (mime, is_video) = match ext.as_str() {
        "jpg" | "jpeg" | "jpe" => ("image/jpeg", false),
        "png" => ("image/png", false),
        "webp" => ("image/webp", false),
        "gif" => ("image/gif", false),
        "bmp" => ("image/bmp", false),
        "heic" | "heif" => ("image/heic", false),
        "mp4" | "m4v" => ("video/mp4", true),
        "mov" | "qt" => ("video/mp4", true), // H.264-in-MOV; DMR decodes it as mp4
        "webm" => ("video/webm", true),
        "mkv" => ("video/x-matroska", true),
        "m2ts" | "mts" | "ts" => ("video/mp2t", true),
        "avi" => ("video/x-msvideo", true),
        _ => ("application/octet-stream", false),
    };
    (mime.to_string(), is_video)
}

// ===========================================================================
// 3. CASTV2 protocol: the connection actor
// ===========================================================================

/// A `CastMessage` we send or receive. We only ever use STRING (JSON) payloads,
/// so this holds the four routing fields plus the JSON text.
struct CastMessage {
    namespace: String,
    source: String,
    dest: String,
    payload: String,
}

/// Encode a `CastMessage` to the CASTV2 wire bytes (the protobuf body, WITHOUT
/// the outer 4-byte length prefix). We always emit the two `required` enum
/// fields even though they're zero — see the module docs for why `prost` can't.
fn encode_cast_message(m: &CastMessage) -> Vec<u8> {
    fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let mut b = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                break;
            }
        }
    }
    fn put_key(out: &mut Vec<u8>, field: u64, wire: u64) {
        put_varint(out, (field << 3) | wire);
    }
    fn put_str(out: &mut Vec<u8>, field: u64, s: &str) {
        put_key(out, field, 2); // wire type 2 = length-delimited
        put_varint(out, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }

    let mut o = Vec::with_capacity(m.payload.len() + 64);
    // field 1: protocol_version (enum, varint) = CASTV2_1_0 (0) — required
    put_key(&mut o, 1, 0);
    put_varint(&mut o, 0);
    // fields 2/3/4: source_id / destination_id / namespace
    put_str(&mut o, 2, &m.source);
    put_str(&mut o, 3, &m.dest);
    put_str(&mut o, 4, &m.namespace);
    // field 5: payload_type (enum, varint) = STRING (0) — required
    put_key(&mut o, 5, 0);
    put_varint(&mut o, 0);
    // field 6: payload_utf8
    put_str(&mut o, 6, &m.payload);
    o
}

/// Decode the fields we care about (source, dest, namespace, payload_utf8) out
/// of a received `CastMessage` protobuf body. Unknown fields are skipped by
/// their wire type so we tolerate receiver additions.
fn decode_cast_message(buf: &[u8]) -> Option<CastMessage> {
    fn get_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let b = *buf.get(*pos)?;
            *pos += 1;
            result |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
    }

    let (mut source, mut dest, mut namespace, mut payload) =
        (String::new(), String::new(), String::new(), String::new());
    let mut pos = 0;
    while pos < buf.len() {
        let key = get_varint(buf, &mut pos)?;
        let field = key >> 3;
        let wire = key & 7;
        match wire {
            0 => {
                get_varint(buf, &mut pos)?; // varint field (1, 5) — skip
            }
            2 => {
                let len = get_varint(buf, &mut pos)? as usize;
                let end = pos.checked_add(len)?;
                let slice = buf.get(pos..end)?;
                pos = end;
                match field {
                    2 => source = String::from_utf8_lossy(slice).into_owned(),
                    3 => dest = String::from_utf8_lossy(slice).into_owned(),
                    4 => namespace = String::from_utf8_lossy(slice).into_owned(),
                    6 => payload = String::from_utf8_lossy(slice).into_owned(),
                    _ => {}
                }
            }
            1 => pos += 8, // 64-bit
            5 => pos += 4, // 32-bit
            _ => return None,
        }
    }
    Some(CastMessage { namespace, source, dest, payload })
}

/// Commands the Tauri thread hands to the connection actor.
enum Cmd {
    /// Load new media (already registered on the media server -> `url`).
    Load { url: String, content_type: String, is_video: bool, title: String, path: String },
    /// Mirror the laptop's transport onto the TV. Only meaningful once the
    /// receiver has reported a media session; before that they're dropped,
    /// because a LOAD is already on its way and arrives autoplaying.
    Play,
    Pause,
    Seek(f64),
    /// Stop the receiver app and tear the connection down; the actor exits.
    Shutdown,
}

/// A live connection to one device, owned by a background actor thread.
struct CastConn {
    /// `ip:port` — how we tell "same device, reuse" from "switch devices."
    device_addr: String,
    cmd_tx: Sender<Cmd>,
    status: Arc<Mutex<CastStatus>>,
    alive: Arc<AtomicBool>,
    _join: JoinHandle<()>,
}

impl CastConn {
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// TLS-connect to the device and spawn the actor thread. The TLS handshake
    /// happens here (synchronously) so an unreachable device surfaces as an
    /// error from `cast_start` rather than a silent background failure.
    fn connect(addr: &str, port: u16, name: &str) -> Result<Self, String> {
        let sockaddr: SocketAddr = format!("{addr}:{port}")
            .parse()
            .map_err(|_| format!("invalid device address {addr}:{port}"))?;

        let tcp = TcpStream::connect_timeout(&sockaddr, Duration::from_secs(5))
            .map_err(|e| format!("could not reach the TV ({sockaddr}): {e}"))?;
        tcp.set_nodelay(true).ok();

        // Chromecast presents a self-signed cert on 8009; we must not verify it
        // (there is no CA and the hostname is meaningless). native-tls uses the
        // OS TLS stack — SChannel on Windows, Security.framework on macOS — so
        // this needs no C toolchain to build.
        let connector = TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .map_err(|e| format!("tls setup failed: {e}"))?;
        let tls = connector
            .connect(addr, tcp)
            .map_err(|e| format!("tls handshake with the TV failed: {e}"))?;

        // After the handshake, give reads a short timeout so the actor's single
        // loop can interleave socket reads with heartbeat sends and command
        // handling without a separate reader thread (a TLS stream can't be
        // safely read and written from two threads at once).
        tls.get_ref()
            .set_read_timeout(Some(Duration::from_millis(300)))
            .ok();

        let status = Arc::new(Mutex::new(CastStatus {
            connected: false,
            device_name: Some(name.to_string()),
            playing_path: None,
        }));
        let alive = Arc::new(AtomicBool::new(true));

        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let st = status.clone();
        let al = alive.clone();
        let name_owned = name.to_string();
        let join = thread::spawn(move || {
            run_actor(tls, cmd_rx, st, name_owned);
            al.store(false, Ordering::Relaxed);
        });

        Ok(CastConn {
            device_addr: sockaddr.to_string(),
            cmd_tx,
            status,
            alive,
            _join: join,
        })
    }
}

/// A framing reader that tolerates the read timeout: it accumulates bytes and
/// yields whole CASTV2 frames as they complete, keeping any partial tail for
/// the next read. Lets one thread poll the socket without blocking forever.
struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    fn new() -> Self {
        FrameReader { buf: Vec::with_capacity(8192) }
    }

    /// Read once from the stream. `Ok(true)` = got bytes / nothing (keep going);
    /// `Ok(false)` = idle timeout (no data this cycle); `Err` = connection dead.
    fn fill(&mut self, stream: &mut TlsStream<TcpStream>) -> std::io::Result<bool> {
        let mut tmp = [0u8; 8192];
        match stream.read(&mut tmp) {
            Ok(0) => Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "peer closed",
            )),
            Ok(n) => {
                self.buf.extend_from_slice(&tmp[..n]);
                Ok(true)
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(false) // just an idle read timeout — not an error
            }
            Err(e) => Err(e),
        }
    }

    /// Pop the next complete frame from the buffer, if one has fully arrived.
    fn next_frame(&mut self) -> Option<CastMessage> {
        if self.buf.len() < 4 {
            return None;
        }
        let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if self.buf.len() < 4 + len {
            return None;
        }
        let msg = decode_cast_message(&self.buf[4..4 + len]);
        self.buf.drain(0..4 + len);
        // If a frame failed to decode we still consumed it; recurse to try the
        // next one so a single bad frame can't wedge the stream.
        msg.or_else(|| self.next_frame())
    }
}

/// Write one frame (4-byte big-endian length prefix + protobuf body).
fn send_frame(
    stream: &mut TlsStream<TcpStream>,
    namespace: &str,
    dest: &str,
    payload: Value,
) -> std::io::Result<()> {
    let msg = CastMessage {
        namespace: namespace.to_string(),
        source: SENDER_ID.to_string(),
        dest: dest.to_string(),
        payload: payload.to_string(),
    };
    let body = encode_cast_message(&msg);
    let len = (body.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()
}

/// The connection actor. Owns the TLS stream for its whole life, so all reads
/// and writes are single-threaded and safe. It:
///   * opens the platform virtual connection and LAUNCHes the media receiver,
///   * heartbeats (PING every few seconds, PONG on demand) so the TV keeps us,
///   * once the receiver reports its transport id, opens a virtual connection
///     to it and LOADs whatever media is pending,
///   * services Load/Shutdown commands from the UI.
/// What the actor should do about queued media on this tick.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum LoadAction {
    /// Nothing queued, or we're waiting out the relaunch backoff.
    Idle,
    /// Media is waiting but the receiver app isn't running — (re)launch it.
    Relaunch,
    /// Media is waiting and we have a transport to send it to.
    Send,
}

/// Pulled out of `run_actor` purely so it can be tested: this three-way choice
/// is the whole of the "cast stopped following me" bug. The original code had
/// no `Relaunch` case at all — LAUNCH was sent once at connect — so as soon as
/// the receiver app closed itself (which it does whenever it goes idle), every
/// later LOAD waited forever for a transport id that was never coming back.
fn load_action(has_pending: bool, has_transport: bool, since_launch: Duration) -> LoadAction {
    if !has_pending {
        return LoadAction::Idle;
    }
    if has_transport {
        return LoadAction::Send;
    }
    // Backoff so a TV that is off/unreachable gets one attempt every few
    // seconds rather than one per loop iteration.
    if since_launch >= RELAUNCH_EVERY {
        LoadAction::Relaunch
    } else {
        LoadAction::Idle
    }
}

/// How often we're willing to re-ask the TV to launch the receiver app.
const RELAUNCH_EVERY: Duration = Duration::from_secs(3);

fn run_actor(
    mut stream: TlsStream<TcpStream>,
    cmd_rx: Receiver<Cmd>,
    status: Arc<Mutex<CastStatus>>,
    device_name: String,
) {
    let mut req_id: i64 = 1;
    let mut next_id = || {
        req_id += 1;
        req_id
    };

    // Open the platform virtual connection and ask for the media receiver.
    if send_frame(&mut stream, NS_CONNECTION, PLATFORM_RECEIVER_ID, json!({"type":"CONNECT"}))
        .is_err()
    {
        return;
    }
    let _ = send_frame(
        &mut stream,
        NS_RECEIVER,
        PLATFORM_RECEIVER_ID,
        json!({"type":"LAUNCH","appId":DEFAULT_MEDIA_RECEIVER,"requestId":next_id()}),
    );
    status.lock().connected = true;

    let mut reader = FrameReader::new();
    let mut last_ping = Instant::now();

    // The receiver app's transport id + session id, learned from RECEIVER_STATUS.
    let mut transport_id: Option<String> = None;
    let mut session_id: Option<String> = None;
    // The MEDIA session id (distinct from the receiver session id above): the
    // handle every PLAY/PAUSE/SEEK must carry. Learned from MEDIA_STATUS, and
    // reset on each LOAD because the receiver mints a new one per item.
    let mut media_session_id: Option<i64> = None;
    // Whether we've opened the virtual connection to the app transport yet.
    let mut app_connected = false;
    // Media queued to LOAD as soon as the transport id is known.
    let mut pending_load: Option<(String, String, bool, String, String)> = None;
    // When we last asked the TV to launch the receiver app. The app is NOT
    // permanent: it closes itself when it goes idle (a clip ends, a still has
    // been up a while, someone presses Home). Before this was tracked, LAUNCH
    // was sent exactly once at connect, so once the app went away every later
    // LOAD had no transport to go to and sat in `pending_load` forever — the
    // connection was still alive and the UI still said "casting", but nothing
    // could ever reach the screen again.
    let mut last_launch = Instant::now();

    loop {
        // 1. Drain UI commands.
        loop {
            match cmd_rx.try_recv() {
                Ok(Cmd::Load { url, content_type, is_video, title, path }) => {
                    // NB: `playing_path` is deliberately NOT set here. It means
                    // "what the TV is showing", and at this point we have only
                    // queued the intent — see where the LOAD is actually sent.
                    pending_load = Some((url, content_type, is_video, title, path));
                }
                // Transport mirroring. These need BOTH the app transport and a
                // media session id (the receiver hands the latter out in its
                // first MEDIA_STATUS after a LOAD). Dropping them when either
                // is missing is correct rather than lossy: the only window
                // where that happens is between LOAD and first status, and the
                // media arrives playing from 0 anyway.
                Ok(cmd @ (Cmd::Play | Cmd::Pause | Cmd::Seek(_))) => {
                    let (Some(tid), Some(msid)) = (transport_id.clone(), media_session_id) else {
                        crate::log::line("cast: transport command dropped (no media session yet)");
                        continue;
                    };
                    let payload = match cmd {
                        Cmd::Play => json!({"type":"PLAY","mediaSessionId":msid,"requestId":next_id()}),
                        Cmd::Pause => json!({"type":"PAUSE","mediaSessionId":msid,"requestId":next_id()}),
                        Cmd::Seek(t) => json!({
                            "type":"SEEK","mediaSessionId":msid,"currentTime":t,"requestId":next_id()
                        }),
                        _ => unreachable!("outer match limits this to the three transport commands"),
                    };
                    crate::log::line(&format!("cast: -> {}", payload["type"]));
                    if send_frame(&mut stream, NS_MEDIA, &tid, payload).is_err() {
                        // Not a `break` — that would only leave this drain loop.
                        // The outer read is on a 300 ms timeout and folds the
                        // connection up properly within one tick.
                        crate::log::line("cast: transport send failed; connection is gone");
                    }
                }
                Ok(Cmd::Shutdown) => {
                    crate::log::line("cast: shutdown requested");
                    // Best-effort: stop the launched app, then let the stream
                    // drop (which closes the TLS + TCP connection).
                    if let Some(sid) = &session_id {
                        let _ = send_frame(
                            &mut stream,
                            NS_RECEIVER,
                            PLATFORM_RECEIVER_ID,
                            json!({"type":"STOP","sessionId":sid,"requestId":next_id()}),
                        );
                    }
                    let mut s = status.lock();
                    s.connected = false;
                    s.playing_path = None;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }

        // 2. Get the queued media onto the screen.
        match load_action(pending_load.is_some(), transport_id.is_some(), last_launch.elapsed()) {
            LoadAction::Idle => {}
            // The receiver app isn't running (it never was, or it idled out and
            // closed). Relaunch it and ask for a status so we learn the new
            // transport id; `pending_load` is KEPT, so the media the user asked
            // for goes out the moment the app is back.
            LoadAction::Relaunch => {
                crate::log::line("cast: receiver app not running — relaunching");
                let _ = send_frame(
                    &mut stream,
                    NS_RECEIVER,
                    PLATFORM_RECEIVER_ID,
                    json!({"type":"LAUNCH","appId":DEFAULT_MEDIA_RECEIVER,"requestId":next_id()}),
                );
                let _ = send_frame(
                    &mut stream,
                    NS_RECEIVER,
                    PLATFORM_RECEIVER_ID,
                    json!({"type":"GET_STATUS","requestId":next_id()}),
                );
                last_launch = Instant::now();
            }
            LoadAction::Send => {
                let tid = transport_id.clone().expect("Send implies a transport id");
                let (url, content_type, is_video, title, path) =
                    pending_load.take().expect("Send implies queued media");
                if !app_connected {
                    let _ = send_frame(&mut stream, NS_CONNECTION, &tid, json!({"type":"CONNECT"}));
                    app_connected = true;
                }
                // BUFFERED for video (seekable stream); NONE for a still image.
                let stream_type = if is_video { "BUFFERED" } else { "NONE" };
                let load = json!({
                    "type": "LOAD",
                    "requestId": next_id(),
                    "autoplay": true,
                    "currentTime": 0,
                    "media": {
                        "contentId": url,
                        "contentUrl": url,
                        "streamType": stream_type,
                        "contentType": content_type,
                        "metadata": { "type": 0, "title": title }
                    }
                });
                crate::log::line(&format!(
                    "cast: LOAD {} ({content_type}, {})",
                    Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.clone()),
                    if is_video { "video" } else { "still" },
                ));
                if send_frame(&mut stream, NS_MEDIA, &tid, load).is_err() {
                    crate::log::line("cast: LOAD send failed; connection lost");
                    break;
                }
                // The receiver mints a fresh media session per LOAD; the old id
                // would be rejected, so transport commands wait for the new one.
                media_session_id = None;
                // NOW it is true. Claiming it when the command was queued meant
                // the UI reported a file the TV had never been told about, and
                // the "cast follows the active item" guard then saw its job as
                // done — so the TV kept showing the previous clip while the app
                // had moved on.
                status.lock().playing_path = Some(path);
            }
        }

        // 3. Heartbeat so the TV doesn't drop us (~8s idle timeout).
        if last_ping.elapsed() >= Duration::from_secs(4) {
            if send_frame(&mut stream, NS_HEARTBEAT, PLATFORM_RECEIVER_ID, json!({"type":"PING"}))
                .is_err()
            {
                break;
            }
            last_ping = Instant::now();
        }

        // 4. Read + dispatch whatever arrived.
        match reader.fill(&mut stream) {
            Ok(_) => {}
            Err(_) => break, // connection lost
        }
        while let Some(msg) = reader.next_frame() {
            let payload: Value = match serde_json::from_str(&msg.payload) {
                Ok(v) => v,
                Err(_) => continue,
            };
            match payload.get("type").and_then(|t| t.as_str()) {
                Some("PING") => {
                    // Reply on the heartbeat namespace to the pinger.
                    let _ = send_frame(&mut stream, NS_HEARTBEAT, &msg.source, json!({"type":"PONG"}));
                }
                Some("RECEIVER_STATUS") => {
                    if let Some(apps) = payload
                        .get("status")
                        .and_then(|s| s.get("applications"))
                        .and_then(|a| a.as_array())
                    {
                        // Find our media receiver and capture its transport id.
                        let app = apps.iter().find(|a| {
                            a.get("appId").and_then(|v| v.as_str()) == Some(DEFAULT_MEDIA_RECEIVER)
                        });
                        if let Some(app) = app {
                            if let Some(tid) = app.get("transportId").and_then(|v| v.as_str()) {
                                if transport_id.as_deref() != Some(tid) {
                                    transport_id = Some(tid.to_string());
                                    app_connected = false; // new transport -> reconnect virtually
                                }
                            }
                            if let Some(sid) = app.get("sessionId").and_then(|v| v.as_str()) {
                                session_id = Some(sid.to_string());
                            }
                        } else {
                            // App went away (user closed it on the TV).
                            if transport_id.is_some() {
                                crate::log::line("cast: receiver app closed on the TV");
                            }
                            transport_id = None;
                            session_id = None;
                            media_session_id = None;
                            app_connected = false;
                            status.lock().playing_path = None;
                        }
                    }
                }
                Some("MEDIA_STATUS") => {
                    // Media accepted / playing — keep connected flag true.
                    status.lock().connected = true;
                    // Capture the media session id: PLAY/PAUSE/SEEK are
                    // addressed to it, not to the transport alone.
                    if let Some(msid) = payload
                        .get("status")
                        .and_then(|s| s.as_array())
                        .and_then(|a| a.first())
                        .and_then(|s| s.get("mediaSessionId"))
                        .and_then(|v| v.as_i64())
                    {
                        if media_session_id != Some(msid) {
                            crate::log::line(&format!("cast: media session {msid}"));
                            media_session_id = Some(msid);
                        }
                    }
                }
                Some("CLOSE") => {
                    // The platform closed our virtual connection.
                    status.lock().connected = false;
                    return;
                }
                _ => {}
            }
        }
    }

    // Fell out of the loop on a connection error.
    let mut s = status.lock();
    s.connected = false;
    s.playing_path = None;
    let _ = device_name; // retained for potential future logging
}

// ===========================================================================
// Tauri commands
// ===========================================================================

/// Cast `path` to the given device. Registers the file with the local media
/// server, ensures a live connection (reusing it when the device is unchanged),
/// and issues a LOAD. Returns the resulting status for the UI.
#[tauri::command]
pub fn cast_start(
    state: State<'_, CastState>,
    path: String,
    device_addr: String,
    device_port: u16,
    device_name: String,
) -> Result<CastStatus, String> {
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err(format!("file not found: {path}"));
    }
    let (content_type, is_video) = mime_for(&file);
    let title = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("FoxCull")
        .to_string();

    // Ensure the media server is up, then register this file and get its URL.
    let url = {
        let mut guard = state.server.lock();
        if guard.is_none() {
            *guard = Some(MediaServer::start()?);
        }
        guard.as_ref().unwrap().register(&file)
    };

    // Ensure a connection to this exact device, reusing the existing one if it's
    // the same device and still alive.
    let target = format!("{device_addr}:{device_port}");
    {
        let mut guard = state.conn.lock();
        let reuse = guard
            .as_ref()
            .map(|c| c.device_addr == target && c.is_alive())
            .unwrap_or(false);
        if !reuse {
            // Tear down any connection to a different (or dead) device.
            if let Some(old) = guard.take() {
                crate::log::line("cast: dropping the previous connection");
                let _ = old.cmd_tx.send(Cmd::Shutdown);
            }
            crate::log::line(&format!("cast: connecting to {device_name} at {target}"));
            let conn = CastConn::connect(&device_addr, device_port, &device_name)
                .inspect_err(|e| crate::log::line(&format!("cast: connect failed — {e}")))?;
            *guard = Some(conn);
        }

        let conn = guard.as_ref().unwrap();
        conn.cmd_tx
            .send(Cmd::Load { url, content_type, is_video, title, path: path.clone() })
            .map_err(|_| "cast connection closed".to_string())?;
        let mut st = conn.status.lock();
        st.playing_path = Some(path);
        let snapshot = st.clone();
        drop(st);
        Ok(snapshot)
    }
}

/// Mirror the laptop's transport onto the TV. All three are best-effort and
/// never error: with no live session there is simply nothing to mirror, and the
/// caller (a keypress, a controller trigger) must not have to care.
fn send_transport(state: &State<'_, CastState>, cmd: Cmd) {
    let guard = state.conn.lock();
    if let Some(conn) = guard.as_ref() {
        if conn.is_alive() {
            let _ = conn.cmd_tx.send(cmd);
        }
    }
}

#[tauri::command]
pub fn cast_play(state: State<'_, CastState>) {
    send_transport(&state, Cmd::Play);
}

#[tauri::command]
pub fn cast_pause(state: State<'_, CastState>) {
    send_transport(&state, Cmd::Pause);
}

/// Seek the TV to `position` seconds. The frontend throttles these — a held
/// shuttle trigger fires ~8x/second locally, which the receiver would not
/// keep up with.
#[tauri::command]
pub fn cast_seek(state: State<'_, CastState>, position: f64) {
    send_transport(&state, Cmd::Seek(position.max(0.0)));
}

/// Stop casting: tell the receiver to stop and close the connection.
#[tauri::command]
pub fn cast_stop(state: State<'_, CastState>) -> CastStatus {
    let mut guard = state.conn.lock();
    if let Some(conn) = guard.take() {
        let _ = conn.cmd_tx.send(Cmd::Shutdown);
    }
    CastStatus::disconnected()
}

/// Current cast status for the UI (connected? which device? what's playing?).
#[tauri::command]
pub fn cast_status(state: State<'_, CastState>) -> CastStatus {
    let guard = state.conn.lock();
    match guard.as_ref() {
        Some(conn) if conn.is_alive() => conn.status.lock().clone(),
        _ => CastStatus::disconnected(),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // The regression this whole module was rewritten for. Reported symptom:
    // "I move to the next video and it just disappears from the TV", and its
    // twin "the previous video kept playing on the TV while the next one played
    // on the laptop". Both are one cause — the Default Media Receiver closes
    // itself when it goes idle, and nothing ever launched it again.
    #[test]
    fn queued_media_relaunches_the_receiver_when_the_app_has_gone() {
        // App gone (no transport id) with media waiting: we must relaunch, not
        // sit on it. Pre-fix this state produced no action for the rest of the
        // session, which is exactly how the TV got stranded.
        assert_eq!(
            load_action(true, false, RELAUNCH_EVERY),
            LoadAction::Relaunch
        );
    }

    #[test]
    fn relaunch_is_rate_limited_so_a_dead_tv_is_not_hammered() {
        assert_eq!(
            load_action(true, false, Duration::from_millis(0)),
            LoadAction::Idle
        );
        assert_eq!(
            load_action(true, false, RELAUNCH_EVERY - Duration::from_millis(1)),
            LoadAction::Idle
        );
    }

    #[test]
    fn media_goes_out_as_soon_as_there_is_a_transport() {
        assert_eq!(load_action(true, true, Duration::from_secs(0)), LoadAction::Send);
        // Even mid-backoff: having a transport beats waiting.
        assert_eq!(load_action(true, true, Duration::from_millis(1)), LoadAction::Send);
    }

    #[test]
    fn an_idle_session_never_launches_anything() {
        // Nothing queued: the user closing the receiver on the TV must not be
        // fought by us relaunching it under them.
        assert_eq!(load_action(false, false, Duration::from_secs(3600)), LoadAction::Idle);
        assert_eq!(load_action(false, true, Duration::from_secs(3600)), LoadAction::Idle);
    }
}
