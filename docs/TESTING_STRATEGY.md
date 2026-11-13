# ChronoStack — Testing Strategy

Principle: **a feature is not "supported" because `date` prints the virtual timestamp.** Support means: correct wall time, no hangs, and the relevant scheduled work demonstrably executes (or is documented as requiring a trigger) — on every platform listed.

No tests have been run yet. All targets below are goals to be measured, not results.

## 1. Test layers

| Layer | Scope | Tooling (proposed) | Runs in |
|---|---|---|---|
| Unit (Rust) | duration parsing, config/scenario schema, override generation (snapshot), env merging (`LD_PRELOAD` append), API types, report redaction | `cargo test`, `insta` | every PR |
| Unit (C shim) | seqlock reader, offset math, deadline translation math, fail-open/strict paths | small C test harness run under each target via Docker | every PR |
| Layout | C header vs Rust struct sizes/offsets identical | Rust test parsing generated `offsetof` table | every PR |
| Concurrency | seqlock: no torn reads under writer stress (1 writer, N readers, ~10⁸ reads) | multithreaded stress test; `loom` on Rust writer | every PR (short), nightly (long) |
| Docker integration | up/advance/freeze/probe/doctor/down | test harness driving real Docker | every PR (Linux) |
| Runtime compatibility | matrix below | containers per image | nightly + release |
| Scheduler execution | BullMQ, Celery beat, pg_cron, Sidekiq (exp), node-cron/APScheduler (negative) | containers | nightly + release |
| No-hang suite | deadline APIs under large offsets | containers with hard timeouts | every PR (core), nightly (full) |
| Fault injection | outage windows, retry delivery | proxy + demo app | every PR |
| Scenario / replay | same scenario → identical outcome N times | demo | nightly |
| Cross-platform | macOS Docker Desktop, OrbStack, Windows Docker Desktop | manual checklist (CI runners lack Docker on macOS) | release |
| Performance | shim overhead, advance latency | benchmarks | nightly, tracked |

## 2. Clock consistency tests

* CC-1: after `advance X`, all probes within ±1 s of controller virtual time.
* CC-2: 1 writer advancing every 1 ms, 8 reader threads per container × 5 containers: virtual time read by any thread never goes backward within that thread (monotonicity of virtual wall time under forward-only advances).
* CC-3: freeze → two reads 1 s apart return identical values; unfreeze resumes from frozen value (no jump back).
* CC-4: controller killed → services keep advancing at real rate from last offset.

## 3. Data store tests

PostgreSQL: PG-1..PG-11 (see TECHNICAL_FEASIBILITY §3.3).
Redis:

| ID | Test |
|---|---|
| RD-1 | `SET k v EX 86400`, `advance 25h`, `GET k` → nil |
| RD-2 | `TTL` after `advance 1h` decreases by 3600 ± 1 |
| RD-3 | `EXPIREAT` with virtual timestamp honoured |
| RD-4 | `TIME` ≈ virtual now |
| RD-5 | active expiry: 10 000 keys EX 60, `advance 2m`, no access, `DBSIZE` → 0 within 5 s real |
| RD-6 | keyspace notification `expired` delivered after advance |

## 4. No-hang suite (most important)

Each test runs with a hard real-time limit; any overrun = fail. Offsets: +1 d, +30 d, +365 d; also while frozen.

