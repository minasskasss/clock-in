//! Clock In app core: Tauri setup. The sync client and local store live in
//! `clockin-sync`; the UI wiring, scheduler, audio and tray arrive in later
//! phases (see `docs/PLAN.md`).

pub mod config;

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime cannot start (e.g. no webview available).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clockin_alarm::init())
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
