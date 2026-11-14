# M3 — Outage windows, scenarios and failure reports: results

**Date:** 2026-11-14
**Exit gate** ([IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md#m3--webhook-outage-windows-scenarios-failure-reports-weeks-89)): *the deliberate-bug demo produces a report a new developer can understand without reading code.*

**Status: met.** Locally on Docker Desktop (WSL2), and in CI on native Linux x86_64 and arm64. In the first nightly run ([43554434710](https://github.com/Asmodeus14/ChronoStack/actions/runs/43554434710)) the scenario ran **5 times in a row with identical outcomes on both architectures**: 3 blocked attempts, 1 delivery at 07:00, all 11 steps passing, in 10–15 s real time per run on CI hardware.

## What was built

- **HTTP fault proxy in `clockd`** (ADR-010).
  - Declared in `chronostack.yml` under `proxies:` with a name, an `upstream` (`http://host:port` inside the Compose network) and the environment variables that hold the URL (`env: { worker: WEBHOOK_URL }`).
  - `up` points those variables at the proxy in the ChronoStack copy (`http://chronostack-clockd:7200/…`, keeping the path). The app and `compose.yml` are unchanged.
  - Each target gets its own port inside the Compose network; none is published to the host. `clockd` joins the services' networks when they are not the default one.
  - Requests are forwarded unchanged. During an **outage window** in virtual time the proxy answers with the window's status (default 503) and a JSON body naming the window. If the upstream fails, it answers 502: it fails closed, never silently bypassing a window.
  - Windows are stored next to the clock page, so they survive a `clockd` restart.
- **Event log** in `clockd`: the last 1,000 clock changes, window changes and proxied requests, each with virtual time and clock generation (`GET /v1/events?since=`).
- **CLI:**
  - `chronostack fault set <target> --from now|+2d|next 09:00|<UTC> --for 2h | --until … [--status 503]`, `fault list`, `fault clear <id> | --all`;
  - `chronostack events [--since N]`;
  - `chronostack run <scenario.yml> [--json]`: exit 0 = passed, 6 = a step failed.
- **Scenarios** (`crates/cli/src/scenario.rs`, [JSON Schema](schema/scenario.schema.json)), with variables (see below). Linear steps, each with one action: `advance` (the full step → trigger → barrier engine), `trigger`, `exec`, `http`, `fault`, `fault_clear`, `assert`, `wait_for`, `sleep`. Validated before anything runs, so a typo fails in milliseconds rather than after 10 virtual days.
- **Reports** in `.chronostack/reports/<time>-<scenario>.json` and `.md`:
  - the result and, on failure, a "What failed" section with the step, the virtual time, and what was expected versus found;
  - every step with its virtual time, every request through the proxy with its outcome, the outage windows, and a probe of every service at the end;
  - the JSON version adds the clock log (virtual time and generation after each step) and the trigger and barrier results of each stop.
  - Output from your services is truncated to 4 KB and **redacted**: URL credentials, `Bearer`/`Basic` values, `password=`/`token:`/`api_key` style values, and bare tokens (32+ hex or 40+ base64 characters). The e2e test checks that the demo's session token does not appear.
- **`chronostack.yml` additions:** `proxies:`; `equals:` on SQL checks (compare a value instead of a boolean); `body:`/`content_type:` on HTTP triggers and checks.
- **Demo** ([examples/saas-trial-demo](../examples/saas-trial-demo/README.md)):
  - the daily job now also queues a `trial.expired` webhook in a transactional outbox;
  - the worker delivers due webhooks every 500 ms using the database's `now()`, and retries failures with backoff (20 min, 40 min, 80 min, …);
  - a `receiver` service counts deliveries per event id;
  - a new barrier waits until the worker has handled every due webhook at each clock stop;
  - [`scenarios/trial.yml`](../examples/saas-trial-demo/scenarios/trial.yml): sign-up → outage on day 14 from 02:00 to 06:00 → 14 days → day 14 hour by hour → assertions;
  - `compose.bug.yml`: a deliberate regression where the worker treats any response, even a 503, as delivered.

## Results (e2e `tests/e2e/run_m3.py`, local Docker Desktop/WSL2)

| Check | Result |
|---|---|
| `up` rewrites `WEBHOOK_URL` to the proxy; the worker sees it | ✅ |
| Proxy forwards with no window; returns 503 inside one; forwards again after `advance 2h` | ✅ |
| Window survives `docker compose restart chronostack-clockd` | ✅ |
| Event log lists the proxied requests with outcomes | ✅ |
| Scenario passes (exit 0), 11/11 steps | ✅ in **40 s** real time for 15 virtual days |
| Report: the webhook was blocked by the outage at 03:00, 04:00 and 05:00, then delivered once at 07:00 | ✅ (4 attempts; receiver count 1) |
| Bug variant: exit code 6, fails at step 9 "the receiver got the trial.expired webhook exactly once" | ✅ `expected 1, got (empty)` |
| Bug report: the proxy table shows a single 503 and no retry | ✅ |
| No session token in the report | ✅ |
| Demo `compose.yml` unchanged; M2 demo e2e still passes (`advance 14d` 24 s with the new barrier) | ✅ |

### Exit gate: the deliberate-bug report

Excerpt of the Markdown report generated by the bug variant (trimmed; the inner code fence is shown indented):

```markdown
**Result: FAILED ❌**

## What failed

Step 9 of 11, **the receiver got the trial.expired webhook exactly once**, at virtual time 2026-11-29T12:00:05Z:

    expected `1`, got `(empty)`

## Requests through the fault proxy

| Virtual time | Target | Request | Response | Outcome |
|---|---|---|---|---|
| 2026-11-29T03:00:01Z | webhooks | POST /hooks | 503 | blocked by outage window #1 |
```

A reader can tell without opening the code that the webhook was attempted once, got a 503 during the outage and was never retried, while the steps before show the user was downgraded. That is the bug.

## Design notes and differences from the plan

1. **Outage = an HTTP status, not "connection refused".** ADR-010 allowed either. A status is visible in the event log and to the app; a refused connection would look like the proxy itself was down. Configurable per window (`--status`, 4xx/5xx).
2. **API shape:** `POST /v1/faults` (create), `GET /v1/faults`, `DELETE /v1/faults/{id}`, `DELETE /v1/faults` (all), instead of the planned `PUT /v1/faults/{target}`. A target can have several windows, so windows have their own ids.
3. **Scenario `http` steps run from the host**, like HTTP triggers. To reach a service from the host, start the stack with `up --keep-ports`. In-network HTTP checks can use `exec` with `wget`/`curl`.
4. **Retry timing must not coincide with clock stops.** The first demo design retried at 30 min, 1 h, 2 h with 1 h steps; due times then fell on stop boundaries and the outcome depended on sub-second timing. Backoff of 20/40/80 min keeps every due time at least 20 virtual minutes from a stop. The same advice applies to users' scenarios and is in the demo README.

## Known gaps

Closed after the M3 review (2026-11-14):

1. ~~Proxy is HTTP only~~ **Fixed.** Upstreams may be `https://`. The app's URL is rewritten to plain HTTP to the proxy inside the Compose network, and the proxy uses TLS (rustls, Mozilla root store) to the real upstream. e2e: an `https://example.com` upstream is fetched through the proxy, and an outage window blocks it with a 503. The controller image grew to 17 MB.
2. ~~Scenarios have no variables~~ **Fixed.** `${run_id}` (random per run) and `save: name` on `exec` steps (last line of output), usable as `${name}` in any later string. References are checked before the run, and reports show the `${…}` form, not the values. The demo scenario now uses its own user each run, so it **passes again on the same stack** (e2e checks a second run gives an identical outcome).
3. ~~Event log is in memory~~ **Fixed.** Appended to `events.jsonl` next to the clock page and reloaded on start (sequence numbers continue); compacted to the last 1,000 events. e2e: all 4 proxied requests are still listed after `docker compose restart chronostack-clockd`.

Still open:

- **Apps that hard-code URLs** can't be routed through the proxy (R15 in RISKS_AND_LIMITATIONS): the URL must come from an environment variable set in the Compose file. Intercepting hard-coded hostnames needs DNS takeover inside the network, and for HTTPS a trusted certificate; see the options in the risk entry.
- **macOS** remains untested (issue #5).
