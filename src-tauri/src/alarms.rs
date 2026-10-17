//! Windows alarms (ARCHITECTURE §9, SPEC §7): the scheduler loop, the alarm
//! window and the sound, and the «Λείπει σήμανση» reminder window after Stop
//! (SPEC §7.6).
//!
//! Every decision comes from [`AlarmScheduler`] and [`ReminderScheduler`] in
//! `clockin-core`; this loop only feeds them the time and the latest local
//! data, and makes the windows and the sounds match what they return. It re-evaluates at least every 30 s
//! (robust to sleep/resume and clock changes), at each alarm minute, and at
//! once when data changes, a mark is made or Stop is pressed.

use crate::audio::Player;
use crate::i18n::text;
use crate::state::AppState;
use crate::views::LocalStamp;
use clockin_core::{
    AlarmScheduler, MarkKind, ReminderName, ReminderScheduler, ReminderState, RingPhase,
    RingingAlarm, Snapshot, Sound, UnmarkedReminder, reminder_delay,
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;
use tauri::{
    AppHandle, Manager, UserAttentionType, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

/// The alarm window's label.
pub const WINDOW: &str = "alarm";
/// The «Λείπει σήμανση» reminder window's label.
pub const REMINDER_WINDOW: &str = "reminder";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the alarm window shows. Times are Greek wall-clock "HH:MM".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmView {
    /// Pass back to `alarm_stop`.
    pub id: u64,
    /// The alarm's minute when every name shares it. `None` when names
    /// joined from different minutes: each name then shows its own time.
    pub at: Option<String>,
    pub check_in: Vec<AlarmName>,
    pub check_out: Vec<AlarmName>,
    /// False during the silent part of the cycle.
    pub ringing: bool,
    /// When a silent alarm rings again.
    pub rering_at: Option<String>,
}

/// One name on the alarm window, with its own alarm time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AlarmName {
    pub name: String,
    pub at: String,
}

impl AlarmView {
    fn new(alarm: &RingingAlarm) -> Self {
        let names = |items: &[clockin_core::PlanItem]| -> Vec<AlarmName> {
            items
                .iter()
                .map(|i| AlarmName {
                    name: i.display_name.clone(),
                    at: LocalStamp::at(i.fires_at).time,
                })
                .collect()
        };
        let event = &alarm.event;
        let one_minute = event
            .check_in
            .iter()
            .chain(&event.check_out)
            .all(|i| i.fires_at == event.at);
        let (ringing, rering_at) = match alarm.phase {
            RingPhase::Ringing { .. } => (true, None),
            RingPhase::Silent { rering_at, .. } => (false, Some(LocalStamp::at(rering_at).time)),
        };
        Self {
            id: alarm.id,
            at: one_minute.then(|| LocalStamp::at(event.at).time),
            check_in: names(&event.check_in),
            check_out: names(&event.check_out),
            ringing,
            rering_at,
        }
    }
}

/// What the reminder window shows (SPEC §7.6). Times are Greek "HH:MM".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderView {
    /// Pass back to `reminder_ok` («Εντάξει»).
    pub group_ids: Vec<u64>,
    pub check_in: Vec<ReminderLine>,
    pub check_out: Vec<ReminderLine>,
    /// When it reminds again; `None` for the last one of the business day.
    pub next_at: Option<String>,
}

/// One name on the reminder, with what «Σήμανση» marks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderLine {
    pub name: String,
    /// The alarm time.
    pub at: String,
    pub staff_id: uuid::Uuid,
    pub source_block_id: uuid::Uuid,
    /// ISO business date.
    pub business_date: String,
    pub kind: MarkKind,
    /// A departure whose arrival was never marked either.
    pub no_arrival_mark: bool,
}

impl ReminderView {
    fn new(reminder: &UnmarkedReminder) -> Self {
        let lines = |names: &[ReminderName]| -> Vec<ReminderLine> {
            names
                .iter()
                .map(|n| ReminderLine {
                    name: n.item.display_name.clone(),
                    at: LocalStamp::at(n.item.fires_at).time,
                    staff_id: n.item.staff_id,
                    source_block_id: n.item.source_block_id,
                    business_date: n.item.business_date.to_string(),
                    kind: n.item.kind,
                    no_arrival_mark: n.no_arrival_mark,
                })
                .collect()
        };
        Self {
            group_ids: reminder.group_ids.clone(),
            check_in: lines(&reminder.check_in),
            check_out: lines(&reminder.check_out),
            next_at: reminder.next_at.map(|t| LocalStamp::at(t).time),
        }
    }
}

