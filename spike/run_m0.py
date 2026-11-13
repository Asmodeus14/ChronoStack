# SPDX-License-Identifier: MIT OR Apache-2.0
"""M0 feasibility harness.

Runs the M0 test matrix against spike/compose.yml and writes
spike/results/m0-results.json and spike/results/m0-results.md.

Usage (from repository root):
    python spike/run_m0.py [--only SECTION[,SECTION...]] [--keep]

Sections: baseline, reads, nohang, timers, failopen, stress, bench, redis, postgres, pgcron, arm64
"""
import argparse
import json
import os
import re
import subprocess
import sys
import time

# Windows consoles/redirects default to a legacy code page; results contain non-ASCII.
for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

HERE = os.path.dirname(os.path.abspath(__file__))
PROJECT = "cs-m0"
DC = ["docker", "compose", "-f", os.path.join(HERE, "compose.yml"), "-p", PROJECT]
PAGE = "/chronostack/clock"
DAY = 86400
RESULTS = []
TOL = 2.0  # seconds of allowed skew for clock-read checks (docker exec latency on Docker Desktop)


# --------------------------------------------------------------------------- helpers

def run(args, timeout=120, input=None):
    try:
        p = subprocess.run(args, capture_output=True, text=True, timeout=timeout, input=input,
                           encoding="utf-8", errors="replace")
        return p.returncode, p.stdout.strip(), p.stderr.strip()
    except subprocess.TimeoutExpired as e:
        return 124, (e.stdout or "") if isinstance(e.stdout, str) else "", "TIMEOUT"


def ex(svc, cmd, timeout=60, env=None):
    args = DC + ["exec", "-T"]
    for k, v in (env or {}).items():
        args += ["-e", f"{k}={v}"]
    return run(args + [svc] + cmd, timeout=timeout)


def ctl(*a):
    rc, out, err = ex("clockd", ["chronoctl", PAGE, *map(str, a)], timeout=30)
    if rc != 0:
        raise RuntimeError(f"chronoctl {a} failed: {err}")
    return json.loads(out.splitlines()[-1])


def clock():
    return ctl("get")


def record(test_id, area, desc, expected, observed, ok):
    status = "PASS" if ok is True else ("FAIL" if ok is False else str(ok))
    RESULTS.append(dict(id=test_id, area=area, description=desc, expected=expected,
                        observed=observed, status=status))
    print(f"[{status:7}] {test_id:24} {area:14} {desc} :: {observed}", flush=True)


def jlines(text):
    out = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def check_value(test_id, area, desc, getter, expect_virtual=True):
    """Compare a clock value read in a container with clockd's virtual (or real) time."""
    before = clock()
    try:
        val = getter()
    except Exception as e:  # noqa: BLE001
        record(test_id, area, desc, "value", f"error: {e}", False)
        return
    after = clock()
    key = "virtual_ns" if expect_virtual else "real_ns"
    lo, hi = before[key] / 1e9 - TOL, after[key] / 1e9 + TOL
    ok = lo <= val <= hi
    ref = (before[key] + after[key]) / 2e9
    record(test_id, area, desc, f"{'virtual' if expect_virtual else 'REAL'} time ±{TOL}s",
           f"skew {val - ref:+.2f}s (offset {before['offset_ns'] / 1e9 / DAY:.2f}d)", ok)


def num(out):
    m = re.search(r"-?\d+(\.\d+)?", out)
    if not m:
        raise ValueError(f"no number in {out!r}")
    return float(m.group(0))


def psql(svc, sql, timeout=60):
    return ex(svc, ["psql", "-U", "postgres", "-d", "postgres", "-Atq", "-v", "ON_ERROR_STOP=1", "-c", sql],
              timeout=timeout)


def rcli(svc, *args, timeout=30):
    return ex(svc, ["redis-cli", *map(str, args)], timeout=timeout)


