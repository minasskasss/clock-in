//! The materialised alarm plan (ARCHITECTURE §2 and §4): every check-in and
//! check-out alarm in a time window, with deterministic ids.

use crate::business_day::business_date_for;
use crate::model::{MarkKind, Snapshot};
use crate::occurrence::{name_sort_key, occurrences};
use crate::shop_time::{add_days, floor_to_minute};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// How late an alarm may still fire after the device was asleep or off
/// (SPEC §7.5).
pub const MISSED_ALARM_GRACE: SignedDuration = SignedDuration::from_mins(15);

/// How far ahead the plan is computed (ARCHITECTURE §8).
pub const PLAN_HORIZON: SignedDuration = SignedDuration::from_hours(14 * 24);

/// One alarm for one person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    /// See [`item_id`].
    pub item_id: String,
    pub fires_at: Timestamp,
    pub kind: MarkKind,
    pub staff_id: Uuid,
    /// "First Last".
    pub display_name: String,
    pub business_date: Date,
    pub source_block_id: Uuid,
}

/// The plan id of an alarm: the first 32 hex characters of
/// SHA-256(`kind|staff_id|source_block_id|business_date`), e.g.
/// `in|<uuid>|<uuid>|2026-10-25` with lowercase hyphenated uuids.
///
/// It depends only on what the alarm is for, not on when it fires, so a
/// recomputed plan keeps the same ids.
#[must_use]
pub fn item_id(
    kind: MarkKind,
    staff_id: Uuid,
    source_block_id: Uuid,
    business_date: Date,
) -> String {
    let input = format!(
        "{}|{}|{}|{}",
        kind.as_str(),
        staff_id.hyphenated(),
        source_block_id.hyphenated(),
        business_date
    );
    let digest = Sha256::digest(input.as_bytes());
    let mut hex = String::with_capacity(32);
    for byte in &digest[..16] {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// The window a freshly uploaded plan covers: from 15 minutes ago (so an
/// alarm that a sleeping device is about to fire late is still in the plan)
/// to 14 days ahead. The second value is the plan's `horizon_end`.
#[must_use]
pub fn plan_window(now: Timestamp) -> (Timestamp, Timestamp) {
    (
        now.saturating_sub(MISSED_ALARM_GRACE).unwrap_or(now),
        now.saturating_add(PLAN_HORIZON).unwrap_or(now),
    )
}

/// Every alarm with `from <= fires_at < until`, sorted by time, then check-in
/// before check-out, then name. Ids are unique.
///
/// - check-in alarm: block start + check-in offset;
/// - check-out alarm: block end + check-out offset.
///
/// Offsets are real minutes, applied to the instants, so they stay correct
/// across a DST change.
#[must_use]
pub fn alarm_plan(snapshot: &Snapshot, from: Timestamp, until: Timestamp) -> Vec<PlanItem> {
    if until <= from {
        return Vec::new();
    }
    let settings = snapshot.settings;
    let rollover = settings.rollover;
    let checkin = SignedDuration::from_mins(i64::from(settings.checkin_offset_min));
    let checkout = SignedDuration::from_mins(i64::from(settings.checkout_offset_min));

    // A business date's alarms can fire up to an hour before it starts
    // (negative offset) and well into the next day (overnight block plus
    // offset), so look one date further on each side, plus one for safety.
    let first = business_date_for(from, rollover);
    let last = business_date_for(until, rollover);
    let (Some(mut date), Some(last)) = (add_days(first, -2), add_days(last, 1)) else {
        return Vec::new();
    };

    let mut items = Vec::new();
    while date <= last {
        for occ in occurrences(
            &snapshot.staff,
            &snapshot.blocks,
            &snapshot.overrides,
            date,
            rollover,
        ) {
            for (kind, at, offset) in [
                (MarkKind::In, occ.start, checkin),
                (MarkKind::Out, occ.end, checkout),
            ] {
                let Ok(fires_at) = at.checked_add(offset) else {
                    continue;
                };
                let fires_at = floor_to_minute(fires_at);
                if fires_at < from || fires_at >= until {
                    continue;
                }
                items.push(PlanItem {
                    item_id: item_id(kind, occ.staff_id, occ.source_block_id, date),
                    fires_at,
                    kind,
                    staff_id: occ.staff_id,
                    display_name: occ.display_name(),
                    business_date: date,
                    source_block_id: occ.source_block_id,
                });
            }
        }
        let Some(next) = add_days(date, 1) else { break };
        date = next;
    }

    items.sort_by(|a, b| {
        a.fires_at
            .cmp(&b.fires_at)
            .then(a.kind.cmp(&b.kind))
            .then_with(|| name_sort_key(&a.display_name).cmp(&name_sort_key(&b.display_name)))
            .then_with(|| a.item_id.cmp(&b.item_id))
    });
    // Ids are unique unless the input repeats a block; keep the first.
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.item_id.clone()));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScheduleSettings;
    use crate::test_support::*;
    use jiff::civil::{Weekday, date, time};

    fn one_day(snapshot: &Snapshot, day: Date) -> Vec<PlanItem> {
        let from = athens(day, 5, 0);
        alarm_plan(snapshot, from, from + SignedDuration::from_hours(24))
    }

    fn times(items: &[PlanItem]) -> Vec<(MarkKind, Timestamp)> {
        items.iter().map(|i| (i.kind, i.fires_at)).collect()
    }

    #[test]
    fn item_id_is_stable_and_documented() {
        let id_in = item_id(MarkKind::In, id(1), id(10), date(2026, 10, 25));
        assert_eq!(id_in.len(), 32);
        assert!(
            id_in
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        // Pinned value: the server and Kotlin treat ids as opaque, but a change
        // here would orphan every plan already uploaded.
        // sha256("in|00000000-0000-0000-0000-000000000001|00000000-0000-0000-0000-00000000000a|2026-10-25")
        assert_eq!(id_in, "273f11eb7f7422ad8ca0706e9a052e6c");
        let id_out = item_id(MarkKind::Out, id(1), id(10), date(2026, 10, 25));
        assert_ne!(id_in, id_out);
        assert_ne!(
            id_in,
            item_id(MarkKind::In, id(1), id(10), date(2026, 10, 26))
        );
    }

    #[test]
    fn day_shift_gives_check_in_and_check_out_alarms() {
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))],
            staff: vec![a],
            ..Snapshot::default()
        };
        let plan = one_day(&snap, MONDAY);
        assert_eq!(
            times(&plan),
            [
                (MarkKind::In, athens(MONDAY, 9, 0)),
                (MarkKind::Out, athens(MONDAY, 17, 0)),
            ]
        );
        assert_eq!(plan[0].display_name, "Anna Alpha");
        assert_eq!(plan[0].business_date, MONDAY);
        assert_eq!(plan[0].source_block_id, id(10));
        assert_eq!(
            plan[0].item_id,
            item_id(MarkKind::In, id(1), id(10), MONDAY)
        );
    }

    #[test]
    fn negative_and_positive_offsets() {
        let a = staff(1, "Anna", "Alpha");
        let mut snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))],
            staff: vec![a],
            ..Snapshot::default()
        };
        for (cin, cout, expect_in, expect_out) in [
            (-60, 30, (8, 0), (17, 30)),
            (-10, -5, (8, 50), (16, 55)),
            (30, -60, (9, 30), (16, 0)),
        ] {
            snap.settings = ScheduleSettings {
                checkin_offset_min: cin,
                checkout_offset_min: cout,
                ..ScheduleSettings::default()
            };
            assert_eq!(
                times(&one_day(&snap, MONDAY)),
                [
                    (MarkKind::In, athens(MONDAY, expect_in.0, expect_in.1)),
                    (MarkKind::Out, athens(MONDAY, expect_out.0, expect_out.1)),
                ],
                "offsets {cin}/{cout}"
            );
        }
    }

    #[test]
    fn negative_offset_pulls_an_alarm_before_the_business_day_starts() {
        // A 05:00 start with a −60 offset fires at 04:00, still inside the
        // previous business day; the plan must include it.
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Tuesday, (5, 0), (9, 0))],
            staff: vec![a],
            settings: ScheduleSettings {
                checkin_offset_min: -60,
                ..ScheduleSettings::default()
            },
            ..Snapshot::default()
        };
        let plan = alarm_plan(&snap, athens(TUESDAY, 3, 0), athens(TUESDAY, 4, 30));
        assert_eq!(times(&plan), [(MarkKind::In, athens(TUESDAY, 4, 0))]);
        assert_eq!(plan[0].business_date, TUESDAY);
    }

    #[test]
    fn overnight_check_out_lands_next_morning() {
        let b = staff(2, "Babis", "Beta");
        let snap = Snapshot {
            blocks: vec![weekly(21, &b, Weekday::Monday, (19, 0), (2, 0))],
            staff: vec![b],
            ..Snapshot::default()
        };
        // A window that starts on Tuesday after midnight still sees Monday's check-out.
        let plan = alarm_plan(&snap, athens(TUESDAY, 0, 30), athens(TUESDAY, 12, 0));
        assert_eq!(times(&plan), [(MarkKind::Out, athens(TUESDAY, 2, 0))]);
        assert_eq!(plan[0].business_date, MONDAY);
    }

    #[test]
    fn window_bounds_are_from_inclusive_until_exclusive() {
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))],
            staff: vec![a],
            ..Snapshot::default()
        };
        let nine = athens(MONDAY, 9, 0);
        let five_pm = athens(MONDAY, 17, 0);
        assert_eq!(alarm_plan(&snap, nine, five_pm).len(), 1);
        assert_eq!(
            alarm_plan(&snap, nine, five_pm + SignedDuration::from_secs(1)).len(),
            2
        );
        assert!(alarm_plan(&snap, five_pm, nine).is_empty());
    }

    #[test]
    fn plan_window_reaches_back_15_minutes_and_ahead_14_days() {
        let now = athens(MONDAY, 12, 0);
        let (from, until) = plan_window(now);
        assert_eq!(from, athens(MONDAY, 11, 45));
        assert_eq!(until, athens(date(2026, 6, 15), 12, 0));
    }

    #[test]
    fn fourteen_days_of_a_weekday_schedule() {
        let a = staff(1, "Anna", "Alpha");
        let blocks = [
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
            Weekday::Thursday,
            Weekday::Friday,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, wd)| weekly(10 + i as u128, &a, wd, (9, 0), (17, 0)))
        .collect();
        let snap = Snapshot {
            blocks,
            staff: vec![a],
            ..Snapshot::default()
        };
        let (from, until) = plan_window(athens(MONDAY, 6, 0));
        let plan = alarm_plan(&snap, from, until);
        assert_eq!(plan.len(), 10 * 2, "10 working days × in/out");
        assert!(plan.windows(2).all(|w| w[0].fires_at <= w[1].fires_at));
    }

    #[test]
    fn same_minute_sorts_check_in_first_then_by_name() {
        let a = staff(1, "Zoe", "Zeta");
        let b = staff(2, "Anna", "Alpha");
        let c = staff(3, "Babis", "Beta");
        let snap = Snapshot {
            blocks: vec![
                weekly(10, &a, Weekday::Monday, (12, 0), (16, 0)),
                weekly(20, &b, Weekday::Monday, (12, 0), (16, 0)),
                weekly(30, &c, Weekday::Monday, (8, 0), (12, 0)),
            ],
            staff: vec![a, b, c],
            ..Snapshot::default()
        };
        let noon: Vec<(MarkKind, String)> = one_day(&snap, MONDAY)
            .into_iter()
            .filter(|i| i.fires_at == athens(MONDAY, 12, 0))
            .map(|i| (i.kind, i.display_name))
            .collect();
        assert_eq!(
            noon,
            [
                (MarkKind::In, "Anna Alpha".to_string()),
                (MarkKind::In, "Zoe Zeta".to_string()),
                (MarkKind::Out, "Babis Beta".to_string()),
            ]
        );
    }

    #[test]
    fn day_off_and_replace_overrides_change_the_alarms() {
        let a = staff(1, "Anna", "Alpha");
        let c = staff(3, "Christos", "Gamma");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))],
            overrides: vec![
                day_off(90, &a, MONDAY),
                replace(91, &c, MONDAY, &[(92, (10, 0), (14, 0))]),
            ],
            staff: vec![a, c],
            ..Snapshot::default()
        };
        let plan = one_day(&snap, MONDAY);
        assert_eq!(
            plan.iter()
                .map(|i| (i.kind, i.fires_at, i.source_block_id))
                .collect::<Vec<_>>(),
            [
                (MarkKind::In, athens(MONDAY, 10, 0), id(92)),
                (MarkKind::Out, athens(MONDAY, 14, 0), id(92)),
            ]
        );
    }

    #[test]
    fn marks_do_not_remove_plan_items() {
        // Suppression happens when an alarm is due (`due_events`, `check_alarm`),
        // so the plan stays the same whatever is marked.
        let a = staff(1, "Anna", "Alpha");
        let mut snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))],
            staff: vec![a],
            ..Snapshot::default()
        };
        let before = one_day(&snap, MONDAY);
        snap.marks.push(mark(100, 1, 10, MONDAY, MarkKind::In));
        assert_eq!(one_day(&snap, MONDAY), before);
    }

    #[test]
    fn duplicate_blocks_do_not_duplicate_ids() {
        let a = staff(1, "Anna", "Alpha");
        let block = weekly(10, &a, Weekday::Monday, (9, 0), (17, 0));
        let snap = Snapshot {
            blocks: vec![block.clone(), block],
            staff: vec![a],
            ..Snapshot::default()
        };
        assert_eq!(one_day(&snap, MONDAY).len(), 2);
    }

    #[test]
    fn fall_back_2026_10_25_shift_spanning_the_change() {
        // Saturday 22:00–06:00 across the night the clocks go back.
        let a = staff(1, "Anna", "Alpha");
        let sat = date(2026, 10, 24);
        let snap = Snapshot {
            blocks: vec![
                weekly(10, &a, Weekday::Saturday, (22, 0), (6, 0)),
                // 03:30 happens twice that night; the alarm fires the first time.
                weekly(11, &a, Weekday::Sunday, (3, 30), (4, 30)),
            ],
            staff: vec![a],
            settings: ScheduleSettings {
                rollover: time(8, 0, 0, 0),
                ..ScheduleSettings::default()
            },
            ..Snapshot::default()
        };
        let plan = alarm_plan(
            &snap,
            utc("2026-10-24T00:00:00Z"),
            utc("2026-10-27T00:00:00Z"),
        );
        assert_eq!(
            times(&plan),
            [
                (MarkKind::In, utc("2026-10-24T19:00:00Z")), // Sat 22:00 EEST
                (MarkKind::Out, utc("2026-10-25T04:00:00Z")), // Sun 06:00 EET
                (MarkKind::In, utc("2026-10-26T01:30:00Z")), // Mon 03:30 EET (+1 of Sunday)
                (MarkKind::Out, utc("2026-10-26T02:30:00Z")), // Mon 04:30 EET
            ]
        );
        let _ = sat;
    }

    #[test]
    fn fall_back_ambiguous_alarm_fires_at_first_occurrence() {
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Saturday, (3, 30), (4, 30))],
            staff: vec![a],
            ..Snapshot::default()
        };
        let plan = alarm_plan(
            &snap,
            utc("2026-10-24T00:00:00Z"),
            utc("2026-10-26T00:00:00Z"),
        );
        assert_eq!(
            times(&plan),
            [
                (MarkKind::In, utc("2026-10-25T00:30:00Z")), // first 03:30
                (MarkKind::Out, utc("2026-10-25T02:30:00Z")), // 04:30 EET
            ]
        );
    }

    #[test]
    fn fall_back_offset_counts_real_minutes() {
        // Check-out at 04:30 (after the change) with a −60 offset fires 60 real
        // minutes earlier: 02:30 UTC − 1 h = 01:30 UTC, the *second* 03:30.
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Saturday, (22, 0), (4, 30))],
            staff: vec![a],
            settings: ScheduleSettings {
                checkout_offset_min: -60,
                ..ScheduleSettings::default()
            },
            ..Snapshot::default()
        };
        let plan = alarm_plan(
            &snap,
            utc("2026-10-24T00:00:00Z"),
            utc("2026-10-26T00:00:00Z"),
        );
        assert_eq!(plan[1].fires_at, utc("2026-10-25T01:30:00Z"));
    }

    #[test]
    fn spring_forward_2027_03_28_shift_spanning_the_change() {
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![
                weekly(10, &a, Weekday::Saturday, (22, 0), (6, 0)),
                // 03:30 does not exist that night; it fires at 04:00.
                weekly(11, &a, Weekday::Sunday, (3, 30), (5, 0)),
            ],
            staff: vec![a],
            settings: ScheduleSettings {
                rollover: time(8, 0, 0, 0),
                ..ScheduleSettings::default()
            },
            ..Snapshot::default()
        };
        let plan = alarm_plan(
            &snap,
            utc("2027-03-27T00:00:00Z"),
            utc("2027-03-30T00:00:00Z"),
        );
        assert_eq!(
            times(&plan),
            [
                (MarkKind::In, utc("2027-03-27T20:00:00Z")), // Sat 22:00 EET
                (MarkKind::Out, utc("2027-03-28T03:00:00Z")), // Sun 06:00 EEST
                (MarkKind::In, utc("2027-03-29T00:30:00Z")), // Mon 03:30 EEST (+1 of Sunday)
                (MarkKind::Out, utc("2027-03-29T02:00:00Z")), // Mon 05:00 EEST
            ]
        );
    }

    #[test]
    fn spring_forward_nonexistent_alarm_fires_at_next_valid_minute() {
        let a = staff(1, "Anna", "Alpha");
        let snap = Snapshot {
            blocks: vec![weekly(10, &a, Weekday::Saturday, (3, 30), (5, 0))],
            staff: vec![a],
            ..Snapshot::default()
        };
        let plan = alarm_plan(
            &snap,
            utc("2027-03-27T00:00:00Z"),
            utc("2027-03-29T00:00:00Z"),
        );
        assert_eq!(
            times(&plan),
            [
                (MarkKind::In, utc("2027-03-28T01:00:00Z")), // 04:00 EEST
                (MarkKind::Out, utc("2027-03-28T02:00:00Z")), // 05:00 EEST
            ]
        );
    }
}
