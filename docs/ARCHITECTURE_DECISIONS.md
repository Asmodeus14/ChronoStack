# ChronoStack — Architecture Decision Records

Format: Context → Decision → Consequences. Status of all ADRs: **Proposed** (pending M0 evidence).

---

## ADR-001 — Virtualize wall-clock time only; never virtualize monotonic clocks

**Context.** Runtime timers, timeouts, pools and event loops depend on `CLOCK_MONOTONIC`. Jumping monotonic forward does not wake sleeping kernel waits and breaks many invariants (see TECHNICAL_FEASIBILITY §3.1.3). libfaketime users routinely disable monotonic faking (Java hangs).
**Decision.** The shim modifies `CLOCK_REALTIME`, `CLOCK_REALTIME_COARSE`, `CLOCK_TAI`, `gettimeofday`, `time`, `timespec_get`, `ftime`. All monotonic and CPU clocks pass through.
**Consequences.** Advancing time does not execute app timers. Scheduled work must be driven by polling schedulers, triggers, or SDKs (ADR-005). This must be stated prominently in docs and CLI output.

## ADR-002 — Translate absolute REALTIME deadlines in the shim

**Context.** With a forward offset, APIs that take absolute REALTIME deadlines (`sem_timedwait`, `pthread_mutex_timedlock`, REALTIME `pthread_cond_timedwait`, `clock_nanosleep(TIMER_ABSTIME)`, ABSTIME `timerfd_settime`/`timer_settime`, `*_clockwait` with REALTIME) would wait for `offset` extra real time → hangs.
**Decision.** Interpose these and subtract the current offset from the deadline before calling the real function; track per-condvar clock by interposing `pthread_cond_init`/`pthread_condattr_setclock` (or by reading the clock from the condattr). When frozen, compute deadline relative to frozen virtual now.
**Consequences.** Most complex part of the shim; highest-risk code. Covered by a dedicated "no-hang" test suite (TESTING_STRATEGY §4). If M0 cannot make this reliable, v0.1 scope is reduced to runtimes proven not to use REALTIME deadlines.

## ADR-003 — Write the preload shim in C, not Rust

**Context.** The shim runs inside every process of every target container (bash, initdb, postgres backends, node). It must build for x86_64/aarch64 × glibc/musl, and be tiny, allocation-free and auditable. Rust `cdylib` for musl targets requires disabling `crt-static` and pulls in std runtime pieces unless `no_std`.
**Decision.** ~400–600 lines of C99 depending only on libc and `<dlfcn.h>`, cross-compiled with `zig cc` for the four targets. Rust is used for everything else.
**Consequences.** Two languages in the repo. Mitigation: the clock page layout is defined once in a C header and checked against the Rust struct by a layout test (size/offset assertions).
**Alternative considered.** Rust `no_std` cdylib — viable; revisit if C becomes a maintenance burden.

## ADR-004 — Clock page in a Docker named volume, written by a controller container (seqlock)

**Context.** Host-mounted files do not provide reliable mmap coherence across the Docker Desktop VM boundary. All containers share one kernel/page cache inside the VM. Two independent atomics allow torn reads.
**Decision.** `chronostack-clockd` container is the single writer of `/chronostack/clock` on volume `chronostack-<project>`; services mount the volume read-only. Layout v1 = `{magic, layout_version, page_size, seq, offset_ns, frozen_virtual_ns, frozen, flags, generation, session_id}` using a seqlock. Host tools talk to the controller over a localhost HTTP API.
**Consequences.** One code path for Linux, macOS, Windows. Needs one extra container and a published port. Clock changes require the controller to be running.

## ADR-005 — Step → trigger → barrier instead of "advance and sleep"

**Context.** Fixed `--dwell` sleeps cannot guarantee that schedulers ran: required wait depends on each library's internal sleep (seconds to hours) and on job duration.
**Decision.** `advance` performs, per step: commit clock → probes → declared triggers → barriers (conditions polled until true or timeout). `--dwell` remains as a best-effort fallback and the CLI warns when only dwell is used.
**Amendment (second pass, source-verified).** pg_cron applies Vixie cron's clock-jump logic: a forward jump > 3 h evaluates only the current minute, skipping intermediate runs (https://github.com/citusdata/pg_cron/blob/main/src/pg_cron.c, `StartAllPendingRuns`). Therefore each step is split into **sub-steps of at most `max_jump`** (default 1 h when pg_cron is detected; configurable). Barriers run after the full step; probes after each sub-step.
**Amendment (M2).** Triggers can be **scheduled in virtual time** (`schedule: {every: 1d, at: "03:00"}`, UTC). The planner stops the clock exactly at each occurrence, so `--step` is optional; it remains for schedulers that poll the clock. pg_cron is detected via `shared_preload_libraries`, which sets `max_jump=1h` and `dwell=1.2s` automatically. Measured cost: about 30 s of real time per virtual day with pg_cron.
**Consequences.** Users of schedulers that don't poll wall time must declare a trigger (one line of YAML). This is honest and reproducible, and it is the product's core value. Long advances with pg_cron take proportionally more sub-steps (14 d ≈ 336 sub-steps; each must be fast — performance target in TESTING_STRATEGY).

