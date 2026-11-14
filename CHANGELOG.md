# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project will adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once the first release is tagged.

ChronoStack is pre-alpha. Pre-releases are published on GitHub Releases.

## [Unreleased]

## [0.1.0-alpha.2] - 2026-11-14

The first published pre-release. It includes everything listed under 0.1.0-alpha.1, which was built but not published.

### Added

- `chronostack update [--check] [--version X] [--pre] [--force]`: updates the CLI from GitHub Releases. It verifies `SHA256SUMS` and replaces the running binary (Windows included), with no admin rights.
- Installers put `chronostack` on PATH. `install.ps1` also updates the current PowerShell session, so it works immediately. `install.sh` adds one marked line to the shell's startup file (zsh, bash, fish or `~/.profile`) and prints the command for the current terminal. `CHRONOSTACK_NO_MODIFY_PATH=1` opts out.
- GitHub Action (`action.yml`), MCP bundles and `server.json` for the MCP Registry, `docs/GUIDE.md`, demo GIF, launch drafts.

## [0.1.0-alpha.1] - 2026-11-14 (not published)

### Added

- Research and planning documentation in `docs/`:
  - product vision
  - competitive analysis
  - technical feasibility
  - architecture
  - architecture decision records
  - MVP scope
  - feature roadmap
  - implementation plan
  - testing strategy
  - security and reliability
  - risks and limitations
  - go-to-market
- `docs/SOURCE_VERIFICATION.md`: second-pass check of the technical claims against primary sources.
- `docs/OPEN_SOURCE_READINESS.md`, `docs/README.md` (documentation index), and `docs/development/` covering release process, licensing decision and repository setup.
- Community files: README, CONTRIBUTING, CODE_OF_CONDUCT (Contributor Covenant 3.0 with a reporting contact), SECURITY, issue and pull request templates, a discussion form.
- CI workflows:
  - documentation link check;
  - Rust CI (see M1 below).
- Dependabot for GitHub Actions.
- **M0 feasibility spike** (`spike/`, throwaway):
  - C preload shim (glibc and musl) with seqlock clock page v2 and absolute-deadline translation;
  - `chronoctl` writer;
  - Docker-based test matrix and harness.
  - Results in `docs/M0_RESULTS.md`: 137/137 pass on Docker Desktop/WSL2 x86_64.
- `LICENSE-MIT` and `LICENSE-APACHE`: the project is dual-licensed `MIT OR Apache-2.0`.
- ADR-017: virtual time is anchored to `CLOCK_MONOTONIC`, because M0 measured host `CLOCK_REALTIME` stepping backwards.

- **M1 — controller and core CLI** (pre-alpha, from source):
  - `crates/core`: clock page layout (asserted against the C header), API types, Compose model, override generator, durations.
  - `crates/clockd`: single writer of the clock page (ADR-017 protocol) and a token-authenticated localhost HTTP API.
  - `crates/cli` (`chronostack`): `up`, `down`, `now`, `advance`, `freeze`, `unfreeze`, `probe`, `doctor`.
  - `shim/`: the preload library promoted from the spike. The glibc build targets glibc 2.17 via zig.
  - `docker/clockd.Dockerfile`, and the end-to-end test `tests/e2e/run_m1.py`.
- CI:
  - Rust checks on Linux (x86_64 and arm64), Windows and macOS;
  - an MSRV check;
  - the e2e test on Linux x86_64 and arm64.
  - Dependabot now covers cargo and docker.

- `doctor` checks every process in each container and flags Go binaries (they bypass the shim), e.g. Gitea's server behind its C supervisor. Covered by a new e2e case (`caddy:2-alpine`).
- `docs/M1_RESULTS.md`: M1 exit gate passed on 3 unmodified docker/awesome-compose projects.

- **M2 — step → trigger → barrier engine:**
  - `chronostack.yml` (triggers scheduled in virtual time or per step; SQL/Redis/exec/HTTP barriers), JSON Schema in `docs/schema/`;
  - advance planner (`core::schedule`) and engine;
  - automatic pg_cron detection with 1 h sub-steps;
  - `advance --to/--step/--max-jump/--dwell/--dry-run`, and `chronostack config`.
