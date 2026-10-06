//! Time rules of the admin screens and the debug clock: Settings re-lock
//! after inactivity (SPEC §2), the passphrase lockout countdown (SPEC §4.6)
//! and the debug-only fake clock (ARCHITECTURE §9). The platform layers call
//! these instead of doing time arithmetic.

use crate::shop_time::{resolve_local, shop_datetime, shop_time_zone};
use jiff::civil::DateTime;
use jiff::tz::AmbiguousOffset;
use jiff::{SignedDuration, Timestamp};

/// Settings lock again after this long without any interaction (SPEC §2).
pub const ADMIN_IDLE_TIMEOUT: SignedDuration = SignedDuration::from_mins(5);

/// Whether an unlocked Settings screen has been idle long enough to lock.
/// A clock that went backwards counts as activity just now.
#[must_use]
pub fn admin_idle_expired(last_activity: Timestamp, now: Timestamp) -> bool {
    now.duration_since(last_activity) >= ADMIN_IDLE_TIMEOUT
}

/// When a lockout reported as `retry_after_s` seconds from `now` ends.
#[must_use]
pub fn lockout_until(now: Timestamp, retry_after_s: u32) -> Timestamp {
    now.saturating_add(SignedDuration::from_secs(i64::from(retry_after_s)))
        .unwrap_or(Timestamp::MAX)
}

/// Whole seconds left until `until`, rounded up; 0 once it has passed.
#[must_use]
pub fn seconds_until(until: Timestamp, now: Timestamp) -> u32 {
    let left = until.duration_since(now);
    if left <= SignedDuration::ZERO {
        return 0;
    }
    let secs = left.as_secs() + i64::from(left.subsec_nanos() > 0);
    u32::try_from(secs).unwrap_or(u32::MAX)
}

/// The offset that makes the clock read Greek wall-clock `target` at the real
/// instant `real_now` (debug builds only). In the hour that repeats when the
/// clocks go back (03:00–03:59 on the last Sunday of October), `second`
/// picks its second pass (winter time); otherwise `target` means its first
/// pass, like block times do. `None` outside jiff's range.
#[must_use]
pub fn clock_offset_to(
    target: DateTime,
    second: bool,
    real_now: Timestamp,
) -> Option<SignedDuration> {
    let instant = match shop_time_zone().to_ambiguous_timestamp(target).offset() {
        AmbiguousOffset::Fold { after, .. } if second => after.to_timestamp(target).ok()?,
        _ => resolve_local(target)?,
    };
    Some(instant.duration_since(real_now))
}

/// Whether `instant` is in the second pass of the repeated hour, which its
/// wall-clock time alone can't tell (the debug clock shows it).
#[must_use]
pub fn in_second_pass(instant: Timestamp) -> bool {
    let tz = shop_time_zone();
    match tz.to_ambiguous_timestamp(shop_datetime(instant)).offset() {
        AmbiguousOffset::Fold { before, after } => {
            before != after && tz.to_offset(instant) == after
        }
        _ => false,
    }
}

/// The app's "now": the real instant moved by the debug offset.
#[must_use]
pub fn offset_now(real_now: Timestamp, offset: SignedDuration) -> Timestamp {
    real_now.saturating_add(offset).unwrap_or(real_now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn settings_lock_after_five_idle_minutes() {
        let last = ts("2026-10-05T10:00:00Z");
        assert!(!admin_idle_expired(last, ts("2026-10-05T10:04:59Z")));
        assert!(admin_idle_expired(last, ts("2026-10-05T10:05:00Z")));
        assert!(admin_idle_expired(last, ts("2026-10-05T11:00:00Z")));
        // Clock went backwards: not expired.
        assert!(!admin_idle_expired(last, ts("2026-10-05T09:00:00Z")));
    }

    #[test]
    fn lockout_countdown_rounds_up_and_stops_at_zero() {
        let now = ts("2026-10-05T10:00:00Z");
        let until = lockout_until(now, 60);
        assert_eq!(until, ts("2026-10-05T10:01:00Z"));
        assert_eq!(seconds_until(until, now), 60);
        assert_eq!(seconds_until(until, ts("2026-10-05T10:00:59.2Z")), 1);
        assert_eq!(seconds_until(until, ts("2026-10-05T10:01:00Z")), 0);
        assert_eq!(seconds_until(until, ts("2026-10-05T10:05:00Z")), 0);
        assert_eq!(lockout_until(now, 0), now);
    }

    #[test]
    fn fake_clock_reads_the_chosen_greek_time() {
        let real = ts("2026-10-05T10:00:00Z");
        // Fall-back night: 03:30 is ambiguous and takes the first occurrence.
        let target = date(2026, 10, 25).at(3, 30, 0, 0);
        let offset = clock_offset_to(target, false, real).unwrap();
        let fake = offset_now(real, offset);
        assert_eq!(shop_datetime(fake), target);
        assert_eq!(fake, ts("2026-10-25T00:30:00Z"));
        // The offset keeps the clock running.
        let later = offset_now(real + SignedDuration::from_mins(90), offset);
        assert_eq!(shop_datetime(later), date(2026, 10, 25).at(4, 0, 0, 0));
        assert_eq!(offset_now(real, SignedDuration::ZERO), real);
    }

    #[test]
    fn fake_clock_can_pick_the_second_pass_of_the_repeated_hour() {
        let real = ts("2026-10-05T10:00:00Z");
        let at = |second, h, m| {
            let target = date(2026, 10, 25).at(h, m, 0, 0);
            offset_now(real, clock_offset_to(target, second, real).unwrap())
        };
        // 03:30 happens at 00:30 UTC (summer time), then at 01:30 UTC.
        assert_eq!(at(true, 3, 30), ts("2026-10-25T01:30:00Z"));
        assert_eq!(
            shop_datetime(at(true, 3, 30)),
            date(2026, 10, 25).at(3, 30, 0, 0)
        );
        // Outside the repeated hour `second` changes nothing.
        assert_eq!(at(true, 2, 30), at(false, 2, 30));
        assert_eq!(at(true, 4, 0), at(false, 4, 0));
        assert_eq!(at(true, 4, 0), ts("2026-10-25T02:00:00Z"));

        assert!(!in_second_pass(ts("2026-10-25T00:30:00Z")));
        assert!(in_second_pass(ts("2026-10-25T01:00:00Z")));
        assert!(in_second_pass(ts("2026-10-25T01:59:59Z")));
        assert!(!in_second_pass(ts("2026-10-25T02:00:00Z")));
        assert!(!in_second_pass(ts("2026-10-05T10:00:00Z")));
    }
}
