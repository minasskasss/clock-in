//! The business day (SPEC §5) and where a block falls inside it (SPEC §4.2).

use crate::shop_time::{add_days, resolve_local_at, shop_datetime};
use jiff::Timestamp;
use jiff::civil::{Date, DateTime, Time};
use serde::{Deserialize, Serialize};

/// Minutes in a civil day.
pub(crate) const DAY_MINUTES: u32 = 24 * 60;

/// The instant business day `date` begins: `rollover` on `date`, Greek time,
/// with the DST rules of [`crate::resolve_local`].
#[must_use]
pub fn business_day_start(date: Date, rollover: Time) -> Option<Timestamp> {
    resolve_local_at(date, rollover)
}

/// The business date that contains `now`.
///
/// Business day D runs from its start (rollover on D) up to, not including,
/// the start of D+1. Comparing instants (not wall-clock times) keeps this
/// monotonic through the fall-back hour, even with a rollover inside it.
#[must_use]
pub fn business_date_for(now: Timestamp, rollover: Time) -> Date {
    let calendar_date = shop_datetime(now).date();
    match business_day_start(calendar_date, rollover) {
        Some(start) if now < start => calendar_date.yesterday().unwrap_or(calendar_date),
        _ => calendar_date,
    }
}

/// Where a block sits in its business day, in wall-clock minutes from
/// midnight at the start of the business date.
///
/// - A start earlier than the rollover is on the following night (+1 day).
/// - An end earlier than or equal to the start is on the following calendar
///   day. Start equal to end therefore reads as 24 hours; validation rejects
///   it, but stored data never makes the logic fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockLayout {
    /// Minutes from midnight of the business date to the start (0–2879).
    pub start_minute: u32,
    /// Minutes from midnight of the business date to the end.
    pub end_minute: u32,
    /// Show "(+1)" next to the start time.
    pub start_next_day: bool,
    /// Show "(+1)" next to the end time. An end of exactly 00:00 at the
    /// midnight that closes the business date reads as "midnight", not +1.
    pub end_next_day: bool,
}

impl BlockLayout {
    /// Lays out a block with wall-clock `start` and `end` (seconds ignored).
    #[must_use]
    pub fn new(start: Time, end: Time, rollover: Time) -> Self {
        let start_of_day = minute_of_day(start);
        let next_night = start_of_day < minute_of_day(rollover);
        let start_minute = start_of_day + if next_night { DAY_MINUTES } else { 0 };
        let mut duration = (minute_of_day(end) + DAY_MINUTES - start_of_day) % DAY_MINUTES;
        if duration == 0 {
            duration = DAY_MINUTES;
        }
        let end_minute = start_minute + duration;
        Self {
            start_minute,
            end_minute,
            start_next_day: start_minute >= DAY_MINUTES,
            end_next_day: end_minute > DAY_MINUTES,
        }
    }

    /// Wall-clock length of the block in minutes.
    #[must_use]
    pub fn duration_minutes(self) -> u32 {
        self.end_minute - self.start_minute
    }

    /// Whether the two blocks share any minute (touching ends don't count).
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.start_minute < other.end_minute && other.start_minute < self.end_minute
    }

    /// The wall-clock start on business date `date`.
    #[must_use]
    pub fn start_on(self, date: Date) -> Option<DateTime> {
        civil_at(date, self.start_minute)
    }

    /// The wall-clock end on business date `date`.
    #[must_use]
    pub fn end_on(self, date: Date) -> Option<DateTime> {
        civil_at(date, self.end_minute)
    }
}

fn minute_of_day(time: Time) -> u32 {
    // Hour and minute of a valid `Time` are never negative.
    u32::try_from(time.hour()).unwrap_or(0) * 60 + u32::try_from(time.minute()).unwrap_or(0)
}

