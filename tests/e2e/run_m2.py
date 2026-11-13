# SPDX-License-Identifier: MIT OR Apache-2.0
"""M2 end-to-end: step → trigger → barrier on the SaaS trial demo, and pg_cron sub-stepping.

Usage (from the repository root):
    python tests/e2e/run_m2.py [--cli PATH] [--repeat N] [--skip-demo] [--skip-pgcron] [--skip-triggers]

Requires Docker with Compose v2 and the `chronostack/clockd:dev` image
(build it with `docker build -f docker/clockd.Dockerfile -t chronostack/clockd:dev .`).
"""
import argparse
import datetime as dt
import hashlib
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
DEMO = os.path.join(ROOT, "examples", "saas-trial-demo")
PGCRON = os.path.join(ROOT, "tests", "e2e", "pgcron")
TRIGGER = os.path.join(ROOT, "tests", "e2e", "trigger")
H = 3600
D = 86400
FAILS = []


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""), flush=True)
    if not ok:
        FAILS.append(name)
    return ok


def sh(args, cwd, timeout=900):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout, p.stderr


class Stack:
    def __init__(self, cli, cwd):
        self.cli, self.cwd = cli, cwd

    def cs(self, *args, timeout=900):
        return sh([self.cli, *args], self.cwd, timeout)

    def js(self, *args, timeout=900):
        rc, out, err = self.cs(*args, "--json", timeout=timeout)
        try:
            return rc, json.loads(out.strip().splitlines()[-1])
        except (ValueError, IndexError):
            return rc, {"_raw": out, "_err": err}

    def exec(self, svc, *cmd, timeout=60):
        s = json.load(open(os.path.join(self.cwd, ".chronostack", "session.json"), encoding="utf-8"))
        args = ["docker", "compose"]
        for f in s["compose_files"]:
            args += ["-f", f]
        return sh(args + ["-p", s["project"], "exec", "-T", svc, *cmd], self.cwd, timeout)

    def virtual_s(self):
        return self.js("now")[1]["virtual_ns"] / 1e9

    def up(self):
        if os.path.exists(os.path.join(self.cwd, ".chronostack", "session.json")):
            self.cs("down", "-v")
        t0 = time.monotonic()
        rc, out, err = self.cs("up", timeout=1800)
        check(f"up ({os.path.basename(self.cwd)})", rc == 0, f"{time.monotonic() - t0:.0f}s" if rc == 0 else (err or out)[-800:])
        return rc == 0

    def wait_healthy(self, services, timeout=120):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            _, d = self.js("doctor")
            ok = {s["service"]: (s.get("probe") or {}).get("status") == "ok" for s in d.get("services", [])}
            if all(ok.get(n) for n in services):
                return True
            time.sleep(2)
        return False

    def down(self):
        rc, out, err = self.cs("down", "-v")
        check(f"down -v ({os.path.basename(self.cwd)})", rc == 0, (err or out)[-300:] if rc else "")


def next_midnight(virtual_s, days_ahead=1):
    d = dt.datetime.fromtimestamp(virtual_s, dt.timezone.utc).date() + dt.timedelta(days=days_ahead)
    return dt.datetime(d.year, d.month, d.day, tzinfo=dt.timezone.utc)


def iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


# ------------------------------------------------------------------------- demo

