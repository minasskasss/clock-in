//! Alarm events: plan items grouped by minute, with suppression applied
//! (SPEC §7.1–§7.2).

use crate::model::{Mark, MarkKind};
use crate::plan::{MISSED_ALARM_GRACE, PlanItem};
use crate::shop_time::floor_to_minute;
use crate::today::mark_exists;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Which of the two generated sounds an event plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    CheckIn,
    CheckOut,
}

/// All alarms due in the same minute, as one event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlarmEvent {
    /// The minute the event is due.
    pub at: Timestamp,
    /// Names under "Check in", in plan order.
    pub check_in: Vec<PlanItem>,
    /// Names under "Check out", in plan order.
    pub check_out: Vec<PlanItem>,
}

impl AlarmEvent {
    /// Any check-in plays the check-in sound; otherwise the check-out sound.
    #[must_use]
    pub fn sound(&self) -> Sound {
        if self.check_in.is_empty() {
            Sound::CheckOut
        } else {
            Sound::CheckIn
        }
    }

    /// The plan ids of every alarm in the event.
    #[must_use]
    pub fn item_ids(&self) -> Vec<&str> {
        self.check_in
            .iter()
            .chain(&self.check_out)
            .map(|i| i.item_id.as_str())
            .collect()
    }

    /// The event recomputed before each re-ring (SPEC §7.3): only the alarms
    /// that are [`is_still_due`] against the current `plan` and `marks`.
    /// `None` when nobody is left: the cycle ends.
    #[must_use]
    pub fn still_due(&self, plan: &[PlanItem], marks: &[Mark]) -> Option<Self> {
        let keep = |items: &[PlanItem]| -> Vec<PlanItem> {
            items
                .iter()
                .filter(|i| is_still_due(i, plan, marks))
                .cloned()
                .collect()
        };
        let check_in = keep(&self.check_in);
        let check_out = keep(&self.check_out);
        if check_in.is_empty() && check_out.is_empty() {
            None
        } else {
            Some(Self {
                at: self.at,
                check_in,
                check_out,
            })
        }
    }
}

/// Whether an alarm a device is about to ring (or re-ring) should still ring:
///
/// - the current `plan` holds an item with the same `item_id` **and the same
///   `fires_at`** (ids don't include the time, so a block whose time was edited
///   keeps its id; the old time must not ring), and
/// - no mark suppresses it ([`is_suppressed`]).
///
/// This is the same rule the server's `check_alarm` applies (ARCHITECTURE §6).
#[must_use]
pub fn is_still_due(item: &PlanItem, plan: &[PlanItem], marks: &[Mark]) -> bool {
    plan.iter()
        .any(|p| p.item_id == item.item_id && p.fires_at == item.fires_at)
        && !is_suppressed(item, marks)
}

/// Whether a mark already makes this alarm unnecessary:
///
/// - a check-in alarm is suppressed by a check-in mark for that block;
/// - a check-out alarm is suppressed by a "left" mark for that block. It still
///   fires for someone who was never marked in.
///
/// Alarms removed by a schedule change are simply no longer in the plan.
#[must_use]
pub fn is_suppressed(item: &PlanItem, marks: &[Mark]) -> bool {
    mark_exists(
        marks,
        item.staff_id,
        item.source_block_id,
        item.business_date,
        item.kind,
    )
}

/// The window of alarms a device should fire right now: due at `now` or up to
/// 15 minutes late (SPEC §7.5). Anything older is skipped.
#[must_use]
pub fn firing_window(now: Timestamp) -> (Timestamp, Timestamp) {
    (now.saturating_sub(MISSED_ALARM_GRACE).unwrap_or(now), now)
}

/// The events with `from <= at <= to` (both inclusive), oldest first, each
/// holding only the alarms that are not suppressed by `marks`.
///
/// `plan` must be in plan order (as [`crate::alarm_plan`] returns it).
#[must_use]
pub fn due_events(
    plan: &[PlanItem],
    marks: &[Mark],
    from: Timestamp,
    to: Timestamp,
) -> Vec<AlarmEvent> {
    let mut events: Vec<AlarmEvent> = Vec::new();
    for item in plan {
        if item.fires_at < from || item.fires_at > to || is_suppressed(item, marks) {
            continue;
        }
        let at = floor_to_minute(item.fires_at);
        let index = match events.iter().position(|e| e.at == at) {
            Some(index) => index,
            None => {
                events.push(AlarmEvent {
                    at,
                    check_in: Vec::new(),
                    check_out: Vec::new(),
                });
                events.len() - 1
            }
        };
        let event = &mut events[index];
        match item.kind {
            MarkKind::In => event.check_in.push(item.clone()),
            MarkKind::Out => event.check_out.push(item.clone()),
        }
    }
    events.sort_by_key(|e| e.at);
    events
}

