//! The tray icon and quitting (SPEC §8.1, ARCHITECTURE §9).
//!
//! Closing the main window only hides it; the tray menu has Open and Quit.
//! Quit opens the main window with the quit-code keypad (the `quit-requested`
//! event); the app exits only through the `quit` command with the right code.

use crate::i18n::text;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

/// The main window's label.
pub const MAIN: &str = "main";

/// Sent to the main window when Quit is chosen in the tray.
pub const QUIT_REQUESTED: &str = "quit-requested";

/// Adds the tray icon. `scale` is the main window's display scale.
///
/// # Errors
///
/// If Windows refuses the tray icon or its menu.
pub fn create(app: &tauri::App, scale: f64) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", text("tray.open"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", text("tray.quit"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(text("tray.tooltip"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = icon(app, scale) {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}

/// The hand-tuned small layer of `icon.ico` at 16 px × scale (DECISIONS,
/// Phase 3 icon), not the large design shrunk.
#[cfg(windows)]
fn icon(app: &tauri::App, scale: f64) -> Option<tauri::image::Image<'static>> {
    let size = u32::try_from(crate::window_icon::small_px(scale)).unwrap_or(16);
    tauri::image::Image::from_app_icon_resource(size)
        .ok()
        .or_else(|| app.default_window_icon().map(|i| i.clone().to_owned()))
}

#[cfg(not(windows))]
fn icon(app: &tauri::App, _scale: f64) -> Option<tauri::image::Image<'static>> {
    app.default_window_icon().map(|i| i.clone().to_owned())
}

/// Shows, restores and focuses the main window.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Tray → Quit: the main window asks for the quit code.
pub fn request_quit(app: &AppHandle) {
    show_main(app);
    let _ = app.emit_to(MAIN, QUIT_REQUESTED, ());
}
