//! The plan the Android alarm code schedules from (ARCHITECTURE §10).
//! Compiled for Android and for the tests.

use clockin_core::{Snapshot, alarm_plan, is_suppressed, plan_window};
use jiff::Timestamp;
use tauri_plugin_clockin_alarm::{BridgeItem, BridgePlan};

/// The 14-day plan window from `snapshot`, without the alarms that marks
/// known on this device (including ones not yet synced) already make
/// unnecessary. No snapshot (unpaired, or never synced): no alarms.
#[must_use]
pub fn bridge_plan(
    snapshot: Option<&Snapshot>,
    alert_mode: &'static str,
    config_version: i64,
    now: Timestamp,
) -> BridgePlan {
    let (from, until) = plan_window(now);
    let items = snapshot.map_or_else(Vec::new, |s| {
        alarm_plan(s, from, until)
            .into_iter()
            .filter(|item| !is_suppressed(item, &s.marks))
            .map(|item| BridgeItem {
                item_id: item.item_id,
                fires_at: item.fires_at.to_string(),
                kind: item.kind.as_str(),
                name: item.display_name,
            })
            .collect()
    });
    BridgePlan {
        items,
        alert_mode,
        config_version,
        horizon_end: snapshot.is_some().then(|| until.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clockin_core::{Mark, MarkKind, Staff, WeeklyBlock};
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

    /// Tuesday 2026-10-06, 09:00 in Athens.
    fn now() -> Timestamp {
        "2026-10-06T06:00:00Z".parse().unwrap()
    }

    #[test]
    fn sends_the_plan_window_with_utc_times() {
        let plan = bridge_plan(Some(&snapshot()), "ring", 4, now());
        // Two Tuesdays in the 14-day window, check-in and check-out each.
        assert_eq!(plan.items.len(), 4);
        assert_eq!(plan.items[0].fires_at, "2026-10-06T09:00:00Z");
        assert_eq!(plan.items[0].kind, "in");
        assert_eq!(plan.items[0].name, "Μαρία Παππά");
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
        let plan = bridge_plan(Some(&s), "notification", 4, now());
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.items[0].kind, "out");
        assert_eq!(plan.alert_mode, "notification");
    }

    #[test]
    fn no_data_means_no_alarms() {
        let plan = bridge_plan(None, "ring", 0, now());
        assert!(plan.items.is_empty());
        assert_eq!(plan.horizon_end, None);
    }
}
