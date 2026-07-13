//! Embedded-JPEG extraction from TIFF-based RAW files (NEF primarily; the same
//! mechanism also covers CR2/ARW/DNG, which share the TIFF 6.0 container) and
//! bulk RAW → JPEG export.
//!
//! ## Why extraction instead of demosaicing
//!
//! A Nikon NEF (and most other camera RAWs) is a TIFF file that carries the
//! sensor data *plus* one or more JPEGs the camera itself rendered at capture
//! time — a tiny EXIF thumbnail (~160px, for camera-back review) and a
//! full-resolution preview built from the camera's own white balance and
//! Picture Control. Demosaicing the raw sensor data from scratch gives a flat,
//! unprocessed image that looks nothing like what the photographer saw on the
//! camera; pulling the embedded full-res preview instead gives *exactly* the
//! out-of-camera JPEG, for free, with zero raw-decode work.
//!
//! ## The TIFF walk
//!
//! A TIFF file is a byte-order header (`II`/`MM` + a magic number + the offset
//! of IFD0) followed by a linked list of Image File Directories (IFDs), each a
//! flat array of 12-byte tag/type/count/value entries plus a "next IFD" offset
//! that continues the chain (IFD0 → IFD1 is how a plain TIFF stores a
//! thumbnail bitmap alongside the main image, for example).
//!
//! RAW files nest further: IFD0's `SubIFDs` tag (0x014A) can point at *several*
//! more IFDs (typically the raw sensor data and, separately, the full-res
//! preview), and the `ExifIFD` tag (0x8769) points at one more IFD holding
//! EXIF-proper fields. Any of these IFDs — not just IFD0 — can be the one
//! carrying the preview pair, so we breadth-first-walk the whole graph (IFD0's
//! chain, every SubIFD, the EXIF IFD, each of *their* chains too) rather than
//! assuming a fixed shape. A visited-offset set plus a hard IFD-count cap keep
//! this bounded even against an adversarial/corrupt file with a cyclic offset
//! chain.
//!
//! In each IFD we look for the standard preview pair — `JPEGInterchangeFormat`
//! (0x0201, the byte offset) + `JPEGInterchangeFormatLength` (0x0202, the byte
//! length) — and, for RAWs that instead store the preview as TIFF strips
//! (`Compression` 0x0103 == 6 "old JPEG" or 7 "new JPEG", strips in 0x0111/
//! 0x0117), we reconstruct the JPEG stream from those strips when the first
//! strip starts with the JPEG SOI marker.
//!
//! ## The largest-candidate heuristic
//!
//! A single NEF can legitimately expose *multiple* preview-shaped tag pairs
//! (a small IFD1 thumbnail, a mid-size "fast preview" some cameras add, and
//! the real full-resolution preview). Rather than special-case which IFD is
//! "the" preview IFD — the layout differs across camera generations — we
//! collect every candidate the walk turns up, validate each one is really a
//! complete JPEG stream, and keep whichever decodes to the most bytes. The
//! biggest embedded JPEG is, by construction, the full-resolution one.
//!
//! If the tagged walk finds nothing usable (some encoders omit these tags, or
//! a file is malformed enough that our parser bails), we fall back to a raw
//! byte-scan for the largest `FFD8..FFD9` span in the file (reusing
//! [`crate::media::largest_embedded_jpeg`], the same brute-force scan already
//! used for thumbnailing), requiring at least 100 KB so we don't accidentally
//! pick up the tiny EXIF thumbnail.
//!
//! ## The verbatim-write guarantee
//!
//! The embedded preview is already the camera's finished JPEG. If the file's
//! EXIF orientation is 1 (or absent), we write those extracted bytes straight
//! to disk with no re-encode — bit-for-bit what the camera produced, zero
//! generational quality loss. Only when the shot needs rotating/flipping to
//! display upright do we decode, transform, and re-encode (quality 95) — the
//! embedded preview itself carries no orientation tag, so an un-rotated
//! verbatim write would come out sideways for portrait shots.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use rayon::prelude::*;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::media::{self, Kind};

// ─────────────────────────── TIFF byte primitives ───────────────────────────

