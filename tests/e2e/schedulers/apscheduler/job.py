# SPDX-License-Identifier: MIT OR Apache-2.0
# APScheduler 3.x BlockingScheduler: daily at 03:00 UTC; counts runs in Redis
# (apscheduler:runs). Default settings, as most applications use it.
import redis
from apscheduler.schedulers.blocking import BlockingScheduler

r = redis.Redis(host="redis")
sched = BlockingScheduler(timezone="UTC")


@sched.scheduled_job("cron", hour=3, minute=0, misfire_grace_time=None, coalesce=True)
def daily():
    r.incr("apscheduler:runs")


sched.start()