/// The sound and the window for one scheduler decision. Both always come
/// from the same decision, so the sound never plays without the window and
/// Stop (or nobody left) ends both together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Output {
    sound: Option<Sound>,
    window: bool,
}

impl Output {
    fn of(alarm: Option<&RingingAlarm>) -> Self {
        Self {
            sound: alarm.and_then(RingingAlarm::sound),
            window: alarm.is_some(),
        }
    }
}

pub struct Alarms {
    scheduler: Mutex<AlarmScheduler>,
    view: RwLock<Option<AlarmView>>,
    reminders: Mutex<ReminderScheduler>,
    reminder_view: RwLock<Option<ReminderView>>,
    player: Player,
    /// The webview data folder of a debug profile (all windows share it).
    webview_dir: Option<PathBuf>,
}

impl Alarms {
    /// Starts from the alarms this device already handled.
    #[must_use]
    pub fn new(state: &AppState, webview_dir: Option<PathBuf>) -> Self {
        Self {
            scheduler: Mutex::new(AlarmScheduler::with_handled(state.handled_alarms())),
            view: RwLock::new(None),
            reminders: Mutex::new(ReminderScheduler::with_groups(state.reminder_groups())),
            reminder_view: RwLock::new(None),
            player: Player::start(),
            webview_dir,
        }
    }

