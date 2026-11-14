<!-- Draft for https://github.com/makeplane/plane/discussions, category: Show and tell. Not posted yet. Post only after the release that fixes one-shot containers (the migrator) is out. -->

**Title:** Tested Plane's auto-close and auto-archive across 9 weeks of virtual time

Hi! I'm building [ChronoStack](https://github.com/Asmodeus14/ChronoStack), an open-source tool that moves a whole Docker Compose stack forward in time together (Postgres `now()`, Python's clock, Redis, …) while your schedulers keep running.

Project automation is hard to test by hand: nobody waits a month to see whether a stale issue gets closed. So I ran the published `makeplane/plane-backend:stable` image (migrator, worker and beat-worker, unmodified, with Postgres, Valkey and RabbitMQ) and let Celery beat run `archive_and_close_old_issues` by itself every virtual day. No triggers or mocks.

Setup: a project with **auto-close after 1 month** (to Cancelled) and **auto-archive after 1 month**, and one In Progress issue that nobody touches.

| Virtual time | Expected | Result |
|---|---|---|
| Day 0, 12:00 | Issue created, In Progress | started ✓ |
| Day 30, 01:01 | 29 days 13 h since the last update: not yet | started ✓ |
| Day 31, 01:01 | Past 30 days: the 01:00 run closes it | cancelled ✓ |
| Day 34 | Closing counts as an update, so no archive yet | cancelled ✓ |
| Day 61 | 30 days after closing: archived | cancelled, archived ✓ |

12/12 checks passed, and the 9 virtual weeks took about 3.5 minutes. Nothing was broken. One detail I confirmed along the way: auto-close refreshes `updated_at`, so the archive timer starts from the close, not from the last human edit. That seems right to me, but it's worth knowing if anyone expects "archive 1 month after the last activity".

The only change to Plane's setup was adding `--max-interval 1` to `celery beat`, so beat notices clock jumps within a second.

The compose file, seed script, scenario and run report are here: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/plane

If it's useful, I'm happy to help turn it into a CI check for the automation tasks. And if anyone is on an Apple-silicon Mac, `chronostack doctor` output would help me a lot: macOS is the one platform I haven't been able to test yet.
