//! The Today view (SPEC §6): every block of the current business day with
//! its status.

use crate::business_day::business_date_for;
use crate::model::{Mark, MarkKind, Snapshot};
use crate::occurrence::{Occurrence, occurrences};
use crate::shop_time::shop_clock;
use jiff::Timestamp;
use jiff::civil::{Date, Time};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The status of a row (SPEC §3 status colours).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    /// Before the start time, not marked (neutral).
    Pending,
    /// Past the start time, not marked (amber).
    Late,
    /// Checked in, before the end time (green).
    CheckedIn,
    /// Checked in, past the end time, not marked out (green, amber accent).
    ShouldHaveLeft,
    /// Marked as left (green, red line through).
    Left,
}

/// One row of the Today view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodayRow {
    #[serde(flatten)]
    pub occurrence: Occurrence,
    pub status: RowStatus,
    /// The mark a tap on this row asks to make; `None` once the person left.
    pub next_mark: Option<MarkKind>,
}

/// The whole Today view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodayView {
    /// The business date shown; the header shows its weekday and date.
    pub business_date: Date,
    /// The header clock (HH:MM, Greek time).
    pub clock: Time,
    /// Ordered by start time, then last name.
    pub rows: Vec<TodayRow>,
}

/// Builds the Today view for the business day that contains `now`.
#[must_use]
pub fn today_view(snapshot: &Snapshot, now: Timestamp) -> TodayView {
    let rollover = snapshot.settings.rollover;
    let business_date = business_date_for(now, rollover);
    let rows = occurrences(
        &snapshot.staff,
        &snapshot.blocks,
        &snapshot.overrides,
        business_date,
        rollover,
    )
    .into_iter()
    .map(|occurrence| {
        let marked_in = has_mark(&snapshot.marks, &occurrence, MarkKind::In);
        let marked_out = has_mark(&snapshot.marks, &occurrence, MarkKind::Out);
        let (status, next_mark) = if marked_out {
            (RowStatus::Left, None)
        } else if marked_in {
            let status = if now >= occurrence.end {
                RowStatus::ShouldHaveLeft
            } else {
                RowStatus::CheckedIn
            };
            (status, Some(MarkKind::Out))
        } else if now >= occurrence.start {
            (RowStatus::Late, Some(MarkKind::In))
        } else {
            (RowStatus::Pending, Some(MarkKind::In))
        };
        TodayRow {
            occurrence,
            status,
            next_mark,
        }
    })
    .collect();
    TodayView {
        business_date,
        clock: shop_clock(now),
        rows,
    }
}

/// Whether `marks` holds a mark of `kind` for this occurrence.
#[must_use]
pub fn has_mark(marks: &[Mark], occurrence: &Occurrence, kind: MarkKind) -> bool {
    mark_exists(
        marks,
        occurrence.staff_id,
        occurrence.source_block_id,
        occurrence.business_date,
        kind,
    )
}