    /// What the alarm window should show now.
    #[must_use]
    pub fn view(&self) -> Option<AlarmView> {
        self.view
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What the reminder window should show now.
    #[must_use]
    pub fn reminder_view(&self) -> Option<ReminderView> {
        self.reminder_view
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Stop pressed on this device (SPEC §7.3). It marks nobody, and starts
    /// the «Λείπει σήμανση» reminder for the alarm's names (SPEC §7.6).
    pub fn stop(&self, state: &AppState, id: u64) {
        let mut scheduler = lock(&self.scheduler);
        if let Some(keys) = scheduler.stop(id) {
            state.save_handled_alarms(&scheduler.handled());
            drop(scheduler);
            let mut reminders = lock(&self.reminders);
            let delay = reminder_delay(state.reminder_min());
            if reminders.start(keys, state.clock.now(), delay).is_some() {
                state.save_reminder_groups(reminders.groups());
            }
        }
        state.alarm_wake.notify_one();
    }

    /// «Εντάξει» on the reminder: its reminders end on this device only. It
    /// marks nobody.
    pub fn reminder_ok(&self, state: &AppState, group_ids: &[u64]) {
        let mut reminders = lock(&self.reminders);
        if reminders.acknowledge(group_ids) {
            state.save_reminder_groups(reminders.groups());
        }
        drop(reminders);
        state.alarm_wake.notify_one();
    }

    /// Makes the reminder window match `state`; the C plays when a reminder
    /// comes due (never while the alarm window shows: the core holds it).
    fn apply_reminder(&self, app: &AppHandle, state: &ReminderState) {
        *self
            .reminder_view
            .write()
            .unwrap_or_else(PoisonError::into_inner) =
            state.reminder.as_ref().map(ReminderView::new);
        if state.reminder.is_none() {
            if let Some(window) = app.get_webview_window(REMINDER_WINDOW) {
                let _ = window.destroy();
            }
            return;
        }
        let window = app.get_webview_window(REMINDER_WINDOW).or_else(|| {
            self.open(
                app,
                REMINDER_WINDOW,
                &text("reminder.windowTitle"),
                (640.0, 560.0),
            )
        });
        if state.chime {
            self.player.chime();
            #[cfg(windows)]
            crate::power::wake_display();
            if let Some(window) = window {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_always_on_top(true);
                let _ = window.set_focus();
                let _ = window.request_user_attention(Some(UserAttentionType::Critical));
            }
        }
    }

    /// Makes the window and the sound match `alarm`. `rang` is the
    /// (alarm, ring) last brought to the front, so each new ring turns the
    /// screen on and raises the window once.
    fn apply(&self, app: &AppHandle, alarm: Option<&RingingAlarm>, rang: &mut Option<(u64, u32)>) {
        let output = Output::of(alarm);
        self.player.set(output.sound);
        *self.view.write().unwrap_or_else(PoisonError::into_inner) = alarm.map(AlarmView::new);
        let (true, Some(alarm)) = (output.window, alarm) else {
            *rang = None;
            if let Some(window) = app.get_webview_window(WINDOW) {
                let _ = window.destroy();
            }
            return;
        };
        let window = app
            .get_webview_window(WINDOW)
            .or_else(|| self.open(app, WINDOW, &text("alarm.windowTitle"), (760.0, 600.0)));
        if let RingPhase::Ringing { cycle, .. } = alarm.phase
            && *rang != Some((alarm.id, cycle))
        {
            *rang = Some((alarm.id, cycle));
            #[cfg(windows)]
            crate::power::wake_display();
            // The Today list comes up behind the alarm (also when the app
            // started hidden in the tray), so marking is one tap after Stop.
            #[cfg(desktop)]
            if let Some(main) = app.get_webview_window(crate::tray::MAIN) {
                let _ = main.show();
                let _ = main.unminimize();
            }
            if let Some(window) = window {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_always_on_top(true);
                let _ = window.set_focus();
                let _ = window.request_user_attention(Some(UserAttentionType::Critical));
            }
        }
    }

    /// Opens an alarm-like window: always on top, no close, minimise or
    /// maximise button.
    fn open(
        &self,
        app: &AppHandle,
        label: &str,
        title: &str,
        (width, height): (f64, f64),
    ) -> Option<WebviewWindow> {
        let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::default())
            .title(title)
            .inner_size(width, height)
            .min_inner_size(480.0, 420.0)
            .center()
            .always_on_top(true)
            .closable(false)
            .minimizable(false)
            .maximizable(false)
            .focused(true);
        if let Some(dir) = &self.webview_dir {
            builder = builder.data_directory(dir.clone());
        }
        match builder.build() {
            Ok(window) => {
                #[cfg(windows)]
                {
                    if let (Ok(hwnd), Ok(scale)) = (window.hwnd(), window.scale_factor()) {
                        crate::window_icon::apply(hwnd.0, scale);
                    }
                    // The scheduler runs on another thread; a subclass must be
                    // installed by the thread that owns the window.
                    let guarded = window.clone();
                    let _ = window.run_on_main_thread(move || {
                        if let Ok(hwnd) = guarded.hwnd() {
                            crate::keyboard_guard::install(hwnd.0);
                        }
                    });
                }
                Some(window)
            }
            Err(e) => {
                // The sound still plays; the next evaluation tries again.
                eprintln!("clock-in: could not open the {label} window: {e}");
                None
            }
        }
    }
}

/// Starts the scheduler loop. `manage_autostart`: keep the "Start with
/// Windows" entry in line with the synced setting (release, default profile).
pub fn start(app: AppHandle, state: Arc<AppState>, alarms: Arc<Alarms>, manage_autostart: bool) {
    tauri::async_runtime::spawn(run(app, state, alarms, manage_autostart));
}

async fn run(app: AppHandle, state: Arc<AppState>, alarms: Arc<Alarms>, manage_autostart: bool) {
    let mut autostart: Option<bool> = None;
    let mut rang = None;
    let mut last = Snapshot::default();
    loop {
        if manage_autostart {
            let wanted = state.autostart_wanted();
            if autostart != Some(wanted) {
                crate::autostart::apply(wanted);
                autostart = Some(wanted);
            }
        }

        refresh(&state, &mut last);
        let delay = reminder_delay(state.reminder_min());
        let now = state.clock.now();
        let about_to_ring = lock(&alarms.scheduler).has_new_due(now, &last);
        // A dry run on a copy: would a reminder come due now?
        let about_to_remind = lock(&alarms.reminders)
            .clone()
            .evaluate(now, &last, delay, false)
            .chime;
        if about_to_ring || about_to_remind {
            // Get marks made elsewhere a moment ago first.
            state.refresh_for_alarm().await;
            refresh(&state, &mut last);
        }
        let now = state.clock.now();
        let decision = {
            let mut scheduler = lock(&alarms.scheduler);
            let decision = scheduler.evaluate(now, &last);
            if decision.handled_changed {
                state.save_handled_alarms(&scheduler.handled());
            }
            decision
        };
        let reminder = {
            let mut reminders = lock(&alarms.reminders);
            let held = decision.alarm.is_some();
            let reminder = reminders.evaluate(now, &last, delay, held);
            if reminder.changed {
                state.save_reminder_groups(reminders.groups());
            }
            reminder
        };
        alarms.apply(&app, decision.alarm.as_ref(), &mut rang);
        alarms.apply_reminder(&app, &reminder);

        let wake_at = decision.wake_at.min(reminder.wake_at);
        let wait =
            Duration::try_from(wake_at.duration_since(state.clock.now())).unwrap_or(Duration::ZERO);
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = state.alarm_wake.notified() => {}
        }
    }
}

