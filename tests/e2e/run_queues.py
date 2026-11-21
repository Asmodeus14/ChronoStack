# SPDX-License-Identifier: MIT OR Apache-2.0
"""Built-in queue barriers (bullmq, sidekiq, celery) against real workers.

Queues 3 BullMQ, 3 Sidekiq and 3 Celery jobs that each take 4 s, plus a Celery
task due in 2 hours, then runs `chronostack advance 1h` with the barriers from
tests/e2e/queues/chronostack.yml. When advance returns, every 4 s job must
have finished exactly once, and the 2-hour task must not have run (the celery
barrier must not wait for a task that isn't due):

- the barriers before the first jump wait for jobs already running (otherwise
  the jump expires BullMQ's locks and it runs the jobs again as "stalled");
- the sidekiq barrier must not trust Sidekiq's process records after the jump
  (their 60 s TTL expires with it) and waits for a heartbeat.

It first runs the sidekiq barrier alone, right after the worker process is
started: Sidekiq takes the jobs before it registers itself (on its first
heartbeat), so "no process registered" must not count as "nothing running".

The next `advance 1h` makes the Celery task due: the barrier must wait for it,
and it must run exactly once (Celery's Redis broker re-delivers tasks it holds
after a 1-hour visibility timeout, which a jump could trigger). A last advance
with nothing queued must also pass.
"""

import argparse
import os
import subprocess
import sys

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DIR = os.path.join(ROOT, "tests", "e2e", "queues")
JOBS = 3


def sh(args, timeout=900):
    env = dict(os.environ, MSYS_NO_PATHCONV="1")
    p = subprocess.run(args, cwd=DIR, capture_output=True, text=True, timeout=timeout,
                       encoding="utf-8", errors="replace", env=env)
    return p.returncode, p.stdout, p.stderr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    a = ap.parse_args()
    cli = os.path.abspath(a.cli) if a.cli else os.path.join(
        ROOT, "target", "debug", "chronostack" + (".exe" if os.name == "nt" else ""))

    def cs(*args, timeout=900):
        return sh([cli, *args], timeout)

    def container(service):
        rc, out, _ = sh(["docker", "compose", "-p", "queues-chronostack", "ps", "-q", service])
        assert rc == 0 and out.strip(), f"no container for {service}"
        return out.strip().splitlines()[0]

    def redis(*cmd):
        rc, out, err = sh(["docker", "exec", container("redis"), "redis-cli", *cmd])
        assert rc == 0, err
        return out.strip()

    def done():
        return [int(v or 0) for v in redis("MGET", "done:bullmq", "done:sidekiq", "done:celery").splitlines()]

    def later():
        return int(redis("GET", "done:celery_later") or 0)

    failures = []
    try:
        cs("down", "-v")
        rc, out, err = cs("up", "--wait", timeout=1200)
        assert rc == 0, f"up failed:\n{out}\n{err}"
        for svc in ("sidekiq", "bullmq", "celery"):
            rc, _, err = sh(["docker", "exec", container(svc), "sh", "-c",
                             "for i in $(seq 1 300); do [ -f /tmp/ready ] && exit 0; sleep 2; done; exit 1"],
                            timeout=700)
            assert rc == 0, f"{svc} worker never became ready: {err}"

        # Phase A: the sidekiq barrier alone, while Sidekiq is still booting.
        config = os.path.join(DIR, "chronostack.yml")
        with open(config, encoding="utf-8") as f:
            both = f.read()
        redis("SET", "done:sidekiq", "0")
        rc, _, err = sh(["docker", "exec", "-w", "/app", "-e", "REDIS_URL=redis://redis:6379/0",
                         container("sidekiq"), "ruby", "enqueue.rb"])
        assert rc == 0, err
        with open(config, "w", encoding="utf-8", newline="\n") as f:
            f.write("version: 1\nadvance: { barrier_timeout: 90s }\n"
                    "barriers:\n  - name: sidekiq-idle\n    sidekiq: { service: redis }\n")
        try:
            rc, out, err = cs("advance", "1h", "--no-probe")
        finally:
            with open(config, "w", encoding="utf-8", newline="\n") as f:
                f.write(both)
        print(out)
        got = int(redis("GET", "done:sidekiq") or 0)
        if rc != 0 or got != JOBS:
            failures.append(f"sidekiq barrier alone, worker booting: exit {rc}, done:sidekiq = {got}, expected {JOBS}")

        # Phase B: all barriers, jobs running when advance starts.
        redis("MSET", "done:bullmq", "0", "done:sidekiq", "0", "done:celery", "0", "done:celery_later", "0")
        rc, _, err = sh(["docker", "exec", "-w", "/work", container("bullmq"), "node", "bull-enqueue.mjs"])
        assert rc == 0, err
        rc, _, err = sh(["docker", "exec", "-w", "/app", "-e", "REDIS_URL=redis://redis:6379/0",
                         container("sidekiq"), "ruby", "enqueue.rb"])
        assert rc == 0, err

        rc, _, err = sh(["docker", "exec", "-w", "/app", container("celery"), "python", "celery_enqueue.py"])
        assert rc == 0, err

        rc, out, err = cs("advance", "1h", "--no-probe")
        print(out)
        if rc != 0:
            failures.append(f"advance with jobs running exited {rc}: {err.strip()}")
        got = done()
        if got != [JOBS, JOBS, JOBS]:
            failures.append(f"when advance returned, done (bullmq, sidekiq, celery) = {got}, expected [{JOBS}, {JOBS}, {JOBS}]")
        if later() != 0:
            failures.append(f"the Celery task due in 2 h already ran {later()} time(s) after 1 h")
        if "(before the jump)" not in out:
            failures.append("advance did not check the barriers before the jump")

        # Phase C: the Celery task becomes due: the barrier waits for it, and it runs once.
        rc, out, err = cs("advance", "1h", "--no-probe")
        print(out)
        if rc != 0:
            failures.append(f"advance making the Celery task due exited {rc}: {err.strip()}")
        if later() != 1:
            failures.append(f"after the 2 h mark, the scheduled Celery task ran {later()} time(s), expected 1")

        # Phase D: nothing queued: every barrier passes, and nothing ran twice in the meantime.
        rc, out, err = cs("advance", "1h", "--no-probe")
        print(out)
        if rc != 0:
            failures.append(f"advance with idle queues exited {rc}: {err.strip()}")
        got = done()
        if got != [JOBS, JOBS, JOBS] or later() != 1:
            failures.append(f"after an idle advance, done = {got}, later = {later()}, expected [{JOBS}, {JOBS}, {JOBS}] and 1")
    finally:
        cs("down", "-v")

    if failures:
        print("FAILED:\n- " + "\n- ".join(failures))
        sys.exit(1)
    print(f"PASSED: {JOBS} BullMQ, {JOBS} Sidekiq and {JOBS} Celery jobs each finished exactly once across "
          "the jump, and the Celery task due later ran once, when it was due")


if __name__ == "__main__":
    main()
