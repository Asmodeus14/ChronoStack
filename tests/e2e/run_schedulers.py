# SPDX-License-Identifier: MIT OR Apache-2.0
"""Scheduler compatibility matrix.

Each scheduler is configured for "daily at 03:00 UTC" (and BullMQ also gets a
job delayed by 2 h). Virtual time is moved from 02:30 past those times with
plain `chronostack advance` (no triggers), and we record whether each
scheduler fires on its own within a real-time budget.

The output documents behaviour for docs/MVP_SCOPE.md. The script fails only
when the observed behaviour differs from the documented expectation below, or
on harness errors.

Usage: python tests/e2e/run_schedulers.py [--cli PATH]
"""
import argparse
import datetime as dt
import json
import os
import subprocess
import sys
import time

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DIR = os.path.join(ROOT, "tests", "e2e", "schedulers")
WAIT_S = 20  # real seconds a scheduler gets to react after each advance

# Documented expectations (update together with docs/MVP_SCOPE.md):
#   fires   = runs on its own after the clock moves
#   trigger = does not fire in time: use a chronostack.yml trigger
EXPECT = {
    "celery beat (beat_max_loop_interval=1)": "fires",
    "node-cron": "trigger",
    "BullMQ job scheduler (cron)": "fires",  # measured 2026-11-13: ~1 s after advance
    "BullMQ delayed job (2h)": "fires",  # measured 2026-11-13: immediately
}


def sh(args, timeout=900):
    p = subprocess.run(args, cwd=DIR, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout, p.stderr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    a = ap.parse_args()
    cli = os.path.abspath(a.cli) if a.cli else os.path.join(ROOT, "target", "debug", "chronostack" + (".exe" if os.name == "nt" else ""))

    def cs(*args):
        return sh([cli, *args])

    def js(*args):
        rc, out, _ = cs(*args, "--json")
        return rc, json.loads(out.strip().splitlines()[-1]) if out.strip() else {}

    def ex(svc, *cmd):
        s = json.load(open(os.path.join(DIR, ".chronostack", "session.json"), encoding="utf-8"))
        args = ["docker", "compose"]
        for f in s["compose_files"]:
            args += ["-f", f]
        return sh(args + ["-p", s["project"], "exec", "-T", svc, *cmd], timeout=120)

    def counter(key):
        out = ex("redis", "redis-cli", "GET", key)[1].strip()
        return int(out) if out.isdigit() else 0

    if os.path.exists(os.path.join(DIR, ".chronostack", "session.json")):
        cs("down", "-v")
    rc, out, err = cs("up")
    if rc:
        print((err or out)[-1500:])
        sys.exit(1)
    rows, failures = [], []
    try:
        for _ in range(60):
            _, d = js("doctor")
            if all((s.get("probe") or {}).get("status") == "ok" for s in d.get("services", []) if s["virtualized"]):
                break
            time.sleep(2)
        time.sleep(5)  # let workers connect

        v = js("now")[1]["virtual_ns"] / 1e9
        day = dt.datetime.fromtimestamp(v, dt.timezone.utc).date() + dt.timedelta(days=2)
        start = dt.datetime(day.year, day.month, day.day, 2, 30, tzinfo=dt.timezone.utc)
        rc, _ = js("advance", "--to", start.strftime("%Y-%m-%dT%H:%M:%SZ"), "--no-probe")
        assert rc == 0, "positioning failed"
        time.sleep(WAIT_S)  # catch-up runs from the positioning jump happen here
        ex("redis", "redis-cli", "DEL", "celery:runs", "nodecron:runs", "done:daily", "done:delayed")
        assert ex("bullmq", "node", "bullmq-add.js", "scheduler")[0] == 0
        assert ex("bullmq", "node", "bullmq-add.js", "delayed")[0] == 0
        time.sleep(3)

        # 02:30 -> 03:30: every daily-at-03:00 scheduler is due.
        js("advance", "1h", "--no-probe")
        t0 = time.monotonic()
        seen = {}
        keys = {"celery beat (beat_max_loop_interval=1)": "celery:runs", "node-cron": "nodecron:runs",
                "BullMQ job scheduler (cron)": "done:daily"}
        while time.monotonic() - t0 < WAIT_S and len(seen) < len(keys):
            for name, key in keys.items():
                if name not in seen and counter(key) > 0:
                    seen[name] = time.monotonic() - t0
            time.sleep(1)
        for name in keys:
            rows.append((name, f"{seen[name]:.0f}s after advance" if name in seen else f"no run within {WAIT_S}s",
                         "fires" if name in seen else "trigger"))

        # 03:30 -> 04:35: the 2h delayed job (added at 02:30) is due.
        js("advance", "65m", "--no-probe")
        t0 = time.monotonic()
        fired = None
        while time.monotonic() - t0 < WAIT_S:
            if counter("done:delayed") > 0:
                fired = time.monotonic() - t0
                break
            time.sleep(1)
        rows.append(("BullMQ delayed job (2h)", f"{fired:.0f}s after advance" if fired is not None else f"no run within {WAIT_S}s",
                     "fires" if fired is not None else "trigger"))
    finally:
        cs("down", "-v")

    print(f"\n{'SCHEDULER':42} {'OBSERVED':28} RESULT   EXPECTED")
    for name, obs, result in rows:
        exp = EXPECT.get(name)
        status = "" if exp is None else ("ok" if exp == result else "MISMATCH")
        print(f"{name:42} {obs:28} {result:8} {exp or '(measured)'} {status}")
        if exp is not None and exp != result:
            failures.append(name)
    print("\n" + ("ALL EXPECTATIONS MET" if not failures else f"MISMATCH: {', '.join(failures)}"))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
