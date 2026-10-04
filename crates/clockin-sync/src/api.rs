//! Typed client for the Supabase RPC functions (ARCHITECTURE §6).
//!
//! Every call is `POST <project>/rest/v1/rpc/<name>` with the publishable key
//! in the `apikey` header (not `Authorization`; the new keys are not JWTs).
//! Functions answer `{ ok: true, ... }` or `{ ok: false, error, ... }`; the
//! latter becomes [`ApiError::Rejected`].

use crate::secret::{DeviceSecret, QuitCode, SessionToken};
use clockin_core::{
    MarkKind, Override, OverrideKind, PlanItem, ScheduleSettings, Snapshot, Staff, WeeklyBlock,
    weekday_number,
};
use jiff::Timestamp;
use jiff::civil::{Date, Time, Weekday};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Overall timeout of one RPC call.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest server error message kept in [`ApiError::Http`].
const MAX_MESSAGE_CHARS: usize = 300;

/// The public app configuration embedded in the build: the Supabase project
/// URL and the publishable key. Both are public by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub url: String,
    pub publishable_key: String,
}

impl ServerConfig {
    /// The project base URL without a trailing `/` or `/rest/v1`, so a URL
    /// copied from the API docs works too.
    #[must_use]
    pub fn base_url(&self) -> &str {
        let url = self.url.trim().trim_end_matches('/');
        url.strip_suffix("/rest/v1").unwrap_or(url)
    }
}

/// Device platform, as stored on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Android,
}

/// Error codes of the RPC contract (ARCHITECTURE §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCode {
    BadSecret,
    Revoked,
    Locked,
    BadPassphrase,
    NoSession,
    InvalidInput,
    VersionConflict,
    AlreadyInitialized,
    NotInitialized,
    /// A code this version of the app doesn't know.
    Other(String),
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Ok(match code.as_str() {
            "bad_secret" => Self::BadSecret,
            "revoked" => Self::Revoked,
            "locked" => Self::Locked,
            "bad_passphrase" => Self::BadPassphrase,
            "no_session" => Self::NoSession,
            "invalid_input" => Self::InvalidInput,
            "version_conflict" => Self::VersionConflict,
            "already_initialized" => Self::AlreadyInitialized,
            "not_initialized" => Self::NotInitialized,
            _ => Self::Other(code),
        })
    }
}

/// An `{ ok: false }` answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rejection {
    pub error: ErrorCode,
    /// For `locked`, and for the `bad_passphrase` that starts a lock.
    #[serde(default)]
    pub retry_after_s: Option<u32>,
    /// For `invalid_input`: which field or constraint.
    #[serde(default)]
    pub details: Option<String>,
    /// For `version_conflict`.
    #[serde(default)]
    pub config_version: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The HTTP client could not be set up.
    #[error("HTTP client setup failed: {0}")]
    Setup(String),
    /// No answer: offline, DNS, timeout, TLS.
    #[error("network error: {0}")]
    Network(String),
    /// The server answered with an HTTP error (e.g. project paused, bad key).
    #[error("server answered HTTP {status}: {message}")]
    Http { status: u16, message: String },
    /// The answer didn't have the expected shape.
    #[error("unexpected response: {0}")]
    Decode(String),
    /// The function refused the call.
    #[error("server refused the call: {:?}", .0.error)]
    Rejected(Rejection),
}

impl ApiError {
    /// The rejection code, if the server refused the call.
    #[must_use]
    pub fn code(&self) -> Option<&ErrorCode> {
        match self {
            Self::Rejected(r) => Some(&r.error),
            _ => None,
        }
    }

    /// The device secret is unknown or revoked: the device must pair again.
    #[must_use]
    pub fn is_unpaired(&self) -> bool {
        matches!(self.code(), Some(ErrorCode::BadSecret | ErrorCode::Revoked))
    }