def demo_once(cli, run_no):
    st = Stack(cli, DEMO)
    h = hashlib.sha256(open(os.path.join(DEMO, "compose.yml"), "rb").read()).hexdigest()
    if not st.up():
        return None
    try:
        check("demo: all services virtualized and healthy", st.wait_healthy(["db", "cache", "api", "worker"]))
        for _ in range(60):
            if st.exec("api", "node", "scripts/call.js", "users")[0] == 0:
                break
            time.sleep(1)

        rc, cfg_out, err = st.cs("config")
        check("config: chronostack.yml valid", rc == 0 and "expire-trials" in cfg_out, (cfg_out + err).strip()[-200:])

        # Start of a fresh UTC day so the outcome is identical on every run.
        day0 = next_midnight(st.virtual_s(), 2)
        rc, r = st.js("advance", "--to", iso(day0), "--no-probe")
        check("position at a UTC midnight (advance --to)", rc == 0, iso(day0))

        rc, token, err = st.exec("api", "node", "scripts/call.js", "signup", f"alice{run_no}@example.com")
        token = token.strip()
        check("signup", rc == 0 and len(token) == 32, err[-200:])
        me = json.loads(st.exec("api", "node", "scripts/call.js", "me", token)[1])
        check("fresh session valid, plan=trial", me["status"] == 200 and me["body"]["plan"] == "trial", json.dumps(me))

        # Dry run of the next 25h: one 03:00 trigger, final stop.
        rc, plan = st.js("advance", "25h", "--dry-run")
        trig = [s for s in plan.get("stops", []) if s["triggers"]]
        check("dry-run 25h: exactly one expire-trials stop at 03:00",
              rc == 0 and len(trig) == 1 and trig[0]["at"].endswith("T03:00:00Z"), json.dumps(trig)[:200])

        rc, r = st.js("advance", "25h")
        check("advance 25h: 1 trigger run, probes ok", rc == 0 and r["trigger_runs"] == 1, f"rc={rc} runs={r.get('trigger_runs')}")
        me = json.loads(st.exec("api", "node", "scripts/call.js", "me", token)[1])
        check("after 25h: session expired (401)", me["status"] == 401, json.dumps(me))
        users = json.loads(st.exec("api", "node", "scripts/call.js", "users")[1])
        check("after 25h: trial still active", users[0]["plan"] == "trial", users[0]["plan"])

        t0 = time.monotonic()
        rc, r = st.js("advance", "14d")
        took = time.monotonic() - t0
        check("advance 14d: 14 daily runs (no --step needed)", rc == 0 and r["trigger_runs"] == 14,
              f"rc={rc} runs={r.get('trigger_runs')}")
        check("advance 14d: under 60 s real time", took < 60, f"{took:.1f}s")
        bad = [s for s in r.get("stops", []) if any(not t["ok"] for t in s["triggers"] + s["barriers"])]
        check("advance 14d: every trigger and barrier succeeded", not bad, json.dumps(bad)[:300])

        users = json.loads(st.exec("api", "node", "scripts/call.js", "users")[1])
        u = users[0]
        expected = day0 + dt.timedelta(days=14, hours=3)
        got = dt.datetime.fromisoformat(u["downgraded_at"].replace("Z", "+00:00")) if u["downgraded_at"] else None
        skew = abs((got - expected).total_seconds()) if got else None
        check("trial expired on day 14 at the 03:00 run", u["plan"] == "free" and skew is not None and skew < 120,
              f"plan={u['plan']} downgraded_at={u['downgraded_at']} expected≈{iso(expected)}")
        rc, cnt, _ = st.exec("db", "psql", "-U", "app", "-d", "app", "-Atc",
                             f"select count(*) || '/' || sum(affected) from job_runs where ran_at >= '{iso(day0)}'")
        check("job_runs since day 0: 15 runs, exactly 1 downgrade", cnt.strip() == "15/1", cnt.strip())

        rc, r = st.js("advance", "3d", "--step", "1d")
        check("advance 3d --step 1d: 3 scheduled runs, 6 stops", rc == 0 and r["trigger_runs"] == 3 and len(r["stops"]) == 6,
              f"runs={r.get('trigger_runs')} stops={len(r.get('stops', []))}")

        return (u["plan"], round(skew / 60) if skew is not None else None, cnt.strip())
    finally:
        st.down()
        same = hashlib.sha256(open(os.path.join(DEMO, "compose.yml"), "rb").read()).hexdigest() == h
        check("demo compose file unchanged", same)


# ------------------------------------------------------- trigger timeout, --wait

PROC_SLEEPS = "for p in /proc/[0-9]*; do tr '\\0' ' ' < $p/cmdline 2>/dev/null; echo; done | grep -c '^sleep 30[01]'"


