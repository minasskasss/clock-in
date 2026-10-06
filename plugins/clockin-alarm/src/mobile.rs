//! Android: calls into the Kotlin `AlarmPlugin`. Every call blocks until
//! Kotlin answers on the main thread (see the crate docs).

use crate::{BridgePlan, PermissionStatus};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tauri::Runtime;
use tauri::plugin::PluginHandle;

/// A failed call. The message never contains a secret value.
#[derive(Debug, thiserror::Error)]
#[error("the Android alarm plugin failed: {0}")]
pub struct BridgeError(String);

/// The Kotlin side, managed as Tauri state by the plugin.
pub struct AlarmBridge<R: Runtime>(PluginHandle<R>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Name<'a> {
    name: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Secret<'a> {
    name: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Kind<'a> {
    kind: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerConfig<'a> {
    url: &'a str,
    publishable_key: &'a str,
}

#[derive(Serialize)]
struct Done {
    done: bool,
}

#[derive(Deserialize)]
struct Value {
    #[serde(default)]
    value: Option<String>,
}

#[derive(Deserialize)]
struct DeviceName {
    name: String,
}

#[derive(Deserialize)]
struct Nothing {}

impl<R: Runtime> AlarmBridge<R> {
    pub(crate) fn new(handle: PluginHandle<R>) -> Self {
        Self(handle)
    }

    fn call<T: DeserializeOwned>(
        &self,
        command: &str,
        payload: impl Serialize,
    ) -> Result<T, BridgeError> {
        self.0
            .run_mobile_plugin(command, payload)
            .map_err(|e| BridgeError(format!("{command}: {e}")))
    }

    /// Replaces the plan and alert mode Kotlin schedules from.
    ///
    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn set_plan(&self, plan: &BridgePlan) -> Result<(), BridgeError> {
        self.call::<Nothing>("setPlan", plan).map(drop)
    }

    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn permission_status(&self) -> Result<PermissionStatus, BridgeError> {
        self.call("permissionStatus", ())
    }

    /// Opens the Android screen that fixes one checklist item.
    ///
    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn open_settings(&self, kind: &str) -> Result<(), BridgeError> {
        self.call::<Nothing>("openSettings", Kind { kind }).map(drop)
    }

    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn set_oem_done(&self, done: bool) -> Result<(), BridgeError> {
        self.call::<Nothing>("setOemDone", Done { done }).map(drop)
    }

    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn secret_get(&self, name: &str) -> Result<Option<String>, BridgeError> {
        self.call::<Value>("secretGet", Name { name })
            .map(|v| v.value)
    }

    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn secret_set(&self, name: &str, value: &str) -> Result<(), BridgeError> {
        self.call::<Nothing>("secretSet", Secret { name, value })
            .map(drop)
    }

    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn secret_delete(&self, name: &str) -> Result<(), BridgeError> {
        self.call::<Nothing>("secretDelete", Name { name }).map(drop)
    }

    /// The project URL and publishable key the background code calls.
    ///
    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn set_server_config(&self, url: &str, publishable_key: &str) -> Result<(), BridgeError> {
        self.call::<Nothing>(
            "setServerConfig",
            ServerConfig {
                url,
                publishable_key,
            },
        )
        .map(drop)
    }

    /// The phone's own name (e.g. "Galaxy S24 Ultra").
    ///
    /// # Errors
    ///
    /// If the Kotlin call fails.
    pub fn device_name(&self) -> Result<String, BridgeError> {
        self.call::<DeviceName>("deviceName", ()).map(|d| d.name)
    }
}
