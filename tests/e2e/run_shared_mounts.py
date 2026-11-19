# SPDX-License-Identifier: MIT OR Apache-2.0
"""`chronostack up` and storage shared with the user's own stack.

In a copy of tests/e2e/shared (a writable bind mount, a read-only bind mount
and an ordinary named volume):
1. `up` refuses to start, names the writable bind mount and the flag, and
   starts nothing (no session file, no containers).
2. `up --allow-shared-mounts --wait` starts, and `doctor` lists both bind
   mounts (text and JSON) but not the named volume.
"""

import argparse
import json
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
PROJECT = "e2e-shared-chronostack"
failures = []


def run(args, cwd, timeout=600):
    env = dict(os.environ, MSYS_NO_PATHCONV="1")
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout,
                          encoding="utf-8", errors="replace", env=env)


def check(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name}")
    if not ok:
        failures.append(name)
        if detail:
            print("     " + detail.strip().replace("\n", "\n     ")[:3000])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli", required=True)
    a = ap.parse_args()
    cli = os.path.abspath(a.cli)

    tmp = tempfile.mkdtemp(prefix="chronostack-shared-")
    work = os.path.join(tmp, "stack")
    shutil.copytree(os.path.join(ROOT, "tests", "e2e", "shared"), work)
    for d in ("data", "conf"):
        os.makedirs(os.path.join(work, d))
    try:
        p = run([cli, "up"], work)
        err = p.stdout + p.stderr
        check("up refuses a writable bind mount", p.returncode != 0
              and "app: ./data is shared with your own stack (bind mount, mounted at /data)" in err
              and "--allow-shared-mounts" in err, err)
        check("the read-only bind mount is only a note",
              "note: app: ./conf is shared with your own stack (bind mount, read-only" in err
              and "./conf" not in err.split("would share storage", 1)[-1], err)
        check("the named volume is not reported", "app-data" not in err, err)
        check("nothing was started",
              not os.path.exists(os.path.join(work, ".chronostack", "session.json"))
              and run(["docker", "ps", "-q", "--filter", f"label=com.docker.compose.project={PROJECT}"],
                      work).stdout.strip() == "", err)

        p = run([cli, "up", "--allow-shared-mounts", "--wait"], work, timeout=900)
        out = p.stdout + p.stderr
        check("up --allow-shared-mounts starts the stack", p.returncode == 0
              and "--allow-shared-mounts" in p.stderr, out)
        p = run([cli, "doctor"], work)
        check("doctor lists the shared mounts", p.returncode == 0
              and "shared with your own stack (not copied):" in p.stdout
              and "./data" in p.stdout and "./conf" in p.stdout, p.stdout + p.stderr)
        p = run([cli, "doctor", "--json"], work)
        try:
            mounts = json.loads(p.stdout.strip().splitlines()[-1])["shared_mounts"]
        except (ValueError, KeyError, IndexError):
            mounts = None
        check("doctor --json lists them", mounts is not None
              and sorted((m["target"], m["kind"], m["read_only"]) for m in mounts)
              == [("/conf", "bind", True), ("/data", "bind", False)], p.stdout + p.stderr)
    finally:
        run([cli, "down", "-v"], work, timeout=300)
        shutil.rmtree(tmp, ignore_errors=True)

    print("ALL CHECKS PASSED" if not failures else f"{len(failures)} check(s) failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