fn read_u16_at(data: &[u8], off: usize, le: bool) -> Option<u16> {
    let b = data.get(off..off.checked_add(2)?)?;
    Some(if le {
        u16::from_le_bytes([b[0], b[1]])
    } else {
        u16::from_be_bytes([b[0], b[1]])
    })
}

fn read_u32_at(data: &[u8], off: usize, le: bool) -> Option<u32> {
    let b = data.get(off..off.checked_add(4)?)?;
    Some(if le {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    })
}

/// Byte width of a TIFF field type. Only the integer-ish types matter for the
/// tags we read (offsets/lengths/compression); RATIONAL/FLOAT/DOUBLE are
/// recognized (so we can skip their width correctly) but never interpreted.
fn type_size(t: u16) -> Option<usize> {
    match t {
        1 | 2 | 6 | 7 => Some(1),  // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => Some(2),          // SHORT, SSHORT
        4 | 9 | 11 => Some(4),     // LONG, SLONG, FLOAT
        5 | 10 | 12 => Some(8),    // RATIONAL, SRATIONAL, DOUBLE
        _ => None,
    }
}

/// One raw 12-byte IFD entry, unparsed beyond the header fields.
struct IfdEntry {
    tag: u16,
    typ: u16,
    count: u32,
    /// The entry's 4-byte value/offset field, verbatim (byte-order not yet
    /// applied) — interpreted by [`entry_values_u32`] once we know whether the
    /// value is stored inline or elsewhere in the file.
    value_bytes: [u8; 4],
}

struct ParsedIfd {
    entries: Vec<IfdEntry>,
    /// Offset of the next IFD in this chain, or 0 if this is the last one.
    next: u32,
}

/// Parse the IFD at `offset`: an entry count, that many 12-byte entries, then
/// a 4-byte "next IFD" offset. Bounds-checked throughout — returns `None`
/// (never panics) on any offset that doesn't fit in `data`, which is the
/// common case when walking a corrupt file or chasing a bogus offset.
fn parse_ifd(data: &[u8], offset: usize, le: bool) -> Option<ParsedIfd> {
    let count = read_u16_at(data, offset, le)? as usize;
    let entries_start = offset.checked_add(2)?;
    let entries_len = count.checked_mul(12)?;
    let entries_end = entries_start.checked_add(entries_len)?;
    if entries_end > data.len() {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let e_off = entries_start + i * 12;
        let tag = read_u16_at(data, e_off, le)?;
        let typ = read_u16_at(data, e_off + 2, le)?;
        let cnt = read_u32_at(data, e_off + 4, le)?;
        let vb = data.get(e_off + 8..e_off + 12)?;
        entries.push(IfdEntry {
            tag,
            typ,
            count: cnt,
            value_bytes: [vb[0], vb[1], vb[2], vb[3]],
        });
    }
    // The "next IFD" offset is optional in principle (a truncated tail), so a
    // missing one just ends the chain rather than failing the whole IFD.
    let next = if entries_end.checked_add(4).map_or(false, |e| e <= data.len()) {
        read_u32_at(data, entries_end, le).unwrap_or(0)
    } else {
        0
    };
    Some(ParsedIfd { entries, next })
}

/// Read an entry's value(s) as `u32`s, honoring the TIFF inline-vs-offset
/// rule: if `type_size * count` fits in the entry's 4-byte value field, the
/// value is stored there directly (left-justified); otherwise the field holds
/// a file offset to the real data. Bounds-checked against `data.len()`.
fn entry_values_u32(data: &[u8], le: bool, e: &IfdEntry) -> Option<Vec<u32>> {
    let sz = type_size(e.typ)?;
    let total = sz.checked_mul(e.count as usize)?;
    if total == 0 {
        return Some(Vec::new());
    }
    let bytes: &[u8] = if total <= 4 {
        &e.value_bytes[..total]
    } else {
        let off = if le {
            u32::from_le_bytes(e.value_bytes)
        } else {
            u32::from_be_bytes(e.value_bytes)
        } as usize;
        let end = off.checked_add(total)?;
        data.get(off..end)?
    };
    let mut out = Vec::with_capacity(e.count as usize);
    for chunk in bytes.chunks(sz) {
        let v = match sz {
            1 => chunk[0] as u32,
            2 => {
                if le {
                    u16::from_le_bytes([chunk[0], chunk[1]]) as u32
                } else {
                    u16::from_be_bytes([chunk[0], chunk[1]]) as u32
                }
            }
            4 => {
                if le {
                    u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                } else {
                    u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                }
            }
            // RATIONAL/DOUBLE (8 bytes) never show up in the tags we read —
            // treat as "no usable integer value" rather than misinterpret.
            _ => return None,
        };
        out.push(v);
    }
    Some(out)
}

