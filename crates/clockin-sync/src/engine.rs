//! The sync loop's logic (ARCHITECTURE §8), one [`SyncEngine::tick`] at a
//! time. The platform layer owns the timer: it calls `tick` with the current
//! time and sleeps for the returned [`TickReport::next_tick_in`] (or less,
//! after a local change or an admin call).
//!
//! Each tick:
//! 1. `get_version` (and the clock-skew check);
//! 2. flushes queued marks;
//! 3. refetches the snapshot if `data_version` changed or marks were sent;
//! 4. keeps the server's alarm plan fresh: if the configuration changed or
//!    the plan reaches less than 13 days ahead, computes a 14-day plan with
//!    `clockin-core` and uploads it (retrying once after a version conflict).

use crate::api::{
    Api, ApiError, ErrorCode, MarkReceipt, MarkRequest, ServerMark, ServerSnapshot, Versions,
};
use crate::secret::DeviceSecret;
use crate::store::{CachedSnapshot, Store, StoreError};
use clockin_core::{
    MarkKind, PlanItem, Snapshot, alarm_plan, clock_skew_exceeded, horizon_short,
    plan_needs_refresh, plan_window, sync_retry_delay,
};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use std::future::Future;
use uuid::Uuid;

/// The server calls the sync loop needs. [`Api`] is the real one; tests use
/// a fake.
pub trait SyncBackend: Send + Sync {
    fn get_version(
        &self,
        secret: &DeviceSecret,
    ) -> impl Future<Output = Result<Versions, ApiError>> + Send;

    fn get_snapshot(
        &self,
        secret: &DeviceSecret,
    ) -> impl Future<Output = Result<ServerSnapshot, ApiError>> + Send;

    fn mark(
        &self,
        secret: &DeviceSecret,
        mark: &MarkRequest,
    ) -> impl Future<Output = Result<MarkReceipt, ApiError>> + Send;

    fn upload_plan(
        &self,
        secret: &DeviceSecret,
        base_config_version: i64,
        horizon_end: Timestamp,
        items: &[PlanItem],
    ) -> impl Future<Output = Result<u32, ApiError>> + Send;
}

impl SyncBackend for Api {
    fn get_version(
        &self,
        secret: &DeviceSecret,
    ) -> impl Future<Output = Result<Versions, ApiError>> + Send {
        Api::get_version(self, secret)
    }

    fn get_snapshot(
        &self,
        secret: &DeviceSecret,
    ) -> impl Future<Output = Result<ServerSnapshot, ApiError>> + Send {
        Api::get_snapshot(self, secret)
    }

    fn mark(
        &self,
        secret: &DeviceSecret,
        mark: &MarkRequest,
    ) -> impl Future<Output = Result<MarkReceipt, ApiError>> + Send {
        Api::mark(self, secret, mark)
    }

    fn upload_plan(
        &self,
        secret: &DeviceSecret,
        base_config_version: i64,
        horizon_end: Timestamp,
        items: &[PlanItem],
    ) -> impl Future<Output = Result<u32, ApiError>> + Send {
        Api::upload_plan(self, secret, base_config_version, horizon_end, items)
    }
}

/// What the banners need to know (SPEC §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStatus {
    /// The last attempt reached the server. False shows the offline banner.
    pub online: bool,
    /// False once the server says this device's secret is unknown or revoked.
    pub paired: bool,
    /// The last successful sync, shown on the offline banner.
    pub last_sync: Option<Timestamp>,
    pub consecutive_failures: u32,
    /// The device clock is more than 2 minutes off server time.
    pub clock_skew: bool,
    /// How far the server's alarm plan reaches.
    pub plan_horizon_end: Option<Timestamp>,
    /// Marks made on this device that the server hasn't confirmed yet.
    pub pending_marks: usize,
}

/// The result of one [`SyncEngine::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// The cached snapshot changed: recompute the Today view and alarms.
    pub data_changed: bool,
    /// When to tick again.
    pub next_tick_in: SignedDuration,
}

enum SyncError {
    Api(ApiError),
    Store(StoreError),
}

impl From<ApiError> for SyncError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

impl From<StoreError> for SyncError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// One paired device's sync state: the backend, the local store and the
/// device secret.
pub struct SyncEngine<B> {
    backend: B,
    store: Store,
    secret: DeviceSecret,
    cached: Option<CachedSnapshot>,
    status: SyncStatus,
}

