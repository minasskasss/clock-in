//! The app's clock. Release builds always use the real time; debug builds
//! can move it with the fake-clock setting in the menu (ARCHITECTURE §9).

use clockin_core::{clock_offset_to, offset_now, shop_datetime};
use jiff::civil::DateTime;
use jiff::{SignedDuration, Timestamp};
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct Clock {
    offset: Mutex<SignedDuration>,
}

impl Clock {
    /// "Now" for everything schedule-related.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        offset_now(Timestamp::now(), self.offset())
    }

    fn offset(&self) -> SignedDuration {
        if cfg!(debug_assertions) {
            *self
                .offset
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        } else {
            SignedDuration::ZERO
        }
    }

    /// Debug builds only: make the clock read Greek wall-clock `target`
    /// now, or the real time again with `None`. Returns false if refused.
    pub fn set_fake(&self, target: Option<DateTime>) -> bool {
        if !cfg!(debug_assertions) {
            return false;
        }
        let offset = match target {
            None => SignedDuration::ZERO,
            Some(target) => match clock_offset_to(target, Timestamp::now()) {
                Some(offset) => offset,
                None => return false,
            },
        };
        *self
            .offset
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = offset;
        true
    }

    /// The fake time as Greek wall clock "YYYY-MM-DDTHH:MM", if one is set.
    #[must_use]
    pub fn fake(&self) -> Option<String> {
        (self.offset() != SignedDuration::ZERO).then(|| {
            shop_datetime(self.now())
                .strftime("%Y-%m-%dT%H:%M")
                .to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn fake_clock_in_debug_builds() {
        let clock = Clock::default();
        assert_eq!(clock.fake(), None);
        let target = date(2026, 10, 25).at(3, 30, 0, 0);
        let set = clock.set_fake(Some(target));
        assert_eq!(set, cfg!(debug_assertions));
        if set {
            assert!(clock.fake().unwrap().starts_with("2026-10-25T03:3"));
            assert!(clock.set_fake(None));
            assert_eq!(clock.fake(), None);
        }
    }
}
