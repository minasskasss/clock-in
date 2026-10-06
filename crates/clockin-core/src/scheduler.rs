//! The Windows alarm scheduler's decisions (ARCHITECTURE §9, SPEC §7).
//!
//! [`AlarmScheduler`] is a pure state machine. The platform layer calls
//! [`AlarmScheduler::evaluate`] with the current time and the latest local
//! snapshot (server data plus marks still queued on this device), then makes
//! the device match the returned [`AlarmState`]: show or hide the alarm
//! window, play or stop the sound, and sleep until `wake_at` (or less, when
//! the data changes or Stop is pressed).
//!
//! Rules:
//!
//! - An alarm fires when it is due, or up to 15 minutes late (a device that
//!   was asleep or off); older ones are skipped (SPEC §7.5).
//! - Alarms of the same minute ring as one event (SPEC §7.1). An alarm that
//!   becomes due while another is still showing joins it, and the ring
//!   starts again so the new names are heard.
//! - The cycle rings 5 minutes, stays silent 5 minutes, and repeats until
//!   Stop (SPEC §7.3, [`ring_cycle_state`]).
//! - Every evaluation drops names that are marked, or whose alarm left the
//!   plan (block moved, removed or a day off; [`crate::is_still_due`]). When
//!   no names are left, the alarm ends by itself.
//! - An alarm that rang is remembered by `(item_id, fires_at)` once it is
//!   stopped or ends, so it never rings twice; a block moved to a later time
//!   is a new alarm and still rings (DECISIONS, Phase 1).

use crate::events::{AlarmEvent, due_events, firing_window, is_suppressed, next_event};
use crate::model::{MarkKind, Snapshot};
use crate::plan::{PlanItem, alarm_plan};
use crate::ring::{RingPhase, ring_cycle_state};
use crate::shop_time::floor_to_minute;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The scheduler re-evaluates at least this often, so sleep/resume and clock
/// changes are noticed (ARCHITECTURE §9).
pub const MAX_SLEEP: SignedDuration = SignedDuration::from_secs(30);

/// How far ahead each evaluation looks for the next alarm.
const LOOKAHEAD: SignedDuration = SignedDuration::from_hours(24);

/// Remembered alarms older than this are forgotten; they can no longer fire
/// anyway (the firing window is 15 minutes).
const FORGET_AFTER: SignedDuration = SignedDuration::from_hours(24);

/// One alarm for one person at one time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AlarmKey {
    pub item_id: String,
    pub fires_at: Timestamp,
}

impl AlarmKey {
    #[must_use]
    pub fn of(item: &PlanItem) -> Self {
        Self {
            item_id: item.item_id.clone(),
            fires_at: item.fires_at,
        }
    }
}

/// The alarm that is showing on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Active {
    /// Changes whenever the ring restarts for new names, so a Stop pressed
    /// for an older version of the alarm doesn't silence names never heard.
    id: u64,
    /// Every alarm this cycle has rung for, including names dropped since.
    keys: BTreeSet<AlarmKey>,
    /// When the current ring cycle started.
    started_at: Timestamp,
}

/// What the device should be doing now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmState {
    /// The alarm to show, if any.
    pub alarm: Option<RingingAlarm>,
    /// Evaluate again no later than this.
    pub wake_at: Timestamp,
    /// The remembered alarms changed: save [`AlarmScheduler::handled`].
    pub handled_changed: bool,
}

/// The alarm window's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RingingAlarm {
    /// Pass back to [`AlarmScheduler::stop`].
    pub id: u64,
    /// The names still due. `event.at` is the earliest of their minutes.
    pub event: AlarmEvent,
    pub phase: RingPhase,
}

impl RingingAlarm {
    /// The sound to play now: none during the silent part of the cycle.
    #[must_use]
    pub fn sound(&self) -> Option<crate::Sound> {
        match self.phase {
            RingPhase::Ringing { .. } => Some(self.event.sound()),
            RingPhase::Silent { .. } => None,
        }
    }
}

