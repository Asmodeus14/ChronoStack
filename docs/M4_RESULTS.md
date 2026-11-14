# M4 — MCP server and Node register: results

**Date:** 2026-11-14
**Exit gate** ([IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md#m4--mcp-server--node-register-week-10)): *both demos recorded.* The automated parts are met. The manual check, a Claude Code session driving the demo three times, has **not been run yet** (see "Not done").

## What was built

### `chronostack mcp` (`crates/cli/src/mcp.rs`)

An MCP server over stdio, for AI coding agents (Claude Code, Cursor, …).

- **Protocol revisions supported** (checked against the specification on 2026-11-14):
  - **2025-11-25, 2025-06-18 and 2025-03-26** use `initialize` → `notifications/initialized` → `tools/list` / `tools/call`, plus `ping`. An unsupported requested version gets 2025-11-25.
  - **2026-07-28** (the latest revision, which removes `initialize`) uses `server/discover`. Every request carries `_meta` with `io.modelcontextprotocol/protocolVersion` and `…/clientCapabilities`. A wrong version gets `-32022` with the supported list; missing capabilities get `-32602`. Results include `resultType`, and list results include caching hints.
  - stdio framing: one JSON-RPC message per line on stdout, logs on stderr. The server exits when stdin closes. Batches are rejected with `-32600`.
- **11 tools.** These are the 10 from ADR-011 plus a read-only `fault_list`:
  - `clock_status`, `clock_advance` (`duration` or `to`: `+2d`, `next 03:00`, a UTC time; optional `step`, `dry_run`), `clock_freeze`, `clock_unfreeze`;
  - `probe`, `doctor`;
  - `fault_list`, `fault_set`, `fault_clear`;
  - `events_tail` (`since`, `limit`);
  - `scenario_run` (`file`).

  Each returns `structuredContent` plus the same JSON as text. Failures (bad arguments, a trigger that failed, a refused advance) are `isError: true` results the agent can read. An unknown tool is a JSON-RPC `-32602`.
- **Same engine as the CLI.** Each tool runs `chronostack <command> --json` as a subprocess. Agents get exactly what a person gets, and nothing but protocol messages reaches stdout.
- **Safety** ([SECURITY_AND_RELIABILITY](SECURITY_AND_RELIABILITY.md)):
  - stdio only; no network listener;
  - a fixed tool set, with no shell and no SQL except the triggers and barriers declared in `chronostack.yml`;
  - `clock_advance` is capped by `mcp.max_advance` in `chronostack.yml` (default 400 days);
  - `scenario_run` accepts only `.yml`/`.yaml` files inside the project directory;
  - unknown arguments are rejected.
- **Setup** (Claude Code):

  ```sh
  claude mcp add chronostack -- chronostack mcp --project-dir /path/to/your/project
  ```

### Node register (`sdk/node`)

`node -r …/sdk/node/register.js app.js` makes `Date` follow the virtual clock in a Node process running on the host ([README](../sdk/node/README.md)).

- The first read is synchronous, through a short-lived child process. After that, a **long-poll** to the new controller endpoint `GET /v1/clock/wait?after=<generation>&timeout_ms=` delivers each change; `onAdvance(cb)` exposes them.
- Fails open with one warning when no session is found; `CHRONOSTACK_STRICT=1` fails instead.
- `instanceof Date`, `Date()` (the string form), `Date.UTC`/`Date.parse` and explicit timestamps are unchanged.

### Controller

- **`GET /v1/clock/wait`** is the long-poll used by the Node register. It returns as soon as the clock generation differs from `after`, or after `timeout_ms` (at most 30 s). Each waiter runs on its own thread so the API is never blocked; at most 64 at once, after which it answers 503.
- This replaces the planned `/v1/stream` (server-sent events). A long-poll is simpler for both sides and has the same latency for this use.

## Results

| Check | Where | Result |
|---|---|---|
| MCP unit tests: handshake, negotiation, 2026-07-28 `_meta`, errors, path checks | `cargo test` | ✅ |
| Long-poll: wakes on change, times out, returns at once on a stale generation | clockd tests (in the image build) | ✅ |
| Node SDK, against a fake controller: offset, `Date` semantics, live update, freeze, no session, wrong token, strict mode | `node --test`, CI on Linux, Windows and macOS × Node 20/22/24 | ✅ all 9 CI combinations |
| e2e: MCP over stdio on the demo, both protocol eras | `tests/e2e/run_m4.py` | ✅ |
| e2e: `clock_advance 1d` ran the 03:00 trigger once; 500 days refused by the cap | same | ✅ |
| e2e: `scenario_run` through MCP passed, with proxy traffic: 3 blocked, 1 delivered | same | ✅ (38 s) |
| e2e: `../../Cargo.toml` refused; unknown method `-32601`; unknown tool `-32602`; only JSON-RPC on stdout; exits on stdin close | same | ✅ |
| e2e: host Node `Date.now()` vs the controller's virtual time | same | ✅ **within 1 ms** (target 50 ms) |
| e2e: `advance 1h` reached the host Node process | same | ✅ 682 ms **before** the CLI command returned; the CLI then probes the services |

## Not done

- **Manual agent runs (3/3).** The plan asks for a Claude Code session to verify webhook retries during a 2-hour outage, checking that the agent uses `fault_set` → `clock_advance` → `events_tail`. That needs your Claude Code account and an interactive session. Run it with the setup command above and the prompt from the plan. The e2e test covers the same tool sequence without a model.
- **npm publishing** stays in v0.2 (ADR-014). The SDK is used from the repository.
- **Recorded demos** (a terminal cast or GIF) belong with the M5 README work.
