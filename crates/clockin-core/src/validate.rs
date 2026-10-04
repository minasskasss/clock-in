//! Validation of what the employer enters in Settings (SPEC §4). The server
//! repeats only cheap checks (ARCHITECTURE §6); duration and overlap rules
//! live here.

use crate::business_day::{BlockLayout, business_date_for};
use crate::model::{OverrideKind, ScheduleSettings, weekday_number};
use jiff::Timestamp;
use jiff::civil::{Date, Time, Weekday};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// Shortest allowed block, in minutes.
pub const MIN_BLOCK_MINUTES: u32 = 15;
/// Longest allowed block, in minutes (16 hours).
pub const MAX_BLOCK_MINUTES: u32 = 16 * 60;
/// Alarm offset range, in minutes (SPEC §4.4).
pub const MIN_OFFSET_MINUTES: i32 = -60;
pub const MAX_OFFSET_MINUTES: i32 = 30;
/// Latest allowed business-day rollover.
pub const MAX_ROLLOVER: Time = Time::constant(8, 0, 0, 0);
/// Longest first or last name, in characters.
pub const MAX_NAME_CHARS: usize = 40;

/// A start–end pair of wall-clock times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: Time,
    pub end: Time,
}

/// A block of the weekly template being edited (no id yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayBlock {
    #[serde(with = "weekday_number")]
    pub weekday: Weekday,
    pub start: Time,
    pub end: Time,
}

/// What is wrong with one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum BlockProblem {
    /// Times must be whole minutes (HH:MM).
    NotWholeMinute,
    StartEqualsEnd,
    /// Shorter than 15 minutes.
    TooShort,
    /// Longer than 16 hours.
    TooLong,
    /// Overlaps the block at index `other` on the same business day.
    Overlaps {
        other: usize,
    },
}

/// A problem with the block at `index` of the submitted list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockIssue {
    pub index: usize,
    #[serde(flatten)]
    pub problem: BlockProblem,
}

/// Why a weekly schedule was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", content = "issues", rename_all = "snake_case")]
pub enum WeekError {
    /// A staff member needs at least one block (SPEC §4.1).
    NoBlocks,
    InvalidBlocks(Vec<BlockIssue>),
}

/// Why a one-off change was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", content = "issues", rename_all = "snake_case")]
pub enum OverrideError {
    /// The business date is before today's.
    PastDate,
    /// A day off cannot have blocks.
    DayOffHasBlocks,
    /// Replace hours needs at least one block (use a day off for none).
    NoBlocks,
    InvalidBlocks(Vec<BlockIssue>),
}

/// Checks a whole weekly template: each block 15 min – 16 h, start ≠ end, and
/// no overlap between one person's blocks on the same business day.
///
/// # Errors
///
/// [`WeekError::NoBlocks`] for an empty week, otherwise every problem found.
pub fn validate_week(blocks: &[DayBlock], rollover: Time) -> Result<(), WeekError> {
    if blocks.is_empty() {
        return Err(WeekError::NoBlocks);
    }
    let issues = check_blocks(
        blocks.iter().map(|b| {
            (
                Some(b.weekday),
                TimeRange {
                    start: b.start,
                    end: b.end,
                },
            )
        }),
        rollover,
    );
    if issues.is_empty() {
        Ok(())
    } else {
        Err(WeekError::InvalidBlocks(issues))
    }
}

/// Checks a one-off change for `business_date`, which must be today's
/// business date or later.
///
/// # Errors
///
/// The first rule broken; for block problems, every problem found.
pub fn validate_override(
    kind: OverrideKind,
    business_date: Date,
    blocks: &[TimeRange],
    rollover: Time,
    now: Timestamp,
) -> Result<(), OverrideError> {
    if business_date < business_date_for(now, rollover) {
        return Err(OverrideError::PastDate);
    }
    match kind {
        OverrideKind::Off if !blocks.is_empty() => Err(OverrideError::DayOffHasBlocks),
        OverrideKind::Off => Ok(()),
        OverrideKind::Replace if blocks.is_empty() => Err(OverrideError::NoBlocks),
        OverrideKind::Replace => {
            let issues = check_blocks(blocks.iter().map(|r| (None, *r)), rollover);
            if issues.is_empty() {
                Ok(())
            } else {
                Err(OverrideError::InvalidBlocks(issues))
            }
        }
    }
}

