//! What the Settings editors send: times as typed ("HH:MM"), checked with
//! the core rules and reported back block by block, so the editor can show
//! "(+1)" and a plain-language problem next to each block while typing.

use clockin_core::{
    BlockLayout, BlockProblem, DayBlock, OverrideError, OverrideKind, TimeRange, WeekError,
    validate_block, validate_override, validate_week,
};
use clockin_sync::{OverrideBlockInput, WeekBlockInput};
use jiff::Timestamp;
use jiff::civil::{Date, Time, Weekday};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A weekly block as typed in the editor.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekBlockDraft {
    /// Kept when an existing block is edited, so its marks stay attached.
    #[serde(default)]
    pub id: Option<Uuid>,
    /// 1 = Monday … 7 = Sunday (business day).
    pub weekday: i8,
    pub start: String,
    pub end: String,
}

/// A replace-hours block as typed in the editor.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeDraft {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub start: String,
    pub end: String,
}

/// The editor's view of one block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockReport {
    pub start_next_day: bool,
    pub end_next_day: bool,
    /// `bad_start`, `bad_end`, `bad_weekday`, `start_equals_end`,
    /// `too_short`, `too_long`, `overlaps` (with `other`).
    pub problem: Option<&'static str>,
    /// For `overlaps`: the index of the block it overlaps.
    pub other: Option<usize>,
}

/// The editor's view of a whole weekly schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekReport {
    pub blocks: Vec<BlockReport>,
    /// A staff member needs at least one block.
    pub no_blocks: bool,
    pub ok: bool,
}

/// The editor's view of a one-off change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideReport {
    /// `bad_date` or `past_date`.
    pub date_problem: Option<&'static str>,
    pub blocks: Vec<BlockReport>,
    /// Replace hours needs at least one block.
    pub no_blocks: bool,
    pub ok: bool,
}

/// Parses "HH:MM" (or "H:MM") as a 24-hour time.
#[must_use]
pub fn parse_hhmm(text: &str) -> Option<Time> {
    let (h, m) = text.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    if !h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    Time::new(h.parse().ok()?, m.parse().ok()?, 0, 0).ok()
}

/// "HH:MM".
#[must_use]
pub fn hhmm(time: Time) -> String {
    format!("{:02}:{:02}", time.hour(), time.minute())
}

/// Parses an ISO date ("2026-10-05").
#[must_use]
pub fn parse_date(text: &str) -> Option<Date> {
    let text = text.trim();
    (text.len() == 10).then(|| text.parse().ok()).flatten()
}

fn parse_range(start: &str, end: &str) -> Result<TimeRange, &'static str> {
    let start = parse_hhmm(start).ok_or("bad_start")?;
    let end = parse_hhmm(end).ok_or("bad_end")?;
    Ok(TimeRange { start, end })
}

fn layout_report(layout: BlockLayout) -> BlockReport {
    BlockReport {
        start_next_day: layout.start_next_day,
        end_next_day: layout.end_next_day,
        ..BlockReport::default()
    }
}

/// "(+1)" flags for a stored block.
#[must_use]
pub fn layout_flags(start: Time, end: Time, rollover: Time) -> (bool, bool) {
    let layout = BlockLayout::new(start, end, rollover);
    (layout.start_next_day, layout.end_next_day)
}

fn problem_code(problem: BlockProblem) -> (&'static str, Option<usize>) {
    match problem {
        // The editor only produces whole minutes; treat it as a bad time.
        BlockProblem::NotWholeMinute => ("bad_start", None),
        BlockProblem::StartEqualsEnd => ("start_equals_end", None),
        BlockProblem::TooShort => ("too_short", None),
        BlockProblem::TooLong => ("too_long", None),
        BlockProblem::Overlaps { other } => ("overlaps", Some(other)),
    }
}

