# Scenario report: Monthly "Nth weekday" recurring tasks, as expected

The same two recurring tasks: "Team sync" on the 1st Monday (first due 7 December 2026) and "Release review" on the 2nd Tuesday (first due 8 December 2026). Expected: each next task is due on that weekday of the next month - 4 January and 1 February 2027 for Team sync, 12 January for Release review.

**Result: PASSED ✅**  
Virtual time: 2026-11-16T02:55:52Z → 2027-01-05T12:01:16Z  
Real time: 229.6 s (started 2026-11-16T02:55:52Z)  
Scenario file: `scenarios/monthly-weekday-as-expected.yml` · ChronoStack project `worklenz-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | 8 December 2026, noon UTC | 2026-12-08T12:01:15Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 2 | 9 December 2026, noon UTC | 2026-12-09T12:01:15Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 3 | the next "1st Monday" task is due 4 January and the next "2nd Tuesday" one 12 January | 2026-12-09T12:01:16Z | ✅ passed | = Release review (2nd Tuesday)=2027-01-12 Team sync (1st Monday)=2027-01-04 |
| 4 | 5 January 2027, noon UTC (the 4 January task is past) | 2027-01-05T12:01:15Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 5 | Team sync moves on to 1 February, the 1st Monday of February | 2027-01-05T12:01:16Z | ✅ passed | = 2026-12-07,2027-01-04,2027-02-01 |

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
chronostack run scenarios/monthly-weekday-as-expected.yml
```

The JSON version of this report has every clock stop, trigger output and event.
