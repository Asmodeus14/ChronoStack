# SPDX-License-Identifier: MIT OR Apache-2.0
# A daily Celery beat task that records each run in Postgres.
#
# The task runs at 03:00 UTC. Each run inserts a row into task_runs, stamped
# with Postgres now(), so the table shows on which (virtual) days it ran.
import psycopg
from celery import Celery
from celery.schedules import crontab

app = Celery("tasks", broker="redis://redis:6379/0")
app.conf.timezone = "UTC"
# Beat sleeps until the next due task, up to 300 s by default. With 1 it
# re-checks the schedule every second, so it notices when ChronoStack moves
# the clock.
app.conf.beat_max_loop_interval = 1
app.conf.beat_schedule = {
    "daily-cleanup": {"task": "tasks.daily_cleanup", "schedule": crontab(hour=3, minute=0)},
}


@app.task
def daily_cleanup():
    with psycopg.connect("host=db user=app password=app dbname=app") as conn:
        conn.execute("INSERT INTO task_runs (task) VALUES ('daily_cleanup')")
