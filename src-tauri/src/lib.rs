//! Clock In app core: Tauri setup, the sync loop and the commands the UI
//! calls. The sync client and local store live in `clockin-sync`, all time
//! logic in `clockin-core`. On Windows the app also runs the alarm scheduler,
//! the alarm window and sound, the tray, keep-awake and start-with-Windows
//! (ARCHITECTURE §9). On Android it feeds the Kotlin alarm plugin
//! (ARCHITECTURE §10).

#[cfg(desktop)]
mod alarms;
#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "android", test))]
mod android_plan;
#[cfg(desktop)]
mod audio;
#[cfg(desktop)]
mod autostart;
mod clock;
mod commands;
pub mod config;
mod crash;
mod drafts;
#[cfg(test)]
mod e2e;
mod error;
#[cfg(any(desktop, test))]
mod i18n;
#[cfg(windows)]
mod keyboard_guard;
mod passgen;
#[cfg(windows)]
mod power;
mod profile;
mod secrets;
mod state;
#[cfg(desktop)]
mod tray;
mod views;
#[cfg(windows)]
mod volume;
#[cfg(windows)]
pub mod watcher;
#[cfg(windows)]
mod window_icon;

#[cfg(desktop)]
use crate::alarms::Alarms;
#[cfg(desktop)]
use crate::profile::Profile;
#[cfg(desktop)]
use crate::secrets::PlatformSecrets;
#[cfg(desktop)]
use crate::state::AppState;
#[cfg(desktop)]
use std::sync::Arc;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Android: the window now, the app core on its own thread (its secrets come
/// through the Kotlin plugin, which answers on this, the main thread).
#[cfg(target_os = "android")]
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("ClockIn")
            .with_max_level(log::LevelFilter::Info),
    );
    let data_dir = app.path().app_local_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    crash::install(data_dir.clone());
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default()).build()?;
    let handle = app.handle().clone();
    std::thread::Builder::new()
        .name("clock-in-start".into())
        .spawn(move || {
            if let Err(e) = android::start_app(&handle, &data_dir) {
                log::error!("could not start: {e}");
            }
        })?;
    Ok(())
}

#[cfg(desktop)]
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let profile = Profile::from_args();
    let data_dir = profile.data_dir(&app.path().app_local_data_dir()?);
    std::fs::create_dir_all(&data_dir)?;
    // First, so a failed start below is in crash.log too.
    crash::install(data_dir.clone());
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
    app.manage(Arc::clone(&state));

    let title = match profile.name() {
        None => i18n::text("app.name"),
        Some(name) => format!("{} ({name})", i18n::text("app.name")),
    };
    // A debug profile keeps its own webview data (theme) next to its database.
    let webview_dir = profile.name().map(|_| data_dir.join("webview"));
    let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title(title)
        .inner_size(1100.0, 760.0)
        .min_inner_size(480.0, 560.0)
        .center()
        // Started at login: straight to the tray (PLAN Phase 4, item 8).
        .visible(!autostart::started_at_login());
    if let Some(dir) = &webview_dir {
        window = window.data_directory(dir.clone());
    }
    let window = window.build()?;
    #[cfg(windows)]
    {
        window_icon::apply(window.hwnd()?.0, window.scale_factor()?);
        // `setup` runs on the main thread, which owns the window.
        keyboard_guard::install(window.hwnd()?.0);
    }

    let alarms = Arc::new(Alarms::new(&state, webview_dir));
    app.manage(Arc::clone(&alarms));

    tray::create(app, window.scale_factor()?)?;
    // Only the installed app registers itself, never a debug build or a
    // second debug profile.
    let manage_autostart = !cfg!(debug_assertions) && profile.name().is_none();
    alarms::start(
        app.handle().clone(),
        Arc::clone(&state),
        alarms,
        manage_autostart,
    );

    #[cfg(windows)]
    {
        // setup() runs on the main thread, which lives as long as the app.
        power::keep_awake();
        start_sound_check(Arc::clone(&state));
    }
    Ok(())
}

/// Windows: checks every 30 s whether alarms would be heard (SPEC §6 banner).
#[cfg(windows)]
fn start_sound_check(state: Arc<AppState>) {
    let _ = std::thread::Builder::new()
        .name("clock-in-volume".into())
        .spawn(move || {
            volume::init_thread();
            loop {
                state.set_sound_off(volume::sound_off());
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        });
}

/// Desktop: single instance, and closing hides to the tray.
#[cfg(desktop)]
fn desktop_builder(mut builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    // One instance only (SPEC §8.1): starting it again shows the window. A
    // debug `--profile` instance may run beside the default one.
    if Profile::from_args().name().is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }));
    }
    builder.on_window_event(|window, event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            // Closing hides the main window to the tray; the alarm window
            // closes only through Stop. Quitting needs the quit code.
            api.prevent_close();
            if window.label() == tray::MAIN {
                let _ = window.hide();
            }
        }
        #[cfg(windows)]
        tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            if let Ok(hwnd) = window.hwnd() {
                window_icon::apply(hwnd.0, *scale_factor);
            }
        }
        _ => {}
    })
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime cannot start (e.g. no webview available).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = desktop_builder(builder);
    builder
        .plugin(tauri_plugin_clockin_alarm::init())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::app_state,
            commands::check_new_passphrase,
            commands::initialize,
            commands::pair,
            commands::mark,
            commands::dismiss_refused_mark,
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
            #[cfg(desktop)]
            commands::alarm_state,
            #[cfg(desktop)]
            commands::alarm_stop,
            #[cfg(desktop)]
            commands::quit,
            commands::set_alert_mode,
            commands::android_open_settings,
            commands::android_set_oem_done,
            commands::set_theme,
            commands::android_stop_alarm,
            commands::android_diagnostics,
            commands::android_insets,
            commands::debug_set_clock,
            commands::debug_crash,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
