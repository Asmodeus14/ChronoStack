# Scenario report: Trash retention, as Twenty v2.58.0 behaves today

The seeded Apple workspace keeps the default trashRetentionDays of 14. Three people are moved to the trash (deletedAt set to the virtual now, as a soft delete does) at chosen times. BullMQ runs TrashCleanupCronJob every day at 00:10 UTC on its own, with no triggers. Each daily step lands at 00:15, after that run. This file pins what the code computes: cutoff = today 00:00 UTC minus (retention - 1) days.

**Result: PASSED ✅**  
Virtual time: 2026-11-15T04:19:49Z → 2026-12-01T00:15:06Z  
Real time: 156.6 s (started 2026-11-15T04:19:49Z)  
Scenario file: `scenarios/trash-retention-current-behaviour.yml` · ChronoStack project `twenty-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | day 0, 00:05 UTC | 2026-11-16T00:05:55Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 2 | move Jeffery (A) to the trash at 00:05 | 2026-11-16T00:05:55Z | ✅ passed | 2026-11-16 00:05 UPDATE 1 (saved as ${a_deleted_at}) |
| 3 | day 0, 23:50 UTC | 2026-11-16T23:50:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 4 | move Terry (B) to the trash at 23:50 | 2026-11-16T23:50:06Z | ✅ passed | 2026-11-16 23:50 UPDATE 1 (saved as ${b_deleted_at}) |
| 5 | to day 1, 00:15 (after the 00:10 cleanup) | 2026-11-17T00:15:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 6 | 12 more days, one day at a time | 2026-11-29T00:15:11Z | ✅ passed | 12 stop(s), 0 trigger run(s) |
| 7 | day 13, 00:15 — both still in the trash | 2026-11-29T00:15:11Z | ✅ passed | = trash,trash |
| 8 | day 13, 23:59 | 2026-11-29T23:59:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 9 | day 13, 23:59 — both still in the trash | 2026-11-29T23:59:05Z | ✅ passed | = trash,trash |
| 10 | day 14, 00:15 (after the 00:10 cleanup) | 2026-11-30T00:15:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 11 | day 14, 00:15 — A (14 d 0 h 10 min in the trash) and B (13 d 0 h 25 min) are both permanently deleted | 2026-11-30T00:15:06Z | ✅ passed | = gone,gone |
| 12 | set the workspace's retention to 1 day | 2026-11-30T00:15:06Z | ✅ passed | 1 UPDATE 1 |
| 13 | day 14, 23:59 | 2026-11-30T23:59:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 14 | move Lee (C) to the trash at 23:59 | 2026-11-30T23:59:06Z | ✅ passed | 2026-11-30 23:59 UPDATE 1 (saved as ${c_deleted_at}) |
| 15 | day 15, 00:15 (after the 00:10 cleanup) | 2026-12-01T00:15:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 16 | day 15, 00:15 — C is permanently deleted 16 minutes after going to the trash, with retention set to 1 day | 2026-12-01T00:15:06Z | ✅ passed | = gone |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| db | ok |  |
| redis | ok |  |
| server | ok |  |
| worker | ok |  |

## Reproduce

```sh
chronostack run scenarios/trash-retention-current-behaviour.yml
```

The JSON version of this report has every clock stop, trigger output and event.
