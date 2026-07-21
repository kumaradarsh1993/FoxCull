//! Video poster frames (and, in Stage 2, lossless trim) via a **bundled ffmpeg**.
//!
//! ffmpeg ships as a Tauri `externalBin`, so at runtime it sits next to our own
//! executable. We invoke it directly with `std::process::Command` (no shell
//! plugin, no extra capability) and cache a single decoded frame as a JPEG in the
//! same on-disk thumbnail cache as images — which, by following the catalog onto
//! the user's SSD, means a clip's poster is generated once and reused on every
//! machine that reads that SSD.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::UNIX_EPOCH;

/// Resolve the bundled ffmpeg sitting beside our executable (Tauri strips the
/// target-triple suffix from the externalBin at bundle time). `None` if absent
/// (e.g. a dev run without the binary) — callers then fall back to a placeholder.
pub fn ffmpeg_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let p = dir.join(name);
    p.exists().then_some(p)
}

fn meta(src: &Path) -> (i64, u64) {
    match std::fs::metadata(src) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (mtime, m.len())
        }
        Err(_) => (0, 0),
    }
}

/// Cache path for a clip's poster, keyed by (path, mtime, size) and prefixed by
/// `prefix` so it never collides with image thumbnails or the hi-res variant.
fn poster_path_pfx(cache_dir: &Path, src: &Path, prefix: char) -> PathBuf {
    let (mtime, size) = meta(src);
    let abs = src.to_string_lossy();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    abs.hash(&mut h);
    mtime.hash(&mut h);
    size.hash(&mut h);
    cache_dir.join(format!("{prefix}{:016x}.jpg", h.finish()))
}

/// Light 480px poster (prefix `v`) — used by the grid, where tiles are tiny.
pub fn poster_path(cache_dir: &Path, src: &Path) -> PathBuf {
    poster_path_pfx(cache_dir, src, 'v')
}

/// Sharp ~1280px poster (prefix `w`) — used by Focus/full-screen view, where a
/// 480px frame scaled up to a 4K stage looked pixelated. Kept a separate cache
/// entry so opening one clip in Focus never bloats the grid's poster memory.
pub fn poster_hires_path(cache_dir: &Path, src: &Path) -> PathBuf {
    poster_path_pfx(cache_dir, src, 'w')
}

/// Extract one representative frame (~1s in) scaled to fit a `box_px` box and
/// write it to `out`. Idempotent. Works for any codec the bundled ffmpeg
/// supports — crucially including HEVC (the Osmo Pocket 3 footage the webview
/// can't decode). `box_px` lets the grid use a light 480px poster while Focus
/// view asks for a sharp ~1280px one (see `ensure_poster_hires`).
pub fn make_poster(ffmpeg: &Path, src: &Path, out: &Path, box_px: u32) -> Result<(), String> {
    if out.exists() {
        return Ok(());
    }
    // q:v 3 keeps the still crisp — the previous default quantizer made the
    // Focus/full-screen first frame look pixelated even at full resolution.
    let scale = format!("scale=w={box_px}:h={box_px}:force_original_aspect_ratio=decrease");
    // `-skip_frame nokey` means the decoder only touches the keyframe at/before
    // the seek point instead of decoding forward frame-by-frame to exactly 1.0s —
    // on 4K60 HEVC that's one frame decoded instead of ~60, and visually the
    // poster is the same shot. The retry below stays exact for odd containers.
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-ss", "1", "-skip_frame", "nokey", "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-vf", &scale, "-q:v", "3", "-y"])
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Don't flash a console window on Windows for each clip.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() && out.exists() {
        Ok(())
    } else {
        // Some clips are shorter than the seek point; retry from the very start.
        let mut cmd2 = Command::new(ffmpeg);
        cmd2.args(["-v", "error", "-i"])
            .arg(src)
            .args(["-frames:v", "1", "-vf", &scale, "-q:v", "3", "-y"])
            .arg(out)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd2.creation_flags(CREATE_NO_WINDOW);
        }
        let s2 = cmd2.status().map_err(|e| e.to_string())?;
        if s2.success() && out.exists() {
            Ok(())
        } else {
            Err("ffmpeg could not extract a poster frame".into())
        }
    }
}

