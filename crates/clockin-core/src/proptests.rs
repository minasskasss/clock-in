//! Property tests (ARCHITECTURE §12): sortedness, id determinism, no
//! duplicates, and the business-day invariants, over random schedules and
//! instants from 2026 to 2028 (which include several DST changes).

use crate::*;
use jiff::civil::{Date, Time, Weekday};
use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;
use std::collections::HashSet;
use uuid::Uuid;

fn time_from_minutes(m: u32) -> Time {
    Time::new((m / 60) as i8, (m % 60) as i8, 0, 0).unwrap()
}

/// 2026-01-01T00:00Z … 2029-01-01T00:00Z.
fn instant() -> impl Strategy<Value = Timestamp> {
    (1_767_225_600_i64..1_861_920_000).prop_map(|s| Timestamp::from_second(s).unwrap())
}

/// Instants close to the 2026 and 2027 DST changes, where bugs hide.
fn dst_instant() -> impl Strategy<Value = Timestamp> {
    let changes = [
        "2026-03-29T01:00:00Z",
        "2026-10-25T01:00:00Z",
        "2027-03-28T01:00:00Z",
        "2027-10-31T01:00:00Z",
    ];
    (0..changes.len(), -36 * 3600_i64..36 * 3600).prop_map(move |(i, off)| {
        changes[i].parse::<Timestamp>().unwrap() + SignedDuration::from_secs(off)
    })
}

fn any_instant() -> impl Strategy<Value = Timestamp> {
    prop_oneof![instant(), dst_instant()]
}

fn settings() -> impl Strategy<Value = ScheduleSettings> {
    (-60_i32..=30, -60_i32..=30, 0_u32..=8 * 60).prop_map(|(cin, cout, roll)| ScheduleSettings {
        checkin_offset_min: cin,
        checkout_offset_min: cout,
        rollover: time_from_minutes(roll),
    })
}

/// (start minute, duration minutes) of a valid-length block.
fn block_times() -> impl Strategy<Value = (Time, Time)> {
    (0_u32..1440, 15_u32..=960).prop_map(|(start, dur)| {
        (
            time_from_minutes(start),
            time_from_minutes((start + dur) % 1440),
        )
    })
}

fn snapshot(around: Timestamp) -> impl Strategy<Value = Snapshot> {
    let staff_n = 1_u128..=4;
    (
        staff_n,
        settings(),
        prop::collection::vec((0_u128..4, 1_i8..=7, block_times()), 0..10),
        prop::collection::vec((0_u128..4, -3_i64..10, any::<bool>(), block_times()), 0..4),
        prop::collection::vec((0_u128..4, any::<bool>()), 0..3),
    )
        .prop_map(move |(n, settings, blocks, overrides, removed)| {
            let mut staff: Vec<Staff> = (0..n)
                .map(|i| Staff {
                    id: Uuid::from_u128(i + 1),
                    first_name: format!("F{i}"),
                    last_name: ["Ζ", "Α", "Α", "Β"][i as usize].to_string(),
                    removed_at: None,
                })
                .collect();
            for (who, early) in removed {
                if let Some(s) = staff.get_mut(who as usize) {
                    let hours = if early { -30 } else { 30 };
                    s.removed_at = Some(around + SignedDuration::from_hours(hours));
                }
            }
            let blocks = blocks
                .into_iter()
                .enumerate()
                .map(|(i, (who, wd, (start, end)))| WeeklyBlock {
                    id: Uuid::from_u128(1000 + i as u128),
                    staff_id: Uuid::from_u128(who % n + 1),
                    weekday: Weekday::from_monday_one_offset(wd).unwrap(),
                    start,
                    end,
                })
                .collect();
            let today = business_date_for(around, settings.rollover);
            let mut seen = HashSet::new();
            let overrides = overrides
                .into_iter()
                .enumerate()
                .filter_map(|(i, (who, day, off, (start, end)))| {
                    let staff_id = Uuid::from_u128(who % n + 1);
                    let business_date = today.checked_add(jiff::Span::new().days(day)).ok()?;
                    seen.insert((staff_id, business_date)).then(|| Override {
                        id: Uuid::from_u128(2000 + i as u128),
                        staff_id,
                        business_date,
                        kind: if off {
                            OverrideKind::Off
                        } else {
                            OverrideKind::Replace
                        },
                        blocks: if off {
                            Vec::new()
                        } else {
                            vec![OverrideBlock {
                                id: Uuid::from_u128(3000 + i as u128),
                                start,
                                end,
                            }]
                        },
                    })
                })
                .collect();
            Snapshot {
                staff,
                blocks,
                overrides,
                settings,
                marks: Vec::new(),
            }
        })
}

fn instant_and_snapshot() -> impl Strategy<Value = (Timestamp, Snapshot)> {
    any_instant().prop_flat_map(|now| (Just(now), snapshot(now)))
}

