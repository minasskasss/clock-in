//! The running app: pairing, the background sync loop (ARCHITECTURE §8),
//! the admin session and the passphrase lockout.
//!
//! The sync loop owns the [`SyncEngine`] and its own connection to the local
//! database. The UI reads the last published snapshot and queues marks on a
//! second connection, so a tap never waits for a sync round that is talking
//! to a slow server. Admin calls go straight to the server and then wait for
//! one fresh sync round, so the screen shows the change at once.

use crate::clock::Clock;
use crate::error::CmdError;
use crate::profile::Profile;
use crate::secrets::SecretStore;
use crate::views;
#[cfg(desktop)]
use clockin_core::AlarmKey;
use clockin_core::{
    MarkKind, Snapshot, admin_idle_expired, check_passphrase, is_valid_quit_code, lockout_until,
    normalize_passphrase, same_passphrase, seconds_until,
};
use clockin_sync::{
    Api, ApiError, DeviceSecret, MarkRequest, Pairing as ServerPairing, Platform, QuitCode,
    ServerConfig, ServerSnapshot, SessionToken, Store, StoreError, StoreKey, SyncEngine,
    SyncStatus, core_snapshot_with_pending, queue_mark,
};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;
use tokio::sync::{Notify, watch};
use uuid::Uuid;

const SECRET_DEVICE: &str = "device-secret";
const SECRET_STORE_KEY: &str = "store-key";
const SETTING_LOCKOUT_UNTIL: &str = "lockout_until";
/// Alarms this device already rang and that were stopped or ended.
#[cfg(desktop)]
const SETTING_ALARMS_HANDLED: &str = "alarms_handled";
/// Android: ring or notification (SPEC §8.2), per device.
const SETTING_ALERT_MODE: &str = "alert_mode";
const DB_FILE: &str = "local.db";
/// How long an admin call waits for the sync round that shows its result.
const SYNC_WAIT: Duration = Duration::from_secs(10);
/// How long an alarm about to ring waits for fresh marks (ARCHITECTURE §9:
/// local state at most a few seconds old; offline it rings anyway).
#[cfg(desktop)]
const ALARM_SYNC_WAIT: Duration = Duration::from_secs(3);

#[cfg(target_os = "android")]
const PLATFORM: Platform = Platform::Android;
#[cfg(not(target_os = "android"))]
const PLATFORM: Platform = Platform::Windows;

/// Locks a std mutex, ignoring poisoning (the data stays usable).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Wakes the sync loop and lets callers wait for a round that started
/// after their request.
struct SyncControl {
    wake: Notify,
    requested: AtomicU64,
    done: watch::Sender<u64>,
    stop: AtomicBool,
}

impl SyncControl {
    fn new() -> Self {
        Self {
            wake: Notify::new(),
            requested: AtomicU64::new(0),
            done: watch::channel(0).0,
            stop: AtomicBool::new(false),
        }
    }

    /// Starts a sync round now and returns its ticket.
    fn request(&self) -> u64 {
        let ticket = self.requested.fetch_add(1, Ordering::SeqCst) + 1;
        self.wake.notify_one();
        ticket
    }

    /// Waits until a round that started after `ticket` was requested has
    /// finished (or the loop stopped), at most `limit`.
    async fn wait(&self, ticket: u64, limit: Duration) {
        let mut done = self.done.subscribe();
        let _ = tokio::time::timeout(limit, done.wait_for(|d| *d >= ticket)).await;
    }

    fn finish(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.done.send_replace(u64::MAX);
    }
}

#[derive(Clone)]
struct Pairing {
    api: Api,
    secret: DeviceSecret,
    sync: Arc<SyncControl>,
}

/// What the sync loop last published.
#[derive(Default)]
struct Published {
    snapshot: Option<ServerSnapshot>,
    status: Option<SyncStatus>,
    /// Marks queued here that the sync loop sent but hasn't published yet.
    /// They keep a just-marked row from flickering back for a moment.
    in_flight: Vec<MarkRequest>,
}

struct Session {
    /// `None` once the server refused it; the next change asks for the
    /// passphrase again.
    token: Option<SessionToken>,
    last_activity: Timestamp,
}

#[derive(Default)]
struct AdminState {
    session: Option<Session>,
    lockout_until: Option<Timestamp>,
}

