# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project will adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once the first release is tagged.

ChronoStack is in alpha. Pre-releases are published on GitHub Releases.

## [Unreleased]

### Added

- The demo has a cohort scenario (`examples/saas-trial-demo/scenarios/cohort.yml`): 20 trialists over 30 virtual days with a webhook outage. Every trialist must be downgraded and every `trial.expired` webhook delivered exactly once; with `compose.bug.yml` it fails on that check. The demo's `chronostack.yml` exposes `api:3000`. A nightly job runs a users scenario 10 times and requires one timeline, runs the demo cohort twice, and checks that the bug variant fails.
- Simulated users on the virtual clock: `users: journeys.yml` in a scenario (or `{ file, resolution }`). Users arrive and think in virtual time from the scenario's start, so `think: 13d` passes in no real time. During each `advance` the clock also stops wherever users are due: they act (up to `max_parallel` at once), then the barriers must hold again, then time moves on. Due times are rounded up to `resolution` (default 1m) and follow only the seed, so the same seed gives the same timeline of who did what when. The report adds a users table, the JSON report has the full timeline (`users.timeline`), JUnit gets a test case per journey step, and a failed user step fails the scenario. Users still mid-journey when the scenario ends are reported as stopped.
- Benchmark B3 (`bench/`): `chronostack users run` with 200 users against a local echo server, about 8,000–24,000 requests per second at `max_parallel` 32 on a laptop and GitHub's runners. Docs for simulated users: GUIDE section 8 and `docs/schema/journeys.schema.json`.
- `chronostack users run <file>` and `chronostack users validate <file>`: simulated users. A journey file lists cohorts (how many users, when they arrive: evenly or Poisson over a window) and the steps each user takes: `http` requests with `expect` (status, JSON values) and `save` (JSON pointers into the response, used later as `${name}`), `think` times, `branch: {p}` and `repeat`. Every random choice comes from a seed (`--seed` or `seed:`), so a run can be repeated. Targets can be `service://` URLs from `expose:`. The summary shows runs, failures and p50/p90/p99/max latency per step; `--json` and `--junit` write reports; a failed step exits with 6. Users run in real time for now, with thinks capped at `max_think` (default 30s).
- `expose:` in `chronostack.yml` publishes chosen container ports of the copy on 127.0.0.1: `app:3000` on a free port, or `{service: app, port: 3000, host_port: 8080}` on a fixed one. Nothing else is published, so the copy still runs next to your own stack. `up` and `doctor` show the URLs (`exposed` in `doctor --json`). In HTTP triggers, barriers and scenario `http` steps, `service://app:3000/path` goes to that port; `chronostack.yml` is checked so such a URL names an exposed port.
- Publishing to crates.io: `chronostack` and `chronostack-core` are published from each release's tag (`cargo install chronostack@<version>`), starting with the next release.
- Scoop (Windows): `scoop bucket add chronostack https://github.com/Asmodeus14/scoop-bucket`, then `scoop install chronostack/chronostack`. The bucket is updated when a release is published.

### Changed

- A CLI built from source in release mode (as `cargo install` does) now uses the published controller image for its version, `ghcr.io/asmodeus14/chronostack-clockd:<version>`, instead of a locally built `chronostack/clockd:dev` it can't find. Debug builds still use the local image. Release downloads are unchanged (pinned by digest).
- `chronostack update` also refuses to replace a Scoop install and says to run `scoop update chronostack`, as it does for Homebrew.

## [0.1.0-alpha.7] - 2026-11-19

### Added

- `chronostack run --junit <path>` also writes the scenario result as JUnit XML for CI test reporters. Each step is a test case: a failed step becomes a `<failure>` with its evidence, steps after it are `<skipped>`, and the run ID and virtual start and end times are recorded as properties.
- `mcp.max_wall` in `chronostack.yml` (default `10m`): an MCP tool call that is still running after this much real time is stopped and returns an error, so a barrier that never clears can't leave an AI agent waiting indefinitely.
- Homebrew: `brew install asmodeus14/tap/chronostack` (macOS and Linux). The tap is updated when a release is published.
- Benchmarks (`bench/`): clock-read overhead against native and libfaketime, and `advance` latency, with results from GitHub's x86_64 and arm64 runners and a laptop. About 12–16 ns per read in C and 10–26 ns in Python and Node.

