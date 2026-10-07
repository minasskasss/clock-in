//! The "Αυτόματο" (automatic) theme: dark in the evening and at night, light
//! during the day (SPEC §3). The UI only asks "is it dark now?".

use crate::business_day::{business_date_for, business_day_start};
use crate::shop_time::{add_days, resolve_local_at};
use jiff::Timestamp;
use jiff::civil::Time;

/// The automatic theme turns dark at this Greek wall-clock time.
pub const AUTO_DARK_FROM: Time = Time::constant(21, 0, 0, 0);

/// Whether the automatic theme is dark at `now`: from 21:00 Greek time on a
/// business day until that business day ends at the next `rollover`.
///
/// Comparing instants inside the business day (not wall-clock times) keeps it
/// right through both daylight-saving changes, which happen at night. A
/// rollover of 00:00 means dark from 21:00 to midnight.
#[must_use]
pub fn auto_theme_is_dark(now: Timestamp, rollover: Time) -> bool {
    let business_date = business_date_for(now, rollover);
    resolve_local_at(business_date, AUTO_DARK_FROM).is_some_and(|dark_from| now >= dark_from)
}

/// The automatic theme's dark periods, `[start, end)`, for the business day
/// containing `from` and the `days` after it: [`auto_theme_is_dark`] is true
/// exactly inside them. For code outside the core (Android's alarm screen)
/// that may only compare instants.
#[must_use]
pub fn auto_dark_windows(
    from: Timestamp,
    days: u32,
    rollover: Time,
) -> Vec<(Timestamp, Timestamp)> {
    let first = business_date_for(from, rollover);
    (0..=i64::from(days))
        .filter_map(|i| {
            let date = add_days(first, i)?;
            let start = resolve_local_at(date, AUTO_DARK_FROM)?;
            let end = business_day_start(add_days(date, 1)?, rollover)?;
            (start < end).then_some((start, end))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::time;

    fn utc(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    const FIVE: Time = Time::constant(5, 0, 0, 0);

    fn check(rollover: Time, cases: &[(&str, bool)]) {
        for &(now, dark) in cases {
            assert_eq!(auto_theme_is_dark(utc(now), rollover), dark, "{now}");
        }
    }

    #[test]
    fn turns_dark_at_21_00() {
        // Summer: 21:00 EEST = 18:00 UTC.
        check(
            FIVE,
            &[
                ("2026-06-01T17:59:00Z", false), // 20:59
                ("2026-06-01T17:59:59Z", false), // 20:59:59
                ("2026-06-01T18:00:00Z", true),  // 21:00
                ("2026-06-01T20:59:00Z", true),  // 23:59
                ("2026-06-01T21:00:00Z", true),  // 00:00
            ],
        );
        // Winter: 21:00 EET = 19:00 UTC.
        check(
            FIVE,
            &[
                ("2026-12-01T18:59:00Z", false), // 20:59
                ("2026-12-01T19:00:00Z", true),  // 21:00
            ],
        );
    }

    #[test]
    fn turns_light_at_the_rollover() {
        // Summer: 05:00 EEST = 02:00 UTC.
        check(
            FIVE,
            &[
                ("2026-06-02T01:59:00Z", true),  // 04:59
                ("2026-06-02T01:59:59Z", true),  // 04:59:59
                ("2026-06-02T02:00:00Z", false), // 05:00
                ("2026-06-02T09:00:00Z", false), // 12:00
            ],
        );
        // A later rollover keeps it dark longer.
        check(
            time(8, 0, 0, 0),
            &[
                ("2026-06-02T04:59:00Z", true),  // 07:59
                ("2026-06-02T05:00:00Z", false), // 08:00
            ],
        );
    }

    #[test]
    fn rollover_at_midnight_is_dark_from_21_00_to_midnight() {
        let midnight = time(0, 0, 0, 0);
        check(
            midnight,
            &[
                ("2026-06-01T17:59:00Z", false), // 20:59
                ("2026-06-01T18:00:00Z", true),  // 21:00
                ("2026-06-01T20:59:00Z", true),  // 23:59
                ("2026-06-01T21:00:00Z", false), // 00:00
                ("2026-06-01T23:00:00Z", false), // 02:00
            ],
        );
    }

    #[test]
    fn spring_forward_night() {
        // 2026-03-28 21:00 EET = 19:00 UTC. On 2026-03-29 the clocks jump from
        // 03:00 EET (01:00 UTC) to 04:00 EEST, so 05:00 EEST = 02:00 UTC.
        check(
            FIVE,
            &[
                ("2026-03-28T18:59:00Z", false), // 20:59 EET
                ("2026-03-28T19:00:00Z", true),  // 21:00 EET
                ("2026-03-29T00:59:00Z", true),  // 02:59 EET
                ("2026-03-29T01:00:00Z", true),  // 04:00 EEST
                ("2026-03-29T01:59:00Z", true),  // 04:59 EEST
                ("2026-03-29T02:00:00Z", false), // 05:00 EEST
            ],
        );
        // The evening after the change: 21:00 EEST = 18:00 UTC.
        check(
            FIVE,
            &[
                ("2026-03-29T17:59:00Z", false), // 20:59 EEST
                ("2026-03-29T18:00:00Z", true),  // 21:00 EEST
            ],
        );
        // A rollover inside the skipped hour moves to the next valid minute,
        // 04:00 EEST (01:00 UTC), like the business day itself.
        check(
            time(3, 30, 0, 0),
            &[
                ("2026-03-29T00:59:00Z", true),  // 02:59 EET
                ("2026-03-29T01:00:00Z", false), // 04:00 EEST
            ],
        );
    }

    #[test]
    fn fall_back_night() {
        // 2026-10-24 21:00 EEST = 18:00 UTC. On 2026-10-25 the clocks go back
        // from 04:00 EEST (01:00 UTC) to 03:00 EET, so 05:00 EET = 03:00 UTC.
        check(
            FIVE,
            &[
                ("2026-10-24T17:59:00Z", false), // 20:59 EEST
                ("2026-10-24T18:00:00Z", true),  // 21:00 EEST
                ("2026-10-25T00:30:00Z", true),  // first 03:30
                ("2026-10-25T01:30:00Z", true),  // second 03:30
                ("2026-10-25T02:59:00Z", true),  // 04:59 EET
                ("2026-10-25T03:00:00Z", false), // 05:00 EET
            ],
        );
        // The evening after the change: 21:00 EET = 19:00 UTC.
        check(
            FIVE,
            &[
                ("2026-10-25T18:59:00Z", false), // 20:59 EET
                ("2026-10-25T19:00:00Z", true),  // 21:00 EET
            ],
        );
        // A rollover inside the repeated hour: light from the first 03:30 on,
        // and it stays light through the second pass.
        check(
            time(3, 30, 0, 0),
            &[
                ("2026-10-25T00:15:00Z", true),  // first 03:15
                ("2026-10-25T00:30:00Z", false), // first 03:30
                ("2026-10-25T01:15:00Z", false), // second 03:15
            ],
        );
    }

    #[test]
    fn dark_windows_match_the_rule() {
        // Every 5 minutes over 4 days around each daylight-saving change, for
        // the earliest, a middle and the latest rollover.
        for start in ["2026-03-27T12:00:00Z", "2026-10-23T12:00:00Z"] {
            for rollover in [time(0, 0, 0, 0), time(3, 30, 0, 0), FIVE, time(8, 0, 0, 0)] {
                let from = utc(start);
                let windows = auto_dark_windows(from, 4, rollover);
                assert_eq!(windows.len(), 5, "{start} {rollover}");
                let mut t = from;
                while t < from + jiff::Span::new().hours(4 * 24) {
                    let inside = windows.iter().any(|&(a, b)| a <= t && t < b);
                    assert_eq!(inside, auto_theme_is_dark(t, rollover), "{t} {rollover}");
                    t += jiff::Span::new().minutes(5);
                }
            }
        }
    }
}