    /// The server could not be reached or didn't answer properly; callers
    /// fall back to the cached state (fail loud).
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::Http { .. } | Self::Decode(_))
    }
}

/// Returned by `admin_initialize` and `pair_device`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Pairing {
    pub device_id: Uuid,
    pub device_secret: DeviceSecret,
}

/// Returned by `get_version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    pub config_version: i64,
    pub data_version: i64,
    pub plan_config_version: i64,
    pub plan_horizon_end: Option<Timestamp>,
    pub server_now: Timestamp,
}

/// Settings as the server stores them (SPEC §4.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSettings {
    pub checkin_offset_min: i32,
    pub checkout_offset_min: i32,
    pub rollover: Time,
    pub autostart: bool,
    /// Null only before `admin_initialize`.
    pub quit_code: Option<QuitCode>,
}

impl ServerSettings {
    #[must_use]
    pub fn schedule(&self) -> ScheduleSettings {
        ScheduleSettings {
            checkin_offset_min: self.checkin_offset_min,
            checkout_offset_min: self.checkout_offset_min,
            rollover: self.rollover,
        }
    }
}

/// A live (non-voided) mark, with the fields Settings → Today's marks shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerMark {
    pub id: Uuid,
    pub staff_id: Uuid,
    pub source_block_id: Uuid,
    pub business_date: Date,
    pub kind: MarkKind,
    pub marked_at: Timestamp,
    pub device_id: Option<Uuid>,
}

impl ServerMark {
    #[must_use]
    pub fn core(&self) -> clockin_core::Mark {
        clockin_core::Mark {
            id: self.id,
            staff_id: self.staff_id,
            source_block_id: self.source_block_id,
            business_date: self.business_date,
            kind: self.kind,
        }
    }
}

/// A paired, non-revoked device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: Uuid,
    pub name: String,
    pub platform: Platform,
    pub created_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
}

/// Returned by `get_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSnapshot {
    pub staff: Vec<Staff>,
    pub blocks: Vec<WeeklyBlock>,
    pub overrides: Vec<Override>,
    pub settings: ServerSettings,
    pub marks: Vec<ServerMark>,
    pub devices: Vec<DeviceInfo>,
    pub this_device_id: Uuid,
    pub config_version: i64,
    pub data_version: i64,
    pub plan_config_version: i64,
    pub plan_horizon_end: Option<Timestamp>,
    pub server_now: Timestamp,
}

impl ServerSnapshot {
    /// The core's view, with `extra_marks` (marks still queued on this
    /// device) added.
    #[must_use]
    pub fn core_snapshot(&self, extra_marks: &[clockin_core::Mark]) -> Snapshot {
        Snapshot {
            staff: self.staff.clone(),
            blocks: self.blocks.clone(),
            overrides: self.overrides.clone(),
            settings: self.settings.schedule(),
            marks: self
                .marks
                .iter()
                .map(ServerMark::core)
                .chain(extra_marks.iter().cloned())
                .collect(),
        }
    }
}

/// Returned by `mark`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MarkReceipt {
    /// The client id, or the id of the mark another device made first.
    pub mark_id: Uuid,
    pub marked_at: Timestamp,
    pub voided: bool,
}

/// A mark to send: the arguments of `mark`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkRequest {
    pub client_id: Uuid,
    pub staff_id: Uuid,
    pub source_block_id: Uuid,
    pub business_date: Date,
    pub kind: MarkKind,
}

impl MarkRequest {
    #[must_use]
    pub fn core(&self) -> clockin_core::Mark {
        clockin_core::Mark {
            id: self.client_id,
            staff_id: self.staff_id,
            source_block_id: self.source_block_id,
            business_date: self.business_date,
            kind: self.kind,
        }
    }
}

/// Returned by `get_plan`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerPlan {
    pub plan_config_version: i64,
    pub horizon_end: Option<Timestamp>,
    pub config_version: i64,
    pub items: Vec<PlanItem>,
}