def triggers(cli):
    st = Stack(cli, TRIGGER)
    if os.path.exists(os.path.join(TRIGGER, ".chronostack", "session.json")):
        st.cs("down", "-v")
    rc, out, err = st.cs("up", "--wait", "--wait-timeout", "90s", timeout=1800)
    check("up --wait: returns 0 once every probe is ok", rc == 0 and "ok" in out, (err or out)[-400:])
    if rc != 0:
        st.down()
        return
    try:
        # Triggers fire at 01:00 (alp) and 02:00 (deb). Step to whichever comes
        # next + 30 min, so exactly one trigger fires per advance.
        for _ in range(2):
            now = dt.datetime.fromtimestamp(st.virtual_s(), dt.timezone.utc)
            due = []
            for svc, hour in (("alp", 1), ("deb", 2)):
                t = now.replace(hour=hour, minute=0, second=0, microsecond=0)
                if t <= now:
                    t += dt.timedelta(days=1)
                due.append((t, svc))
            at, svc = min(due)
            rc, r = st.js("advance", "--to", iso(at + dt.timedelta(minutes=30)), "--no-probe")
            t = [x for s in r.get("stops", []) for x in s["triggers"]]
            detail = (t[0].get("detail") or "") if t else ""
            check(f"trigger timeout ({svc}): exit 4, reported as stopped",
                  rc == 4 and len(t) == 1 and "timed out" in detail and "stopped in the container" in detail, f"rc={rc} {detail}")
            time.sleep(1)
            left = st.exec(svc, "sh", "-c", PROC_SLEEPS)[1].strip()
            check(f"trigger timeout ({svc}): no trigger process left in the container", left == "0", f"{left} left")
    finally:
        st.down()


# ---------------------------------------------------------------------- pg_cron

def pg_jump(st, psql):
    """Postgres background processes after a 14-day jump (TECHNICAL_FEASIBILITY 3.2,
    previously unresolved). The autovacuum launcher and the checkpointer compare
    wall-clock timestamps; a forward jump should make work due once, not cause a
    storm or stall the server."""
    def one(q):
        out = psql(q)[1].strip()
        return int(out) if out.lstrip("-").isdigit() else -1

    psql("create table if not exists av(i int); insert into av select generate_series(1, 20000); delete from av")
    ckpt0 = one("select checkpoints_timed + checkpoints_req from pg_stat_bgwriter")
    rc, _ = st.js("advance", "14d", "--max-jump", "none", "--no-probe")
    check("pg jump: advance 14d in one jump", rc == 0)
    t0 = time.monotonic()
    av_after = None
    while time.monotonic() - t0 < 75:
        if one("select count(*) from pg_stat_user_tables where relname = 'av' and last_autovacuum is not null") == 1:
            av_after = time.monotonic() - t0
            break
        time.sleep(3)
    q0 = time.monotonic()
    rc, out, _ = psql("select 1")
    latency = time.monotonic() - q0
    ckpt = one("select checkpoints_timed + checkpoints_req from pg_stat_bgwriter") - ckpt0
    check("pg jump: server responsive after a 14-day jump", rc == 0 and out.strip() == "1" and latency < 5, f"{latency:.2f}s")
    check("pg jump: no checkpoint storm (at most 3 checkpoints)", 0 <= ckpt <= 3, f"{ckpt} checkpoint(s)")
    print(f"  measured: autovacuum of a table with 20000 dead rows "
          f"{'ran %.0fs after the jump' % av_after if av_after is not None else 'did not run within 75 s'}; "
          f"{ckpt} checkpoint(s) in that time", flush=True)
    psql("drop table av")


