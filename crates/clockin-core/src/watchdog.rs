//! The Windows restart watcher's rule (PLAN Phase 6): restart the app after
//! a crash, but give up after [`MAX_CRASHES`] crashes within
//! [`CRASH_WINDOW`], so an app that crashes at start doesn't loop forever.

use jiff::{SignedDuration, Timestamp};

/// How many crashes within [`CRASH_WINDOW`] end the restarts.
pub const MAX_CRASHES: usize = 5;

/// The window [`MAX_CRASHES`] is counted in: 10 minutes.
pub const CRASH_WINDOW: SignedDuration = SignedDuration::from_mins(10);

/// How long the watcher waits before restarting the app.
pub const RESTART_DELAY: SignedDuration = SignedDuration::from_secs(2);

/// Records a crash at `now` in `crashes` (dropping the ones older than
/// [`CRASH_WINDOW`]) and says whether to restart: `false` once this is the
/// [`MAX_CRASHES`]th crash within the window.
///
/// A clock that went backwards keeps the later entries: they count as
/// recent, which can only stop restarts sooner, never loop longer.
pub fn record_crash(crashes: &mut Vec<Timestamp>, now: Timestamp) -> bool {
    crashes.retain(|&at| now.duration_since(at) < CRASH_WINDOW);
    crashes.push(now);
    crashes.len() < MAX_CRASHES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(min: i64, sec: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + min * 60 + sec).unwrap()
    }

    #[test]
    fn restarts_until_the_fifth_crash_in_ten_minutes() {
        let mut crashes = Vec::new();
        for minute in 0..4 {
            assert!(record_crash(&mut crashes, at(minute, 0)), "crash {minute}");
        }
        assert!(!record_crash(&mut crashes, at(4, 0)), "fifth crash");
    }

    #[test]
    fn crashes_older_than_ten_minutes_dont_count() {
        let mut crashes = Vec::new();
        for minute in [0, 1, 2, 3] {
            assert!(record_crash(&mut crashes, at(minute, 0)));
        }
        // 10:00 after the first: that one has left the window.
        assert!(record_crash(&mut crashes, at(10, 0)));
        assert_eq!(crashes.len(), 4);
        // 09:59 after the second would still be inside it.
        let mut crashes = Vec::new();
        for minute in [0, 1, 2, 3] {
            record_crash(&mut crashes, at(minute, 0));
        }
        assert!(!record_crash(&mut crashes, at(9, 59)));
    }

    #[test]
    fn one_crash_a_day_always_restarts() {
        let mut crashes = Vec::new();
        for day in 0..30 {
            assert!(record_crash(&mut crashes, at(day * 24 * 60, 0)));
        }
        assert_eq!(crashes.len(), 1);
    }

    #[test]
    fn a_clock_that_went_back_never_allows_more_restarts() {
        let mut crashes = Vec::new();
        for minute in [20, 21, 22, 23] {
            record_crash(&mut crashes, at(minute, 0));
        }
        // The clock jumped back an hour: the four still count.
        assert!(!record_crash(&mut crashes, at(-40, 0)));
    }
}
