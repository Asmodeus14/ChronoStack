# SPDX-License-Identifier: MIT OR Apache-2.0
"""M4 end-to-end: MCP server over stdio, and the Node register on the host.

Uses examples/saas-trial-demo (started with `chronostack up --wait`):
  1. `chronostack mcp`: 2025 handshake (initialize → tools/list → tools/call)
     and 2026-07-28 (server/discover + _meta); clock tools, the advance cap,
     outage + events, scenario_run, path checks; stdout carries only JSON-RPC;
  2. `node -r sdk/node/register.js` on the host: Date matches the controller's
     virtual time within 50 ms, and an advance reaches the process via the
     long-poll in well under a second.

Usage: python tests/e2e/run_m4.py [--cli PATH]
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import threading
import time

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEMO = os.path.join(ROOT, "examples", "saas-trial-demo")
REGISTER = os.path.join(ROOT, "sdk", "node", "register.js")
FAILS = []
META = {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {}}


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""), flush=True)
    if not ok:
        FAILS.append(name)


def sh(args, timeout=900, cwd=DEMO):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout, p.stderr


class Mcp:
    """A minimal MCP client over the server's stdio."""

    def __init__(self, cli):
        self.p = subprocess.Popen([cli, "mcp", "--project-dir", DEMO], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
        self.raw = []
        self.n = 0

    def send(self, method, params=None, notify=False):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notify:
            self.n += 1
            msg["id"] = self.n
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        if notify:
            return None
        line = self.p.stdout.readline()
        self.raw.append(line)
        return json.loads(line)

    def tool(self, name, args=None, meta=False):
        params = {"name": name, "arguments": args or {}}
        if meta:
            params["_meta"] = META
        r = self.send("tools/call", params)
        res = r.get("result") or {}
        return res.get("isError"), res.get("structuredContent") or {}, r

    def close(self):
        self.p.stdin.close()
        try:
            rc = self.p.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.p.kill()
            rc = None
        rest = self.p.stdout.read()
        return rc, rest, self.p.stderr.read()


def mcp_checks(cli):
    m = Mcp(cli)
    try:
        r = m.send("tools/list")
        check("mcp: tools/list before initialize is rejected", r.get("error", {}).get("code") == -32602, json.dumps(r)[:200])
        r = m.send("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "1"}})
        check("mcp: initialize negotiates 2025-06-18", r["result"]["protocolVersion"] == "2025-06-18" and "tools" in r["result"]["capabilities"])
        m.send("notifications/initialized", notify=True)
        tools = [t["name"] for t in m.send("tools/list")["result"]["tools"]]
        check("mcp: tools/list has the fixed tool set (11 tools)", len(tools) == 11 and "clock_advance" in tools and "scenario_run" in tools, str(tools))

        err, st, _ = m.tool("clock_status")
        check("mcp: clock_status", not err and "virtual_ns" in st, json.dumps(st)[:200])
        v0 = st.get("virtual_ns", 0)

        err, st, _ = m.tool("clock_advance", {"duration": "1d"})
        runs = st.get("trigger_runs")
        check("mcp: clock_advance 1d runs the daily trigger", not err and runs == 1 and st.get("ok") is True, f"isError={err} runs={runs}")
        err, st, _ = m.tool("clock_status")
        check("mcp: virtual time moved by about 1 day", abs((st["virtual_ns"] - v0) / 1e9 - 86400) < 120, f"{(st['virtual_ns'] - v0) / 1e9:.0f}s")

        err, st, _ = m.tool("clock_advance", {"duration": "500d"})
        check("mcp: advance beyond mcp.max_advance (400d) is refused", err is True and "max_advance" in st.get("error", ""), json.dumps(st)[:200])
        err, st, _ = m.tool("clock_advance", {"duration": "1d", "to": "+1d"})
        check("mcp: duration and to together is a tool error", err is True)
        err, st, _ = m.tool("clock_advance", {"to": "next 03:30", "dry_run": True})
        check("mcp: dry run with `next HH:MM` returns the plan", not err and len(st.get("stops", [])) >= 1 and st["stops"][-1]["at"].endswith("T03:30:00Z"), json.dumps(st)[:200])

        err, st, _ = m.tool("fault_set", {"target": "webhooks", "from": "+1h", "for": "2h"})
        check("mcp: fault_set", not err and st.get("status") == 503, json.dumps(st)[:200])
        err, st, _ = m.tool("fault_set", {"target": "nope", "for": "1h"})
        check("mcp: fault_set on an unknown target is a tool error", err is True)
        err, st, _ = m.tool("fault_list")
        check("mcp: fault_list shows the window", not err and len(st.get("faults", [])) == 1)
        err, st, _ = m.tool("fault_clear", {"all": True})
        check("mcp: fault_clear all", not err and st["remaining"]["faults"] == [], json.dumps(st)[:200])
        err, st, _ = m.tool("events_tail", {"limit": 5})
        kinds = [e["kind"] for e in st.get("events", [])]
        check("mcp: events_tail returns the latest events", not err and 0 < len(kinds) <= 5 and "fault_clear" in kinds, str(kinds))
        err, st, _ = m.tool("probe")
        check("mcp: probe", not err)
        err, st, _ = m.tool("doctor")
        check("mcp: doctor", not err and "services" in st)

        err, st, _ = m.tool("scenario_run", {"file": "../../Cargo.toml"})
        check("mcp: scenario_run outside the project is refused", err is True and "inside the project" in st.get("error", ""))
        t0 = time.monotonic()
        err, st, _ = m.tool("scenario_run", {"file": "scenarios/trial.yml"})
        outcomes = [p["outcome"] for p in st.get("proxy_requests", [])]
        check("mcp: scenario_run passes and reports the proxy traffic",
              not err and st.get("result") == "passed" and outcomes == ["fault", "fault", "fault", "forwarded"],
              f"{time.monotonic() - t0:.0f}s result={st.get('result')} {outcomes} {json.dumps(st.get('failed_step'))}")

        r = m.send("nope/method")
        check("mcp: unknown method → -32601", r.get("error", {}).get("code") == -32601)
        r = m.send("tools/call", {"name": "shell", "arguments": {}})
        check("mcp: unknown tool → -32602", r.get("error", {}).get("code") == -32602)
    finally:
        rc, rest, err = m.close()
    check("mcp: exits when stdin closes", rc == 0, f"rc={rc}")
    def is_rpc(line):
        try:
            return json.loads(line).get("jsonrpc") == "2.0"
        except ValueError:
            return False
    bad = [l for l in m.raw + rest.splitlines() if l.strip() and not is_rpc(l)]
    check("mcp: stdout carried only JSON-RPC messages", not bad, str(bad[:2]))

    # 2026-07-28: no initialize; discover + _meta on every request.
    m = Mcp(cli)
    try:
        r = m.send("server/discover", {"_meta": META})
        check("mcp 2026-07-28: server/discover", r["result"]["supportedVersions"] == ["2026-07-28"] and r["result"]["resultType"] == "complete")
        r = m.send("tools/list", {"_meta": META})
        check("mcp 2026-07-28: tools/list with _meta (caching hints)", len(r["result"]["tools"]) == 11 and r["result"]["cacheScope"] == "public")
        err, st, _ = m.tool("clock_status", meta=True)
        check("mcp 2026-07-28: tools/call", not err and "virtual_ns" in st)
        r = m.send("tools/list", {"_meta": {**META, "io.modelcontextprotocol/protocolVersion": "2031-01-01"}})
        check("mcp 2026-07-28: unsupported version → -32022", r.get("error", {}).get("code") == -32022)
    finally:
        m.close()


def node_checks(cli):
    node = shutil.which("node")
    if not node:
        check("node: available on the host", False, "node not found")
        return
    rc, out, _ = sh([cli, "now", "--json"])
    offset_ms = json.loads(out.strip().splitlines()[-1])["offset_ns"] / 1e6
    code = ("const cs=require(process.env.REG.replace('register.js','index.js'));"
            "console.log(JSON.stringify({active: cs.active, offset: Date.now() - cs.RealDate.now()}))")
    rc, out, err = sh([node, "-r", REGISTER, "-e", code], cwd=DEMO, timeout=60)
    try:
        r = json.loads(out.strip().splitlines()[-1])
    except (ValueError, IndexError):
        r = {}
    check("node: register finds .chronostack/session.json and activates", r.get("active") is True, (err or out)[-300:])
    if not r.get("active"):
        return
    check("node: Date matches the controller's virtual time within 50 ms",
          abs(r["offset"] - offset_ms) < 50, f"node offset {r['offset']:.0f} ms vs controller {offset_ms:.0f} ms")

    watcher = ("const cs=require(process.env.REG.replace('register.js','index.js'));"
               "console.log(JSON.stringify({ready:true}));"
               "cs.onAdvance(()=>{console.log(JSON.stringify({offset: Date.now()-cs.RealDate.now(), at: cs.RealDate.now()}));process.exit(0)});"
               "setInterval(()=>{},1000);")
    env = {**os.environ, "REG": REGISTER}
    p = subprocess.Popen([node, "-r", REGISTER, "-e", watcher], cwd=DEMO, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                         text=True, encoding="utf-8", env=env)
    first = p.stdout.readline()
    check("node: watcher started", "ready" in first, first)
    time.sleep(0.5)
    rc, out, _ = sh([cli, "advance", "1h", "--no-probe", "--json"])
    done = time.time() * 1000
    rc2, out2, _ = sh([cli, "now", "--json"])
    new_offset = json.loads(out2.strip().splitlines()[-1])["offset_ns"] / 1e6
    line = ""
    t = threading.Timer(15, p.kill)
    t.start()
    line = p.stdout.readline()
    t.cancel()
    p.kill()
    try:
        r = json.loads(line)
    except ValueError:
        r = {}
    check("node: onAdvance fired after `chronostack advance 1h`", "offset" in r, line or p.stderr.read()[-300:])
    if "offset" in r:
        check("node: new offset matches the controller within 50 ms", abs(r["offset"] - new_offset) < 50,
              f"{r['offset']:.0f} vs {new_offset:.0f}")
        check("node: change arrived before the advance command returned (long-poll)", r["at"] <= done + 50,
              f"{r['at'] - done:+.0f} ms relative to the CLI returning")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli")
    a = ap.parse_args()
    cli = os.path.abspath(a.cli) if a.cli else os.path.join(ROOT, "target", "debug", "chronostack" + (".exe" if os.name == "nt" else ""))
    os.environ["REG"] = REGISTER
    if os.path.exists(os.path.join(DEMO, ".chronostack", "session.json")):
        sh([cli, "down", "-v"])
    rc, out, err = sh([cli, "up", "--wait", "--wait-timeout", "180s"], timeout=1800)
    check("up --wait (demo)", rc == 0, (err or out)[-400:] if rc else "")
    if rc == 0:
        try:
            print("\n=== MCP", flush=True)
            mcp_checks(cli)
            print("\n=== Node register (host)", flush=True)
            node_checks(cli)
        finally:
            rc, out, err = sh([cli, "down", "-v"])
            check("down -v", rc == 0)
    print(f"\n{'ALL CHECKS PASSED' if not FAILS else f'{len(FAILS)} FAILED: ' + ', '.join(FAILS)}")
    sys.exit(1 if FAILS else 0)


if __name__ == "__main__":
    main()
