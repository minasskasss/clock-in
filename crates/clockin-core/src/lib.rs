//! Pure scheduling and time logic for Clock In.
//!
//! This crate does no I/O and never reads the system clock: the current
//! instant is always passed in by the caller. All times are interpreted in
//! [`TIME_ZONE_NAME`], whatever the device's own timezone setting is.
//!
//! The real logic (business day, occurrences, alarm plan, validation) arrives
//! in Phase 1; see `docs/ARCHITECTURE.md` §7.

use jiff::tz::TimeZone;

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
    TimeZone::get(TIME_ZONE_NAME).expect("bundled tz database contains Europe/Athens")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn bundled_time_zone_is_available() {
        assert_eq!(shop_time_zone().iana_name(), Some(TIME_ZONE_NAME));
    }

    #[test]
    fn athens_offsets_across_2026_fall_back() {
        let tz = shop_time_zone();
        // Summer time (EEST, UTC+3) before 2026-10-25, winter time (EET, UTC+2) after.
        let summer = date(2026, 10, 24)
            .at(12, 0, 0, 0)
            .to_zoned(tz.clone())
            .unwrap();
        let winter = date(2026, 10, 26).at(12, 0, 0, 0).to_zoned(tz).unwrap();
        assert_eq!(summer.offset().seconds(), 3 * 3600);
        assert_eq!(winter.offset().seconds(), 2 * 3600);
    }
}
