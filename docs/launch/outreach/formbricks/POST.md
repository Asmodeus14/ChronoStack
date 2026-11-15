<!-- Draft GitHub issue for https://github.com/formbricks/formbricks/issues (bug_report.yml). Not posted yet. Plain report: no tool description, one link to the reproduction. -->

**Title:** Scheduled surveys open and close a day late when SURVEY_SCHEDULING_TIME_ZONE is UTC+12 or later (e.g. Pacific/Auckland)

### Issue Summary

With `SURVEY_SCHEDULING_TIME_ZONE` set to a zone at UTC+12 or later (Pacific/Auckland, Pacific/Fiji, Pacific/Tongatapu, Pacific/Kiritimati…), every publish or close date picked in the survey editor is scheduled for the **following** day.

The editor stores the picked day as noon UTC (`toDateOnlySelection`, `DATE_ONLY_SELECTION_UTC_HOUR = 12`). On save, `normalizeDateOnlySelectionToSurveySchedulingDateTime` reads the calendar day of that instant **in the scheduling time zone**. At UTC+12 and later, noon UTC is already the next day there, so the survey is scheduled for that next day:

| `SURVEY_SCHEDULING_TIME_ZONE` | Picked 2026-12-05 → scheduled for |
|---|---|
| Europe/Berlin (default), America/New_York, Asia/Kolkata, Asia/Tokyo | 2026-12-05 00:00 ✓ |
| Pacific/Fiji, Pacific/Auckland, Pacific/Tongatapu, Pacific/Kiritimati | **2026-12-06 00:00** |

(From `createSurveySchedulingDateUtils({ timeZone, localHour: 0, localMinute: 0 }).normalizeDateOnlySelectionToSurveySchedulingDateTime(toDateOnlySelection(new Date(2026, 11, 5)))`. The tests in `date-utils.test.ts` don't cover zones east of UTC+12.)

End to end on the published `ghcr.io/formbricks/formbricks:6.2.0` image (bundled SpiceDB, Postgres, Valkey; `SURVEY_SCHEDULING_TIME_ZONE=Pacific/Auckland`), with the stack's clock moved forward:

1. Created a live link survey through `POST /api/v1/management/surveys` with `closeOn` set the way the editor sends it for a date picked 3 days ahead (`2026-11-19T12:00:00.000Z` for 19 November).
2. It's stored as `2026-11-19 11:00 UTC`, which is **20 November 00:00** in Auckland.
3. At 00:05 Auckland time on 19 November, after the daily `survey-scheduling.reconcile` job had run (confirmed in the queue's event stream), the survey was still `inProgress`.
4. It was closed by the next day's run, 24 hours later than picked.

### Expected Behavior

A survey closes (or opens) at the scheduling time on the day picked in the editor, whatever `SURVEY_SCHEDULING_TIME_ZONE` is, as it does with the default Europe/Berlin.

### Other information

A single UTC hour can't represent a calendar day for every zone: zones span UTC−12 to UTC+14, and the server deliberately re-reads stored values in the scheduling zone so that re-saving a survey doesn't shift it. Sending the picked day as a plain date (`YYYY-MM-DD`) from the editor and converting it on the server with `createSurveySchedulingDateTime` would avoid the ambiguity. Happy to help with a PR if you agree on the approach.

Formbricks 6.2.0, self-hosted with Docker. Compose file, seed scripts, scenarios and run reports: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/formbricks
