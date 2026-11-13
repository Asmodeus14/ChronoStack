# ChronoStack — v0.1 MVP Scope

## The one workflow v0.1 must nail

> "I run `chronostack up` on my existing compose project, run `chronostack advance 14d --step 1d`, and my real Postgres, Redis and worker behave as if 14 days passed — trials expire, sessions expire, the daily job runs 14 times, a webhook outage on day 3 is retried — and I get a pass/fail report in under a minute."

## Reference application (`examples/saas-trial-demo`)

| Piece | Implementation | Time mechanism exercised |
|---|---|---|
| API | **Node (Express)** — decided for the demo (matches the npm/SDK audience); Python is covered by the runtime matrix | `Date.now()` via shim |
| Postgres 16 | `users(trial_ends_at, plan)` | `now()`, `clock_timestamp()` |
| Redis 7 | session key `EX 86400` | TTL / lazy + active expiry |
| Worker | Node worker consuming a BullMQ queue | wake-up behaviour is unresolved → demo relies on triggers + barriers, not on BullMQ timing |
| Daily job | "expire trials" — run by a declared ChronoStack trigger in the main demo; a pg_cron variant (with automatic ≤ 1 h sub-stepping) is tested separately | trigger vs polling scheduler |
| Webhook delivery | worker POSTs `trial.expired` to a receiver through ChronoStack proxy; retries with backoff stored as `next_attempt_at` in Postgres | virtual-time outage window + retry |

Acceptance script (manual and automated):

1. `chronostack up` → doctor shows all 5 services virtualized.
2. Sign up user → `advance 25h` → session gone, trial active.
3. `fault set webhook-receiver down --from +2d --for 2h` (relative to now).
4. `advance 14d --step 1d` with trigger `expire-trials` and barrier `jobs idle` → user downgraded on day 14; webhook delivered after outage (retry visible in event log).
5. Report written to `.chronostack/report-<run>.json`; exit code 0.

## In scope (v0.1)

* C preload shim, 4 builds (x86_64/aarch64 × glibc/musl), wall-clock virtualization + absolute-deadline translation.
* `chronostack-clockd` controller image (multi-arch).
* CLI: `up`, `down`, `status`/`now`, `advance <dur> [--step] [--dwell]`, `freeze`, `unfreeze`, `doctor`, `probe`, `fault set|clear|list`, `run <scenario.yml>`, `mcp`.
* `chronostack.yml`: triggers (`exec`, `http`), barriers (`sql`, `redis`, `http`, `exec`), probes, fault targets, linear scenarios with assertions (`sql`, `redis`, `http`).
* Failure report JSON + human summary.
* MCP server (stdio; calls the same engine as the CLI).
* Automatic sub-stepping (`max_jump`, default 1 h when pg_cron is detected).
* **Stretch goal (not required for v0.1 release):** Node `register` for host-native Node. If it slips, it moves to v0.2.
* Platforms: Linux x86_64/arm64 (Docker Engine), macOS arm64 (Docker Desktop, OrbStack), Windows x64 (Docker Desktop/WSL2) — CLI only talks to Docker.

## Compatibility matrix (target; claims gated on tests)

| Component | Status in v0.1 | Gate |
|---|---|---|
| PostgreSQL 16 (debian, alpine) | Supported | PG-1..5, PG-9, PG-10 |
| pg_cron | Supported via automatic sub-stepping (≤ 1 h; source shows jumps > 3 h skip runs) | PG-7..9 |
| Redis 7 (debian, alpine) | Supported | RD-1..6 |
| Node 20/22 (debian, alpine) — wall clock | Supported | RT-NODE |
| Python 3.11–3.13 (debian) — wall clock | Supported | RT-PY |
| BullMQ | Supported with trigger; dwell-only behaviour unresolved | SCH-BULL |
| Celery beat | Supported with `beat_max_loop_interval ≤ 1` (default scheduler otherwise waits up to 300 s) | SCH-CEL |
| node-cron, APScheduler, Spring cron | Trigger required | NEG-4 |
| Python 3.10 (wall clock) | Supported target (M0: no hangs even without deadline translation) | RT-PY |
| JVM 17/21, Ruby 3.x | Experimental (M0: JVM 21 reads + no-hang pass with translation; `parkUntil` hangs without it) | no-hang suite passes |
| Go, static binaries, setuid binaries | Unsupported | NEG-1/2 |
| MySQL, MongoDB | Untested (likely work: dynamic glibc) | — |

## Explicitly out of scope (v0.1)

Virtual user cohorts, event-heap scheduler, latency/drop/partition faults, seeded RNG faults (beyond outage windows), snapshot/restore, backward time, timeline UI, VS Code extension, npm CLI distribution, Kubernetes, Podman, Go support, monotonic virtualization.

## Definition of done

* All "Supported" rows have green automated tests on Linux x86_64 and arm64 CI.
* macOS Docker Desktop verified manually against the acceptance script, recorded in release notes.
* No-hang suite: zero hangs across supported runtimes at offsets +1 d, +30 d, +365 d.
* README compatibility table matches test results exactly.
