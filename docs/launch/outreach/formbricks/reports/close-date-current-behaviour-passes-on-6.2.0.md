# Scenario report: Survey close date in Pacific/Auckland, as Formbricks 6.2.0 behaves today

SURVEY_SCHEDULING_TIME_ZONE is Pacific/Auckland, so the daily scheduling job runs at 00:00 New Zealand time (11:00 UTC in November). A live survey is created through the management API with a close date picked 3 days ahead, sent as the editor sends it (noon UTC of the picked day). This pins what happens: the close date is stored as 00:00 on the day AFTER the picked one, so the survey is still open on the picked day and closes a day later.

**Result: PASSED ✅**  
Virtual time: 2026-11-15T16:38:42Z → 2026-11-19T11:05:01Z  
Real time: 19.3 s (started 2026-11-15T16:38:42Z)  
Scenario file: `scenarios/close-date-current-behaviour.yml` · ChronoStack project `formbricks-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | seed an organization, workspace and API key | 2026-11-15T16:38:50Z | ✅ passed | cstestworkspace00000000001 (saved as ${workspace}) |
| 2 | create a live survey, picking a close date 3 days ahead | 2026-11-15T16:38:55Z | ✅ passed | 2026-11-19 (saved as ${picked}) |
| 3 | 00:05 New Zealand time on the picked day, in UTC | 2026-11-15T16:38:55Z | ✅ passed | 2026-11-18T11:05:00Z (saved as ${picked_day}) |
| 4 | 00:05 New Zealand time on the day after, in UTC | 2026-11-15T16:38:56Z | ✅ passed | 2026-11-19T11:05:00Z (saved as ${day_after}) |
| 5 | the stored close date is 00:00 New Zealand time, one day after the picked date | 2026-11-15T16:38:56Z | ✅ passed | = 1 day(s) after, at 00:00 |
| 6 | to the picked day, 00:05 New Zealand time (the job ran at 00:00) | 2026-11-18T11:05:01Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 7 | on the picked day the survey is still open | 2026-11-18T11:05:01Z | ✅ passed | = inProgress |
| 8 | to the day after, 00:05 New Zealand time | 2026-11-19T11:05:01Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 9 | a day late, the survey is closed | 2026-11-19T11:05:01Z | ✅ passed | = completed |

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
chronostack run scenarios/close-date-current-behaviour.yml
```

The JSON version of this report has every clock stop, trigger output and event.
