# ChronoStack — Risks & Limitations

## Top technical risks (ranked)

| # | Risk | Likelihood | Impact | Mitigation | Proven by |
|---|---|---|---|---|---|
| R1 | **Absolute REALTIME deadline hangs** (sem/mutex/cond/clock_nanosleep/timerfd) in some runtime | **Confirmed real in M0** (all such APIs hang without translation) | Critical (hung DB/app) | ADR-002 translation: **0 hangs in 272 checks with the fix**; keep the no-hang suite in CI | M0 ✅ (Mac/arm64 pending) |
| R12 | Host `CLOCK_REALTIME` steps backwards (measured about 2 ms on WSL2) | Certain on some hosts | Medium (non-monotonic virtual time) | ADR-017: anchor virtual time to `CLOCK_MONOTONIC` | M0 ✅ |
| R2 | **Schedulers don't run after advance** → users think ChronoStack is broken | High | High | Step/trigger/barrier model (ADR-005); per-library table; CLI warnings | M2 |
| R3 | Docker Desktop volume/mmap behaviour differs from Linux | Low–Medium | Critical | Controller + named volume (ADR-004); test on Docker Desktop, OrbStack | M0 |
| R4 | glibc symbol versioning / musl differences break interposition | Medium | High | `dlvsym`, per-libc builds, matrix tests | M0 |
| R5 | pg_cron skips jobs on jumps > 3 h (**source-verified**, not hypothetical) | Certain without mitigation | Medium | Automatic sub-stepping ≤ 1 h (ADR-005 amendment); negative test SCH-PGCRON-2 | M0 (confirm), M2 |
| R11 | Many sub-steps make long advances slow | Medium | Medium | Fast commit+probe path (< 100 ms); sub-stepping only when pg_cron detected | M2 |
| R6 | Compose edge cases (`network_mode`, `build`, profiles, `include`) | Medium | Medium | `compose config` JSON; test on real projects | M1 |
| R7 | Postgres timeouts firing early when advancing during long statements | Medium | Low | Document; advance only between steps; barriers ensure idle | M2 |
| R8 | Persistent virtual timestamps after `down` (Redis RDB, PG data) confuse later real-time runs | High | Medium | Separate project/volumes; `down -v` default in `run` | M1 |
| R9 | Performance overhead in hot paths | Low | Medium | Benchmarks; hot path = vDSO + seqlock | M0 |
| R10 | Scope creep (cohorts, chaos, UI) delays a usable v0.1 | High | High | MVP_SCOPE guardrails | ongoing |

## Product risks

* **"Just inject a Clock" objection.** Well-architected apps already inject clocks for unit tests. Counter: ChronoStack tests the *integration* (DB `now()`, Redis TTLs, cron, multiple services) that clock injection can't reach.
* **LD_PRELOAD perceived as hacky/unsafe.** Counter: read-only, no privileges, open-source C shim of a few hundred lines, fail-open, doctor output.
* **libfaketime DIY.** Some teams already mount libfaketime with `FAKETIME_TIMESTAMP_FILE`. ChronoStack must be clearly better: no-hang guarantees, orchestration, probes, triggers/barriers, reports.
* **Go-heavy teams excluded.** Large part of the cloud-native audience. Be upfront.
* **Narrow audience.** Primarily SaaS/billing/lifecycle-heavy products.

## Known limitations of v0.1 (to publish in README)

1. Time moves forward only.
2. App timers (`setTimeout`, `asyncio.sleep`, `ScheduledExecutorService`, node-cron, APScheduler, Spring cron) do not fire early; use triggers.
3. Postgres `now()` is fixed per transaction; long-lived transactions see the old time.
4. Go, static binaries, setuid binaries, processes outside Compose: not virtualized.
5. Native host processes: Node only, via `chronostack/register`.
6. External services (Stripe, SendGrid) are not virtualized; use their test-clock features or mocks.
7. Data written with virtual timestamps persists in ChronoStack volumes.
8. Faults: HTTP outage windows only.
9. "Reproducible" means ChronoStack's own decisions are replayable; app concurrency is not.
10. Docker Desktop for macOS verified manually per release, not in CI.