/// See the module docs.
#[derive(Debug, Clone, Default)]
pub struct AlarmScheduler {
    handled: BTreeSet<AlarmKey>,
    active: Option<Active>,
    next_id: u64,
}

impl AlarmScheduler {
    /// Starts with the alarms this device already handled (saved across
    /// restarts), so a stopped alarm doesn't ring again after a restart.
    #[must_use]
    pub fn with_handled(handled: impl IntoIterator<Item = AlarmKey>) -> Self {
        Self {
            handled: handled.into_iter().collect(),
            ..Self::default()
        }
    }

    /// The alarms that rang and were stopped or ended, for saving.
    #[must_use]
    pub fn handled(&self) -> Vec<AlarmKey> {
        self.handled.iter().cloned().collect()
    }

    /// Whether [`AlarmScheduler::evaluate`] would start or extend an alarm
    /// now. The platform layer uses it to refresh its data first, so the
    /// decision uses marks at most a few seconds old.
    #[must_use]
    pub fn has_new_due(&self, now: Timestamp, snapshot: &Snapshot) -> bool {
        let plan = self.plan(now, snapshot);
        !self.newly_due(now, &plan, snapshot).is_empty()
    }

    /// Updates the alarm for `now` and `snapshot`, and says what the device
    /// should be doing.
    pub fn evaluate(&mut self, now: Timestamp, snapshot: &Snapshot) -> AlarmState {
        let mut handled_changed = self.forget_old(now);
        let plan = self.plan(now, snapshot);

        // New alarms join the one showing, or start one.
        let new = self.newly_due(now, &plan, snapshot);
        if !new.is_empty() {
            self.next_id += 1;
            let id = self.next_id;
            let active = self.active.get_or_insert_with(|| Active {
                id,
                keys: BTreeSet::new(),
                started_at: now,
            });
            active.id = id;
            active.started_at = now;
            active.keys.extend(new);
        }

        // Drop names marked or gone from the plan; end when none are left.
        let mut alarm = None;
        if let Some(active) = &self.active {
            match current_event(&active.keys, &plan, snapshot) {
                Some(event) => {
                    alarm = Some(RingingAlarm {
                        id: active.id,
                        event,
                        phase: ring_cycle_state(active.started_at, now),
                    });
                }
                None => {
                    handled_changed |= self.end();
                }
            }
        }

        let mut wake_at = now.saturating_add(MAX_SLEEP).unwrap_or(now);
        if let Some(next) = next_event(&plan, &snapshot.marks, now) {
            wake_at = wake_at.min(next.at);
        }
        if let Some(alarm) = &alarm {
            let change = match alarm.phase {
                RingPhase::Ringing { silent_at, .. } => silent_at,
                RingPhase::Silent { rering_at, .. } => rering_at,
            };
            wake_at = wake_at.min(change);
        }

        AlarmState {
            alarm,
            wake_at,
            handled_changed,
        }
    }

    /// Stop was pressed on this device (SPEC §7.3): the alarm `id` ends here
    /// and its alarms don't ring again. Returns whether anything changed (a
    /// Stop for an alarm that was replaced meanwhile does nothing).
    pub fn stop(&mut self, id: u64) -> bool {
        match &self.active {
            Some(active) if active.id == id => self.end(),
            _ => false,
        }
    }

    fn end(&mut self) -> bool {
        match self.active.take() {
            Some(active) => {
                self.handled.extend(active.keys);
                true
            }
            None => false,
        }
    }

    fn forget_old(&mut self, now: Timestamp) -> bool {
        let Ok(cutoff) = now.checked_sub(FORGET_AFTER) else {
            return false;
        };
        let before = self.handled.len();
        self.handled.retain(|k| k.fires_at >= cutoff);
        self.handled.len() != before
    }

    /// The plan from the earliest alarm that can still matter (the firing
    /// window, or the oldest alarm showing) to a day ahead.
    fn plan(&self, now: Timestamp, snapshot: &Snapshot) -> Vec<PlanItem> {
        let (mut from, _) = firing_window(now);
        if let Some(oldest) = self
            .active
            .as_ref()
            .and_then(|a| a.keys.iter().map(|k| k.fires_at).min())
        {
            from = from.min(oldest);
        }
        let until = now.saturating_add(LOOKAHEAD).unwrap_or(now);
        alarm_plan(snapshot, from, until)
    }

