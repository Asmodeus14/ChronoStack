<!-- Draft GitHub issue for https://github.com/chrisvel/tududi/issues (bug_report.yml). Not posted yet. Plain report: no tool description, one link to the reproduction. -->

**Title:** Monthly recurring tasks move to the wrong day: "on the 1st" becomes the last day of the previous month west of UTC, and "last Friday" skips months

### Description

Two problems in `backend/modules/tasks/recurringTaskService.js`:

1. **Monthly on day N is a day early for users west of UTC** (all of the Americas). Due dates are stored as the end of the day in the user's timezone (`processDueDateForStorage`), which for UTC−X is already the next day in UTC. `calculateMonthlyRecurrence` reads and builds the date with UTC getters, so "monthly on the 1st" lands on the last day of the previous month, every month, and "monthly on the 31st" skips February. `calculateNextDueDate` passes `timezone` only to the weekly calculation; the weekly code already reads the weekday in the user's timezone for this reason.

2. **"Last <weekday> of the month" (and any 5th-week rule) skips a month** whenever the previous occurrence is on the 29th–31st and the next month is shorter. `calculateMonthlyWeekdayRecurrence` moves forward with `setUTCMonth(month + interval)`, so 30 January becomes "30 February", which rolls over to 2 March and the occurrence is computed for March. `calculateMonthlyLastDayRecurrence` already avoids exactly this overflow (see its comment).

Next due dates as the user sees them (stored with `processDueDateForStorage`, read back with `processDueDateForResponse`):

| Recurrence | UTC, Asia/Kolkata | America/New_York, America/Los_Angeles |
|---|---|---|
| Last Friday, from 2026-01-30 | **2026-03-27** (Feb skipped) | **2026-03-27** (Feb skipped) |
| Monthly on the 1st, from 2026-04-01 | 05-01 → 06-01 → 07-01 ✓ | **04-30 → 05-31 → 06-30** |
| Monthly on the 31st, from 2026-01-31 | 02-28 → 03-31 ✓ | **03-31** (Feb skipped) |

### Steps to Reproduce

1. Set the profile timezone to America/New_York.
2. Create a task due 2026-12-01, repeating monthly on day 1 (`recurrence_type: monthly`, `recurrence_month_day: 1`).
3. Mark it done. It moves to **2026-12-31**; done again, **2027-01-31**.
4. Create a task due 2027-01-29, repeating on the last Friday (`monthly_weekday`, weekday 5, week 5). Mark it done: it moves to **2027-03-26**.

### Expected Behavior

Completing moves the task to the next month's occurrence: 2027-01-01 for the rent task, 2027-02-26 for the last-Friday task, in any timezone.

### Actual Behavior

As above: 2026-12-31, then 2027-01-31; and 2027-03-26 (February skipped).

### Operating System

Docker (Linux), `chrisvel/tududi:2.1.0` with Postgres 16. Reproduced through the API, so independent of OS and browser.

### Browser

n/a (reproduced through the REST API the web app uses)

### Tududi Version

2.1.0 (`main` at cda56e7 has the same code)

### Additional Context

Reproduced end to end on the published image with the clock moved forward to each due date and the task completed there. A fix that reads and builds both monthly dates in the user's timezone with moment-timezone, and moves months from the 1st (no overflow), makes all the cases above correct in UTC, Kolkata, New York and Los Angeles, and passes the expected-behaviour run. Happy to open a PR with it and tests.

Setup, scenarios and run reports: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/tududi