/// Lossless trim: copy the stream between `in_s` and `out_s` (seconds) to `dest`
/// with NO re-encode (`-c copy`) — instant even on huge files, exactly like
/// LosslessCut. `-ss` before `-i` does a fast keyframe seek; `-t` gives the
/// duration so the cut length is unambiguous. Returns the output path.
pub fn trim(
    ffmpeg: &Path,
    src: &Path,
    in_s: f64,
    out_s: f64,
    dest: &Path,
) -> Result<(), String> {
    if out_s <= in_s {
        return Err("out point must be after in point".into());
    }
    let dur = out_s - in_s;
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-ss", &format!("{in_s:.3}"), "-i"])
        .arg(src)
        .args([
            "-t",
            &format!("{dur:.3}"),
            "-c",
            "copy",
            "-map",
            "0",
            "-avoid_negative_ts",
            "make_zero",
            "-y",
        ])
        .arg(dest)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() && dest.exists() {
        Ok(())
    } else {
        Err("ffmpeg trim failed".into())
    }
}

/// Decode a still image file (e.g. HEIC, which neither the webview nor the
/// `image` crate can read) into a JPEG at `out`, scaled to fit a `max` box.
/// ffmpeg's HEIF demuxer applies the container's rotation (irot/imir), so the
/// output is upright without us reading EXIF.
///
/// The scale MUST go through `-filter_complex`, not `-vf`. Phone HEICs (Samsung,
/// iPhone) are commonly stored as a *grid* of HEVC tiles, and ffmpeg decodes
/// those by auto-building a complex filtergraph to stitch the tiles into one
/// image. A simple `-vf` on top of that fails outright — "Simple and complex
/// filtering cannot be used together for the same stream" — which is exactly why
/// tiled HEICs showed a placeholder in the grid and "can't preview this file" in
/// the loupe while plain JPEGs worked. `-filter_complex` composes with the
/// tile-stitch graph and is equally fine for single-tile HEIFs.
pub fn decode_still(ffmpeg: &Path, src: &Path, out: &Path, max: u32) -> Result<(), String> {
    let fc = format!("scale=w={max}:h={max}:force_original_aspect_ratio=decrease");
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-filter_complex", &fc, "-q:v", "3", "-y"])
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // Capture stderr rather than discarding it: a silent `Stdio::null()` here is
    // what hid the tiled-HEIC filtergraph failure for so long. On failure the
    // ffmpeg diagnostic goes into the error string (and thus the log).
    let output = cmd.output().map_err(|e| e.to_string())?;
    if output.status.success() && out.exists() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        let tail = err.trim();
        Err(if tail.is_empty() {
            "ffmpeg could not decode this image".into()
        } else {
            format!("ffmpeg could not decode this image: {tail}")
        })
    }
}

// ── H.264 proxy playback (HEVC clips on machines without the OS codec) ───────
//
// The webview plays video through the OS media stack (Media Foundation on
// Windows), so we can't bundle a codec INTO the player — but the bundled ffmpeg
// decodes HEVC fine. When a clip genuinely fails to play, we transcode a capped
// H.264 preview once, cache it beside the thumbnails on the drive, and play
// that. One transcode at a time (a second concurrent one would just thrash the
// disk and halve both).

/// Cache path for a clip's H.264 proxy, keyed like the poster but prefixed `p`.
pub fn proxy_path(cache_dir: &Path, src: &Path) -> PathBuf {
    let (mtime, size) = meta(src);
    let abs = src.to_string_lossy();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    abs.hash(&mut h);
    mtime.hash(&mut h);
    size.hash(&mut h);
    cache_dir.join(format!("p{:016x}.mp4", h.finish()))
}

