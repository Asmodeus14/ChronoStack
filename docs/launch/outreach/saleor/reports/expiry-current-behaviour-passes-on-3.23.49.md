# Scenario report: When unpaid orders actually expire

Pins the current behaviour. The order age is computed as DATE_PART('day')*24 + DATE_PART('hour')*60 + DATE_PART('minute') (saleor/order/tasks.py, _expire_orders), which equals the age in minutes only for ages under one day. Predicted: 60 min -> 1 h; 1440 min -> 47 h 36 min; 2880 min -> 61 d 23 h 36 min.

**Result: PASSED ✅**  
Virtual time: 2027-01-19T17:42:40Z → 2027-03-22T17:23:50Z  
Real time: 73.5 s (started 2026-11-14T18:07:44Z)  
Scenario file: `scenarios/expiry-current-behaviour.yml` · ChronoStack project `saleor-chronostack-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | create three channels (orders expire after 60 min, 24 h, 48 h) with one unpaid order each | 2027-01-19T17:42:47Z | ✅ passed |  |
| 2 | 1 h 5 min later | 2027-01-19T18:47:56Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 3 | the 60-minute order expired on time | 2027-01-19T18:47:56Z | ✅ passed | after 0.4s |
| 4 | 24 h 5 min after creation | 2027-01-20T17:48:05Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 5 | the 24-hour order has NOT expired | 2027-01-20T17:48:05Z | ✅ passed | = unconfirmed |
| 6 | 47 h 30 min after creation | 2027-01-21T17:13:14Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 7 | the 24-hour order still has not expired | 2027-01-21T17:13:14Z | ✅ passed | = unconfirmed |
| 8 | 47 h 40 min after creation | 2027-01-21T17:23:23Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 9 | the 24-hour order expires now (predicted 47 h 36 min) | 2027-01-21T17:23:23Z | ✅ passed | after 0.4s |
| 10 | 48 h 5 min after creation | 2027-01-21T17:48:32Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 11 | the 48-hour order has NOT expired | 2027-01-21T17:48:32Z | ✅ passed | = unconfirmed |
| 12 | 61 d 23 h 30 min after creation | 2027-03-22T17:13:41Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 13 | the 48-hour order still has not expired | 2027-03-22T17:13:41Z | ✅ passed | = unconfirmed |
| 14 | 61 d 23 h 40 min after creation | 2027-03-22T17:23:50Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 15 | the 48-hour order expires now (predicted 61 d 23 h 36 min) | 2027-03-22T17:23:50Z | ✅ passed | after 0.4s |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| beat | ok |  |
| db | ok |  |
| migrate | completed | one-shot container exited with code 0; nothing to probe |
| redis | ok |  |
| worker | ok |  |

## Reproduce

```sh
chronostack run scenarios/expiry-current-behaviour.yml
```

The JSON version of this report has every clock stop, trigger output and event.
