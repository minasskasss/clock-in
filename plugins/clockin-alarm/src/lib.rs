//! Clock In alarm plugin.
//!
//! On Android this will bridge to a Kotlin plugin that schedules alarms with
//! `AlarmManager` (see `docs/ARCHITECTURE.md` §10). Until Phase 5 it is an
//! empty plugin so that the app already wires it in on every platform.

use tauri::{
    Runtime,
    plugin::{Builder, TauriPlugin},
};

/// The plugin name, as registered with Tauri.
pub const PLUGIN_NAME: &str = "clockin-alarm";

/// Creates the plugin. Register it with `tauri::Builder::plugin`.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new(PLUGIN_NAME).build()
}