def wait_ready():
    for svc, cmd in [("pg", ["pg_isready", "-U", "postgres"]), ("pga", ["pg_isready", "-U", "postgres"]),
                     ("redis", ["redis-cli", "ping"]), ("redisa", ["redis-cli", "ping"])]:
        for _ in range(60):
            rc, out, _ = ex(svc, cmd, timeout=20)
            if rc == 0 and ("accepting" in out or "PONG" in out):
                break
            time.sleep(1)
        else:
            raise RuntimeError(f"{svc} not ready")


# --------------------------------------------------------------------------- sections

def sec_baseline():
    ctl("set-offset", 0)
    check_value("BASE-C", "c/glibc", "offset 0: clock_gettime == real", lambda: jlines(ex("toolsg", ["cstest", "now"])[1])[0]["clock_gettime_realtime"], expect_virtual=False)
    check_value("BASE-PG", "postgres16", "offset 0: clock_timestamp == real", lambda: num(psql("pg", "select extract(epoch from clock_timestamp())")[1]), expect_virtual=False)


READ_TARGETS = [
    # id, area, service, command, json key or None (plain number)
    ("RD-C-GNU", "c/glibc", "toolsg", ["cstest", "now"], None),
    ("RD-C-MUSL", "c/musl", "toolsm", ["cstest", "now"], None),
    ("RD-DATE-GNU", "coreutils", "toolsg", ["date", "+%s"], "plain"),
    ("RD-DATE-BUSYBOX", "busybox", "toolsm", ["date", "+%s"], "plain"),
    ("RD-NODE", "node22-slim", "node", ["node", "/t/t.js", "now"], "now"),
    ("RD-NODE-NEWDATE", "node22-slim", "node", ["node", "/t/t.js", "now"], "new_date"),
    ("RD-NODE-ALPINE", "node22-alpine", "nodea", ["node", "/t/t.js", "now"], "now"),
    ("RD-PY310", "python3.10", "py310", ["python", "/t/t.py", "now"], "time"),
    ("RD-PY312", "python3.12", "py312", ["python", "/t/t.py", "now"], "time"),
    ("RD-PY312-DT", "python3.12", "py312", ["python", "/t/t.py", "now"], "datetime_now"),
    ("RD-PY312-ALPINE", "python3.12-alp", "py312a", ["python", "/t/t.py", "now"], "time"),
    ("RD-JAVA", "jvm21", "java", ["java", "/t/Test.java", "now"], "currentTimeMillis"),
    ("RD-JAVA-INSTANT", "jvm21", "java", ["java", "/t/Test.java", "now"], "instant_now"),
]