## ADR-006 — Resolve Compose projects with `docker compose config --format json`

**Context.** Raw YAML parsing misses interpolation, `extends`, profiles, `include`, multiple `-f` files and `.env`.
**Decision.** Shell out to the user's Docker Compose v2 and consume the canonical JSON. Generate an override file; never edit user files or Dockerfiles.
**Consequences.** Requires Compose v2 (≥ 2.20 assumed; verify). Removes `serde_yaml` dependency for compose (still used for `chronostack.yml`).

## ADR-007 — Per-image libc and architecture detection

**Context.** glibc shim won't load in musl images; arch can differ per service under emulation.
**Decision.** Inspect each service image (`docker image inspect` for arch; a one-shot probe for `ld-musl-*` vs `ld-linux-*`), cache by image digest, pick the matching `.so`. Services with no dynamic loader are marked "not virtualizable" and reported.
**Consequences.** One extra short-lived container per new image digest on first `up`.

## ADR-008 — Fail open by default, strict mode for CI

**Context.** A broken shim must never crash a developer's database. But silently using real time makes tests pass incorrectly.
**Decision.** Default: on any attach error, use real time and print one stderr warning. `CHRONOSTACK_STRICT=1` (default for `chronostack run` and whenever `CI=true` is set): abort the process with a clear message. Probes after each advance detect non-virtualized services regardless.
**Consequences.** Two behaviours to test (NEG-6, NEG-7).

## ADR-009 — Forward-only time in v0.1

**Context.** Data stores persist virtual timestamps (Redis absolute expiry, Postgres rows, WAL). Moving backward creates "future" data and confuses apps.
**Decision.** `advance` accepts positive durations only; `freeze` allowed; going back requires `chronostack down -v && up`.
**Consequences.** Snapshot/restore is the future answer (v1.0).

## ADR-010 — Fault injection scope in v0.1: HTTP outage windows in virtual time

