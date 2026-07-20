//! Runtime binding to **libmpv** (the engine inside mpv/VLC-family players) for
//! the experimental native video player — see `docs/design/libmpv-transplant.md`.
//!
//! Loaded via `libloading` (dlopen) rather than linked at build time, on
//! purpose: a missing/broken `libmpv-2.dll` simply makes the native path
//! unavailable and the UI falls back to the `<video>` element. The normal
//! `cargo build`/CI release therefore gains **no** link dependency on mpv, and
//! this whole module can be developed without destabilising the shipping build.
//!
//! Only the tiny slice of the C API needed for `--wid` window embedding is
//! bound here (no render-context API). All FFI is confined to this file behind
//! the safe `Mpv` wrapper.

#![allow(dead_code)] // wrapper methods land ahead of their call sites (M2/M4).

use libloading::os::windows::Symbol as RawSymbol;
use libloading::Library;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::{Path, PathBuf};

// ── mpv C ABI constants ──────────────────────────────────────────────────────
/// `mpv_format` value for a 64-bit int option (used to hand mpv the window id).
const MPV_FORMAT_INT64: c_int = 4;

type MpvHandle = *mut c_void;

// Function-pointer types for the symbols we resolve out of the dll.
type FnClientApiVersion = unsafe extern "C" fn() -> std::os::raw::c_ulong;
type FnCreate = unsafe extern "C" fn() -> MpvHandle;
type FnInitialize = unsafe extern "C" fn(MpvHandle) -> c_int;
type FnSetOption = unsafe extern "C" fn(MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
type FnSetOptionString = unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int;
type FnCommandString = unsafe extern "C" fn(MpvHandle, *const c_char) -> c_int;
type FnSetPropertyString = unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int;
type FnTerminateDestroy = unsafe extern "C" fn(MpvHandle);
type FnErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

/// Resolved symbols. `RawSymbol` is detached from the `Library`'s lifetime, so
/// we keep the `Library` alive in `Mpv` for as long as these are used.
struct MpvFns {
    client_api_version: RawSymbol<FnClientApiVersion>,
    create: RawSymbol<FnCreate>,
    initialize: RawSymbol<FnInitialize>,
    set_option: RawSymbol<FnSetOption>,
    set_option_string: RawSymbol<FnSetOptionString>,
    command_string: RawSymbol<FnCommandString>,
    set_property_string: RawSymbol<FnSetPropertyString>,
    terminate_destroy: RawSymbol<FnTerminateDestroy>,
    error_string: RawSymbol<FnErrorString>,
}

/// Candidate locations for the runtime dll, most-specific first. Bundled next to
/// the exe (CI drops it there, like the ffmpeg sidecar); a user-writable copy in
/// AppData is the dev/manual fallback.
fn dll_candidates(exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = exe_dir {
        v.push(d.join("libmpv-2.dll"));
    }
    if let Some(local) = dirs_local_appdata() {
        v.push(local.join("FoxCull").join("libmpv-2.dll"));
    }
    // Last resort: let the OS loader search PATH.
    v.push(PathBuf::from("libmpv-2.dll"));
    v
}

fn dirs_local_appdata() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// Load the dll and resolve every symbol we need. Returns the opened `Library`
/// (kept alive by the caller) plus the detached symbol table.
unsafe fn open(dll: &Path) -> Result<(Library, MpvFns), String> {
    let lib = Library::new(dll).map_err(|e| format!("dlopen {}: {e}", dll.display()))?;

    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let s = lib
                .get::<$ty>($name)
                .map_err(|e| format!("missing symbol {}: {e}", String::from_utf8_lossy($name)))?;
            s.into_raw()
        }};
    }

    let fns = MpvFns {
        client_api_version: sym!(b"mpv_client_api_version\0", FnClientApiVersion),
        create: sym!(b"mpv_create\0", FnCreate),
        initialize: sym!(b"mpv_initialize\0", FnInitialize),
        set_option: sym!(b"mpv_set_option\0", FnSetOption),
        set_option_string: sym!(b"mpv_set_option_string\0", FnSetOptionString),
        command_string: sym!(b"mpv_command_string\0", FnCommandString),
        set_property_string: sym!(b"mpv_set_property_string\0", FnSetPropertyString),
        terminate_destroy: sym!(b"mpv_terminate_destroy\0", FnTerminateDestroy),
        error_string: sym!(b"mpv_error_string\0", FnErrorString),
    };
    Ok((lib, fns))
}

