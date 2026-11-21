# SPDX-License-Identifier: MIT OR Apache-2.0
"""ChronoStack benchmarks B1, B2 and B3 (see bench/README.md for the method).

B1  clock-read overhead: ns per wall-clock read in C, Python and Node, natively,
    through ChronoStack's preload library, and through libfaketime (cached, and
    live-controlled through a timestamp file with FAKETIME_NO_CACHE=1). All
    configurations run in the same container; only the preload changes.
B2  advance latency: real time `chronostack advance 1h` takes on an idle stack
    of 1, 5 and 20 virtualized services, with and without the probe step.
B3  simulated users: `chronostack users run` with 200 users making 20 GET
    requests each against traefik/whoami on loopback, at max_parallel 32 and
    200: requests per second and request latency as the engine measures them.

Every configuration runs --reps times (default 10); raw samples, summary
statistics and the environment are written to bench/results/<date>-<label>/.

Usage: python bench/run_bench.py [--cli PATH] [--reps N] [--only b1|b2|b3] [--label NAME]
"""
import argparse
import datetime as dt
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BENCH = os.path.join(ROOT, "bench")
CLOCK_DIR = os.path.join(BENCH, "clock")
ADVANCE_DIR = os.path.join(BENCH, "advance")

CHRONO_OFFSET_S = 2 * 86400  # ChronoStack's clock is moved +2 d ...
FAKETIME_OFFSET_S = 1 * 86400  # ... and libfaketime's +1 d, so each run's reading shows which clock it saw.
TOLERANCE_S = 30

RUNTIMES = {
    # name: (service, command prefix, iterations normally, iterations when every call reads a file)
    "c": ("c", ["clockbench"], 10_000_000, 200_000),
    "python": ("python", ["python", "/b/clockbench.py"], 2_000_000, 100_000),
    "node": ("node", ["node", "/b/clockbench.js"], 10_000_000, 200_000),
}
CONFIGS = ["native", "chronostack", "libfaketime", "libfaketime-nocache-file"]
ADVANCE_SIZES = [1, 5, 20]
USERS = 200
REQUESTS_PER_USER = 20
USERS_PARALLEL = [32, 200]


def sh(args, cwd, timeout=1800):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout,
                       encoding="utf-8", errors="replace")
    return p.returncode, p.stdout, p.stderr


def last_json(out):
    for line in reversed(out.strip().splitlines()):
        line = line.strip()
        if line.startswith("{"):
            return json.loads(line)
    raise ValueError(f"no JSON in output: {out[-300:]!r}")


def pct(xs, q):
    xs = sorted(xs)
    if len(xs) == 1:
        return xs[0]
    k = (len(xs) - 1) * q
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def summarize(xs):
    return {"n": len(xs), "median": statistics.median(xs), "p10": pct(xs, 0.10), "p90": pct(xs, 0.90),
            "min": min(xs), "max": max(xs)}


class Stack:
    """A ChronoStack session for one compose project directory."""

    def __init__(self, cli, cwd):
        self.cli, self.cwd = cli, cwd

    def cs(self, *args, timeout=1800):
        return sh([self.cli, *args], self.cwd, timeout)

    def session(self):
        with open(os.path.join(self.cwd, ".chronostack", "session.json"), encoding="utf-8") as f:
            return json.load(f)

    def exec(self, svc, cmd, env=None, timeout=900):
        s = self.session()
        args = ["docker", "compose"]
        for f in s["compose_files"]:
            args += ["-f", f]
        args += ["-p", s["project"], "exec", "-T"]
        for k, v in (env or {}).items():
            args += ["-e", f"{k}={v}"]
        return sh(args + [svc, *cmd], self.cwd, timeout)

    def up(self):
        if os.path.exists(os.path.join(self.cwd, ".chronostack", "session.json")):
            self.cs("down", "-v")
        rc, out, err = self.cs("up", "--wait", "--wait-timeout", "300s", timeout=3600)
        if rc:
            sys.exit(f"`chronostack up` failed in {self.cwd}:\n{(err or out)[-2000:]}")

    def down(self):
        self.cs("down", "-v")

    def virtual_now(self):
        return last_json(self.cs("now", "--json")[1])["virtual_ns"] / 1e9


