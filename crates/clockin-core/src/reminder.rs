//! The «Λείπει σήμανση» reminder (SPEC §7.6): after «Σταμάτημα» on an alarm,
//! the device reminds about the names in it that are still not marked.
//!
//! [`ReminderScheduler`] is a pure state machine for Windows, fed like
//! [`crate::AlarmScheduler`]. Android's Kotlin code follows the same rules
//! with [`reminder_at`] / [`reminders_due`] arithmetic on instants that Rust
//! computes ([`reminder_cutoffs`]), because it runs while the app is closed
//! (ARCHITECTURE §10).
//!
//! Rules:
//!
//! - Stop starts a group holding the alarms it ended
//!   ([`ReminderScheduler::start`]); with the setting Off, nothing starts.
//! - Reminder `k` (1, 2, …) of a group is due at Stop + k·N, in real minutes.
//! - Only names still due are listed: in the plan at the same time and not
//!   suppressed by a mark ([`crate::is_still_due`]). Removed and moved blocks
//!   and days off leave the plan. When no names are left, the group ends.
//! - Each name stops at the end of its alarm's business day
//!   ([`reminder_cutoff`]): nothing shows at or after it.
//! - «Εντάξει» ends the groups shown ([`ReminderScheduler::acknowledge`]).
//! - While an alarm is showing the reminder waits (`held`); once the alarm
//!   has gone it shows, with its sound if a reminder came due meanwhile.
//! - Off ends every group. A new N applies from the next reminder.

use crate::business_day::{business_date_for, business_day_start};
use crate::model::{MarkKind, Snapshot};
use crate::plan::{PlanItem, alarm_plan};
use crate::scheduler::{AlarmKey, MAX_SLEEP};
use crate::shop_time::add_days;
use crate::today::mark_exists;
use jiff::civil::{Date, Time};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The setting's default, in minutes (SPEC §4.4).
pub const DEFAULT_REMINDER_MIN: i32 = 10;
/// The largest delay the server accepts, in minutes; 0 means Off.
pub const MAX_REMINDER_MIN: i32 = 60;

/// The delay for the setting `minutes`, or `None` for Off (0, or anything
/// outside 1–60, which the server never stores).
#[must_use]
pub fn reminder_delay(minutes: i32) -> Option<SignedDuration> {
    (1..=MAX_REMINDER_MIN)
        .contains(&minutes)
        .then(|| SignedDuration::from_mins(i64::from(minutes)))
}

/// The end of business date `business_date`: the start of the next one. A
/// reminder for an alarm of that date never shows at or after it.
#[must_use]
pub fn reminder_cutoff(business_date: Date, rollover: Time) -> Option<Timestamp> {
    business_day_start(add_days(business_date, 1)?, rollover)
}

/// The cut-off of every business date from the one before `now`'s to `days`
/// after it, for the Android alarm code (which keeps them while the app is
/// closed).
#[must_use]
pub fn reminder_cutoffs(now: Timestamp, rollover: Time, days: u32) -> Vec<(Date, Timestamp)> {
    let today = business_date_for(now, rollover);
    (-1..=i64::from(days))
        .filter_map(|n| {
            let date = add_days(today, n)?;
            Some((date, reminder_cutoff(date, rollover)?))
        })
        .collect()
}

/// When reminder `k` (1 = the first) of a group started at `started_at` is due.
#[must_use]
pub fn reminder_at(started_at: Timestamp, delay: SignedDuration, k: u32) -> Timestamp {
    let k = i32::try_from(k).unwrap_or(i32::MAX);
    delay
        .checked_mul(k)
        .and_then(|span| started_at.checked_add(span).ok())
        .unwrap_or(Timestamp::MAX)
}

/// How many reminders of a group started at `started_at` have come due by
/// `now`: 0 before the first (also if the clock went back before the Stop).
#[must_use]
pub fn reminders_due(started_at: Timestamp, delay: SignedDuration, now: Timestamp) -> u32 {
    let step = delay.as_secs();
    if step <= 0 {
        return 0;
    }
    let elapsed = now.duration_since(started_at).as_secs();
    if elapsed <= 0 {
        return 0;
    }
    u32::try_from(elapsed / step).unwrap_or(u32::MAX)
}

/// The alarms one Stop ended, and how far its reminders have got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderGroup {
    pub id: u64,
    pub keys: BTreeSet<AlarmKey>,
    /// When «Σταμάτημα» was pressed.
    pub started_at: Timestamp,
    /// The last reminder that showed with its sound (0: none yet).
    pub shown: u32,
}