static PROXY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Transcode `src` to a capped (≤1920 long edge) H.264/AAC preview at `out`,
/// reporting progress (0.0..=1.0) via `on_progress`. Writes to a `.part` file
/// and renames on success so a crash never leaves a half-written proxy that
/// would later "play" as truncated. Serialized process-wide.
pub fn ensure_proxy(
    cache_dir: &Path,
    ffmpeg: Option<&Path>,
    src: &Path,
    mut on_progress: impl FnMut(f64),
) -> Result<PathBuf, String> {
    let out = proxy_path(cache_dir, src);
    if out.exists() {
        return Ok(out);
    }
    let ff = ffmpeg.ok_or("ffmpeg not available")?;
    let _guard = PROXY_LOCK.lock().map_err(|_| "proxy lock poisoned")?;
    if out.exists() {
        return Ok(out); // built while we waited for the lock
    }
    let dur = probe_duration(ff, src).unwrap_or(0.0);
    let part = out.with_extension("part.mp4");
    let _ = std::fs::remove_file(&part);

    // `-hwaccel auto` offloads the DECODE side (NVDEC on the GTX 1070, D3D11VA /
    // VideoToolbox elsewhere) and silently falls back to software when no
    // accelerator fits — the HEVC→H.264 proxy build was fully CPU-bound before.
    let mut cmd = Command::new(ff);
    cmd.args(["-v", "error", "-hwaccel", "auto", "-i"])
        .arg(src)
        .args([
            "-map", "0:v:0", "-map", "0:a:0?",
            "-vf",
            "scale=w=1920:h=1920:force_original_aspect_ratio=decrease:force_divisible_by=2",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "22",
            "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "128k",
            "-movflags", "+faststart",
            "-progress", "pipe:1", "-nostats",
            "-y",
        ])
        .arg(&part)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    if let Some(stdout) = child.stdout.take() {
        use std::io::BufRead;
        // ffmpeg's -progress stream is `key=value` lines; out_time_us tracks the
        // encoded position, which against the probed duration gives a fraction.
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if dur > 0.0 {
                if let Some(v) = line.strip_prefix("out_time_us=") {
                    if let Ok(us) = v.trim().parse::<f64>() {
                        on_progress((us / 1_000_000.0 / dur).clamp(0.0, 1.0));
                    }
                }
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() && part.exists() {
        std::fs::rename(&part, &out).map_err(|e| e.to_string())?;
        on_progress(1.0);
        Ok(out)
    } else {
        let _ = std::fs::remove_file(&part);
        Err("ffmpeg could not convert this clip (the build may lack an H.264 encoder)".into())
    }
}

/// Ensure a poster exists for `src`; returns its cache path. `ffmpeg=None` (not
/// bundled / dev) yields an error so the UI shows the film placeholder.
pub fn ensure_poster(cache_dir: &Path, ffmpeg: Option<&Path>, src: &Path) -> Result<PathBuf, String> {
    let out = poster_path(cache_dir, src);
    if out.exists() {
        return Ok(out);
    }
    let ff = ffmpeg.ok_or("ffmpeg not available")?;
    make_poster(ff, src, &out, 480)?;
    Ok(out)
}

/// Ensure the sharp Focus-view poster exists; returns its cache path. Same
/// keyframe extraction as `ensure_poster` but at ~1280px so it holds up on a
/// full-screen stage.
pub fn ensure_poster_hires(
    cache_dir: &Path,
    ffmpeg: Option<&Path>,
    src: &Path,
) -> Result<PathBuf, String> {
    let out = poster_hires_path(cache_dir, src);
    if out.exists() {
        return Ok(out);
    }
    let ff = ffmpeg.ok_or("ffmpeg not available")?;
    make_poster(ff, src, &out, 1280)?;
    Ok(out)
}

// ── filmstrip scrub (Tier 2: hover-preview + seek without webview decode) ─────
//
// We pre-extract a fixed grid of frames spread across the clip into ONE sprite
// JPEG (the "filmstrip"), cached on the SSD beside the poster. The webview then
// shows the frame under the scrub cursor instantly by offsetting a CSS sprite —
// no per-frame decode in the player, so scrubbing is smooth even on HEVC the
// webview can't natively decode (the Osmo Pocket 3 footage). Generated lazily on
// first hover/loupe open, then reused on every machine that reads the SSD.
//
// HOW frames are pulled matters enormously (the 2026-07 rework): the original
// implementation ran ONE ffmpeg pass with an `fps=` filter, which DECODES EVERY
// FRAME of the clip to keep ~40 of them — on a 5-minute 4K60 HEVC Osmo clip
// that's ~18,000 frames of software HEVC decode to build one hover strip
// (minutes on the XPS 13; the "Live Scrub hangs forever" complaint). Now each
// sampled frame is its own keyframe-seek: `-ss T` before `-i` jumps straight
// through the container index, `-skip_frame nokey` makes the decoder emit just
// the ONE keyframe there. ~40 frames costs ~40 keyframe decodes total, so a
// strip builds in a couple of seconds regardless of clip length, and the build
// can be CANCELLED between frames (hover moved on / folder switched).

const FILMSTRIP_COLS: u32 = 10;
const SCRUBSTRIP_COLS: u32 = 8;
/// Pixel width each frame is scaled to inside the sprite (height follows aspect,
/// so portrait phone clips stay portrait). The filmstrip tile is sized for the
/// Focus drag overlay (a full-canvas frame while you scrub); the scrub tile only
/// ever paints inside a grid cell.
const FILMSTRIP_TILE_W: u32 = 240;
const SCRUBSTRIP_TILE_W: u32 = 160;
/// Concurrent per-frame extractions inside ONE sprite build.
///
/// Measured on the Alienware (12 cores, 4K60 HEVC Main10 clips on the internal
/// HDD, 2026-07-21) — 12 cold frames from a 1.8 GB clip:
///   parallel 2 → 6.16 s · parallel 4 → 4.40 s · parallel 6 → 3.61 s
///
/// The old value of 2 was chosen to keep a USB SSD's read queue shallow, but
/// the same benchmark shows this work is **not** I/O bound: re-extracting the
/// identical frames with the OS cache warm took 5.04 s against 5.92 s cold, so
/// only ~15% of the time is disk. The rest is ffmpeg startup plus re-parsing a
/// multi-gigabyte container index, paid once per frame — CPU work that
/// genuinely parallelises. Scaled by cores so the 4-core XPS 13 keeps the old
/// conservative 2 while the 12-core machine takes the win.
fn sprite_parallel() -> u32 {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4);
    (cores / 3).clamp(2, 4)
}

/// Error string a cancelled sprite build returns — callers match on it to stay
/// quiet (a cancelled hover is not a failure).
pub const SPRITE_CANCELLED: &str = "sprite build cancelled";

/// Builds are serialized process-wide: a second hover queues behind the current
/// build instead of racing it for the disk, and a cancelled build drains within
/// one frame-extraction (~100–300 ms) so the queue moves quickly.
static SPRITE_BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Geometry of a generated filmstrip, persisted in a tiny sidecar so we don't
/// re-probe the clip on every loupe open.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Filmstrip {
    pub cols: u32,
    pub rows: u32,
    /// Number of real frames in the sprite (<= cols*rows; trailing cells blank).
    pub count: u32,
    pub tile_w: u32,
    pub tile_h: u32,
    /// Clip duration in seconds — lets the frontend map cursor → time → frame.
    pub duration: f64,
}