def environment(cli, stack=None):
    env = {
        "date_utc": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "host": {"os": platform.platform(), "machine": platform.machine(), "python": platform.python_version()},
        "chronostack": sh([cli, "--version"], ROOT)[1].strip(),
        "git_commit": sh(["git", "rev-parse", "HEAD"], ROOT)[1].strip(),
        "docker": sh(["docker", "version", "--format", "{{.Server.Version}} ({{.Server.Os}}/{{.Server.Arch}})"], ROOT)[1].strip(),
        "docker_context": sh(["docker", "context", "show"], ROOT)[1].strip(),
    }
    if stack:
        # One `key=value` line per fact, so a fact the CPU doesn't report (ARM has
        # no "model name" in /proc/cpuinfo) can't shift or drop the others.
        script = (
            "echo cpus=$(nproc); "
            "echo cpu_model=$(grep -m1 -E '^(model name|Model|CPU part)' /proc/cpuinfo | cut -d: -f2); "
            "echo kernel=$(uname -r); "
            "echo mem_kib=$(awk '/MemTotal/ {print $2}' /proc/meminfo); "
            "echo libfaketime_deb=$(dpkg-query -W -f '${Version}' libfaketime)"
        )
        rc, out, _ = stack.exec("c", ["sh", "-c", script], env={"LD_PRELOAD": ""})
        facts = dict(l.split("=", 1) for l in out.splitlines() if "=" in l)
        env["linux"] = {
            "cpus": int(facts["cpus"]) if facts.get("cpus", "").isdigit() else None,
            "cpu_model": facts.get("cpu_model", "").strip() or "(not reported)",
            "kernel": facts.get("kernel", "").strip() or "?",
            "mem_kib": int(facts["mem_kib"]) if facts.get("mem_kib", "").isdigit() else None,
            "libfaketime_deb": facts.get("libfaketime_deb", "").strip() or "?",
        }
    return env


def b1(cli, reps):
    st = Stack(cli, CLOCK_DIR)
    print("B1: building and starting bench/clock ...", flush=True)
    st.up()
    try:
        rc, out, err = st.cs("advance", f"{CHRONO_OFFSET_S}s", "--no-probe")
        if rc:
            sys.exit(f"advance failed: {(err or out)[-500:]}")
        lft = {}
        for name, (svc, _, _, _) in RUNTIMES.items():
            rc, out, _ = st.exec(svc, ["sh", "-c", "ls /usr/lib/*/faketime/libfaketime.so.1; echo +1d > /tmp/faketime"],
                                 env={"LD_PRELOAD": ""})
            lft[name] = out.split()[0] if rc == 0 and out.strip() else sys.exit(f"{svc}: libfaketime not found")
        env = environment(cli, st)
        samples = {r: {c: [] for c in CONFIGS} for r in RUNTIMES}
        checks = []
        for rep in range(reps):
            # Rotate the order every repetition so drift (thermal, noisy neighbours) spreads evenly.
            order = CONFIGS[rep % len(CONFIGS):] + CONFIGS[:rep % len(CONFIGS)]
            for name, (svc, cmd, n_fast, n_slow) in RUNTIMES.items():
                for cfg in order:
                    if cfg == "native":
                        e, n, expect = {"LD_PRELOAD": ""}, n_fast, 0
                    elif cfg == "chronostack":
                        e, n, expect = None, n_fast, CHRONO_OFFSET_S
                    elif cfg == "libfaketime":
                        e, n, expect = {"LD_PRELOAD": lft[name], "FAKETIME": "+1d"}, n_fast, FAKETIME_OFFSET_S
                    else:
                        e = {"LD_PRELOAD": lft[name], "FAKETIME_TIMESTAMP_FILE": "/tmp/faketime", "FAKETIME_NO_CACHE": "1"}
                        n, expect = n_slow, FAKETIME_OFFSET_S
                    real_before = time.time()
                    rc, out, err = st.exec(svc, [*cmd, str(n)], env=e)
                    if rc:
                        sys.exit(f"B1 {name}/{cfg} failed (rc={rc}): {(err or out)[-500:]}")
                    r = last_json(out)
                    off = r["wall_clock"] - real_before
                    ok = abs(off - expect) <= TOLERANCE_S
                    checks.append({"runtime": name, "config": cfg, "rep": rep, "offset_s": round(off, 1),
                                   "expected_offset_s": expect, "ok": ok})
                    if not ok:
                        sys.exit(f"B1 {name}/{cfg}: read {off:+.0f}s from real time, expected {expect:+d}s; "
                                 "the configuration did not take effect")
                    samples[name][cfg].append(r["ns_per_call"])
            print(f"  rep {rep + 1}/{reps} done", flush=True)
    finally:
        st.down()
    result = {"bench": "B1 clock-read overhead", "unit": "ns per wall-clock read", "reps": reps,
              "iterations": {k: {"default": v[2], "libfaketime-nocache-file": v[3]} for k, v in RUNTIMES.items()},
              "environment": env, "samples": samples,
              "summary": {r: {c: summarize(v) for c, v in cs.items()} for r, cs in samples.items()},
              "clock_checks_passed": all(c["ok"] for c in checks), "clock_checks": checks}
    return result


