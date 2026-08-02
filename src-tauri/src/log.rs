//! Tiny file logger for perf diagnosis. Writes timestamped lines to
//! `%APPDATA%/com.foxcull.app/foxcull.log` (truncated each launch) AND to
//! stderr (the `tauri dev` terminal). Low overhead; only hot paths log.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

static LOGFILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn init(path: PathBuf) {
    // Robust open. Truncating on every launch is nice for keeping the log small,
    // BUT combined with the single-instance relaunch race it silently disabled
    // logging for whole sessions: a fresh instance starting while a stuck one
    // still held the handle failed the truncate-open, `.ok()` swallowed it, and
    // we went blind on exactly the crashed/frozen sessions we needed to see.
    // So: truncate only when the file has grown large; otherwise APPEND (which
    // can't lose a session to a race), and if even that fails, fall back to a
    // per-process file so a session is never lost.
    let big = std::fs::metadata(&path).map(|m| m.len() > 5_000_000).unwrap_or(false);
    let mut opts = OpenOptions::new();
    opts.create(true).write(true);
    if big {
        opts.truncate(true);
    } else {
        opts.append(true);
    }
    let file = opts.open(&path).ok().or_else(|| {
        let alt = path.with_file_name(format!("foxcull-{}.log", std::process::id()));
        OpenOptions::new().create(true).append(true).open(alt).ok()
    });
    let _ = LOGFILE.set(Mutex::new(file));
    line(&format!(
        "=== FoxCull session start pid={} ; log at {} ===",
        std::process::id(),
        path.display()
    ));
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn line(msg: &str) {
    eprintln!("[FoxCull] {msg}");
    if let Some(m) = LOGFILE.get() {
        if let Some(f) = m.lock().as_mut() {
            let _ = writeln!(f, "{} {}", now_ms(), msg);
            let _ = f.flush();
        }
    }
}