/// Sprite-sheet cache path, prefixed `f` so it never collides with image
/// thumbnails (`<hash>`) or posters (`v<hash>`).
pub fn filmstrip_path(cache_dir: &Path, src: &Path) -> PathBuf {
    sprite_path(cache_dir, src, "f")
}

/// Lighter sprite-sheet cache path for grid thumbnail hover skimming.
pub fn scrubstrip_path(cache_dir: &Path, src: &Path) -> PathBuf {
    sprite_path(cache_dir, src, "s")
}

fn sprite_path(cache_dir: &Path, src: &Path, prefix: &str) -> PathBuf {
    let (mtime, size) = meta(src);
    let abs = src.to_string_lossy();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    abs.hash(&mut h);
    mtime.hash(&mut h);
    size.hash(&mut h);
    cache_dir.join(format!("{prefix}{:016x}.jpg", h.finish()))
}

/// Read the clip's duration (seconds) from ffmpeg's stderr banner. ffmpeg with
/// only `-i` (no output) exits non-zero but still prints `Duration: HH:MM:SS.xx`,
/// so we parse that — no separate ffprobe binary needed.
fn probe_duration(ffmpeg: &Path, src: &Path) -> Option<f64> {
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-i")
        .arg(src)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    let err = String::from_utf8_lossy(&out.stderr);
    let idx = err.find("Duration:")?;
    let token = err[idx + "Duration:".len()..]
        .trim_start()
        .split(',')
        .next()?
        .trim();
    if token.starts_with("N/A") {
        return None;
    }
    let mut parts = token.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let m: f64 = parts.next()?.trim().parse().ok()?;
    let s: f64 = parts.next()?.trim().parse().ok()?;
    let secs = h * 3600.0 + m * 60.0 + s;
    (secs > 0.0).then_some(secs)
}