def b2(cli, reps):
    results, env = {}, None
    for n in ADVANCE_SIZES:
        work = os.path.join(ADVANCE_DIR, f"work-{n}")
        shutil.rmtree(work, ignore_errors=True)
        os.makedirs(work)
        with open(os.path.join(work, "compose.yml"), "w", encoding="utf-8") as f:
            f.write(f"# generated by bench/run_bench.py: {n} idle virtualized services\nname: bench-advance-{n}\nservices:\n")
            for i in range(1, n + 1):
                f.write(f"  s{i}:\n    image: debian:bookworm-slim\n    command: [sleep, infinity]\n")
        st = Stack(cli, work)
        print(f"B2: {n} service(s) ...", flush=True)
        st.up()
        try:
            env = env or environment(cli)
            st.cs("advance", "1h")  # warm-up: first exec into each container is slower
            per = {}
            for label, extra in (("with_probe", []), ("no_probe", ["--no-probe"])):
                xs = []
                for _ in range(reps):
                    before = st.virtual_now()
                    t0 = time.perf_counter()
                    rc, out, err = st.cs("advance", "1h", *extra)
                    elapsed = time.perf_counter() - t0
                    if rc:
                        sys.exit(f"B2 advance failed ({n} services, {label}): {(err or out)[-500:]}")
                    moved = st.virtual_now() - before
                    if not 3600 <= moved < 3600 + 120:
                        sys.exit(f"B2: virtual time moved {moved:.0f}s, expected about 3600s")
                    xs.append(elapsed * 1000)
                per[label] = {"samples_ms": xs, "summary": summarize(xs)}
            results[str(n)] = per
        finally:
            st.down()
            shutil.rmtree(work, ignore_errors=True)
    return {"bench": "B2 advance latency", "unit": "ms of real time for `chronostack advance 1h`",
            "reps": reps, "environment": env, "services": results}


