# SPDX-License-Identifier: MIT OR Apache-2.0
"""Simulated users on the virtual clock: reproducibility and the demo (nightly).

1. tests/e2e/expose, scenarios/users.yml: run --repeat times (default 10). Every
   run passes and gives the same timeline: each user's steps at the same
   offsets from the scenario's start, with the same choices and statuses.
2. examples/saas-trial-demo, scenarios/cohort.yml: 20 trialists over 30
   virtual days with a webhook outage. Two runs pass with the same timeline.
3. The same scenario with compose.bug.yml (the worker treats a 503 as
   delivered) fails at its last step, the exactly-once webhook check.
"""

import argparse
import datetime as dt
import json
import os
import subprocess
import sys

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
EXPOSE = os.path.join(ROOT, "tests", "e2e", "expose")
DEMO = os.path.join(ROOT, "examples", "saas-trial-demo")
failures = []


def run(args, cwd, timeout=1800):
    env = dict(os.environ, MSYS_NO_PATHCONV="1")
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout,
                          encoding="utf-8", errors="replace", env=env)


def check(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name}", flush=True)
    if not ok:
        failures.append(name)
        if detail:
            print("     " + str(detail).strip().replace("\n", "\n     ")[:3000])


def report_of(p):
    try:
        return json.loads(p.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return {}


def timeline(rep):
    """The issue timeline without per-run details (run ids in URLs, ports):
    (offset s, cohort, user, step id, what, outcome)."""
    start = dt.datetime.fromisoformat(rep["virtual_start"].replace("Z", "+00:00"))
    out = []
    for e in rep.get("users", {}).get("timeline", []):
        at = dt.datetime.fromisoformat(e["virtual_time"].replace("Z", "+00:00"))
        words = e["event"].split()
        step, what = words[0], words[1]
        if what == "think":
            outcome = words[2]
        elif what == "branch":
            outcome = words[2]
        elif "->" in words:
            outcome = words[words.index("->") + 1]
        else:
            outcome = " ".join(words[1:3])
        out.append(((at - start).total_seconds(), e["cohort"], e["user"], step, what, outcome))
    return out


def repeated(cli, cwd, scenario, n, label, up_args=()):
    """Bring the stack up, run the scenario n times, compare timelines."""
    if os.path.exists(os.path.join(cwd, ".chronostack", "session.json")):
        run([cli, "down", "-v"], cwd, 600)
    p = run([cli, *up_args, "up", "--wait"], cwd, 1800)
    check(f"{label}: up --wait", p.returncode == 0, p.stdout[-1500:] + p.stderr)
    if p.returncode:
        return None
    try:
        lines = []
        for i in range(n):
            p = run([cli, "run", scenario, "--json"], cwd, 1800)
            rep = report_of(p)
            if not up_args:
                check(f"{label}: run {i + 1} passes", p.returncode == 0 and rep.get("result") == "passed",
                      p.stdout[-2000:] + p.stderr)
            lines.append((p, rep))
        return lines
    finally:
        run([cli, "down", "-v"], cwd, 600)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli", required=True)
    ap.add_argument("--repeat", type=int, default=10)
    ap.add_argument("--skip-demo", action="store_true")
    a = ap.parse_args()
    cli = os.path.abspath(a.cli)

    runs = repeated(cli, EXPOSE, "scenarios/users.yml", a.repeat, "expose users")
    if runs:
        tls = [timeline(rep) for _, rep in runs if rep]
        check(f"expose users: {a.repeat} runs, one timeline", len(tls) == a.repeat and tls[0]
              and all(t == tls[0] for t in tls), [t[:4] for t in tls[:2]])

    if not a.skip_demo:
        runs = repeated(cli, DEMO, "scenarios/cohort.yml", 2, "demo cohort")
        if runs:
            tls = [timeline(rep) for _, rep in runs if rep]
            u = runs[0][1].get("users", {}).get("users", {})
            check("demo cohort: 20 trialists, all completed", u.get("total") == 20 and u.get("completed") == 20, u)
            check("demo cohort: two runs, one timeline", len(tls) == 2 and tls[0] and tls[0] == tls[1],
                  [t[:4] for t in tls])
        runs = repeated(cli, DEMO, "scenarios/cohort.yml", 1, "demo cohort with compose.bug.yml",
                        up_args=("-f", "compose.yml", "-f", "compose.bug.yml"))
        if runs:
            p, rep = runs[0]
            failed = rep.get("failed_step") or {}
            check("the bug variant fails at the exactly-once webhook check",
                  p.returncode == 6 and failed.get("index") == 5 and "exactly once" in str(failed.get("detail")),
                  json.dumps(failed)[:1500] + p.stderr[-500:])

    print("ALL CHECKS PASSED" if not failures else f"{len(failures)} check(s) failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