def sec_reads():
    ctl("set-offset", 7 * DAY)
    for tid, area, svc, cmd, key in READ_TARGETS:
        if key is None:  # cstest now: check every API separately
            rc, out, err = ex(svc, cmd)
            d = jlines(out)[0]
            ref = clock()["virtual_ns"] / 1e9
            for api in ["clock_gettime_realtime", "clock_gettime_coarse", "gettimeofday", "time", "timespec_get"]:
                skew = d[api] - ref
                record(f"{tid}-{api}", area, f"{api} at +7d", "virtual ±2s", f"skew {skew:+.2f}s", abs(skew) <= TOL + 1)
            tai_skew = d["clock_gettime_tai"] - ref
            record(f"{tid}-tai", area, "CLOCK_TAI at +7d (TAI = UTC + leap offset)", "virtual +0..40s",
                   f"skew {tai_skew:+.2f}s", -TOL <= tai_skew <= 40 + TOL)
            real = clock()["real_ns"] / 1e9
            record(f"{tid}-rawsyscall", area, "NEG-8 raw syscall(SYS_clock_gettime)", "REAL time (not intercepted)",
                   f"skew vs real {d['raw_syscall'] - real:+.2f}s", abs(d["raw_syscall"] - real) <= TOL + 1)
            continue
        if key == "plain":
            check_value(tid, area, f"{' '.join(cmd)} at +7d", lambda: num(ex(svc, cmd)[1]))
        else:
            check_value(tid, area, f"{key} at +7d", lambda svc=svc, cmd=cmd, key=key: jlines(ex(svc, cmd, timeout=120)[1])[0][key])
    check_value("RD-PG-CLOCK", "postgres16", "clock_timestamp() at +7d", lambda: num(psql("pg", "select extract(epoch from clock_timestamp())")[1]))
    check_value("RD-PG-NOW", "postgres16", "now() (new txn) at +7d", lambda: num(psql("pg", "select extract(epoch from now())")[1]))
    check_value("RD-PG-TIMEOFDAY", "postgres16", "timeofday() at +7d", lambda: num(psql("pg", "select extract(epoch from timeofday()::timestamptz)")[1]))
    check_value("RD-PGA-CLOCK", "postgres16-alp", "clock_timestamp() at +7d", lambda: num(psql("pga", "select extract(epoch from clock_timestamp())")[1]))
    check_value("RD-REDIS-TIME", "redis7", "TIME at +7d", lambda: num(rcli("redis", "TIME")[1]))
    check_value("RD-REDISA-TIME", "redis7-alp", "TIME at +7d", lambda: num(rcli("redisa", "TIME")[1]))
    # Negative: static binaries / Go
    check_value("NEG-1-GO", "go (static)", "Go time.Now() at +7d", lambda: num(ex("toolsm", ["gonow"])[1]), expect_virtual=False)
    check_value("NEG-2-STATIC", "c static musl", "static binary time() at +7d", lambda: num(ex("toolsm", ["staticnow"])[1]), expect_virtual=False)


NOHANG_SUITES = [
    ("c/glibc", "toolsg", ["cstest", "nohang"]),
    ("c/musl", "toolsm", ["cstest", "nohang"]),
    ("python3.10", "py310", ["python", "/t/t.py", "nohang"]),
    ("python3.12", "py312", ["python", "/t/t.py", "nohang"]),
    ("python3.12-alp", "py312a", ["python", "/t/t.py", "nohang"]),
    ("node22-slim", "node", ["node", "/t/t.js", "nohang"]),
    ("node22-alpine", "nodea", ["node", "/t/t.js", "nohang"]),
    ("jvm21", "java", ["java", "/t/Test.java", "nohang"]),
]


def run_nohang_suite(label, area, svc, cmd, env=None, expect_hang=False):
    rc, out, err = ex(svc, cmd, timeout=180, env=env)
    rows = jlines(out)
    if not rows:
        record(f"NH-{label}", area, "suite produced no output", "results", f"rc={rc} err={err[-200:]}", False)
        return
    for r in rows:
        st = r["status"]
        if st == "SKIP":
            ok = "SKIP"
        elif expect_hang:
            ok = True if st in ("HANG", "SLOW") else ("N/A" if st == "OK" else False)
        else:
            ok = st == "OK"
        exp = "HANG/SLOW (unfixed)" if expect_hang else "OK (0.9–2.5 s)"
        record(f"NH-{label}", area, r["test"], exp, f"{st} {r['elapsed_ms']} ms", ok)


def sec_nohang_quick():
    """Regression check: all runtimes, deadline fix on, +30d only."""
    ctl("set-offset", 30 * DAY)
    for area, svc, cmd in NOHANG_SUITES:
        run_nohang_suite("30d-recheck", area, svc, cmd)


def sec_nohang():
    for label, setup in [("1d", lambda: ctl("set-offset", 1 * DAY)),
                         ("30d", lambda: ctl("set-offset", 30 * DAY)),
                         ("365d", lambda: ctl("set-offset", 365 * DAY)),
                         ("365d-frozen", lambda: ctl("freeze"))]:
        setup()
        for area, svc, cmd in NOHANG_SUITES:
            run_nohang_suite(label, area, svc, cmd)
    ctl("unfreeze")
    # Negative evidence: same suites with deadline translation disabled.
    ctl("set-offset", 1 * DAY)
    for area, svc, cmd in NOHANG_SUITES:
        run_nohang_suite("1d-UNFIXED", area, svc, cmd, env={"CHRONOSTACK_DEADLINE_FIX": "0"}, expect_hang=True)


