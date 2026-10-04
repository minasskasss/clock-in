# Ideas (not in the v1 spec)

Ideas noticed while building. None of these is planned; Minas decides.

- **Show yesterday's still-running blocks after the rollover.** With a late rollover and a block ending after it (e.g. 22:00–06:00 with rollover 05:00), the row disappears from the Today view at 05:00 although the person is still working; the check-out alarm still fires. SPEC §5 says Today shows only the current business day, so v1 follows that.
- **Warn when a rollover change moves existing blocks.** Changing the rollover can turn a 04:00 start from "+1 night" into "same morning" and create overlaps that validation never saw.