pub struct AppState {
    pub clock: Clock,
    profile: Profile,
    config: Option<ServerConfig>,
    secrets: Box<dyn SecretStore>,
    db_path: PathBuf,
    store_key: StoreKey,
    ui_store: Mutex<Store>,
    pairing: Mutex<Option<Pairing>>,
    published: RwLock<Published>,
    admin: Mutex<AdminState>,
    /// Wakes the alarm scheduler: new data, a mark, a clock change, Stop.
    pub alarm_wake: Notify,
    /// Windows output muted, at zero or missing (the banner).
    sound_off: AtomicBool,
    /// The name suggested on the first-run screens.
    default_device_name: RwLock<String>,
}

/// How an Android device alerts (SPEC §7.3, §7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertMode {
    /// Full-screen alarm, looping sound, repeats until Stop.
    Ring,
    /// One notification with the standard sound.
    Notification,
}

impl AlertMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ring => "ring",
            Self::Notification => "notification",
        }
    }
}

/// Where the app is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// This build has no server configuration.
    NotConfigured,
    /// The first-run screens: set up as first device, or pair.
    Unpaired,
    Paired,
}

impl AppState {
    /// Opens the local database (creating its key on first run) and loads
    /// the device secret.
    ///
    /// # Errors
    ///
    /// If the data folder, the OS secret store or the database fail.
    pub fn open(
        profile: Profile,
        data_dir: &Path,
        secrets: Box<dyn SecretStore>,
        config: Option<ServerConfig>,
    ) -> Result<(Self, Option<DeviceSecret>), CmdError> {
        std::fs::create_dir_all(data_dir).map_err(|e| CmdError::internal(e.kind()))?;
        let db_path = data_dir.join(DB_FILE);
        let stored_key = secrets
            .get(SECRET_STORE_KEY)?
            .and_then(|hex| StoreKey::from_hex(&hex));
        let store_key = match stored_key {
            Some(key) => key,
            None => {
                // Without its key an old database is unreadable; it only
                // held a cache, so start afresh.
                remove_db_files(&db_path);
                let key = StoreKey::generate()?;
                secrets.set(SECRET_STORE_KEY, &key.expose_hex())?;
                key
            }
        };
        let store = match Store::open(&db_path, &store_key) {
            Err(StoreError::WrongKey) => {
                remove_db_files(&db_path);
                Store::open(&db_path, &store_key)?
            }
            other => other?,
        };
        let lockout_until = store
            .device_setting(SETTING_LOCKOUT_UNTIL)?
            .and_then(|s| s.parse().ok());
        let secret = secrets.get(SECRET_DEVICE)?.map(DeviceSecret::new);
        let default_device_name = RwLock::new(profile.default_device_name());
        let state = Self {
            clock: Clock::default(),
            profile,
            config,
            secrets,
            db_path,
            store_key,
            ui_store: Mutex::new(store),
            pairing: Mutex::new(None),
            published: RwLock::new(Published::default()),
            admin: Mutex::new(AdminState {
                session: None,
                lockout_until,
            }),
            alarm_wake: Notify::new(),
            sound_off: AtomicBool::new(false),
            default_device_name,
        };
        Ok((state, secret))
    }

