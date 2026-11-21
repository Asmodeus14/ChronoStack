# SPDX-License-Identifier: MIT OR Apache-2.0
# Celery worker for run_queues.py: `slow` takes 4 s, `later` is scheduled 2 h ahead.
import os
import time

import redis
from celery import Celery

app = Celery("celery_tasks", broker=os.environ["REDIS_URL"])
r = redis.Redis.from_url(os.environ["REDIS_URL"])


@app.task
def slow(i):
    time.sleep(4)
    r.incr("done:celery")


@app.task
def later(i):
    r.incr("done:celery_later")