def b3(cli, reps):
    rc, out, err = sh(["docker", "run", "-d", "--rm", "-p", "127.0.0.1::80", "traefik/whoami"], BENCH, 600)
    if rc:
        sys.exit(f"B3: could not start traefik/whoami: {(err or out)[-500:]}")
    cid = out.strip()
    try:
        port = sh(["docker", "port", cid, "80"], BENCH)[1].strip().splitlines()[0].rsplit(":", 1)[1]
        url = f"http://127.0.0.1:{port}"
        for _ in range(50):  # wait for whoami to answer
            if sh(["curl", "-fsS", "-o", os.devnull, url], BENCH, 10)[0] == 0:
                break
            time.sleep(0.2)
        env = environment(cli)
        results = {}
        for par in USERS_PARALLEL:
            path = os.path.join(BENCH, f"b3-users-{par}.yml")
            with open(path, "w", encoding="utf-8") as f:
                f.write("# generated by bench/run_bench.py (B3)\nversion: 1\nseed: 1\n"
                        f"max_parallel: {par}\ntargets: {{ api: \"{url}\" }}\n"
                        f"cohorts: [{{ name: bench, users: {USERS}, journey: j }}]\n"
                        f"journeys:\n  j:\n    - repeat: {{ times: {REQUESTS_PER_USER}, steps: "
                        "[{ name: get, http: { url: \"${api}/bench?u=${user}\" } }] }\n")
            print(f"B3: {USERS} users x {REQUESTS_PER_USER} requests, max_parallel {par} ...", flush=True)
            sh([cli, "users", "run", path, "--json"], BENCH, 600)  # warm-up
            rps, p50, p99 = [], [], []
            try:
                for _ in range(reps):
                    rc, out, err = sh([cli, "users", "run", path, "--json"], BENCH, 600)
                    r = last_json(out)
                    get = next(st for st in r["steps"] if st["name"] == "get")
                    want = USERS * REQUESTS_PER_USER
                    if rc or r["users"]["failed"] or get["runs"] != want or get["failed"]:
                        sys.exit(f"B3: run failed or incomplete (max_parallel {par}): {out[-800:]} {err[-300:]}")
                    rps.append(want / (r["elapsed_ms"] / 1000))
                    p50.append(get["latency_us"]["p50"] / 1000)
                    p99.append(get["latency_us"]["p99"] / 1000)
            finally:
                os.remove(path)
            results[str(par)] = {
                "requests_per_s": {"samples": rps, "summary": summarize(rps)},
                "latency_p50_ms": {"samples": p50, "summary": summarize(p50)},
                "latency_p99_ms": {"samples": p99, "summary": summarize(p99)},
            }
    finally:
        sh(["docker", "stop", cid], BENCH, 120)
    return {"bench": "B3 simulated users", "users": USERS, "requests_per_user": REQUESTS_PER_USER,
            "server": "traefik/whoami on 127.0.0.1", "reps": reps, "environment": env,
            "max_parallel": results}


def fmt(x):
    return f"{x:,.1f}" if x < 1000 else f"{x:,.0f}"