/// Capture time (Unix secs) of a clip from its `creation_time` metadata, parsed
/// from ffmpeg's `-i` banner. ISO-8601 UTC ("2024-05-01T12:34:56.000000Z").
/// `None` when absent (e.g. re-encoded clips) so the caller falls back to mtime.
pub fn creation_time(ffmpeg: &Path, src: &Path) -> Option<i64> {
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-i")
        .arg(src)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    let err = String::from_utf8_lossy(&out.stderr);
    let idx = err.find("creation_time")?;
    // After the key: "creation_time   : 2024-05-01T12:34:56.000000Z"
    let after = &err[idx..];
    let colon = after.find(':')?;
    let val = after[colon + 1..].trim_start();
    let token = val.split_whitespace().next()?;
    parse_iso(token)
}

/// Parse "YYYY-MM-DDThh:mm:ss…" (ignoring sub-seconds / trailing Z) to Unix secs.
fn parse_iso(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> { s.get(a..z)?.parse().ok() };
    let y = num(0, 4)?;
    let mo = num(5, 7)?;
    let d = num(8, 10)?;
    let h = num(11, 13)?;
    let mi = num(14, 16)?;
    let se = num(17, 19)?;
    if y < 1970 || !(1..=12).contains(&mo) {
        return None;
    }
    Some(crate::media::civil_to_unix(y, mo, d, h, mi, se))
}

/// Fallback sprite render for containers whose index can't be seeked (rare —
/// broken AVIs, raw streams): ONE ffmpeg pass with an `fps=` filter, which
/// decodes every frame. `-hwaccel auto` + `-skip_frame nokey` keep even that
/// pass bearable: only keyframes are actually decoded, in hardware when the
/// machine has a decoder for the codec.
fn make_filmstrip_fullscan(
    ffmpeg: &Path,
    src: &Path,
    out: &Path,
    cols: u32,
    rows: u32,
    fps: f64,
    tile_w: u32,
) -> Result<(), String> {
    let vf = format!(
        "fps={fps:.6},scale={tile_w}:-2,tile={cols}x{rows}:padding=0:margin=0"
    );
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-hwaccel", "auto", "-skip_frame", "nokey"])
        .arg("-i")
        .arg(src)
        .args(["-vf", &vf, "-frames:v", "1", "-q:v", "5", "-an", "-y"])
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() && out.exists() {
        Ok(())
    } else {
        Err("ffmpeg could not build filmstrip".into())
    }
}

/// Extract the single frame nearest `at` seconds into `out`, scaled to `tile_w`
/// wide. `keyframe_only` = the fast path (decode exactly one keyframe);
/// without it the decoder walks from the previous keyframe to the exact time
/// (used as a retry for containers where the fast path yields nothing).
fn extract_frame_at(
    ffmpeg: &Path,
    src: &Path,
    at: f64,
    tile_w: u32,
    keyframe_only: bool,
    out: &Path,
) -> bool {
    let mut cmd = Command::new(ffmpeg);
    // `-hwaccel auto`: decode the keyframe on the GPU when a decoder exists
    // (NVDEC on the GTX 1070 does 4K HEVC ~5-10x faster than software; d3d11va
    // otherwise), fall back to software transparently. This was only on the
    // rare fullscan path before — on HDD libraries the software decode
    // dominated the whole strip build (~0.3-1.5s per 4K HEVC frame).
    cmd.args(["-v", "error", "-hwaccel", "auto", "-ss", &format!("{at:.3}")]);
    if keyframe_only {
        cmd.args(["-skip_frame", "nokey"]);
    }
    cmd.arg("-i")
        .arg(src)
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!("scale={tile_w}:-2"),
            "-q:v",
            "4",
            "-an",
            "-y",
        ])
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    matches!(cmd.status(), Ok(s) if s.success())
        && std::fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false)
}