def sec_timers():
    """NEG-3: advancing wall time does not fire pending setTimeout early."""
    ctl("set-offset", 1 * DAY)
    p = subprocess.Popen(DC + ["exec", "-T", "node", "node", "/t/t.js", "timer"], stdout=subprocess.PIPE, text=True)
    time.sleep(1.5)
    ctl("advance", DAY)
    out, _ = p.communicate(timeout=30)
    rows = jlines(out)
    fired = next((r for r in rows if "fired_after_ms" in r), None)
    armed = next((r for r in rows if "armed_at_virtual" in r), None)
    ok = fired is not None and fired["fired_after_ms"] >= 4800
    record("NEG-3", "node22-slim", "setTimeout(5s) armed, then +1d advance", "fires after ~5 s real (NOT early)",
           f"fired after {fired and fired['fired_after_ms']} ms; virtual jump during wait "
           f"{(fired['fired_at_virtual'] - armed['armed_at_virtual']) / 3600:.1f} h" if fired and armed else out, ok)


def sec_failopen():
    real = clock()["real_ns"] / 1e9
    rc, out, err = ex("toolsg", ["date", "+%s"], env={"CHRONOSTACK_CLOCK_PATH": "/nonexistent"})
    ok = rc == 0 and abs(num(out) - real) <= TOL + 1 and "using real time" in err
    record("NEG-6", "c/glibc", "missing page, default mode", "real time + one stderr warning", f"rc={rc} stderr={err!r}", ok)
    rc, out, err = ex("toolsg", ["date", "+%s"], env={"CHRONOSTACK_CLOCK_PATH": "/nonexistent", "CHRONOSTACK_STRICT": "1"})
    ok = rc != 0 and "aborting" in err
    record("NEG-7", "c/glibc", "missing page, CHRONOSTACK_STRICT=1", "process aborts", f"rc={rc} stderr={err!r}", ok)
    rc, out, err = ex("toolsg", ["sh", "-c", "head -c 4096 /dev/zero > /tmp/bad && CHRONOSTACK_CLOCK_PATH=/tmp/bad date +%s"])
    ok = rc == 0 and "invalid clock page" in err
    record("NEG-6b", "c/glibc", "corrupt page (bad magic)", "real time + warning", f"rc={rc} stderr={err!r}", ok)


def sec_stress():
    ctl("set-offset", 1 * DAY)
    p = subprocess.Popen(DC + ["exec", "-T", "toolsg", "cstest", "stress", "6"], stdout=subprocess.PIPE, text=True)
    time.sleep(0.5)
    rc, out, err = ex("clockd", ["chronoctl", PAGE, "churn", "4"], timeout=60)
    sout, _ = p.communicate(timeout=60)
    s = jlines(sout)[0]
    it = re.search(r"churn iterations: (\d+)", err)
    record("CC-2", "c/glibc", "4 readers vs writer churning freeze/advance/unfreeze across containers",
           "0 backward steps", f"{s['reads']} reads, {s['backwards']} backward (worst {s['worst_back_ns']} ns), "
           f"{it.group(1) if it else '?'} churn iterations", s["backwards"] == 0)
    ctl("freeze")
    a = jlines(ex("toolsg", ["cstest", "now"])[1])[0]["clock_gettime_realtime"]
    time.sleep(1.5)
    b = jlines(ex("toolsg", ["cstest", "now"])[1])[0]["clock_gettime_realtime"]
    ctl("unfreeze")
    c = jlines(ex("toolsg", ["cstest", "now"])[1])[0]["clock_gettime_realtime"]
    record("CC-3", "c/glibc", "freeze: two reads 1.5 s apart; unfreeze resumes", "identical while frozen; no jump back",
           f"frozen delta {b - a:.6f}s; after unfreeze {c - b:+.3f}s", b == a and c >= b)