fn plan_order(a: &PlanItem, b: &PlanItem) -> std::cmp::Ordering {
    a.fires_at
        .cmp(&b.fires_at)
        .then(a.kind.cmp(&b.kind))
        .then_with(|| {
            crate::occurrence::name_sort_key(&a.display_name)
                .cmp(&crate::occurrence::name_sort_key(&b.display_name))
        })
        .then_with(|| a.item_id.cmp(&b.item_id))
}

fn next_date(d: Date) -> Date {
    d.tomorrow().unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn plan_is_sorted_unique_in_window_and_ids_match((now, snap) in instant_and_snapshot()) {
        let (from, until) = plan_window(now);
        let plan = alarm_plan(&snap, from, until);
        prop_assert!(plan.windows(2).all(|w| plan_order(&w[0], &w[1]).is_lt()));
        let ids: HashSet<&str> = plan.iter().map(|i| i.item_id.as_str()).collect();
        prop_assert_eq!(ids.len(), plan.len());
        for item in &plan {
            prop_assert!(item.fires_at >= from && item.fires_at < until);
            prop_assert_eq!(item.fires_at, floor_to_minute(item.fires_at));
            prop_assert_eq!(
                &item.item_id,
                &item_id(item.kind, item.staff_id, item.source_block_id, item.business_date)
            );
        }
    }

    #[test]
    fn plan_ignores_input_order((now, snap) in instant_and_snapshot()) {
        let (from, until) = plan_window(now);
        let mut shuffled = snap.clone();
        shuffled.staff.reverse();
        shuffled.blocks.reverse();
        shuffled.overrides.reverse();
        prop_assert_eq!(alarm_plan(&snap, from, until), alarm_plan(&shuffled, from, until));
    }

    #[test]
    fn plan_ids_survive_recomputation_later((now, snap) in instant_and_snapshot(), hours in 1_i64..72) {
        // A plan recomputed later keeps the id and time of every alarm both cover.
        let later = now + SignedDuration::from_hours(hours);
        let a = alarm_plan(&snap, now, now + SignedDuration::from_hours(96));
        let b = alarm_plan(&snap, later, later + SignedDuration::from_hours(96));
        let overlap = |i: &&PlanItem| i.fires_at >= later && i.fires_at < now + SignedDuration::from_hours(96);
        let a: Vec<&PlanItem> = a.iter().filter(overlap).collect();
        let b: Vec<&PlanItem> = b.iter().filter(overlap).collect();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn plan_windows_split_cleanly((now, snap) in instant_and_snapshot(), split in 0_i64..(5 * 24 * 60)) {
        let end = now + SignedDuration::from_hours(5 * 24);
        let mid = now + SignedDuration::from_mins(split);
        let whole = alarm_plan(&snap, now, end);
        let mut parts = alarm_plan(&snap, now, mid);
        parts.extend(alarm_plan(&snap, mid, end));
        prop_assert_eq!(whole, parts);
    }

    #[test]
    fn every_occurrence_yields_both_alarms((now, snap) in instant_and_snapshot()) {
        // Over a generous window, the plan holds exactly one check-in and one
        // check-out per occurrence of the middle business day.
        let day = business_date_for(now, snap.settings.rollover);
        let occ = occurrences(&snap.staff, &snap.blocks, &snap.overrides, day, snap.settings.rollover);
        let plan = alarm_plan(
            &snap,
            now - SignedDuration::from_hours(72),
            now + SignedDuration::from_hours(72),
        );
        for o in &occ {
            for kind in [MarkKind::In, MarkKind::Out] {
                let id = item_id(kind, o.staff_id, o.source_block_id, o.business_date);
                prop_assert_eq!(plan.iter().filter(|i| i.item_id == id).count(), 1);
            }
        }
    }

    #[test]
    fn business_date_is_monotonic_and_bracketed(a in any_instant(), b in any_instant(), roll in 0_u32..=480) {
        let rollover = time_from_minutes(roll);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(business_date_for(lo, rollover) <= business_date_for(hi, rollover));
        let d = business_date_for(a, rollover);
        let start = business_day_start(d, rollover).unwrap();
        let next = business_day_start(next_date(d), rollover).unwrap();
        prop_assert!(start <= a && a < next);
    }

    #[test]
    fn occurrences_fall_inside_their_business_day((now, snap) in instant_and_snapshot()) {
        let rollover = snap.settings.rollover;
        let day = business_date_for(now, rollover);
        let start = business_day_start(day, rollover).unwrap();
        let next = business_day_start(next_date(day), rollover).unwrap();
        let occ = occurrences(&snap.staff, &snap.blocks, &snap.overrides, day, rollover);
        prop_assert!(occ.windows(2).all(|w| w[0].display_cmp(&w[1]).is_lt()));
        for o in &occ {
            prop_assert_eq!(o.business_date, day);
            // `<= next`: a start in the spring-forward gap moves to the
            // next valid minute, which can be exactly the next day's start.
            prop_assert!(o.start >= start && o.start <= next, "{} not in [{}, {}]", o.start, start, next);
            prop_assert!(o.end >= o.start);
            prop_assert!(o.end.duration_since(o.start) <= SignedDuration::from_hours(17));
        }
    }

    #[test]
    fn today_view_rows_are_unique_and_ordered((now, snap) in instant_and_snapshot()) {
        let view = today_view(&snap, now);
        prop_assert_eq!(view.business_date, business_date_for(now, snap.settings.rollover));
        let keys: HashSet<(Uuid, Date)> = view
            .rows
            .iter()
            .map(|r| (r.occurrence.source_block_id, r.occurrence.business_date))
            .collect();
        prop_assert_eq!(keys.len(), view.rows.len());
        prop_assert!(view.rows.windows(2).all(|w| w[0].occurrence.display_cmp(&w[1].occurrence).is_lt()));
        for r in &view.rows {
            let expected = if now >= r.occurrence.start { RowStatus::Late } else { RowStatus::Pending };
            prop_assert_eq!(r.status, expected);
        }
    }

    #[test]
    fn due_events_are_grouped_and_never_suppressed(
        (now, mut snap) in instant_and_snapshot(),
        mark_every in 1_usize..4,
    ) {
        let (from, until) = plan_window(now);
        let plan = alarm_plan(&snap, from, until);
        snap.marks = plan
            .iter()
            .step_by(mark_every)
            .enumerate()
            .map(|(i, item)| Mark {
                id: Uuid::from_u128(9000 + i as u128),
                staff_id: item.staff_id,
                source_block_id: item.source_block_id,
                business_date: item.business_date,
                kind: item.kind,
            })
            .collect();
        let events = due_events(&plan, &snap.marks, from, until);
        prop_assert!(events.windows(2).all(|w| w[0].at < w[1].at));
        let mut count = 0;
        for e in &events {
            prop_assert!(!e.check_in.is_empty() || !e.check_out.is_empty());
            for item in e.check_in.iter().chain(&e.check_out) {
                prop_assert_eq!(floor_to_minute(item.fires_at), e.at);
                prop_assert!(!is_suppressed(item, &snap.marks));
                count += 1;
            }
            prop_assert!(e.check_in.iter().all(|i| i.kind == MarkKind::In));
            prop_assert!(e.check_out.iter().all(|i| i.kind == MarkKind::Out));
            prop_assert_eq!(e.sound() == Sound::CheckIn, !e.check_in.is_empty());
        }
        let unsuppressed = plan.iter().filter(|i| !is_suppressed(i, &snap.marks)).count();
        prop_assert_eq!(count, unsuppressed);
    }

    #[test]
    fn ring_cycle_is_consistent(started in instant(), secs in -600_i64..(48 * 3600)) {
        let now = started + SignedDuration::from_secs(secs);
        match ring_cycle_state(started, now) {
            RingPhase::Ringing { cycle, silent_at } => {
                prop_assert!(silent_at > now);
                // A clock that went backwards keeps the first ring going longer.
                prop_assert!(secs < 0 || silent_at.duration_since(now) <= RING_DURATION);
                prop_assert_eq!(
                    silent_at,
                    started + SignedDuration::from_mins(10 * i64::from(cycle) + 5)
                );
            }
            RingPhase::Silent { cycle, rering_at } => {
                prop_assert!(rering_at > now);
                prop_assert!(rering_at.duration_since(now) <= SILENT_DURATION);
                prop_assert_eq!(
                    rering_at,
                    started + SignedDuration::from_mins(10 * i64::from(cycle) + 10)
                );
            }
        }
    }

    #[test]
    fn accepted_weeks_have_valid_non_overlapping_blocks(
        raw in prop::collection::vec((1_i8..=7, 0_u32..1440, 0_u32..1440), 1..8),
        roll in 0_u32..=480,
    ) {
        let rollover = time_from_minutes(roll);
        let blocks: Vec<DayBlock> = raw
            .iter()
            .map(|&(wd, s, e)| DayBlock {
                weekday: Weekday::from_monday_one_offset(wd).unwrap(),
                start: time_from_minutes(s),
                end: time_from_minutes(e),
            })
            .collect();
        if validate_week(&blocks, rollover).is_ok() {
            let layouts: Vec<BlockLayout> =
                blocks.iter().map(|b| BlockLayout::new(b.start, b.end, rollover)).collect();
            for (i, a) in layouts.iter().enumerate() {
                prop_assert!((MIN_BLOCK_MINUTES..=MAX_BLOCK_MINUTES).contains(&a.duration_minutes()));
                for (j, b) in layouts.iter().enumerate().skip(i + 1) {
                    if blocks[i].weekday == blocks[j].weekday {
                        prop_assert!(!a.overlaps(*b));
                    }
                }
            }
        }
    }
}
