//! Converting between instants and Greek local (Europe/Athens) wall-clock time.
//!
//! Every civil date and time in the app means Europe/Athens, whatever the
//! device's own timezone setting is (SPEC §4.2).

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{SignedDuration, Timestamp};
use std::sync::OnceLock;

/// The IANA name of the only timezone the app works in.
pub const TIME_ZONE_NAME: &str = "Europe/Athens";

/// Returns the shop's timezone (Europe/Athens), from the bundled tz database.
///
/// # Panics
///
/// Never in practice: the tz database is compiled into the binary
/// (`jiff` feature `tzdb-bundle-always`), and a unit test checks the lookup.
#[must_use]
pub fn shop_time_zone() -> TimeZone {
    static TZ: OnceLock<TimeZone> = OnceLock::new();
    TZ.get_or_init(|| {
        TimeZone::get(TIME_ZONE_NAME).expect("bundled tz database contains Europe/Athens")
    })
    .clone()
}

/// The Greek wall-clock date and time at `instant`.
#[must_use]
pub fn shop_datetime(instant: Timestamp) -> DateTime {
    instant.to_zoned(shop_time_zone()).datetime()
}

/// The Greek wall-clock time at `instant`, truncated to the minute (for the
/// HH:MM clock in the Today view header).
#[must_use]
pub fn shop_clock(instant: Timestamp) -> Time {
    let time = shop_datetime(instant).time();
    Time::constant(time.hour(), time.minute(), 0, 0)
}

/// Turns a Greek wall-clock date and time into an instant, using the app's
/// daylight-saving rules (SPEC §7.5):
///
/// - a time that doesn't exist (spring-forward gap) moves to the next valid
///   minute, e.g. 03:30 on 2027-03-28 becomes 04:00;
/// - an ambiguous time (fall-back fold) takes its first occurrence.
///
/// Returns `None` only for dates outside jiff's supported range.
#[must_use]
pub fn resolve_local(datetime: DateTime) -> Option<Timestamp> {
    let tz = shop_time_zone();
    // Block times are whole minutes; drop anything finer so the gap walk below
    // lands exactly on a minute.
    let mut dt = DateTime::from_parts(
        datetime.date(),
        Time::constant(datetime.hour(), datetime.minute(), 0, 0),
    );
    // A gap is at most a couple of hours anywhere in the tz database; the bound
    // only guards against an impossible endless loop.
    for _ in 0..(48 * 60) {
        match tz.to_ambiguous_timestamp(dt).offset() {
            AmbiguousOffset::Unambiguous { offset } => return offset.to_timestamp(dt).ok(),
            AmbiguousOffset::Fold { before, .. } => return before.to_timestamp(dt).ok(),
            AmbiguousOffset::Gap { .. } => {
                dt = dt.checked_add(SignedDuration::from_mins(1)).ok()?;
            }
        }
    }
    None
}

/// The instant at Greek wall-clock `time` on `date`, with the DST rules of
/// [`resolve_local`].
#[must_use]
pub fn resolve_local_at(date: Date, time: Time) -> Option<Timestamp> {
    resolve_local(DateTime::from_parts(date, time))
}

/// Truncates an instant down to the start of its minute.
#[must_use]
pub fn floor_to_minute(instant: Timestamp) -> Timestamp {
    let second = instant.as_second().div_euclid(60) * 60;
    // Flooring keeps the value inside the range the original timestamp was in,
    // except at the very first representable minute; fall back to the input.
    Timestamp::from_second(second).unwrap_or(instant)
}