def pgcron(cli):
    st = Stack(cli, PGCRON)
    if not st.up():
        return
    try:
        check("pg_cron: db healthy", st.wait_healthy(["db"]))
        psql = lambda q: st.exec("db", "psql", "-U", "postgres", "-Atc", q)  # noqa: E731
        pg_jump(st, psql)
        psql("create extension if not exists pg_cron")
        psql("create table if not exists hb(t timestamptz default now()); create table if not exists daily(t timestamptz default now())")
        psql("select cron.schedule('hb', '* * * * *', 'insert into hb default values')")
        psql("select cron.schedule('daily', '0 3 * * *', 'insert into daily default values')")

        start = next_midnight(st.virtual_s(), 2) + dt.timedelta(hours=2, minutes=30)
        rc, _ = st.js("advance", "--to", iso(start), "--max-jump", "none", "--no-probe")
        check("pg_cron: positioned at 02:30 UTC", rc == 0, iso(start))
        time.sleep(3)

        rc, plan = st.js("advance", "3d", "--dry-run")
        pinned = [x for x in plan.get("stops", []) if x.get("pinned")]
        check("pg_cron detected → schedule-aware plan: 3 job stops at 03:00:01 + final stop",
              plan.get("pg_cron") == ["db"] and plan.get("pg_cron_mode") == "schedule-aware"
              and len(plan.get("stops", [])) == 4 and len(pinned) == 3
              and all(x["at"].endswith("T03:00:01Z") for x in pinned),
              f"mode={plan.get('pg_cron_mode')} stops={len(plan.get('stops', []))} pinned={[x['at'] for x in pinned]}")

        psql("truncate daily")
        rc, _ = st.js("advance", "3d", "--max-jump", "none", "--no-probe")
        time.sleep(4)
        n = int(psql("select count(*) from daily")[1].strip() or -1)
        check("pg_cron negative: one raw 3-day jump skips the daily job", rc == 0 and n == 0, f"{n} runs")

        psql("truncate daily")
        t0 = time.monotonic()
        rc, r = st.js("advance", "3d", "--no-probe")
        took = time.monotonic() - t0
        time.sleep(4)
        n = int(psql("select count(*) from daily")[1].strip() or -1)
        check("pg_cron: advance 3d (schedule-aware) runs the daily job 3 times",
              rc == 0 and n == 3 and r.get("pg_cron_mode") == "schedule-aware", f"{n} runs, {len(r.get('stops', []))} stops")
        check("pg_cron: schedule-aware advance 3d is fast (< 30 s)", took < 30, f"{took:.1f}s (hourly mode took ~92s)")

        psql("select cron.schedule('every30s', '30 seconds', 'select 1')")
        rc, plan = st.js("advance", "3d", "--dry-run")
        check("pg_cron fallback: unsupported schedule → hourly sub-steps (72 stops)",
              plan.get("pg_cron_mode") == "hourly" and len(plan.get("stops", [])) == 72 and "30 seconds" in (plan.get("pg_cron_reason") or ""),
              f"mode={plan.get('pg_cron_mode')} reason={plan.get('pg_cron_reason')}")
        psql("select cron.unschedule('every30s')")

        # Wildcard jobs: `hb` (* * * * *) runs once per stop unless --pg-cron-all.
        rc, plan = st.js("advance", "3d", "--dry-run")
        hb = [w for w in plan.get("pg_cron_wildcards", []) if w["schedule"] == "* * * * *"]
        check("pg_cron wildcard: dry-run reports due vs. will-run for `* * * * *`",
              len(hb) == 1 and hb[0]["due"] == 4320 and hb[0]["will_run"] < 10, json.dumps(hb))
        rc, _, err = st.cs("advance", "3d", "--pg-cron-all", "--dry-run")
        check("pg_cron wildcard: --pg-cron-all refuses more than 2000 stops", rc != 0 and "2000" in err, err.strip()[-200:])
        cnt = lambda: int(psql("select count(*) from hb")[1].strip() or -1)  # noqa: E731
        before = cnt()
        rc, _ = st.js("advance", "30m", "--no-probe")
        time.sleep(3)
        plain = cnt() - before
        before = cnt()
        rc2, r = st.js("advance", "30m", "--pg-cron-all", "--no-probe")
        time.sleep(3)
        every = cnt() - before
        check("pg_cron wildcard: one 30m jump runs `* * * * *` at most twice", rc == 0 and plain <= 2, f"{plain} runs")
        check("pg_cron wildcard: advance 30m --pg-cron-all runs it about 30 times",
              rc2 == 0 and 29 <= every <= 33, f"{every} runs, {len(r.get('stops', []))} stops")
    finally:
        st.down()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--skip-demo", action="store_true")
    ap.add_argument("--skip-pgcron", action="store_true")
    ap.add_argument("--skip-triggers", action="store_true")
    a = ap.parse_args()
    cli = os.path.abspath(a.cli) if a.cli else None
    if not cli:
        rc, _, err = sh(["cargo", "build", "-p", "chronostack"], ROOT)
        if rc:
            print(err)
            sys.exit(1)
        cli = os.path.join(ROOT, "target", "debug", "chronostack" + (".exe" if os.name == "nt" else ""))

    if not a.skip_demo:
        outcomes = []
        for i in range(a.repeat):
            print(f"\n=== demo run {i + 1}/{a.repeat}", flush=True)
            outcomes.append(demo_once(cli, i))
        if a.repeat > 1:
            check(f"demo: {a.repeat} runs produced identical outcomes", len(set(outcomes)) == 1 and None not in outcomes,
                  str(outcomes))
    if not a.skip_triggers:
        print("\n=== trigger timeout, up --wait", flush=True)
        triggers(cli)
    if not a.skip_pgcron:
        print("\n=== pg_cron", flush=True)
        pgcron(cli)

    print(f"\n{'ALL CHECKS PASSED' if not FAILS else f'{len(FAILS)} FAILED: ' + ', '.join(FAILS)}")
    sys.exit(1 if FAILS else 0)


if __name__ == "__main__":
    main()
