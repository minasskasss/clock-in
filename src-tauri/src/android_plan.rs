//! The plan the Android alarm code schedules from (ARCHITECTURE §10).
//! Compiled for Android and for the tests.

use clockin_core::{
    Snapshot, alarm_plan, auto_dark_windows, is_suppressed, reminder_cutoffs, upload_window,
};
use jiff::Timestamp;
use jiff::civil::Time;
use tauri_plugin_clockin_alarm::{BridgeDayEnd, BridgeItem, BridgePlan, BridgeWindow};

/// Days of automatic-theme dark periods sent after today's: the plan's 14.
const DARK_WINDOW_DAYS: u32 = 15;

/// Business-day ends sent for the reminder cut-off: far more than the plan,
/// because the background refresh keeps taking the server's plan for as long
/// as the app stays closed.
const DAY_END_DAYS: u32 = 400;

/// The plan from `snapshot`, from the start of the previous business day
/// (like the server's, `upload_window`: an alarm still ringing, or silent
/// between rings, keeps its names when a new plan arrives; Kotlin fires
/// nothing more than 15 minutes late) to 14 days ahead, without the alarms
/// that marks known on this device (including ones not yet synced) already
/// make unnecessary. No snapshot (unpaired, or never synced): no alarms.
/// Also the alarm screen's theme, the automatic theme's dark periods, and
/// the «Λείπει σήμανση» delay and business-day ends.
#[must_use]
pub fn bridge_plan(
    snapshot: Option<&Snapshot>,
    alert_mode: &'static str,
    theme: &'static str,
    config_version: i64,
    rollover: Time,
    reminder_min: i32,
    now: Timestamp,
) -> BridgePlan {
    let (from, until) = upload_window(now, rollover);
    let items = snapshot.map_or_else(Vec::new, |s| {
        alarm_plan(s, from, until)
            .into_iter()
            .filter(|item| !is_suppressed(item, &s.marks))
            .map(|item| BridgeItem {
                item_id: item.item_id,
                fires_at: item.fires_at.to_string(),
                kind: item.kind.as_str(),
                name: item.display_name,
                staff_id: item.staff_id.hyphenated().to_string(),
                source_block_id: item.source_block_id.hyphenated().to_string(),
                business_date: item.business_date.to_string(),
            })
            .collect()
    });
    BridgePlan {
        items,
        alert_mode,
        config_version,
        horizon_end: snapshot.is_some().then(|| until.to_string()),
        theme,
        dark_windows: auto_dark_windows(now, DARK_WINDOW_DAYS, rollover)
            .into_iter()
            .map(|(start, end)| BridgeWindow {
                start: start.as_millisecond(),
                end: end.as_millisecond(),
            })
            .collect(),
        reminder_min,
        day_ends: reminder_cutoffs(now, rollover, DAY_END_DAYS)
            .into_iter()
            .map(|(date, end)| BridgeDayEnd {
                date: date.to_string(),
                end: end.as_millisecond(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clockin_core::{Mark, MarkKind, Staff, WeeklyBlock, plan_window};
    use jiff::civil::{Time, Weekday, date};
    use uuid::Uuid;

    const STAFF: Uuid = Uuid::from_u128(0xA);
    const BLOCK: Uuid = Uuid::from_u128(0xB);

    fn snapshot() -> Snapshot {
        Snapshot {
            staff: vec![Staff {
                id: STAFF,
                first_name: "Μαρία".into(),
                last_name: "Παππά".into(),
                removed_at: None,
            }],
            blocks: vec![WeeklyBlock {
                id: BLOCK,
                staff_id: STAFF,
                weekday: Weekday::Tuesday,
                start: Time::constant(12, 0, 0, 0),
                end: Time::constant(16, 0, 0, 0),
            }],
            ..Snapshot::default()
        }
    }

    const FIVE: Time = Time::constant(5, 0, 0, 0);

    /// Tuesday 2026-10-06, 09:00 in Athens.
    fn now() -> Timestamp {
        "2026-10-06T06:00:00Z".parse().unwrap()
    }

    #[test]
    fn sends_the_plan_window_with_utc_times() {
        let plan = bridge_plan(Some(&snapshot()), "ring", "auto", 4, FIVE, 10, now());
        // Two Tuesdays in the 14-day window, check-in and check-out each.
        assert_eq!(plan.items.len(), 4);
        assert_eq!(plan.items[0].fires_at, "2026-10-06T09:00:00Z");
        assert_eq!(plan.items[0].kind, "in");
        assert_eq!(plan.items[0].name, "Μαρία Παππά");
        assert_eq!(plan.items[0].staff_id, STAFF.hyphenated().to_string());
        assert_eq!(
            plan.items[0].source_block_id,
            BLOCK.hyphenated().to_string()
        );
        assert_eq!(plan.items[0].business_date, "2026-10-06");
        assert_eq!(plan.items[1].fires_at, "2026-10-06T13:00:00Z");
        assert_eq!(plan.items[1].kind, "out");
        assert_eq!(plan.alert_mode, "ring");
        assert_eq!(plan.config_version, 4);
        assert_eq!(
            plan.horizon_end.as_deref(),
            Some(plan_window(now()).1.to_string().as_str())
        );
    }

    #[test]
    fn leaves_out_alarms_a_known_mark_suppresses() {
        let mut s = snapshot();
        s.marks.push(Mark {
            id: Uuid::from_u128(1),
            staff_id: STAFF,
            source_block_id: BLOCK,
            business_date: date(2026, 10, 6),
            kind: MarkKind::In,
        });
        let plan = bridge_plan(Some(&s), "notification", "dark", 4, FIVE, 10, now());
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.items[0].kind, "out");
        assert_eq!(plan.alert_mode, "notification");
    }

    #[test]
    fn no_data_means_no_alarms() {
        let plan = bridge_plan(None, "ring", "auto", 0, FIVE, 10, now());
        assert!(plan.items.is_empty());
        assert_eq!(plan.horizon_end, None);
    }

    #[test]
    fn sends_the_theme_and_the_automatic_themes_dark_periods() {
        let plan = bridge_plan(None, "ring", "system", 0, FIVE, 10, now());
        assert_eq!(plan.theme, "system");
        assert_eq!(plan.dark_windows.len(), 16);
        // Tonight 21:00 until tomorrow 05:00, Athens summer time (UTC+3).
        let ms = |s: &str| s.parse::<Timestamp>().unwrap().as_millisecond();
        assert_eq!(plan.dark_windows[0].start, ms("2026-10-06T18:00:00Z"));
        assert_eq!(plan.dark_windows[0].end, ms("2026-10-07T02:00:00Z"));
    }

    #[test]
    fn keeps_alarms_that_already_rang_so_a_long_ring_keeps_its_names() {
        // Tuesday 15:00: the 12:00 check-in rang three hours ago and stays
        // in the plan (Kotlin never fires it again: 15 minutes at most).
        let later: Timestamp = "2026-10-06T12:00:00Z".parse().unwrap();
        let plan = bridge_plan(Some(&snapshot()), "ring", "auto", 4, FIVE, 10, later);
        assert_eq!(plan.items[0].fires_at, "2026-10-06T09:00:00Z");
        assert!(plan_window(later).0 > "2026-10-06T09:00:00Z".parse().unwrap());
    }

    #[test]
    fn sends_the_reminder_delay_and_the_business_day_ends() {
        let plan = bridge_plan(None, "ring", "auto", 0, FIVE, 25, now());
        assert_eq!(plan.reminder_min, 25);
        assert_eq!(plan.day_ends.len(), 402);
        // Yesterday (Monday 5 Oct) ends today at 05:00 Athens = 02:00 UTC.
        assert_eq!(plan.day_ends[0].date, "2026-10-05");
        let ms = |s: &str| s.parse::<Timestamp>().unwrap().as_millisecond();
        assert_eq!(plan.day_ends[0].end, ms("2026-10-06T02:00:00Z"));
        assert_eq!(plan.day_ends[1].date, "2026-10-06");
        // Sunday 25 Oct (the fall-back night) ends Monday 05:00 EET = 03:00 UTC.
        let sunday = plan
            .day_ends
            .iter()
            .find(|d| d.date == "2026-10-25")
            .unwrap();
        assert_eq!(sunday.end, ms("2026-10-26T03:00:00Z"));
    }
}