def sec_bench():
    n = 20_000_000
    _, base, _ = ex("toolsg", ["cstest", "bench", str(n)], env={"LD_PRELOAD": ""})
    _, shim, _ = ex("toolsg", ["cstest", "bench", str(n)])
    b, s = jlines(base)[0]["ns_per_call"], jlines(shim)[0]["ns_per_call"]
    record("PERF-1", "c/glibc", "clock_gettime(REALTIME) cost, shim vs none", "< +50 ns/call",
           f"baseline {b:.1f} ns, shim {s:.1f} ns, overhead {s - b:+.1f} ns", (s - b) < 50)
    _, base, _ = ex("toolsm", ["cstest", "bench", str(n)], env={"LD_PRELOAD": ""})
    _, shim, _ = ex("toolsm", ["cstest", "bench", str(n)])
    b, s = jlines(base)[0]["ns_per_call"], jlines(shim)[0]["ns_per_call"]
    record("PERF-1-MUSL", "c/musl", "clock_gettime(REALTIME) cost, shim vs none", "< +50 ns/call",
           f"baseline {b:.1f} ns, shim {s:.1f} ns, overhead {s - b:+.1f} ns", (s - b) < 50)


def sec_redis():
    for svc, area in [("redis", "redis7"), ("redisa", "redis7-alp")]:
        rcli(svc, "FLUSHALL")
        rcli(svc, "SET", "sess", "v", "EX", DAY)
        before = rcli(svc, "GET", "sess")[1]
        ctl("advance", 25 * 3600)
        after = rcli(svc, "GET", "sess")[1]
        record(f"RD-1-{svc}", area, "SET EX 86400 → advance 25h → GET", "value before, nil after",
               f"before={before!r} after={after!r}", before == "v" and after == "")
        rcli(svc, "SET", "k2", "v", "EX", 7200)
        ctl("advance", 3600)
        ttl = num(rcli(svc, "TTL", "k2")[1])
        record(f"RD-2-{svc}", area, "EX 7200 → advance 1h → TTL", "3600 ± 3", f"TTL={ttl:.0f}", abs(ttl - 3600) <= 3)
        v = clock()["virtual_ns"] // 1_000_000_000
        rcli(svc, "SET", "k3", "v")
        rcli(svc, "EXPIREAT", "k3", v + 100)
        ttl = num(rcli(svc, "TTL", "k3")[1])
        record(f"RD-3-{svc}", area, "EXPIREAT virtual_now+100 → TTL", "100 ± 3", f"TTL={ttl:.0f}", abs(ttl - 100) <= 3)
        rcli(svc, "EVAL", "for i=1,10000 do redis.call('SET','ae:'..i,'v','EX',60) end return 1", 0)
        n0 = num(rcli(svc, "DBSIZE")[1])
        ctl("advance", 120)
        t0 = time.monotonic()
        n = n0
        while time.monotonic() - t0 < 15:
            n = num(rcli(svc, "DBSIZE")[1])
            if n <= 1:  # k3 may remain
                break
            time.sleep(0.5)
        record(f"RD-5-{svc}", area, "10k keys EX 60 → advance 2m → DBSIZE without access (active expiry)",
               "≈0 within 15 s", f"{n0:.0f} → {n:.0f} after {time.monotonic() - t0:.1f}s", n <= 1)
        t0 = time.monotonic()
        rc, out, err = rcli(svc, "BLPOP", "nokey", 1, timeout=20)
        el = time.monotonic() - t0
        record(f"NH-RD-{svc}", area, "BLPOP 1s under offset", "returns after ~1 s (+exec overhead)",
               f"{el:.2f}s wall incl. docker exec", 0.9 <= el <= 4)


