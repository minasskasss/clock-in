//! Android (ARCHITECTURE §10): starting the app core, and keeping the
//! Kotlin alarm code supplied with the plan, the alert mode and the server
//! config while the app runs.
//!
//! Calls into Kotlin block until the Android main thread answers, so none of
//! this may run on the main thread: the core starts on its own thread, and
//! the loop makes its calls with `spawn_blocking`.

use crate::android_plan::bridge_plan;
use crate::profile::Profile;
use crate::secrets::PlatformSecrets;
use crate::state::{AppState, Phase};
use jiff::Timestamp;
use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_clockin_alarm::{AlarmBridge, BridgeItem, PermissionStatus};
use tokio::sync::Notify;

/// How often the checklist is re-read (the user may come back from Android's settings).
const STATUS_EVERY: Duration = Duration::from_secs(3);
/// The plan is sent again at least this often, so its horizon keeps moving.
const RESEND_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Android-only state the commands read.
#[derive(Default)]
pub struct Android {
    status: RwLock<Option<PermissionStatus>>,
    /// Re-send the plan now (alert mode changed).
    pub wake: Notify,
}

impl Android {
    /// The checklist as last read (`None` before the first read).
    #[must_use]
    pub fn status(&self) -> Option<PermissionStatus> {
        self.status
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_status(&self, status: PermissionStatus) {
        *self.status.write().unwrap_or_else(PoisonError::into_inner) = Some(status);
    }
}

/// Runs a Kotlin call off the async workers and the main thread. `None` if
/// the plugin isn't there or the call panicked.
pub async fn bridge<T: Send + 'static>(
    app: &AppHandle,
    call: impl FnOnce(&AlarmBridge<Wry>) -> T + Send + 'static,
) -> Option<T> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.try_state::<AlarmBridge<Wry>>().map(|b| call(&b))
    })
    .await
    .ok()
    .flatten()
}

/// Opens the local database (its key and the device secret come from
/// Android Keystore through Kotlin), starts syncing and the alarm feed, and
/// hands the state to Tauri. Runs on its own thread; until it is done,
/// commands answer "not ready" and the UI shows "loading".
///
/// # Errors
///
/// If the secret store or the local database can't be opened.
pub fn start_app(app: &AppHandle, data_dir: &Path) -> Result<(), String> {
    log::info!("starting in {}", data_dir.display());
    std::fs::create_dir_all(data_dir).map_err(|e| e.kind().to_string())?;
    let (state, secret) = AppState::open(
        Profile::default(),
        data_dir,
        Box::new(PlatformSecrets::new(app.clone())),
        crate::config::server_config(),
    )
    .map_err(|e| format!("{e:?}"))?;
    log::info!("local database open");
    let state = Arc::new(state);
    if let Some(bridge) = app.try_state::<AlarmBridge<Wry>>() {
        if let Ok(name) = bridge.device_name() {
            state.set_default_device_name(&name);
        }
        if let Some(config) = crate::config::server_config()
            && let Err(e) = bridge.set_server_config(config.base_url(), &config.publishable_key)
        {
            log::error!("{e}");
        }
    }
    if let Some(secret) = secret
        && state.phase() != Phase::NotConfigured
    {
        state.start_sync(secret).map_err(|e| format!("{e:?}"))?;
    }
    let android = Arc::new(Android::default());
    app.manage(Arc::clone(&state));
    app.manage(Arc::clone(&android));
    tauri::async_runtime::spawn(feed(app.clone(), state, android));
    log::info!("started");
    Ok(())
}

/// Sends Kotlin the plan whenever it changes (new data, a mark, the alert
/// mode, pairing) and reads the permission checklist every few seconds.
async fn feed(app: AppHandle, state: Arc<AppState>, android: Arc<Android>) {
    let mut sent: Option<(Vec<BridgeItem>, &'static str, i64)> = None;
    let mut sent_at = Instant::now();
    loop {
        let snapshot = match state.core_snapshot() {
            Ok(snapshot) if state.phase() == Phase::Paired => snapshot,
            _ => None,
        };
        // Kotlin compares with the phone's real clock, so the plan does too
        // (the debug fake clock only moves the screens).
        let plan = bridge_plan(
            snapshot.as_ref(),
            state.alert_mode().as_str(),
            state.config_version().unwrap_or(0),
            Timestamp::now(),
        );
        let key = (plan.items.clone(), plan.alert_mode, plan.config_version);
        if sent.as_ref() != Some(&key) || sent_at.elapsed() > RESEND_EVERY {
            match bridge(&app, move |b| b.set_plan(&plan)).await {
                Some(Ok(())) => {
                    sent = Some(key);
                    sent_at = Instant::now();
                }
                Some(Err(e)) => log::error!("{e}"),
                None => {}
            }
        }
        if let Some(Ok(status)) = bridge(&app, |b| b.permission_status()).await {
            android.set_status(status);
        }
        tokio::select! {
            () = tokio::time::sleep(STATUS_EVERY) => {}
            () = state.alarm_wake.notified() => {}
            () = android.wake.notified() => {}
        }
    }
}