/// Returned by `check_alarm`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AlarmCheck {
    pub items: Vec<AlarmCheckItem>,
    pub config_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AlarmCheckItem {
    pub item_id: String,
    pub due: bool,
}

/// Returned by `admin_login`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminSession {
    pub session_token: SessionToken,
    pub expires_in_s: u32,
}

/// A weekly block to save. `id` keeps an existing block's identity when it
/// is edited, so marks stay attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WeekBlockInput {
    pub id: Option<Uuid>,
    #[serde(with = "weekday_number")]
    pub weekday: Weekday,
    pub start: Time,
    pub end: Time,
}

/// A replace-hours block to save. `id` as in [`WeekBlockInput`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverrideBlockInput {
    pub id: Option<Uuid>,
    pub start: Time,
    pub end: Time,
}

/// Every setting `settings_update` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsInput {
    pub checkin_offset_min: i32,
    pub checkout_offset_min: i32,
    pub rollover: Time,
    pub autostart: bool,
    pub quit_code: QuitCode,
}

#[derive(Deserialize)]
struct OkOnly {}

#[derive(Deserialize)]
struct UploadReceipt {
    items: u32,
}

#[derive(Deserialize)]
struct StaffCreated {
    staff_id: Uuid,
}

#[derive(Deserialize)]
struct OverrideSaved {
    override_id: Uuid,
}

#[derive(Deserialize)]
struct Deleted {
    deleted: bool,
}

#[derive(Deserialize)]
struct Voided {
    voided: bool,
}

/// The RPC client. Cheap to clone.
#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    rpc_base: Arc<str>,
    publishable_key: Arc<str>,
}

impl std::fmt::Debug for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Api")
            .field("rpc_base", &self.rpc_base)
            .finish_non_exhaustive()
    }
}

