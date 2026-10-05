//! What the screens show, built from the cached snapshot with the core's
//! rules. Times are already Greek wall-clock strings ("HH:MM"), so the UI
//! never does time arithmetic.

use crate::drafts::{hhmm, layout_flags};
use clockin_core::{
    MarkKind, OverrideKind, RowStatus, Snapshot, business_date_for, shop_datetime, today_view,
};
use clockin_sync::{Platform, ServerSnapshot, SyncStatus};
use jiff::Timestamp;
use jiff::civil::Time;
use serde::Serialize;
use uuid::Uuid;

/// A Greek wall-clock date and time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalStamp {
    /// ISO date, e.g. "2026-10-05".
    pub date: String,
    /// "HH:MM".
    pub time: String,
}

impl LocalStamp {
    #[must_use]
    pub fn at(instant: Timestamp) -> Self {
        let dt = shop_datetime(instant);
        Self {
            date: dt.date().to_string(),
            time: hhmm(dt.time()),
        }
    }
}

/// One row of the Today view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowView {
    /// Stable React key: block and business date.
    pub key: String,
    pub staff_id: Uuid,
    pub source_block_id: Uuid,
    pub business_date: String,
    pub first_name: String,
    pub last_name: String,
    pub start: String,
    pub end: String,
    pub start_next_day: bool,
    pub end_next_day: bool,
    pub status: RowStatus,
    pub next_mark: Option<MarkKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayView {
    /// The business date shown (the header's weekday and date).
    pub business_date: String,
    /// The live clock, "HH:MM", Greek time.
    pub clock: String,
    pub rows: Vec<RowView>,
}

/// Builds the Today view for `now`.
#[must_use]
pub fn today(snapshot: &Snapshot, now: Timestamp) -> TodayView {
    let view = today_view(snapshot, now);
    TodayView {
        business_date: view.business_date.to_string(),
        clock: hhmm(view.clock),
        rows: view
            .rows
            .into_iter()
            .map(|r| {
                let o = r.occurrence;
                RowView {
                    key: format!("{}:{}", o.source_block_id, o.business_date),
                    staff_id: o.staff_id,
                    source_block_id: o.source_block_id,
                    business_date: o.business_date.to_string(),
                    first_name: o.first_name,
                    last_name: o.last_name,
                    start: hhmm(o.start_time),
                    end: hhmm(o.end_time),
                    start_next_day: o.start_next_day,
                    end_next_day: o.end_next_day,
                    status: r.status,
                    next_mark: r.next_mark,
                }
            })
            .collect(),
    }
}

/// The banners of SPEC §6 that this build can know about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Banners {
    pub offline: bool,
    pub last_sync: Option<LocalStamp>,
    pub clock_skew: bool,
    pub horizon_short: bool,
    /// Windows sound output muted, at zero or missing.
    pub sound_off: bool,
}

#[must_use]
pub fn banners(status: &SyncStatus, now: Timestamp) -> Banners {
    Banners {
        offline: !status.online && status.consecutive_failures > 0,
        last_sync: status.last_sync.map(LocalStamp::at),
        clock_skew: status.online && status.clock_skew,
        // Offline, a known horizon still counts; an unknown one is unknown.
        horizon_short: (status.online || status.plan_horizon_end.is_some())
            && clockin_core::horizon_short(status.plan_horizon_end, now),
        sound_off: false,
    }
}

// --- Settings -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockView {
    pub id: Uuid,
    pub start: String,
    pub end: String,
    pub start_next_day: bool,
    pub end_next_day: bool,
}

