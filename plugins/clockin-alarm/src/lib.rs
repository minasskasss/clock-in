//! Clock In alarm plugin: the bridge to the Kotlin alarm code on Android
//! (`android/`, ARCHITECTURE §10). Other platforms register an empty plugin.
//!
//! Every call blocks until Kotlin answers on the Android main thread, so
//! call it only from a background thread or an async task, never from the
//! main thread (e.g. Tauri's `setup` or a synchronous command): that would
//! deadlock.

use serde::{Deserialize, Serialize};
use tauri::{
    Runtime,
    plugin::{Builder, TauriPlugin},
};

#[cfg(target_os = "android")]
mod mobile;
#[cfg(target_os = "android")]
pub use mobile::{AlarmBridge, BridgeError};

/// The plugin name, as registered with Tauri.
pub const PLUGIN_NAME: &str = "clockin-alarm";

/// One alarm for one person (`clockin-core::PlanItem`), as Kotlin keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeItem {
    pub item_id: String,
    /// RFC 3339; sent back unchanged to `check_alarm`.
    pub fires_at: String,
    /// `in` or `out`.
    pub kind: &'static str,
    pub name: String,
}

/// A period, in epoch milliseconds, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWindow {
    pub start: i64,
    pub end: i64,
}

/// What Kotlin schedules from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgePlan {
    pub items: Vec<BridgeItem>,
    /// `ring` or `notification`.
    pub alert_mode: &'static str,
    /// The server `config_version` the plan was computed from.
    pub config_version: i64,
    /// RFC 3339.
    pub horizon_end: Option<String>,
    /// This phone's theme for the alarm screen: `auto`, `system`, `light` or `dark`.
    pub theme: &'static str,
    /// When the automatic theme is dark (`clockin-core::auto_dark_windows`),
    /// so Kotlin only compares instants.
    pub dark_windows: Vec<BridgeWindow>,
}

/// The ring-mode alarm cycle in progress on this phone, if any.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmStatus {
    pub active: bool,
    /// False during the silent minutes between rings.
    pub ringing: bool,
    pub check_in: Vec<String>,
    pub check_out: Vec<String>,
}

/// What the diagnostics view shows from Kotlin. Times are epoch milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Diagnostics {
    pub manufacturer: String,
    pub model: String,
    /// e.g. "13".
    pub android_version: String,
    pub sdk: i32,
    /// MIUI or HyperOS version, or "".
    pub maker_os: String,
    pub last_refresh_at: Option<i64>,
    pub last_refresh_ok: Option<bool>,
    pub next_alarm_at: Option<i64>,
    pub last_alarm_at: Option<i64>,
    /// `fullScreen`, `opened` (from the notification), `notification` or `notificationMode`.
    pub last_alarm_how: Option<String>,
}

/// The onboarding checklist (SPEC §8.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub notifications: bool,
    pub exact_alarms: bool,
    pub full_screen: bool,
    pub battery: bool,
    /// "Pause app activity if unused" is off.
    pub unused_apps: bool,
    /// The phone maker if it needs an extra step (`samsung`, `xiaomi`, …), or "".
    pub oem: String,
    /// The user confirmed doing that extra step.
    pub oem_done: bool,
}

impl PermissionStatus {
    /// Everything the alarms need is granted (the maker step is advice).
    #[must_use]
    pub fn all_granted(&self) -> bool {
        self.notifications
            && self.exact_alarms
            && self.full_screen
            && self.battery
            && self.unused_apps
    }
}

/// Creates the plugin. Register it with `tauri::Builder::plugin`.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new(PLUGIN_NAME)
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin(
                    "io.github.minasskasss.clockin.alarm",
                    "AlarmPlugin",
                )?;
                _app.manage(AlarmBridge::new(handle));
            }
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_is_sent_in_kotlins_field_names() {
        let plan = BridgePlan {
            items: vec![BridgeItem {
                item_id: "abc".into(),
                fires_at: "2026-10-06T07:00:00Z".into(),
                kind: "in",
                name: "Μαρία Παππά".into(),
            }],
            alert_mode: "ring",
            config_version: 7,
            horizon_end: None,
            theme: "auto",
            dark_windows: vec![BridgeWindow { start: 1, end: 2 }],
        };
        assert_eq!(
            serde_json::to_value(&plan).unwrap(),
            serde_json::json!({
                "items": [{
                    "itemId": "abc",
                    "firesAt": "2026-10-06T07:00:00Z",
                    "kind": "in",
                    "name": "Μαρία Παππά",
                }],
                "alertMode": "ring",
                "configVersion": 7,
                "horizonEnd": null,
                "theme": "auto",
                "darkWindows": [{ "start": 1, "end": 2 }],
            })
        );
    }

    #[test]
    fn the_checklist_needs_every_permission_but_not_the_maker_step() {
        let mut status = PermissionStatus {
            notifications: true,
            exact_alarms: true,
            full_screen: true,
            battery: true,
            unused_apps: true,
            oem: "samsung".into(),
            oem_done: false,
        };
        assert!(status.all_granted());
        status.full_screen = false;
        assert!(!status.all_granted());
        let parsed: PermissionStatus = serde_json::from_str(
            r#"{"notifications":true,"exactAlarms":true,"fullScreen":true,"battery":false,"unusedApps":true,"oem":"","oemDone":false}"#,
        )
        .unwrap();
        assert!(!parsed.battery);
    }
}