/// rustls with the `ring` provider and Mozilla's root certificates
/// (`webpki-roots`), the same on Windows and Android.
fn tls_config() -> Result<rustls::ClientConfig, ApiError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| ApiError::Setup(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

impl Api {
    /// # Errors
    ///
    /// [`ApiError::Setup`] if the HTTP client cannot be built.
    pub fn new(config: &ServerConfig) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .https_only(true)
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("clock-in/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ApiError::Setup(e.to_string()))?;
        Ok(Self {
            http,
            rpc_base: format!("{}/rest/v1/rpc/", config.base_url()).into(),
            publishable_key: config.publishable_key.trim().into(),
        })
    }

    /// Calls one RPC function with named arguments.
    async fn call<T: DeserializeOwned>(&self, function: &str, args: Value) -> Result<T, ApiError> {
        let response = self
            .http
            .post(format!("{}{function}", self.rpc_base))
            .header("apikey", &*self.publishable_key)
            .json(&args)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        if !status.is_success() {
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: error_message(&body),
            });
        }
        parse_answer(&body)
    }

    // --- Public ---------------------------------------------------------

    /// First device only: sets the passphrase and quit code and pairs this
    /// device.
    ///
    /// # Errors
    ///
    /// `already_initialized`, `invalid_input`, or a transport error.
    pub async fn admin_initialize(
        &self,
        passphrase: &str,
        quit_code: &QuitCode,
        device_name: &str,
        platform: Platform,
    ) -> Result<Pairing, ApiError> {
        self.call(
            "admin_initialize",
            json!({
                "p_passphrase": passphrase,
                "p_quit_code": quit_code.expose(),
                "p_device_name": device_name,
                "p_platform": platform,
            }),
        )
        .await
    }

    /// # Errors
    ///
    /// `bad_passphrase`, `locked`, `not_initialized`, `invalid_input`, or a
    /// transport error.
    pub async fn pair_device(
        &self,
        passphrase: &str,
        device_name: &str,
        platform: Platform,
    ) -> Result<Pairing, ApiError> {
        self.call(
            "pair_device",
            json!({
                "p_passphrase": passphrase,
                "p_device_name": device_name,
                "p_platform": platform,
            }),
        )
        .await
    }

    // --- Device ---------------------------------------------------------

    /// # Errors
    ///
    /// `bad_secret`, `revoked`, or a transport error.
    pub async fn get_version(&self, secret: &DeviceSecret) -> Result<Versions, ApiError> {
        self.call("get_version", json!({ "p_secret": secret.expose() }))
            .await
    }

    /// # Errors
    ///
    /// `bad_secret`, `revoked`, or a transport error.
    pub async fn get_snapshot(&self, secret: &DeviceSecret) -> Result<ServerSnapshot, ApiError> {
        self.call("get_snapshot", json!({ "p_secret": secret.expose() }))
            .await
    }

    /// Idempotent on `mark.client_id`.
    ///
    /// # Errors
    ///
    /// `bad_secret`, `revoked`, `invalid_input`, or a transport error.
    pub async fn mark(
        &self,
        secret: &DeviceSecret,
        mark: &MarkRequest,
    ) -> Result<MarkReceipt, ApiError> {
        self.call(
            "mark",
            json!({
                "p_secret": secret.expose(),
                "p_client_id": mark.client_id,
                "p_staff_id": mark.staff_id,
                "p_source_block_id": mark.source_block_id,
                "p_business_date": mark.business_date,
                "p_kind": mark.kind,
            }),
        )
        .await
    }

    /// Replaces the alarm plan; returns how many items were stored.
    ///
    /// # Errors
    ///
    /// `version_conflict` if the configuration changed since
    /// `base_config_version`, or as [`Api::get_version`].
    pub async fn upload_plan(
        &self,
        secret: &DeviceSecret,
        base_config_version: i64,
        horizon_end: Timestamp,
        items: &[PlanItem],
    ) -> Result<u32, ApiError> {
        let receipt: UploadReceipt = self
            .call(
                "upload_plan",
                json!({
                    "p_secret": secret.expose(),
                    "p_base_config_version": base_config_version,
                    "p_horizon_end": horizon_end,
                    "p_items": items,
                }),
            )
            .await?;
        Ok(receipt.items)
    }

    /// # Errors
    ///
    /// As [`Api::get_version`].
    pub async fn get_plan(&self, secret: &DeviceSecret) -> Result<ServerPlan, ApiError> {
        self.call("get_plan", json!({ "p_secret": secret.expose() }))
            .await
    }

    /// Whether each `(item_id, fires_at)` is still due.
    ///
    /// # Errors
    ///
    /// As [`Api::get_version`].
    pub async fn check_alarm(
        &self,
        secret: &DeviceSecret,
        items: &[(&str, Timestamp)],
    ) -> Result<AlarmCheck, ApiError> {
        let items: Vec<Value> = items
            .iter()
            .map(|(id, at)| json!({ "item_id": id, "fires_at": at }))
            .collect();
        self.call(
            "check_alarm",
            json!({ "p_secret": secret.expose(), "p_items": items }),
        )
        .await
    }

    /// # Errors
    ///
    /// `bad_passphrase`, `locked`, or as [`Api::get_version`].
    pub async fn admin_login(
        &self,
        secret: &DeviceSecret,
        passphrase: &str,
    ) -> Result<AdminSession, ApiError> {
        self.call(
            "admin_login",
            json!({ "p_secret": secret.expose(), "p_passphrase": passphrase }),
        )
        .await
    }

    /// # Errors
    ///
    /// As [`Api::get_version`].
    pub async fn admin_logout(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
    ) -> Result<(), ApiError> {
        let _: OkOnly = self
            .call(
                "admin_logout",
                json!({ "p_secret": secret.expose(), "p_session": session.expose() }),
            )
            .await?;
        Ok(())
    }

    // --- Admin ----------------------------------------------------------

    async fn admin_call<T: DeserializeOwned>(
        &self,
        function: &str,
        secret: &DeviceSecret,
        session: &SessionToken,
        args: Value,
    ) -> Result<T, ApiError> {
        let mut all = json!({ "p_secret": secret.expose(), "p_session": session.expose() });
        if let (Some(all), Value::Object(args)) = (all.as_object_mut(), args) {
            all.extend(args);
        }
        self.call(function, all).await
    }

    /// Returns the new staff id.
    ///
    /// # Errors
    ///
    /// `no_session`, `invalid_input`, or as [`Api::get_version`].
    pub async fn staff_create(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        first_name: &str,
        last_name: &str,
        blocks: &[WeekBlockInput],
    ) -> Result<Uuid, ApiError> {
        let created: StaffCreated = self
            .admin_call(
                "staff_create",
                secret,
                session,
                json!({ "p_first_name": first_name, "p_last_name": last_name, "p_blocks": blocks }),
            )
            .await?;
        Ok(created.staff_id)
    }

    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn staff_update(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        staff_id: Uuid,
        first_name: &str,
        last_name: &str,
    ) -> Result<(), ApiError> {
        let _: OkOnly = self
            .admin_call(
                "staff_update",
                secret,
                session,
                json!({ "p_staff_id": staff_id, "p_first_name": first_name, "p_last_name": last_name }),
            )
            .await?;
        Ok(())
    }

    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn staff_remove(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        staff_id: Uuid,
    ) -> Result<(), ApiError> {
        let _: OkOnly = self
            .admin_call(
                "staff_remove",
                secret,
                session,
                json!({ "p_staff_id": staff_id }),
            )
            .await?;
        Ok(())
    }

    /// Replaces the whole weekly template of one person.
    ///
    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn schedule_set(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        staff_id: Uuid,
        blocks: &[WeekBlockInput],
    ) -> Result<(), ApiError> {
        let _: OkOnly = self
            .admin_call(
                "schedule_set",
                secret,
                session,
                json!({ "p_staff_id": staff_id, "p_blocks": blocks }),
            )
            .await?;
        Ok(())
    }

    /// Creates or replaces an override; returns its id.
    ///
    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn override_set(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        staff_id: Uuid,
        business_date: Date,
        kind: OverrideKind,
        blocks: &[OverrideBlockInput],
    ) -> Result<Uuid, ApiError> {
        let saved: OverrideSaved = self
            .admin_call(
                "override_set",
                secret,
                session,
                json!({
                    "p_staff_id": staff_id,
                    "p_business_date": business_date,
                    "p_kind": kind,
                    "p_blocks": blocks,
                }),
            )
            .await?;
        Ok(saved.override_id)
    }

    /// Returns whether an override was deleted.
    ///
    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn override_delete(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        override_id: Uuid,
    ) -> Result<bool, ApiError> {
        let deleted: Deleted = self
            .admin_call(
                "override_delete",
                secret,
                session,
                json!({ "p_override_id": override_id }),
            )
            .await?;
        Ok(deleted.deleted)
    }

    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn settings_update(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        settings: &SettingsInput,
    ) -> Result<(), ApiError> {
        let _: OkOnly = self
            .admin_call(
                "settings_update",
                secret,
                session,
                json!({
                    "p_checkin_offset_min": settings.checkin_offset_min,
                    "p_checkout_offset_min": settings.checkout_offset_min,
                    "p_rollover": settings.rollover,
                    "p_autostart": settings.autostart,
                    "p_quit_code": settings.quit_code.expose(),
                }),
            )
            .await?;
        Ok(())
    }

    /// Returns whether a live mark was voided.
    ///
    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn mark_void(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        mark_id: Uuid,
    ) -> Result<bool, ApiError> {
        let voided: Voided = self
            .admin_call(
                "mark_void",
                secret,
                session,
                json!({ "p_mark_id": mark_id }),
            )
            .await?;
        Ok(voided.voided)
    }

    /// # Errors
    ///
    /// As [`Api::staff_create`].
    pub async fn device_revoke(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        device_id: Uuid,
    ) -> Result<(), ApiError> {
        let _: OkOnly = self
            .admin_call(
                "device_revoke",
                secret,
                session,
                json!({ "p_device_id": device_id }),
            )
            .await?;
        Ok(())
    }

    /// Signs out every admin session, including this one.
    ///
    /// # Errors
    ///
    /// `bad_passphrase` / `locked` for a wrong current passphrase, or as
    /// [`Api::staff_create`].
    pub async fn change_passphrase(
        &self,
        secret: &DeviceSecret,
        session: &SessionToken,
        old_passphrase: &str,
        new_passphrase: &str,
    ) -> Result<(), ApiError> {
        let _: OkOnly = self
            .admin_call(
                "change_passphrase",
                secret,
                session,
                json!({ "p_old_passphrase": old_passphrase, "p_new_passphrase": new_passphrase }),
            )
            .await?;
        Ok(())
    }
}

