# SPDX-License-Identifier: MIT OR Apache-2.0
"""`chronostack init`: generated chronostack.yml for the e2e fixtures and the demo.

1. `init --print` in each fixture must detect what's in its compose file
   (only `docker compose config` is used; nothing is started).
2. In a copy of the demo without its chronostack.yml: `init` writes the file,
   refuses to overwrite it without --force, and the generated file is accepted
   by `config`, `up --wait`, `doctor` and `advance 1d` (skip with --no-stack).
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# fixture directory -> strings the printed file or summary must contain
EXPECT = {
    "tests/e2e/queues": [
        "sidekiq: { service: redis }",
        "#   bullmq: { service: redis, queues: [<queue-name>] }",
    ],
    "tests/e2e/schedulers": [
        "celery: Celery worker with embedded beat (-B)",
        "Add `--max-interval 1`",
    ],
    "tests/e2e/pgcron": ["db: Postgres with pg_cron"],
    "examples/saas-trial-demo": [
        "db: Postgres",
        "cache: Redis",
        "#     exec: { service: worker,",
    ],
}

failures = []


def run(cli, args, cwd, timeout=600):
    env = dict(os.environ, MSYS_NO_PATHCONV="1")
    return subprocess.run([cli, *args], cwd=cwd, capture_output=True, text=True,
                          timeout=timeout, encoding="utf-8", errors="replace", env=env)


def check(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name}")
    if not ok:
        failures.append(name)
        if detail:
            print("     " + detail.strip().replace("\n", "\n     ")[:3000])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli", required=True)
    ap.add_argument("--no-stack", action="store_true", help="skip up/doctor/advance")
    a = ap.parse_args()
    cli = os.path.abspath(a.cli)

    for rel, needles in EXPECT.items():
        p = run(cli, ["init", "--print"], os.path.join(ROOT, rel))
        out = p.stdout + p.stderr
        check(f"init --print in {rel}", p.returncode == 0 and all(n in out for n in needles),
              f"exit {p.returncode}; missing {[n for n in needles if n not in out]}\n{out}")

    tmp = tempfile.mkdtemp(prefix="chronostack-init-")
    demo = os.path.join(tmp, "demo")
    shutil.copytree(os.path.join(ROOT, "examples", "saas-trial-demo"), demo,
                    ignore=shutil.ignore_patterns(".chronostack", "node_modules"))
    os.remove(os.path.join(demo, "chronostack.yml"))
    try:
        p = run(cli, ["init", "--print"], demo)
        check("--print writes nothing", p.returncode == 0
              and not os.path.exists(os.path.join(demo, "chronostack.yml")), p.stderr)
        p = run(cli, ["init"], demo)
        check("init writes chronostack.yml", p.returncode == 0
              and os.path.exists(os.path.join(demo, "chronostack.yml")), p.stdout + p.stderr)
        p = run(cli, ["init"], demo)
        check("init refuses to overwrite", p.returncode != 0 and "--force" in p.stderr, p.stderr)
        p = run(cli, ["init", "--force"], demo)
        check("init --force overwrites", p.returncode == 0, p.stderr)
        p = run(cli, ["config"], demo)
        check("config accepts the generated file", p.returncode == 0 and "is valid" in p.stdout,
              p.stdout + p.stderr)
        if not a.no_stack:
            try:
                p = run(cli, ["up", "--wait"], demo, timeout=900)
                check("up --wait with the generated file", p.returncode == 0, p.stdout + p.stderr)
                p = run(cli, ["doctor"], demo)
                check("doctor", p.returncode == 0, p.stdout + p.stderr)
                p = run(cli, ["advance", "1d"], demo)
                check("advance 1d", p.returncode == 0, p.stdout + p.stderr)
            finally:
                run(cli, ["down", "-v"], demo, timeout=300)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print("ALL CHECKS PASSED" if not failures else f"{len(failures)} check(s) failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