/// Checks one block on its own (for live feedback in the editor).
///
/// # Errors
///
/// The block's problem.
pub fn validate_block(range: TimeRange, rollover: Time) -> Result<BlockLayout, BlockProblem> {
    if !is_whole_minute(range.start) || !is_whole_minute(range.end) {
        return Err(BlockProblem::NotWholeMinute);
    }
    if range.start == range.end {
        return Err(BlockProblem::StartEqualsEnd);
    }
    let layout = BlockLayout::new(range.start, range.end, rollover);
    match layout.duration_minutes() {
        d if d < MIN_BLOCK_MINUTES => Err(BlockProblem::TooShort),
        d if d > MAX_BLOCK_MINUTES => Err(BlockProblem::TooLong),
        _ => Ok(layout),
    }
}

/// Blocks grouped by business day (`None` = all the same day).
fn check_blocks(
    blocks: impl Iterator<Item = (Option<Weekday>, TimeRange)>,
    rollover: Time,
) -> Vec<BlockIssue> {
    let mut issues = Vec::new();
    let mut valid: Vec<(usize, Option<Weekday>, BlockLayout)> = Vec::new();
    for (index, (day, range)) in blocks.enumerate() {
        match validate_block(range, rollover) {
            Ok(layout) => {
                if let Some(&(other, ..)) = valid
                    .iter()
                    .find(|(_, other_day, other)| *other_day == day && other.overlaps(layout))
                {
                    issues.push(BlockIssue {
                        index,
                        problem: BlockProblem::Overlaps { other },
                    });
                } else {
                    valid.push((index, day, layout));
                }
            }
            Err(problem) => issues.push(BlockIssue { index, problem }),
        }
    }
    issues
}

fn is_whole_minute(time: Time) -> bool {
    time.second() == 0 && time.subsec_nanosecond() == 0
}

/// Why a name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", content = "character", rename_all = "snake_case")]
pub enum NameError {
    Empty,
    /// More than 40 characters.
    TooLong,
    /// Only spaces, hyphens or apostrophes.
    NoLetters,
    /// Something other than a Greek or Latin letter, space, hyphen or apostrophe.
    InvalidCharacter(char),
}

/// Normalises (Unicode NFC, trimmed) and checks a first or last name:
/// 1–40 characters of Greek or Latin letters, spaces, hyphens and apostrophes.
///
/// # Errors
///
/// The first problem found.
pub fn normalize_name(raw: &str) -> Result<String, NameError> {
    let name: String = raw.nfc().collect::<String>().trim().to_string();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(NameError::TooLong);
    }
    if let Some(bad) = name
        .chars()
        .find(|&c| !(is_greek_or_latin_letter(c) || matches!(c, ' ' | '-' | '\'' | '’')))
    {
        return Err(NameError::InvalidCharacter(bad));
    }
    if !name.chars().any(is_greek_or_latin_letter) {
        return Err(NameError::NoLetters);
    }
    Ok(name)
}

fn is_greek_or_latin_letter(c: char) -> bool {
    c.is_alphabetic()
        && matches!(c,
            'A'..='Z' | 'a'..='z'
            | '\u{00C0}'..='\u{024F}'   // Latin-1 Supplement, Latin Extended-A/B
            | '\u{1E00}'..='\u{1EFF}'   // Latin Extended Additional
            | '\u{0370}'..='\u{03FF}'   // Greek and Coptic
            | '\u{1F00}'..='\u{1FFF}'   // Greek Extended (polytonic)
        )
        && !matches!(c, '\u{00D7}' | '\u{00F7}')
}

