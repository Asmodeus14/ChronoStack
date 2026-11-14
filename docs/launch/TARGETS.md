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
| Plane | Auto-close and auto-archive via Celery beat ([files](outreach/plane/)) | 12/12 passed on `makeplane/plane-backend:stable`, 2026-11-14; found the one-shot container bug | Draft ready (waits for the fix release) | | |
