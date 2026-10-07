//! Windows: keeps "dead key" messages away from tao (DECISIONS 2026-10-07).
//!
//! tao 0.37.1 (and winit, whose keyboard code it copies) panics on a
//! `WM_DEADCHAR` that its window did not see the key-down for
//! (`keyboard.rs`: `event_info.take().unwrap()`), which ends the whole app.
//! A dead key waits for the next letter; the Greek keyboard has three: the
//! tonos ΄, the dialytika ¨ and the dialytika with tonos ΅.
//! The app's own windows never need these messages: typing happens in the
//! WebView's own child window, which this does not touch. So each window gets
//! a subclass that drops every `WM_DEADCHAR` and `WM_SYSDEADCHAR`, whatever
//! the key or keyboard layout, before tao sees them; tao already ignores the
//! character that may follow.

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
    use std::cell::RefCell;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayoutList, HKL, KLF_NOTELLSHELL, LoadKeyboardLayoutW, MAPVK_VK_TO_VSC,
        MapVirtualKeyExW, ToUnicodeEx, UnloadKeyboardLayout, VK_CONTROL, VK_MENU, VK_OEM_1,
        VK_SHIFT, VK_W,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassW, SendMessageW,
        WM_CHAR, WM_KEYDOWN, WNDCLASSW,
    };

    /// The Greek keyboard's dead keys, with a letter each one accents.
    const GREEK_DEAD_KEYS: [(char, char); 3] = [('΄', 'ά'), ('¨', 'ϊ'), ('΅', 'ΐ')];

    thread_local! {
        /// The key messages the window procedure under the guard received.
        static SEEN: RefCell<Vec<(u32, usize)>> = const { RefCell::new(Vec::new()) };
    }

    fn seen() -> Vec<(u32, usize)> {
        SEEN.with_borrow_mut(std::mem::take)
    }

    unsafe extern "system" fn record(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if matches!(msg, WM_KEYDOWN | WM_CHAR | WM_DEADCHAR | WM_SYSDEADCHAR) {
            SEEN.with_borrow_mut(|seen| seen.push((msg, wparam)));
        }
        // SAFETY: default handling for everything else.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// A message-only window of this thread that records key messages, with
    /// the guard installed.
    fn guarded_window() -> HWND {
        let class: Vec<u16> = "ClockInKeyboardGuardTest\0".encode_utf16().collect();
        // SAFETY: registers a class (again is harmless) and creates a
        // message-only window on this thread; the tests destroy it.
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
        hwnd
    }

    fn send(hwnd: HWND, msg: u32, wparam: usize) {
        // SAFETY: synchronous send to our own window.
        unsafe { SendMessageW(hwnd, msg, wparam, 0x0027_0001) };
    }

    fn destroy(hwnd: HWND) {
        // SAFETY: our window; the guard removes itself on WM_NCDESTROY.
        assert_ne!(unsafe { DestroyWindow(hwnd) }, 0);
    }

    #[test]
    fn every_greek_dead_key_is_dropped_and_the_accented_letter_arrives() {
        let hwnd = guarded_window();
        for (dead, letter) in GREEK_DEAD_KEYS {
            // With Alt held, Windows sends the "system" variant.
            for dead_msg in [WM_DEADCHAR, WM_SYSDEADCHAR] {
                send(hwnd, WM_KEYDOWN, VK_OEM_1.into());
                send(hwnd, dead_msg, dead as usize);
                send(hwnd, WM_KEYDOWN, 0x41);
                send(hwnd, WM_CHAR, letter as usize);
                assert_eq!(
                    seen(),
                    [
                        (WM_KEYDOWN, VK_OEM_1.into()),
                        (WM_KEYDOWN, 0x41),
                        (WM_CHAR, letter as usize),
                    ],
                    "{dead} {dead_msg:#x}"
                );
            }
        }
        destroy(hwnd);
    }

    /// Reads the Windows "Greek" keyboard layout (loaded only for this test if
    /// it isn't already) to check that the dead keys above are all of them.
    #[test]
    fn the_greek_keyboard_has_no_other_dead_keys() {
        const GREEK: usize = 0x0408_0408;
        // SAFETY: fills a buffer of the size Windows asked for.
        let loaded = unsafe {
            let mut list =
                vec![std::ptr::null_mut(); GetKeyboardLayoutList(0, std::ptr::null_mut()) as usize];
            let n = GetKeyboardLayoutList(list.len() as i32, list.as_mut_ptr());
            list.truncate(n.max(0) as usize);
            list.iter().any(|&hkl: &HKL| hkl as usize == GREEK)
        };
        let id: Vec<u16> = "00000408\0".encode_utf16().collect();
        // SAFETY: loads the layout without making it active or telling the shell.
        let hkl = unsafe { LoadKeyboardLayoutW(id.as_ptr(), KLF_NOTELLSHELL) };
        assert_eq!(hkl as usize, GREEK);

        let mut dead = Vec::new();
        for (name, modifiers) in [
            ("", &[][..]),
            ("Shift+", &[VK_SHIFT][..]),
            ("AltGr+", &[VK_CONTROL, VK_MENU][..]),
            ("Shift+AltGr+", &[VK_SHIFT, VK_CONTROL, VK_MENU][..]),
        ] {
            let mut keys = [0u8; 256];
            for &m in modifiers {
                keys[usize::from(m)] = 0x80;
            }
            for vk in 1..255 {
                let mut out = [0u16; 8];
                // SAFETY: valid buffers; flag 4 leaves the keyboard state alone.
                let n = unsafe {
                    let scan = MapVirtualKeyExW(vk, MAPVK_VK_TO_VSC, hkl);
                    ToUnicodeEx(vk, scan, keys.as_ptr(), out.as_mut_ptr(), 8, 4, hkl)
                };
                if n < 0 {
                    dead.push((
                        format!("{name}{vk:#x}"),
                        char::from_u32(out[0].into()).unwrap(),
                    ));
                }
            }
        }
        if !loaded {
            // SAFETY: unloads only what this test loaded.
            unsafe { UnloadKeyboardLayout(hkl) };
        }

        let key = |vk: u16| format!("{vk:#x}");
        assert_eq!(
            dead,
            [
                (key(VK_OEM_1), '΄'),
                (format!("Shift+{}", key(VK_W)), '΅'),
                (format!("Shift+{}", key(VK_OEM_1)), '¨'),
                (format!("AltGr+{}", key(VK_OEM_1)), '΅'),
            ]
        );
        for (_, c) in dead {
            assert!(GREEK_DEAD_KEYS.iter().any(|&(d, _)| d == c), "{c}");
        }
    }
}
