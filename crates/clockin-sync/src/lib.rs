//! Clock In sync client (ARCHITECTURE §5, §6, §8).
//!
//! - [`Api`]: typed client for the Supabase RPC functions.
//! - [`Store`]: the encrypted (SQLCipher) local database: snapshot cache,
//!   pending-marks queue, per-device settings.
//! - [`SyncEngine`]: one round of the sync loop at a time; the platform
//!   layer owns the timer and the OS secret store.
//!
//! Credentials ([`DeviceSecret`], [`SessionToken`], [`QuitCode`],
//! [`StoreKey`]) have redacted `Debug` output and never appear in errors.
//! All time logic comes from `clockin-core`.

mod api;
mod engine;
mod secret;
mod store;

pub use api::{
    AdminSession, AlarmCheck, AlarmCheckItem, Api, ApiError, DeviceInfo, ErrorCode, MarkReceipt,
    MarkRequest, OverrideBlockInput, Pairing, Platform, Rejection, ServerConfig, ServerMark,
    ServerPlan, ServerSettings, ServerSnapshot, SettingsInput, Versions, WeekBlockInput,
};
pub use engine::{
    SyncBackend, SyncEngine, SyncStatus, TickReport, core_snapshot_with_pending, queue_mark,
};
pub use secret::{DeviceSecret, QuitCode, SessionToken};
pub use store::{CachedSnapshot, RefusedMark, Store, StoreError, StoreKey};