impl BlockView {
    fn new(id: Uuid, start: Time, end: Time, rollover: Time) -> Self {
        let (start_next_day, end_next_day) = layout_flags(start, end, rollover);
        Self {
            id,
            start: hhmm(start),
            end: hhmm(end),
            start_next_day,
            end_next_day,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekBlockView {
    /// 1 = Monday … 7 = Sunday.
    pub weekday: i8,
    #[serde(flatten)]
    pub block: BlockView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffView {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    /// Ordered by weekday, then business-day position.
    pub blocks: Vec<WeekBlockView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideView {
    pub id: Uuid,
    pub staff_id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub business_date: String,
    pub kind: OverrideKind,
    pub blocks: Vec<BlockView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub checkin_offset_min: i32,
    pub checkout_offset_min: i32,
    pub rollover: String,
    pub autostart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkView {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub kind: MarkKind,
    pub marked_at: LocalStamp,
    /// The block's hours, if it is still in the schedule.
    pub block: Option<BlockView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: Uuid,
    pub name: String,
    pub platform: Platform,
    pub paired_at: LocalStamp,
    pub last_seen: Option<LocalStamp>,
    pub this_device: bool,
}

/// Everything the Settings screen lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminView {
    /// Today's business date: the earliest date a one-off change may have.
    pub today: String,
    /// Active staff, by last name.
    pub staff: Vec<StaffView>,
    /// Today's and later one-off changes, by date.
    pub overrides: Vec<OverrideView>,
    pub settings: SettingsView,
    /// Today's live marks, oldest first.
    pub marks: Vec<MarkView>,
    pub devices: Vec<DeviceView>,
}

fn name_key(last: &str, first: &str) -> (String, String) {
    (last.to_lowercase(), first.to_lowercase())
}

#[must_use]
pub fn admin(snapshot: &ServerSnapshot, now: Timestamp) -> AdminView {
    let rollover = snapshot.settings.rollover;
    let today = business_date_for(now, rollover);
    let name_of = |id: Uuid| {
        snapshot
            .staff
            .iter()
            .find(|s| s.id == id)
            .map(|s| (s.first_name.clone(), s.last_name.clone()))
            .unwrap_or_default()
    };

    let mut staff: Vec<StaffView> = snapshot
        .staff
        .iter()
        .filter(|s| s.removed_at.is_none())
        .map(|s| {
            let mut blocks: Vec<_> = snapshot
                .blocks
                .iter()
                .filter(|b| b.staff_id == s.id)
                .map(|b| {
                    let order =
                        clockin_core::BlockLayout::new(b.start, b.end, rollover).start_minute;
                    (
                        (b.weekday.to_monday_one_offset(), order),
                        WeekBlockView {
                            weekday: b.weekday.to_monday_one_offset(),
                            block: BlockView::new(b.id, b.start, b.end, rollover),
                        },
                    )
                })
                .collect();
            blocks.sort_by_key(|(key, _)| *key);
            StaffView {
                id: s.id,
                first_name: s.first_name.clone(),
                last_name: s.last_name.clone(),
                blocks: blocks.into_iter().map(|(_, b)| b).collect(),
            }
        })
        .collect();
    staff.sort_by_key(|s| name_key(&s.last_name, &s.first_name));

    let mut overrides: Vec<OverrideView> = snapshot
        .overrides
        .iter()
        .filter(|o| o.business_date >= today)
        .map(|o| {
            let (first_name, last_name) = name_of(o.staff_id);
            OverrideView {
                id: o.id,
                staff_id: o.staff_id,
                first_name,
                last_name,
                business_date: o.business_date.to_string(),
                kind: o.kind,
                blocks: o
                    .blocks
                    .iter()
                    .map(|b| BlockView::new(b.id, b.start, b.end, rollover))
                    .collect(),
            }
        })
        .collect();
    overrides.sort_by(|a, b| {
        a.business_date.cmp(&b.business_date).then_with(|| {
            name_key(&a.last_name, &a.first_name).cmp(&name_key(&b.last_name, &b.first_name))
        })
    });

    let block_of = |id: Uuid| {
        snapshot
            .blocks
            .iter()
            .find(|b| b.id == id)
            .map(|b| BlockView::new(b.id, b.start, b.end, rollover))
            .or_else(|| {
                snapshot
                    .overrides
                    .iter()
                    .flat_map(|o| &o.blocks)
                    .find(|b| b.id == id)
                    .map(|b| BlockView::new(b.id, b.start, b.end, rollover))
            })
    };
    let mut marks: Vec<_> = snapshot
        .marks
        .iter()
        .filter(|m| m.business_date == today)
        .collect();
    marks.sort_by_key(|m| (m.marked_at, m.id));
    let marks = marks
        .into_iter()
        .map(|m| {
            let (first_name, last_name) = name_of(m.staff_id);
            MarkView {
                id: m.id,
                first_name,
                last_name,
                kind: m.kind,
                marked_at: LocalStamp::at(m.marked_at),
                block: block_of(m.source_block_id),
            }
        })
        .collect();

    let mut devices: Vec<DeviceView> = snapshot
        .devices
        .iter()
        .map(|d| DeviceView {
            id: d.id,
            name: d.name.clone(),
            platform: d.platform,
            paired_at: LocalStamp::at(d.created_at),
            last_seen: d.last_seen_at.map(LocalStamp::at),
            this_device: d.id == snapshot.this_device_id,
        })
        .collect();
    devices.sort_by_key(|d| {
        (
            !d.this_device,
            d.paired_at.date.clone(),
            d.paired_at.time.clone(),
        )
    });

    AdminView {
        today: today.to_string(),
        staff,
        overrides,
        settings: SettingsView {
            checkin_offset_min: snapshot.settings.checkin_offset_min,
            checkout_offset_min: snapshot.settings.checkout_offset_min,
            rollover: hhmm(rollover),
            autostart: snapshot.settings.autostart,
        },
        marks,
        devices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clockin_core::{Override, OverrideBlock, Staff, WeeklyBlock};
    use clockin_sync::{DeviceInfo, ServerMark, ServerSettings};
    use jiff::civil::{Weekday, date};

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn t(h: i8, m: i8) -> Time {
        Time::constant(h, m, 0, 0)
    }

    /// Monday 2026-10-05, 13:00 in Athens.
    fn now() -> Timestamp {
        ts("2026-10-05T10:00:00Z")
    }

    fn person(n: u128, first: &str, last: &str) -> Staff {
        Staff {
            id: id(n),
            first_name: first.into(),
            last_name: last.into(),
            removed_at: None,
        }
    }

    fn snapshot() -> ServerSnapshot {
        ServerSnapshot {
            staff: vec![
                person(1, "Babis", "Beta"),
                person(2, "Anna", "Alpha"),
                Staff {
                    removed_at: Some(ts("2026-10-04T10:00:00Z")),
                    ..person(3, "Gone", "Away")
                },
            ],
            blocks: vec![
                WeeklyBlock {
                    id: id(11),
                    staff_id: id(1),
                    weekday: Weekday::Monday,
                    start: t(19, 0),
                    end: t(2, 0),
                },
                WeeklyBlock {
                    id: id(10),
                    staff_id: id(1),
                    weekday: Weekday::Monday,
                    start: t(12, 0),
                    end: t(16, 0),
                },
                WeeklyBlock {
                    id: id(20),
                    staff_id: id(2),
                    weekday: Weekday::Tuesday,
                    start: t(9, 0),
                    end: t(17, 0),
                },
            ],
            overrides: vec![
                Override {
                    id: id(30),
                    staff_id: id(2),
                    business_date: date(2026, 10, 7),
                    kind: OverrideKind::Replace,
                    blocks: vec![OverrideBlock {
                        id: id(31),
                        start: t(10, 0),
                        end: t(14, 0),
                    }],
                },
                Override {
                    id: id(32),
                    staff_id: id(2),
                    business_date: date(2026, 10, 4),
                    kind: OverrideKind::Off,
                    blocks: vec![],
                },
            ],
            settings: ServerSettings {
                checkin_offset_min: -5,
                checkout_offset_min: 0,
                rollover: t(5, 0),
                autostart: true,
                quit_code: None,
            },
            marks: vec![
                ServerMark {
                    id: id(40),
                    staff_id: id(1),
                    source_block_id: id(10),
                    business_date: date(2026, 10, 5),
                    kind: MarkKind::In,
                    marked_at: ts("2026-10-05T08:58:00Z"),
                    device_id: Some(id(50)),
                },
                ServerMark {
                    id: id(41),
                    staff_id: id(1),
                    source_block_id: id(10),
                    business_date: date(2026, 10, 4),
                    kind: MarkKind::In,
                    marked_at: ts("2026-10-04T08:58:00Z"),
                    device_id: None,
                },
            ],
            devices: vec![
                DeviceInfo {
                    id: id(51),
                    name: "Phone".into(),
                    platform: Platform::Android,
                    created_at: ts("2026-10-01T10:00:00Z"),
                    last_seen_at: None,
                },
                DeviceInfo {
                    id: id(50),
                    name: "Shop PC".into(),
                    platform: Platform::Windows,
                    created_at: ts("2026-10-02T10:00:00Z"),
                    last_seen_at: Some(ts("2026-10-05T09:59:00Z")),
                },
            ],
            this_device_id: id(50),
            config_version: 1,
            data_version: 1,
            plan_config_version: 1,
            plan_horizon_end: None,
            server_now: now(),
        }
    }

    #[test]
    fn today_rows_carry_display_strings() {
        let view = today(&snapshot().core_snapshot(&[]), now());
        assert_eq!(view.business_date, "2026-10-05");
        assert_eq!(view.clock, "13:00");
        assert_eq!(view.rows.len(), 2);
        let lunch = &view.rows[0];
        assert_eq!(
            (lunch.start.as_str(), lunch.end.as_str()),
            ("12:00", "16:00")
        );
        assert_eq!(lunch.status, RowStatus::CheckedIn);
        assert_eq!(lunch.next_mark, Some(MarkKind::Out));
        assert_eq!(lunch.key, format!("{}:2026-10-05", id(10)));
        let evening = &view.rows[1];
        assert_eq!(evening.end, "02:00");
        assert!(evening.end_next_day);
        assert_eq!(evening.status, RowStatus::Pending);

        let json = serde_json::to_value(lunch).unwrap();
        assert_eq!(json["status"], "checked_in");
        assert_eq!(json["nextMark"], "out");
        assert_eq!(json["startNextDay"], false);
    }

    #[test]
    fn admin_view_lists_what_settings_needs() {
        let view = admin(&snapshot(), now());
        assert_eq!(view.today, "2026-10-05");
        // Active staff only, by last name; blocks by weekday then time.
        let names: Vec<_> = view.staff.iter().map(|s| s.last_name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Beta"]);
        let beta = &view.staff[1];
        assert_eq!(beta.blocks[0].block.start, "12:00");
        assert_eq!(beta.blocks[1].block.end, "02:00");
        assert!(beta.blocks[1].block.end_next_day);
        // Past overrides are left out.
        assert_eq!(view.overrides.len(), 1);
        assert_eq!(view.overrides[0].business_date, "2026-10-07");
        assert_eq!(view.overrides[0].first_name, "Anna");
        // Only today's marks, with the block's hours and Greek time.
        assert_eq!(view.marks.len(), 1);
        assert_eq!(view.marks[0].marked_at.time, "11:58");
        assert_eq!(view.marks[0].block.as_ref().unwrap().start, "12:00");
        assert_eq!(view.settings.rollover, "05:00");
        assert_eq!(view.settings.checkin_offset_min, -5);
        // This device first.
        assert!(view.devices[0].this_device);
        assert_eq!(view.devices[0].last_seen.as_ref().unwrap().time, "12:59");
        assert_eq!(view.devices[1].name, "Phone");
    }

    #[test]
    fn banners_need_a_failed_contact_to_show_offline() {
        let mut status = SyncStatus {
            online: false,
            paired: true,
            last_sync: None,
            consecutive_failures: 0,
            clock_skew: false,
            plan_horizon_end: None,
            pending_marks: 0,
        };
        // Just started: not "offline" yet.
        assert!(!banners(&status, now()).offline);
        status.consecutive_failures = 1;
        status.last_sync = Some(ts("2026-10-05T09:00:00Z"));
        let b = banners(&status, now());
        assert!(b.offline);
        assert_eq!(b.last_sync.unwrap().time, "12:00");
        // Offline: skew and an unknown horizon don't show.
        status.clock_skew = true;
        let b = banners(&status, now());
        assert!(!b.clock_skew && !b.horizon_short);
        status.plan_horizon_end = Some(ts("2026-10-06T10:00:00Z"));
        assert!(banners(&status, now()).horizon_short);
        status.plan_horizon_end = None;
        status.online = true;
        status.consecutive_failures = 0;
        let b = banners(&status, now());
        assert!(b.clock_skew && b.horizon_short && !b.offline);
        status.plan_horizon_end = Some(ts("2026-10-19T10:00:00Z"));
        assert!(!banners(&status, now()).horizon_short);
    }
}
