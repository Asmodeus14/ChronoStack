# ChronoStack — Feature Roadmap & Scoring

Scores 1–5 (5 = highest value / hardest / riskiest / most differentiated / most suitable). Subjective, from this research phase.

| Feature | Dev value | Difficulty | Tech risk | Differentiation | MVP suitability | Decision |
|---|---|---|---|---|---|---|
| Cross-service virtual wall time (shim + controller) | 5 | 4 | 4 | 4 | 5 | **v0.1 core** |
| Step / trigger / barrier advance | 5 | 3 | 3 | 5 | 5 | **v0.1 core** |
| Probes + `doctor` (verification) | 4 | 2 | 2 | 4 | 5 | **v0.1** |
| Automatic failure reports / CI artifact export | 4 | 2 | 1 | 3 | 5 | **v0.1** (JSON report) |
| HTTP outage windows in virtual time (webhook sim) | 4 | 3 | 2 | 4 | 4 | **v0.1** (minimal) |
| Linear YAML scenarios with assertions | 4 | 3 | 2 | 3 | 4 | **v0.1** |
| MCP integration | 4 | 2 | 2 | 4 | 4 | **v0.1 late** |
| Node `register` for host-native Node | 4 | 2 | 2 | 3 | 3 | **v0.1 stretch goal → v0.2 if it slips** |
| Automatic sub-stepping (`max_jump`) for pg_cron-style schedulers | 4 | 2 | 1 | 3 | 5 | **v0.1** (source-verified need) |
| Scheduler inspection (list next runs for BullMQ/Celery/pg_cron) | 4 | 3 | 2 | 4 | 2 | v0.2 |
| Background job visibility (queue depth in step output) | 3 | 2 | 1 | 3 | 3 | v0.2 (barriers cover basics) |
| GitHub Actions integration (action + artifact upload) | 4 | 2 | 1 | 2 | 3 | v0.2 |
| Seeded latency/drop/partition (ChaCha8) | 3 | 3 | 2 | 3 | 2 | v0.2 (or drive Toxiproxy) |
| TCP connection disruption | 3 | 3 | 2 | 2 | 2 | v0.2 via Toxiproxy |
| Virtual user cohorts + event heap | 3 | 4 | 3 | 4 | 1 | v0.2 |
| Scenario recording (capture CLI/MCP session → YAML) | 3 | 2 | 1 | 3 | 2 | v0.2 |
| Deterministic replay | 2 | 5 | 5 | 3 | 1 | Reject claim; "reproducible re-run" only |
| Timeline debugger (TUI/web) | 3 | 3 | 1 | 4 | 1 | v1.0 |
| Snapshot & restore (PG + Redis state at virtual t) | 4 | 4 | 3 | 4 | 1 | v1.0 (enables going backward) |
| Browser testing integration (Playwright helpers + browser clock) | 4 | 2 | 2 | 3 | 2 | v0.2 (`page.clock` exists in Playwright; sync it) |
| VS Code integration | 2 | 3 | 1 | 2 | 1 | v1.0+ |
| AI-generated testing scenarios | 2 | 2 | 3 | 2 | 1 | Reject (MCP already enables it) |
| Stripe Test Clock sync (advance both together) | 4 | 3 | 2 | 5 | 1 | v0.2 idea — validate demand |
| Go support (ptrace/seccomp vDSO) | 4 | 5 | 5 | 3 | 1 | v1.0 investigation |
| Testcontainers module | 3 | 2 | 1 | 2 | 1 | v1.0 |

## Three strongest differentiators

1. **Whole-stack consistent virtual time with zero app changes — safely** (deadline translation, probes, fail-open/strict).
2. **Scheduler-aware stepping (trigger + barrier)** — actually executes and waits for lifecycle jobs; the true "Stripe Test Clocks for your code" experience.
3. **Agent-drivable and verifiable** — MCP tools plus probe-backed reports that agents and CI can trust.

## Release outline

* **v0.1** — core above; Postgres/Redis/Node/Python; demo; MCP.
* **v0.2** — cohorts + event heap; seeded network faults (own or Toxiproxy-driven); npm distribution + JS SDK (`clock`, `fault` for Playwright/Vitest, Playwright `page.clock` sync); scheduler inspection adapters (BullMQ, Celery, pg_cron, Sidekiq); JVM/Ruby promoted if tests pass; GitHub Action.
* **v1.0** — snapshot/restore (+ backward time), timeline viewer, Go support investigation, Testcontainers module, stable config/API with semver guarantees.
