# Scenario report: Monthly recurring tasks for a New York user, as expected

The same two tasks for a user in America/New_York: "Pay rent", monthly on the 1st (from 1 December 2026), and "Team retro", the last Friday of every month (from 29 January 2027). Expected: completing each on its due date moves it to the next month's occurrence, 1 January and 26 February.

**Result: FAILED ❌**  
Virtual time: 2026-11-15T19:06:25Z → 2026-12-01T17:00:02Z  
Real time: 18.4 s (started 2026-11-15T19:06:25Z)  
Scenario file: `scenarios/monthly-recurrence-as-expected.yml` · ChronoStack project `tududi-chronostack`

## What failed

Step 4 of 7, **rent is next due on 1 January**, at virtual time 2026-12-01T17:00:02Z:

```
exit Some(1): 
```

The steps after it were not run. The proxy traffic and outage windows below show what the services did up to that point.

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | create the user's tasks (time zone America/New_York) | 2026-11-15T19:06:40Z | ✅ passed | 2wsa9f1g1h6mm4x 38we526nfuy1469 |
| 2 | 1 December 2026, noon in New York | 2026-12-01T17:00:00Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 3 | complete "Pay rent" on its due date | 2026-12-01T17:00:01Z | ✅ passed | 2026-12-31 |
| 4 | rent is next due on 1 January | 2026-12-01T17:00:02Z | ❌ failed | exit Some(1):  |
| 5 | 29 January 2027, noon in New York (last Friday of January) |  | skipped |  |
| 6 | complete "Team retro" |  | skipped |  |
| 7 | the retro is next due on 26 February, the last Friday of February |  | skipped |  |

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
chronostack run scenarios/monthly-recurrence-as-expected.yml
```

The JSON version of this report has every clock stop, trigger output and event.
