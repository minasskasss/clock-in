//! Clock In app core: Tauri setup, the sync loop and the commands the UI
//! calls. The sync client and local store live in `clockin-sync`, all time
//! logic in `clockin-core`. The alarm scheduler, audio and tray arrive in
//! Phase 4 (see `docs/PLAN.md`).

mod clock;
mod commands;
pub mod config;
mod drafts;
#[cfg(test)]
mod e2e;
mod error;
mod passgen;
mod profile;
mod secrets;
mod state;
mod views;
#[cfg(windows)]
mod window_icon;

use crate::profile::Profile;
use crate::secrets::PlatformSecrets;
use crate::state::AppState;
use std::sync::Arc;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let profile = Profile::from_args();
    let data_dir = profile.data_dir(&app.path().app_local_data_dir()?);
    std::fs::create_dir_all(&data_dir)?;
    let secrets = PlatformSecrets::new(profile.credential_service(), &data_dir)?;
    let (state, secret) = AppState::open(
        profile.clone(),
        &data_dir,
        Box::new(secrets),
        config::server_config(),
    )
    .map_err(|e| format!("Clock In could not start: {e:?}"))?;
    let state = Arc::new(state);
    if let Some(secret) = secret
        && state.phase() != state::Phase::NotConfigured
    {
        state
            .start_sync(secret)
            .map_err(|e| format!("Clock In could not start syncing: {e:?}"))?;
    }
    app.manage(state);

    let title = match profile.name() {
        None => "Clock In".to_owned(),
        Some(name) => format!("Clock In ({name})"),
    };
    let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title(title)
        .inner_size(1100.0, 760.0)
        .min_inner_size(480.0, 560.0)
        .center();
    if profile.name().is_some() {
        // Its own webview data (language, theme) next to its own database.
        window = window.data_directory(data_dir.join("webview"));
    }
    let _window = window.build()?;
    #[cfg(windows)]
    window_icon::apply(_window.hwnd()?.0, _window.scale_factor()?);
    Ok(())
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime cannot start (e.g. no webview available).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clockin_alarm::init())
        .setup(setup)
        .on_window_event(|_window, _event| {
            #[cfg(windows)]
            if let tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } = _event
                && let Ok(hwnd) = _window.hwnd()
            {
                window_icon::apply(hwnd.0, *scale_factor);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_state,
            commands::check_new_passphrase,
            commands::initialize,
            commands::pair,
            commands::mark,
            commands::admin_login,
            commands::admin_logout,
            commands::admin_touch,
            commands::admin_view,
            commands::validate_week,
            commands::validate_override,
            commands::staff_save,
            commands::staff_remove,
            commands::override_save,
            commands::override_delete,
            commands::settings_save,
            commands::mark_void,
            commands::device_revoke,
            commands::change_passphrase,
            commands::generate_passphrase,
            commands::debug_set_clock,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