    #[must_use]
    pub fn phase(&self) -> Phase {
        if self.config.is_none() {
            Phase::NotConfigured
        } else if lock(&self.pairing).is_some() {
            Phase::Paired
        } else {
            Phase::Unpaired
        }
    }

    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The device name the first-run screens suggest.
    #[must_use]
    pub fn default_device_name(&self) -> String {
        self.default_device_name
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Android: the phone's own name instead of the computer name.
    #[cfg(target_os = "android")]
    pub fn set_default_device_name(&self, name: &str) {
        let name: String = name.trim().chars().take(60).collect();
        if !name.is_empty() {
            *self
                .default_device_name
                .write()
                .unwrap_or_else(PoisonError::into_inner) = name;
        }
    }

    /// This device's alert mode (Android, SPEC §8.2); Ring by default.
    #[cfg(any(target_os = "android", test))]
    #[must_use]
    pub fn alert_mode(&self) -> AlertMode {
        match lock(&self.ui_store)
            .device_setting(SETTING_ALERT_MODE)
            .ok()
            .flatten()
            .as_deref()
        {
            Some("notification") => AlertMode::Notification,
            _ => AlertMode::Ring,
        }
    }

    /// # Errors
    ///
    /// If the local database can't be written.
    pub fn set_alert_mode(&self, mode: AlertMode) -> Result<(), CmdError> {
        lock(&self.ui_store).set_device_setting(
            SETTING_ALERT_MODE,
            mode.as_str(),
            Timestamp::now(),
        )?;
        Ok(())
    }

    /// The server `config_version` of the last snapshot.
    #[cfg(any(target_os = "android", test))]
    #[must_use]
    pub fn config_version(&self) -> Option<i64> {
        self.with_snapshot(|s| s.map(|s| s.config_version))
    }

    fn pairing(&self) -> Result<Pairing, CmdError> {
        lock(&self.pairing).clone().ok_or(CmdError::NotPaired)
    }

    fn api(&self) -> Result<Api, CmdError> {
        let config = self.config.as_ref().ok_or(CmdError::NotConfigured)?;
        Api::new(config).map_err(CmdError::internal)
    }

    // --- Sync loop ------------------------------------------------------

    /// Starts syncing as the device with `secret`.
    ///
    /// # Errors
    ///
    /// If the second database connection or the HTTP client can't be set up.
    pub fn start_sync(self: &Arc<Self>, secret: DeviceSecret) -> Result<(), CmdError> {
        let api = self.api()?;
        let store = Store::open(&self.db_path, &self.store_key)?;
        let engine = SyncEngine::new(api.clone(), store, secret.clone())?;
        let sync = Arc::new(SyncControl::new());
        {
            let mut published = self
                .published
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            published.snapshot = engine.snapshot().cloned();
            published.status = Some(engine.status().clone());
        }
        *lock(&self.pairing) = Some(Pairing {
            api,
            secret,
            sync: Arc::clone(&sync),
        });
        tauri::async_runtime::spawn(sync_loop(Arc::clone(self), engine, sync));
        Ok(())
    }

    fn publish(&self, engine: &SyncEngine<Api>) {
        let mut published = self
            .published
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let pending: HashSet<Uuid> = engine
            .store()
            .pending_marks()
            .map(|marks| marks.iter().map(|m| m.client_id).collect())
            .unwrap_or_default();
        published.snapshot = engine.snapshot().cloned();
        published.status = Some(engine.status().clone());
        published
            .in_flight
            .retain(|m| pending.contains(&m.client_id));
        drop(published);
        self.alarm_wake.notify_one();
    }

    /// The server no longer knows this device: back to the pairing screen.
    fn forget_pairing(&self, sync: &SyncControl) {
        // Forget the data first, so nothing of it shows once the phase says
        // "unpaired".
        sync.finish();
        lock(&self.admin).session = None;
        if let Err(e) = self.secrets.delete(SECRET_DEVICE) {
            eprintln!("clock-in: could not delete the device secret: {e}");
        }
        if let Err(e) = lock(&self.ui_store).clear_server_data() {
            eprintln!("clock-in: could not clear local data: {e}");
        }
        *self
            .published
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Published::default();
        let mut pairing = lock(&self.pairing);
        if pairing
            .as_ref()
            .is_some_and(|p| std::ptr::eq(Arc::as_ptr(&p.sync), sync))
        {
            *pairing = None;
        }
        drop(pairing);
        self.alarm_wake.notify_one();
    }

    /// Makes the sync loop run now (for a mark: no waiting).
    fn sync_soon(&self) {
        if let Ok(p) = self.pairing() {
            p.sync.request();
        }
    }

    /// Debug builds: moves the clock and re-syncs. Returns false if refused.
    pub fn set_fake_clock(&self, target: Option<jiff::civil::DateTime>, second: bool) -> bool {
        let done = self.clock.set_fake(target, second);
        if done {
            self.sync_soon();
            self.alarm_wake.notify_one();
        }
        done
    }

    // --- What the screens show ------------------------------------------

    /// The Today view, or `None` before the first snapshot arrived.
    ///
    /// # Errors
    ///
    /// If the local queue can't be read.
    pub fn today(&self) -> Result<Option<views::TodayView>, CmdError> {
        Ok(self
            .core_snapshot()?
            .map(|core| views::today(&core, self.clock.now())))
    }

    /// The last server data plus every mark made on this device that the
    /// server hasn't confirmed yet, or `None` before the first snapshot.
    ///
    /// # Errors
    ///
    /// If the local queue can't be read.
    pub fn core_snapshot(&self) -> Result<Option<Snapshot>, CmdError> {
        let published = self
            .published
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(snapshot) = &published.snapshot else {
            return Ok(None);
        };
        let mut core = core_snapshot_with_pending(snapshot, &lock(&self.ui_store))?;
        core.marks
            .extend(published.in_flight.iter().map(MarkRequest::core));
        Ok(Some(core))
    }

    #[must_use]
    pub fn banners(&self) -> views::Banners {
        let published = self
            .published
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let mut banners = published
            .status
            .as_ref()
            .map(|s| views::banners(s, self.clock.now()))
            .unwrap_or_default();
        drop(published);
        banners.sound_off = self.sound_off.load(Ordering::Relaxed);
        match lock(&self.ui_store).refused_marks() {
            Ok(refused) => {
                banners.refused_marks =
                    views::refused_marks(&refused, self.clock.now(), self.rollover());
            }
            Err(e) => eprintln!("clock-in: could not read the refused marks: {e}"),
        }
        banners
    }

    /// «Εντάξει» on a refused-mark notice.
    ///
    /// # Errors
    ///
    /// If the local database can't be written.
    pub fn dismiss_refused_mark(&self, id: Uuid) -> Result<(), CmdError> {
        lock(&self.ui_store).dismiss_refused_mark(id)?;
        Ok(())
    }

    /// Records whether Windows sound output is muted, at zero or missing.
    #[cfg(windows)]
    pub fn set_sound_off(&self, off: bool) {
        self.sound_off.store(off, Ordering::Relaxed);
    }

    /// Changes whenever the server data changed (the Settings lists reload).
    #[must_use]
    pub fn data_version(&self) -> i64 {
        let published = self
            .published
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        published.snapshot.as_ref().map_or(0, |s| s.data_version)
    }

    /// The Settings lists.
    ///
    /// # Errors
    ///
    /// `no_session` unless Settings are unlocked; not paired.
    pub fn admin_view(&self) -> Result<Option<views::AdminView>, CmdError> {
        self.unlocked()?;
        let published = self
            .published
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        Ok(published
            .snapshot
            .as_ref()
            .map(|s| views::admin(s, self.clock.now())))
    }

    fn with_snapshot<T>(&self, f: impl FnOnce(Option<&ServerSnapshot>) -> T) -> T {
        let published = self
            .published
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        f(published.snapshot.as_ref())
    }

    /// The business-day rollover in force (05:00 before the first sync).
    #[must_use]
    pub fn rollover(&self) -> jiff::civil::Time {
        self.with_snapshot(|s| {
            s.map_or(jiff::civil::Time::constant(5, 0, 0, 0), |s| {
                s.settings.rollover
            })
        })
    }

    // --- Marks ----------------------------------------------------------

    /// Marks a check-in or check-out (SPEC §6). Shows at once, also offline;
    /// the sync loop sends it right away.
    ///
    /// # Errors
    ///
    /// Not paired, or the local queue can't be written.
    pub fn mark(
        &self,
        staff_id: Uuid,
        source_block_id: Uuid,
        business_date: Date,
        kind: MarkKind,
    ) -> Result<(), CmdError> {
        let pairing = self.pairing()?;
        {
            let mut published = self
                .published
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            let store = lock(&self.ui_store);
            let same = |m: &MarkRequest| {
                m.staff_id == staff_id
                    && m.source_block_id == source_block_id
                    && m.business_date == business_date
                    && m.kind == kind
            };
            if !published.in_flight.iter().any(same) {
                let id = queue_mark(
                    &store,
                    published.snapshot.as_ref(),
                    staff_id,
                    source_block_id,
                    business_date,
                    kind,
                    self.clock.now(),
                )?;
                if let Some(queued) = store
                    .pending_marks()?
                    .into_iter()
                    .find(|m| m.client_id == id)
                {
                    published.in_flight.push(queued);
                }
            }
        }
        // A ringing alarm drops this name at once.
        self.alarm_wake.notify_one();
        pairing.sync.request();
        Ok(())
    }

    // --- First run ------------------------------------------------------

    fn adopt(self: &Arc<Self>, pairing: &ServerPairing) -> Result<(), CmdError> {
        lock(&self.ui_store).clear_server_data()?;
        *self
            .published
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Published::default();
        self.secrets
            .set(SECRET_DEVICE, pairing.device_secret.expose())?;
        self.clear_lockout();
        self.start_sync(pairing.device_secret.clone())
    }

    fn device_name(raw: &str) -> Result<String, CmdError> {
        let name = raw.trim();
        if (1..=60).contains(&name.chars().count()) {
            Ok(name.to_owned())
        } else {
            Err(CmdError::invalid("device_name", "length"))
        }
    }

    /// "Set up as first device" (SPEC §4.6).
    ///
    /// # Errors
    ///
    /// Invalid input, `already_initialized`, offline.
    pub async fn initialize(
        self: &Arc<Self>,
        passphrase: &str,
        quit_code: &str,
        device_name: &str,
    ) -> Result<(), CmdError> {
        if self.pairing().is_ok() {
            return Ok(());
        }
        let passphrase = check_passphrase(passphrase).map_err(passphrase_error)?;
        if !is_valid_quit_code(quit_code) {
            return Err(CmdError::invalid("quit_code", "format"));
        }
        let name = Self::device_name(device_name)?;
        let pairing = self
            .api()?
            .admin_initialize(
                &passphrase,
                &QuitCode::new(quit_code.to_owned()),
                &name,
                PLATFORM,
            )
            .await?;
        self.adopt(&pairing)
    }

    /// "Pair this device".
    ///
    /// # Errors
    ///
    /// `bad_passphrase`, `locked`, `not_initialized`, offline.
    pub async fn pair(
        self: &Arc<Self>,
        passphrase: &str,
        device_name: &str,
    ) -> Result<(), CmdError> {
        if self.pairing().is_ok() {
            return Ok(());
        }
        let name = Self::device_name(device_name)?;
        let passphrase = nonempty_passphrase(passphrase)?;
        let result = self.api()?.pair_device(&passphrase, &name, PLATFORM).await;
        let pairing = self.note_lockout(result)?;
        self.adopt(&pairing)
    }

    // --- Lockout ----------------------------------------------------------

    /// Seconds left of the passphrase lockout this device last heard of.
    #[must_use]
    pub fn lockout_remaining_s(&self) -> u32 {
        let until = lock(&self.admin).lockout_until;
        until.map_or(0, |until| seconds_until(until, Timestamp::now()))
    }

    /// Remembers a lockout the server reported (it survives a restart), or
    /// forgets it after a success.
    fn note_lockout<T>(&self, result: Result<T, ApiError>) -> Result<T, CmdError> {
        match result {
            Ok(value) => {
                self.clear_lockout();
                Ok(value)
            }
            Err(ApiError::Rejected(r)) if r.retry_after_s.is_some() => {
                let until = lockout_until(Timestamp::now(), r.retry_after_s.unwrap_or(0));
                lock(&self.admin).lockout_until = Some(until);
                let _ = lock(&self.ui_store).set_device_setting(
                    SETTING_LOCKOUT_UNTIL,
                    &until.to_string(),
                    Timestamp::now(),
                );
                Err(ApiError::Rejected(r).into())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn clear_lockout(&self) {
        let mut admin = lock(&self.admin);
        if admin.lockout_until.take().is_some() {
            let _ = lock(&self.ui_store).set_device_setting(
                SETTING_LOCKOUT_UNTIL,
                "",
                Timestamp::now(),
            );
        }
    }

    // --- Admin session ------------------------------------------------------

    /// Whether Settings are unlocked. Locks them after 5 idle minutes.
    #[must_use]
    pub fn admin_unlocked(&self) -> bool {
        self.unlocked().is_ok()
    }

    /// Settings are open on this device (they may still need the
    /// passphrase again if the server session ran out).
    fn unlocked(&self) -> Result<(), CmdError> {
        let pairing = self.pairing()?;
        let mut admin = lock(&self.admin);
        match &admin.session {
            Some(s) if !admin_idle_expired(s.last_activity, Timestamp::now()) => Ok(()),
            Some(_) => {
                if let Some(token) = admin.session.take().and_then(|s| s.token) {
                    logout_in_background(&pairing, token);
                }
                Err(no_session())
            }
            None => Err(no_session()),
        }
    }

    /// The server session token, or `no_session` if the passphrase is needed.
    fn session_token(&self) -> Result<SessionToken, CmdError> {
        self.unlocked()?;
        lock(&self.admin)
            .session
            .as_ref()
            .and_then(|s| s.token.clone())
            .ok_or_else(no_session)
    }

    /// The server refused the token: keep Settings open, ask again.
    fn drop_token(&self) {
        if let Some(s) = &mut lock(&self.admin).session {
            s.token = None;
        }
    }

    /// The employer is still using Settings.
    pub fn admin_touch(&self) {
        if self.unlocked().is_ok()
            && let Some(s) = &mut lock(&self.admin).session
        {
            s.last_activity = Timestamp::now();
        }
    }

    /// Unlocks Settings (or renews the server session while they are open).
    ///
    /// # Errors
    ///
    /// `bad_passphrase`, `locked`, offline.
    pub async fn admin_login(&self, passphrase: &str) -> Result<(), CmdError> {
        let pairing = self.pairing()?;
        let passphrase = nonempty_passphrase(passphrase)?;
        let result = pairing.api.admin_login(&pairing.secret, &passphrase).await;
        let session = self.note_lockout(result)?;
        lock(&self.admin).session = Some(Session {
            token: Some(session.session_token),
            last_activity: Timestamp::now(),
        });
        Ok(())
    }

    /// Locks Settings (leaving the screen).
    pub fn admin_logout(&self) {
        let token = lock(&self.admin).session.take().and_then(|s| s.token);
        if let (Some(token), Ok(pairing)) = (token, self.pairing()) {
            logout_in_background(&pairing, token);
        }
    }

    /// Runs one admin call with this device's secret and session, then
    /// waits for the sync round that shows its result.
    ///
    /// # Errors
    ///
    /// `no_session` (Settings locked again), or the call's own error.
    pub async fn admin_call<T, F, Fut>(&self, call: F) -> Result<T, CmdError>
    where
        F: FnOnce(Api, DeviceSecret, SessionToken) -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let pairing = self.pairing()?;
        let token = self.session_token()?;
        match call(pairing.api.clone(), pairing.secret.clone(), token).await {
            Ok(value) => {
                self.admin_touch();
                let ticket = pairing.sync.request();
                pairing.sync.wait(ticket, SYNC_WAIT).await;
                Ok(value)
            }
            Err(e) => {
                if e.code() == Some(&clockin_sync::ErrorCode::NoSession) {
                    self.drop_token();
                }
                Err(e.into())
            }
        }
    }

    /// Changes the passphrase. The server ends every admin session, this one
    /// too: Settings stay open for the confirmation, but any further change
    /// needs the new passphrase.
    ///
    /// # Errors
    ///
    /// Invalid new passphrase, wrong current one (counts for the lockout),
    /// `no_session`, offline.
    pub async fn change_passphrase(&self, old: &str, new: &str) -> Result<(), CmdError> {
        let new = check_passphrase(new).map_err(passphrase_error)?;
        let old = nonempty_passphrase(old)?;
        if same_passphrase(&old, &new) {
            return Err(CmdError::invalid("passphrase", "same_as_current"));
        }
        let pairing = self.pairing()?;
        let token = self.session_token()?;
        let result = pairing
            .api
            .change_passphrase(&pairing.secret, &token, &old, &new)
            .await;
        if let Err(ApiError::Rejected(r)) = &result
            && r.error == clockin_sync::ErrorCode::NoSession
        {
            self.drop_token();
        }
        self.note_lockout(result)?;
        self.drop_token();
        Ok(())
    }

    /// The quit code currently in force, for settings saves that keep it.
    #[must_use]
    pub fn current_quit_code(&self) -> Option<QuitCode> {
        self.with_snapshot(|s| s.and_then(|s| s.settings.quit_code.clone()))
    }

    // --- Windows: quitting, autostart, alarms ------------------------------

    /// Whether `typed` lets the app quit (SPEC §8.1): the synced quit code,
    /// compared on this device so it works offline. Before a quit code is
    /// known (not paired yet, or never synced) there is nothing to protect
    /// and any answer quits.
    #[cfg(desktop)]
    #[must_use]
    pub fn quit_allowed(&self, typed: &str) -> bool {
        self.current_quit_code()
            .is_none_or(|code| code.expose() == typed)
    }

    /// Whether a quit code is known (else Quit only asks for confirmation).
    #[must_use]
    pub fn quit_code_set(&self) -> bool {
        self.current_quit_code().is_some()
    }

    /// The "Start with Windows" setting; on (the default, SPEC §4.4) until
    /// the first sync.
    #[cfg(desktop)]
    #[must_use]
    pub fn autostart_wanted(&self) -> bool {
        self.with_snapshot(|s| s.is_none_or(|s| s.settings.autostart))
    }

    /// The alarms this device already rang and that were stopped or ended.
    #[cfg(desktop)]
    #[must_use]
    pub fn handled_alarms(&self) -> Vec<AlarmKey> {
        lock(&self.ui_store)
            .device_setting(SETTING_ALARMS_HANDLED)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    #[cfg(desktop)]
    pub fn save_handled_alarms(&self, keys: &[AlarmKey]) {
        let Ok(json) = serde_json::to_string(keys) else {
            return;
        };
        if let Err(e) =
            lock(&self.ui_store).set_device_setting(SETTING_ALARMS_HANDLED, &json, Timestamp::now())
        {
            eprintln!("clock-in: could not save the stopped alarms: {e}");
        }
    }

    /// Before an alarm rings: one sync round for the latest marks, at most a
    /// few seconds (offline it gives up and the alarm rings: fail loud).
    #[cfg(desktop)]
    pub async fn refresh_for_alarm(&self) {
        if let Ok(pairing) = self.pairing() {
            let ticket = pairing.sync.request();
            pairing.sync.wait(ticket, ALARM_SYNC_WAIT).await;
        }
    }
}

pub(crate) fn no_session() -> CmdError {
    CmdError::Rejected {
        code: "no_session".into(),
        retry_after_s: None,
        details: None,
    }
}

fn nonempty_passphrase(raw: &str) -> Result<String, CmdError> {
    let normalized = normalize_passphrase(raw);
    if normalized.is_empty() {
        Err(CmdError::invalid("passphrase", "empty"))
    } else {
        Ok(normalized)
    }
}

/// The core's passphrase problem as an `Invalid` error (word positions only,
/// never the words).
#[must_use]
pub fn passphrase_error(e: clockin_core::PassphraseError) -> CmdError {
    use clockin_core::PassphraseError as P;
    match e {
        P::TooFewWords { .. } => CmdError::invalid("passphrase", "too_few_words"),
        P::TooShort => CmdError::invalid("passphrase", "too_short"),
        P::UnknownWords { positions } => CmdError::Invalid {
            field: "passphrase",
            problem: "unknown_words".into(),
            positions: Some(positions),
            character: None,
        },
    }
}

fn logout_in_background(pairing: &Pairing, token: SessionToken) {
    let api = pairing.api.clone();
    let secret = pairing.secret.clone();
    tauri::async_runtime::spawn(async move {
        // Best effort: the server session expires by itself anyway.
        let _ = api.admin_logout(&secret, &token).await;
    });
}

fn remove_db_files(db_path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut path = db_path.as_os_str().to_owned();
        path.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(path));
    }
}

async fn sync_loop(state: Arc<AppState>, mut engine: SyncEngine<Api>, sync: Arc<SyncControl>) {
    loop {
        if sync.stop.load(Ordering::SeqCst) {
            return;
        }
        let ticket = sync.requested.load(Ordering::SeqCst);
        let delay = match engine.tick(state.clock.now()).await {
            Ok(report) => report.next_tick_in,
            Err(e) => {
                eprintln!("clock-in: local database error during sync: {e}");
                SignedDuration::from_secs(5)
            }
        };
        if sync.stop.load(Ordering::SeqCst) {
            return;
        }
        state.publish(&engine);
        if !engine.status().paired {
            state.forget_pairing(&sync);
            return;
        }
        sync.done.send_modify(|done| *done = (*done).max(ticket));
        let delay = Duration::try_from(delay).unwrap_or(Duration::from_secs(5));
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = sync.wake.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clockin-app-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_start_creates_the_key_and_reopens_with_it() {
        let dir = temp_dir();
        let secrets = Arc::new(MemorySecrets::default());
        struct Shared(Arc<MemorySecrets>);
        impl SecretStore for Shared {
            fn get(&self, n: &str) -> Result<Option<String>, crate::secrets::SecretError> {
                self.0.get(n)
            }
            fn set(&self, n: &str, v: &str) -> Result<(), crate::secrets::SecretError> {
                self.0.set(n, v)
            }
            fn delete(&self, n: &str) -> Result<(), crate::secrets::SecretError> {
                self.0.delete(n)
            }
        }
        let (state, secret) = AppState::open(
            Profile::default(),
            &dir,
            Box::new(Shared(Arc::clone(&secrets))),
            None,
        )
        .unwrap();
        assert!(secret.is_none());
        assert_eq!(state.phase(), Phase::NotConfigured);
        lock(&state.ui_store)
            .set_device_setting("x", "1", Timestamp::now())
            .unwrap();
        let key = secrets.get(SECRET_STORE_KEY).unwrap().unwrap();
        drop(state);

        // Same key: the data is still there.
        let (state, _) = AppState::open(
            Profile::default(),
            &dir,
            Box::new(Shared(Arc::clone(&secrets))),
            None,
        )
        .unwrap();
        assert_eq!(
            lock(&state.ui_store)
                .device_setting("x")
                .unwrap()
                .as_deref(),
            Some("1")
        );
        drop(state);

        // Key lost: a fresh, empty database instead of an error.
        secrets.delete(SECRET_STORE_KEY).unwrap();
        let (state, _) = AppState::open(
            Profile::default(),
            &dir,
            Box::new(Shared(Arc::clone(&secrets))),
            None,
        )
        .unwrap();
        assert_eq!(lock(&state.ui_store).device_setting("x").unwrap(), None);
        assert_ne!(secrets.get(SECRET_STORE_KEY).unwrap().unwrap(), key);
        drop(state);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unpaired_state_refuses_device_calls() {
        let dir = temp_dir();
        let config = ServerConfig {
            url: "https://example.supabase.co".into(),
            publishable_key: "sb_publishable_x".into(),
        };
        let (state, _) = AppState::open(
            Profile::default(),
            &dir,
            Box::new(MemorySecrets::default()),
            Some(config),
        )
        .unwrap();
        assert_eq!(state.phase(), Phase::Unpaired);
        assert!(!state.admin_unlocked());
        assert_eq!(state.today().unwrap(), None);
        assert_eq!(
            state.mark(
                Uuid::nil(),
                Uuid::nil(),
                Date::constant(2026, 10, 5),
                MarkKind::In
            ),
            Err(CmdError::NotPaired)
        );
        assert_eq!(state.admin_view(), Err(CmdError::NotPaired));
        assert_eq!(state.lockout_remaining_s(), 0);
        // An unchanged passphrase is refused before anything is sent.
        let same = tauri::async_runtime::block_on(state.change_passphrase(
            "abacus zoom cloud tiger mango",
            " Abacus Zoom Cloud Tiger Mango",
        ));
        assert_eq!(
            same,
            Err(CmdError::invalid("passphrase", "same_as_current"))
        );
        drop(state);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_alert_mode_is_ring_until_changed_and_survives_a_restart() {
        let dir = temp_dir();
        let secrets = || -> Box<dyn SecretStore> {
            let s = MemorySecrets::default();
            s.set(SECRET_STORE_KEY, &"cd".repeat(32)).unwrap();
            Box::new(s)
        };
        let (state, _) = AppState::open(Profile::default(), &dir, secrets(), None).unwrap();
        assert_eq!(state.alert_mode(), AlertMode::Ring);
        state.set_alert_mode(AlertMode::Notification).unwrap();
        assert_eq!(state.alert_mode(), AlertMode::Notification);
        drop(state);
        let (state, _) = AppState::open(Profile::default(), &dir, secrets(), None).unwrap();
        assert_eq!(state.alert_mode(), AlertMode::Notification);
        assert_eq!(state.config_version(), None);
        assert!(!state.default_device_name().is_empty());
        drop(state);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_reported_lockout_is_remembered_across_restarts() {
        let dir = temp_dir();
        let secrets = || -> Box<dyn SecretStore> {
            // Each open gets the same key through a file-backed store would be
            // more realistic; a fixed key in memory does the same here.
            let s = MemorySecrets::default();
            s.set(SECRET_STORE_KEY, &"ab".repeat(32)).unwrap();
            Box::new(s)
        };
        let (state, _) = AppState::open(Profile::default(), &dir, secrets(), None).unwrap();
        let rejected = ApiError::Rejected(clockin_sync::Rejection {
            error: clockin_sync::ErrorCode::BadPassphrase,
            retry_after_s: Some(60),
            details: None,
            config_version: None,
        });
        let err = state.note_lockout::<()>(Err(rejected)).unwrap_err();
        assert_eq!(err.code(), Some("bad_passphrase"));
        assert!((59..=60).contains(&state.lockout_remaining_s()));
        drop(state);

        let (state, _) = AppState::open(Profile::default(), &dir, secrets(), None).unwrap();
        assert!((59..=60).contains(&state.lockout_remaining_s()));
        state.note_lockout(Ok(())).unwrap();
        assert_eq!(state.lockout_remaining_s(), 0);
        drop(state);
        let (state, _) = AppState::open(Profile::default(), &dir, secrets(), None).unwrap();
        assert_eq!(state.lockout_remaining_s(), 0);
        drop(state);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
