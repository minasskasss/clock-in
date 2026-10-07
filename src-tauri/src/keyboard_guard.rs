//! Windows: keeps "dead key" messages away from tao (DECISIONS 2026-10-07).
//!
//! tao 0.37.1 (and winit, whose keyboard code it copies) panics on a
//! `WM_DEADCHAR` that its window did not see the key-down for
//! (`keyboard.rs`: `event_info.take().unwrap()`), which ends the whole app.
//! Greek keyboards have such a key: the tonos (΄) waits for the next letter.
//! The app's own windows never need these messages: typing happens in the
//! WebView's own child window, which this does not touch. So each window gets
//! a subclass that drops `WM_DEADCHAR` and `WM_SYSDEADCHAR` before tao sees
//! them; tao already ignores the character that may follow.

use std::ffi::c_void;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_DEADCHAR, WM_NCDESTROY, WM_SYSDEADCHAR};

/// This subclass's id ("CKDG"); any value unique to this window procedure.
const ID: usize = 0x434b_4447;

/// Whether the guard drops this message.
const fn dropped(msg: u32) -> bool {
    matches!(msg, WM_DEADCHAR | WM_SYSDEADCHAR)
}

/// Installs the guard on one of the app's windows. Must run on the thread that
/// owns the window (the main thread). Installing it twice is harmless.
#[allow(unsafe_code)]
pub fn install(hwnd: *mut c_void) -> bool {
    // SAFETY: `hwnd` is a live window of this thread; `guard` matches
    // SUBCLASSPROC and removes itself on WM_NCDESTROY.
    unsafe { SetWindowSubclass(hwnd as HWND, Some(guard), ID, 0) != 0 }
}

#[allow(unsafe_code)]
unsafe extern "system" fn guard(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if dropped(msg) {
        return 0;
    }
    if msg == WM_NCDESTROY {
        // SAFETY: removing this subclass from its own window.
        unsafe { RemoveWindowSubclass(hwnd, Some(guard), ID) };
    }
    // SAFETY: passes the message on to tao's window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassW, SendMessageW,
        WM_CHAR, WM_KEYDOWN, WNDCLASSW,
    };

    /// The messages the window procedure under the guard received.
    static SEEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());

    unsafe extern "system" fn record(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if matches!(msg, WM_KEYDOWN | WM_CHAR | WM_DEADCHAR | WM_SYSDEADCHAR) {
            SEEN.lock().unwrap().push(msg);
        }
        // SAFETY: default handling for everything else.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    #[test]
    fn dead_keys_never_reach_the_window_and_other_keys_do() {
        let class: Vec<u16> = "ClockInKeyboardGuardTest\0".encode_utf16().collect();
        // SAFETY: a message-only window on this thread, destroyed below.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(record),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&raw const wc);
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        assert!(install(hwnd));
        assert!(install(hwnd), "installing again is harmless");
        for msg in [WM_KEYDOWN, WM_DEADCHAR, WM_SYSDEADCHAR, WM_CHAR] {
            // SAFETY: synchronous send to our own window.
            unsafe { SendMessageW(hwnd, msg, 0x0384, 0x0027_0001) };
        }
        assert_eq!(*SEEN.lock().unwrap(), [WM_KEYDOWN, WM_CHAR]);
        // SAFETY: our window; the guard removes itself on WM_NCDESTROY.
        assert_ne!(unsafe { DestroyWindow(hwnd) }, 0);
    }
}