**Context.** Toxiproxy already offers mature TCP faults (toxics: latency, bandwidth, slow_close, timeout, reset_peer, slicer, limit_data, packet_loss; "down" by disabling a proxy) with an HTTP API (https://github.com/Shopify/toxiproxy, verified). ChronoStack's unique angle is scheduling faults on the *virtual* timeline (e.g. "webhook endpoint down from day 3 09:00 to day 3 11:00").
**Decision.** v0.1 ships a minimal reverse proxy inside clockd for declared HTTP egress targets, supporting `down` (503 or connection refused) windows keyed to virtual time. Latency/drop/partition with seeded `ChaCha8Rng` move to v0.2. Document Toxiproxy interop.
**Consequences.** Smaller surface; webhook-retry demo still possible.
**Amendment (M3, 2026-11-14).** Implemented as an HTTP status during the window (default 503, configurable 4xx/5xx), not connection refused: a status shows up in the app's logs and the event log, while a refused connection looks like the proxy is broken. Targets are wired by rewriting a URL environment variable in the override (`proxies[].env`), so apps need an env-configurable URL. Upstream failures return 502 (fail closed).

## ADR-011 — MCP is a thin client of the controller API

**Context.** Agents are a strong 2026 distribution channel, but MCP must not bypass safety.
**Decision.** `chronostack mcp` is a stdio server exposing `clock_status`, `clock_advance`, `clock_freeze`, `clock_unfreeze`, `probe`, `doctor`, `fault_set`, `fault_clear`, `scenario_run`, `events_tail`. No arbitrary command execution; only triggers declared in `chronostack.yml` can run. Delivered after clock correctness (M4).
**Consequences.** Spec: https://modelcontextprotocol.io/specification. Revision 2025-06-18 (verified: stdio transport, JSON-RPC 2.0, `tools/list`, `tools/call`); a newer 2026-07-28 revision exists — target the current revision at M4 and pin it in docs.

## ADR-012 — Say "reproducible", not "deterministic"

**Context.** Real threads, I/O, and schedulers are nondeterministic; only ChronoStack's own decisions (clock steps, fault RNG, scenario order) can be replayed.
**Decision.** Positioning and docs use "reproducible" and "seeded". Deterministic simulation is explicitly out of scope (cf. Antithesis).
**Consequences.** Avoids credibility loss on HN/r/rust.

## ADR-013 — Defer event-heap scheduler and virtual user cohorts to v0.2

**Context.** A `BinaryHeap` of scheduled events can only jump to timestamps it knows; it cannot see app-internal timers. Without barriers it would race real work.
**Decision.** v0.1 scenarios are linear step lists. v0.2 adds the heap: next jump = min(next cohort event, next declared trigger time), each jump followed by barriers.
**Consequences.** M4 of the original plan moves out of v0.1.

## ADR-014 — Distribution: GitHub Releases + install script in v0.1; npm in v0.2

**Context.** npm packaging with platform `optionalDependencies` is valuable but premature before correctness.
**Decision.** v0.1: prebuilt CLI binaries (cargo-zigbuild for Linux and macOS targets; **Windows built on a native Windows runner** — cargo-zigbuild states "only Linux and macOS targets are supported"), `cargo install`, controller image on GHCR (multi-arch). The Node SDK ships as a small npm package only if M4 lands. https://github.com/rust-cross/cargo-zigbuild
**Consequences.** Less reach at launch; less maintenance.

## ADR-015 — Go is unsupported in v0.1

**Context.** Go's runtime resolves `__vdso_clock_gettime` itself (https://github.com/golang/go/blob/master/src/runtime/vdso_linux_amd64.go, verified) and libfaketime documents that such runtimes bypass it. Possible approaches (ptrace-based vDSO patching, seccomp user notification) need elevated privileges and are complex. Chaos Mesh TimeChaos does offset time for arbitrary containers on Kubernetes, but its documentation page does not describe its mechanism — investigate its source before relying on it as a reference.
**Decision.** Document as unsupported with a negative test. Revisit for v1.0.

## ADR-016 — Orchestration runs in the host CLI; no Docker socket in containers

**Context.** The first-pass ARCHITECTURE placed `exec` triggers in clockd (requiring the Docker socket — root-equivalent on the Docker host), while SECURITY preferred running them from the host. Contradiction.
**Decision.** clockd = clock writer + minimal API + network probes + fault proxy + event log. The host CLI runs the step/trigger/barrier engine, sub-stepping, `exec` triggers/probes via `docker compose exec`, scenarios and reports. MCP calls the same engine in-process.
**Consequences.** No container ever receives the Docker socket. Advances require the CLI (or MCP server) process to be running for the duration of the step loop — acceptable for a developer/CI tool. SDK consumers that only need the clock still talk to clockd directly.
**Amendment (M1).** *All* probes, including Postgres and Redis, also run from the CLI via `docker compose exec` (`psql`, `redis-cli`, `date`). clockd therefore needs no database credentials, network access to services or probe endpoint; its API is just clock state plus health. Doctor's "shim in PID 1" check runs as PID 1's own uid, because `/proc/1/maps` of non-root processes (Postgres, Redis) is not readable by root without CAP_SYS_PTRACE.

## ADR-017 — Virtual time is anchored to CLOCK_MONOTONIC

**Status:** Proposed, with M0 evidence ([M0_RESULTS.md](M0_RESULTS.md)).
**Context.** Under Docker Desktop (WSL2), M0 measured the VM's raw `CLOCK_REALTIME` stepping **backwards by about 2 ms** in 4 of 6 twenty-second windows, with no ChronoStack code involved. With `virtual = CLOCK_REALTIME + offset` (ADR-004 as first written), every such host correction leaked into virtual time, and readers observed time going backwards.
**Decision.**
- Clock page layout v2 defines `virtual = CLOCK_MONOTONIC + offset_ns`, or `frozen_virtual_ns` when frozen. All containers on one kernel share `CLOCK_MONOTONIC`.
- The real clock is read only when needed: for absolute-deadline translation (`real_deadline = real_now + (virtual_deadline − virtual_now)`) and for `CLOCK_TAI`.
- **Writer requirement:** a `SEQ_CST` fence after the odd `seq` store, before reading any clock inside the write window.
- **Reader requirements:**
  - retry while `seq` is changing;
  - fall back to a per-thread snapshot only if `seq` stays odd for more than 1 s.
**Consequences.**
- Virtual time no longer follows NTP corrections of the host. Over a long session it may drift from the host wall clock by the host's corrections. That is irrelevant for tests and preferable to non-monotonic virtual time.
- `chronostack now` reports `virtual − real` as the human-meaningful offset.
- The Rust `clockd` (M1) must implement the writer rules above, verified by the cross-container stress test (CC-2).
