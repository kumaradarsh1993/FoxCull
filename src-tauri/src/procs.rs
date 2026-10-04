//! Running ffmpeg children and half-written files: what a Pause, a Stop or a
//! Quit has to reach.
//!
//! - **Pause/Resume** (merges): the merge's ffmpeg is suspended in place
//!   (SIGSTOP/SIGCONT; NtSuspendProcess/NtResumeProcess on Windows). Its files
//!   stay open and it carries on exactly where it was. A convert-merge runs one
//!   ffmpeg per clip, so the paused state is a flag too: a part that starts
//!   while paused is suspended the moment it's registered.
//! - **Quit**: a child process outlives its parent on macOS and Linux, so
//!   quitting FoxCull mid-merge used to leave ffmpeg writing a file nobody was
//!   waiting for. On exit every registered child is killed and its partial
//!   output removed, along with scratch paths (a convert-merge's work folder,
//!   a copy that hadn't finished).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::LazyLock;

use parking_lot::Mutex;

#[derive(Clone, Copy)]
struct Proc {
    #[cfg(unix)]
    pid: i32,
    #[cfg(windows)]
    handle: isize,
}

#[cfg(windows)]
#[link(name = "ntdll")]
extern "system" {
    fn NtSuspendProcess(process: *mut std::ffi::c_void) -> i32;
    fn NtResumeProcess(process: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
extern "system" {
    fn TerminateProcess(process: *mut std::ffi::c_void, exit_code: u32) -> i32;
}

impl Proc {
    fn of(child: &Child) -> Self {
        #[cfg(unix)]
        {
            Proc { pid: child.id() as i32 }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            Proc { handle: child.as_raw_handle() as isize }
        }
    }

    fn suspend(self) -> bool {
        #[cfg(unix)]
        {
            unsafe { libc::kill(self.pid, libc::SIGSTOP) == 0 }
        }
        #[cfg(windows)]
        {
            unsafe { NtSuspendProcess(self.handle as *mut std::ffi::c_void) >= 0 }
        }
    }

    fn resume(self) -> bool {
        #[cfg(unix)]
        {
            unsafe { libc::kill(self.pid, libc::SIGCONT) == 0 }
        }
        #[cfg(windows)]
        {
            unsafe { NtResumeProcess(self.handle as *mut std::ffi::c_void) >= 0 }
        }
    }

    fn kill(self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
        }
        #[cfg(windows)]
        unsafe {
            TerminateProcess(self.handle as *mut std::ffi::c_void, 1);
        }
    }
}

struct Entry {
    proc: Proc,
    /// The file this child is writing, removed if FoxCull quits under it.
    partial: Option<PathBuf>,
    /// Part of the running merge (the one Pause applies to).
    merge: bool,
    /// Suspended by us. Tracked so Pause/Resume stay idempotent: Windows
    /// counts suspensions, so two suspends would need two resumes.
    suspended: bool,
}

static RUNNING: LazyLock<Mutex<HashMap<u64, Entry>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static SCRATCH: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static SEQ: AtomicU64 = AtomicU64::new(0);
static MERGE_PAUSED: AtomicBool = AtomicBool::new(false);

/// Track a running ffmpeg. Hand the returned token back to `unregister`
/// before the `Child` is dropped (on Windows its handle dies with it).
pub fn register(child: &Child, partial: Option<&Path>, merge: bool) -> u64 {
    let id = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let mut map = RUNNING.lock();
    let mut e = Entry { proc: Proc::of(child), partial: partial.map(Path::to_path_buf), merge, suspended: false };
    // Started while the merge is paused (the next clip of a conversion):
    // hold it until Resume.
    if merge && MERGE_PAUSED.load(Ordering::SeqCst) {
        e.suspended = e.proc.suspend();
    }
    map.insert(id, e);
    id
}

pub fn unregister(id: u64) {
    RUNNING.lock().remove(&id);
}

/// Pause or resume the running merge. Returns whether anything changed.
pub fn set_merge_paused(paused: bool) -> bool {
    let was = MERGE_PAUSED.swap(paused, Ordering::SeqCst);
    let mut map = RUNNING.lock();
    for e in map.values_mut().filter(|e| e.merge) {
        if paused && !e.suspended {
            e.suspended = e.proc.suspend();
        } else if !paused && e.suspended {
            e.proc.resume();
            e.suspended = false;
        }
    }
    was != paused
}

pub fn merge_paused() -> bool {
    MERGE_PAUSED.load(Ordering::SeqCst)
}

/// A path to remove if FoxCull quits before it's finished with it.
pub fn scratch_add(p: &Path) {
    SCRATCH.lock().insert(p.to_path_buf());
}

pub fn scratch_remove(p: &Path) {
    SCRATCH.lock().remove(p);
}

/// Anything still running or half-written (asked before quitting).
pub fn busy() -> bool {
    !RUNNING.lock().is_empty() || !SCRATCH.lock().is_empty()
}

/// Quitting: stop every child and remove what it was writing.
pub fn kill_all_and_clean() {
    MERGE_PAUSED.store(false, Ordering::SeqCst);
    let entries: Vec<Entry> = RUNNING.lock().drain().map(|(_, e)| e).collect();
    for e in &entries {
        // A suspended process can still be killed; resume anyway so it
        // releases its files promptly on every platform.
        if e.suspended {
            e.proc.resume();
        }
        e.proc.kill();
    }
    // Give the kernel a moment to close the files before deleting them.
    if !entries.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    for e in entries {
        if let Some(p) = e.partial {
            let _ = std::fs::remove_file(p);
        }
    }
    for p in SCRATCH.lock().drain() {
        if p.is_dir() {
            let _ = std::fs::remove_dir_all(&p);
        } else {
            let _ = std::fs::remove_file(&p);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A paused merge child really stops (its clock doesn't advance) and
    /// resumes; a child registered while paused starts suspended.
    #[test]
    fn pause_resume_and_quit_cleanup() {
        let dir = std::env::temp_dir().join(format!("foxcull-procs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let partial = dir.join("partial.mp4");
        std::fs::write(&partial, b"x").unwrap();
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let id = register(&child, Some(&partial), true);
        assert!(set_merge_paused(true));
        assert!(merge_paused());
        // A second pause is a no-op, not a second suspend.
        assert!(!set_merge_paused(true));
        let state = || {
            String::from_utf8(std::process::Command::new("ps").args(["-o", "stat=", "-p", &child.id().to_string()]).output().unwrap().stdout)
                .unwrap()
                .trim()
                .to_string()
        };
        assert!(state().starts_with('T'), "stopped: {}", state());
        assert!(set_merge_paused(false));
        assert!(!state().starts_with('T'), "running again: {}", state());

        // Registered while paused → suspended straight away.
        set_merge_paused(true);
        let mut late = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let late_id = register(&late, None, true);
        let late_state = String::from_utf8(std::process::Command::new("ps").args(["-o", "stat=", "-p", &late.id().to_string()]).output().unwrap().stdout).unwrap();
        assert!(late_state.trim().starts_with('T'), "late part held: {late_state}");

        let scratch = dir.join(".foxcull-merge-test");
        std::fs::create_dir_all(&scratch).unwrap();
        scratch_add(&scratch);
        assert!(busy());
        kill_all_and_clean();
        assert!(child.wait().is_ok() && late.wait().is_ok());
        assert!(!partial.exists(), "partial output removed on quit");
        assert!(!scratch.exists(), "scratch removed on quit");
        assert!(!busy());
        unregister(id);
        unregister(late_id);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