    /// Alarms due now (or up to 15 minutes late) that haven't rung here.
    fn newly_due(&self, now: Timestamp, plan: &[PlanItem], snapshot: &Snapshot) -> Vec<AlarmKey> {
        let (from, to) = firing_window(now);
        due_events(plan, &snapshot.marks, from, to)
            .iter()
            .flat_map(|e| e.check_in.iter().chain(&e.check_out))
            .map(AlarmKey::of)
            .filter(|k| {
                !self.handled.contains(k)
                    && self.active.as_ref().is_none_or(|a| !a.keys.contains(k))
            })
            .collect()
    }
}

/// The names of `keys` that are still due, in plan order: still in the plan
/// with the same time, and not suppressed by a mark.
fn current_event(
    keys: &BTreeSet<AlarmKey>,
    plan: &[PlanItem],
    snapshot: &Snapshot,
) -> Option<AlarmEvent> {
    let due: Vec<&PlanItem> = plan
        .iter()
        .filter(|i| keys.contains(&AlarmKey::of(i)) && !is_suppressed(i, &snapshot.marks))
        .collect();
    let at = floor_to_minute(due.iter().map(|i| i.fires_at).min()?);
    let pick = |kind: MarkKind| -> Vec<PlanItem> {
        due.iter()
            .filter(|i| i.kind == kind)
            .map(|&i| i.clone())
            .collect()
    };
    Some(AlarmEvent {
        at,
        check_in: pick(MarkKind::In),
        check_out: pick(MarkKind::Out),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sound;
    use crate::model::ScheduleSettings;
    use crate::test_support::*;
    use jiff::civil::{Date, Weekday, date, time};

    /// Anna 09:00–17:00, Babis 12:00–16:00, Christos 08:00–12:00,
    /// Dimitra 09:00–12:00 on Mondays.
    fn snapshot() -> Snapshot {
        let a = staff(1, "Anna", "Alpha");
        let b = staff(2, "Babis", "Beta");
        let c = staff(3, "Christos", "Gamma");
        let d = staff(4, "Dimitra", "Delta");
        Snapshot {
            blocks: vec![
                weekly(10, &a, Weekday::Monday, (9, 0), (17, 0)),
                weekly(20, &b, Weekday::Monday, (12, 0), (16, 0)),
                weekly(30, &c, Weekday::Monday, (8, 0), (12, 0)),
                weekly(40, &d, Weekday::Monday, (9, 0), (12, 0)),
            ],
            staff: vec![a, b, c, d],
            ..Snapshot::default()
        }
    }

    fn at(h: i8, m: i8) -> Timestamp {
        athens(MONDAY, h, m)
    }

    fn secs(t: Timestamp, s: i64) -> Timestamp {
        t + SignedDuration::from_secs(s)
    }

    fn mins(t: Timestamp, m: i64) -> Timestamp {
        t + SignedDuration::from_mins(m)
    }

    fn display_names(items: &[PlanItem]) -> Vec<&str> {
        items.iter().map(|i| i.display_name.as_str()).collect()
    }

    fn names(alarm: &RingingAlarm) -> (Vec<&str>, Vec<&str>) {
        (
            display_names(&alarm.event.check_in),
            display_names(&alarm.event.check_out),
        )
    }

    fn ringing(state: &AlarmState) -> bool {
        state.alarm.as_ref().is_some_and(|a| a.sound().is_some())
    }

    fn mark_in(snap: &mut Snapshot, n: u128, who: u128, block: u128) {
        snap.marks.push(mark(n, who, block, MONDAY, MarkKind::In));
    }

    #[test]
    fn nothing_before_the_first_alarm_and_wakes_for_it() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let state = s.evaluate(at(7, 0), &snap);
        assert_eq!(state.alarm, None);
        assert_eq!(state.wake_at, secs(at(7, 0), 30));
        // Closer than 30 s: wake exactly at the alarm minute.
        let state = s.evaluate(secs(at(7, 59), 50), &snap);
        assert_eq!(state.alarm, None);
        assert_eq!(state.wake_at, at(8, 0));
    }

    #[test]
    fn rings_at_the_minute_with_the_right_sound() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let state = s.evaluate(at(8, 0), &snap);
        let alarm = state.alarm.unwrap();
        assert_eq!(names(&alarm), (vec!["Christos Gamma"], vec![]));
        assert_eq!(alarm.event.at, at(8, 0));
        assert_eq!(alarm.sound(), Some(Sound::CheckIn));
        assert_eq!(
            alarm.phase,
            RingPhase::Ringing {
                cycle: 0,
                silent_at: at(8, 5)
            }
        );
        // Wakes 30 s later (before the 5-minute mark).
        assert_eq!(state.wake_at, secs(at(8, 0), 30));
    }

    #[test]
    fn same_minute_alarms_ring_as_one_event() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let alarm = s.evaluate(at(9, 0), &snap).alarm.unwrap();
        assert_eq!(names(&alarm), (vec!["Anna Alpha", "Dimitra Delta"], vec![]));
    }

    #[test]
    fn check_in_and_check_out_together_play_the_check_in_sound() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        // Skip the morning alarms.
        let alarm = s.evaluate(at(12, 0), &snap).alarm.unwrap();
        assert_eq!(
            names(&alarm),
            (vec!["Babis Beta"], vec!["Christos Gamma", "Dimitra Delta"])
        );
        assert_eq!(alarm.sound(), Some(Sound::CheckIn));
    }

    #[test]
    fn check_out_only_plays_the_check_out_sound() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let alarm = s.evaluate(at(16, 0), &snap).alarm.unwrap();
        assert_eq!(names(&alarm), (vec![], vec!["Babis Beta"]));
        assert_eq!(alarm.sound(), Some(Sound::CheckOut));
    }

    #[test]
    fn rings_five_minutes_silent_five_then_rings_again_until_stop() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let start = at(8, 0);
        assert!(ringing(&s.evaluate(start, &snap)));
        assert!(ringing(&s.evaluate(secs(start, 299), &snap)));

        let silent = s.evaluate(mins(start, 5), &snap);
        let alarm = silent.alarm.as_ref().unwrap();
        assert_eq!(alarm.sound(), None, "silent after 5 minutes");
        assert_eq!(
            alarm.phase,
            RingPhase::Silent {
                cycle: 0,
                rering_at: at(8, 10)
            }
        );
        assert_eq!(silent.wake_at, secs(mins(start, 5), 30));
        let near = s.evaluate(secs(mins(start, 9), 45), &snap);
        assert_eq!(near.wake_at, at(8, 10));

        let again = s.evaluate(mins(start, 10), &snap);
        assert!(ringing(&again), "rings again at +10");
        assert!(matches!(
            again.alarm.as_ref().unwrap().phase,
            RingPhase::Ringing { cycle: 1, .. }
        ));
        // Hours later, still repeating.
        assert!(ringing(&s.evaluate(mins(start, 120), &snap)));

        let id = again.alarm.unwrap().id;
        assert!(s.stop(id));
        let after = s.evaluate(mins(start, 121), &snap);
        assert_eq!(after.alarm, None);
        assert!(!after.handled_changed);
        // Never again, even within the 15-minute window.
        assert_eq!(s.evaluate(mins(start, 1), &snap).alarm, None);
    }

    #[test]
    fn marking_during_the_silent_gap_ends_the_cycle() {
        let mut snap = snapshot();
        let mut s = AlarmScheduler::default();
        s.evaluate(at(8, 0), &snap);
        assert!(!ringing(&s.evaluate(at(8, 6), &snap)));
        mark_in(&mut snap, 100, 3, 30);
        let state = s.evaluate(at(8, 7), &snap);
        assert_eq!(state.alarm, None, "no re-ring");
        assert!(state.handled_changed);
        assert_eq!(s.evaluate(at(8, 10), &snap).alarm, None);
    }

    #[test]
    fn marked_names_drop_out_and_the_others_keep_ringing() {
        let mut snap = snapshot();
        let mut s = AlarmScheduler::default();
        s.evaluate(at(9, 0), &snap);
        mark_in(&mut snap, 100, 1, 10);
        let alarm = s.evaluate(at(9, 1), &snap).alarm.unwrap();
        assert_eq!(names(&alarm), (vec!["Dimitra Delta"], vec![]));
        assert!(alarm.sound().is_some());
        // A voided mark brings the name back while the alarm is showing.
        snap.marks.clear();
        let alarm = s.evaluate(at(9, 2), &snap).alarm.unwrap();
        assert_eq!(alarm.event.check_in.len(), 2);
    }

    #[test]
    fn a_mark_before_the_alarm_prevents_it() {
        let mut snap = snapshot();
        let mut s = AlarmScheduler::default();
        mark_in(&mut snap, 100, 3, 30);
        let state = s.evaluate(at(7, 58), &snap);
        assert_eq!(state.alarm, None);
        // Next wake is not 08:00 any more (nothing due then), but 30 s.
        assert_eq!(state.wake_at, secs(at(7, 58), 30));
        assert_eq!(s.evaluate(at(8, 0), &snap).alarm, None);
    }

    #[test]
    fn a_moved_block_rings_at_its_new_time_only() {
        let mut snap = snapshot();
        let mut s = AlarmScheduler::default();
        // Christos moves from 08:00 to 08:30 before 08:00 (same block id).
        for b in &mut snap.blocks {
            if b.id == id(30) {
                b.start = time(8, 30, 0, 0);
            }
        }
        assert_eq!(s.evaluate(at(8, 0), &snap).alarm, None);
        assert!(s.evaluate(at(8, 30), &snap).alarm.is_some());
    }

    #[test]
    fn a_block_moved_after_it_rang_rings_again_at_the_new_time() {
        let mut snap = snapshot();
        let mut s = AlarmScheduler::default();
        let id0 = s.evaluate(at(8, 0), &snap).alarm.unwrap().id;
        s.stop(id0);
        for b in &mut snap.blocks {
            if b.id == id(30) {
                b.start = time(8, 30, 0, 0);
            }
        }
        assert_eq!(s.evaluate(at(8, 1), &snap).alarm, None);
        assert!(s.evaluate(at(8, 30), &snap).alarm.is_some());
    }

    #[test]
    fn a_block_moved_during_the_silent_gap_ends_the_old_alarm() {
        let mut snap = snapshot();
        let mut s = AlarmScheduler::default();
        s.evaluate(at(8, 0), &snap);
        for b in &mut snap.blocks {
            if b.id == id(30) {
                b.start = time(10, 0, 0, 0);
            }
        }
        assert_eq!(s.evaluate(at(8, 6), &snap).alarm, None);
    }

    #[test]
    fn missed_alarms_fire_up_to_15_minutes_late() {
        let snap = snapshot();
        // Woke up 14 minutes late: rings now, showing the original minute.
        let mut s = AlarmScheduler::default();
        let late = s.evaluate(at(8, 14), &snap).alarm.unwrap();
        assert_eq!(late.event.at, at(8, 0));
        assert!(late.sound().is_some());

        let mut s = AlarmScheduler::default();
        assert!(s.evaluate(at(8, 15), &snap).alarm.is_some());

        // 15 minutes and 1 second: skipped.
        let mut s = AlarmScheduler::default();
        assert_eq!(s.evaluate(secs(at(8, 15), 1), &snap).alarm, None);
    }

    #[test]
    fn several_missed_events_ring_as_one_alarm() {
        // Christos 08:00 and Eleni 08:10; the PC wakes from sleep at 08:12.
        let mut snap = snapshot();
        let e = staff(5, "Eleni", "Epsilon");
        snap.blocks
            .push(weekly(50, &e, Weekday::Monday, (8, 10), (12, 0)));
        snap.staff.push(e);
        let mut s = AlarmScheduler::default();
        let alarm = s.evaluate(at(8, 12), &snap).alarm.unwrap();
        assert_eq!(
            names(&alarm),
            (vec!["Christos Gamma", "Eleni Epsilon"], vec![])
        );
        assert_eq!(alarm.event.at, at(8, 0));
        assert!(alarm.sound().is_some());
    }

    #[test]
    fn a_new_alarm_joins_the_one_showing_and_restarts_the_ring() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let first = s.evaluate(at(8, 0), &snap).alarm.unwrap();
        // Unanswered since 08:00: 08:55–09:00 is a silent gap.
        assert_eq!(s.evaluate(at(8, 57), &snap).alarm.unwrap().sound(), None);
        let joined = s.evaluate(at(9, 0), &snap).alarm.unwrap();
        assert_ne!(joined.id, first.id);
        assert_eq!(
            joined.phase,
            RingPhase::Ringing {
                cycle: 0,
                silent_at: at(9, 5)
            }
        );
        assert_eq!(joined.event.check_in.len(), 3);
        // A Stop meant for the old version is ignored.
        assert!(!s.stop(first.id));
        assert!(s.evaluate(at(9, 1), &snap).alarm.is_some());
        assert!(s.stop(joined.id));
        assert_eq!(s.evaluate(at(9, 2), &snap).alarm, None);
    }

    #[test]
    fn stop_needs_an_alarm() {
        let mut s = AlarmScheduler::default();
        assert!(!s.stop(1));
    }

    #[test]
    fn handled_alarms_survive_a_restart() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let alarm = s.evaluate(at(8, 0), &snap).alarm.unwrap();
        assert!(s.stop(alarm.id));
        let saved = s.handled();
        assert_eq!(saved.len(), 1);

        let mut restarted = AlarmScheduler::with_handled(saved);
        assert_eq!(restarted.evaluate(at(8, 3), &snap).alarm, None);
        // Without the saved list it would ring again (fail loud).
        assert!(
            AlarmScheduler::default()
                .evaluate(at(8, 3), &snap)
                .alarm
                .is_some()
        );
    }

    #[test]
    fn old_handled_alarms_are_forgotten() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let alarm = s.evaluate(at(8, 0), &snap).alarm.unwrap();
        s.stop(alarm.id);
        let next_day = athens(TUESDAY, 8, 0);
        let state = s.evaluate(secs(next_day, 1), &Snapshot::default());
        assert!(state.handled_changed);
        assert!(s.handled().is_empty());
    }

    #[test]
    fn has_new_due_does_not_change_anything() {
        let mut snap = snapshot();
        let mut s = AlarmScheduler::default();
        assert!(!s.has_new_due(at(7, 59), &snap));
        assert!(s.has_new_due(at(8, 0), &snap));
        mark_in(&mut snap, 100, 3, 30);
        assert!(!s.has_new_due(at(8, 0), &snap));
        snap.marks.clear();
        let alarm = s.evaluate(at(8, 0), &snap).alarm.unwrap();
        assert!(!s.has_new_due(at(8, 1), &snap), "already showing");
        s.stop(alarm.id);
        assert!(!s.has_new_due(at(8, 1), &snap), "already handled");
    }

    #[test]
    fn empty_snapshot_has_no_alarms() {
        let mut s = AlarmScheduler::default();
        let state = s.evaluate(at(8, 0), &Snapshot::default());
        assert_eq!(state.alarm, None);
        assert_eq!(state.wake_at, secs(at(8, 0), 30));
    }

    #[test]
    fn offsets_move_the_alarm() {
        let mut snap = snapshot();
        snap.settings = ScheduleSettings {
            checkin_offset_min: -10,
            ..ScheduleSettings::default()
        };
        let mut s = AlarmScheduler::default();
        assert_eq!(s.evaluate(at(7, 49), &snap).alarm, None);
        assert_eq!(s.evaluate(secs(at(7, 49), 50), &snap).wake_at, at(7, 50));
        assert!(s.evaluate(at(7, 50), &snap).alarm.is_some());
    }

    /// PLAN Phase 4 checklist item 11: a shift spanning the fall-back change
    /// (2026-10-25 04:00 → 03:00) rings at the right instants.
    #[test]
    fn dst_fall_back_night() {
        let saturday: Date = date(2026, 10, 24);
        let who = staff(1, "Anna", "Alpha");
        // 22:00–06:00 Saturday night (rollover 05:00 is fine: it starts on
        // Saturday's business day).
        let snap = Snapshot {
            blocks: vec![weekly(10, &who, Weekday::Saturday, (22, 0), (6, 0))],
            staff: vec![who],
            ..Snapshot::default()
        };
        let mut s = AlarmScheduler::default();
        let start = athens(saturday, 22, 0);
        let alarm = s.evaluate(start, &snap).alarm.unwrap();
        s.stop(alarm.id);
        // Shift end: 06:00 EET on Sunday = 04:00 UTC, nine real hours later.
        let end: Timestamp = "2026-10-25T04:00:00Z".parse().unwrap();
        assert_eq!(end.duration_since(start), SignedDuration::from_hours(9));
        assert_eq!(s.evaluate(secs(end, -1), &snap).alarm, None);
        assert_eq!(s.evaluate(secs(end, -10), &snap).wake_at, end);
        let out = s.evaluate(end, &snap).alarm.unwrap();
        assert_eq!(out.sound(), Some(Sound::CheckOut));
    }

    #[test]
    fn dst_fall_back_ambiguous_time_fires_at_the_first_occurrence() {
        // A block ending 03:30 on 2026-10-25: 03:30 happens twice; the first
        // (EEST, 00:30 UTC) rings, the second doesn't ring again.
        let who = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &who, Weekday::Saturday, (20, 0), (3, 30))],
            staff: vec![who],
            ..Snapshot::default()
        };
        let mut s = AlarmScheduler::default();
        let first: Timestamp = "2026-10-25T00:30:00Z".parse().unwrap();
        let alarm = s.evaluate(first, &snap).alarm.unwrap();
        assert_eq!(alarm.event.check_out.len(), 1);
        s.stop(alarm.id);
        let second: Timestamp = "2026-10-25T01:30:00Z".parse().unwrap();
        assert_eq!(s.evaluate(second, &snap).alarm, None);
    }

    #[test]
    fn dst_spring_forward_missing_time_fires_at_the_next_valid_minute() {
        // 2027-03-28: 03:00 → 04:00. A block starting 03:30 rings at 04:00.
        let sunday: Date = date(2027, 3, 28);
        let who = staff(1, "Anna", "Alpha");
        // Starts before the rollover: belongs to Saturday's business day.
        let snap = Snapshot {
            blocks: vec![weekly(10, &who, Weekday::Saturday, (3, 30), (8, 0))],
            staff: vec![who],
            ..Snapshot::default()
        };
        let mut s = AlarmScheduler::default();
        let four = athens(sunday, 4, 0);
        assert_eq!(s.evaluate(secs(four, -1), &snap).alarm, None);
        assert!(s.evaluate(four, &snap).alarm.is_some());
    }

    #[test]
    fn the_wake_time_is_always_in_the_future_and_at_most_30_seconds_away() {
        let snap = snapshot();
        let mut s = AlarmScheduler::default();
        let mut now = at(7, 50);
        let mut rang = 0;
        while now < at(17, 30) {
            let state = s.evaluate(now, &snap);
            assert!(state.wake_at > now, "{now}");
            assert!(state.wake_at.duration_since(now) <= MAX_SLEEP, "{now}");
            // Stop every alarm a minute after it shows.
            if let Some(alarm) = state.alarm
                && now.duration_since(alarm.event.at) >= SignedDuration::from_mins(1)
            {
                assert!(s.stop(alarm.id));
                rang += 1;
            }
            now = state.wake_at;
        }
        // 08:00, 09:00, 12:00, 16:00 and 17:00.
        assert_eq!(rang, 5);
    }
}