/// One name on the reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReminderName {
    pub item: PlanItem,
    /// A check-out name whose arrival was never marked either.
    pub no_arrival_mark: bool,
}

/// The reminder window's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnmarkedReminder {
    /// Pass back to [`ReminderScheduler::acknowledge`] («Εντάξει»).
    pub group_ids: Vec<u64>,
    /// In plan order.
    pub check_in: Vec<ReminderName>,
    pub check_out: Vec<ReminderName>,
    /// When it reminds again; `None` when this is the last one before the
    /// end of the business day.
    pub next_at: Option<Timestamp>,
}

/// What the device should be doing now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderState {
    /// The reminder to show, if any.
    pub reminder: Option<UnmarkedReminder>,
    /// Play the reminder sound once, now.
    pub chime: bool,
    /// Evaluate again no later than this.
    pub wake_at: Timestamp,
    /// The groups changed: save [`ReminderScheduler::groups`].
    pub changed: bool,
}

/// See the module docs.
#[derive(Debug, Clone, Default)]
pub struct ReminderScheduler {
    groups: Vec<ReminderGroup>,
    next_id: u64,
}

impl ReminderScheduler {
    /// Starts with the groups saved on this device, so a restart keeps
    /// pending reminders.
    #[must_use]
    pub fn with_groups(groups: Vec<ReminderGroup>) -> Self {
        let next_id = groups.iter().map(|g| g.id).max().unwrap_or(0);
        Self { groups, next_id }
    }

    /// The groups, for saving.
    #[must_use]
    pub fn groups(&self) -> &[ReminderGroup] {
        &self.groups
    }

    /// «Σταμάτημα» ended the alarms `keys` at `at`. Returns the new group's
    /// id; `None` when the setting is Off or there is nothing to remind.
    pub fn start(
        &mut self,
        keys: impl IntoIterator<Item = AlarmKey>,
        at: Timestamp,
        delay: Option<SignedDuration>,
    ) -> Option<u64> {
        delay?;
        let keys: BTreeSet<AlarmKey> = keys.into_iter().collect();
        if keys.is_empty() {
            return None;
        }
        self.next_id += 1;
        self.groups.push(ReminderGroup {
            id: self.next_id,
            keys,
            started_at: at,
            shown: 0,
        });
        Some(self.next_id)
    }

    /// «Εντάξει» pressed on this device: the groups `ids` end. Returns whether
    /// anything changed. A group that started showing after the window was
    /// drawn isn't in `ids`, so it stays.
    pub fn acknowledge(&mut self, ids: &[u64]) -> bool {
        let before = self.groups.len();
        self.groups.retain(|g| !ids.contains(&g.id));
        self.groups.len() != before
    }

    /// Updates the reminders for `now` and says what to show. `delay` is the
    /// setting ([`reminder_delay`]); `held` is true while an alarm is showing.
    pub fn evaluate(
        &mut self,
        now: Timestamp,
        snapshot: &Snapshot,
        delay: Option<SignedDuration>,
        held: bool,
    ) -> ReminderState {
        let mut wake_at = now.saturating_add(MAX_SLEEP).unwrap_or(now);
        let Some(delay) = delay else {
            let changed = !self.groups.is_empty();
            self.groups.clear();
            return ReminderState {
                reminder: None,
                chime: false,
                wake_at,
                changed,
            };
        };
        if self.groups.is_empty() {
            return ReminderState {
                reminder: None,
                chime: false,
                wake_at,
                changed: false,
            };
        }

        let plan = self.plan(now, snapshot);
        let rollover = snapshot.settings.rollover;
        let cutoff = |item: &PlanItem| reminder_cutoff(item.business_date, rollover);
        let mut changed = false;
        let mut chime = false;
        let mut group_ids = Vec::new();
        let mut shown_keys = BTreeSet::new();
        let mut next_at: Option<Timestamp> = None;

        self.groups.retain_mut(|group| {
            let due: Vec<&PlanItem> = plan
                .iter()
                .filter(|i| {
                    group.keys.contains(&AlarmKey::of(i))
                        && !crate::is_suppressed(i, &snapshot.marks)
                        && cutoff(i).is_some_and(|c| now < c)
                })
                .collect();
            if due.is_empty() {
                changed = true;
                return false;
            }
            let last_cutoff = due.iter().filter_map(|i| cutoff(i)).max();
            if let Some(first_cutoff) = due.iter().filter_map(|i| cutoff(i)).min() {
                wake_at = wake_at.min(first_cutoff);
            }
            let k = reminders_due(group.started_at, delay, now);
            let next = reminder_at(group.started_at, delay, k.saturating_add(1));
            wake_at = wake_at.min(next);
            if k == 0 || held {
                return true;
            }
            if k > group.shown {
                group.shown = k;
                chime = true;
                changed = true;
            }
            group_ids.push(group.id);
            shown_keys.extend(due.iter().map(|i| AlarmKey::of(i)));
            if last_cutoff.is_some_and(|c| next < c) {
                next_at = Some(next_at.map_or(next, |n| n.min(next)));
            }
            true
        });

        let reminder = (!group_ids.is_empty()).then(|| {
            let pick = |kind: MarkKind| -> Vec<ReminderName> {
                plan.iter()
                    .filter(|i| i.kind == kind && shown_keys.contains(&AlarmKey::of(i)))
                    .map(|i| ReminderName {
                        item: i.clone(),
                        no_arrival_mark: kind == MarkKind::Out
                            && !mark_exists(
                                &snapshot.marks,
                                i.staff_id,
                                i.source_block_id,
                                i.business_date,
                                MarkKind::In,
                            ),
                    })
                    .collect()
            };
            UnmarkedReminder {
                group_ids,
                check_in: pick(MarkKind::In),
                check_out: pick(MarkKind::Out),
                next_at,
            }
        });
        ReminderState {
            reminder,
            chime,
            wake_at: wake_at.max(
                now.saturating_add(SignedDuration::from_secs(1))
                    .unwrap_or(now),
            ),
            changed,
        }
    }

