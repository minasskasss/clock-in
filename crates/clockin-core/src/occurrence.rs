//! Concrete block occurrences for one business date: the weekly template with
//! the date's overrides applied, turned into real instants.

use crate::business_day::BlockLayout;
use crate::model::{Override, OverrideKind, Staff, WeeklyBlock};
use crate::shop_time::resolve_local;
use jiff::Timestamp;
use jiff::civil::{Date, Time};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;
use uuid::Uuid;

/// One block of one person on one business date.
///
/// It is identified by (`source_block_id`, `business_date`); the source is a
/// weekly block or an override block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub staff_id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub source_block_id: Uuid,
    pub business_date: Date,
    /// Wall-clock start and end, as entered (for display).
    pub start_time: Time,
    pub end_time: Time,
    /// Show "(+1)" next to the start / end time.
    pub start_next_day: bool,
    pub end_next_day: bool,
    /// Real instants, after the DST rules of [`crate::resolve_local`].
    pub start: Timestamp,
    pub end: Timestamp,
}

impl Occurrence {
    /// "First Last".
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
    }

    /// The display order of SPEC §6: start time, then last name, then first
    /// name; ids break any remaining tie so the order is always the same.
    #[must_use]
    pub fn display_cmp(&self, other: &Self) -> Ordering {
        self.start
            .cmp(&other.start)
            .then_with(|| name_sort_key(&self.last_name).cmp(&name_sort_key(&other.last_name)))
            .then_with(|| name_sort_key(&self.first_name).cmp(&name_sort_key(&other.first_name)))
            .then_with(|| self.staff_id.cmp(&other.staff_id))
            .then_with(|| self.business_date.cmp(&other.business_date))
            .then_with(|| self.source_block_id.cmp(&other.source_block_id))
    }
}

/// All occurrences on business date `date`, in display order.
///
/// - A person with an override that date gets the override's blocks (none for
///   a day off) instead of the template's.
/// - Removed staff keep only blocks that started before the removal.
#[must_use]
pub fn occurrences(
    staff: &[Staff],
    blocks: &[WeeklyBlock],
    overrides: &[Override],
    date: Date,
    rollover: Time,
) -> Vec<Occurrence> {
    let weekday = date.weekday();
    let mut out = Vec::new();
    for person in staff {
        // `(staff_id, business_date)` is unique on the server; take the first.
        let override_today = overrides
            .iter()
            .find(|o| o.staff_id == person.id && o.business_date == date);
        let times: Vec<(Uuid, Time, Time)> = match override_today {
            Some(o) => match o.kind {
                OverrideKind::Off => Vec::new(),
                OverrideKind::Replace => o.blocks.iter().map(|b| (b.id, b.start, b.end)).collect(),
            },
            None => blocks
                .iter()
                .filter(|b| b.staff_id == person.id && b.weekday == weekday)
                .map(|b| (b.id, b.start, b.end))
                .collect(),
        };
        for (block_id, start_time, end_time) in times {
            let Some(occ) = occurrence(person, block_id, start_time, end_time, date, rollover)
            else {
                continue;
            };
            if person.works_block_starting(occ.start) {
                out.push(occ);
            }
        }
    }
    out.sort_by(Occurrence::display_cmp);
    out
}

fn occurrence(
    person: &Staff,
    block_id: Uuid,
    start_time: Time,
    end_time: Time,
    date: Date,
    rollover: Time,
) -> Option<Occurrence> {
    let layout = BlockLayout::new(start_time, end_time, rollover);
    let start = resolve_local(layout.start_on(date)?)?;
    let end = resolve_local(layout.end_on(date)?)?;
    Some(Occurrence {
        staff_id: person.id,
        first_name: person.first_name.clone(),
        last_name: person.last_name.clone(),
        source_block_id: block_id,
        business_date: date,
        start_time,
        end_time,
        start_next_day: layout.start_next_day,
        end_next_day: layout.end_next_day,
        start,
        end,
    })
}

