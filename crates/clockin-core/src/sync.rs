//! Time checks used by the sync loop and the banners (ARCHITECTURE §8,
//! SPEC §6). The platform layers call these instead of doing time arithmetic.

use crate::plan::PLAN_HORIZON;
use jiff::{SignedDuration, Timestamp};

/// The device clock may differ from server time by this much before the
/// clock banner shows (SPEC §6).
pub const MAX_CLOCK_SKEW: SignedDuration = SignedDuration::from_mins(2);

/// The plan is recomputed when it covers less than this much ahead.
pub const PLAN_REFRESH_BELOW: SignedDuration = SignedDuration::from_hours(13 * 24);

/// The "alarm schedule running short" banner shows below this (SPEC §6).
pub const HORIZON_WARNING_BELOW: SignedDuration = SignedDuration::from_hours(3 * 24);

/// How often the sync loop polls `get_version` while the server answers
/// (ARCHITECTURE §8).
pub const POLL_INTERVAL: SignedDuration = SignedDuration::from_secs(5);

/// Delay before the next sync attempt after `consecutive_failures` failed
/// ones in a row: 5 → 10 → 30 → 60 s, then 60 s (ARCHITECTURE §8). With no
/// failures it is the normal [`POLL_INTERVAL`].
#[must_use]
pub fn sync_retry_delay(consecutive_failures: u32) -> SignedDuration {
    match consecutive_failures {
        0 | 1 => POLL_INTERVAL,
        2 => SignedDuration::from_secs(10),
        3 => SignedDuration::from_secs(30),
        _ => SignedDuration::from_secs(60),
    }
}

/// Whether the device clock is more than 2 minutes off server time.
#[must_use]
pub fn clock_skew_exceeded(server_now: Timestamp, device_now: Timestamp) -> bool {
    device_now.duration_since(server_now).abs() > MAX_CLOCK_SKEW
}

/// Whether this device should recompute and upload the alarm plan: the
/// schedule changed since the plan was made, or the plan reaches less than
/// 13 days ahead (or there is none yet).
#[must_use]
pub fn plan_needs_refresh(
    config_version: i64,
    plan_config_version: i64,
    plan_horizon_end: Option<Timestamp>,
    now: Timestamp,
) -> bool {
    config_version > plan_config_version
        || covers_less_than(plan_horizon_end, now, PLAN_REFRESH_BELOW)
}

/// Whether the alarm plan covers fewer than 3 days ahead (or there is none).
#[must_use]
pub fn horizon_short(plan_horizon_end: Option<Timestamp>, now: Timestamp) -> bool {
    covers_less_than(plan_horizon_end, now, HORIZON_WARNING_BELOW)
}

fn covers_less_than(horizon_end: Option<Timestamp>, now: Timestamp, span: SignedDuration) -> bool {
    horizon_end.is_none_or(|end| end.duration_since(now) < span)
}

// A plan freshly computed for PLAN_HORIZON must not immediately need refreshing.
const _: () = assert!(PLAN_HORIZON.as_secs() > PLAN_REFRESH_BELOW.as_secs());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_window;

    fn now() -> Timestamp {
        "2026-06-01T09:00:00Z".parse().unwrap()
    }

    fn plus(mins: i64) -> Timestamp {
        now() + SignedDuration::from_mins(mins)
    }

    #[test]
    fn clock_skew_over_two_minutes() {
        assert!(!clock_skew_exceeded(now(), now()));
        assert!(!clock_skew_exceeded(now(), plus(2)));
        assert!(!clock_skew_exceeded(now(), plus(-2)));
        assert!(clock_skew_exceeded(
            now(),
            plus(2) + SignedDuration::from_secs(1)
        ));
        assert!(clock_skew_exceeded(now(), plus(-3)));
    }

    #[test]
    fn plan_refresh_rules() {
        let day = 24 * 60;
        // Schedule changed.
        assert!(plan_needs_refresh(5, 4, Some(plus(14 * day)), now()));
        // Up to date and long enough.
        assert!(!plan_needs_refresh(5, 5, Some(plus(14 * day)), now()));
        assert!(!plan_needs_refresh(5, 5, Some(plus(13 * day)), now()));
        // Horizon getting short.
        assert!(plan_needs_refresh(5, 5, Some(plus(13 * day - 1)), now()));
        // No plan yet.
        assert!(plan_needs_refresh(0, 0, None, now()));
        // A plan made from a newer config than we know (another device was
        // faster) is fine.
        assert!(!plan_needs_refresh(5, 6, Some(plus(14 * day)), now()));
    }

    #[test]
    fn fresh_plan_does_not_need_refreshing() {
        let (_, horizon_end) = plan_window(now());
        assert!(!plan_needs_refresh(1, 1, Some(horizon_end), now()));
        assert!(!horizon_short(Some(horizon_end), now()));
    }

    #[test]
    fn retry_delay_backs_off_to_a_minute() {
        let secs: Vec<i64> = (0..=6).map(|n| sync_retry_delay(n).as_secs()).collect();
        assert_eq!(secs, [5, 5, 10, 30, 60, 60, 60]);
        assert_eq!(sync_retry_delay(u32::MAX).as_secs(), 60);
    }

    #[test]
    fn horizon_short_below_three_days() {
        let day = 24 * 60;
        assert!(!horizon_short(Some(plus(3 * day)), now()));
        assert!(horizon_short(Some(plus(3 * day - 1)), now()));
        assert!(horizon_short(Some(plus(-5)), now()));
        assert!(horizon_short(None, now()));
    }
}
