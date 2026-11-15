# Scenario report: Survey close date in Pacific/Auckland, as expected

SURVEY_SCHEDULING_TIME_ZONE is Pacific/Auckland, so the daily scheduling job runs at 00:00 New Zealand time (11:00 UTC in November). A live survey is created through the management API with a close date picked 3 days ahead, sent as the editor sends it (noon UTC of the picked day). Expected: the survey closes at the scheduling time (00:00) on the picked day, as it does with the default time zone (Europe/Berlin).

**Result: FAILED ❌**  
Virtual time: 2026-11-15T16:39:57Z → 2026-11-18T11:05:01Z  
Real time: 16.5 s (started 2026-11-15T16:39:57Z)  
Scenario file: `scenarios/close-date-as-expected.yml` · ChronoStack project `formbricks-chronostack`

## What failed

Step 6 of 7, **on the picked day the survey is closed**, at virtual time 2026-11-18T11:05:01Z:

```
expected `completed`, got inProgress
```

The steps after it were not run. The proxy traffic and outage windows below show what the services did up to that point.

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | seed an organization, workspace and API key | 2026-11-15T16:40:05Z | ✅ passed | cstestworkspace00000000001 (saved as ${workspace}) |
| 2 | create a live survey, picking a close date 3 days ahead | 2026-11-15T16:40:09Z | ✅ passed | 2026-11-19 (saved as ${picked}) |
| 3 | 00:05 New Zealand time on the picked day, in UTC | 2026-11-15T16:40:10Z | ✅ passed | 2026-11-18T11:05:00Z (saved as ${picked_day}) |
| 4 | 00:05 New Zealand time on the day after, in UTC | 2026-11-15T16:40:10Z | ✅ passed | 2026-11-19T11:05:00Z (saved as ${day_after}) |
| 5 | to the picked day, 00:05 New Zealand time (the job ran at 00:00) | 2026-11-18T11:05:01Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 6 | on the picked day the survey is closed | 2026-11-18T11:05:01Z | ❌ failed | expected `completed`, got inProgress |
| 7 | the stored close date is 00:00 New Zealand time on the picked date |  | skipped |  |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| authzed-db-bootstrap | completed | one-shot container exited with code 0; nothing to probe |
| authzed-initialize | completed | one-shot container exited with code 0; nothing to probe |
| formbricks | ok |  |
| formbricks-migrate | completed | one-shot container exited with code 0; nothing to probe |
| postgres | ok |  |
| redis | ok |  |

## Reproduce

```sh
chronostack run scenarios/close-date-as-expected.yml
```

The JSON version of this report has every clock stop, trigger output and event.
