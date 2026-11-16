<!-- Posted as https://github.com/Worklenz/worklenz/issues/959 on 2026-11-16. -->

**Title:** [BUG] Monthly "Nth weekday" recurring tasks land on the wrong date, and "1st <weekday>" schedules can stop recurring

### Bug Summary

`calculateNextEndDate` (`worklenz-backend/src/shared/utils.ts`) handles "monthly on the Nth weekday" like this:

```ts
} else if (schedule.day_of_month && schedule.week_of_month) {
  nextDate.add(1, "month").startOf("month").day(schedule.day_of_month);
  nextDate.add(schedule.week_of_month - 1, "weeks");
}
```

moment's `.day(n)` moves to weekday `n` **within the current Sunday-to-Saturday week**, so when the month starts after that weekday the result falls in the previous week, often in the previous month. That gives four different wrong results:

| Schedule | Next dates from December 2026 (current code) | Expected |
|---|---|---|
| 1st Monday, from 2026-12-07 | 2026-12-28, then 2026-12-28 forever | 2027-01-04, 02-01, 03-01, 04-05 |
| 2nd Tuesday, from 2026-12-08 | 2027-01-05, 02-09, 03-09, **04-06**, **05-04** | 2027-01-12, 02-09, 03-09, 04-13, 05-11 |
| 1st Sunday, from 2026-12-06 | 2027-01-06 (Wed), 02-06 (Sat), 03-06 (Sat) | 2027-01-03, 02-07, 03-07 |
| Last Friday (week 5), from 2026-12-25 | 2027-01-29, **03-05**, 04-30, 05-28, **07-02** | 2027-01-29, 02-26, 03-26, 04-30, 05-28, 06-25 |

- **Stuck schedule (1st Monday):** the next date is computed as 28 December, still in December. After that, every run computes 28 December again, the job logs `Skipped duplicate: "Team sync (1st Monday)" for 2026-12-28`, and no further task is ever created.
- **A week early (2nd Tuesday):** in months that start on a Wednesday through Saturday.
- **Sunday ignored:** Sunday is `day_of_month = 0`, which is falsy, so the condition fails and the schedule falls through to "same date next month", on any weekday.
- **Last <weekday>:** week 5 adds four weeks from a week that may have started in the previous month, so it skips months or lands in the following one.

### Steps to Reproduce

1. Create a task due Monday 2026-12-07 and make it recurring: monthly, first Monday (`day_of_month = 1`, `week_of_month = 1`).
2. With `ENABLE_RECURRING_JOBS=true`, let the recurring-task job run after 7 December.
3. The next task is created due **2026-12-28**. After 28 December, the job keeps skipping a duplicate for 2026-12-28 and creates nothing for January or February.

Reproduced end to end on the published `chamikajaycey/worklenz-backend:3.3.1` image with Postgres 15, moving the stack's clock to each date so the recurring job ran on its own (`RECURRING_JOBS_INTERVAL` set to every minute). The code is unchanged on `main` and in v3.4.0.

### Expected Behavior

The next task is due on the chosen weekday of the following month: 2027-01-04 and 2027-02-01 for the first Monday, 2027-01-12 for the second Tuesday, and Sundays work like the other days.

### Actual Behavior

As in the table: 2026-12-28 and then no more tasks for the first Monday; 2027-01-05 for the second Tuesday.

### Severity

High: a recurring task can silently stop recurring, and others land a week early or in the wrong month.

### Environment

Self-hosted (Docker), Worklenz 3.3.1.

### Evidence

A fix that steps to the first day of the next month, adds the days to the chosen weekday, adds the weeks, and steps back a week if a 5th week doesn't exist:

```ts
} else if (schedule.day_of_month != null && schedule.week_of_month) {
  nextDate.add(1, "month").startOf("month");
  const targetMonth = nextDate.month();
  nextDate.add((schedule.day_of_month - nextDate.day() + 7) % 7, "days");
  nextDate.add(schedule.week_of_month - 1, "weeks");
  if (nextDate.month() !== targetMonth) nextDate.subtract(1, "week");
}
```

gives the "Expected" column above for all four rows. With it applied in the backend container, the same run creates 2027-01-04, 2027-02-01 and 2027-01-12.

Setup, scenarios and run reports: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/worklenz
