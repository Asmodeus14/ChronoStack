# SPDX-License-Identifier: MIT OR Apache-2.0
"""M1 end-to-end test: the real `chronostack` CLI against tests/e2e/compose.yml.

Usage (from the repository root):
    python tests/e2e/run_m1.py [--cli PATH] [--skip-image-build]

Requires Docker with Compose v2. Builds the CLI (cargo) and the clockd image
unless told otherwise. Exits non-zero if any check fails.
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
E2E = os.path.join(ROOT, "tests", "e2e")
DAY = 86400
FAILS = []


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""), flush=True)
    if not ok:
        FAILS.append(name)


def sh(args, cwd=E2E, timeout=600):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout,
                       encoding="utf-8", errors="replace")
    return p.returncode, p.stdout, p.stderr


def cs(cli, *args, timeout=600):
    return sh([cli, *args], timeout=timeout)


def session():
    with open(os.path.join(E2E, ".chronostack", "session.json"), encoding="utf-8") as f:
        return json.load(f)


def compose_exec(svc, *cmd):
    s = session()
    args = ["docker", "compose"]
    for f in s["compose_files"]:
        args += ["-f", f]
    args += ["-p", s["project"], "exec", "-T", svc, *cmd]
    return sh(args, timeout=60)


def now_json(cli):
    rc, out, err = cs(cli, "now", "--json")
    return json.loads(out)


def sha(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    ap.add_argument("--skip-image-build", action="store_true")
    a = ap.parse_args()

    cli = a.cli
    if not cli:
        rc, _, err = sh(["cargo", "build", "-p", "chronostack"], cwd=ROOT)
        if rc != 0:
            print(err)
            sys.exit(1)
        cli = os.path.join(ROOT, "target", "debug", "chronostack" + (".exe" if os.name == "nt" else ""))
    if not a.skip_image_build:
        rc, _, err = sh(["docker", "build", "-q", "-f", "docker/clockd.Dockerfile", "-t", "chronostack/clockd:dev", "."],
                        cwd=ROOT, timeout=1800)
        check("clockd image builds (runs clockd unit tests)", rc == 0, err[-300:] if rc else "")
        if rc:
            sys.exit(1)

    if os.path.exists(os.path.join(E2E, ".chronostack", "session.json")):
        cs(cli, "down", "-v")
    compose_hash = sha(os.path.join(E2E, "compose.yml"))

    # --- up ----------------------------------------------------------------
    t0 = time.monotonic()
    rc, out, err = cs(cli, "up")
    check("chronostack up", rc == 0, f"{time.monotonic() - t0:.0f}s" if rc == 0 else (err or out)[-600:])
    if rc != 0:
        sys.exit(1)
    try:
        run_checks(cli)
    finally:
        rc, out, err = cs(cli, "down", "-v")
        check("chronostack down -v", rc == 0, (err or out)[-300:] if rc else "")
        _, ps, _ = sh(["docker", "ps", "-aq", "--filter", "label=com.docker.compose.project=e2e-chronostack"])
        check("no containers left after down", ps.strip() == "", ps.strip())
        _, vols, _ = sh(["docker", "volume", "ls", "-q", "--filter", "label=com.docker.compose.project=e2e-chronostack"])
        check("no volumes left after down -v", vols.strip() == "", vols.strip())
        check("session file removed", not os.path.exists(os.path.join(E2E, ".chronostack", "session.json")))
        check("user compose file unchanged", sha(os.path.join(E2E, "compose.yml")) == compose_hash)

    print(f"\n{'ALL CHECKS PASSED' if not FAILS else f'{len(FAILS)} FAILED: ' + ', '.join(FAILS)}")
    sys.exit(1 if FAILS else 0)


def run_checks(cli):
    # --- doctor: plan, shim in PID 1, probes ---------------------------------
    # Services (e.g. Postgres initdb) may still be starting right after `up`.
    deadline = time.monotonic() + 90
    while True:
        rc, out, err = cs(cli, "doctor", "--json")
        if rc == 0 or time.monotonic() > deadline:
            break
        time.sleep(2)
    d = json.loads(out)
    svc = {s["service"]: s for s in d["services"]}
    for name in ["db", "cache", "app", "legacy"]:
        s = svc[name]
        check(f"doctor: {name} virtualized ({s['arch']}/{s['libc']})", s["virtualized"] is True, s.get("reason") or "")
        check(f"doctor: {name} shim loaded in PID 1", s["shim_in_pid1"] is True, str(s["shim_in_pid1"]))
        check(f"doctor: {name} probe ok", (s["probe"] or {}).get("status") == "ok", json.dumps(s["probe"]))
    w = svc["whoami"]
    check("doctor: static Go image (whoami) flagged not virtualizable",
          w["virtualized"] is False and "static" in (w["reason"] or ""), w["reason"] or "")
    check("doctor exit code 0 when healthy", rc == 0, f"rc={rc}")

    s = session()
    check("session: clockd published on 127.0.0.1 only", True, f"port {s['port']}")
    _, lp, _ = compose_exec("app", "sh", "-c", "echo $LD_PRELOAD")
    lp = lp.strip()
    check("app: existing LD_PRELOAD preserved after ours", lp.endswith(":/lib/libz.so.1") and "libchronoclock-" in lp, lp)

    # --- advance + probes ------------------------------------------------------
    rc, out, err = cs(cli, "advance", "7d", "--json")
    r = json.loads(out)
    off_d = r["clock"]["offset_ns"] / 1e9 / DAY
    check("advance 7d: offset ≈ 7d", abs(off_d - 7) < 0.001, f"{off_d:.5f}d")
    bad = [p for p in r["probes"] if p["status"] != "ok"]
    check("advance 7d: every virtualized service sees it", rc == 0 and not bad,
          ", ".join(f"{p['service']} {p['skew_s']:+.2f}s" if p["skew_s"] is not None else p["service"] for p in r["probes"]))

    # --- Redis expiry ----------------------------------------------------------
    compose_exec("cache", "redis-cli", "SET", "session", "v", "EX", str(DAY))
    rc, _, _ = cs(cli, "advance", "25h", "--no-probe")
    _, got, _ = compose_exec("cache", "redis-cli", "GET", "session")
    check("redis: EX 86400 key gone after advance 25h", rc == 0 and got.strip() == "", repr(got.strip()))

    # --- Postgres now() ----------------------------------------------------------
    v = now_json(cli)["virtual_ns"] / 1e9
    _, pg, _ = compose_exec("db", "psql", "-U", "shop", "-d", "shop", "-Atc", "select extract(epoch from now())")
    try:
        skew = float(pg.strip()) - v
    except ValueError:
        skew = 999
    check("postgres: now() ≈ virtual time", abs(skew) < 2, f"{skew:+.2f}s")

    # --- freeze / unfreeze ---------------------------------------------------------
    cs(cli, "freeze")
    a = now_json(cli)["virtual_ns"]
    time.sleep(1.5)
    b = now_json(cli)["virtual_ns"]
    _, dt1, _ = compose_exec("legacy", "date", "+%s")
    time.sleep(1.2)
    _, dt2, _ = compose_exec("legacy", "date", "+%s")
    check("freeze: virtual time stops (controller)", a == b, f"Δ={b - a} ns")
    check("freeze: virtual time stops (inside container)", dt1.strip() == dt2.strip(), f"{dt1.strip()} vs {dt2.strip()}")
    cs(cli, "unfreeze")
    c = now_json(cli)
    check("unfreeze: resumes from frozen value, no jump", 0 <= c["virtual_ns"] - b < 2e9 and not c["frozen"],
          f"+{(c['virtual_ns'] - b) / 1e9:.2f}s")

    # --- clockd restart keeps the clock ------------------------------------------------
    before = now_json(cli)["offset_ns"]
    s = session()
    args = ["docker", "compose"]
    for f in s["compose_files"]:
        args += ["-f", f]
    sh(args + ["-p", s["project"], "restart", "chronostack-clockd"], timeout=120)
    after = None
    for _ in range(40):
        try:
            after = now_json(cli)["offset_ns"]
            break
        except Exception:  # noqa: BLE001
            time.sleep(0.5)
    check("clockd restart: clock state preserved", after is not None and abs(after - before) < 2e9,
          f"{(after or 0) / 1e9 / DAY:.3f}d vs {before / 1e9 / DAY:.3f}d")

    # --- API security & input validation -------------------------------------------------
    s = session()  # the CLI may have re-resolved the port after the restart
    req = urllib.request.Request(f"http://127.0.0.1:{s['port']}/v1/clock", headers={"Authorization": "Bearer wrong"})
    try:
        urllib.request.urlopen(req, timeout=5)
        code = 200
    except urllib.error.HTTPError as e:
        code = e.code
    check("API rejects a wrong token (401)", code == 401, str(code))
    rc, _, err = cs(cli, "advance", "0s")
    check("advance 0s rejected", rc != 0, err.strip()[-120:])
    rc, _, err = cs(cli, "advance", "-1d")
    check("advance -1d rejected", rc != 0, err.strip()[-120:])


if __name__ == "__main__":
    main()
