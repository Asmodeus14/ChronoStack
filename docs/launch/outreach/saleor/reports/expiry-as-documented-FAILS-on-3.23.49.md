# Scenario report: Unpaid orders expire when the channel setting says

Channels set expireOrdersAfter to 60, 1440 (24 h) and 2880 (48 h) minutes. Each unconfirmed order should expire 60 min, 24 h and 48 h after it was created. Saleor's Celery beat runs expire_orders_task every 5 minutes on its own (no triggers).

**Result: FAILED ❌**  
Virtual time: 2026-11-14T17:56:03Z → 2026-11-15T18:01:58Z  
Real time: 57.1 s (started 2026-11-14T17:56:03Z)  
Scenario file: `scenarios/expiry-as-documented.yml` · ChronoStack project `saleor-chronostack-chronostack`

## What failed

Step 6 of 8, **the 24-hour order has expired**, at virtual time 2026-11-15T18:01:58Z:

```
not satisfied after 30s (last: = f)
```

The steps after it were not run. The proxy traffic and outage windows below show what the services did up to that point.

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | create three channels (orders expire after 60 min, 24 h, 48 h) with one unpaid order each | 2026-11-14T17:56:10Z | ✅ passed |  |
| 2 | all three orders are unconfirmed | 2026-11-14T17:56:10Z | ✅ passed | = unconfirmed |
| 3 | 1 h 5 min later | 2026-11-14T19:01:19Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 4 | the 60-minute order has expired | 2026-11-14T19:01:20Z | ✅ passed | after 0.3s |
| 5 | 24 h 5 min after creation | 2026-11-15T18:01:28Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 6 | the 24-hour order has expired | 2026-11-15T18:01:58Z | ❌ failed | not satisfied after 30s (last: = f) |
| 7 | 48 h 5 min after creation |  | skipped |  |
| 8 | the 48-hour order has expired |  | skipped |  |

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
chronostack run scenarios/expiry-as-documented.yml
```

The JSON version of this report has every clock stop, trigger output and event.
