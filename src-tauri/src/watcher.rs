//! Windows: the restart watcher (PLAN Phase 6, DECISIONS 2026-10-07).
//!
//! The installed app runs as two processes of the same `clock-in.exe`: this
//! watcher, which shows no window, and the app it starts with `--watched`.
//! If the app ends any other way than Tray → Quit with the quit code, the
//! watcher adds a line to `crash.log` and starts it again 2 seconds later,
//! with its window showing. After 5 crashes within 10 minutes
//! (`clockin_core::record_crash`) it gives up, says so on screen and ends.
//!
//! - Exit code 0 means "don't restart": Tray → Quit with the code, or a
//!   second copy that handed over to the one already running.
//! - Signing out, shutting down, and an installer closing the app for an
//!   update (Windows' Restart Manager) end the watcher too, without a
//!   restart: a hidden top-level window receives `WM_QUERYENDSESSION` /
//!   `WM_ENDSESSION`, and `SM_SHUTTINGDOWN` is checked before each restart.
//!
//! Release builds of the default profile always start under the watcher; a
//! debug build only with `--watch` (to test it with a deliberate crash).

use crate::crash;
use crate::i18n::text;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics, MB_ICONERROR,
    MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MSG, MessageBoxW, RegisterClassW, SM_SHUTTINGDOWN,
    TranslateMessage, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
};

/// Passed to the app the watcher starts.
pub const WATCHED: &str = "--watched";

/// Debug builds: run under the watcher.
pub const WATCH: &str = "--watch";

/// Windows is ending the session or an installer is closing the app.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// Whether this process should be the watcher rather than the app.
#[must_use]
pub fn wanted() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    wanted_for(&args, cfg!(debug_assertions))
}

fn wanted_for(args: &[String], debug: bool) -> bool {
    if args.iter().any(|a| a == WATCHED) {
        return false;
    }
    !debug || args.iter().any(|a| a == WATCH)
}

/// The app's arguments: the watcher's own, without `--watch`, plus
/// `--watched`. After a crash, without `--autostart`, so the window shows.
fn child_args(args: &[String], restarted: bool) -> Vec<String> {
    args.iter()
        .filter(|a| *a != WATCH && !(restarted && *a == crate::autostart::FLAG))
        .cloned()
        .chain(std::iter::once(WATCHED.to_owned()))
        .collect()
}

/// `%LOCALAPPDATA%\<identifier>[\profile-<name>]\crash.log`, the same
/// folder Tauri gives the app.
fn log_path() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(crate::profile::IDENTIFIER);
    let dir = crate::profile::Profile::from_args().data_dir(&base);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(crash::LOG_FILE))
}

fn log(path: Option<&Path>, what: &str) {
    if let Some(path) = path {
        let _ = crash::append_line(path, &crash::line(jiff::Timestamp::now(), "watcher", what));
    }
}

#[allow(unsafe_code)]
fn shutting_down() -> bool {
    // SAFETY: a plain query, no pointers.
    STOPPING.load(Ordering::SeqCst) || unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) } != 0
}

/// Runs the watcher until the app quits for good. Never returns.
pub fn run() -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let log_path = log_path();
    let log_path = log_path.as_deref();
    let Ok(exe) = std::env::current_exe() else {
        log(
            log_path,
            "cannot find its own program file; not starting the app",
        );
        std::process::exit(1);
    };
    let _ = std::thread::Builder::new()
        .name("clock-in-watcher-window".into())
        .spawn(session_window);

    let mut crashes = Vec::new();
    let mut restarted = false;
    loop {
        let ended = Command::new(&exe)
            .args(child_args(&args, restarted))
            .status();
        let reason = match ended {
            Ok(status) if status.success() => std::process::exit(0),
            Ok(status) => match status.code() {
                // Shown as Windows does (e.g. 0xC0000409 for a Rust panic).
                Some(code) => format!(
                    "the app ended unexpectedly (exit code 0x{:08X})",
                    code.cast_unsigned()
                ),
                None => "the app ended unexpectedly".to_owned(),
            },
            Err(e) => format!("could not start the app: {e}"),
        };
        if shutting_down() {
            std::process::exit(0);
        }
        if !clockin_core::record_crash(&mut crashes, jiff::Timestamp::now()) {
            log(
                log_path,
                &format!(
                    "{reason}; gave up after {} crashes within {} minutes, not restarting",
                    clockin_core::MAX_CRASHES,
                    clockin_core::CRASH_WINDOW.as_mins()
                ),
            );
            gave_up_message();
            std::process::exit(1);
        }
        log(log_path, &format!("{reason}; restarting it"));
        let delay =
            Duration::try_from(clockin_core::RESTART_DELAY).unwrap_or(Duration::from_secs(2));
        let step = Duration::from_millis(100);
        let mut waited = Duration::ZERO;
        while waited < delay {
            std::thread::sleep(step);
            waited += step;
            if shutting_down() {
                std::process::exit(0);
            }
        }
        restarted = true;
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Tells whoever is at the PC that alarms have stopped.
#[allow(unsafe_code)]
fn gave_up_message() {
    let (title, body) = (
        wide(&text("watcher.gaveUpTitle")),
        wide(&text("watcher.gaveUp")),
    );
    // SAFETY: null-terminated UTF-16 strings that outlive the call; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

/// A hidden top-level window (never shown), so Windows and the Restart
/// Manager can tell the watcher that the session or the app is ending.
#[allow(unsafe_code)]
fn session_window() {
    let class = wide("ClockInWatcher");
    // SAFETY: the class name outlives the window (this thread runs until the
    // process ends); `session_proc` matches WNDPROC; the window is never
    // shown and lives on this thread, which pumps its messages.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(session_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[allow(unsafe_code)]
unsafe extern "system" fn session_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_QUERYENDSESSION => {
            STOPPING.store(true, Ordering::SeqCst);
            1
        }
        WM_ENDSESSION => {
            if wparam == 0 {
                // The session goes on after all.
                STOPPING.store(false, Ordering::SeqCst);
            } else {
                // Let go of the program file at once (an update replaces it).
                std::process::exit(0);
            }
            0
        }
        // SAFETY: the default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn release_builds_always_watch_debug_builds_only_when_asked() {
        assert!(wanted_for(&args(&[]), false));
        assert!(wanted_for(&args(&["--autostart"]), false));
        assert!(!wanted_for(&args(&[]), true));
        assert!(wanted_for(&args(&["--watch"]), true));
        // The app the watcher started never watches itself.
        assert!(!wanted_for(&args(&["--watched"]), false));
        assert!(!wanted_for(&args(&["--watch", "--watched"]), true));
    }

    #[test]
    fn the_app_gets_the_arguments_and_shows_its_window_after_a_crash() {
        let given = args(&["--autostart", "--watch", "--profile", "b"]);
        assert_eq!(
            child_args(&given, false),
            args(&["--autostart", "--profile", "b", "--watched"])
        );
        assert_eq!(
            child_args(&given, true),
            args(&["--profile", "b", "--watched"])
        );
    }

    #[test]
    fn its_message_exists() {
        for key in ["watcher.gaveUpTitle", "watcher.gaveUp"] {
            assert_ne!(text(key), key, "missing in el.json: {key}");
        }
    }
}
