# Scenario report: Monthly recurring tasks for a New York user, as tududi 2.1.0 behaves today

A user in America/New_York has two recurring tasks, created through the API as the web app creates them: "Pay rent", monthly on the 1st (from 1 December 2026), and "Team retro", the last Friday of every month (from 29 January 2027). Time moves to each due date (noon in New York) and the user completes the task that day; tududi then moves it to its next due date. This pins what happens today.

**Result: PASSED ✅**  
Virtual time: 2026-11-15T19:05:17Z → 2027-01-29T17:00:02Z  
Real time: 22.5 s (started 2026-11-15T19:05:17Z)  
Scenario file: `scenarios/monthly-recurrence-current-behaviour.yml` · ChronoStack project `tududi-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | create the user's tasks (time zone America/New_York) | 2026-11-15T19:05:31Z | ✅ passed | ynhsqtyi80t9vnv yefiqptiqkeijvp |
| 2 | 1 December 2026, noon in New York | 2026-12-01T17:00:00Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 3 | complete "Pay rent" on its due date; it moves to 31 December, not 1 January | 2026-12-01T17:00:01Z | ✅ passed | 2026-12-31 |
| 4 | rent is next due on 31 December | 2026-12-01T17:00:02Z | ✅ passed |  |
| 5 | 31 December 2026, noon in New York | 2026-12-31T17:00:00Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 6 | complete "Pay rent" again | 2026-12-31T17:00:01Z | ✅ passed | 2027-01-31 |
| 7 | rent is next due on 31 January, the last day of the month instead of the 1st | 2026-12-31T17:00:02Z | ✅ passed |  |
| 8 | 29 January 2027, noon in New York (last Friday of January) | 2027-01-29T17:00:00Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 9 | complete "Team retro" | 2027-01-29T17:00:01Z | ✅ passed | 2027-03-26 |
| 10 | the retro is next due on 26 March; February's last Friday (26 Feb) is skipped | 2027-01-29T17:00:02Z | ✅ passed |  |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| db | ok |  |
| tududi | ok |  |

## Reproduce

```sh
chronostack run scenarios/monthly-recurrence-current-behaviour.yml
```

The JSON version of this report has every clock stop, trigger output and event.
