//! Windows: each window gets the `icon.ico` layers drawn for its sizes.
//!
//! Tauri gives a window one icon, the 32 px layer, as its small icon only,
//! and Windows shrinks that for the title bar. The shop PC runs at 100 %,
//! where Windows draws the title bar at 16 px and the taskbar at 24 px
//! (125 %: 20 and 30), and `icon.ico` has hand-tuned layers for those sizes
//! (`tools/build-icons.mjs`). So the small icon (title bar) is loaded at
//! 16 px Ã— scale and the big icon (taskbar) at 24 px Ã— scale, each at its
//! exact size from the icon in the .exe, and again when the window moves to
//! a monitor with another scale.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SendMessageW,
    WM_SETICON,
};

/// Title bar, tray and context menus at 100 % (Microsoft's icon sizes).
const SMALL_PX: f64 = 16.0;
/// Taskbar and Start list at 100 %.
const BIG_PX: f64 = 24.0;

/// The icons this module set, per window, so the previous ones can be freed.
/// Handles are stored as integers (raw pointers are not `Send`).
static SET: Mutex<Option<HashMap<isize, [isize; 2]>>> = Mutex::new(None);

/// The icon size in pixels for `base` at this scale factor.
fn pixels(base: f64, scale: f64) -> i32 {
    // At most a few hundred pixels; the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation)]
    let px = (base * scale).round() as i32;
    px.max(1)
}

/// The small-icon size (title bar, tray) in pixels at this scale factor.
#[must_use]
pub fn small_px(scale: f64) -> i32 {
    pixels(SMALL_PX, scale)
}

/// The app icon from the .exe, at exactly `size` Ã— `size` pixels (Windows
/// picks the matching layer, or scales the nearest larger one), or `None`.
#[allow(unsafe_code)]
fn load(size: i32) -> Option<*mut c_void> {
    let id = tauri::utils::platform::WINDOWS_APP_ICON_RESOURCE_ID;
    // SAFETY: a null module name is this process's .exe; the resource name is
    // MAKEINTRESOURCE(id); LoadImageW returns null on failure, checked below.
    let icon = unsafe {
        LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            usize::from(id) as *const u16,
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    };
    (!icon.is_null()).then_some(icon)
}

/// Sets the title-bar and taskbar icons of window `hwnd` for `scale`.
#[allow(unsafe_code)]
pub fn apply(hwnd: *mut c_void, scale: f64) {
    let (Some(small), Some(big)) = (load(pixels(SMALL_PX, scale)), load(pixels(BIG_PX, scale)))
    else {
        return;
    };
    // SAFETY: `hwnd` is this live window's handle; WM_SETICON takes an HICON
    // in LPARAM, and the window keeps using it until it is replaced.
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
    }
    let mut set = SET
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = set
        .get_or_insert_with(HashMap::new)
        .insert(hwnd as isize, [small as isize, big as isize]);
    // Free only icons this module made; the first ones replaced were Tauri's.
    for old in previous.into_iter().flatten() {
        // SAFETY: `old` came from LoadImageW above and no window uses it any more.
        unsafe {
            DestroyIcon(old as *mut c_void);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_scale() {
        assert_eq!((pixels(SMALL_PX, 1.0), pixels(BIG_PX, 1.0)), (16, 24));
        assert_eq!((pixels(SMALL_PX, 1.25), pixels(BIG_PX, 1.25)), (20, 30));
        assert_eq!((pixels(SMALL_PX, 1.5), pixels(BIG_PX, 1.5)), (24, 36));
        assert_eq!((pixels(SMALL_PX, 2.0), pixels(BIG_PX, 2.0)), (32, 48));
    }
}
