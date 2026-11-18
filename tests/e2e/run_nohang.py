# SPDX-License-Identifier: MIT OR Apache-2.0
"""No-hang suite through the product (M0's NH tests on the real CLI + clockd).

Starts tests/e2e/nohang with `chronostack up --wait`, moves virtual time to
+1 day, +30 days, +365 days and then freezes it. At each point every runtime
(C glibc/musl, Python 3.10/3.12/3.12-alpine, Node 22 slim/alpine, JVM 21/17,
Ruby 3.3, PHP 8.3, .NET 8) runs
its wait primitives with an absolute deadline of "virtual now + 1 s". Each wait
must return after 0.9-2.5 s of real time; a hang means the shim's deadline
translation is broken on this architecture. Before the waits, each language
runtime's own wall clock must read virtual time (within 5 s), so a runtime
that isn't virtualized at all can't pass by never hanging.

Usage: python tests/e2e/run_nohang.py [--cli PATH]
"""
import argparse
import json
import os
import platform
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DIR = os.path.join(ROOT, "tests", "e2e", "nohang")

SUITES = [
    ("c/glibc", "c-glibc", ["cstest", "nohang"]),
    ("c/musl", "c-musl", ["cstest", "nohang"]),
    ("python3.10", "py310", ["python", "/t/t.py", "nohang"]),
    ("python3.12", "py312", ["python", "/t/t.py", "nohang"]),
    ("python3.12-alpine", "py312-alpine", ["python", "/t/t.py", "nohang"]),
    ("node22", "node22", ["node", "/t/t.js", "nohang"]),
    ("node22-alpine", "node22-alpine", ["node", "/t/t.js", "nohang"]),
    ("jvm21", "jvm21", ["java", "/t/Test.java", "nohang"]),
    ("jvm17", "jvm17", ["java", "/t/Test.java", "nohang"]),
    ("ruby3.3", "ruby33", ["ruby", "/t/t.rb"]),
    ("php8.3", "php83", ["php", "/t/t.php"]),
    ("dotnet8", "dotnet8", ["dotnet", "/app/nohang.dll"]),
]


# Language-level wall-clock reads: (service, command, JSON field in epoch seconds).
NOW = [
    ("python3.10", "py310", ["python", "/t/t.py", "now"], "time"),
    ("python3.12-alpine", "py312-alpine", ["python", "/t/t.py", "now"], "time"),
    ("node22", "node22", ["node", "/t/t.js", "now"], "now"),
    ("jvm21", "jvm21", ["java", "/t/Test.java", "now"], "currentTimeMillis"),
    ("jvm17", "jvm17", ["java", "/t/Test.java", "now"], "currentTimeMillis"),
    ("ruby3.3", "ruby33", ["ruby", "/t/t.rb", "now"], "now"),
    ("php8.3", "php83", ["php", "/t/t.php", "now"], "now"),
    ("dotnet8", "dotnet8", ["dotnet", "/app/nohang.dll", "now"], "now"),
]


def sh(args, timeout=900):
    try:
        p = subprocess.run(args, cwd=DIR, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return 124, "", "TIMEOUT"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    a = ap.parse_args()
    cli = os.path.abspath(a.cli) if a.cli else os.path.join(ROOT, "target", "debug", "chronostack" + (".exe" if os.name == "nt" else ""))

    def exec_in(svc, cmd):
        s = json.load(open(os.path.join(DIR, ".chronostack", "session.json"), encoding="utf-8"))
        args = ["docker", "compose"]
        for f in s["compose_files"]:
            args += ["-f", f]
        return sh(args + ["-p", s["project"], "exec", "-T", svc, *cmd], timeout=240)

    if os.path.exists(os.path.join(DIR, ".chronostack", "session.json")):
        sh([cli, "down", "-v"])
    rc, out, err = sh([cli, "up", "--wait", "--wait-timeout", "180s"], timeout=1800)
    if rc:
        print((err or out)[-2000:])
        sys.exit(1)
    rows, fails = [], []
    try:
        steps = [("+1d", ["advance", "1d", "--no-probe"]),
                 ("+30d", ["advance", "29d", "--no-probe"]),
                 ("+365d", ["advance", "335d", "--no-probe"]),
                 ("+365d frozen", ["freeze"])]
        for label, cmd in steps:
            rc, out, err = sh([cli, *cmd])
            if rc:
                fails.append(f"{label}: `{' '.join(cmd)}` failed: {(err or out)[-300:]}")
                break
            print(f"\n=== {label}", flush=True)
            if label == "+1d":
                rc, out, _ = sh([cli, "now", "--json"])
                virtual = json.loads(out.strip().splitlines()[-1])["virtual_ns"] / 1e9
                with ThreadPoolExecutor(max_workers=4) as pool:
                    seen = list(pool.map(lambda n: (n, exec_in(n[1], n[2])), NOW))
                for (area, _svc, _cmd, field), (rc, out, err) in seen:
                    try:
                        got = float(json.loads(out.strip().splitlines()[-1])[field])
                    except (ValueError, KeyError, IndexError):
                        fails.append(f"{label} {area}: wall clock not readable (rc={rc}) {(err or out).strip()[-200:]}")
                        continue
                    off = got - virtual
                    ok = abs(off) <= 5
                    print(f"  {area:18} wall clock {'virtual' if ok else 'NOT VIRTUAL'} ({off:+.1f}s)", flush=True)
                    if not ok:
                        fails.append(f"{label} {area}: wall clock is {off:+.0f}s from virtual time")
            with ThreadPoolExecutor(max_workers=4) as pool:
                results = list(pool.map(lambda s: (s[0], exec_in(s[1], s[2])), SUITES))
            for area, (rc, out, err) in results:
                lines = [json.loads(l) for l in out.splitlines() if l.strip().startswith("{")]
                if not lines:
                    fails.append(f"{label} {area}: no output (rc={rc}) {err.strip()[-200:]}")
                    print(f"  {area:18} NO OUTPUT rc={rc}")
                    continue
                for r in lines:
                    st = r["status"]
                    rows.append((label, area, r["test"], st, r.get("elapsed_ms")))
                    if st not in ("OK", "SKIP"):
                        fails.append(f"{label} {area} {r['test']}: {st} {r.get('elapsed_ms')} ms")
                ok = sum(1 for r in lines if r["status"] == "OK")
                skip = sum(1 for r in lines if r["status"] == "SKIP")
                print(f"  {area:18} {ok}/{len(lines) - skip} OK" + (f", {skip} skipped" if skip else ""), flush=True)
        sh([cli, "unfreeze"])
    finally:
        sh([cli, "down", "-v"])

    total = len(rows)
    ok = sum(1 for r in rows if r[3] == "OK")
    print(f"\narch={platform.machine()} tests={total} ok={ok} skipped={sum(1 for r in rows if r[3] == 'SKIP')}")
    if fails:
        print("FAILURES:\n  " + "\n  ".join(fails))
        sys.exit(1)
    print("NO HANGS")


if __name__ == "__main__":
    main()