def markdown(b1r, b2r, label, b3r=None):
    m = [f"# Benchmark results: {label}", ""]
    env = (b1r or b2r or b3r)["environment"]
    lx = env.get("linux", {})
    m += ["| | |", "|---|---|",
          f"| Date (UTC) | {env['date_utc']} |",
          f"| Host | {env['host']['os']} ({env['host']['machine']}) |",
          f"| Docker | {env['docker']} · context `{env['docker_context']}` |",
          f"| Linux (containers) | {lx.get('kernel', '?')} · {lx.get('cpus', '?')} CPUs · {lx.get('cpu_model', '?')} |",
          f"| ChronoStack | {env['chronostack']} · commit `{env['git_commit'][:12]}` |",
          *([f"| libfaketime | Debian package {lx.get('libfaketime_deb', '?')} |"] if b1r else []), ""]
    if b1r:
        m += ["## B1: clock-read overhead (ns per read)", "",
              f"Median of {b1r['reps']} runs; p10–p90 in brackets. Overhead = median minus the native median "
              "in the same container. Every run's clock reading was checked against the expected offset "
              f"({'all passed' if b1r['clock_checks_passed'] else 'SOME FAILED'}).", "",
              "| Runtime | Native | ChronoStack | libfaketime (cached) | libfaketime (file, no cache) |",
              "|---|---|---|---|---|"]
        for r, cs in b1r["summary"].items():
            base = cs["native"]["median"]
            cells = []
            for c in CONFIGS:
                s = cs[c]
                cell = f"{fmt(s['median'])} [{fmt(s['p10'])}–{fmt(s['p90'])}]"
                if c != "native":
                    cell += f" (+{fmt(s['median'] - base)})"
                cells.append(cell)
            m.append(f"| {r} | " + " | ".join(cells) + " |")
        m += ["", "Python's figure is the time.time() call with an empty loop of the same length subtracted; "
              "Node's includes the loop.", ""]
    if b2r:
        m += ["## B2: `chronostack advance 1h` (ms of real time)", "",
              f"Median of {b2r['reps']} runs; p10–p90 in brackets. Idle `debian:bookworm-slim` services, no "
              "triggers or barriers. With probes, every service's clock is checked after the move.", "",
              "| Services | With probes | Without probes (`--no-probe`) |", "|---|---|---|"]
        for n, per in b2r["services"].items():
            a, b = per["with_probe"]["summary"], per["no_probe"]["summary"]
            m.append(f"| {n} | {fmt(a['median'])} [{fmt(a['p10'])}–{fmt(a['p90'])}] | "
                     f"{fmt(b['median'])} [{fmt(b['p10'])}–{fmt(b['p90'])}] |")
        m.append("")
    if b3r:
        m += ["## B3: `chronostack users run` (simulated users)", "",
              f"{b3r['users']} users, {b3r['requests_per_user']} GET requests each, against "
              f"{b3r['server']}. Median of {b3r['reps']} runs; p10–p90 in brackets. Requests per second = "
              "all requests / the run's real time, including thread start-up. Latency is the engine's own "
              "per-request measurement (p50 and p99 of each run). This measures the engine on this machine, "
              "against a server that does almost nothing; it is not a load-testing result.", "",
              "| max_parallel | Requests/s | Latency p50 (ms) | Latency p99 (ms) |", "|---|---|---|---|"]
        for par, per in b3r["max_parallel"].items():
            cells = []
            for k in ("requests_per_s", "latency_p50_ms", "latency_p99_ms"):
                x = per[k]["summary"]
                cells.append(f"{fmt(x['median'])} [{fmt(x['p10'])}–{fmt(x['p90'])}]")
            m.append(f"| {par} | " + " | ".join(cells) + " |")
        m.append("")
    raw = [f for f, r in (("`b1-clock.json`", b1r), ("`b2-advance.json`", b2r), ("`b3-users.json`", b3r)) if r]
    m += [f"Raw samples: {', '.join(raw)} in this folder.", ""]
    return "\n".join(m)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    ap.add_argument("--reps", type=int, default=10)
    ap.add_argument("--only", choices=["b1", "b2", "b3"])
    ap.add_argument("--label", default=f"{platform.system().lower()}-{platform.machine().lower()}")
    a = ap.parse_args()
    # Benchmark the controller built from this checkout (release builds of the
    # CLI would otherwise default to the published image for their version).
    os.environ.setdefault("CHRONOSTACK_CLOCKD_IMAGE", "chronostack/clockd:dev")
    exe = "chronostack" + (".exe" if os.name == "nt" else "")
    cli = os.path.abspath(a.cli) if a.cli else os.path.join(ROOT, "target", "release", exe)
    if not os.path.exists(cli):
        sys.exit(f"CLI not found at {cli}; build it with `cargo build --release -p chronostack` or pass --cli")
    b1r = b1(cli, a.reps) if a.only in (None, "b1") else None
    b2r = b2(cli, a.reps) if a.only in (None, "b2") else None
    b3r = b3(cli, a.reps) if a.only in (None, "b3") else None
    out = os.path.join(BENCH, "results", f"{dt.date.today().isoformat()}-{a.label}")
    os.makedirs(out, exist_ok=True)
    if b1r:
        json.dump(b1r, open(os.path.join(out, "b1-clock.json"), "w", encoding="utf-8"), indent=1)
    if b2r:
        json.dump(b2r, open(os.path.join(out, "b2-advance.json"), "w", encoding="utf-8"), indent=1)
    if b3r:
        json.dump(b3r, open(os.path.join(out, "b3-users.json"), "w", encoding="utf-8"), indent=1)
    md = markdown(b1r, b2r, a.label, b3r)
    open(os.path.join(out, "README.md"), "w", encoding="utf-8").write(md)
    print("\n" + md)
    print(f"written to {out}")


if __name__ == "__main__":
    main()