/// A settings value out of range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsProblem {
    /// Check-in offset outside −60 … +30 minutes.
    CheckinOffset,
    /// Check-out offset outside −60 … +30 minutes.
    CheckoutOffset,
    /// Rollover outside 00:00 … 08:00, or not a whole minute.
    Rollover,
}

/// Checks offsets and rollover (SPEC §4.4).
///
/// # Errors
///
/// Every value out of range.
pub fn validate_settings(settings: &ScheduleSettings) -> Result<(), Vec<SettingsProblem>> {
    let offset_ok = |m: i32| (MIN_OFFSET_MINUTES..=MAX_OFFSET_MINUTES).contains(&m);
    let mut problems = Vec::new();
    if !offset_ok(settings.checkin_offset_min) {
        problems.push(SettingsProblem::CheckinOffset);
    }
    if !offset_ok(settings.checkout_offset_min) {
        problems.push(SettingsProblem::CheckoutOffset);
    }
    if settings.rollover > MAX_ROLLOVER || !is_whole_minute(settings.rollover) {
        problems.push(SettingsProblem::Rollover);
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Whether `code` is a valid quit code: exactly 4 ASCII digits.
#[must_use]
pub fn is_valid_quit_code(code: &str) -> bool {
    code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use jiff::civil::{date, time};

    fn range(s: (i8, i8), e: (i8, i8)) -> TimeRange {
        TimeRange {
            start: time(s.0, s.1, 0, 0),
            end: time(e.0, e.1, 0, 0),
        }
    }

    fn day(weekday: Weekday, s: (i8, i8), e: (i8, i8)) -> DayBlock {
        let r = range(s, e);
        DayBlock {
            weekday,
            start: r.start,
            end: r.end,
        }
    }

    #[test]
    fn block_duration_table() {
        let cases = [
            ((9, 0), (17, 0), Ok(())),
            ((9, 0), (9, 15), Ok(())), // exactly 15 min
            ((9, 0), (9, 14), Err(BlockProblem::TooShort)),
            ((9, 0), (9, 0), Err(BlockProblem::StartEqualsEnd)),
            ((0, 0), (0, 0), Err(BlockProblem::StartEqualsEnd)),
            ((8, 0), (0, 0), Ok(())), // exactly 16 h, ends at midnight
            ((8, 0), (0, 1), Err(BlockProblem::TooLong)),
            ((19, 0), (0, 0), Ok(())),
            ((18, 0), (2, 0), Ok(())),
            ((23, 50), (0, 5), Ok(())), // 15 min across midnight
            ((23, 50), (0, 4), Err(BlockProblem::TooShort)),
            ((17, 0), (9, 0), Ok(())), // overnight, exactly 16 h
            ((17, 0), (9, 1), Err(BlockProblem::TooLong)),
            ((1, 0), (3, 0), Ok(())), // after midnight (+1)
        ];
        for (s, e, expected) in cases {
            assert_eq!(
                validate_block(range(s, e), FIVE).map(|_| ()),
                expected,
                "{s:?}–{e:?}"
            );
        }
    }

    #[test]
    fn seconds_are_refused() {
        let r = TimeRange {
            start: time(9, 0, 30, 0),
            end: time(17, 0, 0, 0),
        };
        assert_eq!(validate_block(r, FIVE), Err(BlockProblem::NotWholeMinute));
    }

    #[test]
    fn valid_week_with_split_shift() {
        let week = [
            day(Weekday::Monday, (12, 0), (16, 0)),
            day(Weekday::Monday, (19, 0), (0, 0)),
            day(Weekday::Tuesday, (12, 0), (16, 0)),
            day(Weekday::Tuesday, (16, 0), (20, 0)), // touching is fine
        ];
        assert_eq!(validate_week(&week, FIVE), Ok(()));
    }

    #[test]
    fn empty_week_is_refused() {
        assert_eq!(validate_week(&[], FIVE), Err(WeekError::NoBlocks));
    }

    #[test]
    fn overlap_on_the_same_business_day_is_refused() {
        let week = [
            day(Weekday::Monday, (12, 0), (16, 0)),
            day(Weekday::Monday, (15, 0), (18, 0)),
        ];
        assert_eq!(
            validate_week(&week, FIVE),
            Err(WeekError::InvalidBlocks(vec![BlockIssue {
                index: 1,
                problem: BlockProblem::Overlaps { other: 0 },
            }]))
        );
    }

    #[test]
    fn overlap_after_midnight_counts_on_the_same_business_day() {
        // 23:00–03:00 and 01:00–02:00 (+1) both belong to Monday's business day.
        let week = [
            day(Weekday::Monday, (23, 0), (3, 0)),
            day(Weekday::Monday, (1, 0), (2, 0)),
        ];
        assert!(matches!(
            validate_week(&week, FIVE),
            Err(WeekError::InvalidBlocks(_))
        ));
        // 01:00 (+1) does not clash with a midday block.
        let week = [
            day(Weekday::Monday, (12, 0), (16, 0)),
            day(Weekday::Monday, (1, 0), (2, 0)),
        ];
        assert_eq!(validate_week(&week, FIVE), Ok(()));
    }

    #[test]
    fn same_times_on_different_days_do_not_overlap() {
        let week = [
            day(Weekday::Monday, (9, 0), (17, 0)),
            day(Weekday::Tuesday, (9, 0), (17, 0)),
        ];
        assert_eq!(validate_week(&week, FIVE), Ok(()));
    }

    #[test]
    fn every_problem_is_reported() {
        let week = [
            day(Weekday::Monday, (9, 0), (9, 5)),
            day(Weekday::Monday, (10, 0), (12, 0)),
            day(Weekday::Monday, (11, 0), (13, 0)),
            day(Weekday::Friday, (8, 0), (8, 0)),
        ];
        let Err(WeekError::InvalidBlocks(issues)) = validate_week(&week, FIVE) else {
            panic!("expected block issues");
        };
        assert_eq!(
            issues,
            [
                BlockIssue {
                    index: 0,
                    problem: BlockProblem::TooShort
                },
                BlockIssue {
                    index: 2,
                    problem: BlockProblem::Overlaps { other: 1 }
                },
                BlockIssue {
                    index: 3,
                    problem: BlockProblem::StartEqualsEnd
                },
            ]
        );
    }

    #[test]
    fn override_rules() {
        let now = athens(MONDAY, 12, 0);
        let ok = [range((10, 0), (14, 0))];
        assert_eq!(
            validate_override(OverrideKind::Replace, MONDAY, &ok, FIVE, now),
            Ok(())
        );
        assert_eq!(
            validate_override(OverrideKind::Off, TUESDAY, &[], FIVE, now),
            Ok(())
        );
        assert_eq!(
            validate_override(OverrideKind::Off, date(2026, 5, 31), &[], FIVE, now),
            Err(OverrideError::PastDate)
        );
        assert_eq!(
            validate_override(OverrideKind::Off, MONDAY, &ok, FIVE, now),
            Err(OverrideError::DayOffHasBlocks)
        );
        assert_eq!(
            validate_override(OverrideKind::Replace, MONDAY, &[], FIVE, now),
            Err(OverrideError::NoBlocks)
        );
        let overlapping = [range((10, 0), (14, 0)), range((13, 0), (15, 0))];
        assert!(matches!(
            validate_override(OverrideKind::Replace, MONDAY, &overlapping, FIVE, now),
            Err(OverrideError::InvalidBlocks(_))
        ));
    }

    #[test]
    fn override_today_uses_the_business_date() {
        // Tuesday 01:30 is still Monday's business day: Monday is "today".
        let now = athens(TUESDAY, 1, 30);
        assert_eq!(
            validate_override(OverrideKind::Off, MONDAY, &[], FIVE, now),
            Ok(())
        );
        let now = athens(TUESDAY, 5, 0);
        assert_eq!(
            validate_override(OverrideKind::Off, MONDAY, &[], FIVE, now),
            Err(OverrideError::PastDate)
        );
    }

    #[test]
    fn names() {
        assert_eq!(normalize_name("  Μαρία  "), Ok("Μαρία".to_string()));
        assert_eq!(normalize_name("Anne-Marie"), Ok("Anne-Marie".to_string()));
        assert_eq!(normalize_name("O'Brien"), Ok("O'Brien".to_string()));
        assert_eq!(normalize_name("D’Angelo"), Ok("D’Angelo".to_string()));
        assert_eq!(normalize_name("Ζωή Ελένη"), Ok("Ζωή Ελένη".to_string()));
        assert_eq!(normalize_name("Ἀθηνᾶ"), Ok("Ἀθηνᾶ".to_string()));
        assert_eq!(normalize_name("José"), Ok("José".to_string()));
        assert_eq!(normalize_name("   "), Err(NameError::Empty));
        assert_eq!(normalize_name(""), Err(NameError::Empty));
        assert_eq!(normalize_name("--"), Err(NameError::NoLetters));
        assert_eq!(
            normalize_name("R2D2"),
            Err(NameError::InvalidCharacter('2'))
        );
        assert_eq!(normalize_name("a.b"), Err(NameError::InvalidCharacter('.')));
        assert_eq!(
            normalize_name("Иван"),
            Err(NameError::InvalidCharacter('И'))
        );
        assert_eq!(normalize_name("a×b"), Err(NameError::InvalidCharacter('×')));
        assert_eq!(normalize_name(&"α".repeat(40)), Ok("α".repeat(40)));
        assert_eq!(normalize_name(&"α".repeat(41)), Err(NameError::TooLong));
    }

    #[test]
    fn names_are_normalised_to_nfc() {
        // "ά" typed as α + combining acute becomes one character.
        let decomposed = "Ma\u{0301}ria-\u{03B1}\u{0301}";
        assert_eq!(normalize_name(decomposed), Ok("Mária-ά".to_string()));
    }

    #[test]
    fn settings_ranges() {
        let ok = ScheduleSettings::default();
        assert_eq!(validate_settings(&ok), Ok(()));
        let edges = ScheduleSettings {
            checkin_offset_min: -60,
            checkout_offset_min: 30,
            rollover: time(8, 0, 0, 0),
        };
        assert_eq!(validate_settings(&edges), Ok(()));
        let midnight = ScheduleSettings {
            rollover: time(0, 0, 0, 0),
            ..ok
        };
        assert_eq!(validate_settings(&midnight), Ok(()));
        let bad = ScheduleSettings {
            checkin_offset_min: -61,
            checkout_offset_min: 31,
            rollover: time(8, 1, 0, 0),
        };
        assert_eq!(
            validate_settings(&bad),
            Err(vec![
                SettingsProblem::CheckinOffset,
                SettingsProblem::CheckoutOffset,
                SettingsProblem::Rollover,
            ])
        );
    }

    #[test]
    fn quit_codes() {
        for good in ["0000", "1234", "9999"] {
            assert!(is_valid_quit_code(good), "{good}");
        }
        for bad in ["", "123", "12345", "12a4", " 1234", "١٢٣٤", "12 4"] {
            assert!(!is_valid_quit_code(bad), "{bad:?}");
        }
    }

    #[test]
    fn errors_serialise_for_the_ui() {
        let err = WeekError::InvalidBlocks(vec![BlockIssue {
            index: 2,
            problem: BlockProblem::Overlaps { other: 1 },
        }]);
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            serde_json::json!({
                "error": "invalid_blocks",
                "issues": [{ "index": 2, "problem": "overlaps", "other": 1 }]
            })
        );
        assert_eq!(
            serde_json::to_value(NameError::InvalidCharacter('2')).unwrap(),
            serde_json::json!({ "error": "invalid_character", "character": "2" })
        );
    }
}
