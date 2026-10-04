use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};

use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::catalog::{Catalog, VideoSegment};
use crate::media::{self, Kind};
use crate::{thumbs, video};

/// Long edge (px) of grid/filmstrip thumbnails (matches the frontend <Thumb>).
const GRID_MAX: u32 = 320;

/// Long edge (px) of the capped loupe/focus preview. 1920 is sharp full-screen
/// while letting the JPEG decoder pick a 1/2 DCT scale on 12MP phone shots
/// (4000px → 2000px ≥ 1920), so the sharp decode is ~4x cheaper than full-res.
const LOUPE_MAX: u32 = 1920;

/// Threads the background thumbnail warmer may use. Deliberately small and
/// machine-aware: v0.3.0 warmed across ALL cores, and a dozen simultaneous
/// multi-MB reads thrashed the external SSD so badly that individual reads
/// stalled 50+ seconds and starved the photo the user was looking at. We cap at
/// a quarter of the cores, clamped to 1..=2 — leaving the foreground (loupe +
/// visible cells) plenty of CPU, and keeping the USB-SSD read queue shallow. On
/// the thin XPS 13 (4 cores) this is 1; on the Alienware (12) it's 2.
fn warm_threads() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // Clamped very low: this is I/O-bound, not CPU-bound, and the worst case is a
    // single spinning SATA disk (the user's internal drives), where every extra
    // concurrent original read makes the mechanical head seek-thrash and stalls
    // the foreground thumbnails the user is actually looking at. 1–2 keeps the
    // read queue shallow on an HDD while still using both heads of an SSD.
    (cores / 4).clamp(1, 2)
}

/// Dedicated, size-bounded rayon pool for warming (NOT the global pool, which
/// would grab every core).
fn warm_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(warm_threads())
            .build()
            .expect("warm pool")
    })
}

/// Process-wide state: the currently selected library root and the on-disk
/// thumbnail cache directory (in app-data, never on the user's SSD).
pub struct AppState {
    pub root: Mutex<Option<PathBuf>>,
    /// Thumbnail + poster cache directory. Follows the catalog: when the catalog
    /// is relocated onto the SSD, the cache moves next to it (`<catalogDir>/thumbs`)
    /// so posters/thumbs are generated ONCE and reused on every machine that reads
    /// the SSD — never a separate per-computer cache.
    pub cache_dir: Mutex<PathBuf>,
    /// Where app data (config, default catalog, cache) lives — app-data normally,
    /// or a `foxcull-data` folder next to the exe in portable mode.
    pub data_root: PathBuf,
    /// Current catalog file path (lives inside the active drive's library dir).
    pub catalog_path: Mutex<PathBuf>,
    /// The active library folder (`<drive>/_FoxCull`, or an app-data fallback for
    /// a read-only mount). Holds the catalog, the `thumbs` cache and `recycle`.
    pub lib_dir: Mutex<PathBuf>,
    /// Active per-drive recycle folder (`<libDir>/recycle`) — where folder-mode
    /// deletes land so the in-app Trash can list / restore / purge them.
    pub recycle_dir: Mutex<PathBuf>,
    /// Path to the bundled ffmpeg (next to our exe), or None on a dev build
    /// without it — then video posters fall back to the film placeholder.
    pub ffmpeg: Option<PathBuf>,
    /// Bumped on every folder switch so an in-flight background warming pass for
    /// the previous folder abandons itself instead of fighting for cores.
    pub warm_gen: Arc<AtomicU64>,
    /// Bumped by `cancel_edit_export` (and by each new export) so the in-flight
    /// export's ffmpeg child gets killed and its partial output deleted.
    pub export_gen: Arc<AtomicU64>,
}

/// `<catalogDir>/thumbs` — the cache lives beside whatever catalog file is in use.
pub fn cache_dir_for(catalog_path: &Path) -> PathBuf {
    catalog_path
        .parent()
        .map(|d| d.join("thumbs"))
        .unwrap_or_else(|| PathBuf::from("thumbs"))
}

/// Bundled per-drive library folder name: `<drive>/_FoxCull` holds that drive's
/// catalog, thumbnail cache and recycle bin, so everything for a drive travels
/// with it.
const LIB_DIRNAME: &str = "_FoxCull";

/// The in-app Trash, at the DRIVE ROOT and deliberately visible: `<drive>\FoxCull
/// Trash`. It used to hide inside `_FoxCull\recycle`, which meant a deleted clip
/// could not be browsed, previewed or played before you committed — the owner
/// could not answer "why did I throw this away?" without leaving the app.
///
/// Flat on purpose (no mirrored folder tree). Provenance lives in the catalog and
/// is mirrored into `_trash-index.json` beside the files, so a lost catalog can
/// no longer strand ~19 GB of media with no record of where it belongs.
pub const TRASH_DIRNAME: &str = "FoxCull Trash";

/// Sidecar that survives the catalog: `stored filename -> original rel-path`.
const TRASH_INDEX: &str = "_trash-index.json";

fn is_trash_dirname(name: &str) -> bool {
    name.eq_ignore_ascii_case(TRASH_DIRNAME)
}

/// Rewrite `_trash-index.json` from the catalog's current rows. Cheap (one small
/// file), and always a full rewrite so it can never drift into a half-truth.
fn write_trash_index(recycle: &Path, catalog: &Catalog) {
    if !recycle.is_dir() {
        return;
    }
    let rows = catalog.list_trash();
    let map: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({ "stored": r.stored, "orig": r.orig, "name": r.name, "deleted_at": r.deleted_at })
        })
        .collect();
    let body = serde_json::json!({
        "note": "Where each file in this folder came from. FoxCull rebuilds this automatically; it exists so the files can be restored even if the catalog is lost. Safe to leave alone.",
        "entries": map,
    });
    if let Ok(s) = serde_json::to_string_pretty(&body) {
        let _ = std::fs::write(recycle.join(TRASH_INDEX), s);
    }
}

/// Read the sidecar back: `lowercased stored filename -> (orig, deleted_at)`.
fn read_trash_index(recycle: &Path) -> HashMap<String, (String, i64)> {
    let mut out = HashMap::new();
    let Ok(s) = std::fs::read_to_string(recycle.join(TRASH_INDEX)) else {
        return out;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return out;
    };
    if let Some(arr) = v.get("entries").and_then(|e| e.as_array()) {
        for e in arr {
            let (Some(stored), Some(orig)) = (
                e.get("stored").and_then(|x| x.as_str()),
                e.get("orig").and_then(|x| x.as_str()),
            ) else {
                continue;
            };
            let at = e.get("deleted_at").and_then(|x| x.as_i64()).unwrap_or(0);
            out.insert(stored.to_lowercase(), (orig.to_string(), at));
        }
    }
    out
}

struct Library {
    dir: PathBuf,
    catalog: PathBuf,
    cache: PathBuf,
    recycle: PathBuf,
    on_drive: bool,
}

/// Can we create files directly in `dir`? Decides on-drive vs app-data fallback.
fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".foxcull_write_test.tmp");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Filesystem-safe id for a drive root, for the app-data fallback dir name.
fn drive_id(root: &Path) -> String {
    let id: String = root
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let id = id.trim_matches('_').to_string();
    if id.is_empty() {
        "root".into()
    } else {
        id
    }
}

/// Resolve the library for a drive: on the drive itself (`<drive>/_FoxCull`) when
/// writable, else a per-drive folder under app-data (read-only mounts — e.g. NTFS
/// on a Mac without Paragon — so rating/culling still works there).
fn resolve_library(data_root: &Path, root: &Path) -> Library {
    let (dir, on_drive) = if is_writable(root) {
        (root.join(LIB_DIRNAME), true)
    } else {
        (data_root.join("libraries").join(drive_id(root)), false)
    };
    // The Trash sits beside the user's folders, not inside our hidden library —
    // that is the whole point of the move. A read-only mount can't host it, so
    // that case keeps the old app-data location.
    let recycle = if on_drive {
        root.join(TRASH_DIRNAME)
    } else {
        dir.join("recycle")
    };
    Library {
        catalog: dir.join("catalog.sqlite"),
        cache: dir.join("thumbs"),
        recycle,
        dir,
        on_drive,
    }
}

/// One-time move of an existing hidden `_FoxCull\recycle` into the visible
/// `<drive>\FoxCull Trash`, flattening the mirrored folder tree.
///
/// Same volume, so each file is a metadata-only rename — an 18 GB clip moves
/// instantly. Catalog rows are re-keyed to the new flat `stored` name, and files
/// the catalog never knew about are adopted on the way (that is how the owner's
/// orphans get their record back). Nothing is deleted, ever.
fn migrate_recycle(old: &Path, new: &Path, catalog: &Catalog) {
    if !old.is_dir() || old == new {
        return;
    }
    let mut found: Vec<(PathBuf, i64, u64)> = Vec::new();
    collect(old, true, &mut found);
    if found.is_empty() {
        let _ = std::fs::remove_dir_all(old);
        return;
    }
    if std::fs::create_dir_all(new).is_err() {
        return;
    }
    let known: HashMap<String, crate::catalog::TrashRow> = catalog
        .list_trash()
        .into_iter()
        .map(|r| (r.stored.to_lowercase(), r))
        .collect();
    let mut rows: Vec<(String, String, String, i64)> = Vec::new();
    let mut drop_old: Vec<String> = Vec::new();
    for (path, mtime, _) in found {
        let rel = rel_under(old, &path);
        let name = rel_name(&rel).to_string();
        let target = uniquify(new.join(&name));
        if std::fs::rename(&path, &target).is_err() {
            continue;
        }
        let stored = rel_under(new, &target);
        let prev = known.get(&rel.to_lowercase());
        // An orphan has no row to inherit from; its position under the old
        // mirrored tree IS where it came from, which is exactly why the old
        // layout could be reconstructed and the new flat one needs the sidecar.
        let orig = prev.map(|r| r.orig.clone()).unwrap_or_else(|| rel.clone());
        let at = prev.map(|r| r.deleted_at).unwrap_or(mtime);
        rows.push((stored, orig, name, at));
        drop_old.push(rel);
    }
    if !rows.is_empty() {
        let _ = catalog.add_trash_many(&rows);
        catalog.remove_trash(&drop_old);
        crate::log::line(&format!(
            "TRASH migrated {} file(s) from {} to {}",
            rows.len(),
            old.display(),
            new.display()
        ));
    }
    write_trash_index(new, catalog);
    let _ = std::fs::remove_dir_all(old);
}

// ── background-activity reporting (the job centre, bottom of the sidebar) ────
//
// Every long-running backend job emits `activity` events the frontend folds
// into one progress card + expandable list, so "why is the disk busy / what is
// still loading / when will it finish" is always answerable at a glance.
// `total == 0` means indeterminate (a spinner, no percentage).

#[derive(Clone, Serialize, Default)]
pub struct Activity {
    pub id: String,
    pub label: String,
    pub done: u64,
    pub total: u64,
    /// "running" | "done" | "error" | "cancelled"
    pub state: String,
    /// Second line under the label ("file 3 of 24 · IMG_2041.CR2").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// What `done`/`total` count: "bytes" makes the job centre show sizes and
    /// a transfer speed. Absent = items (or a percentage when total is 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
    /// The job can be stopped with `cancel_job(id)`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cancellable: bool,
    /// Paused by the owner (a merge): no progress until resumed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
    /// The file a finished job made ("Show in folder").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn emit_activity(app: &AppHandle, id: &str, label: &str, done: u64, total: u64, state: &str) {
    emit_job(
        app,
        Activity {
            id: id.to_string(),
            label: label.to_string(),
            done,
            total,
            state: state.to_string(),
            ..Default::default()
        },
    );
}

fn emit_job(app: &AppHandle, a: Activity) {
    let _ = app.emit("activity", a);
}

// Stop buttons. A job that can be stopped registers a flag under its activity
// id; `cancel_job` sets it and the job's loop notices at its next chunk. Kept
// apart from `export_gen` on purpose: a merge running in the background must
// not be killed because an Edit export started (they used to share it).
static JOB_CANCELS: std::sync::LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Register a cancellable job and return its flag. Re-registering an id that
/// is still running hands back a fresh flag (the old run is finished by then).
fn job_token(id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    JOB_CANCELS.lock().insert(id.to_string(), flag.clone());
    flag
}

fn job_finished(id: &str, flag: &Arc<AtomicBool>) {
    let mut map = JOB_CANCELS.lock();
    if map.get(id).is_some_and(|f| Arc::ptr_eq(f, flag)) {
        map.remove(id);
    }
}

/// Stop a running job (a move, a merge). Returns false when nothing by that id
/// is running any more.
#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, id: String) -> bool {
    // Edit exports predate the registry and stop through their generation.
    if id == "edit-export" {
        state.export_gen.fetch_add(1, Ordering::SeqCst);
        return true;
    }
    let found = match JOB_CANCELS.lock().get(&id) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    };
    // A paused merge's ffmpeg writes nothing, so its watcher would never get
    // to see the stop flag: let it run again to be stopped.
    if id == "merge" {
        crate::procs::set_merge_paused(false);
    }
    found
}

/// Seconds since the Unix epoch.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// If `path` is taken, append " (2)", " (3)", … before the extension — used so a
/// recycle move or a restore never clobbers an existing file.
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

#[derive(Serialize)]
pub struct TreeDir {
    pub name: String,
    pub path: String,
    pub has_children: bool,
}

#[derive(Serialize)]
pub struct MediaItem {
    pub name: String,
    pub path: String,
    pub rel: String,
    pub kind: String,
    pub ext: String,
    pub mtime: i64, // file modified time (epoch secs) — for date sorting
    pub size: u64,  // file size in bytes — for the Details view + size sorting
    pub rating: i64,
    pub label: Option<String>,
    pub flag: Option<String>,
    pub tags: Vec<String>,
    /// Events this file belongs to, in join order — `[0]` is its primary event
    /// (the one the grid groups it under when several apply).
    pub events: Vec<String>,
    /// True for a catalog entry whose file was not on disk at the last scan.
    /// The row still carries every mark; the grid draws it as a "?" placeholder
    /// so the metadata can be relinked instead of silently lost.
    pub missing: bool,
    /// A video's marked in/out ranges (subclips, else its trim). Empty = the
    /// whole clip. Carried to the Edit window when the clip is dragged there.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<VideoSegment>,
}

#[derive(Serialize)]
pub struct EditSourceItem {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub ext: String,
    pub mtime: i64,
    pub size: u64,
}

#[derive(Serialize)]
pub struct MediaProbe {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: Option<String>,
    pub camera: Option<String>,
    pub captured: Option<i64>,
    /// True when the clip carries an HDR transfer (PQ/smpte2084 or HLG/arib-std-b67)
    /// — the signal the Instagram export uses to tone-map down to SDR.
    pub hdr: bool,
}

#[derive(Serialize)]
pub struct TrashOutcome {
    pub deleted: usize,
    pub failed: Vec<String>,
    pub errors: Vec<String>,
    /// Trash keys (`stored`) of the files that landed in the in-app Trash, in
    /// dispose order — the frontend keeps these so an Undo can restore exactly
    /// this batch. Empty in OS-recycle-bin mode, where we hold no handle on
    /// what the shell took.
    pub trashed: Vec<String>,
}

/// Path relative to the active library root, using `/` separators so the same
/// catalog key is produced on Windows and macOS.
fn rel_of(root: &Option<PathBuf>, abs: &str) -> String {
    if let Some(r) = root {
        if let Ok(rel) = Path::new(abs).strip_prefix(r) {
            return rel.to_string_lossy().replace('\\', "/");
        }
    }
    abs.replace('\\', "/")
}

fn has_unsafe_components(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

fn canonical_existing(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    std::fs::canonicalize(path).map_err(|e| format!("invalid path: {e}"))
}

fn canonical_dir(path: &Path) -> Result<PathBuf, String> {
    let p = canonical_existing(path)?;
    if !p.is_dir() {
        return Err("not a directory".into());
    }
    Ok(p)
}

fn canonical_file(path: &Path) -> Result<PathBuf, String> {
    let p = canonical_existing(path)?;
    if !p.is_file() {
        return Err("not a file".into());
    }
    Ok(p)
}

fn within(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn canonical_active_root(root: &Option<PathBuf>) -> Result<PathBuf, String> {
    let root = root.as_ref().ok_or("open a folder first")?;
    canonical_dir(root)
}

fn canonical_lib_dir(lib_dir: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(lib_dir).ok()
}

fn validate_active_media_file(
    active_root: &Path,
    lib_dir: Option<&PathBuf>,
    path: &str,
) -> Result<PathBuf, String> {
    let src = canonical_file(Path::new(path))?;
    if !within(&src, active_root) {
        return Err("file is outside the active library".into());
    }
    if let Some(lib) = lib_dir {
        if within(&src, lib) {
            return Err("refusing to operate inside the app library folder".into());
        }
    }
    if !media::is_media(&src) {
        return Err("not a supported media file".into());
    }
    Ok(src)
}

fn validate_active_dir(
    active_root: &Path,
    lib_dir: Option<&PathBuf>,
    path: &str,
) -> Result<PathBuf, String> {
    let dir = canonical_dir(Path::new(path))?;
    if !within(&dir, active_root) {
        return Err("folder is outside the active library".into());
    }
    if let Some(lib) = lib_dir {
        if within(&dir, lib) {
            return Err("refusing to use the app library folder as a destination".into());
        }
    }
    Ok(dir)
}

/// A media file wherever it lives, for the Edit and Merge windows. Their clips
/// can come from a drive other than the one the library has open right now:
/// the owner switches drives while the merge window is up, or puts SSD and
/// phone clips on one timeline. Never a file inside FoxCull's own data.
fn validate_media_anywhere(state: &AppState, path: &str) -> Result<PathBuf, String> {
    let p = canonical_file(Path::new(path))?;
    if !media::is_media(&p) {
        return Err("not a supported media file".into());
    }
    let in_library = p
        .components()
        .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(LIB_DIRNAME));
    if in_library || within(&p, &state.data_root) {
        return Err("refusing to use a file inside FoxCull's library folder".into());
    }
    Ok(p)
}

fn is_audio_file(path: &Path) -> bool {
    matches!(
        media::ext_lower(path).as_str(),
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg"
    )
}

fn is_edit_source_file(path: &Path) -> bool {
    matches!(media::classify(path), Kind::Video) || is_audio_file(path)
}

fn collect_edit_sources(dir: &Path, recursive: bool, out: &mut Vec<(PathBuf, i64, u64)>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let ft = match entry.file_type() {
            Ok(f) => f,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let dname = entry.file_name().to_string_lossy().to_string();
            if !recursive
                || dname.starts_with('.')
                || dname.to_ascii_lowercase().starts_with("_foxcull")
            {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                const REPARSE: u32 = 0x400;
                if let Ok(md) = entry.metadata() {
                    if md.file_attributes() & REPARSE != 0 {
                        continue;
                    }
                }
            }
            collect_edit_sources(&entry.path(), true, out);
        } else if ft.is_file() {
            // Skip dotfiles — chiefly macOS AppleDouble sidecars ("._IMG.JPG")
            // written to exFAT/NTFS drives, which carry a media extension but
            // are resource-fork junk, not media.
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if is_edit_source_file(&path) {
                let md = entry.metadata().ok();
                let mtime = md
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
                out.push((path, mtime, size));
            }
        }
    }
}

fn rel_under(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn safe_recycle_child(recycle: &Path, stored: &str) -> Result<PathBuf, String> {
    let rel = Path::new(stored);
    if rel.is_absolute() || has_unsafe_components(rel) {
        return Err("invalid trash entry".into());
    }
    let root = canonical_dir(recycle)?;
    let path = root.join(rel);
    if path.exists() {
        let canon = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
        if !within(&canon, &root) {
            return Err("trash entry escapes the recycle folder".into());
        }
        Ok(canon)
    } else {
        Ok(path)
    }
}

fn restore_target(drive: &Path, orig: &str) -> Result<PathBuf, String> {
    let rel = Path::new(orig);
    if rel.is_absolute() || has_unsafe_components(rel) {
        return Err("invalid restore target".into());
    }
    let root = canonical_dir(drive)?;
    let target = root.join(rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let canon_parent = canonical_dir(parent)?;
        if !within(&canon_parent, &root) {
            return Err("restore target escapes the active drive".into());
        }
    }
    Ok(uniquify(target))
}

#[derive(Serialize)]
pub struct LibraryInfo {
    /// The drive/volume root — the library root that catalog keys are relative to.
    pub root: String,
    /// The active library folder (`<drive>/_FoxCull` or app-data fallback).
    pub dir: String,
    pub catalog: String,
    pub recycle: String,
    /// True when the library lives on the drive itself; false = app-data fallback
    /// (the drive root wasn't writable, e.g. a read-only mount or `C:\`).
    pub on_drive: bool,
    /// Whether the opened drive root is writable (proxy for "can delete here").
    pub writable: bool,
}

/// Activate the library for the drive that `root` lives on, switching the catalog,
/// thumbnail cache and recycle folder to that drive's bundled `_FoxCull` folder
/// (auto per-drive). Idempotent — re-activating the same drive is a no-op. Adds
/// the drive + cache + recycle to the asset-protocol scope so originals and
/// cached previews can be served to the webview.
///
/// Migration is **data-loss-safe**: a fresh per-drive catalog is seeded by COPYING
/// (never moving) the current catalog or adopting a legacy `<drive>/fox-cull.catalog`,
/// and the legacy source is only removed AFTER the new catalog is open — so a
/// disconnect mid-operation can never lose ratings.
#[tauri::command]
pub fn set_library_root(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    root: String,
) -> Result<LibraryInfo, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err(format!("not a directory: {root}"));
    }
    let drive = drive_root(&root);
    let lib = resolve_library(&state.data_root, &drive);

    let current = state.catalog_path.lock().clone();
    if current != lib.catalog {
        std::fs::create_dir_all(&lib.dir).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&lib.recycle).ok();

        // Seed/adopt a fresh per-drive catalog without ever destroying the source.
        let mut legacy_to_remove: Option<PathBuf> = None;
        if !lib.catalog.exists() {
            let legacy = drive.join("fox-cull.catalog");
            if lib.on_drive && legacy.is_file() {
                catalog.checkpoint(); // fold WAL into the file before copying
                if std::fs::copy(&legacy, &lib.catalog).is_ok() {
                    legacy_to_remove = Some(legacy);
                    // Adopt the legacy `<drive>/thumbs` cache too, if present.
                    let legacy_thumbs = drive.join("thumbs");
                    if legacy_thumbs.is_dir() && !lib.cache.exists() {
                        let _ = std::fs::rename(&legacy_thumbs, &lib.cache);
                    }
                }
            }
            // NOTHING seeds a new drive's catalog from ANOTHER drive's catalog.
            //
            // This used to fall through to `copy(current -> lib.catalog)`, which
            // meant the first visit to a new drive cloned whatever drive you had
            // open before. Catalog keys are paths RELATIVE TO THE DRIVE ROOT, so
            // every cloned row then described a file that does not exist here —
            // and the integrity scan dutifully reported them as missing.
            //
            // That is exactly what happened on this machine: F:'s single trim row
            // for `Movies _ Final Exports_bak/Chaos Day 1 Drone shots.mp4` was
            // cloned onto D: and E:, so both drives permanently claimed "1 file
            // could not be found" for a file that had never been on them. All
            // three also carried an identical 7,254-row capture cache, which is
            // the same clone showing through.
            //
            // A drive that FoxCull has not seen before starts empty. The only
            // legitimate adoption is the same drive's own legacy catalog, above.
        }
        std::fs::create_dir_all(&lib.cache).ok();

        catalog
            .reopen(&lib.catalog)
            .map_err(|e| format!("failed to open catalog: {e}"))?;
        *state.catalog_path.lock() = lib.catalog.clone();
        *state.cache_dir.lock() = lib.cache.clone();
        *state.lib_dir.lock() = lib.dir.clone();
        *state.recycle_dir.lock() = lib.recycle.clone();
        // Relocate a pre-existing hidden recycle folder into the visible Trash.
        // Same volume, so even an 18 GB clip is an instant rename.
        migrate_recycle(&lib.dir.join("recycle"), &lib.recycle, &catalog);
        let _ = app.asset_protocol_scope().allow_directory(&lib.cache, true);
        let _ = app.asset_protocol_scope().allow_directory(&lib.recycle, true);

        // The new catalog is open (old connection closed) → the legacy file is
        // unlocked and safe to remove. Clear any stale config override too.
        if let Some(legacy) = legacy_to_remove {
            let ls = legacy.to_string_lossy().to_string();
            let _ = std::fs::remove_file(&legacy);
            let _ = std::fs::remove_file(format!("{ls}-wal"));
            let _ = std::fs::remove_file(format!("{ls}-shm"));
            let mut cfg = crate::config::load(&state.data_root);
            if cfg.catalog_path.is_some() {
                cfg.catalog_path = None;
                crate::config::save(&state.data_root, &cfg);
            }
        }
    }

    *state.root.lock() = Some(drive.clone());
    let _ = app.asset_protocol_scope().allow_directory(&drive, true);

    Ok(LibraryInfo {
        root: drive.to_string_lossy().to_string(),
        dir: lib.dir.to_string_lossy().to_string(),
        catalog: lib.catalog.to_string_lossy().to_string(),
        recycle: lib.recycle.to_string_lossy().to_string(),
        on_drive: lib.on_drive,
        writable: is_writable(&drive),
    })
}

/// Immediate subdirectories of `dir` (for the lazy folder tree). Dotfolders and
/// internal FoxCull library/cache folders are hidden.
///
/// `has_children` is reported **optimistically** (always true): probing it
/// eagerly meant an extra `read_dir` PER child, an N+1 stat storm that made
/// expanding a folder on the USB SSD take seconds. We instead show the expand
/// chevron for every folder and let the UI hide it the moment an expand turns up
/// no subfolders — one cheap `read_dir` per expand instead of one per sibling.
#[tauri::command]
pub fn list_tree(dir: String) -> Result<Vec<TreeDir>, String> {
    let p = Path::new(&dir);
    let read = std::fs::read_dir(p).map_err(|e| format!("read_dir failed: {e}"))?;
    let mut out: Vec<TreeDir> = read
        .filter_map(|e| e.ok())
        // file_type() is free on Windows (cached from the enumeration) — no stat.
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Same rule the media walk uses (see `skip_dir`). The Trash folder
            // is listed separately: it has its own pinned entry at the foot of
            // the sidebar, so it isn't mixed in among the drive's folders.
            if skip_dir(p, &name) || is_trash_dirname(&name) {
                return None;
            }
            Some(TreeDir {
                name,
                path: e.path().to_string_lossy().to_string(),
                has_children: true,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// The user's scan exclusions (Settings → Excluded folders), pushed from the
/// frontend whenever they change and persisted there. The built-in groups
/// default ON so a walk that starts before the frontend has pushed anything is
/// still safe — the unsafe direction is the one that costs minutes.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanExcludes {
    pub windows_system: bool,
    pub macos_system: bool,
    pub app_data: bool,
    pub developer: bool,
    pub games: bool,
    /// Specific folders (absolute paths). The folder and everything inside it.
    pub paths: Vec<String>,
    /// Folder names skipped wherever they appear; `*` matches any run of
    /// characters. Case-insensitive.
    pub names: Vec<String>,
}

impl Default for ScanExcludes {
    fn default() -> Self {
        Self {
            windows_system: true,
            macos_system: true,
            app_data: true,
            developer: true,
            games: true,
            paths: Vec::new(),
            names: Vec::new(),
        }
    }
}

impl ScanExcludes {
    /// Paths/names pre-normalised once here, not per directory in the walk.
    fn normalised(mut self) -> Self {
        self.paths = self.paths.iter().map(|p| norm_path(p)).filter(|p| !p.is_empty()).collect();
        self.names = self
            .names
            .iter()
            .map(|n| n.trim().to_ascii_lowercase())
            .filter(|n| !n.is_empty())
            .collect();
        self
    }
}

fn scan_excludes() -> &'static parking_lot::RwLock<ScanExcludes> {
    static EX: std::sync::OnceLock<parking_lot::RwLock<ScanExcludes>> = std::sync::OnceLock::new();
    EX.get_or_init(|| parking_lot::RwLock::new(ScanExcludes::default()))
}

#[tauri::command]
pub fn set_scan_excludes(excludes: ScanExcludes) {
    *scan_excludes().write() = excludes.normalised();
}

/// Case-folded, separator-trimmed form used to compare exclude paths. Both
/// Windows and macOS filesystems are case-insensitive by default.
fn norm_path(p: &str) -> String {
    let mut s = p.trim().to_lowercase();
    if cfg!(windows) {
        s = s.replace('/', "\\");
    }
    while s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) && !s.ends_with(":\\") {
        s.pop();
    }
    s
}

/// `*` wildcard match, nothing else special — the whole syntax an exclude
/// rule needs, and small enough to explain in one line of the UI.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(at) = rest.find(part) {
            rest = &rest[at + part.len()..];
        } else {
            return false;
        }
    }
    true
}

/// The one rule every folder walk uses — the tree, the media scan, the folder
/// badges and the relink pass. One function so they cannot drift: a folder the
/// tree offers but the scanner skips reads as a broken scan, and badges that
/// count what the scan skips promise photos that never appear.
fn skip_dir(parent: &Path, name: &str) -> bool {
    skip_dir_with(parent, name, &scan_excludes().read())
}