/// Read the cached sprite geometry for `src` WITHOUT building anything.
/// `film` selects the dense Focus filmstrip; false = the lighter hover strip.
/// Lets the Focus view show a coarse-but-instant strip that the grid hover /
/// Prepare already built while the dense one renders.
pub fn sprite_cached(cache_dir: &Path, src: &Path, film: bool) -> Option<(PathBuf, Filmstrip)> {
    let sprite = if film {
        filmstrip_path(cache_dir, src)
    } else {
        scrubstrip_path(cache_dir, src)
    };
    let meta_path = sprite.with_extension("json");
    if !sprite.exists() {
        return None;
    }
    let txt = std::fs::read_to_string(&meta_path).ok()?;
    let fs = serde_json::from_str::<Filmstrip>(&txt).ok()?;
    Some((sprite, fs))
}

/// Ensure a filmstrip sprite + its geometry sidecar exist for `src`; returns the
/// sprite path and geometry. Cached (sprite `f<hash>.jpg` + `f<hash>.json`).
/// `cancel` is polled between frame extractions; `on_progress(done, total)`
/// fires as frames land.
pub fn ensure_filmstrip(
    cache_dir: &Path,
    ffmpeg: Option<&Path>,
    src: &Path,
    cancel: &(dyn Fn() -> bool + Sync),
    on_progress: &(dyn Fn(u32, u32) + Sync),
) -> Result<(PathBuf, Filmstrip), String> {
    ensure_sprite(
        ffmpeg,
        src,
        filmstrip_path(cache_dir, src),
        FILMSTRIP_COLS,
        FILMSTRIP_TILE_W,
        16,
        // 48, not 100: on a typical seek bar that's still a frame every ~2% of
        // the timeline — visually indistinguishable while halving the build
        // cost on long clips (the count is clamp(duration_secs, min, max), so
        // every clip over the max pays the full price).
        48,
        cancel,
        on_progress,
    )
}

