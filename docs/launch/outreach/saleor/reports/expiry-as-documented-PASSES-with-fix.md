# Scenario report: Unpaid orders expire when the channel setting says

Channels set expireOrdersAfter to 60, 1440 (24 h) and 2880 (48 h) minutes. Each unconfirmed order should expire 60 min, 24 h and 48 h after it was created. Saleor's Celery beat runs expire_orders_task every 5 minutes on its own (no triggers).

**Result: PASSED ✅**  
Virtual time: 2027-03-23T17:31:14Z → 2027-03-25T17:36:49Z  
Real time: 36.7 s (started 2026-11-14T18:11:21Z)  
Scenario file: `scenarios/expiry-as-documented.yml` · ChronoStack project `saleor-chronostack-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | create three channels (orders expire after 60 min, 24 h, 48 h) with one unpaid order each | 2027-03-23T17:31:22Z | ✅ passed |  |
| 2 | all three orders are unconfirmed | 2027-03-23T17:31:22Z | ✅ passed | = unconfirmed |
| 3 | 1 h 5 min later | 2027-03-23T18:36:31Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 4 | the 60-minute order has expired | 2027-03-23T18:36:31Z | ✅ passed | after 0.4s |
| 5 | 24 h 5 min after creation | 2027-03-24T17:36:40Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 6 | the 24-hour order has expired | 2027-03-24T17:36:40Z | ✅ passed | after 0.4s |
| 7 | 48 h 5 min after creation | 2027-03-25T17:36:49Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 8 | the 48-hour order has expired | 2027-03-25T17:36:49Z | ✅ passed | after 0.4s |

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
