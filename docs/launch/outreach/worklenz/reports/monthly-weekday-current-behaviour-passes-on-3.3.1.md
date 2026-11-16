# Scenario report: Monthly "Nth weekday" recurring tasks, as Worklenz 3.3.1 behaves today

Two recurring tasks created with Worklenz's own database functions: "Team sync", monthly on the 1st Monday (first due Monday 7 December 2026), and "Release review", monthly on the 2nd Tuesday (first due Tuesday 8 December 2026). The recurring-task job (ENABLE_RECURRING_JOBS, every minute here) creates the next task once the last one is past due. Each stop lands at noon UTC and waits 75 s so the job runs. January 2027 starts on a Friday, the case where moment's .day() lands in the previous week.

**Result: PASSED ✅**  
Virtual time: 2026-11-16T02:45:32Z → 2027-02-02T12:01:16Z  
Real time: 305.9 s (started 2026-11-16T02:45:32Z)  
Scenario file: `scenarios/monthly-weekday-current-behaviour.yml` · ChronoStack project `worklenz-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | 8 December 2026, noon UTC (Team sync's 7 December task is past) | 2026-12-08T12:01:15Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 2 | the next "1st Monday" task is due Monday 28 December (still December), not 4 January | 2026-12-08T12:01:15Z | ✅ passed | = 2026-12-07,2026-12-28 |
| 3 | 9 December 2026, noon UTC (Release review's 8 December task is past) | 2026-12-09T12:01:15Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 4 | the next "2nd Tuesday" task is due 5 January (the 1st Tuesday), not 12 January | 2026-12-09T12:01:16Z | ✅ passed | = 2026-12-08,2027-01-05 |
| 5 | 29 December 2026, noon UTC (the 28 December task is past) | 2026-12-29T12:01:15Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 6 | 2 February 2027, noon UTC (after the 1st Mondays of January and February) | 2027-02-02T12:01:15Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 7 | the schedule is stuck - no task for January (4 Jan) or February (1 Feb) was ever created | 2027-02-02T12:01:16Z | ✅ passed | = 2026-12-07,2026-12-28 |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| backend | ok |  |
| db | ok |  |

## Reproduce

```sh
chronostack run scenarios/monthly-weekday-current-behaviour.yml
```

The JSON version of this report has every clock stop, trigger output and event.
