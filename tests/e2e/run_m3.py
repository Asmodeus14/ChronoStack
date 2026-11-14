# SPDX-License-Identifier: MIT OR Apache-2.0
"""M3 end-to-end: fault proxy, outage windows, scenarios and reports.

Uses examples/saas-trial-demo:
  1. proxy + `fault` commands: env rewrite, 503 during a window, forwarding
     outside it, windows survive a clockd restart, event log;
  2. `chronostack run scenarios/trial.yml` passes; the report shows the
     webhook blocked 3 times by the outage and delivered once (repeatable);
  3. with compose.bug.yml (the worker treats any response as delivered) the same
     scenario fails at the right step, with exit code 6 and a readable report.

Usage: python tests/e2e/run_m3.py [--cli PATH] [--repeat N]
"""
import argparse
import glob
import json
import os
import re
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
HTTPS = os.path.join(ROOT, "tests", "e2e", "https")
FAILS = []


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""), flush=True)
    if not ok:
        FAILS.append(name)


def sh(args, timeout=900):
    p = subprocess.run(args, cwd=DEMO, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout, p.stderr


class Demo:
    def __init__(self, cli):
        self.cli = cli

    def cs(self, *a, timeout=900):
        return sh([self.cli, *a], timeout)

    def js(self, *a):
        rc, out, err = self.cs(*a, "--json")
        try:
            return rc, json.loads(out.strip().splitlines()[-1])
        except (ValueError, IndexError):
            return rc, {"_raw": out, "_err": err}

    def compose(self, *a, timeout=120):
        s = json.load(open(os.path.join(DEMO, ".chronostack", "session.json"), encoding="utf-8"))
        args = ["docker", "compose"]
        for f in s["compose_files"]:
            args += ["-f", f]
        return sh(args + ["-p", s["project"], *a], timeout)

    def up(self, *files):
        if os.path.exists(os.path.join(DEMO, ".chronostack", "session.json")):
            self.cs("down", "-v")
        args = [x for f in files for x in ("-f", f)]
        rc, out, err = self.cs(*args, "up", "--wait", "--wait-timeout", "180s", timeout=1800)
        check(f"up --wait ({' + '.join(files) or 'compose.yml'})", rc == 0, (err or out)[-600:] if rc else "")
        return rc == 0, out

    def down(self):
        rc, out, err = self.cs("down", "-v")
        check("down -v", rc == 0, (err or out)[-300:] if rc else "")


POST = ("fetch(process.env.WEBHOOK_URL,{method:'POST',headers:{'content-type':'application/json'},"
        "body:JSON.stringify({id:process.argv[1],type:'probe'})})"
        ".then(async r=>console.log(r.status+' '+(await r.text()))).catch(e=>console.log('ERR '+e.message))")


def latest_report():
    files = sorted(glob.glob(os.path.join(DEMO, ".chronostack", "reports", "*.json")), key=os.path.getmtime)
    if not files:
        return None, ""
    md = files[-1][:-5] + ".md"
    return json.load(open(files[-1], encoding="utf-8")), open(md, encoding="utf-8").read() if os.path.exists(md) else ""


def proxy_checks(d, up_out):
    check("up: worker WEBHOOK_URL routed through the proxy",
          "WEBHOOK_URL = http://chronostack-clockd:7200/hooks" in up_out, up_out[-400:])
    rc, env, _ = d.compose("exec", "-T", "worker", "printenv", "WEBHOOK_URL")
    check("worker sees the rewritten WEBHOOK_URL", env.strip() == "http://chronostack-clockd:7200/hooks", env.strip())
    rc, l = d.js("fault", "list")
    check("fault list: target `webhooks` → receiver",
          [t["name"] for t in l.get("targets", [])] == ["webhooks"] and l["targets"][0]["upstream"] == "http://receiver:4000", json.dumps(l)[:300])

    _, out, _ = d.compose("exec", "-T", "worker", "node", "-e", POST, "probe-1")
    check("proxy forwards when no window is active", out.startswith("200 "), out.strip()[:200])

    rc, f = d.js("fault", "set", "webhooks", "--from", "now", "--for", "1h")
    check("fault set webhooks --from now --for 1h", rc == 0 and f.get("status") == 503, json.dumps(f)[:200])
    _, out, _ = d.compose("exec", "-T", "worker", "node", "-e", POST, "probe-2")
    check("proxy returns 503 inside the window", out.startswith("503 ") and "ChronoStack fault" in out, out.strip()[:200])

    rc, _, err = d.compose("restart", "chronostack-clockd", timeout=180)
    for _ in range(60):
        rc, l = d.js("fault", "list")
        if rc == 0:
            break
        time.sleep(1)
    check("outage window survives a clockd restart", rc == 0 and len(l.get("faults", [])) == 1, json.dumps(l)[:200])
    _, out, _ = d.compose("exec", "-T", "worker", "node", "-e", POST, "probe-3")
    check("proxy still blocks after the restart", out.startswith("503 "), out.strip()[:200])

    rc, _ = d.js("advance", "2h", "--no-probe")
    _, out, _ = d.compose("exec", "-T", "worker", "node", "-e", POST, "probe-4")
    check("window over after advance 2h: forwarded again", out.startswith("200 "), out.strip()[:200])

    rc, _, _ = d.cs("fault", "clear", "--all")
    rc, l = d.js("fault", "list")
    check("fault clear --all", rc == 0 and l.get("faults") == [], json.dumps(l)[:200])
    _, got, _ = d.compose("exec", "-T", "cache", "redis-cli", "HGET", "webhook:received", "probe-4")
    check("receiver got the forwarded request body", got.strip() == "1", got.strip())
    rc, ev = d.js("events")
    proxy = [e for e in ev.get("events", []) if e["kind"] == "proxy"]
    check("event log survives the clockd restart (all 4 proxied requests, in order)",
          [e["outcome"] for e in proxy] == ["forwarded", "fault", "fault", "forwarded"], json.dumps(proxy)[:400])


def scenario_pass(d, n):
    t0 = time.monotonic()
    rc, out, err = d.cs("run", "scenarios/trial.yml", timeout=1800)
    took = time.monotonic() - t0
    rep, md = latest_report()
    check(f"run {n}: scenario passed (exit 0)", rc == 0 and rep and rep["result"] == "passed",
          f"rc={rc} {(rep or {}).get('failed_step')} {(err or out)[-400:] if rc else ''}")
    check(f"run {n}: under 120 s real time", took < 120, f"{took:.1f}s")
    if not rep:
        return None
    proxy = [e for e in rep["events"] if e["kind"] == "proxy"]
    outcomes = [e["outcome"] for e in proxy]
    check(f"run {n}: report shows 3 attempts blocked by the outage, then 1 delivery",
          outcomes == ["fault", "fault", "fault", "forwarded"], str(outcomes))
    check(f"run {n}: markdown report written", "PASSED" in md and re.search(r"blocked by outage window #\d+", md) is not None)
    return tuple(outcomes) + tuple(s["status"] for s in rep["steps"]) + (proxy[-1]["virtual_time"][11:13] if proxy else "",)


def scenario_bug(d):
    rc, out, err = d.cs("run", "scenarios/trial.yml", timeout=1800)
    rep, md = latest_report()
    check("bug: scenario fails with exit code 6", rc == 6 and rep and rep["result"] == "failed", f"rc={rc}")
    if not rep:
        return
    f = rep.get("failed_step") or {}
    check("bug: fails at step 10 (receiver got the webhook exactly once)",
          f.get("index") == 10 and "exactly once" in f.get("name", ""), json.dumps(f)[:300])
    check("bug: detail says what was expected and what was found", "expected `1`" in f.get("detail", ""), f.get("detail", ""))
    proxy = [e for e in rep["events"] if e["kind"] == "proxy"]
    check("bug: report shows the single 503 that the worker treated as delivered",
          [(e["outcome"], e["status"]) for e in proxy] == [("fault", 503)], json.dumps(proxy)[:300])
    check("bug: later steps marked skipped", [s["status"] for s in rep["steps"][10:]] == ["skipped", "skipped"])
    check("bug: markdown has a 'What failed' section", "## What failed" in md and "Step 10 of 12" in md)
    text = json.dumps(rep) + md
    leaked = re.findall(r"\b[a-f0-9]{32}\b", text)
    check("bug: no session token in the report (redacted)", not leaked, str(leaked[:2]))
    check("bug: CLI prints the report path", "report:" in out and ".md" in out, out[-300:])


def https_upstream(cli):
    """Proxy to a real https:// upstream (needs internet access)."""
    def cs(*a, timeout=900):
        p = subprocess.run([cli, *a], cwd=HTTPS, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
        return p.returncode, p.stdout, p.stderr

    def ex(*cmd):
        s = json.load(open(os.path.join(HTTPS, ".chronostack", "session.json"), encoding="utf-8"))
        args = ["docker", "compose"]
        for f in s["compose_files"]:
            args += ["-f", f]
        p = subprocess.run(args + ["-p", s["project"], "exec", "-T", "client", *cmd], cwd=HTTPS,
                           capture_output=True, text=True, timeout=120, encoding="utf-8", errors="replace")
        return p.returncode, p.stdout, p.stderr

    if os.path.exists(os.path.join(HTTPS, ".chronostack", "session.json")):
        cs("down", "-v")
    rc, out, err = cs("up", "--wait", timeout=1800)
    check("https: up", rc == 0, (err or out)[-400:] if rc else "")
    if rc:
        return
    try:
        _, url, _ = ex("printenv", "TARGET_URL")
        check("https: client URL points at the proxy over plain HTTP", url.strip() == "http://chronostack-clockd:7200/", url.strip())
        _, body, err = ex("sh", "-c", "wget -q -O- \"$TARGET_URL\"")
        check("https: proxy fetched the https:// upstream", "Example Domain" in body, (body or err)[:200])
        cs("fault", "set", "ext", "--for", "1h")
        _, body, err = ex("sh", "-c", "wget -q -O- \"$TARGET_URL\" 2>&1; echo rc=$?")
        check("https: outage window applies to the https upstream", "503" in body or "rc=1" in body, body[:200])
    finally:
        rc, out, err = cs("down", "-v")
        check("https: down -v", rc == 0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    ap.add_argument("--repeat", type=int, default=1)
    a = ap.parse_args()
    cli = os.path.abspath(a.cli) if a.cli else os.path.join(ROOT, "target", "debug", "chronostack" + (".exe" if os.name == "nt" else ""))
    d = Demo(cli)
    h = open(os.path.join(DEMO, "compose.yml"), "rb").read()

    print("\n=== proxy and fault commands", flush=True)
    ok, out = d.up()
    if ok:
        try:
            proxy_checks(d, out)
        finally:
            d.down()

    outcomes = []
    for i in range(a.repeat):
        print(f"\n=== scenario run {i + 1}/{a.repeat}", flush=True)
        ok, _ = d.up()
        if ok:
            try:
                outcomes.append(scenario_pass(d, i + 1))
                if i == 0:
                    print("\n=== scenario again on the same stack (variables make it repeatable)", flush=True)
                    again = scenario_pass(d, "1b")
                    check("same stack: second run gives the same outcome", again == outcomes[-1], str(again))
            finally:
                d.down()
    if a.repeat > 1:
        check(f"scenario: {a.repeat} runs produced identical outcomes", len(set(outcomes)) == 1 and None not in outcomes, str(set(outcomes)))

    print("\n=== https upstream", flush=True)
    https_upstream(cli)

    print("\n=== deliberate bug (compose.bug.yml)", flush=True)
    ok, _ = d.up("compose.yml", "compose.bug.yml")
    if ok:
        try:
            scenario_bug(d)
        finally:
            d.down()
    check("demo compose.yml unchanged", open(os.path.join(DEMO, "compose.yml"), "rb").read() == h)

    print(f"\n{'ALL CHECKS PASSED' if not FAILS else f'{len(FAILS)} FAILED: ' + ', '.join(FAILS)}")
    sys.exit(1 if FAILS else 0)


if __name__ == "__main__":
    main()