/// Ensure the lighter grid-hover sprite exists: fewer, smaller frames, so a
/// hover pays for its first frames in well under a second of extraction.
pub fn ensure_scrubstrip(
    cache_dir: &Path,
    ffmpeg: Option<&Path>,
    src: &Path,
    cancel: &(dyn Fn() -> bool + Sync),
    on_progress: &(dyn Fn(u32, u32) + Sync),
) -> Result<(PathBuf, Filmstrip), String> {
    ensure_sprite(
        ffmpeg,
        src,
        scrubstrip_path(cache_dir, src),
        SCRUBSTRIP_COLS,
        SCRUBSTRIP_TILE_W,
        12,
        40,
        cancel,
        on_progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn ensure_sprite(
    ffmpeg: Option<&Path>,
    src: &Path,
    sprite: PathBuf,
    cols: u32,
    tile_w: u32,
    min_frames: u32,
    max_frames: u32,
    cancel: &(dyn Fn() -> bool + Sync),
    on_progress: &(dyn Fn(u32, u32) + Sync),
) -> Result<(PathBuf, Filmstrip), String> {
    let meta_path = sprite.with_extension("json");
    let read_cached = || -> Option<Filmstrip> {
        if !sprite.exists() {
            return None;
        }
        let txt = std::fs::read_to_string(&meta_path).ok()?;
        serde_json::from_str::<Filmstrip>(&txt).ok()
    };
    if let Some(fs) = read_cached() {
        return Ok((sprite, fs));
    }
    let ff = ffmpeg.ok_or("ffmpeg not available")?;
    let duration = probe_duration(ff, src).ok_or("could not read video duration")?;
    // ~1 frame/second, clamped so short clips stay dense and long ones do not
    // blow up cache or keep the extractor busy longer than a preview should.
    let count = (duration.round() as u32).clamp(min_frames, max_frames);
    let rows = count.div_ceil(cols);

    // One build at a time process-wide; re-check the cache and the cancel token
    // after the wait (the previous holder may have built this very sprite, or
    // the hover may have moved on while we queued).
    let _guard = SPRITE_BUILD_LOCK
        .lock()
        .map_err(|_| "sprite lock poisoned".to_string())?;
    if let Some(fs) = read_cached() {
        return Ok((sprite, fs));
    }
    if cancel() {
        return Err(SPRITE_CANCELLED.into());
    }

    // Per-frame JPEGs land in a scratch dir (local temp, not the media SSD),
    // then composite into the sprite with the `image` crate.
    let scratch = std::env::temp_dir().join(format!(
        "foxcull-sprite-{}-{}",
        std::process::id(),
        sprite
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("sprite")
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&scratch);
    };

    let next = AtomicU32::new(0);
    let done = AtomicU32::new(0);
    let aborted = AtomicBool::new(false);
    std::thread::scope(|s| {
        for _ in 0..sprite_parallel().min(count) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= count || aborted.load(Ordering::Relaxed) {
                    break;
                }
                if cancel() {
                    aborted.store(true, Ordering::Relaxed);
                    break;
                }
                // Mid-cell timestamps so the first/last frames aren't the (often
                // black) container edges.
                let at = (i as f64 + 0.5) * duration / count as f64;
                let out = scratch.join(format!("{i:03}.jpg"));
                if !extract_frame_at(ff, src, at, tile_w, true, &out) {
                    // Odd container / no keyframe found there — decode exactly.
                    extract_frame_at(ff, src, at, tile_w, false, &out);
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                on_progress(d, count);
            });
        }
    });
    if aborted.load(Ordering::Relaxed) || cancel() {
        cleanup();
        return Err(SPRITE_CANCELLED.into());
    }

    // Composite. All frames of one clip share dimensions (same rotation applied
    // by ffmpeg each time); the first decoded frame defines the tile box and any
    // stray mismatch is resized to fit. Missing frames stay black cells.
    let mut frames: Vec<Option<image::RgbImage>> = Vec::with_capacity(count as usize);
    let mut tile_dims: Option<(u32, u32)> = None;
    for i in 0..count {
        let p = scratch.join(format!("{i:03}.jpg"));
        match image::open(&p) {
            Ok(img) => {
                let rgb = img.to_rgb8();
                if tile_dims.is_none() {
                    tile_dims = Some((rgb.width(), rgb.height()));
                }
                frames.push(Some(rgb));
            }
            Err(_) => frames.push(None),
        }
    }
    let Some((tw, th)) = tile_dims else {
        // Not one frame came out via seeking — an unseekable container. Fall
        // back to the single-pass keyframe scan (slow but universal).
        cleanup();
        let fps = count as f64 / duration;
        make_filmstrip_fullscan(ff, src, &sprite, cols, rows, fps, tile_w)?;
        let (w, h) = image::image_dimensions(&sprite).map_err(|e| e.to_string())?;
        let fs = Filmstrip {
            cols,
            rows,
            count,
            tile_w: (w / cols).max(1),
            tile_h: (h / rows).max(1),
            duration,
        };
        if let Ok(txt) = serde_json::to_string(&fs) {
            let _ = std::fs::write(&meta_path, txt);
        }
        return Ok((sprite, fs));
    };
    let mut canvas = image::RgbImage::new(tw * cols, th * rows);
    for (i, frame) in frames.into_iter().enumerate() {
        let Some(frame) = frame else { continue };
        let frame = if frame.dimensions() == (tw, th) {
            frame
        } else {
            image::imageops::resize(&frame, tw, th, image::imageops::FilterType::Triangle)
        };
        let x = (i as u32 % cols) * tw;
        let y = (i as u32 / cols) * th;
        image::imageops::replace(&mut canvas, &frame, x as i64, y as i64);
    }
    cleanup();
    let file = std::fs::File::create(&sprite).map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 82)
        .encode_image(&canvas)
        .map_err(|e| e.to_string())?;
    std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;
    drop(writer);
    let fs = Filmstrip {
        cols,
        rows,
        count,
        tile_w: tw,
        tile_h: th,
        duration,
    };
    if let Ok(txt) = serde_json::to_string(&fs) {
        let _ = std::fs::write(&meta_path, txt);
    }
    Ok((sprite, fs))
}
