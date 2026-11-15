<!-- Posted as https://github.com/twentyhq/twenty/issues/30460 on 2026-11-15. -->

**Title:** Trash cleanup permanently deletes records up to a day before the retention period ends

## Bug Description

Soft-deleted records can be permanently deleted almost a full day before the workspace's trash retention period ends. The legal FAQ says soft-deleted records "are kept for 14 days, after which they are permanently removed", but with the default of 14 days a record is kept anywhere between 13 and 14 days, depending on the time of day it was deleted:

| `trashRetentionDays` | Deleted at (UTC) | Permanently deleted by the cleanup on | Time in the trash |
|---|---|---|---|
| 14 (default) | day 0, 00:05 | day 14, 00:10 | 14 d 0 h 5 min |
| 14 (default) | day 0, 23:50 | day 14, 00:10 | **13 d 0 h 20 min** |
| 1 | day 0, 23:59 | day 1, 00:10 | **11 min** |

With a retention of 1 day, a record deleted just before midnight UTC is gone a few minutes later, so it can't be restored the next morning.

The cause is in `TrashCleanupService.calculateCutoffDate` (`packages/twenty-server/src/engine/trash-cleanup/services/trash-cleanup.service.ts`):

```ts
private calculateCutoffDate(trashRetentionDays: number): Date {
  const cutoffDate = new Date();

  cutoffDate.setUTCHours(0, 0, 0, 0);
  cutoffDate.setDate(cutoffDate.getDate() - trashRetentionDays + 1);

  return cutoffDate;
}
```

The cutoff is today's midnight UTC minus `trashRetentionDays - 1` days, and records with `deletedAt < cutoff` are deleted by the daily `TrashCleanupCronJob` (`10 0 * * *`). Everything deleted during day 0 is therefore removed at 00:10 on day `trashRetentionDays`, which is up to a day early for records deleted late in the day.

A smaller related point: it mixes `setUTCHours` with local-time `setDate`/`getDate`, so the result also depends on the server's time zone when it isn't UTC.

## Expected behavior

A record is never permanently deleted before it has been in the trash for `trashRetentionDays` days. Because the cleanup runs once a day, it may stay up to a day longer, which matches "kept for 14 days, after which they are permanently removed".

## Technical inputs

Suggested fix (verified, see below): drop the `+ 1` and stay in UTC:

```ts
cutoffDate.setUTCHours(0, 0, 0, 0);
cutoffDate.setUTCDate(cutoffDate.getUTCDate() - trashRetentionDays);
```

Records are then kept between `trashRetentionDays` and `trashRetentionDays + 1` days. Alternatively, `new Date(Date.now() - trashRetentionDays * 24 * 60 * 60 * 1000)` gives an exact cutoff. I couldn't find unit tests for `TrashCleanupService`; a test with `deletedAt` at 23:50 on day 0 and a run at 00:10 on day `trashRetentionDays` would cover this.

**How it was tested.** I ran the published `twentycrm/twenty:v2.58.0` image (server, worker, Postgres 16, Redis) with the seeded dev workspace under [ChronoStack](https://github.com/Asmodeus14/ChronoStack), an open-source tool that moves a Docker Compose stack forward in virtual time. BullMQ's own `TrashCleanupCronJob` ran every day by itself, with no mocks. Records were moved to the trash by setting `deletedAt` to the current time, as a soft delete does.

- One scenario pins the current behaviour in the table above: 16/16 checks pass on v2.58.0.
- A second scenario expects the documented behaviour (still in the trash just before the retention period ends, gone within a day after it, for 14 days and for 1 day). It fails on v2.58.0, because the record deleted at 23:50 is gone after 13 days. It passes 14/14 with the fix above applied in the worker.

Compose file, both scenarios and the three run reports: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/twenty

Happy to open a PR with the fix and a test.
