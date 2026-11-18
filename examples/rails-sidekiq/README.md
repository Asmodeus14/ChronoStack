# Sidekiq example

A small Ruby app with a time-based job: a trial lasts 14 days, and signing up schedules the "trial expired" email for the day it ends. ChronoStack moves the clock and the job runs.

| Piece | Where |
|---|---|
| Redis and a Sidekiq worker | [`compose.yml`](compose.yml), [`Dockerfile`](Dockerfile) |
| `TrialExpiredJob`: records the email in the Redis list `emails_sent` | [`jobs.rb`](jobs.rb) |
| Sign-up script: `TrialExpiredJob.perform_in(14.days)` | [`signup.rb`](signup.rb) |

It is plain Ruby + Sidekiq rather than a full Rails app, to keep it small. In Rails the same job is `TrialExpiredJob.perform_in(14.days, user.id)` from a controller or model.

Nothing in the app knows about ChronoStack. [`chronostack.yml`](chronostack.yml) adds a `sidekiq` barrier, so each clock stop waits until Sidekiq has finished the jobs that became due.

## Why `perform_in` and not sidekiq-cron

`perform_in` and `perform_at` work on their own: Sidekiq's scheduled-job poller compares against the virtual wall clock. [sidekiq-cron](https://github.com/sidekiq-cron/sidekiq-cron) only runs a job if its 30 s poll lands within 60 s after the job's time, so an ordinary `advance` past the job skips it. If you use it, see the [guide](../../docs/GUIDE.md#4-make-your-scheduled-jobs-run).

## Run it

From this directory, after installing the CLI (see [Getting started](../../docs/GETTING_STARTED.md#1-install)):

```sh
chronostack up --wait
docker compose -p sidekiq-trial-chronostack exec sidekiq ruby signup.rb ada@example.com

chronostack advance 13d     # the trial is still running: no email yet
chronostack advance 2d      # day 15: the job has run

docker compose -p sidekiq-trial-chronostack exec redis redis-cli LRANGE emails_sent 0 -1

chronostack down -v
```

`LRANGE` prints one line, `trial_expired:ada@example.com:<virtual time>`. After `advance 13d` the list is empty.
