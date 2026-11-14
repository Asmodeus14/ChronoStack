# ChronoStack — Security & Reliability

## Non-negotiable guarantees

1. **Never modify the host or VM OS clock.** No `settimeofday`, `clock_settime`, `adjtimex`; no `CAP_SYS_TIME`; no `--privileged`; no host PID/network namespaces.
2. **Never edit user files.** Compose files, Dockerfiles and `.env` are read-only inputs; output goes to `.chronostack/`.
3. **Fail safe.** Unsupported or broken virtualization never crashes target services by default (fail-open) and is always *reported* (probes/doctor). CI mode fails loudly (strict).

## Threat / risk review

| Area | Risk | Mitigation |
|---|---|---|
| Accidental host clock change | Bug or misuse alters system time | Architecture has no code path that can; enforced by not requesting capabilities. Test: run clockd with `cap_drop: [ALL]`. |
| Container privileges | Shim injection requiring privileges; Docker socket exposure | Only env var + read-only volume mount. `exec` triggers/probes run from the **host CLI** via `docker compose exec` (ADR-016); **no container ever receives the Docker socket**. |
| Malicious compose file / `chronostack.yml` | Triggers run arbitrary commands | Same trust level as `docker compose up` on that repo (it already runs arbitrary images). Print triggers on first run; `--yes` needed in non-interactive mode for new triggers. |
| Untrusted MCP clients | Agent runs destructive actions | Implemented in M4: stdio only (no network listener); fixed tool set of 11; no arbitrary exec/SQL beyond declared triggers/barriers; `clock_advance` capped by `mcp.max_advance` (default 400 d); `scenario_run` limited to .yml files inside the project directory; unknown arguments rejected. |
| Controller API exposure | Other local users/processes change the clock | Bind `127.0.0.1` only; 256-bit bearer token in `.chronostack/session.json` (0600 on Unix; user-only ACL on Windows); random port. |
| Unix socket permissions (if added later) | Same | 0600 sockets in user-owned dir. |
| Resource exhaustion | Huge `--step` counts, event log growth, barrier loops | Max steps per call (default 10 000), ring buffer bounds, barrier poll interval ≥ 100 ms, timeouts mandatory. |
| Shared-state corruption | Bad page bytes → absurd times | Shim validates magic, layout_version, page_size, and sanity bounds on offset (|offset| < 100 years); on failure uses real time and warns. Volume mounted read-only in services → only clockd can write. |
| Atomic consistency | Torn reads | Seqlock (ADR-004); stress tests (TESTING_STRATEGY §2). |
| Writer dies mid-update | Odd seq forever | Readers bounded retry → last good snapshot; clockd repairs on restart. |
| Fault proxy failure | Silent bypass would hide bugs | Proxy failure = connection refused (fail closed), logged as an event. |
| Secrets leakage | Reports/logs include env vars, trigger output, SQL results | Never serialize environment; redact values matching common secret patterns (implemented in M3: URL credentials, Bearer/Basic values, `password=`/`token:`/`api_key` style values, bare 32+ hex or 40+ base64 tokens; declared `redact:` keys are still planned); truncate trigger output (4 KB). Reports are local files only; nothing is sent over the network. No telemetry in v0.1. |
| Data safety | Running against a dev DB with real data writes "future" timestamps | `up` uses a distinct project name (`<project>-chronostack`) by default so volumes are separate; warn if the target project is already running. |
| Supply chain | Malicious release artifacts | Signed checksums, SBOM, reproducible-ish builds, GHCR image digests pinned in CLI release. |
| Shim in every process | Crash risk in critical processes | No allocation, no locks, no Rust/C++ runtime, constructor does minimal work; `dlsym` failures fall back to raw `syscall` for reads. |

## Reliability behaviours

* Controller restart is idempotent; the page survives; `session_id` prevents attaching to a stale page from another session.
* `chronostack down` always removes the override and ChronoStack containers; `down -v` also removes ChronoStack's volume (never user volumes unless `--volumes` is passed through explicitly).
* Every CLI command has a timeout and a clear error naming the failing service.

## Security disclosure

v0.1 ships a `SECURITY.md` with a private reporting address/GitHub security advisories.
