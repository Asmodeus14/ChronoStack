# Scenario report: A stale issue is auto-closed after 1 month, then auto-archived

A project set to auto-close after 1 month and auto-archive after 1 month. One issue in "In Progress", created at 12:00, is never touched again. Celery beat runs archive_and_close_old_issues every day at 01:00 UTC, on its own (no triggers). Each day's step lands at 01:01, so beat fires the job then.

**Result: PASSED ✅**  
Virtual time: 2026-12-17T01:30:38Z → 2027-02-19T01:01:21Z  
Real time: 206.8 s (started 2026-11-14T16:40:04Z)  
Scenario file: `scenarios/auto-close-archive.yml` · ChronoStack project `plane-chronostack-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | start at 12:00 UTC (day 0) | 2026-12-17T12:00:00Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 2 | create the project and the issue | 2026-12-17T12:00:04Z | ✅ passed | 9008f99d-1ed9-4de9-8424-b9d00c3edfbe (saved as ${issue_id}) |
| 3 | the issue is in progress | 2026-12-17T12:00:04Z | ✅ passed | = started |
| 4 | to day 1, 01:01 | 2026-12-18T01:01:03Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 5 | 29 more days, one day at a time | 2027-01-16T01:01:07Z | ✅ passed | 29 stop(s), 0 trigger run(s) |
| 6 | day 30, 01:01 — not yet (29 days 13 h since the last update) | 2027-01-16T01:01:07Z | ✅ passed | = started |
| 7 | one more day | 2027-01-17T01:01:13Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 8 | day 31, 01:01 — auto-closed to Cancelled | 2027-01-17T01:01:13Z | ✅ passed | = cancelled |
| 9 | record the issue's state for the next 3 days | 2027-01-20T01:01:17Z | ✅ passed | 3 stop(s), 0 trigger run(s) |
| 10 | day 34 — state | 2027-01-20T01:01:17Z | ✅ passed | cancelled (saved as ${day34}) |
| 11 | 30 more days | 2027-02-19T01:01:21Z | ✅ passed | 30 stop(s), 0 trigger run(s) |
| 12 | day 64 — auto-archived | 2027-02-19T01:01:21Z | ✅ passed | = cancelled archived |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| beat-worker | ok |  |
| migrator | completed | one-shot container exited with code 0; nothing to probe |
| plane-db | ok |  |
| plane-mq | ok |  |
| plane-redis | ok |  |
| worker | ok |  |

## Reproduce

```sh
chronostack run scenarios/auto-close-archive.yml
```

The JSON version of this report has every clock stop, trigger output and event.
