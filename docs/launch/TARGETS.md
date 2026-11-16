# Outreach targets: open-source SaaS projects

Open-source projects with real time-dependent features, on stacks ChronoStack covers (Node, Python, Ruby with Postgres and Redis; no Go or PHP backends). Checked on GitHub on 2026-11-14: all active that week.

**Approach for each project:** run ChronoStack against it first, test one real time-based flow, then post the result (scenario file plus what happened). Never a cold "try my tool" post. Ask Apple-silicon Mac users for their `chronostack doctor` output ([#5](https://github.com/Asmodeus14/ChronoStack/issues/5)).

## Billing and subscriptions

| Project | Stars | Stack | Time-dependent features | Reach via |
|---|---|---|---|---|
| [polarsource/polar](https://github.com/polarsource/polar) | 10.5k | Python, Postgres, Redis | Subscriptions, renewals, trials, failed payments | Issues or Discord (no Discussions) |
| [saleor/saleor](https://github.com/saleor/saleor) | 23.6k | Python, Celery, Postgres | Unpaid order expiry, checkout expiry, gift card expiry | Discussions |
| [medusajs/medusa](https://github.com/medusajs/medusa) | 37k | Node, Postgres, Redis | Scheduled jobs, cart and reservation expiry | Discussions |

## Scheduling and reminders

| Project | Stars | Stack | Time-dependent features | Reach via |
|---|---|---|---|---|
| [lukevella/rallly](https://github.com/lukevella/rallly) | 5.4k | Node, Postgres | Poll deadlines, reminders | Discussions |
| [calcom/cal.diy](https://github.com/calcom/cal.diy) (was cal.com) | 50k | Node, Postgres | Booking reminders, workflows | Issues only (hold) |
| [documenso/documenso](https://github.com/documenso/documenso) | 15.8k | Node, Postgres | Signing reminders, document expiry | Issues only (hold) |
| [formbricks/formbricks](https://github.com/formbricks/formbricks) | 13k | Node, Postgres | Survey close dates, follow-ups | Discussions |

## Workflows and background jobs

| Project | Stars | Stack | Time-dependent features | Reach via |
|---|---|---|---|---|
| [makeplane/plane](https://github.com/makeplane/plane) | 62k | Django, Celery beat, Postgres, Redis | Auto-archive and auto-close, cycles | Discussions |
| [twentyhq/twenty](https://github.com/twentyhq/twenty) | 61k | Node, BullMQ, Postgres, Redis | Cron workflows, trash purge | Discussions |
| [Infisical/infisical](https://github.com/Infisical/infisical) | 30k | Node, BullMQ, Postgres, Redis | Secret rotation, expiring access | Discussions |
| [triggerdotdev/trigger.dev](https://github.com/triggerdotdev/trigger.dev) | 16.8k | Node, Postgres, Redis | Scheduled and delayed tasks | Discussions |
| [chatwoot/chatwoot](https://github.com/chatwoot/chatwoot) | 40k | Ruby, Sidekiq, Postgres, Redis | SLA timers, snoozed conversations (Ruby untested) | Discussions |

## Order

1. Rallly (small, one main maintainer)
2. Plane, Polar (best fit: Celery beat, billing)
3. Saleor, Twenty, Infisical
4. Cal.com and Documenso only if a non-issue channel turns up

## Tracker

| Project | Tested flow | Result | Posted | Reply | Mac report |
|---|---|---|---|---|---|
| Rallly | Poll auto-close, inactive delete, purge ([files](outreach/rallly/)) | 12/12 passed on `lukevella/rallly:latest`, 2026-11-14 | [#3721](https://github.com/lukevella/rallly/discussions/3721), 2026-11-14 | | |
| Plane | Auto-close and auto-archive via Celery beat ([files](outreach/plane/)) | 12/12 passed on `makeplane/plane-backend:stable`, 2026-11-14; found the one-shot container bug | [#10286](https://github.com/orgs/makeplane/discussions/10286), 2026-11-14 | | |
| Polar | Cancel at period end, free renewal via their APScheduler job store ([files](outreach/polar/)) | 11/11 passed (built from `server/`, 2026-11-14); found a `KeyError` in their `DebounceMiddleware`; ChronoStack got `${var}` in `advance.to` | Draft bug report (send via dashboard support or Discord; no GitHub issues) | | |
| Saleor | Unpaid order expiry via Celery beat ([files](outreach/saleor/)) | Found a real bug: `expireOrdersAfter` ≥ 1 day expires far too late (24 h → 47 h 36 min, 48 h → ~62 d); pinned 15/15 on 3.23.49, documented behaviour passes 8/8 with the one-line fix. ChronoStack [#9](https://github.com/Asmodeus14/ChronoStack/issues/9): Saleor's custom beat scheduler skips its crontab tasks after a clock jump (interval tasks fine; workaround: triggers) | [#20019](https://github.com/saleor/saleor/issues/20019), 2026-11-15; fix PR [#20020](https://github.com/saleor/saleor/pull/20020), 2026-11-15 | Another contributor offered to fix it; we replied with the PR. A Saleor member redacted the ChronoStack mentions as advertising (2026-11-15); replied "understood" | |
| Twenty | Trash retention via BullMQ `TrashCleanupCronJob` ([files](outreach/twenty/)) | Found a real bug: trash cleanup deletes records up to a day before `trashRetentionDays` ends (14 d → 13 d 0 h 20 min for a record deleted at 23:50; 1 d → 11 min). Pinned 16/16 on v2.58.0; documented behaviour passes 14/14 with the one-line fix. ChronoStack: GUIDE said BullMQ fires in ~1 s, actually up to ~10 s and slow jobs block the queue; fixed with a queue-drained barrier. Untested lead (fix PR #30461 merged, so this can go ahead): "Days between triggers" uses `*/N` day-of-month, so it resets on the 1st | [#30460](https://github.com/twentyhq/twenty/issues/30460), 2026-11-15; fix PR [#30461](https://github.com/twentyhq/twenty/pull/30461), 2026-11-15 | Fix merged 2026-11-16 by a Twenty team member (re-triaged to `prio: medium`) | |
| Chatwoot | Snoozed conversations reopening after an outage, via sidekiq-cron ([files](outreach/chatwoot/)) | Found a bug: `ReopenSnoozedConversationsJob` only looks back 3 days, so a snooze that ended during a >3-day Sidekiq outage is never reopened. Pinned 21/21 on v4.19.1; expected behaviour passes 12/12 with the fix. First Ruby/sidekiq-cron run: works; needs landings within 60 s of the cron minute. ChronoStack bug found: app containers with `POSTGRES_PASSWORD` got the psql probe (fix in ChronoStack PR [#10](https://github.com/Asmodeus14/ChronoStack/pull/10)) | [#16667](https://github.com/chatwoot/chatwoot/issues/16667), 2026-11-15 | | |
| Formbricks | Survey close dates via the daily BullMQ scheduling job ([files](outreach/formbricks/)) | Found a bug: with `SURVEY_SCHEDULING_TIME_ZONE` at UTC+12 or later (e.g. Pacific/Auckland), picked open/close dates are scheduled a day late (editor stores noon UTC). Pinned 9/9 on 6.2.0; expected behaviour fails (survey still open on the picked day). No one-line fix: needs a plain-date representation | [#10021](https://github.com/formbricks/formbricks/issues/10021), 2026-11-15 | | |
| Lago | 13 months of billing via Clockwork + Sidekiq: month-end anniversaries (29/30/31), calendar, quarterly, yearly ([files](outreach/lago/)) | **No bugs found**: periods contiguous, amounts correct, invoice counts exact through 2028-01-23 (run stopped there: host low on memory). Leap-day billing and customer time zones (premium) not covered | Not posted (no bug) | | |
| tududi | Monthly recurring tasks (on day N, last weekday) completed month by month ([files](outreach/tududi/)) | Found bugs: west of UTC "monthly on the 1st" moves to the last day of the previous month and "on the 31st" skips Feb; "last Friday" skips a month after a 29th-31st occurrence (setUTCMonth overflow). Pinned 10/10 on 2.1.0; expected fails; passes 7/7 with a verified fix | [#2100](https://github.com/chrisvel/tududi/issues/2100), 2026-11-15; PR [#2101](https://github.com/chrisvel/tududi/pull/2101) (month-overflow part only; the timezone part is fixed by existing PR #2094) | rome-xi confirmed the bug | |
| Worklenz | Monthly "Nth weekday" recurring tasks via the recurring-task cron job ([files](outreach/worklenz/)) | Found a bug: moment `.day()` lands in the previous week, so 1st Monday goes to 28 Dec and then the schedule is stuck (duplicate guard), 2nd Tuesday is a week early, Sunday (0) is ignored, last weekday skips months. Pinned 7/7 on 3.3.1; expected fails; passes 5/5 with a verified fix. Also: the 3.3.1 image lacks database/pg-migrations-support (migrate:up fails) | [#959](https://github.com/Worklenz/worklenz/issues/959), 2026-11-16; fix PR [#960](https://github.com/Worklenz/worklenz/pull/960) (CLA required) | | |