### Changed

- `chronostack up` refuses to start when the copy would share writable storage with your own stack: a bind mount (`./data:/var/lib/postgresql/data`), a volume with `external: true`, or a volume with a fixed `name:`. Docker can't give the copy its own version of these, so a job that runs during `advance` (a cleanup of "old" records, say) could change or delete your real data. The error names each mount. Use a named volume, mount it read-only (`:ro`, which only prints a note), or pass `--allow-shared-mounts` if sharing is intended. `chronostack doctor` lists these mounts (`shared_mounts` in `--json`).
- `chronostack update` refuses to replace a Homebrew install and says to run `brew upgrade chronostack`; `update --check` suggests the same.

### Fixed

- MCP `scenario_run` now applies `mcp.max_advance`. Before, a scenario file could advance the clock further than `clock_advance` was allowed to. The cap counts from the virtual time at the start of the scenario; a step that would go past it fails with a "refused" message and nothing moves.
- MCP `fault_clear` no longer lists faults after a failed clear; it returns the error straight away.

## [0.1.0-alpha.6] - 2026-11-18

### Added

- `chronostack init`: writes a starter `chronostack.yml` from your compose file. It detects Postgres (and pg_cron), Redis, Sidekiq, Celery (beat, workers, embedded `-B`, django-celery-beat) and Node workers. It adds a Sidekiq barrier when there's a Redis service, and commented suggestions for BullMQ, Celery and triggers. It warns when Celery beat lacks `--max-interval 1`. `--print` shows the file without writing it; an existing file is only replaced with `--force`. The generated file is validated before it's written.

### Fixed

- Windows installer: an account with no user-level PATH made `install.ps1` fail after copying the binary. It now treats a missing PATH as empty.

## [0.1.0-alpha.5] - 2026-11-16

### Added

- Built-in queue barriers: `bullmq: { service, queues, prefix }` (also works with Bull) and `sidekiq: { service, namespace }`. They wait until nothing is queued, running, or delayed and already due, read with Redis's own (virtual) clock. The Sidekiq barrier also waits for a Sidekiq heartbeat after the queues look empty, because Sidekiq only reports running jobs on its heartbeat.
- `settle` on any barrier: wait that long at each stop before the first check, for schedulers that enqueue work on their next poll (sidekiq-cron, Clockwork).

### Changed

- macOS: the native macOS CLI is tested in CI (nightly), against Docker in a Linux VM (Colima) on GitHub's Intel macOS runners: the M1 and M2 end-to-end suites pass. Apple-silicon Macs are still untested (#5).
- `advance` checks the barriers before its first jump too, so no job is running when the clock moves. A job in flight during a jump could see its Redis lock expire with the jump; BullMQ then ran it a second time as "stalled".

## [0.1.0-alpha.4] - 2026-11-15

### Added

- Scenarios: `advance: { to: "${var}" }` accepts a value saved by an earlier step, so a test can advance to a time read from the data (for example a subscription's period end). Found while testing Polar.

### Fixed

- App containers that set `POSTGRES_PASSWORD` or `POSTGRES_USER` (to connect to a database) no longer fail every probe, and `up --wait`, with "unavailable". When the Postgres probe gets no answer, ChronoStack checks the container's own clock instead. Images named pgvector, postgis or timescale are also recognised as Postgres servers. Found while testing Chatwoot.

### Documentation

- Guide: BullMQ can take up to about 10 s to pick up a due job, and a slow job holds up a one-at-a-time queue in real time; a barrier that counts due delayed jobs, with an example. Found while testing Twenty.
- Guide: sidekiq-cron skips a run it notices more than 60 s late; how to land steps for it. Found while testing Chatwoot.

## [0.1.0-alpha.3] - 2026-11-14

### Fixed

- One-shot containers that exit with code 0, such as a migrator, no longer fail every `advance`, scenario and `doctor` run with "unavailable". They are reported as `completed`. A container that exited with an error still fails. Found while testing Plane's self-hosted stack.

### Documentation

- Guide: schedulers that fire on their own (Celery beat) run a missed job when the clock lands, not at the job's own time, and how to land steps for exact-time tests. Also: don't mount your own files at `/chronostack`.

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
