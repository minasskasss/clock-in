//! Pure scheduling and time logic for Clock In.
//!
//! This crate does no I/O and never reads the system clock: the current
//! instant is always passed in by the caller. All civil dates and times are
//! Greek local time ([`TIME_ZONE_NAME`]), whatever the device's own timezone
//! setting is. See `docs/ARCHITECTURE.md` §7.
//!
//! - [`business_date_for`]: which business day an instant belongs to.
//! - [`occurrences`]: concrete blocks of one business date.
//! - [`today_view`]: the rows and statuses of the main screen.
//! - [`alarm_plan`]: every alarm in a window, with deterministic ids.
//! - [`due_events`] / [`next_event`]: alarms grouped by minute, suppressed by marks.
//! - [`ring_cycle_state`]: the ring 5 min / silent 5 min repeat cycle.
//! - [`AlarmScheduler`]: what the Windows alarm should do right now.
//! - [`validate_week`] / [`validate_override`]: durations and overlaps.
//! - [`auto_theme_is_dark`]: the automatic light/dark theme.
//! - [`record_crash`]: when the Windows restart watcher gives up.

mod admin;
mod business_day;
mod events;
mod model;
mod occurrence;
mod passphrase;
mod plan;
mod ring;
mod scheduler;
mod shop_time;
mod sync;
mod theme;
mod today;
mod validate;
mod watchdog;

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod test_support;

pub use admin::{
    ADMIN_IDLE_TIMEOUT, admin_idle_expired, clock_offset_to, in_second_pass, lockout_until,
    offset_now, seconds_until,
};
pub use business_day::{BlockLayout, business_date_for, business_day_start};
pub use events::{
    AlarmEvent, Sound, due_events, firing_window, is_still_due, is_suppressed, next_event,
};
pub use model::{
    Mark, MarkKind, Override, OverrideBlock, OverrideKind, ScheduleSettings, Snapshot, Staff,
    WeeklyBlock, weekday_number,
};
pub use occurrence::{Occurrence, occurrences};
pub use passphrase::{
    EFF_WORD_COUNT, GENERATED_WORDS, MIN_CHARS as PASSPHRASE_MIN_CHARS,
    MIN_WORDS as PASSPHRASE_MIN_WORDS, PassphraseError, check_passphrase, eff_word,
    normalize_passphrase, passphrase_from_indices, same_passphrase,
};
pub use plan::{MISSED_ALARM_GRACE, PLAN_HORIZON, PlanItem, alarm_plan, item_id, plan_window};
pub use ring::{RING_DURATION, RingPhase, SILENT_DURATION, ring_cycle_state};
pub use scheduler::{AlarmKey, AlarmScheduler, AlarmState, MAX_SLEEP, RingingAlarm};
pub use shop_time::{
    TIME_ZONE_NAME, floor_to_minute, resolve_local, resolve_local_at, shop_clock, shop_datetime,
    shop_time_zone,
};
pub use sync::{
    HORIZON_WARNING_BELOW, MAX_CLOCK_SKEW, PLAN_REFRESH_BELOW, POLL_INTERVAL, clock_skew_exceeded,
    horizon_short, plan_needs_refresh, sync_retry_delay,
};
pub use theme::{AUTO_DARK_FROM, auto_dark_windows, auto_theme_is_dark};
pub use today::{RowStatus, TodayRow, TodayView, has_mark, today_view};
pub use validate::{
    BlockIssue, BlockProblem, DayBlock, MAX_BLOCK_MINUTES, MAX_NAME_CHARS, MAX_OFFSET_MINUTES,
    MAX_ROLLOVER, MIN_BLOCK_MINUTES, MIN_OFFSET_MINUTES, NameError, OverrideError, SettingsProblem,
    TimeRange, WeekError, is_valid_quit_code, normalize_name, validate_block, validate_override,
    validate_settings, validate_week,
};
pub use watchdog::{CRASH_WINDOW, MAX_CRASHES, RESTART_DELAY, record_crash};
