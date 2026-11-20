# SPDX-License-Identifier: MIT OR Apache-2.0
"""`expose:` targets and `service://` URLs (tests/e2e/expose).

1. `up --wait` publishes exactly the exposed ports, on 127.0.0.1: web on a port
   Docker picks (its own `18080:80` is dropped), api on the fixed 18181. The
   worker publishes nothing.
2. Both answer over HTTP from the host, and `up` and `doctor` list them.
3. `advance 1d` runs an HTTP trigger to `service://web:80/tick` and waits on an
   HTTP barrier to `service://api:80/health`.
4. A scenario's `http` steps reach both through `service://` URLs.
"""

import argparse
import json
import os
import subprocess
import sys
import urllib.request

for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DIR = os.path.join(ROOT, "tests", "e2e", "expose")
PROJECT = "e2e-expose-chronostack"
failures = []


def run(args, timeout=600):
    env = dict(os.environ, MSYS_NO_PATHCONV="1")
    return subprocess.run(args, cwd=DIR, capture_output=True, text=True, timeout=timeout,
                          encoding="utf-8", errors="replace", env=env)


def check(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name}")
    if not ok:
        failures.append(name)
        if detail:
            print("     " + str(detail).strip().replace("\n", "\n     ")[:3000])


def get(url):
    try:
        with urllib.request.urlopen(url, timeout=10) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except Exception as e:  # noqa: BLE001 - reported as a failed check
        return None, str(e)


def published(service):
    """Host bindings Docker reports for a service of the ChronoStack project."""
    p = run(["docker", "ps", "--filter", f"label=com.docker.compose.project={PROJECT}",
             "--filter", f"label=com.docker.compose.service={service}", "--format", "{{.Ports}}"])
    return p.stdout.strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cli", required=True)
    a = ap.parse_args()
    cli = os.path.abspath(a.cli)

    if os.path.exists(os.path.join(DIR, ".chronostack", "session.json")):
        run([cli, "down", "-v"])
    try:
        p = run([cli, "up", "--wait"], timeout=900)
        out = p.stdout + p.stderr
        check("up --wait", p.returncode == 0, out)
        if p.returncode != 0:
            return 1
        check("up lists the exposed ports", "reachable from your machine" in p.stdout
              and "api:80  ->  http://127.0.0.1:18181" in p.stdout, p.stdout)

        session = json.load(open(os.path.join(DIR, ".chronostack", "session.json"), encoding="utf-8"))
        exposed = {(e["service"], e["port"]): e["host_port"] for e in session.get("exposed", [])}
        check("session records both ports", set(exposed) == {("web", 80), ("api", 80)}
              and exposed[("api", 80)] == 18181, session.get("exposed"))
        web_port = exposed.get(("web", 80))

        web, api, worker = published("web"), published("api"), published("worker")
        check("web is published on loopback only, and not on 18080",
              f"127.0.0.1:{web_port}->80/tcp" in web and "18080" not in web and "0.0.0.0" not in web, web)
        check("api is published on 127.0.0.1:18181",
              "127.0.0.1:18181->80/tcp" in api and "0.0.0.0" not in api, api)
        check("worker publishes nothing", worker == "", worker)

        for name, port in (("web", web_port), ("api", 18181)):
            status, body = get(f"http://127.0.0.1:{port}/hello")
            check(f"{name} answers from the host", status == 200 and "GET /hello" in body, body[:300])

        p = run([cli, "doctor", "--json"])
        try:
            d = json.loads(p.stdout.strip().splitlines()[-1])
            got = sorted((e["service"], e["port"], e["host_port"]) for e in d["exposed"])
        except (ValueError, KeyError, IndexError):
            got = None
        check("doctor --json lists them", got == sorted([("web", 80, web_port), ("api", 80, 18181)]),
              p.stdout + p.stderr)

        p = run([cli, "advance", "1d", "--json"])
        text = p.stdout + p.stderr
        check("advance 1d runs the service:// trigger and barrier", p.returncode == 0
              and "tick-web" in text and "api-answers" in text, text)

        p = run([cli, "run", "scenarios/reach.yml"], timeout=600)
        check("scenario http steps reach service:// URLs", p.returncode == 0, p.stdout + p.stderr)
    finally:
        run([cli, "down", "-v"], timeout=300)

    print("ALL CHECKS PASSED" if not failures else f"{len(failures)} check(s) failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