| ID | Test | Limit |
|---|---|---|
| NH-C1 | C: `sem_timedwait` deadline now+1 s | 3 s |
| NH-C2 | C: `pthread_mutex_timedlock` now+1 s (locked mutex) | 3 s |
| NH-C3 | C: `pthread_cond_timedwait` default (REALTIME) now+1 s | 3 s |
| NH-C4 | C: `pthread_cond_timedwait` with MONOTONIC condattr | 3 s (must not be translated) |
| NH-C5 | C: `clock_nanosleep(REALTIME, ABSTIME)` now+1 s | 3 s |
| NH-C6 | C: `timerfd` REALTIME ABSTIME now+1 s | 3 s |
| NH-PY1 | Python 3.10/3.12: `threading.Event().wait(1)`, `Lock.acquire(timeout=1)`, `queue.get(timeout=1)` | 3 s |
| NH-NODE | Node: `setTimeout 1s`, `Atomics.wait` 1 s, `worker_threads` | 3 s |
| NH-JVM | `Thread.sleep(1000)`, `Object.wait(1000)`, `LockSupport.parkNanos`, `ScheduledExecutor 1s` | 3 s |
| NH-PG | `pg_sleep(1)` with `statement_timeout=5s`; `lock_timeout` 1 s | 3 s / errors as expected |
| NH-RD | `BLPOP k 1` | 3 s |
| NH-ADV | each of the above while an advance happens mid-wait | documented expectation per API |

## 5. Scheduler execution tests

| ID | Setup | Pass criterion |
|---|---|---|
| SCH-PGCRON-1 | `* * * * *` job, `advance 1h --step 1m` + barrier | 60 runs |
| SCH-PGCRON-2 | daily `0 3 * * *` job, `advance 14d` with sub-stepping **disabled** | ≤ 1 run (source predicts skip) — negative test |
| SCH-PGCRON-3 | daily job, `advance 14d --step 1d` with automatic sub-stepping (≤ 1 h) | 14 runs |
| SCH-PGCRON-4 | `*/10 * * * *` wildcard job, sub-step 1 h | documents "wildcard runs once per jump" behaviour (CLOCK_JUMP_FORWARD) |
| SCH-BULL-1 | delayed job +2 h, `advance 3h` | processed within measured bound (recorded as "max dwell") |
| SCH-BULL-2 | job scheduler every 1 d, `advance 7d --step 1d` with trigger/barrier | 7 runs |
| SCH-CEL-1 | Celery beat daily, `beat_max_loop_interval=1`, `--step 1d --dwell 2s` | 14 runs |
| SCH-NODECRON-NEG | node-cron daily, `advance 1d` no trigger | 0 runs (expected) |
| SCH-TRIGGER | any scheduler replaced by declared trigger | exact counts |

## 6. Fault injection & replay

* FI-1: outage window `down` for 2 h virtual; worker retries with backoff stored in DB; delivered once after window.
* FI-2: overlapping windows, window entirely between two steps (must still be observed by requests made during it — requests *made* inside window fail; window not "skipped" by step granularity matters only if no request falls inside: documented).
* RP-1: scenario run 10× → identical final DB state hash and identical event sequence (excluding real timestamps).
* RP-2 (v0.2): seeded latency/drop decisions identical across runs for identical request sequences.

## 7. Negative tests

NEG-1..NEG-9 in TECHNICAL_FEASIBILITY.md, plus:
* NEG-10: compose service using `build:` with distroless static image → doctor reports "not virtualizable".
* NEG-11: setuid binary inside container → real time.
* NEG-12: user-supplied `LD_PRELOAD=libjemalloc.so` preserved and still active.

## 8. Performance targets (to be measured; not results)

| Metric | Target |
|---|---|
| Shim overhead per `clock_gettime(REALTIME)` | < 50 ns p99 over baseline vDSO call |
| Postgres pgbench TPS under shim | ≥ 98 % of baseline |
| Redis `redis-benchmark` SET/GET under shim | ≥ 98 % of baseline |
| `advance` (single step) + probes, 5 services | < 500 ms p95 |
| 14-step advance with trigger + barrier on demo | < 60 s on CI runner |
| Single sub-step (commit + probes, no triggers) | < 100 ms p95 (so 336 pg_cron sub-steps fit in ~35 s) |
| clockd idle memory | < 30 MB RSS |

## 9. CI

* GitHub Actions: `ubuntu-latest` (x86_64) and `ubuntu-24.04-arm` (arm64) for all Docker tests.
* macOS/Windows: CLI unit tests on hosted runners; Docker Desktop scenarios run manually per release with a signed-off checklist in release notes.
* Compatibility table in README is generated from the latest nightly matrix results, never hand-edited.