    /// The plan from the oldest alarm a group holds up to now.
    fn plan(&self, now: Timestamp, snapshot: &Snapshot) -> Vec<PlanItem> {
        let keys = self.groups.iter().flat_map(|g| g.keys.iter());
        let (Some(from), Some(latest)) = (
            keys.clone().map(|k| k.fires_at).min(),
            keys.map(|k| k.fires_at).max(),
        ) else {
            return Vec::new();
        };
        let until = latest
            .max(now)
            .saturating_add(SignedDuration::from_mins(1))
            .unwrap_or(latest);
        alarm_plan(snapshot, from, until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AlarmScheduler;
    use crate::model::{Mark, ScheduleSettings};
    use crate::test_support::*;
    use jiff::civil::{Weekday, date, time};

    const TEN: Option<SignedDuration> = Some(SignedDuration::from_mins(10));

    /// Anna 09:00–17:00, Babis 09:00–13:00, Christos 18:00–02:00 on Mondays.
    fn snapshot() -> Snapshot {
        let a = staff(1, "Anna", "Alpha");
        let b = staff(2, "Babis", "Beta");
        let c = staff(3, "Christos", "Gamma");
        Snapshot {
            blocks: vec![
                weekly(10, &a, Weekday::Monday, (9, 0), (17, 0)),
                weekly(20, &b, Weekday::Monday, (9, 0), (13, 0)),
                weekly(30, &c, Weekday::Monday, (18, 0), (2, 0)),
            ],
            staff: vec![a, b, c],
            ..Snapshot::default()
        }
    }

    fn at(h: i8, m: i8) -> Timestamp {
        athens(MONDAY, h, m)
    }

    fn mins(t: Timestamp, m: i64) -> Timestamp {
        t + SignedDuration::from_mins(m)
    }

    /// Rings the alarms due at `t` on a fresh alarm scheduler, presses Stop,
    /// and starts the reminder: what a device does.
    fn stop_at(r: &mut ReminderScheduler, snap: &Snapshot, t: Timestamp) -> u64 {
        let mut alarms = AlarmScheduler::default();
        let alarm = alarms.evaluate(t, snap).alarm.expect("an alarm rings");
        let keys = alarms.stop(alarm.id).expect("stopped");
        r.start(keys, t, TEN).expect("a group")
    }

    fn display_names(v: &[ReminderName]) -> Vec<&str> {
        v.iter().map(|n| n.item.display_name.as_str()).collect()
    }

    fn names(state: &ReminderState) -> (Vec<&str>, Vec<&str>) {
        let r = state.reminder.as_ref().expect("a reminder");
        (display_names(&r.check_in), display_names(&r.check_out))
    }

    fn mark_in(snap: &mut Snapshot, n: u128, who: u128, block: u128) {
        snap.marks.push(mark(n, who, block, MONDAY, MarkKind::In));
    }

    #[test]
    fn delay_setting() {
        assert_eq!(reminder_delay(0), None);
        assert_eq!(reminder_delay(-5), None);
        assert_eq!(reminder_delay(61), None);
        assert_eq!(reminder_delay(1), Some(SignedDuration::from_mins(1)));
        assert_eq!(reminder_delay(10), TEN);
        assert_eq!(reminder_delay(60), Some(SignedDuration::from_mins(60)));
        assert_eq!(reminder_delay(DEFAULT_REMINDER_MIN), TEN);
    }

    #[test]
    fn reminder_arithmetic() {
        let t = at(9, 0);
        let d = SignedDuration::from_mins(10);
        assert_eq!(reminder_at(t, d, 1), at(9, 10));
        assert_eq!(reminder_at(t, d, 3), at(9, 30));
        assert_eq!(reminders_due(t, d, at(9, 9)), 0);
        assert_eq!(reminders_due(t, d, at(9, 10)), 1);
        assert_eq!(reminders_due(t, d, mins(at(9, 19), 0)), 1);
        assert_eq!(reminders_due(t, d, at(9, 20)), 2);
        assert_eq!(reminders_due(t, d, at(8, 0)), 0, "clock went back");
        assert_eq!(reminders_due(t, SignedDuration::ZERO, at(10, 0)), 0);
    }

    #[test]
    fn stop_hands_over_every_name_of_the_alarm() {
        let snap = snapshot();
        let mut alarms = AlarmScheduler::default();
        let alarm = alarms.evaluate(at(9, 0), &snap).alarm.unwrap();
        let keys = alarms.stop(alarm.id).unwrap();
        assert_eq!(keys.len(), 2, "Anna and Babis");
        assert!(alarms.stop(alarm.id).is_none(), "only once");
    }

    #[test]
    fn shows_n_minutes_after_stop_with_the_sound_then_every_n_minutes() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        let stop = mins(at(9, 0), 2);
        let id = stop_at(&mut r, &snap, stop);

        let before = r.evaluate(mins(stop, 9), &snap, TEN, false);
        assert_eq!(before.reminder, None);
        assert!(!before.chime);
        let before = r.evaluate(
            mins(stop, 9) + SignedDuration::from_secs(45),
            &snap,
            TEN,
            false,
        );
        assert_eq!(before.wake_at, mins(stop, 10), "wakes for it");

        let first = r.evaluate(mins(stop, 10), &snap, TEN, false);
        assert_eq!(names(&first), (vec!["Anna Alpha", "Babis Beta"], vec![]));
        assert!(first.chime, "the C plays when it appears");
        assert!(first.changed);
        let view = first.reminder.unwrap();
        assert_eq!(view.group_ids, [id]);
        assert_eq!(view.next_at, Some(mins(stop, 20)));

        // It stays, silent, until the next one.
        let still = r.evaluate(mins(stop, 15), &snap, TEN, false);
        assert!(still.reminder.is_some());
        assert!(!still.chime);
        assert!(!still.changed);
        assert_eq!(still.wake_at, mins(stop, 15) + MAX_SLEEP);

        let second = r.evaluate(mins(stop, 20), &snap, TEN, false);
        assert!(second.chime, "the C plays again after N minutes");
        assert_eq!(second.reminder.unwrap().next_at, Some(mins(stop, 30)));
        // A device asleep through several reminders plays the sound once.
        let late = r.evaluate(mins(stop, 75), &snap, TEN, false);
        assert!(late.chime);
        assert!(!r.evaluate(mins(stop, 76), &snap, TEN, false).chime);
    }

    #[test]
    fn off_starts_nothing_and_ends_pending_reminders() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        let mut alarms = AlarmScheduler::default();
        let alarm = alarms.evaluate(at(9, 0), &snap).alarm.unwrap();
        let keys = alarms.stop(alarm.id).unwrap();
        assert_eq!(r.start(keys.clone(), at(9, 0), None), None);
        assert!(r.groups().is_empty());
        assert_eq!(
            r.start(Vec::new(), at(9, 0), TEN),
            None,
            "nothing to remind"
        );

        r.start(keys, at(9, 0), TEN).unwrap();
        let off = r.evaluate(at(9, 10), &snap, None, false);
        assert_eq!(off.reminder, None);
        assert!(off.changed);
        assert!(r.groups().is_empty());
        // Back on: the ended reminder doesn't come back.
        assert_eq!(r.evaluate(at(9, 20), &snap, TEN, false).reminder, None);
    }