pub(crate) fn mark_exists(
    marks: &[Mark],
    staff_id: Uuid,
    source_block_id: Uuid,
    business_date: Date,
    kind: MarkKind,
) -> bool {
    marks.iter().any(|m| {
        m.kind == kind
            && m.staff_id == staff_id
            && m.source_block_id == source_block_id
            && m.business_date == business_date
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use jiff::civil::{Weekday, time};

    fn snapshot_ab() -> Snapshot {
        let a = staff(1, "Anna", "Alpha");
        let b = staff(2, "Babis", "Beta");
        Snapshot {
            blocks: vec![
                weekly(10, &a, Weekday::Monday, (9, 0), (17, 0)),
                weekly(20, &b, Weekday::Monday, (12, 0), (16, 0)),
                weekly(21, &b, Weekday::Monday, (19, 0), (2, 0)),
            ],
            staff: vec![a, b],
            ..Snapshot::default()
        }
    }

    fn statuses(view: &TodayView) -> Vec<(Uuid, RowStatus)> {
        view.rows
            .iter()
            .map(|r| (r.occurrence.source_block_id, r.status))
            .collect()
    }

    #[test]
    fn header_shows_business_date_and_clock() {
        let view = today_view(&snapshot_ab(), athens(TUESDAY, 1, 30));
        assert_eq!(view.business_date, MONDAY);
        assert_eq!(view.clock, time(1, 30, 0, 0));
    }

    #[test]
    fn rows_are_ordered_and_split_shift_appears_twice() {
        let view = today_view(&snapshot_ab(), athens(MONDAY, 8, 0));
        assert_eq!(
            statuses(&view),
            [
                (id(10), RowStatus::Pending),
                (id(20), RowStatus::Pending),
                (id(21), RowStatus::Pending),
            ]
        );
        assert!(view.rows[2].occurrence.end_next_day);
        assert!(view.rows.iter().all(|r| r.next_mark == Some(MarkKind::In)));
    }

    #[test]
    fn status_progression_through_the_day() {
        let mut snap = snapshot_ab();
        // 09:00 exactly: Anna is due.
        let view = today_view(&snap, athens(MONDAY, 9, 0));
        assert_eq!(view.rows[0].status, RowStatus::Late);

        snap.marks.push(mark(100, 1, 10, MONDAY, MarkKind::In));
        let view = today_view(&snap, athens(MONDAY, 9, 5));
        assert_eq!(view.rows[0].status, RowStatus::CheckedIn);
        assert_eq!(view.rows[0].next_mark, Some(MarkKind::Out));

        let view = today_view(&snap, athens(MONDAY, 17, 0));
        assert_eq!(view.rows[0].status, RowStatus::ShouldHaveLeft);
        assert_eq!(view.rows[0].next_mark, Some(MarkKind::Out));

        snap.marks.push(mark(101, 1, 10, MONDAY, MarkKind::Out));
        let view = today_view(&snap, athens(MONDAY, 17, 1));
        assert_eq!(view.rows[0].status, RowStatus::Left);
        assert_eq!(view.rows[0].next_mark, None);
    }

    #[test]
    fn checked_in_just_before_the_end_is_not_yet_should_have_left() {
        let mut snap = snapshot_ab();
        snap.marks.push(mark(100, 1, 10, MONDAY, MarkKind::In));
        let view = today_view(&snap, athens(MONDAY, 16, 59));
        assert_eq!(view.rows[0].status, RowStatus::CheckedIn);
    }

    #[test]
    fn marks_before_the_scheduled_time_count() {
        let mut snap = snapshot_ab();
        snap.marks.push(mark(100, 1, 10, MONDAY, MarkKind::In));
        let view = today_view(&snap, athens(MONDAY, 7, 0));
        assert_eq!(view.rows[0].status, RowStatus::CheckedIn);
    }

    #[test]
    fn a_never_marked_person_stays_late_after_the_end() {
        let view = today_view(&snapshot_ab(), athens(MONDAY, 18, 0));
        assert_eq!(view.rows[0].status, RowStatus::Late);
    }

    #[test]
    fn marks_apply_only_to_their_own_block_and_date() {
        let mut snap = snapshot_ab();
        // Babis checked in for lunch; the evening block is separate.
        snap.marks.push(mark(100, 2, 20, MONDAY, MarkKind::In));
        // A mark from last week's Monday does not count.
        snap.marks.push(mark(
            101,
            2,
            21,
            jiff::civil::date(2026, 5, 25),
            MarkKind::In,
        ));
        let view = today_view(&snap, athens(MONDAY, 12, 30));
        assert_eq!(
            statuses(&view),
            [
                (id(10), RowStatus::Late),
                (id(20), RowStatus::CheckedIn),
                (id(21), RowStatus::Pending),
            ]
        );
    }

    #[test]
    fn small_hours_still_show_the_previous_business_day() {
        let mut snap = snapshot_ab();
        snap.marks.push(mark(100, 2, 21, MONDAY, MarkKind::In));
        // Tuesday 01:30: Monday's 19:00–02:00 is still running.
        let view = today_view(&snap, athens(TUESDAY, 1, 30));
        assert_eq!(view.business_date, MONDAY);
        assert_eq!(view.rows[2].status, RowStatus::CheckedIn);
        // Tuesday 04:59 vs 05:00.
        assert_eq!(
            today_view(&snap, athens(TUESDAY, 4, 59)).business_date,
            MONDAY
        );
        let tuesday = today_view(&snap, athens(TUESDAY, 5, 0));
        assert_eq!(tuesday.business_date, TUESDAY);
        assert!(tuesday.rows.is_empty(), "nobody works on Tuesday");
    }

    #[test]
    fn empty_day() {
        let view = today_view(&Snapshot::default(), athens(MONDAY, 12, 0));
        assert!(view.rows.is_empty());
    }

    #[test]
    fn overrides_show_in_the_today_view() {
        let mut snap = snapshot_ab();
        let c = staff(3, "Christos", "Gamma");
        snap.overrides = vec![
            day_off(90, &snap.staff[0], MONDAY),
            replace(91, &c, MONDAY, &[(92, (10, 0), (14, 0))]),
        ];
        snap.staff.push(c);
        let view = today_view(&snap, athens(MONDAY, 8, 0));
        assert_eq!(
            view.rows
                .iter()
                .map(|r| r.occurrence.display_name())
                .collect::<Vec<_>>(),
            ["Christos Gamma", "Babis Beta", "Babis Beta"]
        );
    }

    #[test]
    fn rollover_setting_changes_which_day_is_shown() {
        let mut snap = snapshot_ab();
        snap.settings.rollover = time(0, 0, 0, 0);
        let view = today_view(&snap, athens(TUESDAY, 1, 30));
        assert_eq!(view.business_date, TUESDAY);
    }
}
