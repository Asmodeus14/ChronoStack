<!-- Opened as https://github.com/twentyhq/twenty/pull/30461 on 2026-11-15 (branch fix/trash-cleanup-retention-cutoff on Asmodeus14/twenty). -->

**Title:** Fix trash cleanup deleting records before the retention period ends

Fixes #30460.

The trash cleanup cutoff was today's midnight UTC minus `trashRetentionDays - 1` days, so everything soft-deleted during a day was permanently deleted at the 00:10 run on day `trashRetentionDays`. Records deleted late in the day were kept almost a day less than the setting: 13 days 0 h 20 min with the default of 14, and 11 minutes with a retention of 1 day.

**Change**
- The cutoff is now midnight UTC minus `trashRetentionDays` days, so a record stays in the trash for at least `trashRetentionDays` full days and is deleted at the first daily run after that (at most one day later).
- It uses `setUTCDate`/`getUTCDate` throughout. The old code mixed `setUTCHours` with local-time `setDate`, so the result depended on the server's time zone.
- The calculation moved to `computeTrashCleanupCutoffDate` in `trash-cleanup/utils` so it can be unit-tested.

**Tests**
- New `compute-trash-cleanup-cutoff-date.util.spec.ts` (fake timers): for retentions of 1, 14 and 30 days, a record deleted at 23:59:59.999 survives the cleanup run on day `trashRetentionDays` and is deleted on the next run. The "survives" cases fail on the old code.
- Also verified end to end on the published v2.58.0 image with the fix applied in the worker. The run reports are linked in #30460.