def sec_postgres():
    for svc, area in [("pg", "postgres16"), ("pga", "postgres16-alp")]:
        psql(svc, "drop table if exists t; create table t(id int, created_at timestamptz default now())")
        psql(svc, "insert into t(id) values (1)")
        ctl("advance", 15 * DAY)
        rc, out, err = psql(svc, "select count(*) from t where created_at < now() - interval '14 days'")
        record(f"PG-4-{svc}", area, "insert → advance 15d → created_at < now()-14d", "1 row", f"{out or err}", out == "1")
        t0 = time.monotonic()
        rc, out, err = psql(svc, "set statement_timeout='5s'; select pg_sleep(1)")
        el = time.monotonic() - t0
        record(f"PG-5-{svc}", area, "statement_timeout=5s; pg_sleep(1) under offset", "succeeds in ~1 s",
               f"rc={rc} {el:.2f}s", rc == 0 and el < 4)
        t0 = time.monotonic()
        rc, out, err = psql(svc, "set statement_timeout='1s'; select pg_sleep(3)")
        el = time.monotonic() - t0
        record(f"PG-5b-{svc}", area, "statement_timeout=1s; pg_sleep(3) under offset", "cancelled after ~1 s",
               f"rc={rc} {el:.2f}s {err[-60:]}", rc != 0 and "statement timeout" in err and el < 3.5)
        # PG-2 / PG-6: advance during an open transaction / during pg_sleep
        sql = ("begin; select extract(epoch from now()); select pg_sleep(4); "
               "select extract(epoch from now()), extract(epoch from clock_timestamp()); commit;")
        t0 = time.monotonic()
        p = subprocess.Popen(DC + ["exec", "-T", svc, "psql", "-U", "postgres", "-Atq", "-c", sql],
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        time.sleep(2.0)
        ctl("advance", DAY)
        out, err = p.communicate(timeout=60)
        el = time.monotonic() - t0
        nums = [float(x) for x in re.findall(r"\d+\.\d+", out)]
        if len(nums) >= 3:
            now1, now2, clk = nums[0], nums[1], nums[2]
            record(f"PG-2-{svc}", area, "now() inside txn spanning an advance", "unchanged (txn-start cached)",
                   f"delta {now2 - now1:.3f}s", abs(now2 - now1) < 0.001)
            record(f"PG-3-{svc}", area, "clock_timestamp() inside same txn after +1d", "≈ +1d",
                   f"clock - now = {(clk - now1) / 3600:.2f} h", clk - now1 > DAY - 60)
        else:
            record(f"PG-2-{svc}", area, "now() inside txn spanning an advance", "unchanged", f"out={out!r} err={err!r}", False)
        record(f"PG-6-{svc}", area, "pg_sleep(4) with +1d advance after 2 s", "OBSERVATION (pg_sleep end time is wall-clock based?)",
               f"statement wall time {el:.2f}s (≈2 s ⇒ ended early at advance; ≈4 s ⇒ unaffected)", "INFO")
    rc, _, err = run(DC + ["restart", "pg"], timeout=120)
    wait_ready()
    check_value("PG-11", "postgres16", "restart container under shim, clock_timestamp()",
                lambda: num(psql("pg", "select extract(epoch from clock_timestamp())")[1]))


def pgcron_wait_hb(prev, timeout=8):
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        rc, out, _ = psql("pg", "select count(*) from hb")
        if rc == 0 and int(out) > prev:
            return int(out), time.monotonic() - t0
        time.sleep(0.25)
    return prev, None


def daily_count():
    return int(psql("pg", "select count(*) from daily")[1])


def sec_pgcron():
    psql("pg", "create extension if not exists pg_cron")
    psql("pg", "select cron.unschedule(jobid) from cron.job")
    psql("pg", "drop table if exists hb; drop table if exists daily; "
               "create table hb(t timestamptz default clock_timestamp()); "
               "create table daily(t timestamptz default clock_timestamp())")
    psql("pg", "select cron.schedule('hb', '* * * * *', 'insert into hb default values')")
    time.sleep(2)

    # PG-7: 1-minute steps with heartbeat barrier
    hb = int(psql("pg", "select count(*) from hb")[1])
    misses = 0
    waits = []
    for _ in range(20):
        ctl("advance", 60)
        hb2, w = pgcron_wait_hb(hb)
        if w is None:
            misses += 1
        else:
            waits.append(w)
        hb = hb2
    record("PG-7", "pg_cron", "`* * * * *` job, 20 × 1-min steps, barrier = new run", "20/20 steps produce a run",
           f"{20 - misses}/20; barrier wait avg {sum(waits) / max(len(waits), 1):.2f}s max {max(waits or [0]):.2f}s", misses == 0)

    # Position virtual time at 02:30:00 UTC on a future day, then schedule a daily 03:00 job.
    v = clock()["virtual_ns"] // 1_000_000_000
    target = (v // DAY + 2) * DAY + 2 * 3600 + 30 * 60
    ctl("set-time", target)
    hb, _ = pgcron_wait_hb(int(psql("pg", "select count(*) from hb")[1]))
    psql("pg", "select cron.schedule('daily', '0 3 * * *', 'insert into daily default values')")
    time.sleep(2)

    def run_steps(test_id, step_s, total_s, expected, note):
        nonlocal hb
        psql("pg", "truncate daily")
        steps = int(total_s // step_s)
        missing = 0
        for _ in range(steps):
            ctl("advance", step_s)
            hb2, w = pgcron_wait_hb(hb)
            missing += w is None
            hb = hb2
        time.sleep(3)
        got = daily_count()
        record(test_id, "pg_cron", f"daily 03:00 job; {total_s / DAY:.0f}d in {step_s / 3600:g}h steps ({note})",
               f"{expected} runs", f"{got} runs ({missing} steps without heartbeat)", got == expected)

    run_steps("PG-8", 14 * DAY, 14 * DAY, 0, "single raw jump > 3h → CLOCK_CHANGE")
    run_steps("PG-9", 3600, 3 * DAY, 3, "1h sub-steps → CLOCK_JUMP_FORWARD catch-up")
    run_steps("PG-9b", 4 * 3600, 3 * DAY, 0, "4h steps > 3h → CLOCK_CHANGE skips")
    run_steps("PG-9c", 3 * 3600, 3 * DAY, 3, "3h steps = boundary, not > 180 min")
    psql("pg", "select cron.unschedule(jobid) from cron.job")


def sec_arm64():
    """Build-and-run check of the aarch64 shims under QEMU emulation (single container, local page)."""
    for target, lib in [("tools-gnu", "libchronoclock-gnu.so"), ("tools-musl", "libchronoclock-musl.so")]:
        tag = f"chronostack-m0/{target}:arm64"
        rc, out, err = run(["docker", "buildx", "build", "--platform", "linux/arm64", "--target", target,
                            "-f", os.path.join(HERE, "docker", "Dockerfile"), "-t", tag, "--load", HERE], timeout=1800)
        if rc != 0:
            record(f"ARM64-{target}", "aarch64", "build under emulation", "builds", f"build failed: {err[-300:]}", False)
            continue
        ctl_bin = "/opt/chronostack/chronoctl-musl" if target == "tools-gnu" else "chronoctl"
        script = (f"{ctl_bin} /tmp/c init >/dev/null && {ctl_bin} /tmp/c set-offset {DAY} >/dev/null && "
                  f"uname -m && LD_PRELOAD=/opt/chronostack/lib/{lib} CHRONOSTACK_CLOCK_PATH=/tmp/c cstest now && "
                  f"date +%s && LD_PRELOAD=/opt/chronostack/lib/{lib} CHRONOSTACK_CLOCK_PATH=/tmp/c cstest nohang")
        rc, out, err = run(["docker", "run", "--rm", "--platform", "linux/arm64", tag, "sh", "-c", script], timeout=300)
        lines = out.splitlines()
        arch = lines[0] if lines else "?"
        rows = jlines(out)
        if not rows:
            record(f"ARM64-{target}", "aarch64", "run under emulation", "results", f"rc={rc} err={err[-300:]}", False)
            continue
        now = rows[0]
        real = num(lines[2]) if len(lines) > 2 else 0
        skew = now["clock_gettime_realtime"] - real - DAY
        record(f"ARM64-{target}-read", f"aarch64/{target[6:]}", f"clock_gettime at +1d (uname -m={arch})",
               "virtual ±2s", f"skew {skew:+.2f}s", abs(skew) <= TOL + 1)
        for r in rows[1:]:
            ok = "SKIP" if r["status"] == "SKIP" else r["status"] == "OK"
            record(f"ARM64-{target}-NH", f"aarch64/{target[6:]}", r["test"], "OK", f"{r['status']} {r['elapsed_ms']} ms", ok)


SECTIONS = {
    "baseline": sec_baseline, "reads": sec_reads, "nohang": sec_nohang, "nohang_quick": sec_nohang_quick, "timers": sec_timers,
    "failopen": sec_failopen, "stress": sec_stress, "bench": sec_bench, "redis": sec_redis,
    "postgres": sec_postgres, "pgcron": sec_pgcron, "arm64": sec_arm64,
}


def write_results(env_info):
    outdir = os.path.join(HERE, "results")
    os.makedirs(outdir, exist_ok=True)
    with open(os.path.join(outdir, "m0-results.json"), "w", encoding="utf-8") as f:
        json.dump({"environment": env_info, "results": RESULTS}, f, indent=2)
    counts = {}
    for r in RESULTS:
        counts[r["status"]] = counts.get(r["status"], 0) + 1
    with open(os.path.join(outdir, "m0-results.md"), "w", encoding="utf-8") as f:
        f.write("# M0 raw results\n\nGenerated by `spike/run_m0.py`.\n\n")
        f.write("Environment: " + ", ".join(f"{k}={v}" for k, v in env_info.items()) + "\n\n")
        f.write("Totals: " + ", ".join(f"{k}: {v}" for k, v in sorted(counts.items())) + "\n\n")
        f.write("| ID | Area | Test | Expected | Observed | Status |\n|---|---|---|---|---|---|\n")
        for r in RESULTS:
            cells = [str(r[k]).replace("|", "\\|") for k in ("id", "area", "description", "expected", "observed", "status")]
            f.write("| " + " | ".join(cells) + " |\n")
    print("\nTotals:", counts)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default=",".join(k for k in SECTIONS if k != "arm64") + ",arm64")
    ap.add_argument("--keep", action="store_true", help="do not tear down the stack")
    args = ap.parse_args()
    _, dv, _ = run(["docker", "info", "--format", "{{.ServerVersion}} {{.OperatingSystem}} {{.Architecture}} kernel={{.KernelVersion}}"])
    _, cv, _ = run(["docker", "compose", "version", "--short"])
    env_info = {"docker": dv, "compose": cv, "host": sys.platform, "started": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    print("Environment:", env_info)
    rc, out, err = run(DC + ["up", "-d", "--wait"], timeout=600)
    if rc != 0:
        print(out, err)
        sys.exit(1)
    try:
        wait_ready()
        for name in args.only.split(","):
            print(f"\n=== {name} ===", flush=True)
            try:
                SECTIONS[name]()
            except Exception as e:  # noqa: BLE001
                record(f"SECTION-{name}", name, "section crashed", "completes", repr(e)[:300], False)
    finally:
        write_results(env_info)
        if not args.keep:
            run(DC + ["down", "-v"], timeout=300)


if __name__ == "__main__":
    main()