/// Loads libmpv from any candidate path and reports its client API version, WITHOUT
/// creating a player or a window. Used by the `native_video_probe` command so we
/// can confirm the dll is present and loadable on a machine before wiring any
/// windowing. Returns a human-readable version string.
pub fn probe(exe_dir: Option<&Path>) -> Result<String, String> {
    let mut last_err = String::from("no candidate paths");
    for cand in dll_candidates(exe_dir) {
        // SAFETY: `open` only resolves well-known mpv symbols; we call the
        // no-arg version function, which has no side effects.
        match unsafe { open(&cand) } {
            Ok((_lib, fns)) => {
                let raw = unsafe { (fns.client_api_version)() };
                let major = (raw >> 16) & 0xffff;
                let minor = raw & 0xffff;
                return Ok(format!(
                    "libmpv loaded from {} — client API {major}.{minor}",
                    cand.display()
                ));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// A live mpv player embedded into a host window. Created in M2 once we have a
/// child HWND to hand mpv via `--wid`.
pub struct Mpv {
    _lib: Library, // keep the dll mapped for as long as the raw symbols live
    fns: MpvFns,
    ctx: MpvHandle,
}

// mpv's handle is used only from the command thread; we serialise access at the
// call site. The raw pointer isn't auto-Send, so assert it deliberately.
unsafe impl Send for Mpv {}

impl Mpv {
    /// Create an mpv instance rendering into the native window `wid` (an HWND on
    /// Windows, cast to i64). Sets safe defaults for a culling player: hardware
    /// decoding, no on-screen-controller, keep-open so the last frame stays.
    pub fn embed(dll: &Path, wid: i64) -> Result<Self, String> {
        // SAFETY: FFI to the mpv C API; every pointer below is either a valid
        // CString we own for the call, or mpv's own handle.
        unsafe {
            let (lib, fns) = open(dll)?;
            let ctx = (fns.create)();
            if ctx.is_null() {
                return Err("mpv_create returned null".into());
            }
            let me = Mpv { _lib: lib, fns, ctx };

            // Embed into the host window BEFORE initialize().
            let mut wid_val: i64 = wid;
            let rc = (me.fns.set_option)(
                me.ctx,
                c"wid".as_ptr(),
                MPV_FORMAT_INT64,
                &mut wid_val as *mut i64 as *mut c_void,
            );
            me.check(rc, "set wid")?;

            me.set_opt("hwdec", "auto-safe")?; // NVDEC/d3d11va when available
            me.set_opt("vo", "gpu")?;
            me.set_opt("osc", "no")?; // our HTML overlay is the controller
            me.set_opt("input-default-bindings", "no")?;
            me.set_opt("input-vo-keyboard", "no")?;
            me.set_opt("keep-open", "yes")?; // hold the last frame, don't close
            me.set_opt("pause", "yes")?; // default paused; UI decides autoplay

            let rc = (me.fns.initialize)(me.ctx);
            me.check(rc, "initialize")?;
            Ok(me)
        }
    }

    fn set_opt(&self, name: &str, val: &str) -> Result<(), String> {
        let n = CString::new(name).map_err(|e| e.to_string())?;
        let v = CString::new(val).map_err(|e| e.to_string())?;
        // SAFETY: valid handle + owned C strings for the duration of the call.
        let rc = unsafe { (self.fns.set_option_string)(self.ctx, n.as_ptr(), v.as_ptr()) };
        self.check(rc, "set-option")
    }

    /// Load a file for playback (replaces whatever is playing).
    pub fn loadfile(&self, path: &Path) -> Result<(), String> {
        // `command_string` handles quoting for a single argument reasonably; use
        // the property/command split to avoid path-escaping pitfalls later.
        let cmd = format!("loadfile \"{}\"", path.to_string_lossy().replace('"', "\\\""));
        self.command(&cmd)
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        let n = c"pause";
        let v = if paused { c"yes" } else { c"no" };
        let rc = unsafe { (self.fns.set_property_string)(self.ctx, n.as_ptr(), v.as_ptr()) };
        self.check(rc, "set pause")
    }

    /// Absolute seek to `secs`. Frame-accurate ("exact") — mpv keeps the decoder
    /// hot so this is the smooth, VLC-like scrub the whole transplant is for.
    pub fn seek_abs(&self, secs: f64) -> Result<(), String> {
        self.command(&format!("seek {secs:.3} absolute+exact"))
    }

    pub fn command(&self, cmd: &str) -> Result<(), String> {
        let c = CString::new(cmd).map_err(|e| e.to_string())?;
        // SAFETY: valid handle + owned C string for the duration of the call.
        let rc = unsafe { (self.fns.command_string)(self.ctx, c.as_ptr()) };
        self.check(rc, "command")
    }

    fn check(&self, rc: c_int, what: &str) -> Result<(), String> {
        if rc >= 0 {
            return Ok(());
        }
        // SAFETY: mpv_error_string returns a static C string for any code.
        let msg = unsafe {
            let p = (self.fns.error_string)(rc);
            if p.is_null() {
                "unknown".to_string()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        Err(format!("{what}: mpv error {rc} ({msg})"))
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            // SAFETY: terminates and frees the mpv instance exactly once.
            unsafe { (self.fns.terminate_destroy)(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
    }
}

// ── native child window + player orchestration (M2) ──────────────────────────
// Win32 windowing is hand-declared (4 user32 functions) rather than pulled from
// the `windows-sys` crate ON PURPOSE: that crate's WindowsAndMessaging feature
// adds ~100k public symbols, and our cdylib auto-exports all of them, blowing
// past GNU ld's 65k DLL export-ordinal limit on the local toolchain (see the
// workspace CLAUDE.md). `user32` is already linked; this adds no symbol bloat.
type HWND = *mut c_void;

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;

#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *const c_void,
    ) -> HWND;
    fn DestroyWindow(hwnd: HWND) -> i32;
    fn SetWindowPos(
        hwnd: HWND,
        insert_after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Resolve the first usable libmpv dll path, or an error listing what we tried.
pub fn resolve_dll(exe_dir: Option<&Path>) -> Result<PathBuf, String> {
    for cand in dll_candidates(exe_dir) {
        if cand.is_absolute() && cand.exists() {
            return Ok(cand);
        }
    }
    // Fall back to the bare name and let the OS loader search (may still work).
    Ok(PathBuf::from("libmpv-2.dll"))
}

/// Create a borderless child window (the STATIC system class — no class
/// registration needed) inside `parent`, at client-area pixel rect (x,y,w,h).
/// Returns the child HWND as an isize. mpv draws into this via `--wid`.
fn create_child(parent: isize, x: i32, y: i32, w: i32, h: i32) -> Result<isize, String> {
    let class = wide("STATIC");
    let name = wide("");
    // SAFETY: standard Win32 child-window creation with a system class.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            name.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            x,
            y,
            w.max(1),
            h.max(1),
            parent as HWND,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err("CreateWindowExW returned null".into());
    }
    Ok(hwnd as isize)
}

/// Owns the child window and destroys it on drop. Declared AFTER `mpv` in
/// `NativePlayer` so field-drop order tears mpv down first (it stops rendering),
/// then removes the window it was drawing into.
struct WindowGuard(isize);
impl Drop for WindowGuard {
    fn drop(&mut self) {
        // SAFETY: destroy the child window exactly once.
        unsafe { DestroyWindow(self.0 as HWND) };
    }
}

/// A live native player: the mpv instance plus the child window it renders into.
/// Held in managed state so it outlives individual commands (dropping it would
/// tear down playback).
pub struct NativePlayer {
    mpv: Mpv,
    win: WindowGuard, // drops after `mpv` — see WindowGuard
}

// The child HWND is only touched from Tauri command threads, serialised by the
// state mutex; the raw handle isn't auto-Send, so assert it.
unsafe impl Send for NativePlayer {}

impl NativePlayer {
    /// Create the child window under `parent`, embed mpv, and load `path`.
    pub fn start(
        dll: &Path,
        parent: isize,
        path: &Path,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Self, String> {
        let hwnd = create_child(parent, x, y, w, h)?;
        let mpv = match Mpv::embed(dll, hwnd as i64) {
            Ok(m) => m,
            Err(e) => {
                // SAFETY: undo the child window we just created on failure.
                unsafe { DestroyWindow(hwnd as HWND) };
                return Err(e);
            }
        };
        mpv.loadfile(path)?;
        Ok(NativePlayer {
            mpv,
            win: WindowGuard(hwnd),
        })
    }

    pub fn load(&self, path: &Path) -> Result<(), String> {
        self.mpv.loadfile(path)
    }

    pub fn set_rect(&self, x: i32, y: i32, w: i32, h: i32) {
        // SAFETY: reposition the child within its parent's client area.
        unsafe {
            SetWindowPos(
                self.win.0 as HWND,
                std::ptr::null_mut(),
                x,
                y,
                w.max(1),
                h.max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub fn set_visible(&self, visible: bool) {
        // SAFETY: show/hide the child window.
        unsafe { ShowWindow(self.win.0 as HWND, if visible { SW_SHOW } else { SW_HIDE }) };
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        self.mpv.set_paused(paused)
    }

    pub fn seek_abs(&self, secs: f64) -> Result<(), String> {
        self.mpv.seek_abs(secs)
    }

    pub fn command(&self, cmd: &str) -> Result<(), String> {
        self.mpv.command(cmd)
    }
}

/// Managed Tauri state holding the (at most one) live native player.
#[derive(Default)]
pub struct NativeVideoState(pub parking_lot::Mutex<Option<NativePlayer>>);
