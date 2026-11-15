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
| Saleor | Unpaid order expiry via Celery beat ([files](outreach/saleor/)) | Found a real bug: `expireOrdersAfter` ≥ 1 day expires far too late (24 h → 47 h 36 min, 48 h → ~62 d); pinned 15/15 on 3.23.49, documented behaviour passes 8/8 with the one-line fix. ChronoStack [#9](https://github.com/Asmodeus14/ChronoStack/issues/9): Saleor's custom beat scheduler skips its crontab tasks after a clock jump (interval tasks fine; workaround: triggers) | [#20019](https://github.com/saleor/saleor/issues/20019), 2026-11-15; fix PR [#20020](https://github.com/saleor/saleor/pull/20020), 2026-11-15 | Another contributor offered to fix it; we replied with the PR | |
| Twenty | Trash retention via BullMQ `TrashCleanupCronJob` ([files](outreach/twenty/)) | Found a real bug: trash cleanup deletes records up to a day before `trashRetentionDays` ends (14 d → 13 d 0 h 20 min for a record deleted at 23:50; 1 d → 11 min). Pinned 16/16 on v2.58.0; documented behaviour passes 14/14 with the one-line fix. ChronoStack: GUIDE said BullMQ fires in ~1 s, actually up to ~10 s and slow jobs block the queue; fixed with a queue-drained barrier. Untested lead: "Days between triggers" uses `*/N` day-of-month, so it resets on the 1st | [#30460](https://github.com/twentyhq/twenty/issues/30460), 2026-11-15 | | |