impl<B> std::fmt::Debug for SyncEngine<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncEngine")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl<B: SyncBackend> SyncEngine<B> {
    /// Starts from whatever the store has cached (offline-first).
    ///
    /// # Errors
    ///
    /// If the store can't be read.
    pub fn new(backend: B, store: Store, secret: DeviceSecret) -> Result<Self, StoreError> {
        let cached = store.snapshot()?;
        let status = SyncStatus {
            online: false,
            paired: true,
            last_sync: store.last_sync()?,
            consecutive_failures: 0,
            clock_skew: false,
            plan_horizon_end: cached.as_ref().and_then(|c| c.snapshot.plan_horizon_end),
            pending_marks: store.pending_marks()?.len(),
        };
        Ok(Self {
            backend,
            store,
            secret,
            cached,
            status,
        })
    }

    #[must_use]
    pub fn status(&self) -> &SyncStatus {
        &self.status
    }

    /// Whether the alarm plan covers fewer than 3 days (SPEC §6 banner).
    #[must_use]
    pub fn horizon_short(&self, now: Timestamp) -> bool {
        horizon_short(self.status.plan_horizon_end, now)
    }

    /// The last snapshot from the server, if this device ever synced.
    #[must_use]
    pub fn snapshot(&self) -> Option<&ServerSnapshot> {
        self.cached.as_ref().map(|c| &c.snapshot)
    }

    /// The core's view of the cached snapshot plus marks still queued here,
    /// so a mark shows at once, also offline.
    ///
    /// # Errors
    ///
    /// If the queue can't be read.
    pub fn core_snapshot(&self) -> Result<Option<Snapshot>, StoreError> {
        self.cached
            .as_ref()
            .map(|c| core_snapshot_with_pending(&c.snapshot, &self.store))
            .transpose()
    }

    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    #[must_use]
    pub fn secret(&self) -> &DeviceSecret {
        &self.secret
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Records a check-in or check-out locally; the next tick sends it. If
    /// that block occurrence already has a mark of this kind, returns its id
    /// and queues nothing.
    ///
    /// # Errors
    ///
    /// If the queue can't be written.
    pub fn queue_mark(
        &mut self,
        staff_id: Uuid,
        source_block_id: Uuid,
        business_date: Date,
        kind: MarkKind,
        now: Timestamp,
    ) -> Result<Uuid, StoreError> {
        let id = queue_mark(
            &self.store,
            self.cached.as_ref().map(|c| &c.snapshot),
            staff_id,
            source_block_id,
            business_date,
            kind,
            now,
        )?;
        self.status.pending_marks = self.store.pending_marks()?.len();
        Ok(id)
    }

    /// One round of sync. Server problems never fail the tick: they show in
    /// [`SyncEngine::status`] and lengthen the next delay, and the cached
    /// state stays in use (fail loud).
    ///
    /// # Errors
    ///
    /// Only for local database errors.
    pub async fn tick(&mut self, now: Timestamp) -> Result<TickReport, StoreError> {
        let result = self.contact(now).await;
        // Another connection (the UI's) may queue marks too.
        self.status.pending_marks = self.store.pending_marks()?.len();
        match result {
            Ok(data_changed) => {
                self.status.online = true;
                self.status.paired = true;
                self.status.consecutive_failures = 0;
                self.status.last_sync = Some(now);
                self.store.set_last_sync(now)?;
                Ok(TickReport {
                    data_changed,
                    next_tick_in: sync_retry_delay(0),
                })
            }
            Err(SyncError::Store(e)) => Err(e),
            Err(SyncError::Api(e)) => {
                self.status.consecutive_failures =
                    self.status.consecutive_failures.saturating_add(1);
                self.status.online = !e.is_unreachable();
                if e.is_unpaired() {
                    self.status.paired = false;
                }
                Ok(TickReport {
                    data_changed: false,
                    next_tick_in: sync_retry_delay(self.status.consecutive_failures),
                })
            }
        }
    }

    async fn contact(&mut self, now: Timestamp) -> Result<bool, SyncError> {
        let versions = self.backend.get_version(&self.secret).await?;
        self.status.clock_skew = clock_skew_exceeded(versions.server_now, now);

        let flushed = self.flush_marks(now).await?;
        let stale = self
            .cached
            .as_ref()
            .is_none_or(|c| c.snapshot.data_version != versions.data_version);
        if flushed || stale {
            self.refresh_snapshot(now).await?;
        }
        self.upkeep_plan(&versions, now).await?;
        Ok(flushed || stale)
    }

    async fn refresh_snapshot(&mut self, now: Timestamp) -> Result<(), SyncError> {
        let snapshot = self.backend.get_snapshot(&self.secret).await?;
        self.store.save_snapshot(&snapshot, now)?;
        self.cached = Some(CachedSnapshot {
            snapshot,
            fetched_at: now,
        });
        Ok(())
    }

    /// Sends queued marks, oldest first. Returns whether any left the queue.
    async fn flush_marks(&mut self, now: Timestamp) -> Result<bool, SyncError> {
        let pending = self.store.pending_marks()?;
        let mut any = false;
        for mark in pending {
            match self.backend.mark(&self.secret, &mark).await {
                Ok(receipt) => self.remember_confirmed(&mark, &receipt, now)?,
                // The server will never accept it (e.g. a mark queued more
                // than two business days ago): retrying can't help.
                Err(e) if e.code() == Some(&ErrorCode::InvalidInput) => {}
                Err(e) => return Err(e.into()),
            }
            self.store.remove_pending_mark(mark.client_id)?;
            self.status.pending_marks = self.status.pending_marks.saturating_sub(1);
            any = true;
        }
        Ok(any)
    }

    /// Keeps a confirmed mark visible even if the following snapshot fetch
    /// fails.
    fn remember_confirmed(
        &mut self,
        mark: &MarkRequest,
        receipt: &MarkReceipt,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let Some(cached) = &mut self.cached else {
            return Ok(());
        };
        if receipt.voided
            || cached
                .snapshot
                .marks
                .iter()
                .any(|m| m.id == receipt.mark_id)
        {
            return Ok(());
        }
        cached.snapshot.marks.push(ServerMark {
            id: receipt.mark_id,
            staff_id: mark.staff_id,
            source_block_id: mark.source_block_id,
            business_date: mark.business_date,
            kind: mark.kind,
            marked_at: receipt.marked_at,
            device_id: Some(cached.snapshot.this_device_id),
        });
        self.store.save_snapshot(&cached.snapshot, now)
    }

    async fn upkeep_plan(&mut self, versions: &Versions, now: Timestamp) -> Result<(), SyncError> {
        self.status.plan_horizon_end = versions.plan_horizon_end;
        let Some(cached) = &self.cached else {
            return Ok(());
        };
        let config_version = versions.config_version.max(cached.snapshot.config_version);
        if !plan_needs_refresh(
            config_version,
            versions.plan_config_version,
            versions.plan_horizon_end,
            now,
        ) {
            return Ok(());
        }

        for attempt in 0..2 {
            let Some(cached) = &self.cached else {
                return Ok(());
            };
            let base = cached.snapshot.config_version;
            let (from, until) = plan_window(now);
            let items = alarm_plan(&cached.snapshot.core_snapshot(&[]), from, until);
            match self
                .backend
                .upload_plan(&self.secret, base, until, &items)
                .await
            {
                Ok(_) => {
                    self.status.plan_horizon_end = Some(until);
                    return Ok(());
                }
                Err(e) if e.code() == Some(&ErrorCode::VersionConflict) => {
                    if attempt == 0 {
                        self.refresh_snapshot(now).await?;
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        // Still conflicting: someone keeps editing. The next tick tries again.
        Ok(())
    }
}

/// The core's view of `snapshot` plus the marks still queued in `store`.
///
/// # Errors
///
/// If the queue can't be read.
pub fn core_snapshot_with_pending(
    snapshot: &ServerSnapshot,
    store: &Store,
) -> Result<Snapshot, StoreError> {
    let pending: Vec<_> = store
        .pending_marks()?
        .iter()
        .map(MarkRequest::core)
        .collect();
    Ok(snapshot.core_snapshot(&pending))
}

/// Queues a check-in or check-out in `store`; the next sync tick sends it.
/// If `snapshot` or the queue already has a mark of this kind for that block
/// occurrence, returns its id and queues nothing.
///
/// The UI calls this on its own connection to the store, so a tap never
/// waits for a sync round that is talking to a slow server.
///
/// # Errors
///
/// If the queue can't be read or written.
pub fn queue_mark(
    store: &Store,
    snapshot: Option<&ServerSnapshot>,
    staff_id: Uuid,
    source_block_id: Uuid,
    business_date: Date,
    kind: MarkKind,
    now: Timestamp,
) -> Result<Uuid, StoreError> {
    let pending = store.pending_marks()?;
    let same = |s: Uuid, b: Uuid, d: Date, k: MarkKind| {
        s == staff_id && b == source_block_id && d == business_date && k == kind
    };
    if let Some(existing) = snapshot
        .into_iter()
        .flat_map(|s| s.marks.iter())
        .find(|m| same(m.staff_id, m.source_block_id, m.business_date, m.kind))
    {
        return Ok(existing.id);
    }
    if let Some(existing) = pending
        .iter()
        .find(|m| same(m.staff_id, m.source_block_id, m.business_date, m.kind))
    {
        return Ok(existing.client_id);
    }
    let mark = MarkRequest {
        client_id: Uuid::new_v4(),
        staff_id,
        source_block_id,
        business_date,
        kind,
    };
    store.queue_mark(&mark, now)?;
    Ok(mark.client_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Rejection, ServerSettings};
    use crate::store::StoreKey;
    use clockin_core::{Staff, WeeklyBlock};
    use jiff::civil::{Time, Weekday, date};
    use std::sync::Mutex;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    /// Monday 2026-10-05, 09:00 in Athens.
    fn now() -> Timestamp {
        ts("2026-10-05T06:00:00Z")
    }

    fn secs(n: i64) -> SignedDuration {
        SignedDuration::from_secs(n)
    }

    fn rejection(error: ErrorCode) -> ApiError {
        ApiError::Rejected(Rejection {
            error,
            retry_after_s: None,
            details: None,
            config_version: None,
        })
    }

    const STAFF: Uuid = Uuid::from_u128(0xA);
    const BLOCK: Uuid = Uuid::from_u128(0xB);

    /// A scripted server.
    struct Server {
        snapshot: ServerSnapshot,
        offline: bool,
        unpaired: bool,
        /// Answer the next N uploads with a version conflict.
        conflicts: u32,
        reject_marks: bool,
        snapshot_calls: u32,
        uploads: Vec<(i64, Timestamp, usize)>,
        received_marks: Vec<MarkRequest>,
    }

    struct FakeBackend(Mutex<Server>);

    impl FakeBackend {
        fn new() -> Self {
            let snapshot = ServerSnapshot {
                staff: vec![Staff {
                    id: STAFF,
                    first_name: "Μαρία".into(),
                    last_name: "Παππά".into(),
                    removed_at: None,
                }],
                blocks: vec![WeeklyBlock {
                    id: BLOCK,
                    staff_id: STAFF,
                    weekday: Weekday::Monday,
                    start: Time::constant(12, 0, 0, 0),
                    end: Time::constant(16, 0, 0, 0),
                }],
                overrides: vec![],
                settings: ServerSettings {
                    checkin_offset_min: 0,
                    checkout_offset_min: 0,
                    rollover: Time::constant(5, 0, 0, 0),
                    autostart: true,
                    quit_code: None,
                },
                marks: vec![],
                devices: vec![],
                this_device_id: Uuid::from_u128(0xD),
                config_version: 4,
                data_version: 10,
                plan_config_version: 0,
                plan_horizon_end: None,
                server_now: now(),
            };
            Self(Mutex::new(Server {
                snapshot,
                offline: false,
                unpaired: false,
                conflicts: 0,
                reject_marks: false,
                snapshot_calls: 0,
                uploads: vec![],
                received_marks: vec![],
            }))
        }

        fn with<T>(&self, f: impl FnOnce(&mut Server) -> T) -> T {
            f(&mut self.0.lock().unwrap())
        }

        fn check(s: &Server) -> Result<(), ApiError> {
            if s.offline {
                return Err(ApiError::Network("offline".into()));
            }
            if s.unpaired {
                return Err(rejection(ErrorCode::Revoked));
            }
            Ok(())
        }
    }

    impl SyncBackend for FakeBackend {
        fn get_version(
            &self,
            _: &DeviceSecret,
        ) -> impl Future<Output = Result<Versions, ApiError>> + Send {
            let r = self.with(|s| {
                Self::check(s)?;
                Ok(Versions {
                    config_version: s.snapshot.config_version,
                    data_version: s.snapshot.data_version,
                    plan_config_version: s.snapshot.plan_config_version,
                    plan_horizon_end: s.snapshot.plan_horizon_end,
                    server_now: s.snapshot.server_now,
                })
            });
            async move { r }
        }

        fn get_snapshot(
            &self,
            _: &DeviceSecret,
        ) -> impl Future<Output = Result<ServerSnapshot, ApiError>> + Send {
            let r = self.with(|s| {
                Self::check(s)?;
                s.snapshot_calls += 1;
                Ok(s.snapshot.clone())
            });
            async move { r }
        }

        fn mark(
            &self,
            _: &DeviceSecret,
            mark: &MarkRequest,
        ) -> impl Future<Output = Result<MarkReceipt, ApiError>> + Send {
            let r = self.with(|s| {
                Self::check(s)?;
                if s.reject_marks {
                    return Err(rejection(ErrorCode::InvalidInput));
                }
                s.received_marks.push(mark.clone());
                s.snapshot.marks.push(ServerMark {
                    id: mark.client_id,
                    staff_id: mark.staff_id,
                    source_block_id: mark.source_block_id,
                    business_date: mark.business_date,
                    kind: mark.kind,
                    marked_at: s.snapshot.server_now,
                    device_id: None,
                });
                s.snapshot.data_version += 1;
                Ok(MarkReceipt {
                    mark_id: mark.client_id,
                    marked_at: s.snapshot.server_now,
                    voided: false,
                })
            });
            async move { r }
        }

        fn upload_plan(
            &self,
            _: &DeviceSecret,
            base: i64,
            horizon_end: Timestamp,
            items: &[PlanItem],
        ) -> impl Future<Output = Result<u32, ApiError>> + Send {
            let r = self.with(|s| {
                Self::check(s)?;
                if s.conflicts > 0 {
                    s.conflicts -= 1;
                    return Err(rejection(ErrorCode::VersionConflict));
                }
                if base != s.snapshot.config_version {
                    return Err(rejection(ErrorCode::VersionConflict));
                }
                s.uploads.push((base, horizon_end, items.len()));
                s.snapshot.plan_config_version = base;
                s.snapshot.plan_horizon_end = Some(horizon_end);
                Ok(u32::try_from(items.len()).unwrap())
            });
            async move { r }
        }
    }

    fn engine() -> SyncEngine<FakeBackend> {
        let store = Store::open_in_memory(&StoreKey::generate().unwrap()).unwrap();
        SyncEngine::new(FakeBackend::new(), store, DeviceSecret::new("s".into())).unwrap()
    }

    fn block_on<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn tick_future_is_send() {
        fn assert_send<T: Send>(_: T) {}
        let mut e = engine();
        assert_send(e.tick(now()));
    }

    #[test]
    fn first_tick_fetches_the_snapshot_and_uploads_the_plan() {
        let mut e = engine();
        assert_eq!(e.snapshot(), None);
        assert_eq!(e.core_snapshot().unwrap(), None);

        let report = block_on(e.tick(now())).unwrap();
        assert!(report.data_changed);
        assert_eq!(report.next_tick_in, secs(5));
        assert!(e.status().online);
        assert_eq!(e.status().last_sync, Some(now()));
        assert_eq!(e.snapshot().unwrap().data_version, 10);
        assert_eq!(e.store().last_sync().unwrap(), Some(now()));

        let (_, until) = plan_window(now());
        e.backend().with(|s| {
            // Two Mondays (check-in and check-out each) in the 14-day window.
            assert_eq!(s.uploads, [(4, until, 4)]);
        });
        assert_eq!(e.status().plan_horizon_end, Some(until));
        assert!(!e.horizon_short(now()));
    }

    #[test]
    fn unchanged_server_means_no_refetch_and_no_upload() {
        let mut e = engine();
        block_on(e.tick(now())).unwrap();
        let report = block_on(e.tick(now() + secs(5))).unwrap();
        assert!(!report.data_changed);
        e.backend().with(|s| {
            assert_eq!(s.snapshot_calls, 1);
            assert_eq!(s.uploads.len(), 1);
        });
    }

    #[test]
    fn a_new_data_version_is_refetched_and_a_config_change_replans() {
        let mut e = engine();
        block_on(e.tick(now())).unwrap();
        e.backend().with(|s| {
            s.snapshot.blocks[0].start = Time::constant(12, 30, 0, 0);
            s.snapshot.config_version = 5;
            s.snapshot.data_version = 11;
        });
        let report = block_on(e.tick(now() + secs(5))).unwrap();
        assert!(report.data_changed);
        assert_eq!(
            e.snapshot().unwrap().blocks[0].start,
            Time::constant(12, 30, 0, 0)
        );
        e.backend().with(|s| {
            assert_eq!(s.uploads.len(), 2);
            assert_eq!(s.uploads[1].0, 5);
        });
    }

    #[test]
    fn plan_is_refreshed_when_the_horizon_gets_short() {
        let mut e = engine();
        block_on(e.tick(now())).unwrap();
        let later = now() + SignedDuration::from_hours(25);
        block_on(e.tick(later)).unwrap();
        e.backend().with(|s| assert_eq!(s.uploads.len(), 2));
        assert_eq!(e.status().plan_horizon_end, Some(plan_window(later).1));
    }

    #[test]
    fn queued_marks_show_at_once_and_are_flushed_on_the_next_tick() {
        let mut e = engine();
        block_on(e.tick(now())).unwrap();

        let day = date(2026, 10, 5);
        let id = e
            .queue_mark(STAFF, BLOCK, day, MarkKind::In, now())
            .unwrap();
        assert_eq!(e.status().pending_marks, 1);
        // Marking the same occurrence again returns the same mark.
        assert_eq!(
            e.queue_mark(STAFF, BLOCK, day, MarkKind::In, now())
                .unwrap(),
            id
        );
        assert_eq!(e.status().pending_marks, 1);
        let marks = e.core_snapshot().unwrap().unwrap().marks;
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].id, id);

        let report = block_on(e.tick(now() + secs(1))).unwrap();
        assert!(report.data_changed);
        assert_eq!(e.status().pending_marks, 0);
        assert!(e.store().pending_marks().unwrap().is_empty());
        assert_eq!(e.snapshot().unwrap().marks[0].id, id);
        e.backend().with(|s| assert_eq!(s.received_marks.len(), 1));
        // The server already has it: no second queue entry.
        assert_eq!(
            e.queue_mark(STAFF, BLOCK, day, MarkKind::In, now())
                .unwrap(),
            id
        );
        assert_eq!(e.status().pending_marks, 0);
    }

    #[test]
    fn offline_keeps_the_cache_and_the_queue_and_backs_off() {
        let mut e = engine();
        block_on(e.tick(now())).unwrap();
        e.backend().with(|s| s.offline = true);
        let id = e
            .queue_mark(STAFF, BLOCK, date(2026, 10, 5), MarkKind::In, now())
            .unwrap();

        let delays: Vec<i64> = (1..=5)
            .map(|n| {
                let r = block_on(e.tick(now() + secs(n))).unwrap();
                assert!(!r.data_changed);
                r.next_tick_in.as_secs()
            })
            .collect();
        assert_eq!(delays, [5, 10, 30, 60, 60]);
        assert!(!e.status().online);
        assert!(e.status().paired);
        assert_eq!(e.status().last_sync, Some(now()));
        assert_eq!(e.status().consecutive_failures, 5);
        // Still usable offline: the cached schedule plus the queued mark.
        let snap = e.core_snapshot().unwrap().unwrap();
        assert_eq!(snap.blocks.len(), 1);
        assert_eq!(snap.marks[0].id, id);

        e.backend().with(|s| s.offline = false);
        let report = block_on(e.tick(now() + secs(300))).unwrap();
        assert_eq!(report.next_tick_in, secs(5));
        assert!(e.status().online);
        assert_eq!(e.status().consecutive_failures, 0);
        assert_eq!(e.status().pending_marks, 0);
        e.backend()
            .with(|s| assert_eq!(s.received_marks[0].client_id, id));
    }

    #[test]
    fn a_restarted_engine_starts_from_the_cache() {
        let key = StoreKey::generate().unwrap();
        let dir = std::env::temp_dir().join(format!("clockin-engine-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("local.db");
        {
            let store = Store::open(&path, &key).unwrap();
            let mut e =
                SyncEngine::new(FakeBackend::new(), store, DeviceSecret::new("s".into())).unwrap();
            block_on(e.tick(now())).unwrap();
            e.queue_mark(STAFF, BLOCK, date(2026, 10, 5), MarkKind::In, now())
                .unwrap();
        }
        let store = Store::open(&path, &key).unwrap();
        let e = SyncEngine::new(FakeBackend::new(), store, DeviceSecret::new("s".into())).unwrap();
        assert!(!e.status().online);
        assert_eq!(e.status().last_sync, Some(now()));
        assert_eq!(e.status().pending_marks, 1);
        assert_eq!(e.core_snapshot().unwrap().unwrap().marks.len(), 1);
        drop(e);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn marks_queued_on_another_connection_are_flushed() {
        let key = StoreKey::generate().unwrap();
        let dir = std::env::temp_dir().join(format!("clockin-engine-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("local.db");
        let store = Store::open(&path, &key).unwrap();
        let mut e =
            SyncEngine::new(FakeBackend::new(), store, DeviceSecret::new("s".into())).unwrap();
        block_on(e.tick(now())).unwrap();

        let ui = Store::open(&path, &key).unwrap();
        let day = date(2026, 10, 5);
        let id = queue_mark(&ui, e.snapshot(), STAFF, BLOCK, day, MarkKind::In, now()).unwrap();
        // The UI sees it at once, and queuing it again is a no-op.
        let snap = core_snapshot_with_pending(e.snapshot().unwrap(), &ui).unwrap();
        assert_eq!(snap.marks.len(), 1);
        assert_eq!(
            queue_mark(&ui, e.snapshot(), STAFF, BLOCK, day, MarkKind::In, now()).unwrap(),
            id
        );
        assert_eq!(ui.pending_marks().unwrap().len(), 1);

        let report = block_on(e.tick(now() + secs(1))).unwrap();
        assert!(report.data_changed);
        assert_eq!(e.status().pending_marks, 0);
        assert!(ui.pending_marks().unwrap().is_empty());
        e.backend()
            .with(|s| assert_eq!(s.received_marks[0].client_id, id));
        // Now the server has it: still the same id.
        assert_eq!(
            queue_mark(&ui, e.snapshot(), STAFF, BLOCK, day, MarkKind::In, now()).unwrap(),
            id
        );
        drop((e, ui));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn version_conflict_refetches_and_retries_once() {
        let mut e = engine();
        e.backend().with(|s| s.conflicts = 1);
        let report = block_on(e.tick(now())).unwrap();
        assert_eq!(report.next_tick_in, secs(5));
        e.backend().with(|s| {
            assert_eq!(s.snapshot_calls, 2);
            assert_eq!(s.uploads.len(), 1);
        });

        // Two conflicts in a row: no error, the next tick tries again.
        let mut e = engine();
        e.backend().with(|s| s.conflicts = 2);
        let report = block_on(e.tick(now())).unwrap();
        assert!(e.status().online);
        assert_eq!(report.next_tick_in, secs(5));
        e.backend().with(|s| assert!(s.uploads.is_empty()));
        block_on(e.tick(now() + secs(5))).unwrap();
        e.backend().with(|s| assert_eq!(s.uploads.len(), 1));
    }

    #[test]
    fn marks_the_server_refuses_are_dropped() {
        let mut e = engine();
        block_on(e.tick(now())).unwrap();
        e.backend().with(|s| s.reject_marks = true);
        e.queue_mark(STAFF, BLOCK, date(2026, 10, 5), MarkKind::In, now())
            .unwrap();
        block_on(e.tick(now() + secs(5))).unwrap();
        assert!(e.status().online);
        assert_eq!(e.status().pending_marks, 0);
        assert!(e.store().pending_marks().unwrap().is_empty());
    }

    #[test]
    fn a_revoked_device_is_reported_unpaired() {
        let mut e = engine();
        block_on(e.tick(now())).unwrap();
        e.backend().with(|s| s.unpaired = true);
        let report = block_on(e.tick(now() + secs(5))).unwrap();
        assert!(!e.status().paired);
        assert!(e.status().online);
        assert!(!report.data_changed);
    }

    #[test]
    fn clock_skew_over_two_minutes_is_flagged() {
        let mut e = engine();
        block_on(e.tick(now() + secs(120))).unwrap();
        assert!(!e.status().clock_skew);
        block_on(e.tick(now() + secs(121))).unwrap();
        assert!(e.status().clock_skew);
        block_on(e.tick(now() - secs(200))).unwrap();
        assert!(e.status().clock_skew);
    }
}
