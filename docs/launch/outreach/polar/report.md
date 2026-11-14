# Scenario report: Subscriptions reach the end of their billing period

Two monthly subscriptions created today. A was canceled by the customer to end at the period end; B is free and renews. Polar's own APScheduler job store (polar.worker.scheduler) must pick both up when their period ends, with no triggers: it checks for due subscriptions at least every 30 s.

**Result: PASSED ✅**  
Virtual time: 2027-01-14T17:16:23Z → 2027-02-14T17:17:54Z  
Real time: 42.3 s (started 2026-11-14T17:15:24Z)  
Scenario file: `scenarios/subscription-period-end.yml` · ChronoStack project `polar-chronostack-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | seed two subscriptions with Polar's test factories | 2027-01-14T17:16:48Z | ✅ passed | 03684b36-7baf-464b-abab-5012263f8b97 (saved as ${a_id}) |
| 2 | look up the free subscription | 2027-01-14T17:16:49Z | ✅ passed | 249face2-0750-4f48-b3fd-753e31021671 (saved as ${b_id}) |
| 3 | A is active, set to end at the period end | 2027-01-14T17:16:49Z | ✅ passed | = active true |
| 4 | 29 days pass | 2027-02-12T17:16:50Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 5 | A is still active one day before its period ends | 2027-02-12T17:16:50Z | ✅ passed | = active |
| 6 | B is still in its first period | 2027-02-12T17:16:51Z | ✅ passed | = t |
| 7 | note A's period end, plus one minute | 2027-02-12T17:16:51Z | ✅ passed | 2027-02-14T17:17:42Z (saved as ${a_end}) |
| 8 | to one minute after A's period end | 2027-02-14T17:17:42Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 9 | the scheduler ends A | 2027-02-14T17:17:53Z | ✅ passed | after 10.4s |
| 10 | A ended within 5 virtual minutes of its period end | 2027-02-14T17:17:53Z | ✅ passed | = t |
| 11 | the scheduler renews B into its second period | 2027-02-14T17:17:54Z | ✅ passed | after 0.5s |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| db | ok |  |
| migrate | completed | one-shot container exited with code 0; nothing to probe |
| redis | ok |  |
| worker | ok |  |

## Reproduce

```sh
chronostack run scenarios/subscription-period-end.yml
```

The JSON version of this report has every clock stop, trigger output and event.
