//! Windows alarms (ARCHITECTURE §9, SPEC §7): the scheduler loop, the alarm
//! window and the sound.
//!
//! Every decision comes from [`AlarmScheduler`] in `clockin-core`; this loop
//! only feeds it the time and the latest local data, and makes the window
//! and the sound match what it returns. It re-evaluates at least every 30 s
//! (robust to sleep/resume and clock changes), at each alarm minute, and at
//! once when data changes, a mark is made or Stop is pressed.

use crate::audio::Player;
use crate::i18n::text;
use crate::state::AppState;
use crate::views::LocalStamp;
use clockin_core::{AlarmScheduler, RingPhase, RingingAlarm, Snapshot};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;
use tauri::{
    AppHandle, Manager, UserAttentionType, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

/// The alarm window's label.
pub const WINDOW: &str = "alarm";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the alarm window shows. Times are Greek wall-clock "HH:MM".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmView {
    /// Pass back to `alarm_stop`.
    pub id: u64,
    /// The alarm's minute (the earliest, if several joined).
    pub at: String,
    pub check_in: Vec<String>,
    pub check_out: Vec<String>,
    /// False during the silent part of the cycle.
    pub ringing: bool,
    /// When a silent alarm rings again.
    pub rering_at: Option<String>,
}

impl AlarmView {
    fn new(alarm: &RingingAlarm) -> Self {
        let names = |items: &[clockin_core::PlanItem]| -> Vec<String> {
            items.iter().map(|i| i.display_name.clone()).collect()
        };
        let (ringing, rering_at) = match alarm.phase {
            RingPhase::Ringing { .. } => (true, None),
            RingPhase::Silent { rering_at, .. } => (false, Some(LocalStamp::at(rering_at).time)),
        };
        Self {
            id: alarm.id,
            at: LocalStamp::at(alarm.event.at).time,
            check_in: names(&alarm.event.check_in),
            check_out: names(&alarm.event.check_out),
            ringing,
            rering_at,
        }
    }
}

pub struct Alarms {
    scheduler: Mutex<AlarmScheduler>,
    view: RwLock<Option<AlarmView>>,
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

    /// Stop pressed on this device (SPEC §7.3). It marks nobody.
    pub fn stop(&self, state: &AppState, id: u64) {
        let mut scheduler = lock(&self.scheduler);
        if scheduler.stop(id) {
            state.save_handled_alarms(&scheduler.handled());
        }
        drop(scheduler);
        state.alarm_wake.notify_one();
    }

    /// Makes the window and the sound match `alarm`. `rang` is the
    /// (alarm, ring) last brought to the front, so each new ring turns the
    /// screen on and raises the window once.
    fn apply(&self, app: &AppHandle, alarm: Option<&RingingAlarm>, rang: &mut Option<(u64, u32)>) {
        self.player.set(alarm.and_then(RingingAlarm::sound));
        *self.view.write().unwrap_or_else(PoisonError::into_inner) = alarm.map(AlarmView::new);
        let Some(alarm) = alarm else {
            *rang = None;
            if let Some(window) = app.get_webview_window(WINDOW) {
                let _ = window.destroy();
            }
            return;
        };
        let window = app
            .get_webview_window(WINDOW)
            .or_else(|| self.open_window(app));
        if let RingPhase::Ringing { cycle, .. } = alarm.phase
            && *rang != Some((alarm.id, cycle))
        {
            *rang = Some((alarm.id, cycle));
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

    fn open_window(&self, app: &AppHandle) -> Option<WebviewWindow> {
        let mut builder = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::default())
            .title(text("alarm.windowTitle"))
            .inner_size(760.0, 600.0)
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
                if let (Ok(hwnd), Ok(scale)) = (window.hwnd(), window.scale_factor()) {
                    crate::window_icon::apply(hwnd.0, scale);
                }
                Some(window)
            }
            Err(e) => {
                // The sound still plays; the next evaluation tries again.
                eprintln!("clock-in: could not open the alarm window: {e}");
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
        if lock(&alarms.scheduler).has_new_due(state.clock.now(), &last) {
            // About to ring: get marks made elsewhere a moment ago first.
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
        alarms.apply(&app, decision.alarm.as_ref(), &mut rang);

        let wait = Duration::try_from(decision.wake_at.duration_since(state.clock.now()))
            .unwrap_or(Duration::ZERO);
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
        assert_eq!(view.at, "09:00");
        assert_eq!(view.check_in, ["Anna Alpha"]);
        assert_eq!(view.check_out, ["Babis Beta"]);
        assert!(!view.ringing);
        assert_eq!(view.rering_at.as_deref(), Some("09:10"));
    }
}
