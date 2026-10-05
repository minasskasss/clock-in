# Ideas (not in the v1 spec)

Ideas noticed while building. None of these is planned; Minas decides.

- **Show yesterday's still-running blocks after the rollover.** With a late rollover and a block ending after it (e.g. 22:00–06:00 with rollover 05:00), the row disappears from the Today view at 05:00 although the person is still working; the check-out alarm still fires. SPEC §5 says Today shows only the current business day, so v1 follows that.
- **Edit a one-off change in place.** Today it is deleted and added again (saving a new one for the same person and date replaces it).
- **Warn when a rollover change moves existing blocks.** Changing the rollover can turn a 04:00 start from "+1 night" into "same morning" and create overlaps that validation never saw.

## Proposed for v1.1 (after Phase 6)

### Problems history (admin-only, in Settings)

Out of v1 scope: SPEC §10 lists history screens as out of scope. Proposed by Minas for v1.1. **Do not build it in v1.**

- **What it records:** only things that went wrong, not every mark.
  - A block that ended with no check-in mark.
  - A block with no check-out mark by the end of its business day.
  - Still to decide: whether to also record late check-ins, and with what threshold.
- **Recorded at the time, with a snapshot of the expected block** (staff name, block times, business date). Schedules are overwritten when edited and can't be reconstructed later.
- **Idempotent**, keyed by (`staff_id`, `source_block_id`, `business_date`, `kind`), so any device can write an entry safely. The shop PC is the natural writer, because it is always on.
- **What counts:**
  - a mark voided in Today's marks counts as "not marked";
  - a day-off override or a removed shift produces no entry.
- **Wording:** the screen must say in plain words that it reflects Clock In taps, not ERGANI.
- **Retention and privacy:** entries are kept 30 days, like marks. Staff should be told the history exists (GDPR transparency).
- **Fit with what already exists** (notes, not decisions):
  - `clockin-core` can already tell whether an occurrence has its marks (`occurrences`, `has_mark`) and when a business day ends (`business_day_start` of the next date). Detection would need no new time logic.
  - Entries keep their own copy of the name. They should not hold a foreign key that stops removed staff from being deleted (SPEC §9: deleted once no marks reference them).
  - The daily purge job would need one more table.
- **Phase 2:** don't design around this idea. If a Phase 2 choice would make it much harder to add later, note that in `docs/DECISIONS.md`.