    #[test]
    fn a_new_delay_applies_from_the_next_reminder() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        let twenty = Some(SignedDuration::from_mins(20));
        assert_eq!(r.evaluate(at(9, 10), &snap, twenty, false).reminder, None);
        let s = r.evaluate(at(9, 20), &snap, twenty, false);
        assert!(s.chime);
        assert_eq!(s.reminder.unwrap().next_at, Some(at(9, 40)));
    }

    #[test]
    fn names_marked_anywhere_drop_out_and_the_reminder_ends_when_none_are_left() {
        let mut snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        mark_in(&mut snap, 100, 1, 10);
        let s = r.evaluate(at(9, 10), &snap, TEN, false);
        assert_eq!(names(&s), (vec!["Babis Beta"], vec![]));
        mark_in(&mut snap, 101, 2, 20);
        let s = r.evaluate(at(9, 11), &snap, TEN, false);
        assert_eq!(s.reminder, None, "closes by itself");
        assert!(!s.chime);
        assert!(s.changed);
        assert!(r.groups().is_empty());
    }

    #[test]
    fn everyone_marked_before_the_reminder_means_no_reminder() {
        let mut snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        mark_in(&mut snap, 100, 1, 10);
        mark_in(&mut snap, 101, 2, 20);
        let s = r.evaluate(at(9, 10), &snap, TEN, false);
        assert_eq!(s.reminder, None);
        assert!(!s.chime);
        assert!(r.groups().is_empty());
    }

    #[test]
    fn a_mark_removed_in_settings_counts_as_not_marked() {
        // The snapshot carries live marks only; a voided one is simply gone.
        let mut snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        mark_in(&mut snap, 100, 1, 10);
        assert_eq!(
            names(&r.evaluate(at(9, 10), &snap, TEN, false)).0,
            ["Babis Beta"]
        );
        snap.marks.clear();
        let s = r.evaluate(at(9, 12), &snap, TEN, false);
        assert_eq!(names(&s).0, ["Anna Alpha", "Babis Beta"]);
    }

    #[test]
    fn removed_and_moved_shifts_and_days_off_give_no_reminder() {
        let base = snapshot();
        let mut r0 = ReminderScheduler::default();
        stop_at(&mut r0, &base, at(9, 0));

        // Anna's block deleted.
        let mut removed = base.clone();
        removed.blocks.retain(|b| b.id != id(10));
        let mut r = r0.clone();
        assert_eq!(
            names(&r.evaluate(at(9, 10), &removed, TEN, false)).0,
            ["Babis Beta"]
        );

        // Anna's block moved to 09:30: the 09:00 alarm is gone (the 09:30 one
        // is a new alarm that rings by itself).
        let mut moved = base.clone();
        moved.blocks[0].start = time(9, 30, 0, 0);
        let mut r = r0.clone();
        assert_eq!(
            names(&r.evaluate(at(9, 10), &moved, TEN, false)).0,
            ["Babis Beta"]
        );

        // Both get a day off.
        let mut off = base.clone();
        off.overrides = vec![
            day_off(90, &base.staff[0], MONDAY),
            day_off(91, &base.staff[1], MONDAY),
        ];
        let mut r = r0.clone();
        assert_eq!(r.evaluate(at(9, 10), &off, TEN, false).reminder, None);
        assert!(r.groups().is_empty());

        // Anna removed from the staff after her block started: the block stays
        // (SPEC §4.1), so she is still reminded about.
        let mut gone = base.clone();
        gone.staff[0].removed_at = Some(at(9, 5));
        let mut r = r0;
        assert_eq!(names(&r.evaluate(at(9, 10), &gone, TEN, false)).0.len(), 2);
    }

    #[test]
    fn stops_at_the_end_of_the_business_day() {
        // Christos's 02:00 check-out (Tuesday night, Monday's business day).
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        let stop = athens(TUESDAY, 2, 0);
        stop_at(&mut r, &snap, stop);
        assert_eq!(reminder_cutoff(MONDAY, FIVE), Some(athens(TUESDAY, 5, 0)));

        let s = r.evaluate(athens(TUESDAY, 4, 50), &snap, TEN, false);
        assert_eq!(names(&s), (vec![], vec!["Christos Gamma"]));
        assert_eq!(
            s.reminder.as_ref().unwrap().next_at,
            None,
            "the last one before 05:00"
        );
        assert_eq!(s.wake_at, athens(TUESDAY, 4, 50) + MAX_SLEEP);
        let near = r.evaluate(
            athens(TUESDAY, 4, 59) + SignedDuration::from_secs(45),
            &snap,
            TEN,
            false,
        );
        assert_eq!(near.wake_at, athens(TUESDAY, 5, 0), "wakes at the cut-off");
        let end = r.evaluate(athens(TUESDAY, 5, 0), &snap, TEN, false);
        assert_eq!(end.reminder, None, "the window closes at the rollover");
        assert!(!end.chime);
        assert!(r.groups().is_empty());
    }

    #[test]
    fn no_reminder_due_after_the_cut_off_is_ever_shown() {
        // Stop at 04:55: the first reminder would be 05:05, after the cut-off.
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        let mut alarms = AlarmScheduler::default();
        let alarm = alarms.evaluate(athens(TUESDAY, 2, 0), &snap).alarm.unwrap();
        let keys = alarms.stop(alarm.id).unwrap();
        r.start(keys, athens(TUESDAY, 4, 55), TEN).unwrap();
        for (h, m) in [(4, 56), (5, 0), (5, 5), (6, 0)] {
            assert_eq!(
                r.evaluate(athens(TUESDAY, h, m), &snap, TEN, false)
                    .reminder,
                None
            );
        }
    }

    #[test]
    fn a_check_out_after_the_rollover_gets_no_reminder() {
        // 22:00–06:00 with the 05:00 rollover: the 06:00 check-out is after
        // the end of its business day.
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Monday, (22, 0), (6, 0))],
            staff: vec![a],
            ..Snapshot::default()
        };
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, athens(TUESDAY, 6, 0));
        let s = r.evaluate(athens(TUESDAY, 6, 10), &snap, TEN, false);
        assert_eq!(s.reminder, None);
        assert!(r.groups().is_empty());
    }

    #[test]
    fn cut_off_follows_the_rollover_setting() {
        assert_eq!(
            reminder_cutoff(MONDAY, time(0, 0, 0, 0)),
            Some(athens(TUESDAY, 0, 0))
        );
        assert_eq!(
            reminder_cutoff(MONDAY, time(8, 0, 0, 0)),
            Some(athens(TUESDAY, 8, 0))
        );
        // Rollover 00:00: Anna's check-in reminder ends at midnight.
        let mut snap = snapshot();
        snap.settings = ScheduleSettings {
            rollover: time(0, 0, 0, 0),
            ..ScheduleSettings::default()
        };
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        assert!(r.evaluate(at(23, 50), &snap, TEN, false).reminder.is_some());
        assert_eq!(
            r.evaluate(athens(TUESDAY, 0, 0), &snap, TEN, false)
                .reminder,
            None
        );
    }

    #[test]
    fn cut_off_on_both_dst_nights() {
        // Fall back, 25 Oct 2026 04:00 EEST → 03:00 EET: Saturday's day ends
        // at Sunday 05:00 EET = 03:00 UTC.
        let saturday = date(2026, 10, 24);
        let utc = |s: &str| s.parse::<Timestamp>().unwrap();
        assert_eq!(
            reminder_cutoff(saturday, FIVE),
            Some(utc("2026-10-25T03:00:00Z"))
        );
        // A rollover inside the repeated hour takes its first pass (EEST).
        assert_eq!(
            reminder_cutoff(saturday, time(3, 30, 0, 0)),
            Some(utc("2026-10-25T00:30:00Z"))
        );
        // Spring forward, 28 Mar 2027 03:00 EET → 04:00 EEST: 05:00 EEST =
        // 02:00 UTC; a 03:30 rollover doesn't exist and moves to 04:00 EEST.
        let saturday = date(2027, 3, 27);
        assert_eq!(
            reminder_cutoff(saturday, FIVE),
            Some(utc("2027-03-28T02:00:00Z"))
        );
        assert_eq!(
            reminder_cutoff(saturday, time(3, 30, 0, 0)),
            Some(utc("2027-03-28T01:00:00Z"))
        );
    }

    #[test]
    fn reminders_count_real_minutes_through_the_fall_back_night() {
        // Christos 20:00–02:00 on Saturday 24 Oct 2026. Stop at 02:00 EEST
        // (23:00 UTC); every 30 real minutes until 05:00 EET (03:00 UTC):
        // 4 real hours, 7 reminders, the 8th would be at the cut-off.
        let c = staff(3, "Christos", "Gamma");
        let snap = Snapshot {
            blocks: vec![weekly(30, &c, Weekday::Saturday, (20, 0), (2, 0))],
            staff: vec![c],
            ..Snapshot::default()
        };
        let half = Some(SignedDuration::from_mins(30));
        let stop: Timestamp = "2026-10-24T23:00:00Z".parse().unwrap();
        let mut alarms = AlarmScheduler::default();
        let alarm = alarms.evaluate(stop, &snap).alarm.unwrap();
        let mut r = ReminderScheduler::default();
        r.start(alarms.stop(alarm.id).unwrap(), stop, half).unwrap();
        let mut chimes = 0;
        let mut t = stop;
        while t < "2026-10-25T04:00:00Z".parse().unwrap() {
            if r.evaluate(t, &snap, half, false).chime {
                chimes += 1;
            }
            t = mins(t, 1);
        }
        assert_eq!(chimes, 7);
    }

    #[test]
    fn reminders_count_real_minutes_through_the_spring_forward_night() {
        // Stop at 02:00 EET on 28 Mar 2027 (00:00 UTC); the business day ends
        // at 05:00 EEST (02:00 UTC): reminders at 00:30, 01:00, 01:30 UTC.
        let saturday = date(2027, 3, 27);
        let c = staff(3, "Christos", "Gamma");
        let snap = Snapshot {
            blocks: vec![weekly(30, &c, Weekday::Saturday, (20, 0), (2, 0))],
            staff: vec![c],
            ..Snapshot::default()
        };
        let half = Some(SignedDuration::from_mins(30));
        let stop = athens(date(2027, 3, 28), 2, 0);
        assert_eq!(stop, "2027-03-28T00:00:00Z".parse().unwrap());
        let mut alarms = AlarmScheduler::default();
        let alarm = alarms.evaluate(stop, &snap).alarm.unwrap();
        assert_eq!(alarm.event.check_out[0].business_date, saturday);
        let mut r = ReminderScheduler::default();
        r.start(alarms.stop(alarm.id).unwrap(), stop, half).unwrap();
        let mut chimes = Vec::new();
        let mut t = stop;
        while t < "2027-03-28T03:00:00Z".parse().unwrap() {
            if r.evaluate(t, &snap, half, false).chime {
                chimes.push(t.to_string());
            }
            t = mins(t, 1);
        }
        assert_eq!(
            chimes,
            [
                "2027-03-28T00:30:00Z",
                "2027-03-28T01:00:00Z",
                "2027-03-28T01:30:00Z"
            ]
        );
    }

    #[test]
    fn waits_while_an_alarm_is_showing_then_shows_with_the_sound() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        let held = r.evaluate(at(9, 10), &snap, TEN, true);
        assert_eq!(held.reminder, None);
        assert!(!held.chime);
        let held = r.evaluate(at(9, 40), &snap, TEN, true);
        assert_eq!(held.reminder, None);
        let shown = r.evaluate(at(9, 41), &snap, TEN, false);
        assert!(shown.reminder.is_some());
        assert!(shown.chime, "once, when the alarm has gone");
        assert!(!r.evaluate(at(9, 42), &snap, TEN, false).chime);
        // Held again while it was showing: hidden, and nothing is lost.
        assert_eq!(r.evaluate(at(9, 45), &snap, TEN, true).reminder, None);
        assert!(r.evaluate(at(9, 46), &snap, TEN, false).reminder.is_some());
    }

    #[test]
    fn ok_ends_the_reminders_shown_but_not_a_newer_one() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        let first = stop_at(&mut r, &snap, at(9, 0));
        let shown = r.evaluate(at(9, 10), &snap, TEN, false).reminder.unwrap();
        assert_eq!(shown.group_ids, [first]);
        // Anna's check-out alarm is stopped at 17:00 before «Εντάξει» on the
        // 09:00 one.
        let second = stop_at(&mut r, &snap, at(17, 0));
        assert!(r.acknowledge(&shown.group_ids));
        assert!(!r.acknowledge(&shown.group_ids), "only once");
        assert_eq!(r.groups().len(), 1);
        assert_eq!(r.groups()[0].id, second);
        let s = r.evaluate(at(17, 10), &snap, TEN, false);
        assert_eq!(names(&s), (vec![], vec!["Anna Alpha"]));
        assert!(r.acknowledge(&s.reminder.unwrap().group_ids));
        assert_eq!(r.evaluate(at(17, 20), &snap, TEN, false).reminder, None);
    }

    #[test]
    fn reminders_of_several_alarms_show_together_with_their_own_times() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        let a = stop_at(&mut r, &snap, at(13, 0)); // Babis's check-out
        let b = stop_at(&mut r, &snap, at(17, 0)); // Anna's check-out
        let s = r.evaluate(at(17, 10), &snap, TEN, false);
        let view = s.reminder.unwrap();
        assert_eq!(view.group_ids, [a, b]);
        let shown: Vec<(&str, Timestamp)> = view
            .check_out
            .iter()
            .map(|n| (n.item.display_name.as_str(), n.item.fires_at))
            .collect();
        assert_eq!(
            shown,
            [("Babis Beta", at(13, 0)), ("Anna Alpha", at(17, 0))]
        );
        // Both groups remind again at 17:20.
        assert_eq!(view.next_at, Some(at(17, 20)));
    }

    #[test]
    fn check_in_and_check_out_names_and_the_no_arrival_note() {
        let mut snap = snapshot();
        // Babis 13:00 out (never marked in), Anna's 9:00 in.
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        let s = r.evaluate(at(13, 0), &snap, TEN, false);
        assert_eq!(names(&s).0.len(), 2);
        stop_at(&mut r, &snap, at(13, 0));
        mark_in(&mut snap, 100, 1, 10); // Anna arrives
        let s = r.evaluate(at(13, 10), &snap, TEN, false);
        assert_eq!(names(&s), (vec!["Babis Beta"], vec!["Babis Beta"]));
        let view = s.reminder.unwrap();
        assert!(view.check_out[0].no_arrival_mark);
        assert!(!view.check_in[0].no_arrival_mark);
        // Marking the departure only (allowed from the reminder) ends the
        // check-out name; the check-in name stays.
        let mut departed = snap.clone();
        departed.marks.push(mark(101, 2, 20, MONDAY, MarkKind::Out));
        let s = r.clone().evaluate(at(13, 11), &departed, TEN, false);
        assert_eq!(names(&s), (vec!["Babis Beta"], vec![]));
        // With the arrival marked instead, the note goes.
        mark_in(&mut snap, 102, 2, 20);
        let s = r.evaluate(at(13, 12), &snap, TEN, false);
        assert_eq!(names(&s), (vec![], vec!["Babis Beta"]));
        assert!(!s.reminder.unwrap().check_out[0].no_arrival_mark);
    }

    #[test]
    fn pending_reminders_survive_a_restart() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        // Restart before the first reminder.
        let saved = r.groups().to_vec();
        let json = serde_json::to_string(&saved).unwrap();
        let mut r = ReminderScheduler::with_groups(serde_json::from_str(&json).unwrap());
        let s = r.evaluate(at(9, 10), &snap, TEN, false);
        assert!(s.chime);
        // Restart while it shows: it shows again, without playing again.
        let mut r = ReminderScheduler::with_groups(r.groups().to_vec());
        let s = r.evaluate(at(9, 12), &snap, TEN, false);
        assert!(s.reminder.is_some());
        assert!(!s.chime);
        // New ids keep counting after the saved ones.
        let next = stop_at(&mut r, &snap, at(13, 0));
        assert!(next > r.groups()[0].id);
    }

    #[test]
    fn a_clock_that_went_back_hides_the_reminder_until_it_is_due_again() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        assert!(r.evaluate(at(9, 10), &snap, TEN, false).reminder.is_some());
        let back = r.evaluate(at(8, 0), &snap, TEN, false);
        assert_eq!(back.reminder, None);
        assert!(back.wake_at > at(8, 0));
        assert_eq!(r.groups().len(), 1, "nothing is lost");
    }

    #[test]
    fn wake_time_is_always_in_the_future_and_at_most_30_seconds_away() {
        let snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        let mut t = at(9, 0);
        while t < athens(TUESDAY, 6, 0) {
            let s = r.evaluate(t, &snap, TEN, false);
            assert!(s.wake_at > t && s.wake_at <= t + MAX_SLEEP, "{t}");
            t = mins(t, 7);
        }
    }

    #[test]
    fn cutoffs_for_android_cover_yesterday_to_the_horizon() {
        let list = reminder_cutoffs(at(12, 0), FIVE, 400);
        assert_eq!(list.len(), 402);
        assert_eq!(list[0], (date(2026, 5, 31), athens(MONDAY, 5, 0)));
        assert_eq!(list[1], (MONDAY, athens(TUESDAY, 5, 0)));
        assert!(list.windows(2).all(|w| w[0].1 < w[1].1));
        // Each entry agrees with the cut-off rule.
        assert!(
            list.iter()
                .all(|&(d, c)| reminder_cutoff(d, FIVE) == Some(c))
        );
    }

    #[test]
    fn a_mark_unrelated_to_the_alarm_changes_nothing() {
        let mut snap = snapshot();
        let mut r = ReminderScheduler::default();
        stop_at(&mut r, &snap, at(9, 0));
        // Anna's check-out (not the check-in alarm) and last week's check-in.
        snap.marks.push(mark(100, 1, 10, MONDAY, MarkKind::Out));
        snap.marks.push(Mark {
            business_date: date(2026, 5, 25),
            ..mark(101, 2, 20, MONDAY, MarkKind::In)
        });
        let s = r.evaluate(at(9, 10), &snap, TEN, false);
        assert_eq!(names(&s).0, ["Anna Alpha", "Babis Beta"]);
    }
}
