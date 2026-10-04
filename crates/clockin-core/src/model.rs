//! Plain data types the core works on. They mirror the server snapshot
//! (ARCHITECTURE §4 and §6, `get_snapshot`).

use jiff::Timestamp;
use jiff::civil::{Date, Time, Weekday};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A staff member. Removed staff stay in the snapshot while marks reference
/// them; see [`Staff::works_block_starting`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Staff {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub removed_at: Option<Timestamp>,
}

impl Staff {
    /// "First Last", as shown on alarms.
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
    }

    /// Whether a block starting at `start` still belongs to this person.
    ///
    /// A removed person disappears "from the next block onward" (SPEC §4.1):
    /// a block that started before the removal stays, later ones don't.
    #[must_use]
    pub fn works_block_starting(&self, start: Timestamp) -> bool {
        self.removed_at.is_none_or(|removed| start < removed)
    }
}

/// A block of the weekly template. `weekday` is a **business** day (SPEC §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyBlock {
    pub id: Uuid,
    pub staff_id: Uuid,
    #[serde(with = "weekday_number")]
    pub weekday: Weekday,
    pub start: Time,
    pub end: Time,
}

/// What a one-off change does to a person's business day (SPEC §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverrideKind {
    /// Day off: no blocks that day.
    Off,
    /// Replace hours: the override's own blocks instead of the template.
    Replace,
}

/// A one-off change for one person on one business date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    pub id: Uuid,
    pub staff_id: Uuid,
    pub business_date: Date,
    pub kind: OverrideKind,
    /// The replacement blocks; empty for [`OverrideKind::Off`].
    pub blocks: Vec<OverrideBlock>,
}

/// A block of a replace-hours override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideBlock {
    pub id: Uuid,
    pub start: Time,
    pub end: Time,
}

/// The settings the time logic needs (SPEC §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSettings {
    /// Minutes relative to block start; negative means before.
    pub checkin_offset_min: i32,
    /// Minutes relative to block end; negative means before.
    pub checkout_offset_min: i32,
    /// Business-day rollover time.
    pub rollover: Time,
}

impl Default for ScheduleSettings {
    fn default() -> Self {
        Self {
            checkin_offset_min: 0,
            checkout_offset_min: 0,
            rollover: Time::constant(5, 0, 0, 0),
        }
    }
}

/// Check-in or check-out: the kind of a mark, and of an alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkKind {
    In,
    Out,
}

impl MarkKind {
    /// The lowercase name used on the wire and in plan item ids.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

/// A check-in or check-out mark for one block occurrence. Voided marks are
/// never passed to the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub id: Uuid,
    pub staff_id: Uuid,
    pub source_block_id: Uuid,
    pub business_date: Date,
    pub kind: MarkKind,
}

/// Everything the core needs to build the Today view and the alarm plan.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub staff: Vec<Staff>,
    pub blocks: Vec<WeeklyBlock>,
    pub overrides: Vec<Override>,
    pub settings: ScheduleSettings,
    /// Non-voided marks, including ones still queued locally.
    pub marks: Vec<Mark>,
}

/// Serialises a [`Weekday`] as its ISO number, Monday = 1 … Sunday = 7
/// (the `smallint` stored on the server).
pub mod weekday_number {
    use jiff::civil::Weekday;
    use serde::{Deserialize, Deserializer, Serializer, de};

    /// # Errors
    ///
    /// Propagates the serializer's error.
    pub fn serialize<S: Serializer>(weekday: &Weekday, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i8(weekday.to_monday_one_offset())
    }

    /// # Errors
    ///
    /// Fails for numbers outside 1–7.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Weekday, D::Error> {
        let n = i8::deserialize(deserializer)?;
        Weekday::from_monday_one_offset(n)
            .map_err(|_| de::Error::custom(format!("weekday must be 1–7, got {n}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, time};

    #[test]
    fn display_name_is_first_then_last() {
        let s = Staff {
            id: Uuid::nil(),
            first_name: "Μαρία".into(),
            last_name: "Παπαδοπούλου".into(),
            removed_at: None,
        };
        assert_eq!(s.display_name(), "Μαρία Παπαδοπούλου");
    }

    #[test]
    fn removed_staff_keep_only_blocks_that_started_before_removal() {
        let removed: Timestamp = "2026-06-01T10:00:00Z".parse().unwrap();
        let s = Staff {
            id: Uuid::nil(),
            first_name: "A".into(),
            last_name: "B".into(),
            removed_at: Some(removed),
        };
        assert!(s.works_block_starting("2026-06-01T09:59:00Z".parse().unwrap()));
        assert!(!s.works_block_starting(removed));
        assert!(!s.works_block_starting("2026-06-01T12:00:00Z".parse().unwrap()));
    }

    #[test]
    fn json_round_trip_uses_server_shapes() {
        let block = WeeklyBlock {
            id: Uuid::from_u128(1),
            staff_id: Uuid::from_u128(2),
            weekday: Weekday::Sunday,
            start: time(19, 0, 0, 0),
            end: time(0, 0, 0, 0),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["weekday"], 7);
        assert_eq!(json["start"], "19:00:00");
        assert_eq!(serde_json::from_value::<WeeklyBlock>(json).unwrap(), block);

        let o: Override = serde_json::from_str(
            r#"{"id":"00000000-0000-0000-0000-000000000003",
                "staff_id":"00000000-0000-0000-0000-000000000002",
                "business_date":"2026-10-25","kind":"off","blocks":[]}"#,
        )
        .unwrap();
        assert_eq!(o.kind, OverrideKind::Off);
        assert_eq!(o.business_date, date(2026, 10, 25));

        let m: Mark = serde_json::from_str(
            r#"{"id":"00000000-0000-0000-0000-000000000004",
                "staff_id":"00000000-0000-0000-0000-000000000002",
                "source_block_id":"00000000-0000-0000-0000-000000000001",
                "business_date":"2026-10-25","kind":"in"}"#,
        )
        .unwrap();
        assert_eq!(m.kind, MarkKind::In);
    }

    #[test]
    fn weekday_numbers_outside_1_to_7_are_rejected() {
        for bad in [0, 8, -1] {
            let json = format!(
                r#"{{"id":"00000000-0000-0000-0000-000000000001",
                    "staff_id":"00000000-0000-0000-0000-000000000002",
                    "weekday":{bad},"start":"09:00","end":"17:00"}}"#
            );
            assert!(serde_json::from_str::<WeeklyBlock>(&json).is_err(), "{bad}");
        }
    }

    #[test]
    fn default_settings_match_the_spec() {
        let s = ScheduleSettings::default();
        assert_eq!(s.checkin_offset_min, 0);
        assert_eq!(s.checkout_offset_min, 0);
        assert_eq!(s.rollover, time(5, 0, 0, 0));
    }
}