fn civil_at(date: Date, minutes: u32) -> Option<DateTime> {
    let day = add_days(date, i64::from(minutes / DAY_MINUTES))?;
    let rest = minutes % DAY_MINUTES;
    // `rest` < 1440, so hour < 24 and minute < 60.
    let time = Time::new(
        i8::try_from(rest / 60).ok()?,
        i8::try_from(rest % 60).ok()?,
        0,
        0,
    )
    .ok()?;
    Some(DateTime::from_parts(day, time))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, time};

    fn utc(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    const FIVE: Time = Time::constant(5, 0, 0, 0);

    #[test]
    fn rollover_boundary_04_59_05_00_05_01() {
        // Summer: 05:00 EEST = 02:00 UTC.
        let cases = [
            ("2026-06-02T01:59:00Z", date(2026, 6, 1)), // Tue 04:59 → Mon
            ("2026-06-02T01:59:59Z", date(2026, 6, 1)), // Tue 04:59:59 → Mon
            ("2026-06-02T02:00:00Z", date(2026, 6, 2)), // Tue 05:00 → Tue
            ("2026-06-02T02:01:00Z", date(2026, 6, 2)), // Tue 05:01 → Tue
        ];
        for (now, expected) in cases {
            assert_eq!(business_date_for(utc(now), FIVE), expected, "{now}");
        }
    }

    #[test]
    fn small_hours_belong_to_the_previous_business_day() {
        // SPEC §5: at 01:30 on Tuesday the Today view still shows Monday.
        assert_eq!(
            business_date_for(utc("2026-06-01T22:30:00Z"), FIVE), // Tue 01:30
            date(2026, 6, 1)
        );
        // Calendar midnight is not a boundary.
        assert_eq!(
            business_date_for(utc("2026-06-01T21:00:00Z"), FIVE), // Tue 00:00
            date(2026, 6, 1)
        );
        assert_eq!(
            business_date_for(utc("2026-06-01T20:59:00Z"), FIVE), // Mon 23:59
            date(2026, 6, 1)
        );
    }

    #[test]
    fn rollover_at_midnight_matches_the_calendar() {
        let midnight = time(0, 0, 0, 0);
        assert_eq!(
            business_date_for(utc("2026-06-01T20:59:00Z"), midnight), // Mon 23:59
            date(2026, 6, 1)
        );
        assert_eq!(
            business_date_for(utc("2026-06-01T21:00:00Z"), midnight), // Tue 00:00
            date(2026, 6, 2)
        );
    }

    #[test]
    fn rollover_at_08_00() {
        let eight = time(8, 0, 0, 0);
        assert_eq!(
            business_date_for(utc("2026-06-02T04:59:00Z"), eight), // Tue 07:59
            date(2026, 6, 1)
        );
        assert_eq!(
            business_date_for(utc("2026-06-02T05:00:00Z"), eight), // Tue 08:00
            date(2026, 6, 2)
        );
    }

    #[test]
    fn fall_back_with_rollover_inside_the_repeated_hour_stays_monotonic() {
        // 2026-10-25, rollover 03:30: the business day starts at the first 03:30
        // (00:30 UTC). The second pass through 03:00–03:29 stays in the new day.
        let rollover = time(3, 30, 0, 0);
        assert_eq!(
            business_date_for(utc("2026-10-25T00:15:00Z"), rollover), // first 03:15
            date(2026, 10, 24)
        );
        assert_eq!(
            business_date_for(utc("2026-10-25T00:30:00Z"), rollover), // first 03:30
            date(2026, 10, 25)
        );
        assert_eq!(
            business_date_for(utc("2026-10-25T01:15:00Z"), rollover), // second 03:15
            date(2026, 10, 25)
        );
    }

    #[test]
    fn spring_forward_with_rollover_inside_the_gap() {
        // 2027-03-28, rollover 03:30 does not exist; the day starts at 04:00.
        let rollover = time(3, 30, 0, 0);
        assert_eq!(
            business_day_start(date(2027, 3, 28), rollover),
            Some(utc("2027-03-28T01:00:00Z"))
        );
        assert_eq!(
            business_date_for(utc("2027-03-28T00:59:00Z"), rollover), // 02:59
            date(2027, 3, 27)
        );
        assert_eq!(
            business_date_for(utc("2027-03-28T01:00:00Z"), rollover), // 04:00
            date(2027, 3, 28)
        );
    }

    #[test]
    fn business_day_start_handles_dst_days() {
        assert_eq!(
            business_day_start(date(2026, 10, 25), FIVE),
            Some(utc("2026-10-25T03:00:00Z"))
        );
        assert_eq!(
            business_day_start(date(2027, 3, 28), FIVE),
            Some(utc("2027-03-28T02:00:00Z"))
        );
    }

    #[test]
    fn layout_table() {
        // (start, end, rollover) → (start_minute, end_minute, start +1, end +1)
        let cases = [
            ((9, 0), (17, 0), (5, 0), (540, 1020, false, false)),
            ((12, 0), (16, 0), (5, 0), (720, 960, false, false)),
            ((19, 0), (0, 0), (5, 0), (1140, 1440, false, false)), // ends at midnight
            ((19, 0), (2, 0), (5, 0), (1140, 1560, false, true)),  // ends after midnight
            ((18, 0), (2, 0), (5, 0), (1080, 1560, false, true)),
            ((1, 0), (3, 0), (5, 0), (1500, 1620, true, true)), // starts after midnight
            ((0, 0), (4, 0), (5, 0), (1440, 1680, true, true)),
            ((4, 59), (6, 0), (5, 0), (1739, 1800, true, true)), // just before rollover
            ((5, 0), (6, 0), (5, 0), (300, 360, false, false)),  // exactly at rollover
            ((0, 0), (4, 0), (0, 0), (0, 240, false, false)),    // rollover at midnight
            ((22, 0), (6, 0), (8, 0), (1320, 1800, false, true)),
            ((9, 0), (9, 0), (5, 0), (540, 1980, false, true)), // start == end reads as 24 h
        ];
        for ((sh, sm), (eh, em), (rh, rm), expected) in cases {
            let l = BlockLayout::new(time(sh, sm, 0, 0), time(eh, em, 0, 0), time(rh, rm, 0, 0));
            assert_eq!(
                (
                    l.start_minute,
                    l.end_minute,
                    l.start_next_day,
                    l.end_next_day
                ),
                expected,
                "{sh:02}:{sm:02}–{eh:02}:{em:02} rollover {rh:02}:{rm:02}"
            );
        }
    }

    #[test]
    fn layout_duration_and_overlap() {
        let r = FIVE;
        let lunch = BlockLayout::new(time(12, 0, 0, 0), time(16, 0, 0, 0), r);
        let evening = BlockLayout::new(time(19, 0, 0, 0), time(0, 0, 0, 0), r);
        let touching = BlockLayout::new(time(16, 0, 0, 0), time(19, 0, 0, 0), r);
        let night = BlockLayout::new(time(1, 0, 0, 0), time(3, 0, 0, 0), r);
        let late = BlockLayout::new(time(23, 0, 0, 0), time(2, 0, 0, 0), r);
        assert_eq!(lunch.duration_minutes(), 240);
        assert_eq!(evening.duration_minutes(), 300);
        assert!(!lunch.overlaps(evening));
        assert!(!lunch.overlaps(touching));
        assert!(!touching.overlaps(evening));
        // 01:00 is the following night, so it is after the lunch block…
        assert!(!lunch.overlaps(night));
        // …and inside 23:00–02:00.
        assert!(late.overlaps(night));
        assert!(night.overlaps(late));
    }

    #[test]
    fn layout_civil_start_and_end() {
        let l = BlockLayout::new(time(19, 0, 0, 0), time(2, 0, 0, 0), FIVE);
        let d = date(2026, 6, 1);
        assert_eq!(l.start_on(d), Some(date(2026, 6, 1).at(19, 0, 0, 0)));
        assert_eq!(l.end_on(d), Some(date(2026, 6, 2).at(2, 0, 0, 0)));
        let night = BlockLayout::new(time(1, 0, 0, 0), time(3, 0, 0, 0), FIVE);
        assert_eq!(night.start_on(d), Some(date(2026, 6, 2).at(1, 0, 0, 0)));
        // Month and year ends roll over correctly.
        assert_eq!(
            l.end_on(date(2026, 12, 31)),
            Some(date(2027, 1, 1).at(2, 0, 0, 0))
        );
    }

    #[test]
    fn layout_ignores_seconds() {
        let l = BlockLayout::new(time(9, 0, 30, 0), time(17, 0, 59, 0), time(5, 0, 30, 0));
        assert_eq!((l.start_minute, l.end_minute), (540, 1020));
    }
}