/// Why this is not one flat name list: the names that matter are only
/// unambiguous in a particular PLACE. `C:\Windows` and `/System` are the OS;
/// `D:\Shoots\Architecture\Windows` and `/Volumes/SSD/Library` are someone's
/// photos. So the built-in rules come in three shapes:
///
///   * machine-owned names, skipped wherever they appear (`node_modules`,
///     `AppData`, game libraries, bundles like `Foo.app`);
///   * OS folders, skipped only directly under a drive root (Windows) or the
///     boot volume root (macOS);
///   * the per-user app folders, skipped only directly inside a home folder
///     (`~/Library`).
///
/// Opening the boot drive on a Mac used to walk 241,138 files in 71 s, almost
/// all of them icons and UI assets inside `/System`, `/Library` and `.app`
/// bundles, and left a 234 MB cache behind for nothing.
fn skip_dir_with(parent: &Path, name: &str, ex: &ScanExcludes) -> bool {
    let lower = name.to_ascii_lowercase();
    // Always, whatever the settings say: hidden folders, FoxCull's own library,
    // OS bookkeeping nobody can open anyway, and the boot volume's mount points.
    // `/Volumes` holds every other drive (each already has its own tree entry)
    // and `/System/Volumes/Data` is the user's data a SECOND time, so walking
    // either turns one drive into all of them.
    if name.starts_with('.')
        || lower.starts_with("_foxcull")
        || matches!(lower.as_str(), "$recycle.bin" | "system volume information")
        || (lower == "volumes" && (is_boot_root(parent) || parent == Path::new("/System")))
    {
        return true;
    }
    (ex.windows_system && (is_windows_setup_dir(&lower) || (is_drive_root(parent) && is_windows_root_dir(&lower))))
        || (ex.macos_system && is_boot_root(parent) && is_macos_root_dir(&lower))
        || (ex.app_data
            && (is_app_data_dir(&lower) || is_macos_bundle_dir(&lower) || (is_home_dir(parent) && is_home_app_dir(&lower))))
        || (ex.developer && is_developer_dir(&lower))
        || (ex.games && is_game_dir(&lower))
        || ex.names.iter().any(|pat| wildcard_match(pat, &lower))
        || (!ex.paths.is_empty() && {
            let full = norm_path(&parent.join(name).to_string_lossy());
            ex.paths.iter().any(|p| *p == full)
        })
}

/// Leftovers of Windows setup, upgrades and resets. Always at a drive root in
/// practice, but the names are unambiguous anywhere.
fn is_windows_setup_dir(lower: &str) -> bool {
    matches!(
        lower,
        "windows.old" | "$windows.~bt" | "$windows.~ws" | "$sysreset" | "$winreagent" | "$getcurrent" | "config.msi" | "msocache"
    )
}

/// Per-user application data on Windows. (macOS's `~/Library` is anchored to
/// the home folder instead — see `is_home_app_dir`.)
fn is_app_data_dir(lower: &str) -> bool {
    matches!(lower, "appdata" | "windowsapps")
}

/// Developer tooling: hundreds of thousands of files, never a photo.
fn is_developer_dir(lower: &str) -> bool {
    matches!(lower, "node_modules" | "bower_components" | "__pycache__" | "site-packages" | "venv" | "deriveddata")
}

/// Game libraries: tens of thousands of textures that match the image filter
/// exactly.
fn is_game_dir(lower: &str) -> bool {
    matches!(lower, "steamapps" | "steamlibrary" | "xboxgames" | "epic games" | "riot games" | "gog games")
}

/// Windows OS folders, skipped only directly under a drive root (`C:\`, `D:\`).
fn is_windows_root_dir(lower: &str) -> bool {
    matches!(
        lower,
        "windows"
            | "program files"
            | "program files (x86)"
            | "programdata"
            | "recovery"
            | "perflogs"
            | "intel"
            | "amd"
            | "nvidia"
            | "drivers"
            | "onedrivetemp"
            | "inetpub"
    )
}

/// macOS OS folders, skipped only directly under the boot volume (`/`). An
/// external disk's `Library` or `Applications` folder is left alone — there is
/// no OS on it, so the folder is far more likely to be the user's.
fn is_macos_root_dir(lower: &str) -> bool {
    matches!(
        lower,
        "system"
            | "library"
            | "applications"
            | "private"
            | "usr"
            | "bin"
            | "sbin"
            | "opt"
            | "cores"
            | "dev"
            | "etc"
            | "var"
            | "tmp"
            | "network"
            | "developer"
    )
}

/// App folders inside a home folder: `~/Library` (caches, app support,
/// containers) and `~/Applications`. Windows' equivalent, `AppData`, matches
/// anywhere because the name is unambiguous.
fn is_home_app_dir(lower: &str) -> bool {
    matches!(lower, "library" | "applications")
}

/// `C:\` or `/` — a path with no parent.
fn is_drive_root(p: &Path) -> bool {
    p.parent().is_none()
}

/// The root of the volume the OS boots from. On macOS that is `/`, and also
/// `/Volumes/Macintosh HD`, which is a symlink straight back to `/` (the tree
/// used to list it as a second drive, and opening it walked the whole system).
fn is_boot_root(p: &Path) -> bool {
    if p == Path::new("/") {
        return true;
    }
    cfg!(target_os = "macos")
        && p.parent() == Some(Path::new("/Volumes"))
        && std::fs::read_link(p).map(|t| t == Path::new("/")).unwrap_or(false)
}

/// `C:\Users\<you>` or `/Users/<you>`.
fn is_home_dir(p: &Path) -> bool {
    p.parent()
        .and_then(|g| g.file_name())
        .map(|n| n.eq_ignore_ascii_case("users"))
        .unwrap_or(false)
}

/// macOS bundles are ordinary DIRECTORIES with a known suffix, so a plain
/// recursive walk descends into them and scrapes out every icon, toolbar image
/// and asset the developer shipped. That is the bulk of the junk seen when
/// opening a Mac disk: `Something.app/Contents/Resources` is full of `.png`
/// and `.tiff` files that match the media filter perfectly.
///
/// Matching on the suffix rather than a name list is what makes this work —
/// there is no finite set of app names to enumerate.
///
/// `.photoslibrary` is deliberately NOT here: that one genuinely holds the
/// user's own photos, and silently hiding it would lose real work.
fn is_macos_bundle_dir(lower: &str) -> bool {
    const BUNDLE_SUFFIXES: [&str; 9] = [
        ".app",
        ".framework",
        ".bundle",
        ".plugin",
        ".kext",
        ".xpc",
        ".appex",
        ".lproj",
        ".xcassets",
    ];
    BUNDLE_SUFFIXES.iter().any(|suf| lower.ends_with(suf))
}

/// Is `dir` the root of the drive the OS runs from — `C:\` (really
/// `%SystemDrive%`), `/`, or macOS's `/Volumes/Macintosh HD` alias? The app will
/// not silently reopen one of these at launch: it is the one folder whose scan
/// costs minutes, and landing in it by accident is how the 241k-file walk
/// happened. Other drive roots (an SSD, a camera card) reopen as normal.
#[tauri::command]
pub fn is_system_root(dir: String) -> bool {
    #[cfg(windows)]
    {
        let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let trim = |s: &str| s.trim_end_matches(['\\', '/']).to_string();
        trim(&dir).eq_ignore_ascii_case(&trim(&sys))
    }
    #[cfg(not(windows))]
    {
        is_boot_root(Path::new(&dir))
    }
}

#[derive(Serialize)]
pub struct SuggestedFolder {
    pub label: String,
    pub path: String,
    /// "pictures" | "videos" | "desktop" | "downloads" | "card" — picks the icon.
    pub kind: &'static str,
}

/// Starting points for the welcome screen: the OS's own media folders (resolved
/// through the platform's known-folder APIs, so a relocated or localised
/// Pictures folder is still found), plus any drive with a camera `DCIM` folder
/// at its root. Only folders that exist are returned.
#[tauri::command]
pub fn suggested_folders(app: AppHandle) -> Vec<SuggestedFolder> {
    let mut out: Vec<SuggestedFolder> = Vec::new();
    let paths = app.path();
    let videos = if cfg!(target_os = "macos") { "Movies" } else { "Videos" };
    for (label, kind, p) in [
        ("Pictures", "pictures", paths.picture_dir().ok()),
        (videos, "videos", paths.video_dir().ok()),
        ("Desktop", "desktop", paths.desktop_dir().ok()),
        ("Downloads", "downloads", paths.download_dir().ok()),
    ] {
        if let Some(p) = p.filter(|p| p.is_dir()) {
            out.push(SuggestedFolder {
                label: label.into(),
                path: p.to_string_lossy().to_string(),
                kind,
            });
        }
    }
    for d in list_drives() {
        if d.name == "Home" || is_system_root(d.path.clone()) {
            continue;
        }
        let dcim = Path::new(&d.path).join("DCIM");
        if dcim.is_dir() {
            out.push(SuggestedFolder {
                label: d.name.trim_end_matches(['\\', '/']).to_string(),
                path: dcim.to_string_lossy().to_string(),
                kind: "card",
            });
        }
    }
    out
}

#[cfg(test)]
mod scan_filter_tests {
    use super::{skip_dir_with, wildcard_match, ScanExcludes};
    use std::path::Path;

    fn skipped(parent: &str, name: &str) -> bool {
        skip_dir_with(Path::new(parent), name, &ScanExcludes::default())
    }

    fn none() -> ScanExcludes {
        ScanExcludes {
            windows_system: false,
            macos_system: false,
            app_data: false,
            developer: false,
            games: false,
            paths: vec![],
            names: vec![],
        }
    }

    #[test]
    fn skips_macos_system_dirs_at_boot_root() {
        for d in ["System", "Library", "Applications", "private", "usr", "opt", "Volumes"] {
            assert!(skipped("/", d), "/{d} should be skipped");
        }
    }

    #[test]
    fn skips_library_in_home_only() {
        assert!(skipped("/Users/alex", "Library"));
        assert!(skipped("/Users/alex", "Applications"));
        // A photo folder that happens to be called Library, on an external disk.
        assert!(!skipped("/Volumes/SSD", "Library"));
        assert!(!skipped("/Users/alex/Pictures", "Library"));
    }

    #[test]
    fn os_names_deeper_in_a_tree_are_kept() {
        // Photos OF windows, a shoot called "System", a PhotoRec-style "Recovery".
        for (parent, d) in [
            ("/Volumes/SSD/Architecture", "Windows"),
            ("/Users/alex/Pictures", "System"),
            ("/Volumes/SSD", "Recovery"),
            ("/Users/alex/Pictures", "Private"),
        ] {
            assert!(!skipped(parent, d), "{parent}/{d} must NOT be skipped");
        }
    }

    #[test]
    fn skips_machine_dirs_anywhere() {
        for d in ["node_modules", "AppData", "steamapps", "SteamLibrary", "XboxGames", "Windows.old"] {
            assert!(skipped("/Volumes/SSD/deep/folder", d), "{d} should be skipped");
        }
    }

    #[test]
    fn skips_app_bundles_by_suffix() {
        // The whole point: there is no finite list of app names.
        for d in ["Photos.app", "some random thing.app", "WebKit.framework", "x.bundle"] {
            assert!(skipped("/Users/alex/Downloads", d), "{d} should be skipped");
        }
    }

    #[test]
    fn keeps_real_photo_folders() {
        for d in [
            "Pictures",
            "DCIM",
            "My Library of shots", // contains "library" but is not it
            "holiday.app.photos",  // ends in .photos, not .app
            "2026-goa.photoslibrary", // the user's real photo library
        ] {
            assert!(!skipped("/", d), "{d} must NOT be skipped");
            assert!(!skipped("/Users/alex", d), "{d} must NOT be skipped");
        }
    }

    #[test]
    fn groups_switch_off_independently() {
        let mut ex = none();
        ex.games = true;
        assert!(skip_dir_with(Path::new("/x"), "steamapps", &ex));
        assert!(!skip_dir_with(Path::new("/"), "Library", &ex));
        assert!(!skip_dir_with(Path::new("/x"), "node_modules", &ex));
        assert!(!skip_dir_with(Path::new("/x"), "Photos.app", &ex));
    }

    #[test]
    fn internals_are_skipped_even_with_every_group_off() {
        let ex = none();
        let root = Path::new("/");
        assert!(skip_dir_with(root, ".Spotlight-V100", &ex));
        assert!(skip_dir_with(root, "_FoxCull", &ex));
        assert!(skip_dir_with(root, "Volumes", &ex));
        assert!(skip_dir_with(root, "$RECYCLE.BIN", &ex));
    }

    #[test]
    fn custom_names_and_paths() {
        let ex = ScanExcludes {
            names: vec!["proxy".into(), "*_cache".into()],
            paths: vec!["/Volumes/SSD/Old Exports/".into()],
            ..none()
        }
        .normalised();
        assert!(skip_dir_with(Path::new("/Volumes/SSD/shoot"), "Proxy", &ex));
        assert!(skip_dir_with(Path::new("/Volumes/SSD/shoot"), "render_cache", &ex));
        assert!(!skip_dir_with(Path::new("/Volumes/SSD/shoot"), "proxy shots", &ex));
        assert!(skip_dir_with(Path::new("/Volumes/SSD"), "old exports", &ex));
        assert!(!skip_dir_with(Path::new("/Volumes/SSD"), "Old Exports 2", &ex));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("raw*", "raw_backup"));
        assert!(wildcard_match("*backup", "raw_backup"));
        assert!(wildcard_match("a*c*e", "abcde"));
        assert!(!wildcard_match("a*c*e", "abcd"));
        assert!(!wildcard_match("raw", "raw_backup"));
    }

    #[cfg(windows)]
    #[test]
    fn skips_windows_system_dirs_at_drive_root_only() {
        for d in ["Windows", "Program Files", "Program Files (x86)", "ProgramData", "PerfLogs"] {
            assert!(skipped("C:\\", d), "C:\\{d} should be skipped");
            assert!(!skipped("D:\\Shoots", d), "D:\\Shoots\\{d} must NOT be skipped");
        }
    }
}

/// Recursively gather media file paths under `dir`. Uses `file_type()` (free on
/// Windows, no extra stat) and does NOT follow symlinks, so symlink loops can't
/// hang the walk. Hidden folders (dotfolders) are skipped.
///
/// `tick` is called at every directory with the running file count and returns
/// `true` to abandon the walk. It carries both jobs deliberately: a folder
/// switch must abandon an in-flight walk instead of racing it, and a drive-root
/// walk that runs for minutes must report progress or it looks like a hang.
fn collect_cancellable(
    dir: &Path,
    recursive: bool,
    out: &mut Vec<(PathBuf, i64, u64)>,
    tick: &dyn Fn(usize) -> bool,
) {
    if tick(out.len()) {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let ft = match entry.file_type() {
            Ok(f) => f,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let dname = entry.file_name().to_string_lossy().to_string();
            // Skip our own folders (the SSD cache + Trash), so cached
            // posters/thumbnails and discarded files never appear as photos to
            // cull, plus everything `skip_dir` rules out.
            if !recursive || is_trash_dirname(&dname) || skip_dir(dir, &dname) {
                continue;
            }
            // Skip Windows junctions / reparse points so browsing a whole drive
            // (e.g. C:\) can't loop forever or re-scan the same data.
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                const REPARSE: u32 = 0x400;
                if let Ok(md) = entry.metadata() {
                    if md.file_attributes() & REPARSE != 0 {
                        continue;
                    }
                }
            }
            collect_cancellable(&entry.path(), true, out, tick);
        } else if ft.is_file() {
            // Skip dotfiles (macOS "._*" AppleDouble sidecars on shared
            // exFAT/NTFS drives look like media by extension but aren't).
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if media::is_media(&path) {
                // metadata() is cached from the dir enumeration on Windows (free).
                let md = entry.metadata().ok();
                let mtime = md
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
                out.push((path, mtime, size));
            }
        }
    }
}

/// Uncancellable convenience wrapper for callers with a bounded walk.
fn collect(dir: &Path, recursive: bool, out: &mut Vec<(PathBuf, i64, u64)>) {
    collect_cancellable(dir, recursive, out, &|_| false);
}

/// Recursively count media files under `dir` (extension classification only — no
/// metadata reads), skipping the same folders as `collect` and Windows reparse
/// points. Powers the left-pane folder badges. It used to skip only dotfolders
/// and `_FoxCull`, so a drive's badge counted every icon in every app bundle.
fn count_media(dir: &Path) -> usize {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return 0,
    };
    let mut n = 0usize;
    for entry in rd.flatten() {
        let ft = match entry.file_type() {
            Ok(f) => f,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let dname = entry.file_name().to_string_lossy().to_string();
            if is_trash_dirname(&dname) || skip_dir(dir, &dname) {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                const REPARSE: u32 = 0x400;
                if let Ok(md) = entry.metadata() {
                    if md.file_attributes() & REPARSE != 0 {
                        continue;
                    }
                }
            }
            n += count_media(&entry.path());
        } else if ft.is_file()
            && !entry.file_name().to_string_lossy().starts_with('.')
            && media::is_media(&entry.path())
        {
            n += 1;
        }
    }
    n
}

#[derive(Serialize)]
pub struct FolderCount {
    pub path: String,
    pub count: i64,
}

/// Recursive media counts for a set of folders (the left-pane badges). Returns
/// cached counts instantly; missing ones are computed in parallel on the bounded
/// warm pool and cached, so the FIRST expand of a folder fills its children's
/// badges a moment later and every later run is instant. `recompute` ignores the
/// cache (the manual "↻ recount" path). Keyed by absolute path — a regenerable
/// local cache, never auto-invalidated, so stale counts only change on refresh.
#[tauri::command]
pub async fn folder_counts(
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    recompute: bool,
) -> Result<Vec<FolderCount>, String> {
    let cached = if recompute {
        HashMap::new()
    } else {
        catalog.get_counts()
    };
    let mut out: Vec<FolderCount> = Vec::with_capacity(paths.len());
    let mut need: Vec<String> = Vec::new();
    for p in paths {
        match cached.get(&p) {
            Some(c) => out.push(FolderCount { path: p, count: *c }),
            None => need.push(p),
        }
    }
    if !need.is_empty() {
        let computed: Vec<(String, i64)> = tauri::async_runtime::spawn_blocking(move || {
            warm_pool().install(|| {
                need.par_iter()
                    .map(|abs| (abs.clone(), count_media(Path::new(abs)) as i64))
                    .collect()
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        let _ = catalog.set_counts(&computed);
        for (path, count) in computed {
            out.push(FolderCount { path, count });
        }
    }
    Ok(out)
}

/// Drop every cached folder count so the badges recompute (the tree's ↻ button).
#[tauri::command]
pub fn clear_folder_counts(catalog: State<'_, Catalog>) {
    catalog.clear_counts();
}

/// Top-level browse roots: drive letters on Windows, mounted volumes + home on
/// macOS/Linux. Lets the left pane map the whole machine, not just one folder.
#[tauri::command]
pub fn list_drives() -> Vec<TreeDir> {
    let mut out: Vec<TreeDir> = Vec::new();
    #[cfg(windows)]
    {
        for c in b'A'..=b'Z' {
            let p = format!("{}:\\", c as char);
            if Path::new(&p).is_dir() {
                out.push(TreeDir {
                    name: format!("{}:\\", c as char),
                    path: p,
                    has_children: true,
                });
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            out.push(TreeDir {
                name: "Home".into(),
                path: home,
                has_children: true,
            });
        }
        let mut root_name = "/".to_string();
        let mut volumes: Vec<TreeDir> = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                // The boot volume shows up here as a symlink back to `/`. Listing
                // it too put the same disk in the tree twice; instead it lends
                // its name ("Macintosh HD") to the `/` entry.
                if is_boot_root(&e.path()) {
                    root_name = name;
                    continue;
                }
                if e.path().is_dir() {
                    volumes.push(TreeDir {
                        name,
                        path: e.path().to_string_lossy().to_string(),
                        has_children: true,
                    });
                }
            }
        }
        out.push(TreeDir {
            name: root_name,
            path: "/".into(),
            has_children: true,
        });
        out.extend(volumes);
    }
    out
}

/// All media under `dir` (optionally recursing into subfolders, Lightroom-style),
/// with stored culling decisions joined in via a single catalog query. Folders
/// are excluded — the tree handles navigation. This is the import path; it only
/// enumerates paths (no decode), so even a 10k-image year folder returns quickly.
/// **`async` is load-bearing, not decoration.** Tauri runs a *synchronous*
/// command on the main thread — the same thread that pumps the native window's
/// messages. This walk used to be synchronous, which was invisible on a normal
/// folder (390 ms for 8,403 files) and catastrophic on a drive root: opening
/// `D:\` sent it into `node_modules`, a shared cargo target dir and a Steam
/// library, and the window went "Not Responding" for minutes with the app
/// otherwise healthy (`foxcull.exe Responding=False` while every WebView2
/// process stayed responsive — that asymmetry is the fingerprint).
///
/// The walk now runs on a blocking worker and is cancelled by a folder switch,
/// so no folder can ever freeze the window again.
#[tauri::command]
pub async fn list_folder_media(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    dir: String,
    recursive: bool,
) -> Result<Vec<MediaItem>, String> {
    let p = Path::new(&dir);
    if !p.is_dir() {
        return Err(format!("not a directory: {dir}"));
    }
    // Cancel any in-flight warming for the folder we're leaving, so a rapid
    // folder switch can't leave two warm floods thrashing the disk at once.
    // The same generation doubles as this walk's cancellation token: the NEXT
    // folder open bumps it, and this walk notices and abandons itself.
    let my_gen = state.warm_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let root = state.root.lock().clone();

    let t0 = Instant::now();
    let walk_dir = p.to_path_buf();
    let gen_handle = state.warm_gen.clone();
    let scan_label = format!(
        "Scanning {}",
        p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.clone())
    );
    let app_for_walk = app.clone();
    let label_for_walk = scan_label.clone();
    let paths: Vec<(PathBuf, i64, u64)> = tauri::async_runtime::spawn_blocking(move || {
        let mut out: Vec<(PathBuf, i64, u64)> = Vec::new();
        // A drive-root walk can run for minutes. Report a live file count so the
        // activity chip proves the app is working rather than wedged — this is
        // the only feedback the user gets while the folder is still empty.
        // `Cell` because the walker takes a `Fn`, not a `FnMut`.
        let last_tick = std::cell::Cell::new(Instant::now());
        let announced = std::cell::Cell::new(false);
        collect_cancellable(&walk_dir, recursive, &mut out, &|found| {
            if gen_handle.load(Ordering::SeqCst) != my_gen {
                return true;
            }
            if last_tick.get().elapsed().as_millis() >= 400 {
                last_tick.set(Instant::now());
                announced.set(true);
                emit_activity(
                    &app_for_walk,
                    "scan-folder",
                    &format!("{label_for_walk} — {found} files"),
                    0,
                    0, // indeterminate: the total is unknowable until we finish
                    "running",
                );
            }
            false
        });
        if announced.get() {
            emit_activity(&app_for_walk, "scan-folder", &label_for_walk, 1, 1, "done");
        }
        out
    })
    .await
    .map_err(|e| e.to_string())?;
    // Abandoned mid-walk because the user moved on — return nothing rather than
    // a half-scanned folder the frontend would render as the truth. The frontend
    // discards it anyway (it guards on the folder still being current), but a
    // partial list must never leave this function looking authoritative.
    if state.warm_gen.load(Ordering::SeqCst) != my_gen {
        emit_activity(&app, "scan-folder", &scan_label, 1, 1, "done");
        crate::log::line(&format!("SCAN cancelled dir={dir:?} after {}ms", t0.elapsed().as_millis()));
        return Ok(Vec::new());
    }
    let walk_ms = t0.elapsed().as_millis();
    let file_count = paths.len();

    let mut items: Vec<MediaItem> = paths
        .into_iter()
        .map(|(path, mtime, size)| {
            let abs = path.to_string_lossy().to_string();
            MediaItem {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                rel: rel_of(&root, &abs),
                kind: media::classify(&path).as_str().to_string(),
                ext: media::ext_lower(&path),
                mtime,
                size,
                path: abs,
                rating: 0,
                label: None,
                flag: None,
                tags: Vec::new(),
                events: Vec::new(),
                missing: false,
                ranges: Vec::new(),
            }
        })
        .collect();

    // Sort by full path (groups each subfolder's shots together, ordered within).
    items.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));

    // One query for the whole subtree, then attach.
    let prefix = rel_of(&root, &dir);
    let decisions = catalog.get_under(&prefix);
    let map: HashMap<&str, _> = decisions.iter().map(|d| (d.rel.as_str(), d)).collect();
    for item in &mut items {
        if let Some(d) = map.get(item.rel.as_str()) {
            item.rating = d.rating;
            item.label = d.label.clone();
            item.flag = d.flag.clone();
        }
    }
    // Attach tags (separate many-to-many table) in one query for the subtree.
    let mut tagmap = catalog.tags_under(&prefix);
    for item in &mut items {
        if let Some(tags) = tagmap.remove(&item.rel) {
            item.tags = tags;
        }
    }
    // Attach event membership the same way — one query for the whole subtree.
    let mut evmap = catalog.events_under(&prefix);
    for item in &mut items {
        if let Some(events) = evmap.remove(&item.rel) {
            item.events = events;
        }
    }
    // And each video's marked in/out ranges.
    let mut rangemap = catalog.ranges_under(&prefix);
    for item in items.iter_mut().filter(|i| i.kind == "video") {
        if let Some(r) = rangemap.remove(&item.rel) {
            item.ranges = r;
        }
    }

    // Lightroom's "?" photos: catalog entries under this folder whose file is
    // gone. They are appended as placeholder items so their marks stay visible
    // and reachable (relink / forget) instead of vanishing with the file. Only
    // entries whose parent folder matches the current scope are shown, so a
    // non-recursive view doesn't inherit a whole subtree's ghosts.
    let missing_rels = catalog.missing_under(&prefix);
    if !missing_rels.is_empty() {
        let root_path = root.clone();
        let decision_map: HashMap<&str, _> = decisions.iter().map(|d| (d.rel.as_str(), d)).collect();
        let mut tagmap_missing = catalog.tags_under(&prefix);
        let mut evmap_missing = catalog.events_under(&prefix);
        for rel in missing_rels {
            let parent = rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            if !recursive && !parent.eq_ignore_ascii_case(&prefix) {
                continue;
            }
            let abs = match &root_path {
                Some(r) => r.join(&rel).to_string_lossy().to_string(),
                None => rel.clone(),
            };
            let as_path = PathBuf::from(&abs);
            let d = decision_map.get(rel.as_str());
            items.push(MediaItem {
                name: rel.rsplit('/').next().unwrap_or(&rel).to_string(),
                kind: media::classify(&as_path).as_str().to_string(),
                ext: media::ext_lower(&as_path),
                mtime: 0,
                size: 0,
                path: abs,
                rel: rel.clone(),
                rating: d.map(|d| d.rating).unwrap_or(0),
                label: d.and_then(|d| d.label.clone()),
                flag: d.and_then(|d| d.flag.clone()),
                tags: tagmap_missing.remove(&rel).unwrap_or_default(),
                events: evmap_missing.remove(&rel).unwrap_or_default(),
                missing: true,
                ranges: Vec::new(),
            });
        }
    }
    crate::log::line(&format!(
        "SCAN dir={:?} recursive={} files={} walk={}ms total={}ms",
        Path::new(&dir).file_name().unwrap_or_default(),
        recursive,
        file_count,
        walk_ms,
        t0.elapsed().as_millis()
    ));
    Ok(items)
}

