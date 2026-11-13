# SPDX-License-Identifier: MIT OR Apache-2.0
# Celery beat: daily at 03:00 UTC; counts runs in Redis (celery:runs).
# beat_max_loop_interval=1 makes beat re-check the schedule every second
# (the default scheduler would otherwise sleep up to 300 s).
import redis
from celery import Celery
from celery.schedules import crontab

app = Celery("tasks", broker="redis://redis:6379/1")
app.conf.timezone = "UTC"
app.conf.beat_max_loop_interval = 1
app.conf.beat_schedule = {"daily": {"task": "tasks.daily", "schedule": crontab(hour=3, minute=0)}}
r = redis.Redis(host="redis")


@app.task
def daily():
    r.incr("celery:runs")