/// Per-block reports: first the parse and single-block checks, then the
/// core's whole-list result (overlaps) mapped back to the original indices.
fn block_reports(parsed: &[Result<TimeRange, &'static str>], rollover: Time) -> Vec<BlockReport> {
    parsed
        .iter()
        .map(|p| match p {
            Err(code) => BlockReport {
                problem: Some(code),
                ..BlockReport::default()
            },
            Ok(range) => match validate_block(*range, rollover) {
                Ok(layout) => layout_report(layout),
                Err(problem) => {
                    let layout = BlockLayout::new(range.start, range.end, rollover);
                    let (code, other) = problem_code(problem);
                    BlockReport {
                        problem: Some(code),
                        other,
                        ..layout_report(layout)
                    }
                }
            },
        })
        .collect()
}

/// Applies the core's overlap issues (indices into `kept`) to `reports`.
fn apply_overlaps(
    reports: &mut [BlockReport],
    kept: &[usize],
    issues: impl IntoIterator<Item = clockin_core::BlockIssue>,
) {
    for issue in issues {
        if let BlockProblem::Overlaps { other } = issue.problem
            && let (Some(&index), Some(&other)) = (kept.get(issue.index), kept.get(other))
            && let Some(report) = reports.get_mut(index)
            && report.problem.is_none()
        {
            report.problem = Some("overlaps");
            report.other = Some(other);
        }
    }
}

/// Checks a weekly schedule as typed.
#[must_use]
pub fn check_week(drafts: &[WeekBlockDraft], rollover: Time) -> WeekReport {
    let parsed: Vec<_> = drafts
        .iter()
        .map(|d| {
            Weekday::from_monday_one_offset(d.weekday)
                .map_err(|_| "bad_weekday")
                .and_then(|_| parse_range(&d.start, &d.end))
        })
        .collect();
    let mut reports = block_reports(&parsed, rollover);
    // Overlaps only among blocks that are fine on their own.
    let kept: Vec<usize> = (0..drafts.len())
        .filter(|&i| reports[i].problem.is_none())
        .collect();
    let day_blocks: Vec<DayBlock> = kept
        .iter()
        .filter_map(|&i| {
            let range = parsed[i].ok()?;
            Some(DayBlock {
                weekday: Weekday::from_monday_one_offset(drafts[i].weekday).ok()?,
                start: range.start,
                end: range.end,
            })
        })
        .collect();
    if let Err(WeekError::InvalidBlocks(issues)) = validate_week(&day_blocks, rollover) {
        apply_overlaps(&mut reports, &kept, issues);
    }
    let no_blocks = drafts.is_empty();
    let ok = !no_blocks && reports.iter().all(|r| r.problem.is_none());
    WeekReport {
        blocks: reports,
        no_blocks,
        ok,
    }
}

/// Checks a one-off change as typed.
#[must_use]
pub fn check_override(
    kind: OverrideKind,
    date: &str,
    drafts: &[RangeDraft],
    rollover: Time,
    now: Timestamp,
) -> OverrideReport {
    let drafts: &[RangeDraft] = if kind == OverrideKind::Off {
        &[]
    } else {
        drafts
    };
    let parsed: Vec<_> = drafts
        .iter()
        .map(|d| parse_range(&d.start, &d.end))
        .collect();
    let mut reports = block_reports(&parsed, rollover);
    let kept: Vec<usize> = (0..drafts.len())
        .filter(|&i| reports[i].problem.is_none())
        .collect();
    let ranges: Vec<TimeRange> = kept.iter().filter_map(|&i| parsed[i].ok()).collect();

    let mut date_problem = None;
    match parse_date(date) {
        None => date_problem = Some("bad_date"),
        Some(day) => {
            // Check the date on its own, then the blocks, so both show at once.
            if validate_override(OverrideKind::Off, day, &[], rollover, now)
                == Err(OverrideError::PastDate)
            {
                date_problem = Some("past_date");
            }
            if kind == OverrideKind::Replace
                && !ranges.is_empty()
                && let Err(OverrideError::InvalidBlocks(issues)) =
                    validate_override(kind, far_future(), &ranges, rollover, now)
            {
                apply_overlaps(&mut reports, &kept, issues);
            }
        }
    }
    let no_blocks = kind == OverrideKind::Replace && drafts.is_empty();
    let ok = date_problem.is_none() && !no_blocks && reports.iter().all(|r| r.problem.is_none());
    OverrideReport {
        date_problem,
        blocks: reports,
        no_blocks,
        ok,
    }
}

/// A date that is never in the past, for checking blocks apart from the date.
fn far_future() -> Date {
    Date::constant(9999, 1, 1)
}

/// The server's shape of a checked weekly schedule. `None` if any draft
/// doesn't parse.
#[must_use]
pub fn week_inputs(drafts: &[WeekBlockDraft]) -> Option<Vec<WeekBlockInput>> {
    drafts
        .iter()
        .map(|d| {
            Some(WeekBlockInput {
                id: d.id,
                weekday: Weekday::from_monday_one_offset(d.weekday).ok()?,
                start: parse_hhmm(&d.start)?,
                end: parse_hhmm(&d.end)?,
            })
        })
        .collect()
}

/// The server's shape of checked replace-hours blocks.
#[must_use]
pub fn override_inputs(drafts: &[RangeDraft]) -> Option<Vec<OverrideBlockInput>> {
    drafts
        .iter()
        .map(|d| {
            Some(OverrideBlockInput {
                id: d.id,
                start: parse_hhmm(&d.start)?,
                end: parse_hhmm(&d.end)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::time;

    const ROLLOVER: Time = Time::constant(5, 0, 0, 0);

    fn wb(weekday: i8, start: &str, end: &str) -> WeekBlockDraft {
        WeekBlockDraft {
            id: None,
            weekday,
            start: start.into(),
            end: end.into(),
        }
    }

    fn rd(start: &str, end: &str) -> RangeDraft {
        RangeDraft {
            id: None,
            start: start.into(),
            end: end.into(),
        }
    }

    /// Monday 2026-10-05, 09:00 in Athens.
    fn now() -> Timestamp {
        "2026-10-05T06:00:00Z".parse().unwrap()
    }

    #[test]
    fn parses_24_hour_times_strictly() {
        assert_eq!(parse_hhmm("09:30"), Some(time(9, 30, 0, 0)));
        assert_eq!(parse_hhmm("9:30"), Some(time(9, 30, 0, 0)));
        assert_eq!(parse_hhmm(" 00:00 "), Some(time(0, 0, 0, 0)));
        assert_eq!(parse_hhmm("23:59"), Some(time(23, 59, 0, 0)));
        for bad in [
            "24:00", "12:60", "1230", "12:3", "", ":30", "ab:cd", "-1:00", "123:00",
        ] {
            assert_eq!(parse_hhmm(bad), None, "{bad}");
        }
        assert_eq!(hhmm(time(7, 5, 0, 0)), "07:05");
        assert_eq!(parse_date("2026-10-05"), Some(Date::constant(2026, 10, 5)));
        assert_eq!(parse_date("2026-13-05"), None);
        assert_eq!(parse_date("5/10/2026"), None);
    }

    #[test]
    fn split_shift_with_a_night_end_is_fine_and_flags_plus_one() {
        let r = check_week(
            &[wb(1, "12:00", "16:00"), wb(1, "19:00", "02:00")],
            ROLLOVER,
        );
        assert!(r.ok);
        assert!(!r.blocks[0].end_next_day);
        assert!(r.blocks[1].end_next_day);
        assert!(!r.blocks[1].start_next_day);
        // Midnight is not "+1".
        let r = check_week(&[wb(1, "19:00", "00:00")], ROLLOVER);
        assert!(r.ok);
        assert!(!r.blocks[0].end_next_day);
        // A start before the rollover is the following night.
        let r = check_week(&[wb(5, "01:00", "04:00")], ROLLOVER);
        assert!(r.blocks[0].start_next_day && r.blocks[0].end_next_day);
    }

    #[test]
    fn week_problems_are_reported_per_block() {
        let r = check_week(
            &[
                wb(1, "09:00", "17:00"),
                wb(1, "16:00", "20:00"),
                wb(2, "9", "17:00"),
                wb(3, "10:00", "10:10"),
                wb(4, "10:00", "10:00"),
                wb(5, "06:00", "23:00"),
                wb(8, "10:00", "12:00"),
                wb(2, "10:00", "12:00"),
            ],
            ROLLOVER,
        );
        let problems: Vec<_> = r.blocks.iter().map(|b| (b.problem, b.other)).collect();
        assert_eq!(
            problems,
            [
                (None, None),
                (Some("overlaps"), Some(0)),
                (Some("bad_start"), None),
                (Some("too_short"), None),
                (Some("start_equals_end"), None),
                (Some("too_long"), None),
                (Some("bad_weekday"), None),
                (None, None),
            ]
        );
        assert!(!r.ok);
        let empty = check_week(&[], ROLLOVER);
        assert!(empty.no_blocks && !empty.ok);
    }

    #[test]
    fn overlap_indices_skip_broken_blocks() {
        // Block 1 is broken, so the core sees [0, 2]; index 2 must map back.
        let r = check_week(
            &[
                wb(1, "09:00", "17:00"),
                wb(1, "xx", "17:00"),
                wb(1, "16:00", "18:00"),
            ],
            ROLLOVER,
        );
        assert_eq!(r.blocks[2].problem, Some("overlaps"));
        assert_eq!(r.blocks[2].other, Some(0));
    }

    #[test]
    fn override_checks_date_and_blocks_together() {
        let today = "2026-10-05";
        let ok = check_override(OverrideKind::Off, today, &[], ROLLOVER, now());
        assert!(ok.ok);
        // Blocks sent with a day off are ignored.
        let ok = check_override(OverrideKind::Off, today, &[rd("1", "2")], ROLLOVER, now());
        assert!(ok.ok && ok.blocks.is_empty());

        let past = check_override(
            OverrideKind::Replace,
            "2026-10-04",
            &[rd("10:00", "14:00"), rd("13:00", "15:00")],
            ROLLOVER,
            now(),
        );
        assert_eq!(past.date_problem, Some("past_date"));
        assert_eq!(past.blocks[1].problem, Some("overlaps"));
        assert!(!past.ok);

        let none = check_override(OverrideKind::Replace, today, &[], ROLLOVER, now());
        assert!(none.no_blocks && !none.ok);
        let bad = check_override(OverrideKind::Off, "", &[], ROLLOVER, now());
        assert_eq!(bad.date_problem, Some("bad_date"));

        let fine = check_override(
            OverrideKind::Replace,
            "2026-10-06",
            &[rd("10:00", "14:00")],
            ROLLOVER,
            now(),
        );
        assert!(fine.ok);
    }

    #[test]
    fn yesterday_is_still_today_before_the_rollover() {
        // Tuesday 01:30 belongs to Monday's business day.
        let small_hours: Timestamp = "2026-10-05T22:30:00Z".parse().unwrap();
        let r = check_override(OverrideKind::Off, "2026-10-05", &[], ROLLOVER, small_hours);
        assert!(r.ok);
    }

    #[test]
    fn inputs_keep_ids_and_parse_times() {
        let id = Uuid::from_u128(7);
        let mut d = wb(7, "19:00", "02:00");
        d.id = Some(id);
        let inputs = week_inputs(&[d]).unwrap();
        assert_eq!(inputs[0].id, Some(id));
        assert_eq!(inputs[0].weekday, Weekday::Sunday);
        assert_eq!(inputs[0].end, time(2, 0, 0, 0));
        assert!(week_inputs(&[wb(1, "x", "02:00")]).is_none());
        assert_eq!(
            override_inputs(&[rd("10:00", "14:00")]).unwrap()[0].start,
            time(10, 0, 0, 0)
        );
    }
}
