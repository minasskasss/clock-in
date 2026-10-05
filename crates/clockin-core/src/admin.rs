//! Time rules of the admin screens and the debug clock: Settings re-lock
//! after inactivity (SPEC §2), the passphrase lockout countdown (SPEC §4.6)
//! and the debug-only fake clock (ARCHITECTURE §9). The platform layers call
//! these instead of doing time arithmetic.

use crate::shop_time::resolve_local;
use jiff::civil::DateTime;
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
/// instant `real_now` (debug builds only). `None` outside jiff's range.
#[must_use]
pub fn clock_offset_to(target: DateTime, real_now: Timestamp) -> Option<SignedDuration> {
    Some(resolve_local(target)?.duration_since(real_now))
}

/// The app's "now": the real instant moved by the debug offset.
#[must_use]
pub fn offset_now(real_now: Timestamp, offset: SignedDuration) -> Timestamp {
    real_now.saturating_add(offset).unwrap_or(real_now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shop_time::shop_datetime;
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
        let offset = clock_offset_to(target, real).unwrap();
        let fake = offset_now(real, offset);
        assert_eq!(shop_datetime(fake), target);
        assert_eq!(fake, ts("2026-10-25T00:30:00Z"));
        // The offset keeps the clock running.
        let later = offset_now(real + SignedDuration::from_mins(90), offset);
        assert_eq!(shop_datetime(later), date(2026, 10, 25).at(4, 0, 0, 0));
        assert_eq!(offset_now(real, SignedDuration::ZERO), real);
    }
}