/// Edit-mode source browser: videos plus audio tracks from the current folder.
/// This intentionally stays separate from Library mode so music files do not
/// become culling items.
/// `async` for the same reason as `list_folder_media`: a synchronous command
/// runs on the main thread, and this is an unbounded recursive walk.
#[tauri::command]
pub async fn list_edit_sources(dir: String, recursive: bool) -> Result<Vec<EditSourceItem>, String> {
    let p = Path::new(&dir);
    if !p.is_dir() {
        return Err(format!("not a directory: {dir}"));
    }
    let walk_dir = p.to_path_buf();
    let paths: Vec<(PathBuf, i64, u64)> = tauri::async_runtime::spawn_blocking(move || {
        let mut out: Vec<(PathBuf, i64, u64)> = Vec::new();
        collect_edit_sources(&walk_dir, recursive, &mut out);
        out
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut items: Vec<EditSourceItem> = paths
        .into_iter()
        .map(|(path, mtime, size)| {
            let kind = if is_audio_file(&path) { "audio" } else { "video" };
            EditSourceItem {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                path: path.to_string_lossy().to_string(),
                kind: kind.into(),
                ext: media::ext_lower(&path),
                mtime,
                size,
            }
        })
        .collect();
    items.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(items)
}

fn parse_duration_token(token: &str) -> Option<f64> {
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

fn parse_banner_duration(err: &str) -> f64 {
    err.find("Duration:")
        .and_then(|idx| {
            err[idx + "Duration:".len()..]
                .trim_start()
                .split(',')
                .next()
                .and_then(|s| parse_duration_token(s.trim()))
        })
        .unwrap_or(0.0)
}

fn parse_iso_token(s: &str) -> Option<i64> {
    let token = s.trim().split_whitespace().next()?;
    if token.len() < 19 {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> { token.get(a..z)?.parse().ok() };
    let y = num(0, 4)?;
    let mo = num(5, 7)?;
    let d = num(8, 10)?;
    let h = num(11, 13)?;
    let mi = num(14, 16)?;
    let se = num(17, 19)?;
    if y < 1970 || !(1..=12).contains(&mo) {
        return None;
    }
    Some(media::civil_to_unix(y, mo, d, h, mi, se))
}

fn parse_banner_creation(err: &str) -> Option<i64> {
    for line in err.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("creation_time") {
            if let Some((_, val)) = line.split_once(':') {
                if let Some(ts) = parse_iso_token(val) {
                    return Some(ts);
                }
            }
        }
    }
    None
}

fn parse_video_stream(err: &str) -> (u32, u32, f64, Option<String>, bool) {
    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 0.0f64;
    let mut codec = None;
    let mut hdr = false;
    for line in err.lines() {
        if !line.contains("Video:") {
            continue;
        }
        if let Some(after) = line.split("Video:").nth(1) {
            // HDR transfer characteristics show up in the stream's colour block,
            // e.g. `yuv420p10le(tv, bt2020nc/bt2020/arib-std-b67)`.
            let low = after.to_ascii_lowercase();
            hdr = low.contains("smpte2084") || low.contains("arib-std-b67");
            codec = after
                .trim()
                .split(|c: char| c == ',' || c.is_whitespace())
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_uppercase());
            for raw in after.split(|c: char| c == ',' || c.is_whitespace()) {
                let token = raw.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != 'x');
                if let Some((w, h)) = token.split_once('x') {
                    if let (Ok(ww), Ok(hh)) = (w.parse::<u32>(), h.parse::<u32>()) {
                        if ww >= 120 && hh >= 120 {
                            width = ww;
                            height = hh;
                            break;
                        }
                    }
                }
            }
            if let Some(idx) = after.find(" fps") {
                let before = after[..idx].trim_end();
                if let Some(tok) = before.split_whitespace().last() {
                    fps = tok.parse::<f64>().unwrap_or(0.0);
                }
            }
            break;
        }
    }
    (width, height, fps, codec, hdr)
}

/// Best-effort probe of a single clip's source dimensions, HDR flag, whether it
/// carries an audio stream, and its video codec (used at export time to decide
/// tone-mapping, soft-crop sharpening, multi-clip audio preservation, and
/// concat compatibility). Reads ffmpeg's `-i` banner; zeros/false/None on any
/// error. An audio stream shows up as `Audio:` on a `Stream` line of the banner.
/// Returns (width, height, hdr, has_audio, codec, pq). `pq` distinguishes the
/// HDR transfer function: true = PQ/HDR10 (smpte2084, e.g. S23 Ultra HDR10+),
/// false = HLG (arib-std-b67, e.g. Osmo Pocket 3) — the Keep-HDR export must
/// tag the output with the SOURCE's transfer or players decode the gamma wrong.
fn clip_probe(ffmpeg: &Path, src: &Path) -> (u32, u32, bool, bool, Option<String>, bool) {
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
    match cmd.output() {
        Ok(out) => {
            let banner = String::from_utf8_lossy(&out.stderr);
            let (w, h, _, codec, hdr) = parse_video_stream(&banner);
            let has_audio = banner
                .lines()
                .any(|l| l.contains("Stream") && l.contains("Audio:"));
            let pq = banner.to_ascii_lowercase().contains("smpte2084");
            (w, h, hdr, has_audio, codec, pq)
        }
        Err(_) => (0, 0, false, false, None, false),
    }
}

fn guess_camera(path: &Path, err: &str) -> Option<String> {
    for key in ["com.apple.quicktime.model", "model", "handler_name"] {
        if let Some(idx) = err.to_ascii_lowercase().find(key) {
            let after = &err[idx..];
            if let Some((_, val)) = after.split_once(':') {
                let cleaned = val.trim().lines().next().unwrap_or("").trim();
                if !cleaned.is_empty() && cleaned.len() <= 64 {
                    return Some(cleaned.to_string());
                }
            }
        }
    }
    let name = path.file_name()?.to_string_lossy().to_ascii_uppercase();
    if name.starts_with("DJI_") || name.starts_with("DJI-") {
        Some("DJI".into())
    } else if name.starts_with("VID_") || name.starts_with("PXL_") {
        Some("Phone".into())
    } else {
        None
    }
}

#[tauri::command]
pub async fn probe_media_info(
    state: State<'_, AppState>,
    path: String,
) -> Result<MediaProbe, String> {
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg not available")?;
    let src = canonical_file(Path::new(&path))?;
    if !matches!(media::classify(&src), Kind::Video) && !is_audio_file(&src) {
        return Err("not an editable media file".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new(ffmpeg);
        cmd.arg("-i")
            .arg(&src)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output().map_err(|e| e.to_string())?;
        let err = String::from_utf8_lossy(&out.stderr);
        let (width, height, fps, codec, hdr) = parse_video_stream(&err);
        Ok(MediaProbe {
            duration: parse_banner_duration(&err),
            width,
            height,
            fps,
            codec,
            camera: guess_camera(&src, &err),
            captured: parse_banner_creation(&err),
            hdr,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Frontend-side timing/diagnostic events, funneled into the same logfile.
#[tauri::command]
pub fn log_event(msg: String) {
    crate::log::line(&format!("UI {msg}"));
}

/// Cap for a single `read_file_range` request. The scrub engine reads MP4
/// metadata in ~2 MB chunks and individual video samples (a 4K60 HEVC keyframe
/// is single-digit MB); anything near this cap indicates a caller bug, not a
/// bigger need.
const READ_RANGE_MAX: u64 = 64 * 1024 * 1024;

/// Read `len` bytes at `offset` from a file, returned as a **raw binary** IPC
/// response (an ArrayBuffer on the JS side — no JSON array-of-numbers
/// overhead). This is the I/O primitive of the WebCodecs scrub engine (see
/// docs/design/video-player-migration.md): the frontend parses the MP4 sample
/// tables itself and fetches exactly the bytes of the samples it decodes.
/// A short (or empty) result means EOF was hit — that is the caller's EOF
/// signal, not an error.
#[tauri::command]
pub async fn read_file_range(
    path: String,
    offset: u64,
    len: u64,
) -> Result<tauri::ipc::Response, String> {
    use std::io::{Read, Seek, SeekFrom};
    if len > READ_RANGE_MAX {
        return Err(format!("read_file_range: len {len} exceeds {READ_RANGE_MAX}"));
    }
    let mut f = std::fs::File::open(&path).map_err(|e| format!("open {path}: {e}"))?;
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; len as usize];
    let mut filled = 0usize;
    while filled < buf.len() {
        match f.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) => return Err(e.to_string()),
        }
    }
    buf.truncate(filled);
    Ok(tauri::ipc::Response::new(buf))
}

/// Let the frontend write one line into the app log. Used for decisions that
/// are invisible from the Rust side but matter when diagnosing a machine we
/// can't sit in front of — above all whether the live scrub decoder accepted a
/// clip, and if not, why. Cheap enough to call once per clip opened.
#[tauri::command]
pub fn log_note(msg: String) {
    crate::log::line(&format!("UI {msg}"));
}

/// Dev-only: persist the WebCodecs feasibility-probe verdict somewhere the
/// agent driving the dev loop can read it (the webview console isn't visible
/// from the terminal that runs `tauri dev`). Also mirrored into the app log.
#[tauri::command]
pub fn scrub_probe_report(report: String) -> Result<String, String> {
    let path = std::env::temp_dir().join("foxcull-scrub-probe.json");
    std::fs::write(&path, &report).map_err(|e| e.to_string())?;
    crate::log::line(&format!("SCRUB-PROBE {report}"));
    Ok(path.to_string_lossy().into_owned())
}

/// Cached, orientation-corrected thumbnail for the grid/filmstrip. Returns a
/// filesystem path the frontend converts via `convertFileSrc`.
#[tauri::command]
pub async fn thumbnail(
    state: State<'_, AppState>,
    path: String,
    max: u32,
) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let kind = media::classify(&p);
    // Never decode videos/unknowns for a thumbnail (poster frames are phase 2);
    // the frontend renders a placeholder instead and shouldn't even call this.
    if matches!(kind, Kind::Video | Kind::Other) {
        return Err("no thumbnail for this kind".into());
    }
    let cache_dir = state.cache_dir.lock().clone();
    // Run the CPU-bound decode/resize on the blocking pool so concurrent
    // thumbnail requests genuinely parallelize across cores instead of
    // serializing on a runtime worker.
    tauri::async_runtime::spawn_blocking(move || {
        thumbs::ensure(&cache_dir, &p, kind, max).map(|o| o.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Source path for the large loupe/focus view. Images and RAW serve a generated,
/// orientation-baked, **capped** preview (long edge <= LOUPE_MAX): RAW because the
/// webview can't render `.NEF`, and ordinary images because handing the webview a
/// 50MP original makes it paint top-down over several seconds — the capped preview
/// decodes via the DCT fast path and appears at once. Videos serve the original.
#[tauri::command]
pub async fn loupe_src(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let kind = media::classify(&p);
    match kind {
        Kind::Raw | Kind::Image => {
            let cache_dir = state.cache_dir.lock().clone();
            tauri::async_runtime::spawn_blocking(move || {
                thumbs::ensure(&cache_dir, &p, kind, LOUPE_MAX)
                    .map(|o| o.to_string_lossy().to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        }
        _ => Ok(path),
    }
}

/// Cached poster frame for a video (grid/strip/loupe). Generated by the bundled
/// ffmpeg and cached beside the catalog (on the SSD), so it's made once and
/// reused across machines. Errors (no ffmpeg, read-only cache on a Mac, an
/// undecodable clip) leave the frontend showing the film placeholder.
#[tauri::command]
pub async fn video_poster(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let cache_dir = state.cache_dir.lock().clone();
    let ffmpeg = state.ffmpeg.clone();
    let src = PathBuf::from(&path);
    tauri::async_runtime::spawn_blocking(move || {
        video::ensure_poster(&cache_dir, ffmpeg.as_deref(), &src)
            .map(|o| o.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Sharp ~1280px poster for Focus/full-screen view (the grid keeps `video_poster`
/// at 480px). Generated lazily, only for clips actually opened in Focus.
#[tauri::command]
pub async fn video_poster_hires(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let cache_dir = state.cache_dir.lock().clone();
    let ffmpeg = state.ffmpeg.clone();
    let src = PathBuf::from(&path);
    tauri::async_runtime::spawn_blocking(move || {
        video::ensure_poster_hires(&cache_dir, ffmpeg.as_deref(), &src)
            .map(|o| o.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct FilmstripInfo {
    /// Filesystem path of the sprite JPEG (frontend converts via convertFileSrc).
    pub src: String,
    pub cols: u32,
    pub rows: u32,
    pub count: u32,
    pub tile_w: u32,
    pub tile_h: u32,
    pub duration: f64,
}

fn filmstrip_info(sprite: PathBuf, fs: video::Filmstrip) -> FilmstripInfo {
    FilmstripInfo {
        src: sprite.to_string_lossy().to_string(),
        cols: fs.cols,
        rows: fs.rows,
        count: fs.count,
        tile_w: fs.tile_w,
        tile_h: fs.tile_h,
        duration: fs.duration,
    }
}

// ── sprite build cancellation ────────────────────────────────────────────────
// Every filmstrip/scrubstrip request registers a cancel token keyed by
// `<kind>:<path>`. Hover-away calls `cancel_sprite`; a folder switch calls
// `cancel_all_sprites`; a NEW request for the same clip supersedes (cancels)
// the old one. The build polls its token between frame extractions, so a
// cancelled hover stops burning the disk within a frame or two — the fix for
// "sweep across a row of clips and the app chews for minutes".

fn sprite_tokens() -> &'static Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>> {
    static TOKENS: std::sync::OnceLock<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>> =
        std::sync::OnceLock::new();
    TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn new_sprite_token(kind: &str, path: &str) -> Arc<std::sync::atomic::AtomicBool> {
    let token = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut m = sprite_tokens().lock();
    if let Some(old) = m.insert(format!("{kind}:{path}"), token.clone()) {
        old.store(true, Ordering::SeqCst);
    }
    token
}

fn drop_sprite_token(kind: &str, path: &str, token: &Arc<std::sync::atomic::AtomicBool>) {
    let mut m = sprite_tokens().lock();
    let key = format!("{kind}:{path}");
    if m.get(&key).is_some_and(|cur| Arc::ptr_eq(cur, token)) {
        m.remove(&key);
    }
}

/// Cancel the in-flight/queued sprite build for one clip. `kind` is "film"
/// (Focus filmstrip) or "scrub" (grid hover strip).
#[tauri::command]
pub fn cancel_sprite(kind: String, path: String) {
    let k = if kind == "film" { "f" } else { "s" };
    if let Some(t) = sprite_tokens().lock().get(&format!("{k}:{path}")) {
        t.store(true, Ordering::SeqCst);
    }
}

/// Cancel every pending sprite build — called on folder switch so the old
/// folder's hover backlog can't sit on the disk while the new folder loads.
#[tauri::command]
pub fn cancel_all_sprites() {
    let mut m = sprite_tokens().lock();
    for t in m.values() {
        t.store(true, Ordering::SeqCst);
    }
    m.clear();
}

/// Build (or fetch the cached) filmstrip sprite for a video — a tiled grid of
/// frames the loupe shows under the scrub cursor for instant, decode-free
/// scrubbing. Generated lazily on first open; cached beside the poster on the
/// SSD. Errors (no ffmpeg, unreadable duration) leave the timeline as a plain
/// seek bar with no hover preview; a cancelled build surfaces as an error the
/// frontend silently ignores.
#[tauri::command]
pub async fn video_filmstrip(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<FilmstripInfo, String> {
    let cache_dir = state.cache_dir.lock().clone();
    let ffmpeg = state.ffmpeg.clone();
    let src = PathBuf::from(&path);
    let token = new_sprite_token("f", &path);
    tauri::async_runtime::spawn_blocking(move || {
        let cached = video::filmstrip_path(&cache_dir, &src).exists();
        let act_id = format!("strip:{}", src.to_string_lossy());
        if !cached {
            emit_activity(&app, &act_id, "Building scrub filmstrip", 0, 0, "running");
        }
        let cancel = {
            let t = token.clone();
            move || t.load(Ordering::SeqCst)
        };
        let progress = |done: u32, total: u32| {
            if done % 5 == 0 || done == total {
                emit_activity(
                    &app,
                    &act_id,
                    "Building scrub filmstrip",
                    done as u64,
                    total as u64,
                    "running",
                );
            }
        };
        let res = video::ensure_filmstrip(&cache_dir, ffmpeg.as_deref(), &src, &cancel, &progress);
        if !cached {
            emit_activity(&app, &act_id, "Building scrub filmstrip", 1, 1, "done");
        }
        drop_sprite_token("f", &src.to_string_lossy(), &token);
        res.map(|(sprite, fs)| filmstrip_info(sprite, fs))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lighter sprite for grid/source-thumbnail hover scrubbing. This is separate
/// from the denser Focus filmstrip so Live Scrub can stay responsive on older
/// laptops.
#[tauri::command]
pub async fn video_scrubstrip(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<FilmstripInfo, String> {
    let cache_dir = state.cache_dir.lock().clone();
    let ffmpeg = state.ffmpeg.clone();
    let src = PathBuf::from(&path);
    let token = new_sprite_token("s", &path);
    tauri::async_runtime::spawn_blocking(move || {
        let cached = video::scrubstrip_path(&cache_dir, &src).exists();
        let act_id = format!("scrub:{}", src.to_string_lossy());
        if !cached {
            emit_activity(&app, &act_id, "Building hover scrub", 0, 0, "running");
        }
        let cancel = {
            let t = token.clone();
            move || t.load(Ordering::SeqCst)
        };
        let progress = |done: u32, total: u32| {
            if done % 4 == 0 || done == total {
                emit_activity(&app, &act_id, "Building hover scrub", done as u64, total as u64, "running");
            }
        };
        let res = video::ensure_scrubstrip(&cache_dir, ffmpeg.as_deref(), &src, &cancel, &progress);
        if !cached {
            emit_activity(&app, &act_id, "Building hover scrub", 1, 1, "done");
        }
        drop_sprite_token("s", &src.to_string_lossy(), &token);
        res.map(|(sprite, fs)| filmstrip_info(sprite, fs))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The hover strip's geometry IF it's already cached — never builds. The Focus
/// view uses this as an instant coarse scrub layer while the dense filmstrip
/// renders in the background.
#[tauri::command]
pub fn video_scrubstrip_cached(
    state: State<'_, AppState>,
    path: String,
) -> Option<FilmstripInfo> {
    let cache_dir = state.cache_dir.lock().clone();
    video::sprite_cached(&cache_dir, Path::new(&path), false)
        .map(|(sprite, fs)| filmstrip_info(sprite, fs))
}

/// The DENSE Focus filmstrip's geometry IF it's already cached — never builds.
/// Focus view always calls this (a strip built in an earlier session shows for
/// free); the BUILDING `video_filmstrip` call is gated in the UI on the Live
/// Scrub setting + first scrub intent (see the 2026-07-20 RCA: unconditional
/// builds on Focus open were ~70s of unwanted work per clip on HDD libraries).
#[tauri::command]
pub fn video_filmstrip_cached(
    state: State<'_, AppState>,
    path: String,
) -> Option<FilmstripInfo> {
    let cache_dir = state.cache_dir.lock().clone();
    video::sprite_cached(&cache_dir, Path::new(&path), true)
        .map(|(sprite, fs)| filmstrip_info(sprite, fs))
}

/// How many items the background warmer pre-generates per folder. Bounded so a
/// huge folder can't keep the USB SSD's read queue saturated for a minute — past
/// this the viewport-prioritized on-demand loader handles whatever you scroll to.
const WARM_CAP: usize = 600;

/// Proactively generate (and disk-cache) grid thumbnails for the FIRST part of a
/// folder, so initial scrolling is smooth instead of decoding lazily under the
/// cursor. Fire-and-forget from the frontend right after a folder loads; cancels
/// itself when the user switches folders (the generation token moved on).
///
/// CRITICAL: by default this only warms ordinary images, and only the first
/// `WARM_CAP` of them. Videos (ffmpeg poster extraction) and RAW (whole-file
/// ~25 MB reads) are deliberately LEFT to on-demand loading — pre-reading a
/// whole folder of those pinned the USB SSD's serial command queue for ~a minute
/// and starved the foreground thumbnails you were actually looking at (the "not
/// responding that recovers if you wait" bug). On-demand stays bounded to the
/// viewport, so heavy reads only happen for the handful of RAW/video cells
/// actually on screen.
///
/// `heavy` opts in to RAW previews and video posters — used ONLY by the explicit
/// "Prepare folder" button, where the user has asked for the up-front disk work
/// (it runs in small chunks on the same bounded pool, so it still can't flood
/// the drive). Without it, preparing a RAW/video folder was silently a no-op.
#[tauri::command]
pub async fn warm_thumbnails(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
    max: u32,
    heavy: Option<bool>,
) -> Result<(), String> {
    let my_gen = state.warm_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let gen = state.warm_gen.clone();
    let cache_dir = state.cache_dir.lock().clone();
    let ffmpeg = state.ffmpeg.clone();
    let heavy = heavy.unwrap_or(false);
    // Only the cheap kind (ordinary images) unless heavy; only the first WARM_CAP.
    let work: Vec<(PathBuf, Kind)> = paths
        .iter()
        .map(PathBuf::from)
        .map(|p| { let k = media::classify(&p); (p, k) })
        .filter(|(_, k)| match k {
            Kind::Image => true,
            Kind::Raw | Kind::Video => heavy,
            Kind::Other => false,
        })
        .take(WARM_CAP)
        .collect();
    let total = work.len();
    let t0 = Instant::now();
    crate::log::line(&format!(
        "WARM start items={} (of {} files) max={} heavy={} gen={}",
        total,
        paths.len(),
        max,
        heavy,
        my_gen
    ));
    // Surface big warming passes in the activity indicator. Small batches (the
    // chunked "Prepare folder" calls report their own combined progress) stay
    // quiet to avoid a churn of micro-jobs.
    let announce = total >= 24;
    let act_id = format!("warm:{my_gen}");
    if announce {
        emit_activity(&app, &act_id, "Rendering thumbnails", 0, total as u64, "running");
    }
    let done = tauri::async_runtime::spawn_blocking(move || {
        let count = std::sync::atomic::AtomicUsize::new(0);
        // Run on the small dedicated pool so we never monopolize the cores the
        // foreground (loupe + visible cells) needs.
        warm_pool().install(|| {
            work.par_iter().for_each(|(p, kind)| {
                // Abandon the moment a newer folder selection supersedes us.
                if gen.load(Ordering::SeqCst) != my_gen {
                    return;
                }
                let ok = match kind {
                    // Videos get their grid poster; images and RAW get the
                    // requested-size preview (the RAW path extracts the embedded
                    // camera JPEG, same as loupe_src).
                    //
                    // Prepare NO LONGER pre-builds scrub sprites (2026-07-21).
                    // Both Focus and armed grid tiles now decode frames live, so
                    // a sprite built here would be extracted, written and cached
                    // for nobody — the sprite path survives only as the
                    // per-clip fallback for codecs the decoder rejects, and that
                    // builds on demand. This is the bulk of what Prepare used to
                    // spend its time on for video folders.
                    Kind::Video => video::ensure_poster(&cache_dir, ffmpeg.as_deref(), p).is_ok(),
                    kind => thumbs::ensure(&cache_dir, p, *kind, max).is_ok(),
                };
                if ok {
                    let n = count.fetch_add(1, Ordering::Relaxed) + 1;
                    if announce && n % 16 == 0 {
                        emit_activity(
                            &app,
                            &act_id,
                            "Rendering thumbnails",
                            n as u64,
                            total as u64,
                            "running",
                        );
                    }
                }
            });
        });
        if announce {
            let n = count.load(Ordering::Relaxed) as u64;
            emit_activity(&app, &act_id, "Rendering thumbnails", n, total as u64, "done");
        }
        count.load(Ordering::Relaxed)
    })
    .await
    .unwrap_or(0);
    crate::log::line(&format!(
        "WARM done warmed={}/{} elapsed={}ms gen={}{}",
        done,
        total,
        t0.elapsed().as_millis(),
        my_gen,
        if state.warm_gen.load(Ordering::SeqCst) != my_gen {
            " (cancelled)"
        } else {
            ""
        }
    ));
    Ok(())
}

/// Abandon any in-flight background warming (bump the generation token). Called
/// when the user enters Focus / starts a video, so previews and video playback
/// get the USB SSD's read bandwidth to themselves instead of stuttering behind
/// the warmer.
#[tauri::command]
pub fn cancel_warm(state: State<'_, AppState>) {
    state.warm_gen.fetch_add(1, Ordering::SeqCst);
}

#[derive(Serialize)]
pub struct CaptureDate {
    pub path: String,
    pub captured: i64,
}

/// Real capture timestamps for a set of files — EXIF DateTimeOriginal for
/// images/RAW, container `creation_time` for video, falling back to the file
/// mtime when neither is present. Results are cached in the catalog (validated
/// by mtime+size), so the FIRST call on a folder does the extraction (kept OFF
/// the folder-open path, which never reads EXIF) and every later call returns
/// instantly. Extraction runs on the bounded warm pool so it never floods the
/// USB SSD or starves the foreground.
#[tauri::command]
pub async fn capture_dates(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    dir: String,
    paths: Vec<String>,
) -> Result<Vec<CaptureDate>, String> {
    let root = state.root.lock().clone();
    let prefix = rel_of(&root, &dir);
    let cached = catalog.captures_under(&prefix);
    let ffmpeg = state.ffmpeg.clone();

    struct Pending {
        path: String,
        rel: String,
        mtime: i64,
        size: i64,
        kind: Kind,
    }
    let mut results: Vec<CaptureDate> = Vec::with_capacity(paths.len());
    let mut pending: Vec<Pending> = Vec::new();
    for path in &paths {
        let p = Path::new(path);
        let rel = rel_of(&root, path);
        let (mtime, size) = match std::fs::metadata(p) {
            Ok(m) => (
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                m.len() as i64,
            ),
            Err(_) => (0, 0),
        };
        if let Some(&(captured, cm, cs)) = cached.get(&rel) {
            if cm == mtime && cs == size {
                results.push(CaptureDate {
                    path: path.clone(),
                    captured,
                });
                continue;
            }
        }
        pending.push(Pending {
            path: path.clone(),
            rel,
            mtime,
            size,
            kind: media::classify(p),
        });
    }

    if !pending.is_empty() {
        // First pass on a folder reads EXIF / probes clips for every file —
        // exactly the kind of invisible disk work the activity chip is for.
        let announce = pending.len() >= 24;
        let act_id = format!("captures:{}", prefix);
        let act_total = pending.len() as u64;
        if announce {
            emit_activity(&app, &act_id, "Reading capture dates", 0, act_total, "running");
        }
        let extracted: Vec<(CaptureDate, (String, i64, i64, i64))> =
            tauri::async_runtime::spawn_blocking(move || {
                let counter = std::sync::atomic::AtomicUsize::new(0);
                let res = warm_pool().install(|| {
                    pending
                        .par_iter()
                        .map(|pd| {
                            let p = Path::new(&pd.path);
                            let captured = match pd.kind {
                                Kind::Image | Kind::Raw => media::capture_date(p),
                                Kind::Video => ffmpeg
                                    .as_deref()
                                    .and_then(|ff| video::creation_time(ff, p)),
                                Kind::Other => None,
                            }
                            .unwrap_or(pd.mtime);
                            let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                            if announce && n % 16 == 0 {
                                emit_activity(
                                    &app,
                                    &act_id,
                                    "Reading capture dates",
                                    n as u64,
                                    act_total,
                                    "running",
                                );
                            }
                            (
                                CaptureDate {
                                    path: pd.path.clone(),
                                    captured,
                                },
                                (pd.rel.clone(), captured, pd.mtime, pd.size),
                            )
                        })
                        .collect()
                });
                if announce {
                    emit_activity(&app, &act_id, "Reading capture dates", act_total, act_total, "done");
                }
                res
            })
            .await
            .map_err(|e| e.to_string())?;

        let rows: Vec<(String, i64, i64, i64)> =
            extracted.iter().map(|(_, r)| r.clone()).collect();
        let _ = catalog.set_capture_many(&rows);
        for (cd, _) in extracted {
            results.push(cd);
        }
    }

    Ok(results)
}

// ── Video lengths (grid duration badge) ────────────────────────────────────

#[derive(Serialize)]
pub struct VideoLength {
    pub path: String,
    pub duration: f64,
}

/// (mtime secs, size) — the cache validity stamp used by `captures` too.
fn file_stamp(p: &Path) -> (i64, i64) {
    match std::fs::metadata(p) {
        Ok(m) => (
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            m.len() as i64,
        ),
        Err(_) => (0, 0),
    }
}

/// ffmpeg's `-i` banner (stderr) for one file, or None if ffmpeg won't run.
fn ffmpeg_banner(ffmpeg: &Path, src: &Path) -> Option<String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-hide_banner")
        .arg("-i")
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
    Some(String::from_utf8_lossy(&out.stderr).into_owned())
}

/// Length of one clip: the MP4/MOV header when there is one (a few reads),
/// else ffmpeg's banner.
fn clip_length(ffmpeg: Option<&Path>, p: &Path) -> Option<f64> {
    video::mp4_duration(p).or_else(|| {
        let d = parse_banner_duration(&ffmpeg_banner(ffmpeg?, p)?);
        (d > 0.0).then_some(d)
    })
}

/// Lengths for the videos among `paths`, from the per-drive cache when the
/// file's (mtime, size) still match, else measured and cached. Measuring runs
/// on the bounded warm pool, so a grid full of SD-card clips never takes more
/// than its share of the card from the thumbnails.
#[tauri::command]
pub async fn video_durations(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    dir: String,
    paths: Vec<String>,
) -> Result<Vec<VideoLength>, String> {
    let root = state.root.lock().clone();
    let cached = catalog.durations_under(&rel_of(&root, &dir));
    let ffmpeg = state.ffmpeg.clone();
    let mut out: Vec<VideoLength> = Vec::with_capacity(paths.len());
    let mut pending: Vec<(String, String, i64, i64)> = Vec::new();
    for path in paths {
        let p = Path::new(&path);
        if !matches!(media::classify(p), Kind::Video) {
            continue;
        }
        let rel = rel_of(&root, &path);
        let (mtime, size) = file_stamp(p);
        if let Some(&(d, cm, cs)) = cached.get(&rel) {
            if cm == mtime && cs == size {
                out.push(VideoLength { path, duration: d });
                continue;
            }
        }
        pending.push((path, rel, mtime, size));
    }
    if !pending.is_empty() {
        let measured: Vec<(String, String, f64, i64, i64)> = tauri::async_runtime::spawn_blocking(move || {
            warm_pool().install(|| {
                pending
                    .par_iter()
                    .filter_map(|(path, rel, mtime, size)| {
                        let d = clip_length(ffmpeg.as_deref(), Path::new(path))?;
                        Some((path.clone(), rel.clone(), d, *mtime, *size))
                    })
                    .collect()
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        let rows: Vec<(String, f64, i64, i64)> =
            measured.iter().map(|(_, rel, d, m, s)| (rel.clone(), *d, *m, *s)).collect();
        let _ = catalog.set_duration_many(&rows);
        out.extend(measured.into_iter().map(|(path, _, duration, _, _)| VideoLength { path, duration }));
    }
    Ok(out)
}

// ── Merge videos end to end, losslessly ───────────────────────────────────
//
// The Osmo Pocket 3 workflow: a trip's worth of clips joined in shooting order
// into one file for YouTube, with no re-encode, so it keeps the camera's
// native ~70-110 Mbps HEVC and is as good as anything YouTube will ever be
// given. A stream-copy join is only valid when every clip shares codec,
// profile, pixel format, frame size, frame rate, rotation and audio format;
// the Osmo mixes 59.94 and 29.97 fps, 8- and 10-bit, landscape, vertical and
// square in one folder, so the probe below reports each clip's signature and
// the dialog keeps only the clips that match the main set.

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct MergeClip {
    pub path: String,
    pub name: String,
    /// "video", "photo" or "other". Everything selected is listed, photos
    /// included, so the owner sees what can't go in and why, rather than
    /// FoxCull silently leaving things out.
    pub kind: String,
    pub size: u64,
    pub duration: f64,
    /// Recording time (the container's creation_time), for chronological order.
    pub captured: Option<i64>,
    pub width: u32,
    pub height: u32,
    /// ffmpeg's AVERAGE rate. Phones and glasses record variable frame rate,
    /// so a 30 fps clip can read 29.73; compare `fps_class`, not this.
    pub fps: f64,
    /// The nominal rate the clip was shot at (see `fps_class`).
    pub fps_class: u32,
    pub rotation: i32,
    pub vcodec: String,
    pub profile: String,
    pub pix_fmt: String,
    /// Video stream bitrate in kb/s (0 when ffmpeg doesn't say).
    pub vbitrate: u32,
    /// "hlg", "pq" or "sdr": HDR and SDR can't share one file.
    pub color: String,
    pub acodec: Option<String>,
    pub arate: u32,
    pub alayout: String,
    /// Everything that must match for a stream-copy join, as one comparable string.
    pub signature: String,
    pub error: Option<String>,
}

/// The nominal frame rate a clip was shot at, from ffmpeg's average rate.
/// Phones and glasses record variable frame rate: frames are dropped in low
/// light, so a 30 fps clip averages 29.73 or 29.94, and NTSC 29.97 is a "30"
/// too. A stream copy carries every frame's own timestamp, so clips in one
/// class join cleanly (checked on Meta glasses clips of 29.73-30 fps,
/// 2026-09-29: every frame kept, no decode errors); only a different class
/// (30 vs 60) is a real difference. Averages only ever fall below the nominal
/// rate, so the class is the first one at or above the average.
pub fn fps_class(fps: f64) -> u32 {
    const CLASSES: [u32; 13] = [12, 15, 24, 25, 30, 48, 50, 60, 72, 90, 100, 120, 240];
    if fps.is_nan() || fps <= 0.0 {
        return 0;
    }
    CLASSES
        .iter()
        .copied()
        .find(|&c| c as f64 >= fps * 0.995)
        .unwrap_or(fps.round() as u32)
}

/// Split on commas that aren't inside (...) or [...], as ffmpeg's stream lines
/// nest commas in parentheses: `yuv420p10le(tv, bt709), 3840x2160, ...`.
fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Fill a MergeClip's stream fields from an ffmpeg `-i` banner. The first video
/// stream that isn't an attached picture is the clip (DJI files also carry a
/// 1280x720 JPEG cover as a second "video" stream).
fn parse_merge_streams(err: &str, clip: &mut MergeClip) {
    clip.duration = parse_banner_duration(err);
    clip.captured = parse_banner_creation(err);
    for line in err.lines() {
        let l = line.trim();
        if !l.starts_with("Stream #") {
            if clip.rotation == 0 && l.contains("rotation of") {
                if let Some(v) = l.split("rotation of").nth(1) {
                    let deg = v.trim().split_whitespace().next().and_then(|t| t.parse::<f64>().ok());
                    clip.rotation = deg.map(|d| d.round() as i32).unwrap_or(0);
                }
            }
            continue;
        }
        if clip.vcodec.is_empty() && l.contains("Video:") && !l.contains("attached pic") {
            let parts = split_top(l.split("Video:").nth(1).unwrap_or(""));
            if let Some(first) = parts.first() {
                clip.vcodec = first.split_whitespace().next().unwrap_or("").to_string();
                // `hevc (Main 10) (hvc1 / 0x...)`: the profile is the first
                // parenthesised group that isn't the `tag / fourcc` one.
                clip.profile = first
                    .split('(')
                    .skip(1)
                    .map(|g| g.split(')').next().unwrap_or("").trim())
                    .find(|g| !g.contains('/'))
                    .unwrap_or("")
                    .to_string();
            }
            if let Some(pix) = parts.get(1) {
                clip.pix_fmt = pix.split('(').next().unwrap_or("").trim().to_string();
                // `yuv420p10le(tv, bt2020nc/bt2020/arib-std-b67)`
                let low = pix.to_ascii_lowercase();
                clip.color = if low.contains("arib-std-b67") {
                    "hlg"
                } else if low.contains("smpte2084") {
                    "pq"
                } else {
                    "sdr"
                }
                .into();
            }
            for part in &parts {
                let tok = part.split_whitespace().next().unwrap_or("");
                if let Some((w, h)) = tok.split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                        if clip.width == 0 && w >= 16 && h >= 16 {
                            clip.width = w;
                            clip.height = h;
                        }
                    }
                }
                if let Some(f) = part.strip_suffix(" fps") {
                    clip.fps = f.trim().parse().unwrap_or(0.0);
                }
                if let Some(k) = part.strip_suffix(" kb/s") {
                    clip.vbitrate = k.trim().parse().unwrap_or(0);
                }
            }
        } else if clip.acodec.is_none() && l.contains("Audio:") {
            let parts = split_top(l.split("Audio:").nth(1).unwrap_or(""));
            clip.acodec = parts
                .first()
                .and_then(|p| p.split_whitespace().next())
                .map(|s| s.to_string());
            for part in &parts {
                if let Some(hz) = part.strip_suffix(" Hz") {
                    clip.arate = hz.trim().parse().unwrap_or(0);
                }
            }
            clip.alayout = parts.get(2).cloned().unwrap_or_default();
        }
    }
    clip.fps_class = fps_class(clip.fps);
    clip.signature = format!(
        "{}|{}|{}|{}x{}|{}|{}|{}|{}|{}|{}",
        clip.vcodec,
        clip.profile,
        clip.pix_fmt,
        clip.width,
        clip.height,
        clip.fps_class,
        clip.rotation,
        clip.color,
        clip.acodec.as_deref().unwrap_or("none"),
        clip.arate,
        clip.alayout
    );
}

/// Describe every selected item for the merge list: videos get their stream
/// signature, length and recording time; photos and anything else are listed
/// with their kind and capture date so they sort into place and can be
/// flagged. Returned in shooting order (recording time, then name).
#[tauri::command]
pub async fn merge_probe(state: State<'_, AppState>, paths: Vec<String>) -> Result<Vec<MergeClip>, String> {
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg not available")?;
    let mut files: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for p in &paths {
        files.push(validate_media_anywhere(&state, p).map_err(|e| format!("{}: {e}", file_label(p)))?);
    }
    let mut clips: Vec<MergeClip> = tauri::async_runtime::spawn_blocking(move || {
        warm_pool().install(|| {
            files
                .par_iter()
                .map(|src| {
                    let kind = media::classify(src);
                    let mut clip = MergeClip {
                        path: src.to_string_lossy().to_string(),
                        name: src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        kind: match kind {
                            Kind::Video => "video",
                            Kind::Image | Kind::Raw => "photo",
                            Kind::Other => "other",
                        }
                        .into(),
                        size: std::fs::metadata(src).map(|m| m.len()).unwrap_or(0),
                        ..Default::default()
                    };
                    if !matches!(kind, Kind::Video) {
                        clip.captured = media::capture_date(src);
                        return clip;
                    }
                    match ffmpeg_banner(&ffmpeg, src) {
                        Some(err) => parse_merge_streams(&err, &mut clip),
                        None => clip.error = Some("could not read this file".into()),
                    }
                    if clip.vcodec.is_empty() && clip.error.is_none() {
                        clip.error = Some("no video stream found".into());
                    }
                    clip
                })
                .collect()
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    clips.sort_by(|a, b| {
        let ka = a.captured.unwrap_or(i64::MAX);
        let kb = b.captured.unwrap_or(i64::MAX);
        ka.cmp(&kb).then_with(|| a.name.cmp(&b.name))
    });
    Ok(clips)
}

/// Free bytes on the volume holding `path` (the merge destination check).
#[tauri::command]
pub fn disk_free(path: String) -> Result<u64, String> {
    let dir = canonical_dir(Path::new(&path))?;
    fs4::available_space(&dir).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRequest {
    /// Clips in the order they should play.
    pub paths: Vec<String>,
    pub dest_dir: String,
    /// File name without extension; the first clip's container decides that.
    pub name: String,
    /// Re-encode everything to one format instead of a stream copy. None is
    /// the lossless join.
    #[serde(default)]
    pub convert: Option<MergeConvert>,
}

/// The one format every clip is re-encoded to when the owner chooses
/// "Convert to match": for clips a stream copy can't join, such as Meta
/// glasses footage, where each clip is cropped to a slightly different size
/// (1376×1824 … 1488×1984). A stream copy of mixed sizes decodes the later
/// clips with the first clip's parameter sets and turns them to green
/// garbage (seen 2026-09-29), so there is no lossless way to join those.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MergeConvert {
    pub width: u32,
    pub height: u32,
    /// ffmpeg rate: "30" or "30000/1001".
    pub fps: String,
    pub ten_bit: bool,
    /// "hlg", "pq" or "sdr" — kept as the source's, never tone-mapped.
    pub color: String,
    /// Video bitrate target for the hardware encoders.
    pub bitrate_kbps: u32,
}

impl MergeConvert {
    fn validate(&self) -> Result<(), String> {
        let even = |v: u32| (64..=8192).contains(&v) && v.is_multiple_of(2);
        if !even(self.width) || !even(self.height) {
            return Err(format!("unusable frame size {}x{}", self.width, self.height));
        }
        let rate_ok = {
            let mut it = self.fps.splitn(2, '/');
            let num_ok = it.next().is_some_and(|n| !n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()));
            let den_ok = it.next().is_none_or(|d| !d.is_empty() && d.len() <= 6 && d.bytes().all(|b| b.is_ascii_digit()));
            num_ok && den_ok
        };
        if !rate_ok {
            return Err(format!("unusable frame rate {}", self.fps));
        }
        if !matches!(self.color.as_str(), "hlg" | "pq" | "sdr") {
            return Err(format!("unknown colour {}", self.color));
        }
        Ok(())
    }

    /// Bytes the converted file will take, for the space check.
    fn estimate_bytes(&self, secs: f64) -> u64 {
        let kbps = self.bitrate_kbps.clamp(2_000, 200_000) as f64 + 320.0;
        (secs * kbps * 1000.0 / 8.0) as u64
    }
}

/// The ffmpeg arguments for one attempt at the conversion encode.
/// `encoder` is "videotoolbox" (Mac hardware), "nvenc" (NVIDIA) or "x265"
/// (software, everywhere; slow but always there).
fn merge_convert_encoder_args(c: &MergeConvert, encoder: &str) -> (Vec<String>, &'static str) {
    let kbps = c.bitrate_kbps.clamp(2_000, 200_000);
    let b = format!("{kbps}k");
    let max = format!("{}k", kbps + kbps / 2);
    let mut a: Vec<String> = match encoder {
        "videotoolbox" => vec![
            "-c:v".into(), "hevc_videotoolbox".into(),
            "-profile:v".into(), if c.ten_bit { "main10" } else { "main" }.into(),
            "-b:v".into(), b,
            "-allow_sw".into(), "1".into(),
        ],
        "nvenc" => vec![
            "-c:v".into(), "hevc_nvenc".into(),
            "-preset".into(), "p5".into(),
            "-rc".into(), "vbr".into(),
            "-b:v".into(), b,
            "-maxrate".into(), max,
            "-profile:v".into(), if c.ten_bit { "main10" } else { "main" }.into(),
        ],
        _ => vec![
            "-c:v".into(), "libx265".into(),
            "-preset".into(), "medium".into(),
            "-crf".into(), "18".into(),
        ],
    };
    let (trc, prim, matrix) = match c.color.as_str() {
        "hlg" => ("arib-std-b67", "bt2020", "bt2020nc"),
        "pq" => ("smpte2084", "bt2020", "bt2020nc"),
        _ => ("bt709", "bt709", "bt709"),
    };
    a.extend(
        ["-tag:v", "hvc1", "-color_primaries", prim, "-color_trc", trc, "-colorspace", matrix, "-color_range", "tv"]
            .iter()
            .map(|s| s.to_string()),
    );
    // The pixel format each encoder takes natively, so the filter chain hands
    // it frames without another conversion.
    let pix = match (encoder, c.ten_bit) {
        ("x265", true) => "yuv420p10le",
        ("x265", false) => "yuv420p",
        (_, true) => "p010le",
        (_, false) => "nv12",
    };
    (a, pix)
}

#[derive(Serialize)]
pub struct MergeOutcome {
    pub path: String,
    pub bytes: u64,
}

/// A filename the user typed, made safe for every filesystem FoxCull writes to
/// (FAT/exFAT cards included) while keeping spaces and punctuation readable.
fn friendly_file_stem(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim().to_string();
    if trimmed.is_empty() {
        "Merged video".into()
    } else {
        trimmed.chars().take(180).collect()
    }
}

/// The merge that's running or just finished: one at a time, owned by the
/// backend so the Merge window can be closed mid-merge and reopened onto it.
#[derive(Serialize, Clone, Default)]
pub struct MergeStatus {
    /// "idle" | "running" | "done" | "error" | "cancelled"
    pub state: String,
    pub paused: bool,
    pub label: String,
    /// Output file name and folder.
    pub name: String,
    pub out_path: String,
    pub dest_dir: String,
    pub clips: usize,
    pub total_s: f64,
    pub in_bytes: u64,
    pub convert: bool,
    pub pct: u64,
    pub detail: Option<String>,
    pub started_ms: i64,
    pub finished_ms: i64,
    pub out_bytes: u64,
    pub error: Option<String>,
}

static MERGE: std::sync::LazyLock<Mutex<MergeStatus>> = std::sync::LazyLock::new(|| Mutex::new(MergeStatus::default()));

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn merge_status() -> MergeStatus {
    let mut m = MERGE.lock().clone();
    if m.state.is_empty() {
        m.state = "idle".into();
    }
    m.paused = m.state == "running" && crate::procs::merge_paused();
    m
}

/// Pause or resume the running merge (its ffmpeg is suspended in place).
#[tauri::command]
pub fn merge_pause(app: AppHandle, paused: bool) -> Result<MergeStatus, String> {
    if MERGE.lock().state != "running" {
        return Err("no merge is running".into());
    }
    crate::procs::set_merge_paused(paused);
    let m = merge_status();
    emit_job(
        &app,
        Activity {
            id: "merge".into(),
            label: m.label.clone(),
            done: m.pct,
            total: 100,
            state: "running".into(),
            detail: if paused { Some("Paused".into()) } else { m.detail.clone() },
            cancellable: true,
            paused,
            ..Default::default()
        },
    );
    crate::log::line(&format!("MERGE {}", if paused { "paused" } else { "resumed" }));
    Ok(m)
}

/// Forget a finished merge (the window's "Done").
#[tauri::command]
pub fn merge_dismiss() {
    let mut m = MERGE.lock();
    if m.state != "running" {
        *m = MergeStatus::default();
    }
}

/// What went wrong, in words the owner can act on. ffmpeg's last stderr line
/// ("Error writing trailer: No space left on device") is accurate but says
/// nothing about which drive or what to do.
fn friendly_merge_error(raw: &str, files: &[PathBuf], dest: &Path) -> String {
    let lower = raw.to_lowercase();
    if let Some(gone) = files.iter().find(|f| !f.exists()) {
        let name = gone.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        return format!("Lost access to {name} partway through (was the card or drive removed?). Nothing was saved.");
    }
    if !dest.parent().is_some_and(|d| d.is_dir()) {
        return "The destination folder disappeared partway through (was the drive removed?). Nothing was saved.".into();
    }
    if lower.contains("no space left") || lower.contains("disk full") || lower.contains("not enough space") {
        return "The destination drive ran out of space. Nothing was saved: free some space or pick a bigger drive.".into();
    }
    if lower.contains("input/output error") || lower.contains("device not configured") {
        return format!("A drive stopped responding while merging ({raw}). Nothing was saved. Check the cable or card and try again.");
    }
    if lower.contains("invalid data found") || lower.contains("moov atom not found") {
        return format!("One of the clips is damaged or incomplete ({raw}). Nothing was saved. Remove it from the list and try again.");
    }
    if lower.contains("permission denied") || lower.contains("operation not permitted") {
        return "FoxCull isn't allowed to write to that folder. Pick another one, or allow FoxCull in System Settings → Privacy & Security.".into();
    }
    format!("The merge failed: {raw}. Nothing was saved.")
}

/// Join clips end to end with a stream copy (ffmpeg's concat demuxer): no
/// re-encode, so the result is exactly the camera's video and audio, and the
/// work is a straight file copy (about as fast as the disks allow). Only the
/// main video and first audio stream are kept; DJI's ~5 Mbps debug track,
/// timecode, metadata track and cover JPEG are dropped. Refuses up front if
/// the destination volume can't hold the result, rather than failing an hour in.
/// Runs as job "merge" with its own Stop and Pause (see `MergeStatus`), so the
/// Merge window can close while it works.
#[tauri::command]
pub async fn merge_videos(
    app: AppHandle,
    state: State<'_, AppState>,
    req: MergeRequest,
) -> Result<MergeOutcome, String> {
    if req.paths.len() < 2 {
        return Err("pick at least two videos to merge".into());
    }
    let mut files: Vec<PathBuf> = Vec::with_capacity(req.paths.len());
    for p in &req.paths {
        let src = validate_media_anywhere(&state, p).map_err(|e| {
            if !Path::new(p).exists() {
                format!("{} isn't there any more (was the card or drive removed?)", file_label(p))
            } else {
                format!("{}: {e}", file_label(p))
            }
        })?;
        if !matches!(media::classify(&src), Kind::Video) {
            return Err(format!("not a video: {}", file_label(p)));
        }
        files.push(src);
    }
    let dest_dir = canonical_dir(Path::new(&req.dest_dir))
        .map_err(|_| "That destination folder isn't available (was the drive removed?). Pick another one.".to_string())?;
    if dest_dir
        .components()
        .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(LIB_DIRNAME))
        || within(&dest_dir, &state.data_root)
    {
        return Err("choose a folder outside FoxCull's library".into());
    }
    let ext = files[0]
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| e == "mp4" || e == "mov" || e == "m4v")
        .unwrap_or_else(|| "mp4".into());
    if let Some(c) = &req.convert {
        c.validate()?;
    }
    // A conversion always writes MP4 (HEVC in it is what YouTube and Apple
    // players expect); a stream copy keeps the first clip's container.
    let ext = if req.convert.is_some() { "mp4".to_string() } else { ext };
    let dest = uniquify(dest_dir.join(format!("{}.{ext}", friendly_file_stem(&req.name))));
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg not available")?;

    let lengths: Vec<f64> = files.iter().map(|f| clip_length(Some(&ffmpeg), f).unwrap_or(0.0)).collect();
    let total_s: f64 = lengths.iter().sum();
    let need: u64 = match &req.convert {
        // The converted parts stay until they're joined: twice the result.
        Some(c) => c.estimate_bytes(total_s) * 2,
        None => files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).sum(),
    };
    if let Ok(free) = fs4::available_space(&dest_dir) {
        // The copy drops the debug/metadata tracks, so `need` over-estimates a
        // little; keep a 1 GB margin for the filesystem anyway.
        if free < need + (1 << 30) {
            let gb = |b: u64| b as f64 / 1e9;
            return Err(format!(
                "Not enough space in that folder: the merged file needs about {:.1} GB and only {:.1} GB is free. Pick a folder on a bigger drive.",
                gb(need),
                gb(free)
            ));
        }
    }

    // One merge at a time: it has its own job-centre entry and Stop button, and
    // the window can be hidden while it runs, so a second one would be easy to
    // start by accident and would halve both on the same disks.
    if JOB_CANCELS.lock().contains_key("merge") {
        return Err("A merge is already running. Wait for it to finish, or stop it from the progress panel.".into());
    }
    let flag = job_token("merge");
    crate::procs::set_merge_paused(false);
    let in_bytes: u64 = files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).sum();
    {
        let out_name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        *MERGE.lock() = MergeStatus {
            state: "running".into(),
            label: format!("Merging {} clips → {out_name}", files.len()),
            name: out_name,
            out_path: dest.to_string_lossy().to_string(),
            dest_dir: dest_dir.to_string_lossy().to_string(),
            clips: files.len(),
            total_s,
            in_bytes,
            convert: req.convert.is_some(),
            started_ms: now_ms(),
            ..Default::default()
        };
    }
    tauri::async_runtime::spawn_blocking(move || {
        let out_name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let watch = ExportWatch {
            app: app.clone(),
            job: "merge",
            gen: None,
            flag: Some(flag.clone()),
            label: format!("Merging {} clips → {out_name}", files.len()),
            detail: None,
            total_s,
            base_pct: 0.0,
            span_pct: 100.0,
            // A stream copy writes about what it reads (minus DJI's debug
            // track), so the clips' size is the honest "of" for the bytes line.
            expect_bytes: if req.convert.is_some() { 0 } else { in_bytes },
            started: Instant::now(),
        };
        watch.emit(0, "running");
        let how = if req.convert.is_some() { "converted" } else { "copy" };
        crate::log::line(&format!(
            "MERGE start ({how}) clips={} in_bytes={in_bytes} secs={total_s:.0} src={:?} dest={dest:?}",
            files.len(),
            files[0].parent()
        ));

        let res = match &req.convert {
            Some(conv) => merge_convert(&ffmpeg, &files, conv, &dest, Some(&watch)),
            None => merge_copy(&ffmpeg, &files, &dest, Some(&watch)),
        };
        job_finished("merge", &flag);
        crate::procs::set_merge_paused(false);
        let res = res.map_err(|e| if e == EXPORT_CANCELLED { e } else { friendly_merge_error(&e, &files, &dest) });
        {
            let mut m = MERGE.lock();
            m.finished_ms = now_ms();
            match &res {
                Ok(()) => {
                    m.state = "done".into();
                    m.pct = 100;
                    m.out_bytes = std::fs::metadata(&dest).map(|x| x.len()).unwrap_or(0);
                    m.detail = None;
                }
                Err(e) if e == EXPORT_CANCELLED => m.state = "cancelled".into(),
                Err(e) => {
                    m.state = "error".into();
                    m.error = Some(e.clone());
                }
            }
        }
        match res {
            Ok(()) => {
                let bytes = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                let secs = watch.started.elapsed().as_secs_f64();
                // Any window showing that folder (the library) refreshes.
                let _ = app.emit("media-output", dest.to_string_lossy().to_string());
                emit_job(
                    &app,
                    Activity {
                        id: "merge".into(),
                        label: format!("Merged {} clips → {out_name}", files.len()),
                        done: 100,
                        total: 100,
                        state: "done".into(),
                        detail: Some(format!("{} in {}", fmt_bytes(bytes), fmt_secs(secs))),
                        path: Some(dest.to_string_lossy().to_string()),
                        ..Default::default()
                    },
                );
                // Throughput in the log: merges are bound by the slower of the
                // two disks, and this is how to tell which one it was.
                crate::log::line(&format!(
                    "MERGE ok ({how}) clips={} bytes={bytes} secs={secs:.1} MBps={:.0} dest={dest:?}",
                    files.len(),
                    (in_bytes as f64 / 1e6) / secs.max(0.001)
                ));
                Ok(MergeOutcome { path: dest.to_string_lossy().to_string(), bytes })
            }
            Err(e) if e == EXPORT_CANCELLED => {
                emit_job(
                    &app,
                    Activity {
                        id: "merge".into(),
                        label: "Merge stopped".into(),
                        done: 0,
                        total: 100,
                        state: "cancelled".into(),
                        detail: Some("Nothing was saved".into()),
                        ..Default::default()
                    },
                );
                crate::log::line("MERGE cancelled");
                Err(e)
            }
            Err(e) => {
                // Never leave a half-written file behind to be uploaded by mistake.
                let _ = std::fs::remove_file(&dest);
                emit_job(
                    &app,
                    Activity {
                        id: "merge".into(),
                        label: "Merge failed".into(),
                        done: 0,
                        total: 100,
                        state: "error".into(),
                        detail: Some(e.clone()),
                        ..Default::default()
                    },
                );
                crate::log::line(&format!("MERGE failed: {e}"));
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "45 s", "3 min 20 s", "1 h 12 min".
fn fmt_secs(s: f64) -> String {
    let s = s.round().max(1.0) as u64;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{} h {} min", s / 3600, (s % 3600) / 60)
    }
}

/// The lossless join: ffmpeg's concat demuxer with a stream copy. Each file is
/// offset by its own length, video and audio together, so sound stays with
/// picture across every join.
fn merge_copy(ffmpeg: &Path, files: &[PathBuf], dest: &Path, watch: Option<&ExportWatch>) -> Result<(), String> {
    static MERGE_SEQ: AtomicU64 = AtomicU64::new(0);
    let list_path = std::env::temp_dir().join(format!(
        "foxcull-merge-{}-{}-{}.txt",
        std::process::id(),
        now(),
        MERGE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let write_list = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&list_path)?;
        for clip in files {
            writeln!(f, "file '{}'", concat_list_escape(clip))?;
        }
        Ok(())
    };
    write_list().map_err(|e| e.to_string())?;

    let first_banner = ffmpeg_banner(ffmpeg, &files[0]).unwrap_or_default();
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        // `V` = video that isn't an attached picture, so DJI's cover JPEG
        // can never be picked over the real stream.
        .args(["-map", "0:V:0", "-map", "0:a:0?", "-c", "copy"])
        .args(["-avoid_negative_ts", "make_zero"]);
    if first_banner.contains("Video: hevc") {
        // Apple players and YouTube expect HEVC in MP4 tagged hvc1.
        cmd.args(["-tag:v", "hvc1"]);
    }
    if let Some(ts) = parse_banner_creation(&first_banner) {
        cmd.args(["-metadata", &format!("creation_time={}", iso_utc(ts))]);
    }
    cmd.args(["-progress", "pipe:1", "-nostats"]).arg(dest);
    let res = run_ffmpeg_watched(cmd, watch, dest);
    let _ = std::fs::remove_file(&list_path);
    res
}

/// The ffmpeg command that converts ONE clip to the merge's target format.
fn merge_convert_part_cmd(ffmpeg: &Path, src: &Path, info: &MergeClip, conv: &MergeConvert, encoder: &str, out: &Path) -> Command {
    let (enc_args, pix) = merge_convert_encoder_args(conv, encoder);
    let has_audio = info.acodec.is_some();
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-i"]).arg(src);
    if !has_audio {
        // Silence for the clip's length, so every part has the same streams.
        cmd.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]);
    }
    cmd.args(["-map", "0:V:0", "-map", if has_audio { "0:a:0" } else { "1:a:0" }]);
    // Fill the frame, then centre-crop the few pixels of aspect difference;
    // one constant frame rate for the whole file (phones and glasses record
    // a variable one).
    cmd.arg("-vf").arg(format!(
        "scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={w}:{h},setsar=1,fps={fps},format={pix}",
        w = conv.width,
        h = conv.height,
        fps = conv.fps,
    ));
    cmd.args(&enc_args);
    // A keyframe every 2 s: what YouTube asks for.
    let rate = {
        let mut it = conv.fps.splitn(2, '/');
        let num: f64 = it.next().and_then(|n| n.parse().ok()).unwrap_or(30.0);
        let den: f64 = it.next().and_then(|d| d.parse().ok()).unwrap_or(1.0);
        (num / den.max(1.0)).round().max(1.0)
    };
    cmd.args(["-g", &format!("{}", (rate * 2.0) as u32)]);
    // AAC 48 kHz stereo is what the merged file carries; copy it when it's
    // already that (no generation loss), otherwise encode to it.
    let copy_audio = has_audio && info.acodec.as_deref() == Some("aac") && info.arate == 48_000 && info.alayout == "stereo";
    if copy_audio {
        cmd.args(["-c:a", "copy"]);
    } else {
        cmd.args(["-c:a", "aac", "-b:a", "320k", "-ar", "48000", "-ac", "2"]);
    }
    if !has_audio {
        cmd.arg("-shortest");
    }
    // ffmpeg drops creation_time when it re-encodes; keep the recording time
    // so the join (which reads it off the first part) stamps the real one.
    if let Some(ts) = info.captured {
        cmd.args(["-metadata", &format!("creation_time={}", iso_utc(ts))]);
    }
    cmd.args(["-progress", "pipe:1", "-nostats"]).arg(out);
    cmd
}

/// "Convert to match": every clip re-encoded ON ITS OWN to one format, then
/// the parts joined by `merge_copy`, the same stream copy as the lossless
/// merge.
///
/// Why not a single ffmpeg with every clip as an input and the concat filter:
/// ffmpeg 9 mis-times segments when many inputs feed one filtergraph. On 12
/// Meta glasses clips it left 3 min 16 s of frozen picture at six of the
/// joins (2026-09-29). Converting clip by clip keeps each clip's own
/// timeline, and the concat demuxer offsets video and audio together.
///
/// A stream-copy join needs every part to carry byte-identical parameter
/// sets. The same encoder with the same settings gives exactly that (checked
/// on VideoToolbox with 4 clips of 4 different sizes), so the encoder is
/// chosen on the first clip and kept for the rest, and the parts' decoder
/// configs are compared before joining: a mismatch fails with nothing
/// written, rather than a file that turns to garbage at a join.
fn merge_convert(ffmpeg: &Path, files: &[PathBuf], conv: &MergeConvert, dest: &Path, watch: Option<&ExportWatch>) -> Result<(), String> {
    let n = files.len();
    let infos: Vec<MergeClip> = files
        .iter()
        .map(|f| {
            let mut c = MergeClip::default();
            if let Some(b) = ffmpeg_banner(ffmpeg, f) {
                parse_merge_streams(&b, &mut c);
            }
            c
        })
        .collect();
    let secs_of = |c: &MergeClip| c.duration.max(0.1);
    let total: f64 = infos.iter().map(secs_of).sum();
    // Parts go next to the destination (the drive whose space was checked),
    // in a dot-folder the library scan skips; always removed afterwards.
    static CONVERT_SEQ: AtomicU64 = AtomicU64::new(0);
    let work = dest
        .parent()
        .ok_or("no destination folder")?
        .join(format!(".foxcull-merge-{}-{}-{}", std::process::id(), now(), CONVERT_SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    crate::procs::scratch_add(&work);
    let sub_watch = |detail: String, secs: f64, base: f64, span: f64| {
        watch.map(|w| ExportWatch {
            app: w.app.clone(),
            job: w.job,
            gen: w.gen.clone(),
            flag: w.flag.clone(),
            label: w.label.clone(),
            detail: Some(detail),
            total_s: secs,
            base_pct: base,
            span_pct: span,
            expect_bytes: 0,
            started: w.started,
        })
    };
    const CONVERT_SHARE: f64 = 95.0;
    let res = (|| -> Result<(), String> {
        let ladder: &[&str] = if cfg!(target_os = "macos") { &["videotoolbox", "x265"] } else { &["nvenc", "x265"] };
        let mut chosen: Option<&str> = None;
        let mut parts: Vec<PathBuf> = Vec::with_capacity(n);
        let mut done_s = 0.0;
        for (i, (src, info)) in files.iter().zip(&infos).enumerate() {
            let part = work.join(format!("part{i:04}.mp4"));
            let secs = secs_of(info);
            let w = sub_watch(
                format!("Converting clip {} of {n}", i + 1),
                secs,
                CONVERT_SHARE * done_s / total,
                CONVERT_SHARE * secs / total,
            );
            let candidates: Vec<&str> = match chosen {
                Some(e) => vec![e],
                None => ladder.to_vec(),
            };
            let mut last_err = String::from("no encoder worked");
            for enc in candidates {
                let cmd = merge_convert_part_cmd(ffmpeg, src, info, conv, enc, &part);
                match run_ffmpeg_watched(cmd, w.as_ref(), &part) {
                    Ok(()) => {
                        chosen = Some(enc);
                        break;
                    }
                    Err(e) if e == EXPORT_CANCELLED => return Err(e),
                    Err(e) => {
                        let _ = std::fs::remove_file(&part);
                        crate::log::line(&format!("MERGE convert with {enc} failed on {src:?}: {e}"));
                        last_err = e;
                    }
                }
            }
            if !part.is_file() {
                let name = src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                return Err(format!("Couldn't convert {name}: {last_err}"));
            }
            parts.push(part);
            done_s += secs;
        }
        let first = crate::video::mp4_codec_config(&parts[0]);
        if first.is_none() || parts[1..].iter().any(|p| crate::video::mp4_codec_config(p) != first) {
            return Err("The converted clips came out with different encoder settings, so joining them would corrupt the video. Nothing was saved.".into());
        }
        crate::log::line(&format!("MERGE converted {n} clips with {}", chosen.unwrap_or("?")));
        // Space was checked before starting, but a conversion takes minutes
        // and other copies can fill the drive meanwhile (seen 2026-09-30: the
        // join ran out of room on an external SSD at 4.3 GB). Check again,
        // for the joined file, which is the size of the parts.
        let parts_bytes: u64 = parts.iter().map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)).sum();
        if let Ok(free) = fs4::available_space(&work) {
            if free < parts_bytes + (256 << 20) {
                return Err(format!(
                    "The drive filled up while converting: joining needs about {:.1} GB and only {:.1} GB is free. Free some space or pick another folder.",
                    parts_bytes as f64 / 1e9,
                    free as f64 / 1e9
                ));
            }
        }
        let w = sub_watch("Joining the converted clips".into(), total, CONVERT_SHARE, 100.0 - CONVERT_SHARE);
        // Each part carries its clip's recording time, so the join stamps the
        // first clip's, as the lossless merge does.
        merge_copy(ffmpeg, &parts, dest, w.as_ref())
    })();
    let _ = std::fs::remove_dir_all(&work);
    crate::procs::scratch_remove(&work);
    res
}

/// Unix seconds → `YYYY-MM-DDTHH:MM:SSZ`.
fn iso_utc(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, (secs % 3600) / 60, secs % 60)
}

#[cfg(test)]
mod transfer_tests {
    use super::{copy_file_progress, TRANSFER_CANCELLED};
    use std::sync::atomic::{AtomicBool, Ordering};


    /// One test drive: a root folder with media files and its own catalog.
    fn drive(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("foxcull-xfer-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Trip")).unwrap();
        d
    }

    fn files(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        (0..3)
            .map(|i| {
                let name = format!("DSC_{i:04}.JPG");
                let data: Vec<u8> = (0..(3u32 << 20) + i * 7919).map(|b| ((b + i) * 131 % 251) as u8).collect();
                let p = root.join("Trip").join(&name);
                std::fs::write(&p, &data).unwrap();
                let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000 + i as u64);
                std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
                (name, data)
            })
            .collect()
    }

    /// Moves files from `src_root` to `dest_root/Picks` and checks every
    /// promise a move makes: bytes, times, originals gone, marks carried.
    fn run_move(src_root: &std::path::Path, dest_root: &std::path::Path, copy: bool) {
        use super::{transfer_catalog, transfer_files, TransferPlan};
        use crate::catalog::Catalog;
        let src_cat = Catalog::open(&src_root.join("catalog.sqlite")).unwrap();
        let originals = files(src_root);
        for (name, _) in &originals {
            src_cat.set_rating(&format!("Trip/{name}"), 4).unwrap();
        }
        let dest_dir = dest_root.join("Picks");
        std::fs::create_dir_all(&dest_dir).unwrap();
        let root = std::fs::canonicalize(src_root).unwrap();
        let droot = std::fs::canonicalize(dest_root).unwrap();
        let plan = TransferPlan {
            root: root.clone(),
            lib: None,
            cache_dir: root.join("no-cache"),
            dest_dir: std::fs::canonicalize(&dest_dir).unwrap(),
            dest_root: droot.clone(),
            copy_mode: copy,
            cross_drive: root != droot,
        };
        let paths: Vec<String> = originals.iter().map(|(n, _)| root.join("Trip").join(n).to_string_lossy().to_string()).collect();
        let never = AtomicBool::new(false);
        let seen = std::cell::Cell::new(0u64);
        let (out, pairs) = transfer_files(&plan, paths, &never, &|a| seen.set(seen.get().max(a.done)));
        assert_eq!(out.moved, 3, "{:?}", out.errors);
        assert!(out.failed.is_empty());
        let data_root = std::env::temp_dir().join(format!("foxcull-xfer-data-{}", std::process::id()));
        transfer_catalog(&src_cat, &data_root, &plan, &pairs).unwrap();
        for (i, (name, data)) in originals.iter().enumerate() {
            let at = plan.dest_dir.join(name);
            assert_eq!(&std::fs::read(&at).unwrap(), data, "bytes of {name}");
            let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000 + i as u64);
            assert_eq!(std::fs::metadata(&at).unwrap().modified().unwrap(), t, "mtime of {name}");
            assert_eq!(root.join("Trip").join(name).exists(), copy, "original of {name}");
        }
        // The marks are where the files are now.
        let dest_cat = if plan.cross_drive { Catalog::open(&super::resolve_library(&data_root, &droot).catalog).unwrap() } else { Catalog::open(&src_root.join("catalog.sqlite")).unwrap() };
        for (name, _) in &originals {
            let got = dest_cat.export_entries(&[format!("Picks/{name}")]);
            assert_eq!(got[0].decision.as_ref().map(|d| d.0), Some(4), "mark on moved {name}");
            let left = src_cat.export_entries(&[format!("Trip/{name}")]);
            assert_eq!(left[0].decision.is_some(), copy, "mark left on the original {name}");
        }
        if plan.cross_drive || copy {
            assert!(seen.get() > 0, "a copy reports its bytes");
        }
        let _ = std::fs::remove_dir_all(&data_root);
    }

    #[test]
    fn a_move_on_one_drive_renames_and_rekeys_the_marks() {
        let d = drive("same");
        run_move(&d, &d, false);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_copy_on_one_drive_keeps_the_original_and_its_marks() {
        let d = drive("copy");
        run_move(&d, &d, true);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Another drive, for real: set FOXCULL_XFER_DEST to a folder on a
    /// different volume (an exFAT disk image works) and run with --ignored.
    /// The move then copies across devices, flushes, verifies, deletes, and
    /// the marks go into that drive's own `_FoxCull/catalog.sqlite`.
    #[test]
    #[ignore]
    fn real_cross_drive_move() {
        let dest = std::path::PathBuf::from(std::env::var("FOXCULL_XFER_DEST").expect("FOXCULL_XFER_DEST"));
        let d = drive("x");
        run_move(&d, &dest, false);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(dest.join("Picks"));
        let _ = std::fs::remove_dir_all(dest.join("_FoxCull"));
    }

    /// Drops land only where the tree can show a folder: never in a library,
    /// a Trash, ~/Library or the OS, and `/Volumes` under `/` isn't a reason
    /// to refuse an external drive.
    #[cfg(unix)]
    #[test]
    fn move_destinations_follow_the_trees_rules() {
        use super::hidden_dest_component as h;
        use std::path::Path;
        assert_eq!(h(Path::new("/Users/me/Movies")), None);
        assert_eq!(h(Path::new("/Volumes/MAHINDRA/USA Trip 2026 Sep/Meta AI")), None);
        assert_eq!(h(Path::new("/Volumes/MAHINDRA")), None);
        assert_eq!(h(Path::new("/Volumes/MAHINDRA/_FoxCull/thumbs")).as_deref(), Some("_FoxCull"));
        assert_eq!(h(Path::new("/Volumes/MAHINDRA/FoxCull Trash")).as_deref(), Some("FoxCull Trash"));
        assert_eq!(h(Path::new("/Users/me/Library/Caches")).as_deref(), Some("Library"));
        assert_eq!(h(Path::new("/Volumes/SSD/Library")), None, "a drive's own Library folder is someone's photos");
        assert_eq!(h(Path::new("/Users/me/.hidden")).as_deref(), Some(".hidden"));
    }

    #[test]
    fn a_copy_keeps_bytes_and_times_and_a_cancel_leaves_nothing() {
        let dir = std::env::temp_dir().join(format!("foxcull-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.mp4");
        let data: Vec<u8> = (0..(9u32 << 20)).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options().write(true).open(&src).unwrap().set_modified(old).unwrap();

        let dst = dir.join("b.mp4");
        let never = AtomicBool::new(false);
        let mut seen = 0u64;
        copy_file_progress(&src, &dst, true, &never, &mut |b| seen += b).unwrap();
        assert_eq!(seen, data.len() as u64);
        assert_eq!(std::fs::read(&dst).unwrap(), data);
        assert_eq!(std::fs::metadata(&dst).unwrap().modified().unwrap(), old);

        // Refuses to overwrite.
        assert!(copy_file_progress(&src, &dst, false, &never, &mut |_| {}).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(), data);

        // Stopped after the first chunk: no partial file is left behind.
        let stop = AtomicBool::new(false);
        let dst2 = dir.join("c.mp4");
        let r = copy_file_progress(&src, &dst2, false, &stop, &mut |_| stop.store(true, Ordering::Relaxed));
        assert_eq!(r.unwrap_err(), TRANSFER_CANCELLED);
        assert!(!dst2.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod merge_tests {
    use super::{fps_class, iso_utc, merge_convert, merge_convert_part_cmd, parse_merge_streams, MergeClip, MergeConvert};
    use std::path::Path;

    const OSMO_60: &str = r#"
  Duration: 00:05:34.38, start: 0.000000, bitrate: 115381 kb/s
    creation_time   : 2026-09-21T19:07:48.000000Z
  Stream #0:0[0x1](und): Video: hevc (Main 10) (hvc1 / 0x31637668), yuv420p10le(tv, bt709), 3840x2160, 109895 kb/s, 59.94 fps, 59.94 tbr, 60k tbn (default)
  Stream #0:1[0x2](und): Audio: aac (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 317 kb/s (default)
  Stream #0:2[0x3](und): Data: none (djmd / 0x646D6A64), 27 kb/s
  Stream #0:5[0x0]: Video: mjpeg (Baseline), yuvj420p(pc, bt470bg/unknown/unknown), 1280x720 [SAR 1:1 DAR 16:9], 90k tbr, 90k tbn (attached pic)
"#;

    #[test]
    fn parses_an_osmo_clip() {
        let mut c = MergeClip::default();
        parse_merge_streams(OSMO_60, &mut c);
        assert_eq!(c.vcodec, "hevc");
        assert_eq!(c.profile, "Main 10");
        assert_eq!(c.pix_fmt, "yuv420p10le");
        assert_eq!((c.width, c.height), (3840, 2160));
        assert!((c.fps - 59.94).abs() < 0.001);
        assert_eq!(c.acodec.as_deref(), Some("aac"));
        assert_eq!(c.arate, 48000);
        assert_eq!(c.alayout, "stereo");
        assert!((c.duration - 334.38).abs() < 0.01);
        assert_eq!(iso_utc(c.captured.unwrap()), "2026-09-21T19:07:48Z");
    }

    #[test]
    fn a_30fps_8bit_clip_has_a_different_signature() {
        let other = OSMO_60
            .replace("hevc (Main 10)", "hevc (Main)")
            .replace("yuv420p10le", "yuv420p")
            .replace("59.94 fps", "29.97 fps");
        let (mut a, mut b) = (MergeClip::default(), MergeClip::default());
        parse_merge_streams(OSMO_60, &mut a);
        parse_merge_streams(&other, &mut b);
        assert_ne!(a.signature, b.signature);
        assert_eq!(b.profile, "Main");
        assert!((b.fps - 29.97).abs() < 0.001);
    }

    #[test]
    fn cover_art_never_wins_and_vertical_is_distinct() {
        let vertical = OSMO_60.replace("3840x2160", "1728x3072");
        let mut v = MergeClip::default();
        parse_merge_streams(&vertical, &mut v);
        assert_eq!((v.width, v.height), (1728, 3072));
        let mut c = MergeClip::default();
        parse_merge_streams(OSMO_60, &mut c);
        assert_ne!(v.signature, c.signature);
    }

    // Meta Ray-Ban Display glasses, 2026-09: HLG, variable frame rate, and a
    // slightly different crop per clip.
    const META: &str = r#"
  Duration: 00:01:25.06, start: 0.000000, bitrate: 15241 kb/s
    creation_time   : 2026-09-25T15:47:22.000000Z
  Stream #0:0[0x1](eng): Video: hevc (Main 10) (hvc1 / 0x31637668), yuv420p10le(tv, bt2020nc/bt2020/arib-std-b67), 1392x1856, 15076 kb/s, SAR 1:1 DAR 3:4, 29.88 fps, 120 tbr, 90k tbn (default)
  Stream #0:1[0x2](eng): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 128 kb/s (default)
"#;

    #[test]
    fn frame_rate_classes_absorb_variable_rate_and_ntsc() {
        for (avg, class) in [
            (29.73, 30), (29.88, 30), (29.97, 30), (30.02, 30), (30.0, 30),
            (23.976, 24), (24.0, 24), (25.0, 25), (59.94, 60), (50.0, 50),
            (119.88, 120), (26.5, 30),
        ] {
            assert_eq!(fps_class(avg), class, "{avg}");
        }
        assert_eq!(fps_class(0.0), 0);
    }

    #[test]
    fn meta_clips_differ_only_where_they_really_differ() {
        let mut a = MergeClip::default();
        parse_merge_streams(META, &mut a);
        assert_eq!((a.width, a.height, a.fps_class, a.vbitrate), (1392, 1856, 30, 15076));
        assert_eq!(a.color, "hlg");
        // Same size, a different average rate: the same signature.
        let mut b = MergeClip::default();
        parse_merge_streams(&META.replace("29.88 fps", "30 fps").replace("120 tbr", "30 tbr"), &mut b);
        assert_eq!(a.signature, b.signature);
        // A different crop: a different signature (a stream copy would corrupt).
        let mut c = MergeClip::default();
        parse_merge_streams(&META.replace("1392x1856", "1376x1840"), &mut c);
        assert_ne!(a.signature, c.signature);
        // HDR and SDR never share a signature.
        let mut d = MergeClip::default();
        parse_merge_streams(&META.replace("bt2020nc/bt2020/arib-std-b67", "bt709"), &mut d);
        assert_eq!(d.color, "sdr");
        assert_ne!(a.signature, d.signature);
    }

    fn conv() -> MergeConvert {
        MergeConvert { width: 1488, height: 1984, fps: "30".into(), ten_bit: true, color: "hlg".into(), bitrate_kbps: 37_000 }
    }

    #[test]
    fn convert_requests_are_validated() {
        assert!(conv().validate().is_ok());
        assert!(MergeConvert { fps: "30000/1001".into(), ..conv() }.validate().is_ok());
        assert!(MergeConvert { width: 1487, ..conv() }.validate().is_err());
        assert!(MergeConvert { fps: "30;rm".into(), ..conv() }.validate().is_err());
        assert!(MergeConvert { fps: "/1".into(), ..conv() }.validate().is_err());
        assert!(MergeConvert { color: "bt601".into(), ..conv() }.validate().is_err());
    }

    #[test]
    fn a_part_keeps_hdr_copies_matching_audio_and_fills_missing_audio() {
        let mut clip = MergeClip::default();
        parse_merge_streams(META, &mut clip);
        let args = |c: &MergeClip, enc: &str| -> Vec<String> {
            merge_convert_part_cmd(Path::new("ffmpeg"), Path::new("in.mp4"), c, &conv(), enc, Path::new("out.mp4"))
                .get_args()
                .map(|a| a.to_string_lossy().to_string())
                .collect()
        };
        let vt = args(&clip, "videotoolbox").join(" ");
        assert!(vt.contains("-c:v hevc_videotoolbox -profile:v main10 -b:v 37000k"), "{vt}");
        assert!(vt.contains("scale=1488:1984:force_original_aspect_ratio=increase:flags=lanczos,crop=1488:1984,setsar=1,fps=30,format=p010le"), "{vt}");
        assert!(vt.contains("-color_primaries bt2020 -color_trc arib-std-b67 -colorspace bt2020nc"), "{vt}");
        assert!(vt.contains("-c:a copy"), "{vt}");
        assert!(vt.contains("-g 60"), "{vt}");
        assert!(vt.contains("-metadata creation_time=2026-09-25T15:47:22Z"), "{vt}");
        let x = args(&clip, "x265").join(" ");
        assert!(x.contains("libx265") && x.contains("format=yuv420p10le"), "{x}");
        clip.acodec = None;
        let silent = args(&clip, "videotoolbox").join(" ");
        assert!(silent.contains("anullsrc") && silent.contains("-map 1:a:0") && silent.contains("-shortest"), "{silent}");
        clip.acodec = Some("pcm_s16le".into());
        assert!(args(&clip, "videotoolbox").join(" ").contains("-c:a aac -b:a 320k"));
    }

    /// The whole convert-merge on real files. Opt-in:
    /// FOXCULL_FFMPEG=… FOXCULL_MERGE_FILES="a.mp4\nb.mp4" FOXCULL_MERGE_DEST=out.mp4
    /// FOXCULL_MERGE_CONVERT="1488x1984@30,hlg,10,37000" cargo test --lib real_convert_merge -- --ignored
    #[test]
    #[ignore]
    fn real_convert_merge() {
        let ffmpeg = std::env::var("FOXCULL_FFMPEG").expect("FOXCULL_FFMPEG");
        let files: Vec<std::path::PathBuf> = std::env::var("FOXCULL_MERGE_FILES").expect("FOXCULL_MERGE_FILES").lines().map(Into::into).collect();
        let dest = std::path::PathBuf::from(std::env::var("FOXCULL_MERGE_DEST").expect("FOXCULL_MERGE_DEST"));
        let spec = std::env::var("FOXCULL_MERGE_CONVERT").expect("FOXCULL_MERGE_CONVERT");
        let (size, rest) = spec.split_once('@').unwrap();
        let (w, h) = size.split_once('x').unwrap();
        let f: Vec<&str> = rest.split(',').collect();
        let c = MergeConvert {
            width: w.parse().unwrap(),
            height: h.parse().unwrap(),
            fps: f[0].into(),
            color: f[1].into(),
            ten_bit: f[2] == "10",
            bitrate_kbps: f[3].parse().unwrap(),
        };
        c.validate().unwrap();
        assert!(!dest.exists(), "refusing to overwrite {dest:?}");
        merge_convert(Path::new(&ffmpeg), &files, &c, &dest, None).unwrap();
        assert!(dest.is_file());
    }
}

#[tauri::command]
pub fn set_rating(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
    rating: i64,
) -> Result<(), String> {
    let rel = rel_of(&state.root.lock().clone(), &path);
    catalog.set_rating(&rel, rating).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_label(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
    label: Option<String>,
) -> Result<(), String> {
    let rel = rel_of(&state.root.lock().clone(), &path);
    catalog.set_label(&rel, label).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_flag(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
    flag: Option<String>,
) -> Result<(), String> {
    let rel = rel_of(&state.root.lock().clone(), &path);
    catalog.set_flag(&rel, flag).map_err(|e| e.to_string())
}

fn rels_for(state: &State<'_, AppState>, paths: &[String]) -> Vec<String> {
    let root = state.root.lock().clone();
    paths.iter().map(|p| rel_of(&root, p)).collect()
}

#[tauri::command]
pub fn set_rating_many(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    rating: i64,
) -> Result<(), String> {
    catalog
        .set_rating_many(&rels_for(&state, &paths), rating)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_label_many(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    label: Option<String>,
) -> Result<(), String> {
    catalog
        .set_label_many(&rels_for(&state, &paths), label)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_flag_many(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    flag: Option<String>,
) -> Result<(), String> {
    catalog
        .set_flag_many(&rels_for(&state, &paths), flag)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_tag(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    tag: String,
) -> Result<(), String> {
    let tag = tag.trim().to_string();
    if tag.is_empty() {
        return Ok(());
    }
    catalog
        .add_tag_many(&rels_for(&state, &paths), &tag)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_tag(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    tag: String,
) -> Result<(), String> {
    catalog
        .remove_tag_many(&rels_for(&state, &paths), &tag)
        .map_err(|e| e.to_string())
}

/// Distinct tags with usage counts, for the filter UI.
#[tauri::command]
pub fn list_tags(catalog: State<'_, Catalog>) -> Vec<(String, i64)> {
    catalog.all_tags()
}

// ── video trim (in/out points + lossless cut) ───────────────────────────────

#[tauri::command]
pub fn get_trim(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
) -> Option<(f64, f64)> {
    let rel = rel_of(&state.root.lock().clone(), &path);
    catalog.get_trim(&rel)
}

#[tauri::command]
pub fn set_trim(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
    in_s: f64,
    out_s: f64,
) -> Result<(), String> {
    let rel = rel_of(&state.root.lock().clone(), &path);
    catalog.set_trim(&rel, in_s, out_s).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_trim(state: State<'_, AppState>, catalog: State<'_, Catalog>, path: String) {
    let rel = rel_of(&state.root.lock().clone(), &path);
    catalog.clear_trim(&rel);
}

/// Export a lossless cut of `path` between in/out (seconds) next to the original
/// as `<name>_cut.<ext>` (uniquified). No re-encode. Returns the new file path.
#[tauri::command]
pub async fn trim_video(
    state: State<'_, AppState>,
    path: String,
    in_s: f64,
    out_s: f64,
) -> Result<String, String> {
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg not available")?;
    let root = canonical_active_root(&state.root.lock().clone())?;
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let src = validate_active_media_file(&root, lib.as_ref(), &path)?;
    if !matches!(media::classify(&src), Kind::Video) {
        return Err("not a video file".into());
    }
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "clip".into());
    let ext = src
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "mp4".into());
    // Pick a non-colliding destination in the same folder.
    let mut dest = src.with_file_name(format!("{stem}_cut.{ext}"));
    let mut n = 2;
    while dest.exists() {
        dest = src.with_file_name(format!("{stem}_cut{n}.{ext}"));
        n += 1;
    }
    tauri::async_runtime::spawn_blocking(move || {
        video::trim(&ffmpeg, &src, in_s, out_s, &dest)
            .map(|_| dest.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Already-built H.264 proxy for a clip, if one is cached — lets the player
/// fall back to it instantly on a decode failure without asking the user again.
// ── quick editor export (timeline/crop/music) ───────────────────────────────

#[derive(Serialize)]
pub struct SegmentExportOutcome {
    pub exported: Vec<String>,
    pub failed: Vec<String>,
    pub errors: Vec<String>,
}

fn clean_segments(segments: Vec<VideoSegment>) -> Vec<VideoSegment> {
    let mut out: Vec<VideoSegment> = segments
        .into_iter()
        .filter(|s| s.in_s.is_finite() && s.out_s.is_finite() && s.out_s > s.in_s)
        .map(|s| VideoSegment {
            in_s: s.in_s.max(0.0),
            out_s: s.out_s.max(0.0),
        })
        .collect();
    out.sort_by(|a, b| a.in_s.partial_cmp(&b.in_s).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn subclip_output_path(src: &Path, idx: usize) -> PathBuf {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "clip".into());
    let ext = src
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "mp4".into());
    uniquify(src.with_file_name(format!("{stem}_sub{idx:02}.{ext}")))
}

#[tauri::command]
pub fn get_video_segments(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
) -> Vec<VideoSegment> {
    let rel = rel_of(&state.root.lock().clone(), &path);
    catalog.get_video_segments(&rel)
}

#[tauri::command]
pub fn set_video_segments(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
    segments: Vec<VideoSegment>,
) -> Result<(), String> {
    let root = canonical_active_root(&state.root.lock().clone())?;
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let src = validate_active_media_file(&root, lib.as_ref(), &path)?;
    if !matches!(media::classify(&src), Kind::Video) {
        return Err("not a video file".into());
    }
    let rel = rel_under(&root, &src);
    catalog
        .set_video_segments(&rel, &clean_segments(segments))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_video_segments(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    path: String,
    segments: Vec<VideoSegment>,
) -> Result<SegmentExportOutcome, String> {
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg not available")?;
    let root = canonical_active_root(&state.root.lock().clone())?;
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let src = validate_active_media_file(&root, lib.as_ref(), &path)?;
    if !matches!(media::classify(&src), Kind::Video) {
        return Err("not a video file".into());
    }
    let segments = clean_segments(segments);
    if segments.is_empty() {
        return Err("no marked subclips to export".into());
    }
    let rel = rel_under(&root, &src);
    catalog
        .set_video_segments(&rel, &segments)
        .map_err(|e| e.to_string())?;

    tauri::async_runtime::spawn_blocking(move || {
        let total = segments.len() as u64;
        let label = format!("Exporting {total} subclip{}", if total == 1 { "" } else { "s" });
        emit_activity(&app, "subclips", &label, 0, total, "running");
        let mut out = SegmentExportOutcome {
            exported: Vec::new(),
            failed: Vec::new(),
            errors: Vec::new(),
        };
        for (i, segment) in segments.iter().enumerate() {
            let dest = subclip_output_path(&src, i + 1);
            match video::trim(&ffmpeg, &src, segment.in_s, segment.out_s, &dest) {
                Ok(()) => out.exported.push(dest.to_string_lossy().to_string()),
                Err(e) => {
                    out.failed.push(src.to_string_lossy().to_string());
                    out.errors.push(e);
                }
            }
            emit_activity(&app, "subclips", &label, i as u64 + 1, total, "running");
        }
        emit_activity(&app, "subclips", &label, total, total, "done");
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Deserialize)]
pub struct EditClip {
    pub path: String,
    pub in_s: f64,
    pub out_s: f64,
    /// 0 = left/top, 0.5 = centered, 1 = right/bottom within the cropable range.
    pub crop_x: f64,
    pub crop_y: f64,
    /// 1 = widest crop that fills the target aspect, >1 pushes in.
    pub zoom: f64,
}

#[derive(Clone, Deserialize)]
pub struct EditAdjustments {
    /// ffmpeg eq brightness, roughly -0.25..0.25.
    pub brightness: f64,
    /// ffmpeg eq contrast multiplier, 1.0 = neutral.
    pub contrast: f64,
    /// ffmpeg eq saturation multiplier, 1.0 = neutral.
    pub saturation: f64,
    /// Warm/cool tint, roughly -0.5..0.5.
    pub warmth: f64,
    /// Unsharp amount, 0 = off.
    pub sharpen: f64,
    /// Orange & teal split-tone strength, 0 = off. Defaulted so requests built
    /// before this field existed still deserialize.
    #[serde(default, rename = "splitTone")]
    pub split_tone: f64,
}

#[derive(Deserialize)]
pub struct EditExportRequest {
    pub clips: Vec<EditClip>,
    /// Output canvas. 1080x1920 is the Instagram/Reels preset; 0x0 means original.
    pub output_w: u32,
    pub output_h: u32,
    /// "crop" fills the output canvas; "original" keeps pixels and enables lossless paths.
    pub fit: String,
    /// "auto" tries NVIDIA NVENC first, then x264; "x264" and "nvenc" force one.
    pub encoder: String,
    /// "best" | "high" | "standard" | "small".
    pub quality: String,
    pub adjustments: EditAdjustments,
    /// Optional music bed. When present it replaces source audio and is cut to video length.
    pub music_path: Option<String>,
    pub preserve_source_audio: bool,
    pub destination: Option<String>,
    pub basename: Option<String>,
    /// Instagram/social normalisation: tone-map HDR sources down to SDR Rec.709
    /// (the resolution downscale is already handled by output_w/h).
    #[serde(default)]
    pub normalize: bool,
    /// Target frame rate. The frontend sends the source fps capped at 60
    /// (Instagram supports 60 fps and drone/action footage is shot at it — never
    /// silently halve it to 30). None/0 = keep source timing untouched.
    #[serde(default)]
    pub fps: Option<u32>,
    /// When normalising, preserve HDR (HLG) instead of tone-mapping to SDR —
    /// outputs 10-bit HEVC with HLG tags. Falls back to SDR if that encode fails.
    #[serde(default)]
    pub keep_hdr: bool,
}

#[derive(Deserialize)]
pub struct EditSnapshotRequest {
    pub path: String,
    pub time_s: f64,
    pub output_w: u32,
    pub output_h: u32,
    pub fit: String,
    pub crop_x: f64,
    pub crop_y: f64,
    pub zoom: f64,
    pub adjustments: EditAdjustments,
    pub basename: Option<String>,
}

#[derive(Serialize)]
pub struct EditExportOutcome {
    pub path: String,
    pub mode: String,
    pub reencoded: bool,
}

fn clean_name(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "foxcull_edit".into()
    } else {
        out
    }
}

fn neutral_adjustments(a: &EditAdjustments) -> bool {
    a.brightness.abs() < 0.001
        && (a.contrast - 1.0).abs() < 0.001
        && (a.saturation - 1.0).abs() < 0.001
        && a.warmth.abs() < 0.001
        && a.sharpen.abs() < 0.001
        && a.split_tone.abs() < 0.001
}

// The colour block below is duplicated between the video pipeline and the
// single-frame snapshot path, so the three primitives that must stay pixel-for-
// pixel identical to the EditStudio preview live here.

// Brightness is a multiplicative gain (1+b), matching CSS `brightness(1+b)`.
// Folding it into eq's contrast+brightness reproduces `brightness(1+b) contrast(c)`
// exactly: out = ((in*(1+b)) - 0.5)*c + 0.5 == contrast=(1+b)*c, brightness=0.5*c*b.
fn eq_folded(a: &EditAdjustments) -> String {
    let b = a.brightness.clamp(-0.5, 0.5);
    let c = a.contrast.clamp(0.2, 3.0);
    let g = 1.0 + b;
    format!(
        "eq=brightness={:.4}:contrast={:.4}:saturation={:.4}",
        0.5 * c * b,
        (g * c).clamp(0.0, 3.0),
        a.saturation.clamp(0.0, 3.0)
    )
}

// Warmth as a per-channel gain (red up / blue down for warm) so it mirrors the
// preview's SVG feColorMatrix instead of the old shadows/mids colorbalance shift.
// The 0.5 coefficient (a ±25% R/B swing at full range) must stay identical to the
// preview's lookMatrix in EditStudio.svelte — that parity is the whole point.
fn warmth_filter(w: f64) -> Option<String> {
    if w.abs() < 0.001 {
        return None;
    }
    let w = w.clamp(-0.5, 0.5);
    Some(format!(
        "colorchannelmixer=rr={:.4}:gg=1.0:bb={:.4}",
        1.0 + 0.5 * w,
        1.0 - 0.5 * w
    ))
}

// Orange & teal split-tone: warm highlights, cool/teal shadows. The control
// points mirror the preview's feComponentTransfer tableValues; ffmpeg splines
// them where the SVG ramps linearly, a negligible difference at these deltas.
fn splittone_filter(a: f64) -> Option<String> {
    if a < 0.001 {
        return None;
    }
    let a = a.clamp(0.0, 1.5);
    let xs = [0.0, 0.25, 0.5, 0.75, 1.0];
    // These deltas must stay identical to lookSplit in EditStudio.svelte so the
    // exported orange/teal split matches the preview channel-for-channel.
    let rd = [-0.08, -0.03, 0.05, 0.13, 0.09];
    let gd = [0.04, 0.02, 0.0, -0.03, -0.05];
    let bd = [0.13, 0.07, 0.0, -0.07, -0.12];
    let pts = |d: &[f64; 5]| -> String {
        xs.iter()
            .zip(d)
            .map(|(x, dv)| format!("{x:.3}/{:.4}", (x + a * dv).clamp(0.0, 1.0)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    Some(format!(
        "curves=r='{}':g='{}':b='{}'",
        pts(&rd),
        pts(&gd),
        pts(&bd)
    ))
}

fn edit_requires_reencode(req: &EditExportRequest) -> bool {
    req.normalize
        || req.fit != "original"
        || req.output_w > 0
        || req.output_h > 0
        || !neutral_adjustments(&req.adjustments)
        || req.music_path.as_ref().is_some_and(|p| !p.trim().is_empty())
        // An explicit fps target can't be stream-copied.
        || req.fps.is_some_and(|f| f > 0)
}

fn clip_duration(c: &EditClip) -> Result<f64, String> {
    if !c.in_s.is_finite() || !c.out_s.is_finite() {
        return Err("clip has invalid in/out points".into());
    }
    if c.out_s <= c.in_s {
        return Err("clip out point must be after its in point".into());
    }
    Ok(c.out_s - c.in_s)
}

fn default_edit_dest(req: &EditExportRequest) -> Result<PathBuf, String> {
    let first = req
        .clips
        .first()
        .ok_or_else(|| "nothing to export".to_string())?;
    if let Some(d) = req.destination.as_ref().filter(|d| !d.trim().is_empty()) {
        return Ok(PathBuf::from(d));
    }
    Ok(Path::new(&first.path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".")))
}

fn edit_output_path(req: &EditExportRequest, reencode: bool) -> Result<PathBuf, String> {
    let first = req
        .clips
        .first()
        .ok_or_else(|| "nothing to export".to_string())?;
    let dest_dir = default_edit_dest(req)?;
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let stem = req
        .basename
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(clean_name)
        .or_else(|| {
            Path::new(&first.path)
                .file_stem()
                .map(|s| format!("{}_edit", clean_name(&s.to_string_lossy())))
        })
        .unwrap_or_else(|| "foxcull_edit".into());
    let ext = if reencode {
        "mp4".to_string()
    } else {
        Path::new(&first.path)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_else(|| "mp4".into())
    };
    Ok(uniquify(dest_dir.join(format!("{stem}.{ext}"))))
}

fn concat_list_escape(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace('\'', "'\\''")
}

fn run_ffmpeg(mut cmd: Command) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("ffmpeg export failed");
        Err(msg.to_string())
    }
}

/// Sentinel error for a user-cancelled export — callers must NOT retry/fall back
/// past it (a cancel should end the whole ladder, not trigger the x264 retry).
pub const EXPORT_CANCELLED: &str = "export cancelled";

/// Progress + cancellation context for a watched export run.
struct ExportWatch {
    app: AppHandle,
    /// Activity id the run reports under: "edit-export" or "merge".
    job: &'static str,
    /// Edit exports: cancelled when `export_gen` moves past the run's own value.
    gen: Option<(Arc<AtomicU64>, u64)>,
    /// Merges: their own stop flag (`cancel_job("merge")`), so starting an Edit
    /// export can't kill a merge that's running in the background.
    flag: Option<Arc<AtomicBool>>,
    label: String,
    /// Second line in the job centre ("Converting clip 3 of 12").
    detail: Option<String>,
    /// Total output seconds (sum of clip durations) — drives the percentage.
    total_s: f64,
    /// The slice of the whole job's 0-100 this run covers: a convert-merge
    /// runs one ffmpeg per clip and each reports only its own share.
    base_pct: f64,
    span_pct: f64,
    /// Size the finished file should come to (a lossless merge: the clips'
    /// sizes), so the job centre can say "12.4 of 44.1 GB · 410 MB/s".
    /// 0 = don't show sizes.
    expect_bytes: u64,
    started: Instant,
}

impl ExportWatch {
    fn cancelled(&self) -> bool {
        self.gen.as_ref().is_some_and(|(g, mine)| g.load(Ordering::SeqCst) != *mine)
            || self.flag.as_ref().is_some_and(|f| f.load(Ordering::SeqCst))
    }
    fn emit(&self, pct: u64, state: &str) {
        self.emit_with(pct, state, 0);
    }
    /// `written`: bytes ffmpeg has written so far (its `total_size`), 0 if unknown.
    fn emit_with(&self, pct: u64, state: &str, written: u64) {
        let paused = self.job == "merge" && state == "running" && crate::procs::merge_paused();
        let detail = if paused {
            Some("Paused".to_string())
        } else if self.expect_bytes > 0 && written > 0 && state == "running" {
            let secs = self.started.elapsed().as_secs_f64().max(0.001);
            Some(format!(
                "{} of {} · {}/s",
                fmt_bytes(written),
                fmt_bytes(self.expect_bytes),
                fmt_bytes((written as f64 / secs) as u64)
            ))
        } else {
            self.detail.clone()
        };
        emit_job(
            &self.app,
            Activity {
                id: self.job.to_string(),
                label: self.label.clone(),
                done: pct,
                total: 100,
                state: state.to_string(),
                detail: detail.clone(),
                unit: None,
                cancellable: state == "running",
                paused,
                path: None,
            },
        );
        if self.job == "merge" && state == "running" {
            let mut m = MERGE.lock();
            m.pct = pct;
            m.detail = detail;
        }
        if self.job == "edit-export" {
            let _ = self.app.emit("export-progress", pct);
        }
    }
}

/// A file's name for a message ("DJI_0123.MP4"), or the path if it has none.
fn file_label(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

/// "830 MB", "12.4 GB" — decimal units, as Finder and Explorer's drive sizes.
fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", (b / 1e3).max(1.0))
    }
}

/// Like `run_ffmpeg`, but streams `-progress pipe:1` (the caller must add those
/// args before the output path), emits percentage events, and kills the child +
/// deletes the partial `dest` when the export generation is bumped (cancel).
/// stderr is drained on a side thread so a chatty ffmpeg can't deadlock the pipe.
fn run_ffmpeg_watched(mut cmd: Command, watch: Option<&ExportWatch>, dest: &Path) -> Result<(), String> {
    let Some(w) = watch else {
        return run_ffmpeg(cmd);
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    // Reachable by Pause (a merge) and by Quit (kill it, delete `dest`).
    let token = crate::procs::register(&child, Some(dest), w.job == "merge");
    let stderr_thread = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = String::new();
            let _ = err.read_to_string(&mut buf);
            buf
        })
    });
    let mut cancelled = false;
    if let Some(stdout) = child.stdout.take() {
        use std::io::BufRead;
        let mut last = u64::MAX;
        let mut written = 0u64;
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if w.cancelled() {
                cancelled = true;
                let _ = child.kill();
                break;
            }
            // Each progress block lists total_size= before out_time_us=.
            if let Some(v) = line.strip_prefix("total_size=") {
                written = v.trim().parse().unwrap_or(written);
            }
            if w.total_s > 0.0 {
                if let Some(v) = line.strip_prefix("out_time_us=") {
                    if let Ok(us) = v.trim().parse::<f64>() {
                        let pct = (w.base_pct + (us / 1_000_000.0 / w.total_s).clamp(0.0, 1.0) * w.span_pct) as u64;
                        if pct != last {
                            last = pct;
                            w.emit_with(pct, "running", written);
                        }
                    }
                }
            }
        }
    }
    let status = child.wait();
    crate::procs::unregister(token);
    let status = status.map_err(|e| e.to_string())?;
    let err_text = stderr_thread
        .and_then(|t| t.join().ok())
        .unwrap_or_default();
    if cancelled || w.cancelled() {
        let _ = std::fs::remove_file(dest);
        return Err(EXPORT_CANCELLED.into());
    }
    if status.success() {
        Ok(())
    } else {
        let msg = err_text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("ffmpeg export failed");
        Err(msg.to_string())
    }
}

/// Stream-copy concat requires every clip to share one frame size AND codec —
/// mixing resolutions (an FHD Mavic clip + a 4K Osmo clip) or codecs (an S23
/// HEVC clip + a Mavic H.264 clip) stream-copies into a broken/glitchy file.
/// Probe multi-clip lossless exports and force the re-encode path when dims or
/// codec differ; 0×0 dims / unknown codec are treated as unknown/compatible so
/// lossless stays the default.
fn concat_needs_reencode(ffmpeg: &Path, req: &EditExportRequest) -> bool {
    if req.clips.len() < 2 {
        return false;
    }
    let mut dims: Option<(u32, u32)> = None;
    let mut codecs: Option<String> = None;
    for clip in &req.clips {
        let (w, h, _, _, codec, _) = clip_probe(ffmpeg, Path::new(&clip.path));
        if w > 0 && h > 0 {
            match dims {
                Some(d) if d != (w, h) => return true,
                Some(_) => {}
                None => dims = Some((w, h)),
            }
        }
        // Codec matters too: e.g. an S23 HEVC clip + a Mavic H.264 clip at the
        // same 1920×1080 still stream-copy into a broken file.
        if let Some(c) = codec {
            match &codecs {
                Some(seen) if *seen != c => return true,
                Some(_) => {}
                None => codecs = Some(c),
            }
        }
    }
    false
}

fn export_lossless_concat(
    ffmpeg: &Path,
    req: &EditExportRequest,
    dest: &Path,
    watch: Option<&ExportWatch>,
) -> Result<(), String> {
    // Unique per run: keying only on whole-second `now()` let two overlapping
    // exports (or a rapid cancel-then-restart) share one concat list and clobber
    // each other. Add pid + a process-wide sequence.
    static CONCAT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = CONCAT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let list_path = std::env::temp_dir().join(format!(
        "foxcull-concat-{}-{}-{}.txt",
        std::process::id(),
        now(),
        seq
    ));
    let mut f = std::fs::File::create(&list_path).map_err(|e| e.to_string())?;
    for clip in &req.clips {
        clip_duration(clip)?;
        writeln!(f, "file '{}'", concat_list_escape(Path::new(&clip.path))).map_err(|e| e.to_string())?;
        writeln!(f, "inpoint {:.6}", clip.in_s.max(0.0)).map_err(|e| e.to_string())?;
        writeln!(f, "outpoint {:.6}", clip.out_s).map_err(|e| e.to_string())?;
    }
    drop(f);

    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args(["-c", "copy", "-avoid_negative_ts", "make_zero"])
        .args(["-progress", "pipe:1", "-nostats", "-y"])
        .arg(dest);
    let res = run_ffmpeg_watched(cmd, watch, dest);
    let _ = std::fs::remove_file(&list_path);
    res
}

fn export_lossless_single(ffmpeg: &Path, req: &EditExportRequest, dest: &Path) -> Result<(), String> {
    let clip = req.clips.first().ok_or_else(|| "nothing to export".to_string())?;
    clip_duration(clip)?;
    video::trim(ffmpeg, Path::new(&clip.path), clip.in_s, clip.out_s, dest)
}

fn quality_args(encoder: &str, quality: &str) -> Vec<String> {
    let crf = match quality {
        "best" => "16",
        "standard" => "20",
        "small" => "23",
        _ => "18",
    };
    if encoder == "nvenc" {
        let cq = match quality {
            "best" => "16",
            "standard" => "20",
            "small" => "24",
            _ => "18",
        };
        vec![
            "-c:v".into(),
            "h264_nvenc".into(),
            "-preset".into(),
            "p5".into(),
            "-cq".into(),
            cq.into(),
            "-b:v".into(),
            "0".into(),
        ]
    } else {
        vec![
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            crf.into(),
        ]
    }
}

/// HDR-passthrough encoder: 10-bit HEVC in BT.2020, tagged with the SOURCE's
/// transfer function — HLG (Osmo Pocket 3) stays HLG, PQ/HDR10 (S23 Ultra
/// HDR10+) stays PQ. Blanket-tagging PQ footage as HLG decodes with the wrong
/// gamma (dim/washed). `hvc1` tag keeps it playable in QuickTime/Instagram.
fn hdr_encoder_args(pq: bool) -> Vec<String> {
    let trc = if pq { "smpte2084" } else { "arib-std-b67" };
    [
        "-c:v", "libx265", "-preset", "medium", "-crf", "20",
        "-pix_fmt", "yuv420p10le", "-tag:v", "hvc1",
        "-color_primaries", "bt2020", "-color_trc", trc, "-colorspace", "bt2020nc",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The CPU tone-map chain (PQ/HLG → SDR Rec.709, filmic `hable`). Standard zscale
/// path; if the bundled ffmpeg lacks zscale the caller falls back to no tone-map.
const TONEMAP_CHAIN: &[&str] = &[
    "zscale=transfer=linear:npl=100",
    "format=gbrpf32le",
    "zscale=primaries=bt709",
    "tonemap=tonemap=hable:desat=0",
    "zscale=transfer=bt709:matrix=bt709:range=tv",
    "format=yuv420p",
];

fn build_video_filter(
    req: &EditExportRequest,
    hdr_flags: &[bool],
    src_dims: &[(u32, u32)],
    conform: Option<(u32, u32)>,
    with_audio: bool,
    pix10: bool,
) -> Result<String, String> {
    let mut chains = Vec::new();
    let mut labels = Vec::new();
    let crop = req.fit != "original" && req.output_w > 0 && req.output_h > 0;
    let aspect = if crop {
        req.output_w as f64 / req.output_h as f64
    } else {
        1.0
    };
    for (i, clip) in req.clips.iter().enumerate() {
        clip_duration(clip)?;
        let mut filters = Vec::new();
        // HDR → SDR first, before any spatial/colour work, so the rest of the
        // chain operates on Rec.709.
        if req.normalize && hdr_flags.get(i).copied().unwrap_or(false) {
            for f in TONEMAP_CHAIN {
                filters.push((*f).to_string());
            }
        }
        if crop {
            let x = clip.crop_x.clamp(0.0, 1.0);
            let y = clip.crop_y.clamp(0.0, 1.0);
            let zoom = clip.zoom.clamp(1.0, 4.0);
            filters.push(format!(
                "crop=w='trunc(min(iw,ih*{aspect:.8})/{zoom:.6}/2)*2':h='trunc(min(ih,iw/{aspect:.8})/{zoom:.6}/2)*2':x='min(max((iw-ow)*{x:.6},0),iw-ow)':y='min(max((ih-oh)*{y:.6},0),ih-oh)'"
            ));
            filters.push(format!("scale={}:{}:flags=lanczos", req.output_w, req.output_h));
            // Soft-crop recovery: a vertical crop of a low-res landscape (e.g. a
            // 1080p Mavic/S23 clip) contains fewer pixels than 1080 wide and has
            // to upscale. A mild unsharp is a cheap way to claw back perceived
            // crispness (AI upscaling isn't worth it — Instagram recompresses it
            // away). Only when the user hasn't dialled their own sharpen.
            let (sw, sh) = src_dims.get(i).copied().unwrap_or((0, 0));
            if sw > 0 && sh > 0 && req.adjustments.sharpen < 0.01 {
                let crop_w = (sw as f64).min(sh as f64 * aspect);
                if crop_w + 1.0 < req.output_w as f64 {
                    filters.push("unsharp=5:5:0.5:5:5:0.0".into());
                }
            }
        } else if let Some((cw, ch)) = conform {
            // Mixed-resolution composite (fit=original, >1 clip): concat needs every
            // frame at one identical size. Fit each clip into the shared canvas
            // (largest-area clip's dims) preserving aspect, then letterbox-pad the
            // remainder so nothing is cropped and small clips aren't blown up past
            // their own resolution beyond the fit.
            filters.push(format!(
                "scale={cw}:{ch}:force_original_aspect_ratio=decrease:force_divisible_by=2:flags=lanczos"
            ));
            filters.push(format!("pad={cw}:{ch}:(ow-iw)/2:(oh-ih)/2:black"));
        }
        let a = &req.adjustments;
        if !neutral_adjustments(a) {
            filters.push(eq_folded(a));
            if let Some(f) = warmth_filter(a.warmth) {
                filters.push(f);
            }
            if let Some(f) = splittone_filter(a.split_tone) {
                filters.push(f);
            }
            if a.sharpen > 0.001 {
                filters.push(format!("unsharp=5:5:{:.3}", a.sharpen.clamp(0.0, 1.5)));
            }
        }
        // Frame rate: the frontend sends the target (source fps capped at 60 —
        // Instagram accepts up to 60 fps and 60 uploads play smoother than a
        // pre-halved 30). Multi-clip composites get one common rate so concat
        // stays consistent. Legacy fallback (no fps in the request): 30 only for
        // normalised exports, source timing otherwise.
        if let Some(f) = req.fps.filter(|f| *f > 0) {
            filters.push(format!("fps={}", f.min(60)));
        } else if req.normalize {
            filters.push("fps=30".into());
        }
        filters.push("setsar=1".into());
        filters.push(if pix10 { "format=yuv420p10le".into() } else { "format=yuv420p".to_string() });
        let label = if req.clips.len() == 1 {
            "vout".to_string()
        } else {
            format!("v{i}")
        };
        labels.push(label.clone());
        chains.push(format!("[{i}:v]{}[{label}]", filters.join(",")));
        // Per-clip audio chain, normalised to a common rate/format/layout so the
        // interleaved concat below accepts them.
        if with_audio {
            chains.push(format!(
                "[{i}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"
            ));
        }
    }
    if req.clips.len() > 1 {
        if with_audio {
            // Interleave [v0][a0][v1][a1]… and concat both streams together.
            let inputs = (0..req.clips.len())
                .map(|i| format!("[v{i}][a{i}]"))
                .collect::<String>();
            chains.push(format!(
                "{inputs}concat=n={}:v=1:a=1[vout][aout]",
                req.clips.len()
            ));
        } else {
            let inputs = labels.iter().map(|l| format!("[{l}]")).collect::<String>();
            chains.push(format!("{inputs}concat=n={}:v=1:a=0[vout]", req.clips.len()));
        }
    }
    Ok(chains.join(";"))
}

fn build_single_frame_filter(
    output_w: u32,
    output_h: u32,
    fit: &str,
    crop_x: f64,
    crop_y: f64,
    zoom: f64,
    adjustments: &EditAdjustments,
) -> String {
    let mut filters = Vec::new();
    let crop = fit != "original" && output_w > 0 && output_h > 0;
    if crop {
        let aspect = output_w as f64 / output_h as f64;
        let x = crop_x.clamp(0.0, 1.0);
        let y = crop_y.clamp(0.0, 1.0);
        let zoom = zoom.clamp(1.0, 4.0);
        filters.push(format!(
            "crop=w='trunc(min(iw,ih*{aspect:.8})/{zoom:.6}/2)*2':h='trunc(min(ih,iw/{aspect:.8})/{zoom:.6}/2)*2':x='min(max((iw-ow)*{x:.6},0),iw-ow)':y='min(max((ih-oh)*{y:.6},0),ih-oh)'"
        ));
        filters.push(format!("scale={output_w}:{output_h}:flags=lanczos"));
    }
    if !neutral_adjustments(adjustments) {
        filters.push(eq_folded(adjustments));
        if let Some(f) = warmth_filter(adjustments.warmth) {
            filters.push(f);
        }
        if let Some(f) = splittone_filter(adjustments.split_tone) {
            filters.push(f);
        }
        if adjustments.sharpen > 0.001 {
            filters.push(format!("unsharp=5:5:{:.3}", adjustments.sharpen.clamp(0.0, 1.5)));
        }
    }
    filters.push("setsar=1".into());
    filters.join(",")
}

fn snapshot_output_path(req: &EditSnapshotRequest, src: &Path) -> PathBuf {
    let stem = req
        .basename
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(clean_name)
        .or_else(|| {
            src.file_stem()
                .map(|s| format!("{}_frame", clean_name(&s.to_string_lossy())))
        })
        .unwrap_or_else(|| "foxcull_frame".into());
    let dest_dir = src.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    uniquify(dest_dir.join(format!("{stem}.jpg")))
}

fn export_snapshot(ffmpeg: &Path, req: &EditSnapshotRequest, src: &Path, dest: &Path) -> Result<(), String> {
    let mut cmd = Command::new(ffmpeg);
    let vf = build_single_frame_filter(
        req.output_w,
        req.output_h,
        &req.fit,
        req.crop_x,
        req.crop_y,
        req.zoom,
        &req.adjustments,
    );
    cmd.args(["-v", "error", "-ss"])
        .arg(format!("{:.6}", req.time_s.max(0.0)))
        .arg("-i")
        .arg(src);
    if !vf.is_empty() {
        cmd.args(["-vf", &vf]);
    }
    cmd.args(["-frames:v", "1", "-q:v", "2", "-y"])
        .arg(dest)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    run_ffmpeg(cmd)
}

fn export_reencoded(
    ffmpeg: &Path,
    req: &EditExportRequest,
    dest: &Path,
    encoder: &str,
    tonemap: bool,
    passthrough: bool,
    watch: Option<&ExportWatch>,
) -> Result<(), String> {
    let has_music = req.music_path.as_ref().is_some_and(|p| !p.trim().is_empty());
    // Probe source dims + HDR + audio when they can affect the filtergraph:
    // normalising, a crop that might upscale, any multi-clip composite (needs a
    // shared canvas), or multi-clip source-audio preservation. One cheap `-i` per
    // clip.
    let need_probe = req.normalize
        || (req.fit != "original" && req.output_w > 0)
        || req.clips.len() > 1
        || (req.preserve_source_audio && !has_music);
    let probes: Vec<(u32, u32, bool, bool, Option<String>, bool)> = if need_probe {
        req.clips.iter().map(|c| clip_probe(ffmpeg, Path::new(&c.path))).collect()
    } else {
        vec![(0, 0, false, false, None, false); req.clips.len()]
    };
    // Tone-map HDR→SDR only when normalising, tone-mapping this attempt, and NOT
    // keeping HDR (passthrough). The caller retries with tonemap=false if the
    // tone-map filter is unavailable.
    let hdr_flags: Vec<bool> = if req.normalize && tonemap && !passthrough {
        probes.iter().map(|p| p.2).collect()
    } else {
        vec![false; req.clips.len()]
    };
    let src_dims: Vec<(u32, u32)> = probes.iter().map(|p| (p.0, p.1)).collect();
    // Shared conform canvas for mixed-resolution composites: only when >1 clip AND
    // the crop path isn't already forcing everything to output_w×output_h. Size =
    // largest-area probed clip (even-rounded), fallback 1920×1080 if probes are 0.
    let crop_conforms = req.fit != "original" && req.output_w > 0 && req.output_h > 0;
    let conform: Option<(u32, u32)> = if req.clips.len() > 1 && !crop_conforms {
        let (cw, ch) = src_dims
            .iter()
            .filter(|(w, h)| *w > 0 && *h > 0)
            .max_by_key(|(w, h)| (*w as u64) * (*h as u64))
            .copied()
            .unwrap_or((1920, 1080));
        Some((cw & !1, ch & !1))
    } else {
        None
    };
    // Preserve source audio across a multi-clip re-encode only when there's no
    // music track, the user asked for it, and EVERY clip actually has audio (any
    // silent clip → fall back to a safe silent export).
    let multi_audio = !has_music
        && req.preserve_source_audio
        && req.clips.len() > 1
        && probes.iter().all(|p| p.3);
    let mut cmd = Command::new(ffmpeg);
    for clip in &req.clips {
        let dur = clip_duration(clip)?;
        cmd.arg("-ss")
            .arg(format!("{:.6}", clip.in_s.max(0.0)))
            .arg("-t")
            .arg(format!("{dur:.6}"))
            .arg("-i")
            .arg(&clip.path);
    }
    let music = req.music_path.as_ref().filter(|p| !p.trim().is_empty());
    if let Some(music_path) = music {
        cmd.args(["-stream_loop", "-1", "-i"]).arg(music_path);
    }
    let filter = build_video_filter(req, &hdr_flags, &src_dims, conform, multi_audio, passthrough)?;
    cmd.args(["-filter_complex", &filter, "-map", "[vout]"]);
    if music.is_some() {
        let music_index = req.clips.len();
        cmd.arg("-map").arg(format!("{music_index}:a:0")).arg("-shortest");
    } else if multi_audio {
        cmd.args(["-map", "[aout]"]);
    } else if req.preserve_source_audio && req.clips.len() == 1 {
        cmd.args(["-map", "0:a?"]);
    } else {
        cmd.arg("-an");
    }
    if passthrough {
        // Tag with the source's transfer (any PQ clip → PQ; else HLG).
        let pq = probes.iter().any(|p| p.5);
        for arg in hdr_encoder_args(pq) {
            cmd.arg(arg);
        }
    } else {
        for arg in quality_args(encoder, &req.quality) {
            cmd.arg(arg);
        }
        // Instagram ingest caps at VBR 25 Mbps — belt-and-braces so a grain- or
        // confetti-heavy CRF encode can never spike past what Meta accepts.
        if req.normalize {
            cmd.args(["-maxrate", "25M", "-bufsize", "50M"]);
        }
    }
    if music.is_some() || multi_audio || (req.preserve_source_audio && req.clips.len() == 1) {
        cmd.args(["-c:a", "aac", "-b:a", "192k"]);
    }
    cmd.args(["-movflags", "+faststart"])
        .args(["-progress", "pipe:1", "-nostats", "-y"])
        .arg(dest);
    run_ffmpeg_watched(cmd, watch, dest)
}

/// Run a re-encode, honouring the encoder choice (auto = NVENC then x264 on
/// failure). Returns the mode string. `tonemap` controls whether HDR clips are
/// tone-mapped this attempt.
fn reencode_pick_encoder(
    ffmpeg: &Path,
    req: &EditExportRequest,
    dest: &Path,
    tonemap: bool,
    watch: Option<&ExportWatch>,
) -> Result<String, String> {
    match req.encoder.as_str() {
        "auto" => match export_reencoded(ffmpeg, req, dest, "nvenc", tonemap, false, watch) {
            Ok(()) => Ok("reencoded-nvenc".into()),
            // A user cancel ends the ladder — never "retry" a cancelled export.
            Err(e) if e == EXPORT_CANCELLED => Err(e),
            Err(_) => {
                export_reencoded(ffmpeg, req, dest, "x264", tonemap, false, watch)?;
                Ok("reencoded-x264".into())
            }
        },
        "nvenc" => {
            export_reencoded(ffmpeg, req, dest, "nvenc", tonemap, false, watch)?;
            Ok("reencoded-nvenc".into())
        }
        _ => {
            export_reencoded(ffmpeg, req, dest, "x264", tonemap, false, watch)?;
            Ok("reencoded-x264".into())
        }
    }
}

#[tauri::command]
pub async fn edit_export(
    app: AppHandle,
    state: State<'_, AppState>,
    mut req: EditExportRequest,
) -> Result<EditExportOutcome, String> {
    if req.clips.is_empty() {
        return Err("nothing to export".into());
    }
    for clip in &mut req.clips {
        let p = validate_media_anywhere(&state, &clip.path).map_err(|e| {
            if !Path::new(&clip.path).exists() {
                format!("{} isn't there any more (was the card or drive removed?)", file_label(&clip.path))
            } else {
                format!("{}: {e}", file_label(&clip.path))
            }
        })?;
        if !matches!(media::classify(&p), Kind::Video) {
            return Err(format!("not a video file: {}", file_label(&clip.path)));
        }
        clip.path = p.to_string_lossy().to_string();
    }
    if let Some(path) = &mut req.music_path {
        let p = canonical_file(Path::new(path))?;
        if !is_audio_file(&p) {
            return Err("music track must be an audio file".into());
        }
        *path = p.to_string_lossy().to_string();
    }
    if let Some(dest) = &mut req.destination {
        let p = canonical_dir(Path::new(dest))?;
        *dest = p.to_string_lossy().to_string();
    }
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg not available")?;
    // New export claims the generation token — cancelling bumps it again.
    let my_gen = state.export_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let gen = state.export_gen.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let total_s: f64 = req
            .clips
            .iter()
            .map(|c| (c.out_s - c.in_s).max(0.0))
            .sum();
        let first_name = Path::new(&req.clips[0].path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "clip".into());
        let watch = ExportWatch {
            app,
            job: "edit-export",
            gen: Some((gen, my_gen)),
            flag: None,
            label: format!("Exporting {first_name}"),
            detail: None,
            total_s,
            base_pct: 0.0,
            span_pct: 100.0,
            expect_bytes: 0,
            started: Instant::now(),
        };
        watch.emit(0, "running");
        // Decide re-encode BEFORE picking the output path — it selects the
        // container extension. A "lossless" multi-clip export still has to
        // re-encode when the clips' resolutions don't match (stream-copy concat
        // would produce a broken file).
        let reencode =
            edit_requires_reencode(&req) || concat_needs_reencode(&ffmpeg, &req);
        let dest = match edit_output_path(&req, reencode) {
            Ok(d) => d,
            Err(e) => {
                emit_activity(&watch.app, "edit-export", &e, 0, 100, "error");
                return Err(e);
            }
        };
        let w = Some(&watch);
        let run = || -> Result<EditExportOutcome, String> {
            if reencode {
                let mode = if req.normalize && req.keep_hdr {
                    // Keep HDR: try 10-bit HEVC HLG passthrough. If that encode fails
                    // (no libx265 / HDR support), fall back to SDR so the export still
                    // succeeds — the mode string records what actually happened.
                    match export_reencoded(&ffmpeg, &req, &dest, "x264", false, true, w) {
                        Ok(()) => "reencoded-hdr".to_string(),
                        Err(e) if e == EXPORT_CANCELLED => return Err(e),
                        Err(_) => match reencode_pick_encoder(&ffmpeg, &req, &dest, true, w) {
                            Ok(m) => format!("{m}-sdr-fallback"),
                            Err(e) if e == EXPORT_CANCELLED => return Err(e),
                            Err(_) => reencode_pick_encoder(&ffmpeg, &req, &dest, false, w)?,
                        },
                    }
                } else {
                    // Try HDR tone-mapping first; if that attempt fails while
                    // normalising (e.g. the bundled ffmpeg lacks zscale), retry without
                    // it so the export still succeeds — just without HDR→SDR.
                    match reencode_pick_encoder(&ffmpeg, &req, &dest, true, w) {
                        Ok(m) => m,
                        Err(e) if e == EXPORT_CANCELLED => return Err(e),
                        Err(e) => {
                            if req.normalize {
                                reencode_pick_encoder(&ffmpeg, &req, &dest, false, w)?
                            } else {
                                return Err(e);
                            }
                        }
                    }
                };
                Ok(EditExportOutcome {
                    path: dest.to_string_lossy().to_string(),
                    mode,
                    reencoded: true,
                })
            } else {
                if req.clips.len() == 1 {
                    export_lossless_single(&ffmpeg, &req, &dest)?;
                } else {
                    export_lossless_concat(&ffmpeg, &req, &dest, w)?;
                }
                Ok(EditExportOutcome {
                    path: dest.to_string_lossy().to_string(),
                    mode: "stream-copy".into(),
                    reencoded: false,
                })
            }
        };
        let res = run();
        match &res {
            Ok(out) => {
                let name = Path::new(&out.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                emit_job(
                    &watch.app,
                    Activity {
                        id: "edit-export".into(),
                        label: format!("Exported {name}"),
                        done: 100,
                        total: 100,
                        state: "done".into(),
                        detail: Some(format!("{} in {}", fmt_bytes(std::fs::metadata(&out.path).map(|m| m.len()).unwrap_or(0)), fmt_secs(watch.started.elapsed().as_secs_f64()))),
                        path: Some(out.path.clone()),
                        ..Default::default()
                    },
                );
                let _ = watch.app.emit("export-progress", 100u64);
                let _ = watch.app.emit("media-output", out.path.clone());
            }
            // Terminal state reaches the frontend via the command result — only
            // the activity chip needs closing here.
            Err(e) if e == EXPORT_CANCELLED => {
                emit_activity(&watch.app, "edit-export", "Export cancelled", 100, 100, "done");
            }
            Err(e) => emit_activity(&watch.app, "edit-export", e, 0, 100, "error"),
        }
        res
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Cancel the in-flight edit export: the watched ffmpeg child is killed at the
/// next progress line and its partial output file deleted.
#[tauri::command]
pub fn cancel_edit_export(state: State<'_, AppState>) {
    state.export_gen.fetch_add(1, Ordering::SeqCst);
}

/// Does a path already exist? Used by the export dialog to warn that the chosen
/// name is taken (the export itself still auto-uniquifies, so nothing is ever
/// overwritten either way).
#[tauri::command]
pub fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
pub async fn edit_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
    mut req: EditSnapshotRequest,
) -> Result<String, String> {
    let src = validate_media_anywhere(&state, &req.path)?;
    if !matches!(media::classify(&src), Kind::Video) {
        return Err("not a video file".into());
    }
    req.path = src.to_string_lossy().to_string();
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg not available")?;
    let dest = snapshot_output_path(&req, &src);
    tauri::async_runtime::spawn_blocking(move || {
        export_snapshot(&ffmpeg, &req, &src, &dest)?;
        let out = dest.to_string_lossy().to_string();
        let _ = app.emit("media-output", out.clone());
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn video_proxy_cached(state: State<'_, AppState>, path: String) -> Option<String> {
    let cache_dir = state.cache_dir.lock().clone();
    let p = video::proxy_path(&cache_dir, Path::new(&path));
    p.exists().then(|| p.to_string_lossy().to_string())
}

/// Convert a clip the webview can't decode (HEVC without the OS codec) into a
/// cached H.264 preview via the bundled ffmpeg, reporting progress through the
/// activity indicator. One transcode runs at a time; the result is cached on
/// the drive so it's converted once, ever, per clip.
#[tauri::command]
pub async fn video_proxy(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let cache_dir = state.cache_dir.lock().clone();
    let ffmpeg = state.ffmpeg.clone();
    let src = PathBuf::from(&path);
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "clip".into());
    let id = format!("proxy:{path}");
    let label = format!("Converting {name} for playback");
    tauri::async_runtime::spawn_blocking(move || {
        emit_activity(&app, &id, &label, 0, 100, "running");
        let mut last = 0u64;
        let res = video::ensure_proxy(&cache_dir, ffmpeg.as_deref(), &src, |frac| {
            let pct = (frac * 100.0) as u64;
            if pct != last {
                last = pct;
                emit_activity(&app, &id, &label, pct, 100, "running");
            }
        });
        match &res {
            Ok(_) => emit_activity(&app, &id, &label, 100, 100, "done"),
            Err(e) => emit_activity(&app, &id, e, 0, 100, "error"),
        }
        res.map(|p| p.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── JPEG export (the RAW backlog path: keep a small JPEG, cull the 25 MB NEF) ─

#[derive(Serialize)]
pub struct ExportOutcome {
    /// RAW files exported via their embedded camera-rendered JPEG.
    pub exported: usize,
    /// Ordinary images copied through unchanged.
    pub copied: usize,
    /// Videos / unsupported files left out.
    pub skipped: usize,
    pub failed: Vec<String>,
    pub errors: Vec<String>,
    pub dest: String,
}

#[derive(Serialize)]
pub struct MoveRecord {
    pub from: String,
    pub to: String,
}

#[derive(Serialize)]
pub struct MoveOutcome {
    pub moved: usize,
    pub dest: String,
    pub files: Vec<MoveRecord>,
    pub failed: Vec<String>,
    pub errors: Vec<String>,
    /// The originals were kept (a copy, not a move).
    pub copied: bool,
    /// The destination is on another drive (so another catalog took the marks).
    pub cross_drive: bool,
    /// Stopped from the job centre; `files` lists what got through first.
    pub cancelled: bool,
}

const TRANSFER_CANCELLED: &str = "transfer cancelled";

/// Copy one file in 8 MB chunks, reporting bytes as they land, keeping its
/// modified/created times (capture-date fallbacks and the catalog's
/// (mtime, size) caches depend on them), and checking the size at the end.
/// `durable` flushes it to the device first: a move deletes the original right
/// after, so "copied" must mean on the disk, not in a write cache. Any failure
/// or a cancel removes the partial copy.
fn copy_file_progress(
    src: &Path,
    dst: &Path,
    durable: bool,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), String> {
    use std::io::Read;
    let mut input = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let meta = input.metadata().map_err(|e| e.to_string())?;
    // create_new: never write over a file that's already there, and only a
    // file this call created is ever cleaned up below.
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
        .map_err(|e| e.to_string())?;
    let res = (|| -> Result<(), String> {
        let mut buf = vec![0u8; 8 << 20];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(TRANSFER_CANCELLED.into());
            }
            let n = match input.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.to_string()),
            };
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            on_bytes(n as u64);
        }
        if durable {
            out.sync_all().map_err(|e| e.to_string())?;
        }
        let mut times = std::fs::FileTimes::new();
        if let Ok(m) = meta.modified() {
            times = times.set_modified(m);
        }
        if let Ok(a) = meta.accessed() {
            times = times.set_accessed(a);
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::macos::fs::FileTimesExt;
            if let Ok(c) = meta.created() {
                times = times.set_created(c);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTimesExt;
            if let Ok(c) = meta.created() {
                times = times.set_created(c);
            }
        }
        let _ = out.set_times(times);
        let len = out.metadata().map(|m| m.len()).map_err(|e| e.to_string())?;
        if len != meta.len() {
            return Err(format!("the copy came out at {len} bytes instead of {}", meta.len()));
        }
        Ok(())
    })();
    // Closed before any cleanup: Windows can't delete a file that's still open.
    drop(out);
    if res.is_err() {
        let _ = std::fs::remove_file(dst);
    }
    res
}

/// Same volume = a move is a rename (instant) and a copy can be a clone.
fn same_volume(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let first = |p: &Path| p.components().next().map(|c| c.as_os_str().to_string_lossy().to_lowercase());
        first(a).is_some() && first(a) == first(b)
    }
}

/// Never move files into FoxCull's own data or a place the folder tree hides
/// (another drive's `_FoxCull`, a Trash folder, `~/Library`, the OS folders):
/// the tree can't show those, so a drop there could only be a bug.
fn transfer_dest_allowed(state: &AppState, dest: &Path) -> Result<(), String> {
    if within(dest, &state.data_root) {
        return Err("that folder belongs to FoxCull".into());
    }
    match hidden_dest_component(dest) {
        Some(name) => Err(format!("FoxCull doesn't move files into “{name}”")),
        None => Ok(()),
    }
}

/// The first folder on the way to `dest` that the tree hides, if any. Checked
/// from the drive root down: `/Volumes` itself is hidden under `/` (it holds
/// the other drives), so the walk must start at the destination's own drive.
fn hidden_dest_component(dest: &Path) -> Option<String> {
    let droot = drive_root(&dest.to_string_lossy());
    let rest = dest.strip_prefix(&droot).unwrap_or(dest);
    let mut parent = droot.clone();
    for comp in rest.components() {
        if let Component::Normal(name) = comp {
            let name = name.to_string_lossy();
            if skip_dir(&parent, &name) || is_trash_dirname(&name) {
                return Some(name.to_string());
            }
            parent.push(name.as_ref());
        }
    }
    None
}

/// Where a move/copy goes and how. Built by `move_media_files`; kept apart
/// from Tauri so the transfer itself can be tested against real volumes.
struct TransferPlan {
    /// The active drive (where the sources are, and whose catalog has them).
    root: PathBuf,
    lib: Option<PathBuf>,
    cache_dir: PathBuf,
    dest_dir: PathBuf,
    /// The destination's drive: its catalog takes the records.
    dest_root: PathBuf,
    copy_mode: bool,
    cross_drive: bool,
}

/// Move or copy the files of a plan, reporting progress through `emit` (the
/// caller fills in the job id). Returns the outcome and the
/// (from rel, to rel) pairs for the catalog.
fn transfer_files(
    plan: &TransferPlan,
    paths: Vec<String>,
    cancel: &AtomicBool,
    emit: &dyn Fn(Activity),
) -> (MoveOutcome, Vec<(String, String)>) {
    let mut out = MoveOutcome {
        moved: 0,
        dest: plan.dest_dir.to_string_lossy().to_string(),
        files: Vec::new(),
        failed: Vec::new(),
        errors: Vec::new(),
        copied: plan.copy_mode,
        cross_drive: plan.cross_drive,
        cancelled: false,
    };
    // (from rel under the active root, to rel under the dest's root)
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    let mut srcs: Vec<PathBuf> = Vec::new();
    for p in paths {
        if !seen.insert(p.clone()) {
            continue;
        }
        match validate_active_media_file(&plan.root, plan.lib.as_ref(), &p) {
            Ok(src) if src.parent() == Some(plan.dest_dir.as_path()) && !plan.copy_mode => {
                out.failed.push(src.to_string_lossy().to_string());
                out.errors.push("already in that folder".into());
            }
            Ok(src) => srcs.push(src),
            Err(e) => {
                out.failed.push(p);
                out.errors.push(e);
            }
        }
    }
    // A move within one volume is a rename; everything else copies bytes.
    let renames = !plan.copy_mode && srcs.first().is_some_and(|s| same_volume(s, &plan.dest_dir));
    let mut total: u64 = if renames {
        0
    } else {
        srcs.iter().map(|s| std::fs::metadata(s).map(|m| m.len()).unwrap_or(0)).sum()
    };
    let n = srcs.len();
    let mut done: u64 = 0;
    let mut last_emit = Instant::now() - std::time::Duration::from_secs(1);
    let report = |done: u64, total: u64, i: usize, name: &str, force: bool, last: &mut Instant| {
        if !force && last.elapsed().as_millis() < 150 {
            return;
        }
        *last = Instant::now();
        emit(Activity {
            id: String::new(),
            label: String::new(), // the frontend owns the title
            done,
            total,
            state: "running".into(),
            detail: Some(format!("{} of {n} · {name}", i + 1)),
            unit: (total > 0).then_some("bytes"),
            cancellable: true,
            paused: false,
            path: None,
        });
    };
    for (i, src) in srcs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let Some(name) = src.file_name() else {
            out.failed.push(src.to_string_lossy().to_string());
            out.errors.push("file has no filename".into());
            continue;
        };
        let shown = name.to_string_lossy().to_string();
        report(done, total, i, &shown, true, &mut last_emit);
        let target = uniquify(plan.dest_dir.join(name));
        let caches = if plan.copy_mode { Vec::new() } else { cache_files_for(&plan.cache_dir, &src.to_string_lossy()) };
        let mut result: Result<(), String> = Err(String::new());
        if renames {
            result = std::fs::rename(src, &target).map_err(|e| e.to_string());
            if result.is_err() {
                // Not the volume we thought (a mount point inside a folder):
                // count its bytes and fall through to a copy.
                total += std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
            }
        }
        if result.is_err() {
            let mut on_bytes = |b: u64| {
                done += b;
                report(done, total, i, &shown, false, &mut last_emit);
            };
            // Copied under a hidden name and renamed into place only when
            // it's complete and checked, so a quit or crash mid-copy can't
            // leave a truncated file that looks like the real one.
            let part = target.with_file_name(format!(".{}.foxcull-part", target.file_name().unwrap_or_default().to_string_lossy()));
            crate::procs::scratch_add(&part);
            result = copy_file_progress(src, &part, !plan.copy_mode, cancel, &mut on_bytes);
            if result.is_ok() {
                if let Err(e) = std::fs::rename(&part, &target) {
                    let _ = std::fs::remove_file(&part);
                    result = Err(format!("couldn't finish the copy: {e}"));
                }
            }
            crate::procs::scratch_remove(&part);
            if result.is_ok() && !plan.copy_mode {
                // Copied and on the disk: now the original can go. If it
                // won't, the "move" must not leave a duplicate behind.
                if let Err(e) = std::fs::remove_file(src) {
                    let _ = std::fs::remove_file(&target);
                    result = Err(format!("couldn't remove the original: {e}"));
                }
            }
        }
        match result {
            Ok(()) => {
                for c in caches {
                    let _ = std::fs::remove_file(c);
                }
                pairs.push((rel_under(&plan.root, src), rel_under(&plan.dest_root, &target)));
                out.moved += 1;
                out.files.push(MoveRecord {
                    from: src.to_string_lossy().to_string(),
                    to: target.to_string_lossy().to_string(),
                });
            }
            Err(e) if e == TRANSFER_CANCELLED => {
                out.cancelled = true;
                break;
            }
            Err(e) => {
                out.failed.push(src.to_string_lossy().to_string());
                out.errors.push(e);
            }
        }
    }
    (out, pairs)
}

/// Carry the catalog records of transferred files: within one catalog a
/// re-key of the rows (or a copy of them); across drives an export here, an
/// import into the destination drive's own catalog, then (for a move) a
/// forget here. The import comes first, so a failure leaves the records where
/// they were rather than nowhere.
fn transfer_catalog(catalog: &Catalog, data_root: &Path, plan: &TransferPlan, pairs: &[(String, String)]) -> Result<(), String> {
    if pairs.is_empty() {
        return Ok(());
    }
    if !plan.cross_drive {
        let res = if plan.copy_mode { catalog.copy_media_entries(pairs) } else { catalog.move_media_entries(pairs) };
        return res.map_err(|e| e.to_string());
    }
    let froms: Vec<String> = pairs.iter().map(|(f, _)| f.clone()).collect();
    let metas = catalog.export_entries(&froms);
    let dest_lib = resolve_library(data_root, &plan.dest_root);
    let _ = std::fs::create_dir_all(&dest_lib.dir);
    let rows: Vec<(String, crate::catalog::MediaMeta)> = pairs.iter().map(|(_, to)| to.clone()).zip(metas).collect();
    Catalog::open(&dest_lib.catalog)
        .and_then(|c| c.import_entries(&rows))
        .map_err(|e| e.to_string())?;
    if !plan.copy_mode {
        catalog.forget(&froms);
        catalog.clear_counts();
    }
    Ok(())
}

/// Move (or, with `copy`, copy) media into `dest`, like dragging in Finder or
/// Explorer, with the catalog following each file.
///
/// - Same drive: a rename, instant whatever the size. Marks move with it.
/// - Another drive (or `copy`): the bytes are copied in chunks with progress
///   in the job centre (`job` is its activity id), each copy is flushed and
///   size-checked, and only then is the original removed. The marks, tags,
///   trims and events go into the destination drive's own catalog.
/// - The job centre's Stop ends it between chunks: the file in flight is
///   removed from the destination and everything before it stays done.
///
/// `async` + a blocking worker, so a thousand files never wedge the window.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn move_media_files(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    dest: String,
    copy: Option<bool>,
    job: Option<String>,
) -> Result<MoveOutcome, String> {
    let copy_mode = copy.unwrap_or(false);
    let job = job.unwrap_or_else(|| "move".into());
    let fail_all = |paths: Vec<String>, dest: String, e: String| MoveOutcome {
        moved: 0,
        dest,
        files: Vec::new(),
        failed: paths,
        errors: vec![e],
        copied: copy_mode,
        cross_drive: false,
        cancelled: false,
    };
    let root_state = state.root.lock().clone();
    let root = match canonical_active_root(&root_state) {
        Ok(r) => r,
        Err(e) => return Ok(fail_all(paths, dest, e)),
    };
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let dest_dir = match canonical_dir(Path::new(&dest)) {
        Ok(d) => d,
        Err(e) => return Ok(fail_all(paths, dest, e)),
    };
    if let Some(l) = &lib {
        if within(&dest_dir, l) {
            return Ok(fail_all(paths, dest, "refusing to use the app library folder as a destination".into()));
        }
    }
    if let Err(e) = transfer_dest_allowed(&state, &dest_dir) {
        return Ok(fail_all(paths, dest, e));
    }
    // Which catalog the destination belongs to. On a Mac the boot volume's
    // catalog covers everything under `/` except `/Volumes/*`, so this — not
    // `within(dest, root)` — is the test: `/Volumes/SSD` IS under `/`.
    let dest_root = canonical_dir(&drive_root(&dest_dir.to_string_lossy())).unwrap_or_else(|_| root.clone());
    let cross_drive = dest_root != root;
    let cache_dir = state.cache_dir.lock().clone();
    let cancel = job_token(&job);

    let plan = TransferPlan {
        root: root.clone(),
        lib,
        cache_dir,
        dest_dir,
        dest_root,
        copy_mode,
        cross_drive,
    };
    let (plan, (mut out, pairs)) = {
        let app = app.clone();
        let job = job.clone();
        let cancel = cancel.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let emit = |a: Activity| emit_job(&app, Activity { id: job.clone(), ..a });
            let res = transfer_files(&plan, paths, &cancel, &emit);
            (plan, res)
        })
        .await
        .map_err(|e| e.to_string())?
    };
    job_finished(&job, &cancel);

    if let Err(e) = transfer_catalog(&catalog, &state.data_root, &plan, &pairs) {
        out.errors.push(format!("catalog update failed: {e}"));
    }
    crate::log::line(&format!(
        "MOVE {} n={} failed={} cross_drive={cross_drive} cancelled={} dest={:?}",
        if copy_mode { "copy" } else { "move" },
        out.moved,
        out.failed.len(),
        out.cancelled,
        out.dest
    ));
    Ok(out)
}

// ── folder management (Lightroom-style: the app owns the moves) ─────────────

/// Create `name` as a direct child of `parent` and return its absolute path.
/// Restricted to the active library (never the `_FoxCull` folder), and the name
/// must be a single plain segment — no separators, no `..`, no drive prefix —
/// so a typed name can never escape the folder the user right-clicked.
#[tauri::command]
pub fn create_folder(
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> Result<String, String> {
    let trimmed = name.trim().trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        return Err("folder name is empty".into());
    }
    if trimmed.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(r#"a folder name cannot contain \ / : * ? " < > |"#.into());
    }
    if has_unsafe_components(Path::new(&trimmed)) || trimmed == ".." || trimmed == "." {
        return Err("invalid folder name".into());
    }
    let root_state = state.root.lock().clone();
    let root = canonical_active_root(&root_state)?;
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let parent_dir = validate_active_dir(&root, lib.as_ref(), &parent)?;
    let target = parent_dir.join(&trimmed);
    if target.exists() {
        return Err(format!("\"{trimmed}\" already exists here"));
    }
    std::fs::create_dir(&target).map_err(|e| format!("could not create folder: {e}"))?;
    Ok(target.to_string_lossy().to_string())
}

// ── catalog integrity: find, flag and relink moved/renamed files ────────────

#[derive(Serialize)]
pub struct ScanReport {
    /// Rel-paths carrying user metadata that were checked.
    pub tracked: usize,
    /// How many of those were absent from disk when the scan began.
    pub missing: usize,
    /// Absences the scan resolved by itself (folder moved, file renamed away).
    pub relinked: usize,
    /// Still unresolved — these are the "?" items the user can relink by hand.
    pub still_missing: usize,
    /// Media files walked while hunting for relink candidates (0 = no walk was
    /// needed because nothing was missing — the fast, common path).
    pub scanned_files: usize,
    pub elapsed_ms: u64,
}

/// Directory part of a rel-path ("" for a file at the library root).
fn rel_parent(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

fn rel_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Verify every metadata-carrying catalog entry still points at a real file and,
/// when it doesn't, try to find where the file went — the guarantee that makes
/// moving photos outside FoxCull survivable.
///
/// Two passes, cheapest first:
/// 1. **Existence check** over the tracked rel-paths. If nothing is missing the
///    command returns without ever walking the drive, which is why a normal
///    launch costs a few hundred stats rather than a full scan.
/// 2. **Relink hunt** (only if something IS missing): walk the library once,
///    then match by *folder cohort* before individual filenames. A whole folder
///    that moved is recognised as one move because most of its filenames turn up
///    together under a single new directory; leftovers fall back to a unique
///    filename match. Metadata is NEVER deleted here — anything unresolved is
///    flagged so the UI can show a "?" and let the user point at the file.
#[tauri::command]
pub async fn catalog_scan(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    relink: bool,
) -> Result<ScanReport, String> {
    let t0 = Instant::now();
    let root_state = state.root.lock().clone();
    let root = canonical_active_root(&root_state)?;

    let tracked = catalog.tracked_rels();
    let tracked_count = tracked.len();

    let root_for_stat = root.clone();
    let tracked_for_stat = tracked.clone();
    let (mut missing, present): (Vec<String>, Vec<String>) =
        tauri::async_runtime::spawn_blocking(move || {
            warm_pool().install(|| {
                tracked_for_stat
                    .into_par_iter()
                    .partition(|rel| !root_for_stat.join(rel).exists())
            })
        })
        .await
        .map_err(|e| e.to_string())?;

    let missing_before = missing.len();
    let mut relinked: Vec<(String, String)> = Vec::new();
    let mut scanned_files = 0usize;

    if !missing.is_empty() && relink {
        let root_for_walk = root.clone();
        let disk: Vec<PathBuf> = tauri::async_runtime::spawn_blocking(move || {
            let mut paths: Vec<(PathBuf, i64, u64)> = Vec::new();
            collect(&root_for_walk, true, &mut paths);
            paths.into_iter().map(|(p, _, _)| p).collect()
        })
        .await
        .map_err(|e| e.to_string())?;
        scanned_files = disk.len();

        // Files that already carry metadata are off-limits as relink targets —
        // adopting one would overwrite a second photo's marks to "fix" the first.
        let owned: HashSet<String> = present.iter().map(|r| r.to_lowercase()).collect();
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for p in &disk {
            let rel = rel_under(&root, p);
            if owned.contains(&rel.to_lowercase()) {
                continue;
            }
            by_name
                .entry(rel_name(&rel).to_lowercase())
                .or_default()
                .push(rel);
        }

        // Pass 2a — folder cohorts. Group the absent entries by their old folder
        // and ask which single directory now holds most of those filenames.
        let mut cohorts: HashMap<&str, Vec<&String>> = HashMap::new();
        for rel in &missing {
            cohorts.entry(rel_parent(rel)).or_default().push(rel);
        }
        let mut claimed: HashSet<String> = HashSet::new();
        for (old_dir, members) in &cohorts {
            let mut votes: HashMap<&str, usize> = HashMap::new();
            for m in members {
                if let Some(cands) = by_name.get(&rel_name(m).to_lowercase()) {
                    for c in cands {
                        *votes.entry(rel_parent(c)).or_insert(0) += 1;
                    }
                }
            }
            let Some((best_dir, score)) = votes.into_iter().max_by_key(|(_, n)| *n) else {
                continue;
            };
            // A cohort move needs real agreement: at least half the folder's
            // entries, and never a "match" back onto the folder we came from.
            if best_dir.eq_ignore_ascii_case(old_dir) || score * 2 < members.len().max(1) {
                continue;
            }
            let best_dir = best_dir.to_string();
            for m in members {
                let want = if best_dir.is_empty() {
                    rel_name(m).to_string()
                } else {
                    format!("{best_dir}/{}", rel_name(m))
                };
                let found = by_name
                    .get(&rel_name(m).to_lowercase())
                    .and_then(|c| c.iter().find(|r| r.eq_ignore_ascii_case(&want)))
                    .cloned();
                if let Some(to) = found {
                    if claimed.insert(to.to_lowercase()) {
                        relinked.push(((*m).clone(), to));
                    }
                }
            }
        }

        // Pass 2b — leftovers. A single unambiguous filename match anywhere in
        // the library is safe to adopt; two or more candidates is not, and stays
        // a "?" for the user to resolve.
        let done: HashSet<String> = relinked.iter().map(|(f, _)| f.to_lowercase()).collect();
        for m in &missing {
            if done.contains(&m.to_lowercase()) {
                continue;
            }
            let Some(cands) = by_name.get(&rel_name(m).to_lowercase()) else {
                continue;
            };
            let free: Vec<&String> = cands
                .iter()
                .filter(|c| !claimed.contains(&c.to_lowercase()))
                .collect();
            if free.len() == 1 {
                let to = free[0].clone();
                claimed.insert(to.to_lowercase());
                relinked.push((m.clone(), to));
            }
        }

        if !relinked.is_empty() {
            catalog
                .move_media_entries(&relinked)
                .map_err(|e| format!("relink failed: {e}"))?;
            let resolved: HashSet<String> = relinked.iter().map(|(f, _)| f.clone()).collect();
            missing.retain(|m| !resolved.contains(m));
        }
    }

    catalog
        .set_missing(&missing)
        .map_err(|e| format!("could not record missing files: {e}"))?;

    let report = ScanReport {
        tracked: tracked_count,
        missing: missing_before,
        relinked: relinked.len(),
        still_missing: missing.len(),
        scanned_files,
        elapsed_ms: t0.elapsed().as_millis() as u64,
    };
    crate::log::line(&format!(
        "CATALOG-SCAN tracked={} missing={} relinked={} unresolved={} walked={} {}ms",
        report.tracked,
        report.missing,
        report.relinked,
        report.still_missing,
        report.scanned_files,
        report.elapsed_ms
    ));
    Ok(report)
}

/// Every unresolved "?" entry, so the UI can offer a relink list.
#[tauri::command]
pub fn list_missing(catalog: State<'_, Catalog>) -> Vec<String> {
    catalog.missing_under("")
}

/// Point one missing catalog entry at the file the user picked.
#[tauri::command]
pub fn relink_missing(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    rel: String,
    path: String,
) -> Result<String, String> {
    let root_state = state.root.lock().clone();
    let root = canonical_active_root(&root_state)?;
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let target = validate_active_media_file(&root, lib.as_ref(), &path)?;
    let to = rel_under(&root, &target);
    if to.eq_ignore_ascii_case(&rel) {
        catalog.clear_missing(std::slice::from_ref(&rel));
        return Ok(to);
    }
    catalog
        .move_media_entries(&[(rel, to.clone())])
        .map_err(|e| format!("relink failed: {e}"))?;
    Ok(to)
}

#[derive(Serialize)]
pub struct RelinkOutcome {
    pub relinked: usize,
    pub unresolved: Vec<String>,
}

/// Relink every missing entry that used to live under `rel_dir` by looking for
/// each one inside `new_dir` — the "I know where that folder went" action. The
/// sub-path below `rel_dir` is preserved first (a whole tree that moved), with a
/// flat filename match in `new_dir` as the fallback.
#[tauri::command]
pub fn relink_folder(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    // Invoked from JS as `relDir` / `newDir` — Tauri 2 camelCases argument
    // lookup (see the note in src/lib/api.ts).
    rel_dir: String,
    new_dir: String,
) -> Result<RelinkOutcome, String> {
    let root_state = state.root.lock().clone();
    let root = canonical_active_root(&root_state)?;
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let dest = validate_active_dir(&root, lib.as_ref(), &new_dir)?;
    let dest_rel = rel_under(&root, &dest);

    let prefix = rel_dir.trim_end_matches('/').to_string();
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    for rel in catalog.missing_under(&prefix) {
        let sub = rel
            .strip_prefix(&format!("{prefix}/"))
            .unwrap_or(rel_name(&rel));
        let nested = dest.join(sub);
        let flat = dest.join(rel_name(&rel));
        let hit = if nested.is_file() {
            Some(nested)
        } else if flat.is_file() {
            Some(flat)
        } else {
            None
        };
        match hit {
            Some(p) => pairs.push((rel, rel_under(&root, &p))),
            None => unresolved.push(rel),
        }
    }
    if !pairs.is_empty() {
        catalog
            .move_media_entries(&pairs)
            .map_err(|e| format!("relink failed: {e}"))?;
    }
    crate::log::line(&format!(
        "RELINK-FOLDER from={prefix} to={dest_rel} relinked={} unresolved={}",
        pairs.len(),
        unresolved.len()
    ));
    Ok(RelinkOutcome {
        relinked: pairs.len(),
        unresolved,
    })
}

/// Drop the metadata for entries the user confirms are gone for good. This is
/// the ONLY path that deletes marks for a missing file — a scan never does.
#[tauri::command]
pub fn forget_missing(catalog: State<'_, Catalog>, rels: Vec<String>) -> usize {
    catalog.forget(&rels);
    rels.len()
}

// ── events (virtual collections spanning folders) ───────────────────────────

#[tauri::command]
pub fn list_events(catalog: State<'_, Catalog>) -> Vec<crate::catalog::EventInfo> {
    // Sweep before listing: `forget` and a bulk removal can both empty an event,
    // and an event with no photos should be as gone as an unused tag.
    catalog.prune_empty_events();
    catalog.list_events()
}

#[tauri::command]
pub fn create_event(catalog: State<'_, Catalog>, name: String) -> Result<i64, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("event name is empty".into());
    }
    catalog.create_event(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_event(catalog: State<'_, Catalog>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("event name is empty".into());
    }
    catalog.rename_event(id, &name).map_err(|e| e.to_string())
}

/// Delete the event itself. Member files are untouched — an event is metadata
/// about photos, never a container that owns them.
#[tauri::command]
pub fn delete_event(catalog: State<'_, Catalog>, id: i64) {
    catalog.delete_event(id);
}

#[tauri::command]
pub fn add_to_event(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    id: i64,
    paths: Vec<String>,
) -> Result<(), String> {
    let root = state.root.lock().clone();
    let rels: Vec<String> = paths.iter().map(|p| rel_of(&root, p)).collect();
    catalog.add_to_event(id, &rels).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_from_event(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    id: i64,
    paths: Vec<String>,
) -> Result<(), String> {
    let root = state.root.lock().clone();
    let rels: Vec<String> = paths.iter().map(|p| rel_of(&root, p)).collect();
    catalog
        .remove_from_event(id, &rels)
        .map_err(|e| e.to_string())
}

/// Choose the shot that fronts an event's block in the grid ("album art").
/// `path = None` reverts to "let the grid pick the first member".
#[tauri::command]
pub fn set_event_cover(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    id: i64,
    path: Option<String>,
) -> Result<(), String> {
    let root = state.root.lock().clone();
    let rel = path.map(|p| rel_of(&root, &p));
    catalog
        .set_event_cover(id, rel.as_deref())
        .map_err(|e| e.to_string())
}

/// Export `paths` into `dest` as viewable files. RAW (NEF etc.) becomes a JPEG
/// built from the **camera's own embedded full-resolution rendering** — the
/// same white balance / Picture Control the camera baked, NOT a washed-out
/// linear raw decode — extracted byte-for-byte (no re-encode) when the shot is
/// upright, or rotated once at quality 92 when it isn't. Ordinary images are
/// copied verbatim (keeping their EXIF/ICC). Each output's file time is set to
/// the capture date so exports sort correctly anywhere. Sequential on purpose:
/// one reader is the fastest way through a spinning disk.
#[tauri::command]
pub async fn export_jpegs(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
    dest: String,
) -> Result<ExportOutcome, String> {
    let root = canonical_active_root(&state.root.lock().clone())?;
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let mut safe_paths = Vec::with_capacity(paths.len());
    for p in paths {
        let src = validate_active_media_file(&root, lib.as_ref(), &p)?;
        safe_paths.push(src.to_string_lossy().to_string());
    }
    let dest_dir = PathBuf::from(&dest);
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let total = safe_paths.len() as u64;
        let label = format!("Exporting {total} file{}", if total == 1 { "" } else { "s" });
        emit_activity(&app, "export", &label, 0, total, "running");
        let mut out = ExportOutcome {
            exported: 0,
            copied: 0,
            skipped: 0,
            failed: Vec::new(),
            errors: Vec::new(),
            dest: dest_dir.to_string_lossy().to_string(),
        };
        for (i, p) in safe_paths.iter().enumerate() {
            let src = Path::new(p);
            let r = match media::classify(src) {
                Kind::Raw => export_raw_as_jpeg(src, &dest_dir).map(|_| out.exported += 1),
                Kind::Image => copy_preserving(src, &dest_dir).map(|_| out.copied += 1),
                _ => {
                    out.skipped += 1;
                    Ok(())
                }
            };
            if let Err(e) = r {
                out.failed.push(p.clone());
                out.errors.push(e);
            }
            emit_activity(&app, "export", &label, i as u64 + 1, total, "running");
        }
        emit_activity(&app, "export", &label, total, total, "done");
        crate::log::line(&format!(
            "EXPORT dest={:?} exported={} copied={} skipped={} failed={}",
            dest_dir.file_name().unwrap_or_default(),
            out.exported,
            out.copied,
            out.skipped,
            out.failed.len()
        ));
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Non-colliding `<dest>/<stem>.<ext>` for an export.
fn export_target(dest: &Path, src: &Path, force_ext: Option<&str>) -> PathBuf {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".into());
    let ext = force_ext
        .map(|e| e.to_string())
        .or_else(|| src.extension().map(|e| e.to_string_lossy().to_string()))
        .unwrap_or_else(|| "jpg".into());
    uniquify(dest.join(format!("{stem}.{ext}")))
}

/// Stamp `out`'s modified time with the shot's capture date (EXIF), falling
/// back to the source file's own mtime — so exports sort chronologically in
/// any file manager instead of clumping at "just now".
fn stamp_capture_time(src: &Path, out: &Path) {
    let ts = media::capture_date(src).or_else(|| {
        std::fs::metadata(src)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
    });
    if let Some(ts) = ts {
        let _ = filetime::set_file_mtime(out, filetime::FileTime::from_unix_time(ts, 0));
    }
}

/// RAW → JPEG: extract the largest embedded JPEG (the camera's processed
/// full-res preview). Upright shots are written byte-for-byte (zero quality
/// loss vs. what the camera rendered); rotated shots are decoded, oriented and
/// re-encoded once at quality 92 so they don't come out sideways (the embedded
/// preview carries no orientation tag of its own).
fn export_raw_as_jpeg(src: &Path, dest: &Path) -> Result<(), String> {
    let data = std::fs::read(src).map_err(|e| e.to_string())?;
    let jpg = media::largest_embedded_jpeg(&data)
        .ok_or_else(|| "no embedded JPEG preview in this RAW file".to_string())?;
    // Sanity: a real preview, not a postage-stamp thumbnail.
    let (w, h) =
        image::ImageReader::with_format(std::io::Cursor::new(jpg), image::ImageFormat::Jpeg)
            .into_dimensions()
            .map_err(|e| format!("embedded preview unreadable: {e}"))?;
    if w.max(h) < 1024 {
        return Err(format!(
            "embedded preview too small ({w}x{h}) — this RAW needs a real converter"
        ));
    }
    let o = media::orientation(src);
    let out = export_target(dest, src, Some("jpg"));
    if o == 1 {
        std::fs::write(&out, jpg).map_err(|e| e.to_string())?;
    } else {
        let img = image::load_from_memory(jpg).map_err(|e| e.to_string())?;
        let img = thumbs::apply_orientation(img, o);
        let icc = media::icc_from_jpeg(jpg);
        let f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(
            std::io::BufWriter::new(f),
            92,
        );
        enc.encode_image(&image::DynamicImage::ImageRgb8(img.to_rgb8()))
            .map_err(|e| e.to_string())?;
        if let Some(icc) = icc {
            let _ = thumbs::embed_icc(&out, &icc);
        }
    }
    stamp_capture_time(src, &out);
    Ok(())
}

/// Plain image export: byte-for-byte copy (keeps EXIF/ICC), capture-dated.
fn copy_preserving(src: &Path, dest: &Path) -> Result<(), String> {
    let out = export_target(dest, src, None);
    std::fs::copy(src, &out).map_err(|e| e.to_string())?;
    stamp_capture_time(src, &out);
    Ok(())
}

/// Absolute paths of every file currently flagged `reject`, across the whole
/// catalog — the input to the delete sweep.
#[tauri::command]
pub fn list_rejected(state: State<'_, AppState>, catalog: State<'_, Catalog>) -> Vec<String> {
    let root = state.root.lock().clone();
    catalog
        .list_by_flag("reject")
        .into_iter()
        .map(|rel| match &root {
            Some(r) => r.join(&rel).to_string_lossy().to_string(),
            None => rel,
        })
        .collect()
}

/// Every cache file FoxCull may have generated for `src` (grid thumb, loupe
/// preview, video poster). Computed while the original still exists so the
/// content-hashed keys resolve, then removed after the file is disposed — so the
/// cache never accumulates orphaned "ghost" thumbnails.
fn cache_files_for(cache_dir: &Path, src: &str) -> Vec<PathBuf> {
    let p = Path::new(src);
    let mut out = Vec::new();
    for max in [GRID_MAX, LOUPE_MAX] {
        if let Some(cp) = thumbs::cache_path(cache_dir, p, max) {
            out.push(cp);
        }
    }
    out.push(video::poster_path(cache_dir, p));
    // Filmstrip sprite + its geometry sidecar, so a deleted clip leaves no orphan.
    let strip = video::filmstrip_path(cache_dir, p);
    out.push(strip.with_extension("json"));
    out.push(strip);
    let scrub = video::scrubstrip_path(cache_dir, p);
    out.push(scrub.with_extension("json"));
    out.push(scrub);
    // H.264 playback proxy (HEVC-without-codec machines) — can be sizable.
    out.push(video::proxy_path(cache_dir, p));
    out
}

/// Volume/drive root of an absolute path: `C:\` on Windows; `/Volumes/<name>`
/// (or `/`) on macOS/Linux. Also the library root for catalog keys, and the base
/// the recycle folder mirrors structure from
/// (`C:\…\alpha\beta\x.jpg` → `<recycle>\alpha\beta\x.jpg`).
fn drive_root(path: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let b = path.as_bytes();
        if b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
            return PathBuf::from(format!("{}:\\", b[0] as char));
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(rest) = path.strip_prefix("/Volumes/") {
            if let Some(name) = rest.split('/').next() {
                if !name.is_empty() {
                    return PathBuf::from(format!("/Volumes/{name}"));
                }
            }
        }
    }
    PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" })
}

/// Move one file into the visible per-drive Trash, FLAT. Returns
/// `(stored, orig)` — the filename within the Trash folder, and the original
/// drive-relative path (for Restore). Never clobbers an existing file.
fn move_into_recycle(root: &Path, recycle: &Path, src: &Path) -> Result<(String, String), String> {
    let rel = src
        .strip_prefix(root)
        .map(|r| r.to_path_buf())
        .unwrap_or_else(|_| PathBuf::from(src.file_name().unwrap_or_default()));
    let orig = rel.to_string_lossy().replace('\\', "/");
    // FLAT. The Trash is somewhere the user actually browses now, and a mirrored
    // twenty-deep folder tree is hostile to that — you would be clicking through
    // `2026 > S23 Ultra > Munnar trip` to reach three rejects. Collisions are
    // handled by `uniquify`, and provenance moves to the catalog + sidecar.
    let name = PathBuf::from(src.file_name().unwrap_or_default());
    let target = uniquify(recycle.join(&name));
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // The Trash folder is ALWAYS on the same volume as the source (it lives at
    // `<drive-root>/FoxCull Trash`, and `src` is validated to sit under that
    // same drive root). So `rename` is a metadata-only move that's instant even
    // for a 17 GB clip — and a failure here means the file is *locked* (a preview
    // or playback still holds it open) or permission-denied, NOT a cross-device
    // move. The old code fell back to `copy` + `remove_file`, which on a huge
    // locked file copied gigabytes and then failed the remove anyway — the exact
    // path that made deleting a big in-use HEVC clip hang the app ("not
    // responding"). Surface a clear, retryable error instead of copying.
    if let Err(e) = std::fs::rename(src, &target) {
        // A read-only attribute denies the move exactly like an ACL does, but is
        // ours to clear — do that once and retry before blaming the user.
        if is_permission_denied(&e) && clear_readonly(src) {
            if std::fs::rename(src, &target).is_ok() {
                return finish_move(recycle, &rel, &target, orig);
            }
        }
        return Err(describe_move_failure(src, &e));
    }
    finish_move(recycle, &rel, &target, orig)
}

fn finish_move(
    recycle: &Path,
    rel: &Path,
    target: &Path,
    orig: String,
) -> Result<(String, String), String> {
    let stored = target
        .strip_prefix(recycle)
        .unwrap_or(rel)
        .to_string_lossy()
        .replace('\\', "/");
    Ok((stored, orig))
}

fn is_permission_denied(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied
}

/// Drop the read-only attribute so a retry can proceed. Returns whether anything
/// changed (false = it wasn't read-only, so the denial is a real ACL problem).
fn clear_readonly(src: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(src) else {
        return false;
    };
    let mut perms = meta.permissions();
    if !perms.readonly() {
        return false;
    }
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(src, perms).is_ok()
}

/// Explain a failed dispose in terms the user can act on.
///
/// The previous version called EVERY failure "file is in use", which sent the
/// owner hunting a phantom lock when the real cause was an ACL: media copied to
/// a data drive BEFORE a Windows reinstall carries the old installation's SID,
/// so the new account inherits no delete right. The tell is that Explorer also
/// refuses — "File Access Denied · You'll need to provide administrator
/// permission to delete this file" — while playback works fine, because reading
/// is allowed and only the directory write is not. A sharing violation and a
/// permission denial need opposite responses from the user, so name them apart.
fn describe_move_failure(src: &Path, e: &std::io::Error) -> String {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    #[cfg(windows)]
    {
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        match e.raw_os_error() {
            Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION) => {
                return format!(
                    "{name}: another program still has this file open — close whatever is playing or reading it, then delete again"
                )
            }
            _ => {}
        }
    }
    if is_permission_denied(e) {
        return format!(
            "{name}: Windows denied permission to move this file. Its folder's security settings don't grant your account delete rights — Explorer asks for administrator permission on it too. This usually means the files predate a Windows reinstall. Take ownership of the folder once (Properties → Security → Advanced → Change owner, apply to contents) and it will delete normally."
        );
    }
    format!("{name}: could not be moved to Trash ({e})")
}

/// Dispose of rejected files. `mode` = "recycle" (OS Recycle Bin / Trash) or
/// "folder" (move into the active drive's `_FoxCull/recycle`, tracked by the
/// in-app Trash so it can be previewed, restored or purged). Drops catalog
/// decision rows for disposed files; records folder-mode deletes in `trash`.
///
/// ASYNC deliberately: synchronous commands run on the main thread, and this one
/// touches the filesystem per file — when a huge in-use clip made the old
/// copy-fallback grind for minutes, the whole window went "not responding".
/// Async keeps the UI alive no matter how slow a dispose turns out to be.
#[tauri::command]
pub async fn dispose_rejected(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    paths: Vec<String>,
    mode: String,
) -> Result<TrashOutcome, String> {
    let root = state.root.lock().clone();
    let root_canon = match canonical_active_root(&root) {
        Ok(r) => r,
        Err(e) => {
            return Ok(TrashOutcome {
                deleted: 0,
                failed: paths,
                errors: vec![e],
                trashed: Vec::new(),
            })
        }
    };
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let cache_dir = state.cache_dir.lock().clone();
    let recycle = state.recycle_dir.lock().clone();
    let folder = mode == "folder";
    let at = now();
    let mut deleted = 0usize;
    let mut failed = Vec::new();
    let mut errors = Vec::new();
    let mut forget = Vec::new();
    let mut trash_rows: Vec<(String, String, String, i64)> = Vec::new();
    for p in &paths {
        let src = match validate_active_media_file(&root_canon, lib.as_ref(), p) {
            Ok(src) => src,
            Err(e) => {
                failed.push(p.clone());
                errors.push(e);
                continue;
            }
        };
        let src_s = src.to_string_lossy().to_string();
        // Compute the cache files NOW, while the original still exists (the keys
        // hash its metadata) — we remove them only after a successful dispose.
        let caches = cache_files_for(&cache_dir, &src_s);
        let result: Result<Option<(String, String)>, String> = if folder {
            move_into_recycle(&root_canon, &recycle, &src).map(Some)
        } else {
            trash::delete(&src).map(|_| None).map_err(|e| e.to_string())
        };
        match result {
            Ok(stored) => {
                deleted += 1;
                forget.push(rel_under(&root_canon, &src));
                for c in caches {
                    let _ = std::fs::remove_file(c);
                }
                if let Some((stored, orig)) = stored {
                    let name = src
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    trash_rows.push((stored, orig, name, at));
                }
            }
            Err(e) => {
                failed.push(p.clone());
                errors.push(e);
            }
        }
    }
    catalog.forget(&forget);
    if !trash_rows.is_empty() {
        let _ = catalog.add_trash_many(&trash_rows);
        write_trash_index(&recycle, &catalog);
    }
    Ok(TrashOutcome {
        deleted,
        failed,
        errors,
        trashed: trash_rows.iter().map(|r| r.0.clone()).collect(),
    })
}

// ── in-app Trash (per-drive recycle folder) ─────────────────────────────────

#[derive(Serialize)]
pub struct TrashItem {
    pub stored: String,
    pub orig: String,
    /// Absolute path of the file in the recycle dir (for the thumbnail/preview).
    pub path: String,
    pub name: String,
    pub kind: String,
    pub ext: String,
    pub deleted_at: i64,
}

/// Everything in the active drive's Trash, most recently rejected first. Prunes
/// rows whose file has vanished (e.g. emptied outside the app).
#[tauri::command]
pub async fn list_trash(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
) -> Result<Vec<TrashItem>, String> {
    let recycle = state.recycle_dir.lock().clone();

    // ── adopt orphans before listing ────────────────────────────────────────
    // Files can end up in the per-drive recycle folder with no `trash` row —
    // a catalog reset or a lost write is enough. The consequence is severe and
    // silent: the Trash panel shows nothing, Restore can't reach them, and the
    // files sit there consuming disk forever while the user is told the Trash
    // is empty. That is how an 18 GB merged clip and a gigabyte of drone
    // footage went unaccounted for on this machine.
    //
    // The recycle layout mirrors the original rel-path (`stored == orig` for
    // every row FoxCull writes), so an orphan can be reconstructed exactly:
    // its position under `recycle/` IS where it came from. Adopting it makes it
    // visible and restorable — never deleted, just accounted for.
    let recycle_for_scan = recycle.clone();
    let known: HashSet<String> = catalog
        .list_trash()
        .into_iter()
        .map(|r| r.stored.to_lowercase())
        .collect();
    let adopted: Vec<(String, String, String, i64)> =
        tauri::async_runtime::spawn_blocking(move || {
            let mut found: Vec<(PathBuf, i64, u64)> = Vec::new();
            collect(&recycle_for_scan, true, &mut found);
            // The sidecar is the reason a flat Trash is safe: it remembers
            // where each file came from independently of the catalog, so a
            // catalog that is reset or lost no longer strands the files with no
            // record of their home.
            let sidecar = read_trash_index(&recycle_for_scan);
            found
                .into_iter()
                .filter_map(|(path, mtime, _)| {
                    let rel = rel_under(&recycle_for_scan, &path);
                    if rel.is_empty() || known.contains(&rel.to_lowercase()) {
                        return None;
                    }
                    let name = rel_name(&rel).to_string();
                    let (orig, at) = sidecar
                        .get(&rel.to_lowercase())
                        .cloned()
                        .unwrap_or_else(|| (rel.clone(), mtime));
                    Some((rel, orig, name, at))
                })
                .collect()
        })
        .await
        .map_err(|e| e.to_string())?;
    if !adopted.is_empty() {
        crate::log::line(&format!(
            "TRASH adopted {} orphaned file(s) from the recycle folder",
            adopted.len()
        ));
        let _ = catalog.add_trash_many(&adopted);
        write_trash_index(&recycle, &catalog);
    }

    let mut stale: Vec<String> = Vec::new();
    let items: Vec<TrashItem> = catalog
        .list_trash()
        .into_iter()
        .filter_map(|r| {
            let path = match safe_recycle_child(&recycle, &r.stored) {
                Ok(path) => path,
                Err(_) => {
                    stale.push(r.stored);
                    return None;
                }
            };
            if !path.exists() {
                stale.push(r.stored);
                return None;
            }
            Some(TrashItem {
                kind: media::classify(&path).as_str().to_string(),
                ext: media::ext_lower(&path),
                path: path.to_string_lossy().to_string(),
                stored: r.stored,
                orig: r.orig,
                name: r.name,
                deleted_at: r.deleted_at,
            })
        })
        .collect();
    if !stale.is_empty() {
        catalog.remove_trash(&stale);
    }
    Ok(items)
}

#[derive(Serialize)]
pub struct RestoreOutcome {
    pub restored: usize,
    pub failed: Vec<String>,
}

/// Move trashed files back to their original location on the drive. Uniquifies
/// if something now occupies the original path. Removes restored trash rows.
#[tauri::command]
pub fn restore_trash(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    stored: Vec<String>,
) -> RestoreOutcome {
    let recycle = state.recycle_dir.lock().clone();
    let drive = match state.root.lock().clone() {
        Some(r) => r,
        None => {
            return RestoreOutcome {
                restored: 0,
                failed: stored,
            }
        }
    };
    let orig_of: HashMap<String, String> = catalog
        .list_trash()
        .into_iter()
        .map(|r| (r.stored, r.orig))
        .collect();
    let mut restored = 0usize;
    let mut failed = Vec::new();
    let mut done = Vec::new();
    for s in &stored {
        let Some(orig) = orig_of.get(s) else {
            failed.push(s.clone());
            continue;
        };
        let from = match safe_recycle_child(&recycle, s).and_then(|p| {
            if p.is_file() {
                Ok(p)
            } else {
                Err("trash file is missing".into())
            }
        }) {
            Ok(p) => p,
            Err(_) => {
                failed.push(s.clone());
                continue;
            }
        };
        let to = match restore_target(&drive, orig) {
            Ok(p) => p,
            Err(_) => {
                failed.push(s.clone());
                continue;
            }
        };
        let ok = if std::fs::rename(&from, &to).is_ok() {
            true
        } else if std::fs::copy(&from, &to).is_ok() {
            // Cross-volume fallback: only a removed source counts as restored. If
            // the source can't be removed, delete the copy so the file isn't left
            // duplicated (a later retry would otherwise uniquify a 2nd restore).
            if std::fs::remove_file(&from).is_ok() {
                true
            } else {
                let _ = std::fs::remove_file(&to);
                false
            }
        } else {
            false
        };
        if ok {
            restored += 1;
            done.push(s.clone());
        } else {
            failed.push(s.clone());
        }
    }
    catalog.remove_trash(&done);
    write_trash_index(&recycle, &catalog);
    RestoreOutcome { restored, failed }
}

/// Permanently delete trashed files (and their cached thumbs/posters). Returns
/// the number removed.
#[tauri::command]
pub fn purge_trash(
    state: State<'_, AppState>,
    catalog: State<'_, Catalog>,
    stored: Vec<String>,
) -> usize {
    let recycle = state.recycle_dir.lock().clone();
    let cache_dir = state.cache_dir.lock().clone();
    let known: HashSet<String> = catalog
        .list_trash()
        .into_iter()
        .map(|r| r.stored)
        .collect();
    let mut n = 0usize;
    let mut done = Vec::new();
    for s in &stored {
        if !known.contains(s) {
            continue;
        }
        let p = match safe_recycle_child(&recycle, s) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let caches = cache_files_for(&cache_dir, &p.to_string_lossy());
        if std::fs::remove_file(&p).is_ok() || !p.exists() {
            n += 1;
            done.push(s.clone());
            for c in caches {
                let _ = std::fs::remove_file(c);
            }
        }
    }
    catalog.remove_trash(&done);
    write_trash_index(&recycle, &catalog);
    n
}

/// Where the active library lives (catalog + cache + recycle), and whether it's
/// on the drive or an app-data fallback.
#[tauri::command]
pub fn library_info(state: State<'_, AppState>) -> LibraryInfo {
    let dir = state.lib_dir.lock().clone();
    let catalog = state.catalog_path.lock().clone();
    let recycle = state.recycle_dir.lock().clone();
    let root = state.root.lock().clone();
    let writable = root.as_ref().map(|r| is_writable(r)).unwrap_or(false);
    let on_drive = !dir.starts_with(&state.data_root);
    LibraryInfo {
        root: root
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_default(),
        dir: dir.to_string_lossy().to_string(),
        catalog: catalog.to_string_lossy().to_string(),
        recycle: recycle.to_string_lossy().to_string(),
        on_drive,
        writable,
    }
}

/// Reveal a file in the OS file manager (Explorer / Finder), selected.
#[tauri::command]
pub fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    // Windows: drive Explorer directly with `/select,` so the file lands
    // SELECTED in its folder. The opener plugin's reveal reuses an already-open
    // Explorer window on that folder and then leaves nothing highlighted, so
    // "Show in Explorer" on one shot out of six hundred dropped you in the
    // folder with no idea which one it meant. Directories still just open.
    #[cfg(windows)]
    {
        let p = Path::new(&path);
        if p.is_file() {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            // explorer.exe parses its OWN command line and splits `/select,…` on
            // spaces, so an unquoted path with a space silently loses everything
            // after it and Explorer falls back to opening Documents. That is
            // exactly what "Show in Explorer" did on
            // `P:\All media MASTER\Pics\2010\…` — it looked like the external
            // drive was unsupported; the real trigger was the space in the
            // folder name, and it would have failed identically on any drive.
            //
            // The path therefore has to be quoted INSIDE the single `/select,`
            // token. `raw_arg` is required to do that: `arg()` applies Rust's
            // own MSVC-style quoting on top, producing a doubly-quoted token
            // Explorer rejects the same way. Backslashes are forced because
            // Explorer will not accept a forward-slash path here.
            let native = p.to_string_lossy().replace('/', "\\");
            return std::process::Command::new("explorer.exe")
                .raw_arg(format!("/select,\"{native}\""))
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                // explorer.exe exits non-zero even when it worked, so only the
                // spawn itself is worth checking.
                .map(|_| ())
                .map_err(|e| e.to_string());
        }
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())
}

/// Open a file in the user's default application (e.g. a system video player for
/// HEVC clips the webview can't decode — the Osmo Pocket 3 footage).
#[tauri::command]
pub fn open_external(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Whether `dir` is writable — used to detect a read-only mount (e.g. NTFS on
/// macOS) so the UI can disable the delete sweep with an explanation.
#[tauri::command]
pub fn folder_writable(state: State<'_, AppState>, dir: String) -> bool {
    let root = match canonical_active_root(&state.root.lock().clone()) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let lib = canonical_lib_dir(&state.lib_dir.lock().clone());
    let dir = match validate_active_dir(&root, lib.as_ref(), &dir) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let probe = dir.join(".foxcull_write_test.tmp");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