/// A sort key for names: accents removed, lowercase, final sigma folded, so
/// "Άννα", "ΑΝΝΑ" and "αννα" sort together.
pub(crate) fn name_sort_key(name: &str) -> String {
    name.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ς' { 'σ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OverrideBlock;
    use crate::test_support::*;
    use jiff::civil::{Weekday, date, time};

    #[test]
    fn name_sort_key_folds_accents_case_and_final_sigma() {
        assert_eq!(name_sort_key("Άννα"), "αννα");
        assert_eq!(name_sort_key("ΑΝΝΑ"), "αννα");
        assert_eq!(name_sort_key("Νίκος"), "νικοσ");
        assert_eq!(name_sort_key("Ζωή-Ϊνα"), "ζωη-ινα");
        assert_eq!(name_sort_key("Émile"), "emile");
    }

    #[test]
    fn plain_day_shift() {
        let a = staff(1, "Anna", "Alpha");
        let blocks = [weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))];
        let occ = occurrences(&[a], &blocks, &[], MONDAY, FIVE);
        assert_eq!(occ.len(), 1);
        let o = &occ[0];
        assert_eq!(o.start, athens(MONDAY, 9, 0));
        assert_eq!(o.end, athens(MONDAY, 17, 0));
        assert_eq!(o.source_block_id, id(10));
        assert_eq!(o.business_date, MONDAY);
        assert!(!o.start_next_day && !o.end_next_day);
        // Nothing on Tuesday.
        assert!(occurrences(&[staff(1, "Anna", "Alpha")], &blocks, &[], TUESDAY, FIVE).is_empty());
    }

    #[test]
    fn split_shift_gives_one_occurrence_per_block() {
        let b = staff(2, "Babis", "Beta");
        let blocks = [
            weekly(21, &b, Weekday::Monday, (19, 0), (2, 0)),
            weekly(20, &b, Weekday::Monday, (12, 0), (16, 0)),
        ];
        let occ = occurrences(&[b], &blocks, &[], MONDAY, FIVE);
        assert_eq!(occ.len(), 2);
        assert_eq!(occ[0].source_block_id, id(20));
        assert_eq!(occ[0].start, athens(MONDAY, 12, 0));
        assert_eq!(occ[1].source_block_id, id(21));
        assert_eq!(occ[1].start, athens(MONDAY, 19, 0));
        assert_eq!(occ[1].end, athens(TUESDAY, 2, 0));
        assert!(occ[1].end_next_day);
    }

    #[test]
    fn end_at_midnight_and_after_midnight() {
        let b = staff(2, "Babis", "Beta");
        let blocks = [
            weekly(20, &b, Weekday::Monday, (12, 0), (16, 0)),
            weekly(21, &b, Weekday::Monday, (19, 0), (0, 0)),
        ];
        let occ = occurrences(&[b], &blocks, &[], MONDAY, FIVE);
        assert_eq!(occ[1].end, athens(TUESDAY, 0, 0));
        assert!(
            !occ[1].end_next_day,
            "00:00 is shown as midnight, without (+1)"
        );
    }

    #[test]
    fn start_before_rollover_is_the_following_night() {
        let c = staff(3, "Christos", "Gamma");
        let blocks = [weekly(30, &c, Weekday::Monday, (1, 0), (4, 0))];
        let occ = occurrences(&[c], &blocks, &[], MONDAY, FIVE);
        assert_eq!(occ[0].start, athens(TUESDAY, 1, 0));
        assert_eq!(occ[0].end, athens(TUESDAY, 4, 0));
        assert!(occ[0].start_next_day && occ[0].end_next_day);
        // The Tuesday business day does not pick it up.
        assert!(
            occurrences(
                &[staff(3, "Christos", "Gamma")],
                &blocks,
                &[],
                TUESDAY,
                FIVE
            )
            .is_empty()
        );
    }

    #[test]
    fn day_off_override_removes_the_day() {
        let a = staff(1, "Anna", "Alpha");
        let blocks = [weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))];
        let overrides = [day_off(90, &a, MONDAY)];
        assert!(
            occurrences(std::slice::from_ref(&a), &blocks, &overrides, MONDAY, FIVE).is_empty()
        );
        // Only that date: next Monday is back to normal.
        let next_monday = date(2026, 6, 8);
        assert_eq!(
            occurrences(&[a], &blocks, &overrides, next_monday, FIVE).len(),
            1
        );
    }

    #[test]
    fn replace_override_swaps_the_blocks() {
        let a = staff(1, "Anna", "Alpha");
        let blocks = [weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))];
        let overrides = [replace(90, &a, MONDAY, &[(91, (10, 0), (14, 0))])];
        let occ = occurrences(&[a], &blocks, &overrides, MONDAY, FIVE);
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].source_block_id, id(91));
        assert_eq!(occ[0].start, athens(MONDAY, 10, 0));
        assert_eq!(occ[0].end, athens(MONDAY, 14, 0));
    }

    #[test]
    fn replace_override_adds_someone_who_does_not_normally_work() {
        let c = staff(3, "Christos", "Gamma");
        let overrides = [replace(90, &c, MONDAY, &[(91, (10, 0), (14, 0))])];
        let occ = occurrences(&[c], &[], &overrides, MONDAY, FIVE);
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].source_block_id, id(91));
    }

    #[test]
    fn override_for_someone_else_or_another_date_is_ignored() {
        let a = staff(1, "Anna", "Alpha");
        let b = staff(2, "Babis", "Beta");
        let blocks = [weekly(10, &a, Weekday::Monday, (9, 0), (17, 0))];
        let overrides = [day_off(90, &b, MONDAY), day_off(91, &a, TUESDAY)];
        assert_eq!(
            occurrences(&[a, b], &blocks, &overrides, MONDAY, FIVE).len(),
            1
        );
    }

    #[test]
    fn replace_override_with_split_and_overnight_blocks() {
        let a = staff(1, "Anna", "Alpha");
        let o = Override {
            id: id(90),
            staff_id: a.id,
            business_date: MONDAY,
            kind: OverrideKind::Replace,
            blocks: vec![
                OverrideBlock {
                    id: id(92),
                    start: time(20, 0, 0, 0),
                    end: time(3, 0, 0, 0),
                },
                OverrideBlock {
                    id: id(91),
                    start: time(11, 0, 0, 0),
                    end: time(15, 0, 0, 0),
                },
            ],
        };
        let occ = occurrences(&[a], &[], &[o], MONDAY, FIVE);
        assert_eq!(
            occ.iter().map(|o| o.source_block_id).collect::<Vec<_>>(),
            [id(91), id(92)]
        );
        assert_eq!(occ[1].end, athens(TUESDAY, 3, 0));
    }

    #[test]
    fn ordered_by_start_then_last_name_then_first_name() {
        let people = [
            staff(1, "Ζωή", "Παπά"),
            staff(2, "Άννα", "Παπά"),
            staff(3, "Μαρία", "Άλφα"),
            staff(4, "Νίκος", "Αλεξίου"),
            staff(5, "Early", "Ωμέγα"),
        ];
        let blocks: Vec<WeeklyBlock> = people
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let start = if i == 4 { (8, 0) } else { (9, 0) };
                weekly(10 * (i as u128 + 1), p, Weekday::Monday, start, (17, 0))
            })
            .collect();
        let names: Vec<String> = occurrences(&people, &blocks, &[], MONDAY, FIVE)
            .iter()
            .map(Occurrence::display_name)
            .collect();
        // Earliest start first; then last name ignoring accents (Αλεξίου <
        // Άλφα < Παπά), then first name (Άννα < Ζωή).
        assert_eq!(
            names,
            [
                "Early Ωμέγα",
                "Νίκος Αλεξίου",
                "Μαρία Άλφα",
                "Άννα Παπά",
                "Ζωή Παπά"
            ]
        );
    }

    #[test]
    fn removed_staff_disappear_from_the_next_block_onward() {
        let mut b = staff(2, "Babis", "Beta");
        let blocks = [
            weekly(20, &b, Weekday::Monday, (12, 0), (16, 0)),
            weekly(21, &b, Weekday::Monday, (19, 0), (2, 0)),
        ];
        // Removed at 13:00 on Monday: the lunch block stays, the evening goes.
        b.removed_at = Some(athens(MONDAY, 13, 0));
        let occ = occurrences(&[b.clone()], &blocks, &[], MONDAY, FIVE);
        assert_eq!(
            occ.iter().map(|o| o.source_block_id).collect::<Vec<_>>(),
            [id(20)]
        );
        // And from all future days.
        let next_monday = date(2026, 6, 8);
        assert!(occurrences(&[b], &blocks, &[], next_monday, FIVE).is_empty());
    }

    #[test]
    fn fall_back_night_shift_is_one_hour_longer_in_real_time() {
        // Saturday 2026-10-24, 22:00–06:00: clocks go back at 04:00 on Sunday.
        let a = staff(1, "Anna", "Alpha");
        let sat = date(2026, 10, 24);
        let blocks = [weekly(10, &a, Weekday::Saturday, (22, 0), (6, 0))];
        let occ = occurrences(&[a], &blocks, &[], sat, FIVE);
        assert_eq!(occ[0].start, utc("2026-10-24T19:00:00Z")); // 22:00 EEST
        assert_eq!(occ[0].end, utc("2026-10-25T04:00:00Z")); // 06:00 EET
        assert_eq!(occ[0].end.duration_since(occ[0].start).as_hours(), 9);
    }

    #[test]
    fn fall_back_ambiguous_start_takes_the_first_occurrence() {
        // 03:30–04:30 on the night after Saturday 2026-10-24.
        let a = staff(1, "Anna", "Alpha");
        let blocks = [weekly(10, &a, Weekday::Saturday, (3, 30), (4, 30))];
        let occ = occurrences(&[a], &blocks, &[], date(2026, 10, 24), FIVE);
        assert_eq!(occ[0].start, utc("2026-10-25T00:30:00Z")); // first 03:30 (EEST)
        assert_eq!(occ[0].end, utc("2026-10-25T02:30:00Z")); // 04:30 EET
    }

    #[test]
    fn spring_forward_nonexistent_start_moves_to_04_00() {
        // 03:30–05:00 on the night after Saturday 2027-03-27.
        let a = staff(1, "Anna", "Alpha");
        let blocks = [weekly(10, &a, Weekday::Saturday, (3, 30), (5, 0))];
        let occ = occurrences(&[a], &blocks, &[], date(2027, 3, 27), FIVE);
        assert_eq!(occ[0].start, utc("2027-03-28T01:00:00Z")); // 04:00 EEST
        assert_eq!(occ[0].end, utc("2027-03-28T02:00:00Z")); // 05:00 EEST
    }

    #[test]
    fn spring_forward_night_shift_is_one_hour_shorter_in_real_time() {
        let a = staff(1, "Anna", "Alpha");
        let blocks = [weekly(10, &a, Weekday::Saturday, (22, 0), (6, 0))];
        let occ = occurrences(&[a], &blocks, &[], date(2027, 3, 27), FIVE);
        assert_eq!(occ[0].start, utc("2027-03-27T20:00:00Z")); // 22:00 EET
        assert_eq!(occ[0].end, utc("2027-03-28T03:00:00Z")); // 06:00 EEST
        assert_eq!(occ[0].end.duration_since(occ[0].start).as_hours(), 7);
    }
}