/// The latest local data. An unpaired device has none (its alarms end); if
/// the local database can't be read, the previous data stays (fail loud).
fn refresh(state: &AppState, last: &mut Snapshot) {
    match state.core_snapshot() {
        Ok(Some(snapshot)) => *last = snapshot,
        Ok(None) => *last = Snapshot::default(),
        Err(e) => eprintln!("clock-in: alarms use the previous data: {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clockin_core::{AlarmEvent, MarkKind, PlanItem};
    use jiff::Timestamp;
    use uuid::Uuid;

    fn item(name: &str, kind: MarkKind, at: Timestamp) -> PlanItem {
        PlanItem {
            item_id: name.into(),
            fires_at: at,
            kind,
            staff_id: Uuid::nil(),
            display_name: name.into(),
            business_date: jiff::civil::date(2026, 6, 1),
            source_block_id: Uuid::nil(),
        }
    }

    #[test]
    fn the_view_shows_greek_times_and_names() {
        // 06:00 UTC = 09:00 in Greece (summer time).
        let at: Timestamp = "2026-06-01T06:00:00Z".parse().unwrap();
        let event = AlarmEvent {
            at,
            check_in: vec![item("Anna Alpha", MarkKind::In, at)],
            check_out: vec![item("Babis Beta", MarkKind::Out, at)],
        };
        let silent = RingingAlarm {
            id: 7,
            event,
            phase: RingPhase::Silent {
                cycle: 0,
                rering_at: "2026-06-01T06:10:00Z".parse().unwrap(),
            },
        };
        let view = AlarmView::new(&silent);
        assert_eq!(view.id, 7);
        assert_eq!(view.at.as_deref(), Some("09:00"));
        let name = |name: &str, at: &str| AlarmName {
            name: name.into(),
            at: at.into(),
        };
        assert_eq!(view.check_in, [name("Anna Alpha", "09:00")]);
        assert_eq!(view.check_out, [name("Babis Beta", "09:00")]);
        assert!(!view.ringing);
        assert_eq!(view.rering_at.as_deref(), Some("09:10"));
    }

    #[test]
    fn the_reminder_view_shows_greek_times_and_what_to_mark() {
        let at: Timestamp = "2026-06-01T06:00:00Z".parse().unwrap();
        let mut out = item("Babis Beta", MarkKind::Out, at);
        out.staff_id = Uuid::from_u128(2);
        out.source_block_id = Uuid::from_u128(20);
        let reminder = UnmarkedReminder {
            group_ids: vec![3, 4],
            check_in: vec![ReminderName {
                item: item("Anna Alpha", MarkKind::In, at),
                no_arrival_mark: false,
            }],
            check_out: vec![ReminderName {
                item: out,
                no_arrival_mark: true,
            }],
            next_at: Some("2026-06-01T06:20:00Z".parse().unwrap()),
        };
        let view = ReminderView::new(&reminder);
        assert_eq!(view.group_ids, [3, 4]);
        assert_eq!(view.check_in[0].name, "Anna Alpha");
        assert_eq!(view.check_in[0].at, "09:00");
        assert_eq!(view.check_in[0].kind, MarkKind::In);
        let line = &view.check_out[0];
        assert_eq!(line.staff_id, Uuid::from_u128(2));
        assert_eq!(line.source_block_id, Uuid::from_u128(20));
        assert_eq!(line.business_date, "2026-06-01");
        assert!(line.no_arrival_mark);
        assert_eq!(view.next_at.as_deref(), Some("09:20"));
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["checkOut"][0]["noArrivalMark"], true);
        assert_eq!(json["checkOut"][0]["kind"], "out");
        assert_eq!(json["groupIds"], serde_json::json!([3, 4]));
        let last = ReminderView::new(&UnmarkedReminder {
            next_at: None,
            ..reminder
        });
        assert_eq!(last.next_at, None);
    }

    /// Anna 09:00–17:00 and Babis 09:02–17:00 on Monday 1 June 2026 (Greek
    /// summer time: 09:00 = 06:00 UTC).
    fn overlapping() -> Snapshot {
        let person = |n: u128, first: &str| clockin_core::Staff {
            id: Uuid::from_u128(n),
            first_name: first.into(),
            last_name: "Test".into(),
            removed_at: None,
        };
        let block = |n: u128, staff: u128, h: i8, m: i8| clockin_core::WeeklyBlock {
            id: Uuid::from_u128(n),
            staff_id: Uuid::from_u128(staff),
            weekday: jiff::civil::Weekday::Monday,
            start: jiff::civil::time(h, m, 0, 0),
            end: jiff::civil::time(17, 0, 0, 0),
        };
        Snapshot {
            staff: vec![person(1, "Anna"), person(2, "Babis")],
            blocks: vec![block(10, 1, 9, 0), block(20, 2, 9, 2)],
            ..Snapshot::default()
        }
    }

    fn utc(hm: &str) -> Timestamp {
        format!("2026-06-01T{hm}:00Z").parse().unwrap()
    }

    /// Evaluates at `hm` (UTC) and checks the rule for every decision: no
    /// sound without the window.
    fn output(s: &mut AlarmScheduler, hm: &str, snap: &Snapshot) -> (Option<RingingAlarm>, Output) {
        let alarm = s.evaluate(utc(hm), snap).alarm;
        let out = Output::of(alarm.as_ref());
        assert!(
            out.sound.is_none() || out.window,
            "sound without the window at {hm}"
        );
        (alarm, out)
    }

    #[test]
    fn stop_always_ends_the_sound_and_the_window_together() {
        let ringing_in = Output {
            sound: Some(Sound::CheckIn),
            window: true,
        };
        let off = Output {
            sound: None,
            window: false,
        };
        let mut snap = overlapping();
        let mut s = AlarmScheduler::default();

        // 09:00 Anna rings; at 09:02 Babis joins the same alarm.
        let (first, out) = output(&mut s, "06:00", &snap);
        assert_eq!(out, ringing_in);
        let (joined, out) = output(&mut s, "06:02", &snap);
        assert_eq!(out, ringing_in);
        let (first, joined) = (first.unwrap(), joined.unwrap());
        assert_eq!(joined.event.check_in.len(), 2);

        // A Stop from a window that still showed the 09:00 version is
        // ignored: sound and window both stay.
        assert!(s.stop(first.id).is_none());
        assert_eq!(output(&mut s, "06:02", &snap).1, ringing_in);

        // Stop on the current version: both end at once, for good.
        assert!(s.stop(joined.id).is_some());
        assert_eq!(output(&mut s, "06:02", &snap).1, off);

        // Then Anna is marked: still nothing, also at the re-ring time.
        snap.marks.push(clockin_core::Mark {
            id: Uuid::from_u128(100),
            staff_id: Uuid::from_u128(1),
            source_block_id: Uuid::from_u128(10),
            business_date: jiff::civil::date(2026, 6, 1),
            kind: MarkKind::In,
        });
        for hm in ["06:03", "06:12", "06:17"] {
            assert_eq!(output(&mut s, hm, &snap).1, off, "{hm}");
        }
    }

    #[test]
    fn names_that_joined_later_show_their_own_time() {
        let snap = overlapping();
        let mut s = AlarmScheduler::default();
        // 09:00: Anna alone, the time in the header.
        let (alarm, _) = output(&mut s, "06:00", &snap);
        let view = AlarmView::new(&alarm.unwrap());
        assert_eq!(view.at.as_deref(), Some("09:00"));
        // 09:02: Babis joins; each name keeps its own time, none in the header.
        let (alarm, _) = output(&mut s, "06:02", &snap);
        let view = AlarmView::new(&alarm.unwrap());
        assert_eq!(view.at, None);
        let shown: Vec<(&str, &str)> = view
            .check_in
            .iter()
            .map(|n| (n.name.as_str(), n.at.as_str()))
            .collect();
        assert_eq!(shown, [("Anna Test", "09:00"), ("Babis Test", "09:02")]);
    }

    #[test]
    fn stop_in_the_silent_gap_closes_the_window_too() {
        let snap = overlapping();
        let mut s = AlarmScheduler::default();
        output(&mut s, "06:00", &snap);
        output(&mut s, "06:02", &snap);
        // 09:07: Babis's ring (from 09:02) has gone silent; the window stays.
        let (alarm, out) = output(&mut s, "06:07", &snap);
        assert_eq!(
            out,
            Output {
                sound: None,
                window: true
            }
        );
        assert!(s.stop(alarm.unwrap().id).is_some());
        assert_eq!(
            output(&mut s, "06:07", &snap).1,
            Output {
                sound: None,
                window: false
            }
        );
        assert!(!output(&mut s, "06:12", &snap).1.window, "no re-ring");
    }
}
