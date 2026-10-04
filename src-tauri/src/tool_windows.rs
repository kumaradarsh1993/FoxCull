//! The Edit, Merge and Reel windows: separate OS windows beside the library.
//!
//! Every window loads the same app (`index.html`); the frontend picks what to
//! render from the window's label ("main" = library, "edit", "merge", "reel"). That
//! keeps routing out of it entirely, in dev and in the bundle alike.
//!
//! The library hands work to a tool window through an **inbox** held here,
//! not through the window itself: the window may not exist yet, or may be
//! mid-load and not listening. `open_tool_window` queues the payload, creates
//! or focuses the window, then pings it; the window drains its inbox on load
//! and on every ping, so nothing is lost to that race.

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

static INBOX: LazyLock<Mutex<HashMap<String, Vec<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// What a tool window is, and how big it opens.
fn spec(kind: &str) -> Option<(&'static str, &'static str, f64, f64, f64, f64)> {
    match kind {
        // label, title, width, height, min width, min height
        "edit" => Some(("edit", "FoxCull Edit", 1180.0, 780.0, 760.0, 520.0)),
        "merge" => Some(("merge", "FoxCull Merge", 1120.0, 760.0, 680.0, 480.0)),
        "reel" => Some(("reel", "FoxCull Reel", 1240.0, 860.0, 860.0, 560.0)),
        _ => None,
    }
}

/// Queue `payload` (JSON the window understands) for a tool window, open or
/// focus it, and tell it to look. `payload: None` just brings it forward.
#[tauri::command]
pub fn open_tool_window(app: AppHandle, kind: String, payload: Option<String>) -> Result<(), String> {
    let (label, title, w, h, min_w, min_h) = spec(&kind).ok_or_else(|| format!("unknown window {kind}"))?;
    if let Some(p) = payload {
        INBOX.lock().entry(label.to_string()).or_default().push(p);
    }
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        let mut b = WebviewWindowBuilder::new(&app, label, WebviewUrl::App("index.html".into()))
            .title(title)
            .inner_size(w, h)
            .min_inner_size(min_w, min_h);
        // Edit and Merge take clips dragged in from the library (HTML5 drag
        // and drop), which the native file-drop handler would eat. The Reel
        // window wants the opposite: a song dropped from Finder/Explorer,
        // with its path, which only the native handler gives. Its clips come
        // from the library's menu or ⌘C/⌘V, and it reorders with the pointer.
        if label != "reel" {
            b = b.disable_drag_drop_handler();
        }
        b.build().map_err(|e| format!("couldn't open the {kind} window: {e}"))?;
    }
    let _ = app.emit_to(label, "tool-inbox", ());
    Ok(())
}

/// "Show in library" from a tool window: bring the library forward and have
/// it open that clip's folder with the clip selected.
#[tauri::command]
pub fn show_in_library(app: AppHandle, path: String) -> Result<(), String> {
    let main = app.get_webview_window("main").ok_or("the library window is closed")?;
    let _ = main.unminimize();
    let _ = main.show();
    let _ = main.set_focus();
    app.emit_to("main", "library-reveal", path).map_err(|e| e.to_string())
}

/// Everything queued for this window since it last looked, oldest first.
#[tauri::command]
pub fn take_tool_inbox(kind: String) -> Vec<String> {
    INBOX.lock().remove(&kind).unwrap_or_default()
}

#[derive(Serialize)]
pub struct Tiled {
    pub tiled: bool,
}

/// Side by side: the library on the left half of its screen, `kind` on the
/// right half (Windows Snap / macOS tiling, done in-app). Uses the screen's
/// work area, so the menu bar, Dock and taskbar stay clear.
#[tauri::command]
pub fn tile_windows(app: AppHandle, kind: String) -> Result<Tiled, String> {
    let (label, ..) = spec(&kind).ok_or_else(|| format!("unknown window {kind}"))?;
    let main = app.get_webview_window("main").ok_or("the library window is closed")?;
    let other = app.get_webview_window(label).ok_or("that window is closed")?;
    let monitor = main
        .current_monitor()
        .map_err(|e| e.to_string())?
        .or(main.primary_monitor().map_err(|e| e.to_string())?)
        .ok_or("no screen found")?;
    let area = monitor.work_area();
    let (x, y) = (area.position.x, area.position.y);
    let (w, h) = (area.size.width as i32, area.size.height as i32);
    let half = w / 2;
    for (win, left) in [(&main, x), (&other, x + half)] {
        // A maximised or full-screen window ignores a new size until restored.
        let _ = win.set_fullscreen(false);
        let _ = win.unmaximize();
        let _ = win.unminimize();
        let _ = win.set_position(tauri::PhysicalPosition::new(left, y));
        let _ = win.set_size(tauri::PhysicalSize::new(half.max(400) as u32, h.max(300) as u32));
    }
    let _ = other.set_focus();
    Ok(Tiled { tiled: true })
}

/// Set once the owner has agreed to quit with work running.
pub static QUIT_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Quit now: stop whatever is running, remove half-written files, exit.
/// Called after the library asked "Quit and stop the merge?".
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    QUIT_OK.store(true, std::sync::atomic::Ordering::SeqCst);
    crate::procs::kill_all_and_clean();
    app.exit(0);
}

/// Something is still running or half-written (a merge, an export, a copy).
pub fn work_in_progress() -> bool {
    crate::procs::busy() && !QUIT_OK.load(std::sync::atomic::Ordering::SeqCst)
}

// A shared scratchpad between windows: "clips" is the library's ⌘C (pasted
// into the Edit window with ⌘V), "drag" is the drag in progress (read on a
// drop in another window when the browser didn't carry the drag's own data
// across windows). Values are JSON the frontend owns.
static STASH: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[tauri::command]
pub fn stash_set(key: String, value: Option<String>) {
    let mut s = STASH.lock();
    match value {
        Some(v) => {
            s.insert(key, v);
        }
        None => {
            s.remove(&key);
        }
    }
}

#[tauri::command]
pub fn stash_get(key: String) -> Option<String> {
    STASH.lock().get(&key).cloned()
}
