# Scenario report: Trash retention, as documented

Twenty's legal FAQ says soft-deleted records "are kept for 14 days, after which they are permanently removed", and the retention setting is a number of days. The cleanup runs once a day (00:10 UTC), so a record may stay up to a day longer than the setting, but it must never be removed before it. This checks both sides: still in the trash just before the retention period ends, gone within a day after it. Default retention (14 days), then 1 day.

**Result: FAILED ❌**  
Virtual time: 2026-11-15T04:34:24Z → 2026-11-29T00:15:12Z  
Real time: 109.4 s (started 2026-11-15T04:34:24Z)  
Scenario file: `scenarios/trash-retention-as-documented.yml` · ChronoStack project `twenty-chronostack`

## What failed

Step 5 of 14, **day 14, 00:15 — B has been in the trash 13 d 0 h 25 min, so it must still be there**, at virtual time 2026-11-29T00:15:12Z:

```
expected `trash`, got gone
```

The steps after it were not run. The proxy traffic and outage windows below show what the services did up to that point.

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | day 0, 23:50 UTC | 2026-11-15T23:50:25Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 2 | move Terry (B) to the trash at 23:50 | 2026-11-15T23:50:25Z | ✅ passed | 2026-11-15 23:50 UPDATE 1 (saved as ${b_deleted_at}) |
| 3 | to day 1, 00:15 (after the 00:10 cleanup) | 2026-11-16T00:15:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 4 | 13 more days, one day at a time | 2026-11-29T00:15:12Z | ✅ passed | 13 stop(s), 0 trigger run(s) |
| 5 | day 14, 00:15 — B has been in the trash 13 d 0 h 25 min, so it must still be there | 2026-11-29T00:15:12Z | ❌ failed | expected `trash`, got gone |
| 6 | one more day |  | skipped |  |
| 7 | day 15, 00:15 — B (14 d 0 h 25 min) is gone |  | skipped |  |
| 8 | set the workspace's retention to 1 day |  | skipped |  |
| 9 | day 15, 23:59 |  | skipped |  |
| 10 | move Lee (C) to the trash at 23:59 |  | skipped |  |
| 11 | day 16, 00:15 (after the 00:10 cleanup) |  | skipped |  |
| 12 | day 16, 00:15 — C has been in the trash 16 min with retention 1 day, so it must still be there |  | skipped |  |
| 13 | one more day |  | skipped |  |
| 14 | day 17, 00:15 — C (1 d 0 h 16 min) is gone |  | skipped |  |

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
chronostack run scenarios/trash-retention-as-documented.yml
```

The JSON version of this report has every clock stop, trigger output and event.