/// Splits an RPC answer into success or [`ApiError::Rejected`].
fn parse_answer<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    let value: Value = serde_json::from_slice(body).map_err(|e| ApiError::Decode(e.to_string()))?;
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) => serde_json::from_value(value).map_err(|e| ApiError::Decode(e.to_string())),
        Some(false) => Err(ApiError::Rejected(
            serde_json::from_value(value).map_err(|e| ApiError::Decode(e.to_string()))?,
        )),
        None => Err(ApiError::Decode("answer has no `ok` field".into())),
    }
}

/// The `message` of a PostgREST error body, shortened. PostgREST messages
/// name functions and parameters, never argument values of type text.
fn error_message(body: &[u8]) -> String {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    message.chars().take(MAX_MESSAGE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_accepts_common_forms() {
        for url in [
            "https://abc.supabase.co",
            "https://abc.supabase.co/",
            "https://abc.supabase.co/rest/v1/",
            " https://abc.supabase.co/rest/v1 ",
        ] {
            let config = ServerConfig {
                url: url.into(),
                publishable_key: "k".into(),
            };
            assert_eq!(config.base_url(), "https://abc.supabase.co", "{url}");
        }
    }

    #[test]
    fn answers_split_into_ok_and_rejected() {
        let v: Versions = parse_answer(
            br#"{"ok": true, "config_version": 3, "data_version": 9, "plan_config_version": 2,
                 "plan_horizon_end": null, "server_now": "2026-10-05T10:00:00.123456+00:00"}"#,
        )
        .unwrap();
        assert_eq!(v.data_version, 9);
        assert_eq!(v.plan_horizon_end, None);

        let err =
            parse_answer::<Versions>(br#"{"ok": false, "error": "locked", "retry_after_s": 42}"#)
                .unwrap_err();
        let ApiError::Rejected(r) = err else {
            panic!("expected a rejection")
        };
        assert_eq!(r.error, ErrorCode::Locked);
        assert_eq!(r.retry_after_s, Some(42));

        let err =
            parse_answer::<Versions>(br#"{"ok": false, "error": "something_new"}"#).unwrap_err();
        assert_eq!(err.code(), Some(&ErrorCode::Other("something_new".into())));
        assert!(!err.is_unreachable());

        assert!(matches!(
            parse_answer::<Versions>(b"[]"),
            Err(ApiError::Decode(_))
        ));
        assert!(matches!(
            parse_answer::<Versions>(b"not json"),
            Err(ApiError::Decode(_))
        ));
    }

    #[test]
    fn unpaired_codes() {
        for (code, unpaired) in [
            ("bad_secret", true),
            ("revoked", true),
            ("no_session", false),
            ("locked", false),
        ] {
            let body = format!(r#"{{"ok": false, "error": "{code}"}}"#);
            let err = parse_answer::<Versions>(body.as_bytes()).unwrap_err();
            assert_eq!(err.is_unpaired(), unpaired, "{code}");
        }
    }

    #[test]
    fn snapshot_parses_server_json() {
        let snap: ServerSnapshot = parse_answer(
            r#"{"ok": true,
                 "staff": [{"id": "00000000-0000-0000-0000-000000000001", "first_name": "Μαρία",
                            "last_name": "Π", "removed_at": null}],
                 "blocks": [{"id": "00000000-0000-0000-0000-000000000002",
                             "staff_id": "00000000-0000-0000-0000-000000000001",
                             "weekday": 1, "start": "19:00:00", "end": "00:00:00"}],
                 "overrides": [{"id": "00000000-0000-0000-0000-000000000003",
                                "staff_id": "00000000-0000-0000-0000-000000000001",
                                "business_date": "2026-10-06", "kind": "replace",
                                "blocks": [{"id": "00000000-0000-0000-0000-000000000004",
                                            "start": "10:00:00", "end": "14:00:00"}]}],
                 "settings": {"checkin_offset_min": -5, "checkout_offset_min": 0,
                              "rollover": "05:00:00", "autostart": true, "quit_code": "1234"},
                 "marks": [{"id": "00000000-0000-0000-0000-000000000005",
                            "staff_id": "00000000-0000-0000-0000-000000000001",
                            "source_block_id": "00000000-0000-0000-0000-000000000002",
                            "business_date": "2026-10-05", "kind": "in",
                            "marked_at": "2026-10-05T16:01:02.5+00:00", "device_id": null}],
                 "devices": [{"id": "00000000-0000-0000-0000-000000000006", "name": "Shop PC",
                              "platform": "windows", "created_at": "2026-10-05T10:00:00+00:00",
                              "last_seen_at": null}],
                 "this_device_id": "00000000-0000-0000-0000-000000000006",
                 "config_version": 4, "data_version": 7, "plan_config_version": 4,
                 "plan_horizon_end": "2026-10-19T10:00:00+00:00",
                 "server_now": "2026-10-05T10:00:00+00:00"}"#
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(snap.settings.rollover, Time::constant(5, 0, 0, 0));
        assert_eq!(snap.settings.quit_code.as_ref().unwrap().expose(), "1234");
        assert_eq!(snap.blocks[0].weekday, Weekday::Monday);
        assert_eq!(snap.overrides[0].kind, OverrideKind::Replace);

        let extra = clockin_core::Mark {
            id: Uuid::from_u128(9),
            staff_id: Uuid::from_u128(1),
            source_block_id: Uuid::from_u128(2),
            business_date: jiff::civil::date(2026, 10, 5),
            kind: MarkKind::Out,
        };
        let core = snap.core_snapshot(std::slice::from_ref(&extra));
        assert_eq!(core.settings.checkin_offset_min, -5);
        assert_eq!(core.marks.len(), 2);
        assert_eq!(core.marks[1], extra);

        // The cached copy round-trips.
        let json = serde_json::to_string(&snap).unwrap();
        assert_eq!(serde_json::from_str::<ServerSnapshot>(&json).unwrap(), snap);
    }

    #[test]
    fn block_inputs_use_the_server_shape() {
        let b = WeekBlockInput {
            id: None,
            weekday: Weekday::Sunday,
            start: Time::constant(19, 0, 0, 0),
            end: Time::constant(2, 0, 0, 0),
        };
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["weekday"], 7);
        assert_eq!(v["start"], "19:00:00");
        assert_eq!(v["end"], "02:00:00");
        assert!(v["id"].is_null());
    }

    #[test]
    fn http_error_messages_are_short() {
        let long = format!(r#"{{"message": "{}"}}"#, "x".repeat(1000));
        assert_eq!(error_message(long.as_bytes()).len(), MAX_MESSAGE_CHARS);
        assert_eq!(error_message(b"plain"), "plain");
    }
}
