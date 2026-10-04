//! Shared fixtures for the unit tests.

use crate::model::{Mark, MarkKind, Override, OverrideBlock, OverrideKind, Staff, WeeklyBlock};
use crate::shop_time::resolve_local_at;
use jiff::Timestamp;
use jiff::civil::{Date, Time, Weekday, date, time};
use uuid::Uuid;

/// A Monday in summer time (EEST, UTC+3).
pub const MONDAY: Date = date(2026, 6, 1);
pub const TUESDAY: Date = date(2026, 6, 2);
/// The default rollover.
pub const FIVE: Time = time(5, 0, 0, 0);

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

pub fn utc(s: &str) -> Timestamp {
    s.parse().unwrap()
}

/// The instant of Greek wall-clock `h:m` on `day`.
pub fn athens(day: Date, h: i8, m: i8) -> Timestamp {
    resolve_local_at(day, time(h, m, 0, 0)).unwrap()
}

pub fn staff(n: u128, first: &str, last: &str) -> Staff {
    Staff {
        id: id(n),
        first_name: first.into(),
        last_name: last.into(),
        removed_at: None,
    }
}

pub fn weekly(block: u128, who: &Staff, weekday: Weekday, s: (i8, i8), e: (i8, i8)) -> WeeklyBlock {
    WeeklyBlock {
        id: id(block),
        staff_id: who.id,
        weekday,
        start: time(s.0, s.1, 0, 0),
        end: time(e.0, e.1, 0, 0),
    }
}

pub fn day_off(n: u128, who: &Staff, day: Date) -> Override {
    Override {
        id: id(n),
        staff_id: who.id,
        business_date: day,
        kind: OverrideKind::Off,
        blocks: Vec::new(),
    }
}

/// (block id, start (h, m), end (h, m)).
pub type BlockSpec = (u128, (i8, i8), (i8, i8));

pub fn replace(n: u128, who: &Staff, day: Date, blocks: &[BlockSpec]) -> Override {
    Override {
        id: id(n),
        staff_id: who.id,
        business_date: day,
        kind: OverrideKind::Replace,
        blocks: blocks
            .iter()
            .map(|&(b, s, e)| OverrideBlock {
                id: id(b),
                start: time(s.0, s.1, 0, 0),
                end: time(e.0, e.1, 0, 0),
            })
            .collect(),
    }
}

pub fn mark(n: u128, who: u128, block: u128, day: Date, kind: MarkKind) -> Mark {
    Mark {
        id: id(n),
        staff_id: id(who),
        source_block_id: id(block),
        business_date: day,
        kind,
    }
}

#[test]
fn fixture_days_are_monday_and_tuesday() {
    assert_eq!(MONDAY.weekday(), Weekday::Monday);
    assert_eq!(TUESDAY.weekday(), Weekday::Tuesday);
}