- `examples/saas-trial-demo`: 14-day trial, 24 h sessions, daily 03:00 job (Node, Postgres, Redis).
- `tests/e2e/run_m2.py`: demo lifecycle, determinism repeats, and pg_cron (negative and positive). Runs in CI.
- pg_cron schedule-aware stepping: stops only at fixed-time job minutes (3 virtual days: 92 s → 6.4 s), with an hourly fallback. Includes `core::cron`.
- Isolation: the ChronoStack copy drops published host ports and `container_name` via Compose `!reset` (override is now YAML; `up --keep-ports` to opt out).
- Scheduler compatibility matrix (`tests/e2e/run_schedulers.py`): BullMQ and Celery beat fire on their own; node-cron needs a trigger.
- Nightly CI: the demo 10× on Linux x86_64 and arm64, plus the scheduler matrix.
- Dependency: `serde_norway` 0.9 (maintained fork of `serde_yaml`).
- `up --wait [--wait-timeout 120s]`: waits until every virtualized service answers its time probe (exit 3 on skew or timeout).
- `advance` reports wildcard pg_cron jobs (`*/5 * * * *`) that will run fewer times than they are due, in text, `--json` and `--dry-run`. New `advance --pg-cron-all` stops at every occurrence (limit 2,000 stops). Includes `Cron::estimate_runs`.
- No-hang suite through the product (`tests/e2e/run_nohang.py`): C glibc/musl, Python 3.10/3.12, Node 22, JVM 21 at +1 d, +30 d, +365 d and frozen. Runs nightly on x86_64 and arm64.
- Scheduler matrix: APScheduler 3 (BlockingScheduler).
- e2e: Postgres after a single 14-day jump (responsive, no checkpoint storm, autovacuum runs once).

- **M3 — outage windows, scenarios, reports:**
  - HTTP fault proxy in `clockd` (`proxies:` in `chronostack.yml`): URL environment variables are pointed at it in the override; outage windows in virtual time return 503 (or a chosen status); windows persist across restarts.
  - Event log in `clockd` (last 1,000 events) and `chronostack events`.
  - `chronostack fault set|list|clear` with `now`, `+<dur>`, `next HH:MM` and UTC times.
  - `chronostack run <scenario.yml>`: linear steps (`advance`, `trigger`, `exec`, `http`, `fault`, `fault_clear`, `assert`, `wait_for`, `sleep`); JSON and Markdown reports in `.chronostack/reports/` with secrets redacted; exit code 6 on failure. JSON Schema in `docs/schema/scenario.schema.json`.
  - `chronostack.yml`: `equals:` for SQL checks, `body:`/`content_type:` for HTTP.
  - Demo: transactional outbox, webhook retries with backoff, a receiver, `scenarios/trial.yml` and a deliberate bug (`compose.bug.yml`).
  - `tests/e2e/run_m3.py` in CI; nightly runs the scenario 5×.
- After the M3 review: `https://` proxy upstreams (TLS from the proxy; rustls), scenario variables (`${run_id}`, `save:` on exec steps), and a persistent event log (`events.jsonl`, survives `clockd` restarts).

- **M4 — MCP server and Node register:**
  - `chronostack mcp [--project-dir]`: MCP over stdio supporting protocol revisions 2026-07-28 (`server/discover`, `_meta`) and 2025-11-25/06-18/03-26 (`initialize`); 11 tools (`clock_*`, `probe`, `doctor`, `fault_*`, `events_tail`, `scenario_run`); `mcp.max_advance` cap (default 400 days) in `chronostack.yml`.
  - `sdk/node`: `node -r register` makes `Date` follow the virtual clock in host-native Node processes; `onAdvance(cb)`; fails open (strict mode for CI). Tested on Node 20/22/24 × Linux/Windows/macOS.
  - Controller: `GET /v1/clock/wait` long-poll.
  - `tests/e2e/run_m4.py` in CI.

- **M5 — release:**
  - `.github/workflows/release.yml`: on a `v*` tag, builds the controller image (x86_64 + arm64, pushed to GHCR) and the CLI for 5 targets, the preload libraries, `SHA256SUMS` and an SBOM; smoke-tests the install on fresh Linux runners; creates a draft GitHub Release. Manual runs are dry runs.
  - `install/install.sh` (Linux, macOS) and `install/install.ps1` (Windows): download a release, verify `SHA256SUMS`, install to a user directory.
  - The CLI's default controller image is set at build time; release builds pull `ghcr.io/asmodeus14/chronostack-clockd` pinned by digest. `doctor` shows the controller version and warns on a mismatch (`/v1/health` now reports it).
  - README: install instructions and a demo transcript from a real run.

### Fixed

- A timed-out `exec` trigger is now stopped inside its container (TERM, then KILL), including child processes. Previously only the local `docker` CLI was killed.
- A timed-out `docker compose exec` could hang the CLI: killing `docker` left the compose plugin process holding the output pipes. The CLI now kills the whole process tree and stops waiting for output after 2 s.

### Changed

- Architecture: the step / trigger / barrier engine moves to the host CLI, and no container receives the Docker socket (ADR-016).
- Stepped advances are split into sub-steps of at most 1 hour when pg_cron is present. Verified pg_cron source shows that clock jumps over 3 hours skip scheduled runs.
