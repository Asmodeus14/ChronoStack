# Scenario report: A poll closes after its last date, then is cleaned up

A poll whose only option is in 2 days. The hourly auto-close job must close it once that option has ended (Postgres now()). Thirty days of inactivity later, the daily job must mark it deleted (Node Date.now()), and 7 days after that the purge job must remove it.

**Result: PASSED ✅**  
Virtual time: 2026-11-20T12:00:17Z → 2027-01-01T12:00:04Z  
Real time: 406.3 s (started 2026-11-14T16:03:34Z)  
Scenario file: `scenarios/poll-lifecycle.yml` · ChronoStack project `rallly-chronostack-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | start at a UTC midnight | 2026-11-21T00:00:00Z | ✅ passed | 13 stop(s), 12 trigger run(s) |
| 2 | create a poll whose only option is in 2 days, 10:00-11:00 | 2026-11-21T00:00:00Z | ✅ passed | INSERT 0 1 INSERT 0 1 |
| 3 | the poll is open | 2026-11-21T00:00:01Z | ✅ passed | = open |
| 4 | two days pass, to day 2 at 11:00 (auto-close runs every hour at :15) | 2026-11-23T11:00:01Z | ✅ passed | 66 stop(s), 65 trigger run(s) |
| 5 | the poll is still open (the option ends at 11:00, next run is 11:15) | 2026-11-23T11:00:01Z | ✅ passed | = open |
| 6 | one more hour | 2026-11-23T12:00:02Z | ✅ passed | 2 stop(s), 1 trigger run(s) |
| 7 | the poll was auto-closed | 2026-11-23T12:00:02Z | ✅ passed | = closed |
| 8 | by the 11:15 run, the first one after the option ended | 2026-11-23T12:00:02Z | ✅ passed | = 11:15 |
| 9 | 30 days of inactivity pass | 2026-12-24T12:00:03Z | ✅ passed | 807 stop(s), 806 trigger run(s) |
| 10 | the poll was marked deleted | 2026-12-24T12:00:03Z | ✅ passed | = t |
| 11 | 8 more days pass | 2027-01-01T12:00:03Z | ✅ passed | 209 stop(s), 208 trigger run(s) |
| 12 | the poll was removed for good | 2027-01-01T12:00:04Z | ✅ passed | = 0 |

## Requests through the fault proxy

None.

*Older events were dropped from the controller's buffer (it keeps the last 1000).*

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| rallly | ok |  |
| rallly_db | ok |  |

## Reproduce

```sh
chronostack run scenarios/poll-lifecycle.yml
```

The JSON version of this report has every clock stop, trigger output and event.