/// Adds whole days to a civil date, or `None` outside jiff's range.
pub(crate) fn add_days(date: Date, days: i64) -> Option<Date> {
    date.checked_add(jiff::Span::new().try_days(days).ok()?)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, time};

    fn utc(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn bundled_time_zone_is_available() {
        assert_eq!(shop_time_zone().iana_name(), Some(TIME_ZONE_NAME));
    }

    #[test]
    fn athens_offsets_across_2026_fall_back() {
        let summer = resolve_local(date(2026, 10, 24).at(12, 0, 0, 0)).unwrap();
        let winter = resolve_local(date(2026, 10, 26).at(12, 0, 0, 0)).unwrap();
        assert_eq!(summer, utc("2026-10-24T09:00:00Z"));
        assert_eq!(winter, utc("2026-10-26T10:00:00Z"));
    }

    #[test]
    fn plain_times_resolve_with_the_current_offset() {
        // Winter (EET, UTC+2) and summer (EEST, UTC+3).
        assert_eq!(
            resolve_local(date(2026, 1, 15).at(9, 0, 0, 0)),
            Some(utc("2026-01-15T07:00:00Z"))
        );
        assert_eq!(
            resolve_local(date(2026, 7, 15).at(9, 0, 0, 0)),
            Some(utc("2026-07-15T06:00:00Z"))
        );
    }

    #[test]
    fn spring_forward_gap_moves_to_the_next_valid_minute() {
        // 2027-03-28: 03:00 EET jumps to 04:00 EEST (01:00 UTC).
        for (h, m) in [(3, 0), (3, 1), (3, 30), (3, 59)] {
            assert_eq!(
                resolve_local(date(2027, 3, 28).at(h, m, 0, 0)),
                Some(utc("2027-03-28T01:00:00Z")),
                "{h:02}:{m:02}"
            );
        }
        // Just before and just after the gap are untouched.
        assert_eq!(
            resolve_local(date(2027, 3, 28).at(2, 59, 0, 0)),
            Some(utc("2027-03-28T00:59:00Z"))
        );
        assert_eq!(
            resolve_local(date(2027, 3, 28).at(4, 0, 0, 0)),
            Some(utc("2027-03-28T01:00:00Z"))
        );
        assert_eq!(
            resolve_local(date(2027, 3, 28).at(4, 1, 0, 0)),
            Some(utc("2027-03-28T01:01:00Z"))
        );
    }

    #[test]
    fn fall_back_fold_takes_the_first_occurrence() {
        // 2026-10-25: 04:00 EEST falls back to 03:00 EET (01:00 UTC);
        // 03:00–03:59 happen twice. The first time is still EEST (UTC+3).
        assert_eq!(
            resolve_local(date(2026, 10, 25).at(3, 0, 0, 0)),
            Some(utc("2026-10-25T00:00:00Z"))
        );
        assert_eq!(
            resolve_local(date(2026, 10, 25).at(3, 30, 0, 0)),
            Some(utc("2026-10-25T00:30:00Z"))
        );
        assert_eq!(
            resolve_local(date(2026, 10, 25).at(4, 0, 0, 0)),
            Some(utc("2026-10-25T02:00:00Z"))
        );
        assert_eq!(
            resolve_local(date(2026, 10, 25).at(2, 59, 0, 0)),
            Some(utc("2026-10-24T23:59:00Z"))
        );
    }

    #[test]
    fn seconds_are_dropped_before_resolving() {
        assert_eq!(
            resolve_local(date(2026, 1, 15).at(9, 0, 42, 5)),
            Some(utc("2026-01-15T07:00:00Z"))
        );
    }

    #[test]
    fn shop_clock_and_datetime_use_athens_time() {
        let now = utc("2026-10-25T00:30:59Z");
        assert_eq!(shop_datetime(now), date(2026, 10, 25).at(3, 30, 59, 0));
        assert_eq!(shop_clock(now), time(3, 30, 0, 0));
        // Second pass through 03:30, now in winter time.
        assert_eq!(shop_clock(utc("2026-10-25T01:30:00Z")), time(3, 30, 0, 0));
    }

    #[test]
    fn floor_to_minute_truncates() {
        assert_eq!(
            floor_to_minute(utc("2026-01-15T07:00:59.9Z")),
            utc("2026-01-15T07:00:00Z")
        );
        assert_eq!(
            floor_to_minute(utc("1969-12-31T23:59:30Z")),
            utc("1969-12-31T23:59:00Z")
        );
    }
}
