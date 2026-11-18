# Celery beat example

A small Python app with a daily [Celery beat](https://docs.celeryq.dev/en/stable/userguide/periodic-tasks.html) task, to show that ChronoStack makes it run once per virtual day.

| Piece | Where |
|---|---|
| Postgres, Redis, a Celery worker and Celery beat | [`compose.yml`](compose.yml) |
| A task that runs every day at 03:00 UTC and inserts a row into `task_runs` | [`tasks.py`](tasks.py) |
| The table; `ran_at` defaults to Postgres `now()`, so it shows the virtual day | [`schema.sql`](schema.sql) |

It is plain Python + Celery rather than a full Django project, to keep it small. A Django project that uses Celery's default beat scheduler behaves the same way.

Nothing in the app knows about ChronoStack. [`chronostack.yml`](chronostack.yml) only sets a short wait at each stop.

## Why it works without a trigger

- Beat polls the wall clock. By default it can sleep up to 300 s between checks, so it would not notice the clock moving. `--max-interval 1` (also set as `beat_max_loop_interval` in `tasks.py`) makes it re-check every second. `chronostack init` warns when this is missing.
- This example uses beat's default scheduler. django-celery-beat's `DatabaseScheduler` has a limitation with crontab tasks after a clock jump ([#9](https://github.com/Asmodeus14/ChronoStack/issues/9)); for that, declare a trigger (see the [guide](../../docs/GUIDE.md#4-make-your-scheduled-jobs-run)).
- One big jump runs a daily task **once**, because beat runs whatever is due when it notices the jump. Use `--step 1d` so the clock stops on each day.

## Run it

From this directory, after installing the CLI (see [Getting started](../../docs/GETTING_STARTED.md#1-install)):

```sh
chronostack up --wait
chronostack advance 3d --step 1d

# one row per virtual day
docker compose -p celery-daily-task-chronostack exec db \
  psql -U app -c "select id, ran_at from task_runs order by id"

chronostack down -v
```

The query returns three rows, one on each of the next three days:

```
 id |            ran_at
----+-------------------------------
  1 | 2026-11-19 16:12:47.994454+00
  2 | 2026-11-20 16:12:47.985946+00
  3 | 2026-11-21 16:12:47.987074+00
```

Without `--step`, `chronostack advance 3d` adds a single row.

The run time (`16:12` above) is when the clock landed, not 03:00: beat sends a missed job when it notices the jump (see the [guide](../../docs/GUIDE.md#4-make-your-scheduled-jobs-run)). To land just after 03:00, use `advance: { to: next 03:01 }` with `step: 1d` in `chronostack.yml`.
