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
    /// The Android System WebView's version (e.g. "120.0.6099.230"), or "".
    pub web_view_version: String,
    /// The WebView is new enough for the app's screens (`Permissions.MIN_WEBVIEW`).
    pub web_view_ok: Option<bool>,
    pub last_refresh_at: Option<i64>,
    pub last_refresh_ok: Option<bool>,
    pub next_alarm_at: Option<i64>,
    pub last_alarm_at: Option<i64>,
    /// `fullScreen`, `opened` (from the notification), `notification` or `notificationMode`.
    pub last_alarm_how: Option<String>,
    /// The last uncaught Kotlin/Java exception: when, in which app version, and what.
    pub last_crash_at: Option<i64>,
    pub last_crash_version: Option<String>,
    pub last_crash_error: Option<String>,
}

/// The status and navigation bars (and any camera cutout) around the app,
/// in CSS pixels. The app draws edge to edge; older Android System WebViews
/// report 0 for `env(safe-area-inset-*)`, so the page uses these as well.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
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
    /// The phone's Android API level.
    #[serde(default)]
    pub sdk: i32,
    /// Items with no setting on this Android version (always allowed
    /// there), which the checklist doesn't show, e.g. `exactAlarms` before 12.
    #[serde(default)]
    pub not_applicable: Vec<String>,
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
            sdk: 34,
            not_applicable: vec![],
        };
        assert!(status.all_granted());
        status.full_screen = false;
        assert!(!status.all_granted());
        let parsed: PermissionStatus = serde_json::from_str(
            r#"{"notifications":true,"exactAlarms":true,"fullScreen":true,"battery":false,"unusedApps":true,"oem":"","oemDone":false}"#,
        )
        .unwrap();
        assert!(!parsed.battery);
        assert!(parsed.not_applicable.is_empty());
    }

    #[test]
    fn android_11_reports_what_its_checklist_leaves_out() {
        let parsed: PermissionStatus = serde_json::from_str(
            r#"{"notifications":true,"exactAlarms":true,"fullScreen":true,"battery":true,"unusedApps":true,"oem":"xiaomi","oemDone":false,"sdk":30,"notApplicable":["exactAlarms","fullScreen","unusedApps"]}"#,
        )
        .unwrap();
        assert_eq!(parsed.sdk, 30);
        assert_eq!(
            parsed.not_applicable,
            ["exactAlarms", "fullScreen", "unusedApps"]
        );
        assert!(parsed.all_granted());
        assert_eq!(
            serde_json::to_value(&parsed).unwrap()["notApplicable"],
            serde_json::json!(["exactAlarms", "fullScreen", "unusedApps"])
        );
    }

    #[test]
    fn diagnostics_read_the_web_view() {
        let d: Diagnostics = serde_json::from_str(
            r#"{"manufacturer":"Xiaomi","model":"Redmi Note 11S","androidVersion":"11","sdk":30,"makerOs":"MIUI V130","webViewVersion":"90.0.4430.210","webViewPackage":"com.google.android.webview","webViewOk":false}"#,
        )
        .unwrap();
        assert_eq!(d.web_view_version, "90.0.4430.210");
        assert_eq!(d.web_view_ok, Some(false));
    }
}