// ───────────────────────────── preview candidates ────────────────────────────

/// Where a candidate preview's bytes live in the file: either one contiguous
/// run (the common `JPEGInterchangeFormat`/`Length` case), or several TIFF
/// strips that concatenate into one JPEG stream (the `Compression` 6/7 strip
/// case some encoders use instead).
enum PreviewSource {
    Span(usize, usize),
    Strips(Vec<(usize, usize)>),
}

struct Candidate {
    source: PreviewSource,
    /// Declared/approximate byte length, used only to order candidates before
    /// the (more expensive) validate-and-materialize pass confirms the real
    /// length.
    approx_len: usize,
}

impl Candidate {
    fn span(start: usize, len: usize) -> Self {
        Candidate {
            source: PreviewSource::Span(start, len),
            approx_len: len,
        }
    }
    fn strips(pieces: Vec<(usize, usize)>) -> Self {
        let approx_len = pieces.iter().map(|(_, l)| l).sum();
        Candidate {
            source: PreviewSource::Strips(pieces),
            approx_len,
        }
    }
}

/// Index just past a `FFD9` end-of-image marker, searching `[start, bound)`.
/// `bound` is always clamped to `data.len()` first, so every index this
/// touches is in range.
fn find_eoi(data: &[u8], start: usize, bound: usize) -> Option<usize> {
    let bound = bound.min(data.len());
    let mut i = start;
    while i + 1 < bound {
        if data[i] == 0xFF && data[i + 1] == 0xD9 {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

/// Confirm `data[start..]` is really a JPEG stream (`FFD8` start) and locate
/// its true end. We search for `FFD9` starting from the declared length
/// rather than trusting it exactly, allowing generous trailing padding (some
/// encoders round strip/segment lengths up) — capped at 64 KB past the
/// declared length so a bogus `declared_len` can't turn this into an
/// unbounded scan of the rest of the file. Returns `(start, real_len)`.
fn validate_jpeg_span(data: &[u8], start: usize, declared_len: usize) -> Option<(usize, usize)> {
    if data.get(start..start.checked_add(2)?)? != [0xFF, 0xD8] {
        return None;
    }
    let search_end = start
        .checked_add(declared_len)?
        .saturating_add(65_536)
        .min(data.len());
    let eoi = find_eoi(data, start.checked_add(2)?, search_end)?;
    Some((start, eoi - start))
}

/// Turn a candidate into its actual JPEG bytes, validating along the way.
/// Returns `None` if the candidate's offsets don't check out (out of bounds,
/// no SOI, no locatable EOI) — malformed input is simply dropped, never
/// panics.
fn materialize(data: &[u8], cand: &Candidate) -> Option<Vec<u8>> {
    match &cand.source {
        PreviewSource::Span(start, len) => {
            let (s, real_len) = validate_jpeg_span(data, *start, *len)?;
            data.get(s..s.checked_add(real_len)?).map(|b| b.to_vec())
        }
        PreviewSource::Strips(pieces) => {
            let mut buf = Vec::with_capacity(cand.approx_len);
            for (off, len) in pieces {
                let end = off.checked_add(*len)?;
                buf.extend_from_slice(data.get(*off..end)?);
            }
            if buf.len() < 4 || buf[0] != 0xFF || buf[1] != 0xD8 {
                return None;
            }
            let end = find_eoi(&buf, 2, buf.len())?;
            buf.truncate(end);
            Some(buf)
        }
    }
}

/// Breadth-first walk of the IFD graph starting at IFD0: follows each IFD's
/// "next" chain, and recurses into SubIFDs (0x014A, possibly several) and the
/// EXIF IFD (0x8769). A visited-offset set skips repeats (guards against a
/// cyclic/self-referential chain in a corrupt file) and `MAX_IFDS` bounds the
/// total work even against an adversarial input.
fn collect_candidates(data: &[u8], le: bool, ifd0_offset: usize) -> Vec<Candidate> {
    const MAX_IFDS: usize = 64;
    let mut candidates = Vec::new();
    let mut visited: HashSet<usize> = HashSet::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(ifd0_offset);

    while let Some(offset) = queue.pop_front() {
        if visited.len() >= MAX_IFDS {
            break;
        }
        if !visited.insert(offset) {
            continue;
        }
        let ifd = match parse_ifd(data, offset, le) {
            Some(i) => i,
            None => continue,
        };
        if ifd.next != 0 {
            queue.push_back(ifd.next as usize);
        }

        let mut jpeg_off: Option<u32> = None;
        let mut jpeg_len: Option<u32> = None;
        let mut compression: Option<u32> = None;
        let mut strip_offsets: Option<Vec<u32>> = None;
        let mut strip_counts: Option<Vec<u32>> = None;

        for e in &ifd.entries {
            match e.tag {
                0x0201 => jpeg_off = entry_values_u32(data, le, e).and_then(|v| v.first().copied()),
                0x0202 => jpeg_len = entry_values_u32(data, le, e).and_then(|v| v.first().copied()),
                0x0103 => compression = entry_values_u32(data, le, e).and_then(|v| v.first().copied()),
                0x0111 => strip_offsets = entry_values_u32(data, le, e),
                0x0117 => strip_counts = entry_values_u32(data, le, e),
                // SubIFDs: 0..N more IFDs, each possibly holding a preview.
                0x014A => {
                    if let Some(offs) = entry_values_u32(data, le, e) {
                        for o in offs {
                            queue.push_back(o as usize);
                        }
                    }
                }
                // EXIF IFD: one more IFD (rarely holds a preview itself, but
                // some encoders tuck MakerNote-adjacent previews here).
                0x8769 => {
                    if let Some(offs) = entry_values_u32(data, le, e) {
                        if let Some(&o) = offs.first() {
                            queue.push_back(o as usize);
                        }
                    }
                }
                _ => {}
            }
        }

        if let (Some(off), Some(len)) = (jpeg_off, jpeg_len) {
            candidates.push(Candidate::span(off as usize, len as usize));
        }

        // Some RAWs (and DNGs) store the preview as TIFF strips instead of a
        // JPEGInterchangeFormat pair: Compression 6 (old-style JPEG) or 7
        // (new-style, full JPEG stream per strip) with the strip data itself
        // starting FFD8. Only trust this when the strip really looks like a
        // JPEG — otherwise it's genuine (non-JPEG) strip-compressed raster
        // data, e.g. Compression 7 CCITT-adjacent strips.
        if matches!(compression, Some(6) | Some(7)) {
            if let (Some(offs), Some(cnts)) = (&strip_offsets, &strip_counts) {
                if !offs.is_empty() && offs.len() == cnts.len() {
                    let first_off = offs[0] as usize;
                    if data.get(first_off..first_off.saturating_add(2)) == Some(&[0xFF, 0xD8]) {
                        if offs.len() == 1 {
                            candidates.push(Candidate::span(offs[0] as usize, cnts[0] as usize));
                        } else {
                            let pieces: Vec<(usize, usize)> = offs
                                .iter()
                                .zip(cnts.iter())
                                .map(|(&o, &c)| (o as usize, c as usize))
                                .collect();
                            candidates.push(Candidate::strips(pieces));
                        }
                    }
                }
            }
        }
    }

    candidates
}

/// Extract the largest embedded JPEG preview from a TIFF-based RAW file's raw
/// bytes. Walks the tagged IFD structure first (see module docs); if that
/// finds nothing valid, falls back to a brute-force scan for the largest
/// `FFD8..FFD9` span in the file (≥100 KB, to skip the tiny EXIF thumbnail).
pub fn best_embedded_preview(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 8 {
        return Err("file too small to be a TIFF-based RAW".to_string());
    }
    let le = match &data[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err("not a TIFF-based RAW (missing II/MM byte-order marker)".to_string()),
    };
    if read_u16_at(data, 2, le) != Some(42) {
        return Err("not a TIFF-based RAW (bad TIFF magic number)".to_string());
    }
    let ifd0 = match read_u32_at(data, 4, le) {
        Some(o) => o as usize,
        None => return Err("truncated TIFF header".to_string()),
    };

    let candidates = collect_candidates(data, le, ifd0);
    let mut best: Option<Vec<u8>> = None;
    for cand in &candidates {
        if let Some(bytes) = materialize(data, cand) {
            if best.as_ref().map_or(true, |b| bytes.len() > b.len()) {
                best = Some(bytes);
            }
        }
    }
    if let Some(bytes) = best {
        return Ok(bytes);
    }

    // No tagged preview validated — brute-force fallback, same scan used for
    // thumbnailing elsewhere in the app, with a size floor so we don't return
    // the postage-stamp EXIF thumbnail instead of a real preview.
    match media::largest_embedded_jpeg(data) {
        Some(jpg) if jpg.len() >= 100_000 => Ok(jpg.to_vec()),
        _ => Err("no usable embedded JPEG preview found in this RAW file".to_string()),
    }
}

// ───────────────────────────────── commands ──────────────────────────────────

/// If `path` is taken, append " (2)", " (3)", … before the extension — mirrors
/// `commands::uniquify` (private to that module, so duplicated here rather
/// than exposed cross-module) — used so a re-run, or a real RAW+JPEG pair from
/// the camera, never gets clobbered by the export.
fn uniquify(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
    let mut n = 2;
    loop {
        let name = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let cand = parent.join(name);
        if !cand.exists() {
            return cand;
        }
        n += 1;
    }
}

/// Dimensions + size of a RAW's best embedded preview, without writing
/// anything. `width`/`height` are the *final displayed* dimensions — swapped
/// from the raw JPEG's own dimensions when `needs_rotate` implies a 90°/270°
/// turn — so the caller can show an accurate size without doing the rotation
/// math itself.
#[derive(Serialize)]
pub struct RawPreviewInfo {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub needs_rotate: bool,
}

/// Probe a RAW file's best embedded preview: dimensions and byte size, cheap
/// (reads the file once, decodes only the JPEG header for dimensions via
/// `into_dimensions` — no full pixel decode) and side-effect-free.
#[tauri::command]
pub fn raw_embedded_probe(path: String) -> Result<RawPreviewInfo, String> {
    let src = Path::new(&path);
    let data = std::fs::read(src).map_err(|e| format!("read {path}: {e}"))?;
    let jpg = best_embedded_preview(&data)?;
    let (w, h) =
        image::ImageReader::with_format(std::io::Cursor::new(&jpg), image::ImageFormat::Jpeg)
            .into_dimensions()
            .map_err(|e| format!("embedded preview unreadable: {e}"))?;
    let o = media::orientation(src);
    let (width, height) = if matches!(o, 5 | 6 | 7 | 8) { (h, w) } else { (w, h) };
    Ok(RawPreviewInfo {
        width,
        height,
        bytes: jpg.len() as u64,
        needs_rotate: o != 1,
    })
}

/// Extract `src`'s embedded preview and write it as `<stem>.JPG` next to the
/// source file (uniquified — never overwrites), stamping the output's mtime
/// to match the source so the export sorts sensibly in any file manager.
/// Verbatim write when upright (orientation 1/absent); decode+rotate+encode
/// at quality 95 otherwise, carrying the preview's ICC profile along either
/// way (`embed_icc` is a no-op for the verbatim path — nothing to add, the
/// bytes already have whatever profile the camera embedded).
fn export_one_raw(src: &Path) -> Result<PathBuf, String> {
    let data = std::fs::read(src).map_err(|e| format!("read: {e}"))?;
    let jpg = best_embedded_preview(&data)?;
    let o = media::orientation(src);

    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let dir = src
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let target = uniquify(dir.join(format!("{stem}.JPG")));

    if o == 1 {
        // The camera's own bytes, byte-for-byte — no re-encode, zero quality
        // loss vs. what shipped in the NEF.
        std::fs::write(&target, &jpg).map_err(|e| format!("write: {e}"))?;
    } else {
        let img = image::load_from_memory_with_format(&jpg, image::ImageFormat::Jpeg)
            .map_err(|e| format!("decode embedded preview: {e}"))?;
        let img = crate::thumbs::apply_orientation(img, o);
        let icc = media::icc_from_jpeg(&jpg);
        let f = std::fs::File::create(&target).map_err(|e| format!("write: {e}"))?;
        let mut enc =
            image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 95);
        enc.encode_image(&image::DynamicImage::ImageRgb8(img.to_rgb8()))
            .map_err(|e| format!("encode: {e}"))?;
        if let Some(icc) = icc {
            let _ = crate::thumbs::embed_icc(&target, &icc);
        }
    }

    if let Ok(meta) = std::fs::metadata(src) {
        if let Ok(mtime) = meta.modified() {
            let _ = filetime::set_file_mtime(&target, filetime::FileTime::from_system_time(mtime));
        }
    }
    Ok(target)
}

/// Progress payload emitted after each file during `export_raw_jpegs` — the
/// frontend folds this into a progress bar for the bulk conversion.
#[derive(Clone, Serialize)]
pub struct RawExportProgress {
    pub done: u64,
    pub total: u64,
    pub current: String,
}

#[derive(Serialize)]
pub struct RawExportResult {
    pub written: Vec<String>,
    /// (path, reason) — inputs that weren't RAW files at all, left untouched.
    pub skipped: Vec<(String, String)>,
    /// (path, reason) — RAW files we attempted and failed to convert.
    pub failed: Vec<(String, String)>,
}

/// Dedicated, size-2 rayon pool for the export — deliberately NOT the global
/// pool. Same house rule as `commands::warm_pool`: this is disk-seek bound,
/// and the user's "20GB backlog of NEFs from friends" lives on drives that
/// include spinning disks, where more than a couple of concurrent multi-MB
/// reads thrashes the head and makes every read slower, not faster.
fn export_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .expect("raw export pool")
    })
}