/// The first event strictly after `after`, if any (for the scheduler's
/// sleep-until time).
#[must_use]
pub fn next_event(plan: &[PlanItem], marks: &[Mark], after: Timestamp) -> Option<AlarmEvent> {
    let first = plan
        .iter()
        .filter(|i| i.fires_at > after && !is_suppressed(i, marks))
        .map(|i| floor_to_minute(i.fires_at))
        .min()?;
    // Everything in that minute (a sub-minute `after` never splits it, since
    // plan times are whole minutes).
    due_events(plan, marks, first, first).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alarm_plan;
    use crate::model::{ScheduleSettings, Snapshot};
    use crate::test_support::*;
    use jiff::SignedDuration;
    use jiff::civil::Weekday;

    /// Anna 09:00–17:00, Babis 12:00–16:00 + 19:00–02:00, Christos 08:00–12:00,
    /// Dimitra 09:00–12:00.
    fn snapshot() -> Snapshot {
        let a = staff(1, "Anna", "Alpha");
        let b = staff(2, "Babis", "Beta");
        let c = staff(3, "Christos", "Gamma");
        let d = staff(4, "Dimitra", "Delta");
        Snapshot {
            blocks: vec![
                weekly(10, &a, Weekday::Monday, (9, 0), (17, 0)),
                weekly(20, &b, Weekday::Monday, (12, 0), (16, 0)),
                weekly(21, &b, Weekday::Monday, (19, 0), (2, 0)),
                weekly(30, &c, Weekday::Monday, (8, 0), (12, 0)),
                weekly(40, &d, Weekday::Monday, (9, 0), (12, 0)),
            ],
            staff: vec![a, b, c, d],
            ..Snapshot::default()
        }
    }

    fn plan(snap: &Snapshot) -> Vec<PlanItem> {
        let from = athens(MONDAY, 5, 0);
        alarm_plan(snap, from, from + SignedDuration::from_hours(24))
    }

    fn names(items: &[PlanItem]) -> Vec<&str> {
        items.iter().map(|i| i.display_name.as_str()).collect()
    }

    #[test]
    fn same_minute_alarms_are_grouped_into_one_event() {
        let snap = snapshot();
        let nine = athens(MONDAY, 9, 0);
        let events = due_events(&plan(&snap), &snap.marks, nine, nine);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].at, nine);
        assert_eq!(names(&events[0].check_in), ["Anna Alpha", "Dimitra Delta"]);
        assert!(events[0].check_out.is_empty());
        assert_eq!(events[0].sound(), Sound::CheckIn);
    }

    #[test]
    fn check_in_and_check_out_in_one_minute_play_the_check_in_sound() {
        let snap = snapshot();
        let noon = athens(MONDAY, 12, 0);
        let events = due_events(&plan(&snap), &snap.marks, noon, noon);
        assert_eq!(events.len(), 1);
        assert_eq!(names(&events[0].check_in), ["Babis Beta"]);
        assert_eq!(
            names(&events[0].check_out),
            ["Christos Gamma", "Dimitra Delta"]
        );
        assert_eq!(events[0].sound(), Sound::CheckIn);
        assert_eq!(events[0].item_ids().len(), 3);
    }

    #[test]
    fn check_out_only_plays_the_check_out_sound() {
        let snap = snapshot();
        let five_pm = athens(MONDAY, 17, 0);
        let events = due_events(&plan(&snap), &snap.marks, five_pm, five_pm);
        assert_eq!(events[0].sound(), Sound::CheckOut);
    }

    #[test]
    fn check_in_mark_suppresses_only_the_check_in_alarm() {
        let mut snap = snapshot();
        snap.marks.push(mark(100, 1, 10, MONDAY, MarkKind::In));
        let p = plan(&snap);
        let nine = athens(MONDAY, 9, 0);
        assert_eq!(
            names(&due_events(&p, &snap.marks, nine, nine)[0].check_in),
            ["Dimitra Delta"]
        );
        // Her check-out alarm still fires.
        let five_pm = athens(MONDAY, 17, 0);
        assert_eq!(due_events(&p, &snap.marks, five_pm, five_pm).len(), 1);
    }

    #[test]
    fn left_mark_suppresses_the_check_out_alarm() {
        let mut snap = snapshot();
        snap.marks.push(mark(100, 1, 10, MONDAY, MarkKind::In));
        snap.marks.push(mark(101, 1, 10, MONDAY, MarkKind::Out));
        let five_pm = athens(MONDAY, 17, 0);
        assert!(due_events(&plan(&snap), &snap.marks, five_pm, five_pm).is_empty());
    }

    #[test]
    fn check_out_fires_for_someone_never_marked_in() {
        let snap = snapshot();
        let five_pm = athens(MONDAY, 17, 0);
        let events = due_events(&plan(&snap), &snap.marks, five_pm, five_pm);
        assert_eq!(names(&events[0].check_out), ["Anna Alpha"]);
    }

    #[test]
    fn marks_for_other_blocks_or_dates_do_not_suppress() {
        let mut snap = snapshot();
        // Babis marked in for lunch: his evening check-in still rings.
        snap.marks.push(mark(100, 2, 20, MONDAY, MarkKind::In));
        // Anna marked in last Monday: today's alarm still rings.
        snap.marks.push(mark(
            101,
            1,
            10,
            jiff::civil::date(2026, 5, 25),
            MarkKind::In,
        ));
        let p = plan(&snap);
        let seven_pm = athens(MONDAY, 19, 0);
        assert_eq!(due_events(&p, &snap.marks, seven_pm, seven_pm).len(), 1);
        let nine = athens(MONDAY, 9, 0);
        assert_eq!(due_events(&p, &snap.marks, nine, nine)[0].check_in.len(), 2);
    }

    #[test]
    fn early_mark_prevents_the_alarm() {
        // Acceptance criterion 2: marked a minute before the alarm.
        let mut snap = snapshot();
        let p = plan(&snap);
        snap.marks.push(mark(100, 3, 30, MONDAY, MarkKind::In));
        let eight = athens(MONDAY, 8, 0);
        assert!(due_events(&p, &snap.marks, eight, eight).is_empty());
    }

    #[test]
    fn removed_block_no_longer_fires() {
        // Schedule changes rebuild the plan; the deleted block has no item.
        let mut snap = snapshot();
        snap.blocks.retain(|b| b.id != id(30));
        let eight = athens(MONDAY, 8, 0);
        assert!(due_events(&plan(&snap), &snap.marks, eight, eight).is_empty());
    }

    #[test]
    fn firing_window_covers_up_to_15_minutes_late() {
        let snap = snapshot();
        let p = plan(&snap);
        // Christos's 08:00 alarm.
        for (h, m, fires) in [
            (7, 59, false),
            (8, 0, true),
            (8, 14, true),
            (8, 15, true),
            (8, 16, false),
        ] {
            let (from, to) = firing_window(athens(MONDAY, h, m));
            let due = due_events(&p, &snap.marks, from, to);
            let has_eight = due.iter().any(|e| e.at == athens(MONDAY, 8, 0));
            assert_eq!(has_eight, fires, "{h:02}:{m:02}");
        }
        // 15 minutes and one second late is skipped.
        let late = athens(MONDAY, 8, 15) + SignedDuration::from_secs(1);
        let (from, to) = firing_window(late);
        assert!(
            due_events(&p, &snap.marks, from, to)
                .iter()
                .all(|e| e.at != athens(MONDAY, 8, 0))
        );
    }

    #[test]
    fn several_missed_events_come_back_oldest_first() {
        let snap = snapshot();
        let (from, to) = (athens(MONDAY, 7, 0), athens(MONDAY, 12, 0));
        let ats: Vec<Timestamp> = due_events(&plan(&snap), &snap.marks, from, to)
            .iter()
            .map(|e| e.at)
            .collect();
        assert_eq!(
            ats,
            [
                athens(MONDAY, 8, 0),
                athens(MONDAY, 9, 0),
                athens(MONDAY, 12, 0)
            ]
        );
    }

    #[test]
    fn next_event_skips_suppressed_minutes() {
        let mut snap = snapshot();
        let p = plan(&snap);
        let morning = athens(MONDAY, 6, 0);
        assert_eq!(
            next_event(&p, &snap.marks, morning).unwrap().at,
            athens(MONDAY, 8, 0)
        );
        snap.marks.push(mark(100, 3, 30, MONDAY, MarkKind::In));
        let next = next_event(&p, &snap.marks, morning).unwrap();
        assert_eq!(next.at, athens(MONDAY, 9, 0));
        assert_eq!(next.check_in.len(), 2);
        // Strictly after.
        assert_eq!(
            next_event(&p, &snap.marks, athens(MONDAY, 9, 0))
                .unwrap()
                .at,
            athens(MONDAY, 12, 0)
        );
        assert!(next_event(&p, &snap.marks, athens(TUESDAY, 3, 0)).is_none());
    }

    #[test]
    fn re_ring_drops_marked_names_and_ends_when_none_are_left() {
        let mut snap = snapshot();
        let p = plan(&snap);
        let noon = athens(MONDAY, 12, 0);
        let event = due_events(&p, &snap.marks, noon, noon).remove(0);
        assert_eq!(event.still_due(&p, &snap.marks).as_ref(), Some(&event));

        snap.marks.push(mark(100, 2, 20, MONDAY, MarkKind::In));
        let left = event.still_due(&p, &snap.marks).unwrap();
        assert!(left.check_in.is_empty());
        assert_eq!(left.check_out.len(), 2);
        assert_eq!(left.sound(), Sound::CheckOut);

        snap.marks.push(mark(101, 3, 30, MONDAY, MarkKind::Out));
        snap.marks.push(mark(102, 4, 40, MONDAY, MarkKind::Out));
        assert_eq!(event.still_due(&p, &snap.marks), None);
    }

    #[test]
    fn edited_block_time_keeps_its_id_but_the_old_time_no_longer_rings() {
        // Christos 08:00–12:00 is moved to 08:30–12:00, keeping its block id
        // (Phase 2 `schedule_set` keeps ids of edited blocks).
        let mut snap = snapshot();
        let old_plan = plan(&snap);
        let eight = athens(MONDAY, 8, 0);
        let event = due_events(&old_plan, &snap.marks, eight, eight).remove(0);
        let stale = event.check_in[0].clone();

        for b in &mut snap.blocks {
            if b.id == id(30) {
                b.start = jiff::civil::time(8, 30, 0, 0);
            }
        }
        let new_plan = plan(&snap);
        let moved = new_plan
            .iter()
            .find(|i| i.item_id == stale.item_id)
            .expect("same id after the edit");
        assert_eq!(moved.fires_at, athens(MONDAY, 8, 30));

        // The 08:00 alarm (stale device, or a re-ring) must not ring…
        assert!(!is_still_due(&stale, &new_plan, &snap.marks));
        assert_eq!(event.still_due(&new_plan, &snap.marks), None);
        // …and the 08:30 one does.
        assert!(is_still_due(moved, &new_plan, &snap.marks));
    }

    #[test]
    fn re_ring_drops_alarms_removed_by_a_schedule_change() {
        // During the silent gap, Babis's lunch block is deleted and Dimitra
        // gets a day off: only Christos is left in the 12:00 event.
        let mut snap = snapshot();
        let noon = athens(MONDAY, 12, 0);
        let event = due_events(&plan(&snap), &snap.marks, noon, noon).remove(0);

        snap.blocks.retain(|b| b.id != id(20));
        snap.overrides.push(day_off(90, &snap.staff[3], MONDAY));
        let left = event.still_due(&plan(&snap), &snap.marks).unwrap();
        assert!(left.check_in.is_empty());
        assert_eq!(names(&left.check_out), ["Christos Gamma"]);

        // An empty plan (e.g. everyone removed) ends the cycle.
        assert_eq!(event.still_due(&[], &snap.marks), None);
    }

    #[test]
    fn offsets_shift_grouping() {
        // With a −10 check-in offset, 12:00 check-in and 12:00 check-out no
        // longer share a minute.
        let mut snap = snapshot();
        snap.settings = ScheduleSettings {
            checkin_offset_min: -10,
            ..ScheduleSettings::default()
        };
        let p = plan(&snap);
        let events = due_events(
            &p,
            &snap.marks,
            athens(MONDAY, 11, 50),
            athens(MONDAY, 12, 0),
        );
        assert_eq!(events.len(), 2);
        assert_eq!(names(&events[0].check_in), ["Babis Beta"]);
        assert!(events[1].check_in.is_empty());
    }
}
