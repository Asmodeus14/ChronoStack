# M0 Feasibility Spike — Results

**Date:** 2026-11-13
**Code:** [`spike/`](../spike/README.md) (throwaway)
**Harness:** `spike/run_m0.py`
**Raw results:** [`spike/results/final-x86_64.md`](../spike/results/final-x86_64.md) and `.json`

**Environment actually tested:**

- Docker Desktop 29.6.2 on Windows 11 (WSL2 Linux VM, kernel 6.6.87.2, x86_64, 16 vCPU)
- Compose 5.3.1

**Not tested:**

- macOS / Apple Silicon (no Mac available)
- native Linux hosts
- arm64. The run under QEMU was stopped by the host before it finished, because the machine ran low on memory.

## Verdict

**Exit gate: PASSED on the platform tested. M1 can start**, with two items carried over that require hardware this spike did not have (see [Open items](#open-items)).

| Gate criterion ([IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md#m0--feasibility-spike-weeks-12)) | Result |
|---|---|
| Correct virtual wall time in all supported-target images | ✅ 35/35 read checks across Postgres 16 (Debian and Alpine), Redis 7 (Debian and Alpine), Node 22 (slim and Alpine), Python 3.10/3.12/3.12-alpine, Temurin 21, coreutils and busybox. Observed skew ≤ 0.8 s, all of it `docker exec` latency. |
| Zero hangs in the no-hang suite (Postgres, Redis, Node, Python 3.11+) | ✅ 272/272 checks at +1 d, +30 d, +365 d and frozen, plus a 30-day re-check on the final build. JVM and Python 3.10 also had zero hangs. |
| Works on Linux and on Docker Desktop for Mac | ⚠️ Docker Desktop (WSL2) only. macOS arm64 is still open. |
| Shim overhead measured | ✅ +11.7 ns (glibc), +13.8 ns (musl) per `clock_gettime(CLOCK_REALTIME)`. Target was < 50 ns. |

**Final build, x86_64: 137 pass, 0 fail.** Two skips are expected: musl has no `sem_clockwait` or `pthread_cond_clockwait`. Two results are informational only.

## What was proven

### 1. The deadline-hang risk is real, and the fix works

With deadline translation **disabled** (`CHRONOSTACK_DEADLINE_FIX=0`, offset +1 day), every absolute `CLOCK_REALTIME` wait hung until the 6-second watchdog killed it, on both glibc and musl:

| API | Fix off | Fix on (+1 d / +30 d / +365 d / frozen) |
|---|---|---|
| `sem_timedwait` | HANG | OK |
| `pthread_mutex_timedlock` | HANG | OK |
| `pthread_rwlock_timedwrlock` | HANG | OK |
| `pthread_cond_timedwait` (default and `PTHREAD_COND_INITIALIZER`) | HANG | OK |
| `pthread_cond_clockwait` / `sem_clockwait` (REALTIME, glibc) | HANG | OK |
| `clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME)` | HANG | OK |
| `timerfd` REALTIME + `TFD_TIMER_ABSTIME` | HANG | OK |
| JVM `LockSupport.parkUntil(wallclock)` | HANG | OK |
| `pthread_cond_timedwait` on a monotonic condvar, `nanosleep`, `poll` | OK | OK (correctly left untranslated) |

**Python 3.10 and 3.12 never hang, even with the fix off.** Every wait primitive tested (Lock, RLock, Event, Condition, Semaphore, queue, Timer, sleep, select, asyncio) uses monotonic or relative timeouts. This disproves the first-pass assumption that Python < 3.11 was at risk.

Node (`setTimeout`, `setInterval`, `Atomics.wait`, worker threads) passes with the fix on. It was not run with the fix off, because that run was cut short.

### 2. Cross-container shared clock works through the Docker Desktop VM

The writer and the readers ran in different containers and shared a page on a named volume:

- every read check passed;
- a stress test of 4 reader threads against a writer churning freeze → advance → unfreeze (about 2 million commits per second) recorded **0 backward steps in about 96 million reads**, consistently across 7 runs on the final build.

### 3. Datastores behave as predicted from source

- **Redis 7 (Debian and Alpine):**
  - `SET EX 86400` + advance 25 h → key gone;
  - TTL after +1 h = 3599;
  - `EXPIREAT virtual+100` → TTL 98;
  - 10,000 keys with `EX 60` + advance 2 min → **active expiry** removed them within 0.3–1.2 s, without the keys being accessed;
  - `BLPOP 1` returns in about 1 s.
- **Postgres 16 (Debian and Alpine):**
  - rows older than 14 days are found after advancing 15 days;
  - `now()` stays frozen inside a transaction that spans an advance, while `clock_timestamp()` moves +24 h;
  - `statement_timeout` works under an offset, in both the succeed and cancel cases;
  - the container restarts cleanly under the shim.
- **pg_cron** confirms the source reading in [SOURCE_VERIFICATION §2](SOURCE_VERIFICATION.md#2-postgresql-16) exactly:

| Daily `0 3 * * *` job, 3 days | Expected | Observed |
|---|---|---|
| One 14-day jump (PG-8) | 0 | **0** |
| 1 h steps (PG-9) | 3 | **3** |
| 4 h steps (PG-9b, > 3 h → `CLOCK_CHANGE`) | 0 | **0** |
| 3 h steps (PG-9c, boundary = `CLOCK_JUMP_FORWARD`) | 3 | **3** |
| `* * * * *` with 1 min steps and a heartbeat barrier (PG-7) | 20/20 | **20/20** (barrier wait avg 0.68 s) |

This empirically confirms that `MINUTE_COUNT = 60` and that the sub-stepping design (ADR-005) is necessary and sufficient. A heartbeat job (`* * * * *`) works as a barrier.

### 4. Negative tests behave as documented

| Test | Result |
|---|---|
| Go binary | Real time; not interceptable |
| Static musl binary | Real time; not interceptable |
| Raw `syscall(SYS_clock_gettime)` | Real time; not interceptable |
| `setTimeout(5 s)` + a 1-day advance | Fires after 5005 ms real; advancing time does **not** fire timers |
| Missing page | Real time + `chronostack: clock unavailable, using real time` |
| Corrupt page | Real time + `chronostack: clock unavailable, using real time` |
| `CHRONOSTACK_STRICT=1` | Aborts (exit 134) |

## Design changes forced by M0

| # | Finding | Change |
|---|---|---|
| 1 | **The host clock itself steps backwards.** Under WSL2, raw `CLOCK_REALTIME` stepped back about 2 ms in 4 of 6 twenty-second windows, with no ChronoStack code involved. `real + offset` therefore leaked host corrections into virtual time. | **Virtual time is anchored to `CLOCK_MONOTONIC`** (`virtual = monotonic + offset`). Clock page layout v2. Proposed as [ADR-017](ARCHITECTURE_DECISIONS.md#adr-017--virtual-time-is-anchored-to-clock_monotonic). |
| 2 | Seqlock fallback: a shared, multi-field "last good" snapshot mixed values across threads, giving backward steps of up to 104 ms. | Per-thread fallback snapshot. Fall back only when the writer is stuck for more than **1 s of wall time**; a spin count fired after about 2 ms of normal preemption. |
| 3 | The writer read the real clock before its "update in progress" mark was visible to other CPUs, giving backward steps of up to 616 ns. | **Writer requires a `SEQ_CST` fence after the odd `seq` store.** This is a requirement for the Rust `clockd` in M1. |
| 4 | musl has no `*_clockwait` / `*_clocklock`. Exported stubs made feature detection lie, causing a spin and an immediate return. | Interpose those only on glibc builds. |
| 5 | Reading both the monotonic and real clocks on every call doubled the overhead (+49 ns). | Read the real clock only when needed (deadline translation, `CLOCK_TAI`). Overhead is back to +12–14 ns. |

## Informational results

- **PG-6:** a `pg_sleep(4)` with a +1 day advance after 2 s still took about 4.4 s, so the advance did not end the sleep early. The first-pass worry that pg_sleep is wall-clock bound was not observed. Advancing *during* statements is still discouraged (ADR-005: barriers first).
- **`CLOCK_TAI`** keeps the kernel's TAI−UTC offset on top of virtual time. The observed +36.7 s is the 37 s leap offset minus `docker exec` latency.

## Open items

1. **Docker Desktop for Mac (arm64).** Not tested; no Mac was available. It is required by the gate, so it is carried into M1 as a blocking manual check before alpha.
2. **arm64 run.** The QEMU build was stopped when the host ran low on memory. **Partly closed by M1 CI (2026-11-13):** the M1 e2e test passes on a native `ubuntu-24.04-arm` runner. Postgres (musl), Redis (glibc), Node (musl) and Debian bullseye (glibc) were all virtualized on aarch64, with probes and restart checks passing. The full M0 no-hang suite has not yet been run on arm64.
3. **Native Linux host (Docker Engine).** **Closed by M1 CI:** the e2e test passes on `ubuntu-latest` (x86_64) and `ubuntu-24.04-arm`.
4. **Not covered by M0:**
   - `timer_settime(TIMER_ABSTIME)` on POSIX timers
   - `mq_timed*`
   - raw futex syscalls
   - Ruby
   - BullMQ wake-up bound (source reading still pending)
   - Node with the deadline fix off
5. **glibc floor.** The spike's shim was built on glibc 2.36. M5 must build against an older glibc (zig target such as `x86_64-linux-gnu.2.17`) and test on older images.

## How to reproduce

```sh
docker compose -f spike/compose.yml -p cs-m0 build
python spike/run_m0.py --only baseline,reads,nohang,timers,failopen,stress,bench,redis,postgres,pgcron
```

The full `nohang` section includes the fix-off runs and takes about 30 minutes. `nohang_quick` re-checks every runtime at +30 days in about 3 minutes.