/// Bulk RAW → JPEG export: for each of `paths`, extract the embedded preview
/// and write it next to the source (see [`export_one_raw`]). Runs on a small
/// capped pool (not serial — two readers keeps an SSD busy without thrashing
/// an HDD) and emits `raw-export-progress` after every file so the frontend
/// can show a live counter.
#[tauri::command]
pub async fn export_raw_jpegs(app: AppHandle, paths: Vec<String>) -> Result<RawExportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let total = paths.len() as u64;
        let done = AtomicU64::new(0);
        let written: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let skipped: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
        let failed: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

        export_pool().install(|| {
            paths.par_iter().for_each(|p| {
                let src = PathBuf::from(p);
                let name = src
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.clone());

                if !matches!(media::classify(&src), Kind::Raw) {
                    skipped.lock().push((p.clone(), "not a RAW file".to_string()));
                } else {
                    match export_one_raw(&src) {
                        Ok(out) => written.lock().push(out.to_string_lossy().to_string()),
                        Err(e) => failed.lock().push((p.clone(), e)),
                    }
                }

                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = app.emit(
                    "raw-export-progress",
                    RawExportProgress {
                        done: n,
                        total,
                        current: name,
                    },
                );
            });
        });

        Ok(RawExportResult {
            written: written.into_inner(),
            skipped: skipped.into_inner(),
            failed: failed.into_inner(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le_u16(v: u16) -> [u8; 2] {
        v.to_le_bytes()
    }
    fn le_u32(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    /// A minimal but complete `FFD8 .. FFD9` JPEG stream of exactly `len`
    /// bytes (filler in between) — enough to exercise the SOI/EOI validation
    /// without a real JPEG encoder.
    fn fake_jpeg(len: usize) -> Vec<u8> {
        let mut v = vec![0u8; len];
        v[0] = 0xFF;
        v[1] = 0xD8;
        let n = v.len();
        v[n - 2] = 0xFF;
        v[n - 1] = 0xD9;
        v
    }

    /// Build a minimal synthetic little-endian TIFF that mirrors the shape a
    /// real NEF uses for its full-res preview: IFD0 has one entry (SubIFDs,
    /// tag 0x014A) pointing at a second IFD that carries the
    /// JPEGInterchangeFormat/Length pair (0x0201/0x0202) for `jpeg`. This
    /// exercises the byte-order header, the IFD0 walk, the SubIFD recursion,
    /// and the preview-tag pair + FFD8/FFD9 validation — the exact path a
    /// real NEF's preview takes — without needing a camera file on disk.
    fn build_synthetic_nef(jpeg: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&le_u16(42));
        let ifd0_ptr_pos = buf.len();
        buf.extend_from_slice(&[0, 0, 0, 0]); // patched below
        let ifd0_offset = buf.len() as u32;
        buf[ifd0_ptr_pos..ifd0_ptr_pos + 4].copy_from_slice(&le_u32(ifd0_offset));

        // IFD0: one entry — SubIFDs (0x014A, LONG, count 1) — value patched
        // once we know where the SubIFD lands.
        buf.extend_from_slice(&le_u16(1)); // entry count
        buf.extend_from_slice(&le_u16(0x014A));
        buf.extend_from_slice(&le_u16(4)); // type LONG
        buf.extend_from_slice(&le_u32(1)); // count
        let subifd_ptr_pos = buf.len();
        buf.extend_from_slice(&[0, 0, 0, 0]); // patched below
        buf.extend_from_slice(&le_u32(0)); // next IFD = none

        let subifd_offset = buf.len() as u32;
        buf[subifd_ptr_pos..subifd_ptr_pos + 4].copy_from_slice(&le_u32(subifd_offset));

        // SubIFD (the "preview IFD"): JPEGInterchangeFormat + Length.
        buf.extend_from_slice(&le_u16(2)); // entry count
        buf.extend_from_slice(&le_u16(0x0201));
        buf.extend_from_slice(&le_u16(4)); // type LONG
        buf.extend_from_slice(&le_u32(1)); // count
        let jpeg_off_ptr_pos = buf.len();
        buf.extend_from_slice(&[0, 0, 0, 0]); // patched below
        buf.extend_from_slice(&le_u16(0x0202));
        buf.extend_from_slice(&le_u16(4)); // type LONG
        buf.extend_from_slice(&le_u32(1)); // count
        buf.extend_from_slice(&le_u32(jpeg.len() as u32));
        buf.extend_from_slice(&le_u32(0)); // next IFD = none

        let jpeg_offset = buf.len() as u32;
        buf[jpeg_off_ptr_pos..jpeg_off_ptr_pos + 4].copy_from_slice(&le_u32(jpeg_offset));
        buf.extend_from_slice(jpeg);
        buf
    }

    #[test]
    fn extracts_preview_via_subifd() {
        let jpeg = fake_jpeg(50_000);
        let tiff = build_synthetic_nef(&jpeg);
        let out = best_embedded_preview(&tiff).expect("preview extracted");
        assert_eq!(out, jpeg);
    }

    #[test]
    fn picks_the_largest_of_multiple_candidates() {
        // Two SubIFDs off IFD0: a small "thumbnail-shaped" preview and a big
        // one. The walk must return the big one regardless of IFD order.
        let small = fake_jpeg(2_000);
        let big = fake_jpeg(80_000);

        // Build by hand: IFD0 with TWO SubIFD offsets (count=2 in one 0x014A
        // entry), each pointing at its own tiny preview IFD.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&le_u16(42));
        buf.extend_from_slice(&le_u32(8));

        // IFD0 at offset 8: one entry, SubIFDs with count=2 (doesn't fit
        // inline, so value field holds an offset to two u32s).
        buf.extend_from_slice(&le_u16(1));
        buf.extend_from_slice(&le_u16(0x014A));
        buf.extend_from_slice(&le_u16(4)); // LONG
        buf.extend_from_slice(&le_u32(2)); // count = 2 -> offset-stored
        let subifd_array_ptr_pos = buf.len();
        buf.extend_from_slice(&[0, 0, 0, 0]);
        buf.extend_from_slice(&le_u32(0)); // next = none

        // The array of two SubIFD offsets, patched after we know where each
        // preview IFD lands.
        let subifd_array_offset = buf.len() as u32;
        buf[subifd_array_ptr_pos..subifd_array_ptr_pos + 4]
            .copy_from_slice(&le_u32(subifd_array_offset));
        buf.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // two placeholders

        let mk_preview_ifd = |buf: &mut Vec<u8>, jpeg: &[u8]| -> u32 {
            let ifd_offset = buf.len() as u32;
            buf.extend_from_slice(&le_u16(2));
            buf.extend_from_slice(&le_u16(0x0201));
            buf.extend_from_slice(&le_u16(4));
            buf.extend_from_slice(&le_u32(1));
            let jpeg_off_ptr_pos = buf.len();
            buf.extend_from_slice(&[0, 0, 0, 0]);
            buf.extend_from_slice(&le_u16(0x0202));
            buf.extend_from_slice(&le_u16(4));
            buf.extend_from_slice(&le_u32(1));
            buf.extend_from_slice(&le_u32(jpeg.len() as u32));
            buf.extend_from_slice(&le_u32(0));
            let jpeg_offset = buf.len() as u32;
            buf[jpeg_off_ptr_pos..jpeg_off_ptr_pos + 4].copy_from_slice(&le_u32(jpeg_offset));
            buf.extend_from_slice(jpeg);
            ifd_offset
        };

        let small_ifd_off = mk_preview_ifd(&mut buf, &small);
        let big_ifd_off = mk_preview_ifd(&mut buf, &big);
        buf[subifd_array_offset as usize..subifd_array_offset as usize + 4]
            .copy_from_slice(&le_u32(small_ifd_off));
        buf[subifd_array_offset as usize + 4..subifd_array_offset as usize + 8]
            .copy_from_slice(&le_u32(big_ifd_off));

        let out = best_embedded_preview(&buf).expect("preview extracted");
        assert_eq!(out, big, "must pick the larger of the two candidate previews");
    }

    #[test]
    fn malformed_input_never_panics() {
        assert!(best_embedded_preview(&[]).is_err());
        assert!(best_embedded_preview(&[0u8; 4]).is_err());
        assert!(best_embedded_preview(&vec![0u8; 16]).is_err());
        // Valid header, but IFD0 offset points past EOF.
        let mut junk = Vec::new();
        junk.extend_from_slice(b"II");
        junk.extend_from_slice(&le_u16(42));
        junk.extend_from_slice(&le_u32(999_999));
        assert!(best_embedded_preview(&junk).is_err());
        // Big-endian header, truncated right after it.
        let mut mm = Vec::new();
        mm.extend_from_slice(b"MM");
        mm.extend_from_slice(&[0, 42]);
        mm.extend_from_slice(&[0, 0, 0, 8]);
        assert!(best_embedded_preview(&mm).is_err());
    }

    #[test]
    fn falls_back_to_brute_force_scan() {
        // No TIFF structure the walker can use at all, but a real embedded
        // JPEG sits in the bytes anyway (e.g. an encoder that skipped the
        // tags) — the >=100KB brute-force fallback should still find it.
        let mut data = b"II".to_vec();
        data.extend_from_slice(&le_u16(42));
        data.extend_from_slice(&le_u32(8));
        data.extend_from_slice(&le_u16(0)); // IFD0 with zero entries
        data.extend_from_slice(&le_u32(0)); // next = none
        data.extend(std::iter::repeat(0u8).take(64)); // filler before the JPEG
        let jpeg = fake_jpeg(150_000);
        data.extend_from_slice(&jpeg);

        let out = best_embedded_preview(&data).expect("fallback found the JPEG");
        assert_eq!(out, jpeg);
    }

    /// Opt-in validation against a real camera file: set
    /// `FOXCULL_TEST_NEF=<path to a .NEF>` and run `cargo test` with that
    /// variable set. No `.NEF`/`.CR2`/`.ARW` file was found anywhere on the
    /// machine this was developed on, so this test is a no-op (not a pass —
    /// it prints and returns) unless the variable is provided.
    #[test]
    fn real_raw_file_if_available() {
        let Ok(path) = std::env::var("FOXCULL_TEST_NEF") else {
            eprintln!(
                "FOXCULL_TEST_NEF not set — skipping real-RAW validation \
                 (no .NEF was found on the dev machine to bake in a fixture)"
            );
            return;
        };
        let data = std::fs::read(&path).expect("read FOXCULL_TEST_NEF");
        let jpg = best_embedded_preview(&data).expect("extract preview from real RAW");
        assert!(
            jpg.len() > 100_000,
            "extracted preview suspiciously small: {} bytes",
            jpg.len()
        );
        assert_eq!(&jpg[0..2], &[0xFF, 0xD8]);
        let (w, h) = image::ImageReader::with_format(
            std::io::Cursor::new(&jpg),
            image::ImageFormat::Jpeg,
        )
        .into_dimensions()
        .expect("decode dimensions of extracted preview");
        eprintln!("real RAW ({path}) preview: {w}x{h}, {} bytes", jpg.len());
    }
}
