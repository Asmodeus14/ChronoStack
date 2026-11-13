# SPDX-License-Identifier: MIT OR Apache-2.0
"""Python probes for M0.

  python t.py now     -> {"time": time.time(), "datetime_now": ...}
  python t.py nohang  -> one JSON line per wait primitive (elapsed via time.monotonic)
"""
import asyncio
import datetime
import json
import os
import queue
import select
import sys
import threading
import time


def now():
    print(json.dumps({
        "time": time.time(),
        "datetime_now": datetime.datetime.now(datetime.timezone.utc).timestamp(),
        "monotonic": time.monotonic(),
    }))


def nohang():
    held = threading.Lock()
    held.acquire()

    def lock_acquire():
        held.acquire(timeout=1)

    def rlock_acquire():
        r = threading.RLock()
        t = threading.Thread(target=lambda: (r.acquire(), time.sleep(10)), daemon=True)
        t.start()
        time.sleep(0.1)
        r.acquire(timeout=1)

    def asyncio_sleep():
        asyncio.run(asyncio.sleep(1))

    def timer_1s():
        e = threading.Event()
        threading.Timer(1, e.set).start()
        e.wait(10)

    tests = {
        "Event.wait": lambda: threading.Event().wait(1),
        "Lock.acquire_timeout": lock_acquire,
        "RLock.acquire_timeout": rlock_acquire,
        "Condition.wait": lambda: (lambda c: (c.acquire(), c.wait(1)))(threading.Condition()),
        "Semaphore.acquire_timeout": lambda: threading.Semaphore(0).acquire(timeout=1),
        "queue.get_timeout": lambda: _ignore(queue.Empty, lambda: queue.Queue().get(timeout=1)),
        "threading.Timer": timer_1s,
        "time.sleep": lambda: time.sleep(1),
        "select.select": lambda: select.select([], [], [], 1),
        "asyncio.sleep": asyncio_sleep,
    }
    for name, fn in tests.items():
        done = threading.Event()

        def run(fn=fn, done=done):
            fn()
            done.set()

        t0 = time.monotonic()
        threading.Thread(target=run, daemon=True).start()
        finished = done.wait(6)  # the harness waits are monotonic in 3.11+; that is what we test
        ms = (time.monotonic() - t0) * 1000
        status = "HANG" if not finished else ("EARLY" if ms < 900 else ("SLOW" if ms > 2500 else "OK"))
        print(json.dumps({"test": name, "elapsed_ms": round(ms), "status": status}), flush=True)
    os._exit(0)


def _ignore(exc, fn):
    try:
        fn()
    except exc:
        pass


if __name__ == "__main__":
    {"now": now, "nohang": nohang}[sys.argv[1]]()
