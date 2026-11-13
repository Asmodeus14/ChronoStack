# ChronoStack — Technical Feasibility

Status: research phase. **No code was written and no experiments were run while producing this document.**

> **Second-pass update (2026-11-13):** Key references were opened and checked. See [SOURCE_VERIFICATION.md](SOURCE_VERIFICATION.md). Labels below were updated accordingly; notably the pg_cron clock-jump behaviour is now **verified** (it skips runs on jumps > 3 h), and several claims were downgraded to assumptions.

Evidence labels used throughout:

| Label | Meaning |
|---|---|
| **[Verified-source]** | Stated in upstream documentation or source code (URL given). Not re-executed. |
| **[Search-confirmed]** | Corroborated by web search during this research phase (URL given). |
| **[Assumption]** | Expert belief based on how the system is known to work. Must be proven in M0 before any support claim. |
| **[Unresolved]** | Open question; recorded instead of guessed. |
| **[Recommendation]** | Design decision derived from the above. |

---

## 0. Executive summary

1. **Virtual wall-clock time via `LD_PRELOAD` is feasible** for dynamically linked glibc/musl programs (Postgres, Redis, Node, Python, JVM, Ruby official images). This is proven in practice by libfaketime. **[Verified-source]**
2. **Virtual wall-clock time does not execute scheduled work.** Almost every runtime timer is based on `CLOCK_MONOTONIC` plus kernel relative timeouts. Advancing `CLOCK_REALTIME` changes what `Date.now()`/`NOW()` returns; it does not wake sleeping timers. **[Verified-source]**
3. **Advancing `CLOCK_REALTIME` can hang programs** that pass absolute REALTIME deadlines to the kernel (`sem_timedwait`, `pthread_mutex_timedlock`, REALTIME `pthread_cond_timedwait`, `clock_nanosleep(TIMER_ABSTIME)`, ABSTIME timerfd/POSIX timers). The shim must translate these deadlines. **This is the single biggest technical risk.** **[Search-confirmed + Assumption]**
4. **Host-file shared memory is not portable** to Docker Desktop (macOS/Windows). Clock state must live inside the Docker VM (named volume + controller container). **[Assumption, strongly supported; M0 test]**
5. **Kernel time namespaces cannot virtualize `CLOCK_REALTIME`.** **[Verified-source]**
6. **Go and fully static binaries cannot be intercepted with `LD_PRELOAD`.** **[Verified-source]**

Conclusion: the original architecture is viable only with the changes recorded in [ARCHITECTURE_DECISIONS.md](ARCHITECTURE_DECISIONS.md): a seqlock-protected clock page inside the Docker VM, a C shim that translates absolute deadlines, and a **step → trigger → barrier** model instead of "advance and sleep".

---

## 3.1 Linux time interception

### 3.1.1 How time reaches a process

| Path | Interceptable by `LD_PRELOAD`? | Notes |
|---|---|---|
| App → libc `clock_gettime` (PLT) → vDSO | **Yes** | The normal path for C/C++, Node (V8/libuv), CPython, HotSpot, Ruby. **[Verified-source]** `ld.so(8)`: https://man7.org/linux/man-pages/man8/ld.so.8.html |
| App → libc `gettimeofday` / `time` (IFUNC → vDSO) | **Yes**, exported symbol can be interposed | **[Verified-source]** `vdso(7)`: https://man7.org/linux/man-pages/man7/vdso.7.html |
| libc-internal calls (libc calling its own `__clock_gettime`) | **No** | Internal calls bind locally (hidden symbols). E.g. glibc functions that read the clock internally are not affected. **[Assumption]**: list in M0 which public functions are affected (`timespec_get`, `ftime`, `clock`, `localtime` uses passed time so is fine). |
| Direct vDSO call (Go runtime) | **No** | Go's runtime locates the vDSO itself and calls `__vdso_clock_gettime`. **[Verified-source]** https://github.com/golang/go/blob/master/src/runtime/vdso_linux_amd64.go |
| Raw `syscall(SYS_clock_gettime, …)` | **No** | Rare in practice. **[Assumption]** |
| Statically linked binaries (static musl, Go, some Rust) | **No** | No dynamic loader → no preload. **[Verified-source]** `ld.so(8)` |
| Setuid/setgid binaries | **No** (secure-execution mode ignores most `LD_PRELOAD`) | **[Verified-source]** `ld.so(8)` |

### 3.1.2 Interfaces the shim must intercept

**Read interfaces (virtualize when the clock id is wall-clock based):**

| Function | Clock | Action |
|---|---|---|
| `clock_gettime(CLOCK_REALTIME / CLOCK_REALTIME_COARSE / CLOCK_TAI)` | wall | add offset (or return frozen value) |
| `clock_gettime(CLOCK_MONOTONIC / _RAW / _COARSE / BOOTTIME)` | monotonic | **pass through unchanged** |
| `clock_gettime(CLOCK_PROCESS_CPUTIME_ID / THREAD_CPUTIME_ID)` | CPU | pass through |
| `gettimeofday` | wall | add offset |
| `time` | wall | add offset |
| `timespec_get(TIME_UTC)` | wall | add offset (C11; glibc implements it internally) |
| `ftime` (obsolete) | wall | add offset |

Reference: `clock_gettime(2)`: https://man7.org/linux/man-pages/man2/clock_gettime.2.html **[Verified-source]**

**Absolute-deadline interfaces (translate virtual → real before calling the real function):**

| Function | Why it matters |
|---|---|
| `sem_timedwait` | Absolute deadline "since the Epoch"; the man page example builds it from `CLOCK_REALTIME`. https://man7.org/linux/man-pages/man3/sem_timedwait.3.html **[Partially verified]** |
| `pthread_mutex_timedlock` | "The timeout shall be based on the CLOCK_REALTIME clock." https://man7.org/linux/man-pages/man3/pthread_mutex_timedlock.3p.html **[Verified-source, POSIX]** |
| `pthread_rwlock_timedrdlock / timedwrlock` | REALTIME deadline. **[Assumption]** |
| `pthread_cond_timedwait` | Uses the condvar's clock attribute (default REALTIME, may be MONOTONIC via `pthread_condattr_setclock`). Shim must track the clock per condvar (intercept `pthread_cond_init`) — as libfaketime does. https://man7.org/linux/man-pages/man3/pthread_cond_timedwait.3p.html **[Verified-source]** |
| `pthread_cond_clockwait`, `sem_clockwait`, `pthread_mutex_clocklock` (glibc ≥ 2.30) | Explicit clock argument: translate only when REALTIME. **[Assumption]** |
| `clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME, …)` | Absolute REALTIME sleep. https://man7.org/linux/man-pages/man2/clock_nanosleep.2.html **[Verified-source]** |
| `timerfd_settime(fd with CLOCK_REALTIME, TFD_TIMER_ABSTIME)` / `timerfd_gettime` | Absolute expiration. https://man7.org/linux/man-pages/man2/timerfd_create.2.html **[Verified-source]** |
| `timer_settime(TIMER_ABSTIME)` on REALTIME POSIX timers | Absolute expiration. **[Verified-source]** |
| `mq_timedreceive / mq_timedsend`, `pthread_timedjoin_np`, `futex(FUTEX_CLOCK_REALTIME)` via raw syscall | Lower priority; raw futex not interceptable. **[Assumption]** |

**Relative-timeout interfaces need no translation:** `nanosleep`, `usleep`, `sleep`, `poll`, `epoll_wait`, `select`, relative timerfd, `setitimer`. They are measured by the kernel, not by the app's notion of wall time. **[Verified-source]**

### 3.1.3 Virtual wall-clock time vs virtual monotonic time

* **Wall-clock (`CLOCK_REALTIME`)**: "what time is it" — used for timestamps, TTL arithmetic, `NOW()`, cron matching, token expiry. Can jump (NTP, admin). Programs are *expected* to tolerate jumps.
* **Monotonic (`CLOCK_MONOTONIC`)**: "how much time passed" — used for timers, timeouts, rate limiting, event loops, mutex waits, connection keepalives. Must never go backward; programs assume it advances at about real rate.

**Can we advance wall time while leaving monotonic unchanged and expect scheduled callbacks to fire?**

**No, not in general.** **[Verified-source / Assumption per library]**

* Node `setTimeout`/`setInterval` are libuv timers keyed on `uv_now()`, which comes from `uv__hrtime(UV_CLOCK_FAST)` → `CLOCK_MONOTONIC` (or `CLOCK_MONOTONIC_COARSE` when its resolution ≤ 1 ms). The event loop blocks in `epoll_wait` with a relative timeout. A 24 h timer armed before a 14-day wall-clock jump still fires 24 real hours later. https://github.com/libuv/libuv/blob/v1.x/src/unix/linux.c (`uv__hrtime`) **[Verified-source]**
* Python `asyncio` uses `loop.time()` = `time.monotonic()`. https://docs.python.org/3/library/asyncio-eventloop.html#asyncio.loop.time **[Verified-source]**
* JVM `ScheduledExecutorService` uses `System.nanoTime()` (monotonic). **[Verified-source]**
* Redis provides a monotonic clock (`getMonotonicUs` → `clock_gettime(CLOCK_MONOTONIC)`) in `monotonic.c` **[Verified-source]** https://github.com/redis/redis/blob/7.2/src/monotonic.c; its use for event-loop timers (`ae.c`) is **[Assumption]** (not opened).

**What does work:** schedulers that *re-read wall time* on a bounded polling interval. After a jump, they notice within one polling interval. Examples (see §3.5): Celery beat (bounded by `beat_max_loop_interval`), Sidekiq scheduled-set poller, BullMQ worker (bounded by its drain/marker wait), pg_cron (per-tick loop). Schedulers that compute `delay = next_run − now()` once and then sleep the full delay (node-cron style, Spring `@Scheduled(cron)`, APScheduler between wakeups) will **not** notice until that delay expires.

**Why not virtualize monotonic too?**

1. Jumping monotonic forward does not wake already-sleeping timers: the kernel's relative `epoll_wait`/`futex` timeouts are real. You would also have to intercept every blocking call — infeasible and fragile.
2. Monotonic jumps break assumptions everywhere: connection pools mass-expire, keepalives fire, rate limiters reset, Java GC ergonomics and JIT heuristics misbehave **[Assumption]**. libfaketime's README states: "(FAKETIME_DONT_FAKE_MONOTONIC). Without this argument the java command usually hangs." https://github.com/wolfcw/libfaketime/blob/master/README **[Verified-source]**
3. Kernel time namespaces only offset MONOTONIC/BOOTTIME (and only at namespace creation). https://man7.org/linux/man-pages/man7/time_namespaces.7.html **[Verified-source]**

**[Recommendation]** Never virtualize monotonic clocks in v0.1. Make scheduled work happen through:

* **Polling schedulers** — supported with a measured per-library "minimum dwell".
* **Triggers** — `chronostack.yml` declares commands/HTTP calls executed at each step (deterministic, any language).
* **SDK adapters** — opt-in in-process integrations (Node first).

And make advancement *acknowledged*: after each step, wait on a **barrier** (SQL/Redis/HTTP condition) rather than a fixed sleep.

**Application-level clock adapter vs system interception.** An injected `Clock` abstraction (e.g. `java.time.Clock`, Go `benbjohnson/clock`, Temporal's time-skipping test server) is the *only* way to make timers fire deterministically. But it requires code changes, which contradicts "zero changes". **[Recommendation]**: system interception for *wall time* (the 80 % case: timestamps, TTLs, expiry comparisons), plus triggers/SDKs for *scheduling*. Be explicit about this split in docs and CLI output.

### 3.1.4 musl vs glibc

* Both support `LD_PRELOAD` for dynamically linked programs. **[Verified-source]**
* A shim linked against glibc will not load in an Alpine (musl) image and vice versa (missing `libc.so.6` / symbol versions). → Build four artifacts: `{x86_64, aarch64} × {gnu, musl}`. **[Assumption → M0]**
* musl has no symbol versioning; glibc's `pthread_cond_timedwait` has two versions (`GLIBC_2.2.5` and `GLIBC_2.3.2` on x86_64). The shim must interpose the default version and preserve ABI. libfaketime has handled this historically. **[Assumption]**
* glibc 2.34 merged libpthread into libc; interposition still works, but symbol lookup via `dlsym(RTLD_NEXT, …)` must target the correct version (`dlvsym`). **[Assumption → M0]**

### 3.1.5 Feasibility verdict

Wall-clock virtualization: **feasible** (high confidence). Deadline translation: **feasible but must be proven** (medium confidence; libfaketime is an existence proof with a history of bugs). Timer execution via clock change alone: **not feasible**; requires triggers/barriers/SDKs.

---

## 3.2 Shared clock architecture

| Option | Linux native | Docker Desktop macOS/Windows | Verdict |
|---|---|---|---|
| A. Host file (`/dev/shm/...`) bind-mounted, host CLI writes via mmap | Works (same kernel, same page cache) | **Unreliable**: bind mounts cross the VM boundary (virtiofs / gRPC-FUSE / 9p on WSL2). Host-side mmap writes have no coherence guarantee with in-VM mmaps; changes may surface late or only after revalidation. | **Reject** |
| B. Docker named volume, host CLI writes via `docker exec`/`docker cp` | Works | Works (volume lives in VM) but writes from host are slow (process spawn per write) | Partial |
| C. Controller container + Unix socket only (shim asks controller over socket) | Works | Works | **Reject** for reads: a socket round-trip per `gettimeofday` is far too slow (Postgres calls it many times per query) |
| **D. Controller container owns a mmap page on a named volume; host talks to controller via HTTP API** | Works | Works — all containers share the VM kernel and page cache | **Recommend** |
| E. Dedicated daemon on host + client adapters | Works | Same VM boundary problem as A for the shim | Reject for v0.1 |

**[Recommendation] Option D.**

* `chronostack-clockd` container mounts named volume `chronostack-<project>-clock` read-write at `/chronostack`; target services mount it **read-only**.
* Volume type: **default = disk-backed local named volume** (Docker docs: "A given volume can be mounted into multiple containers simultaneously." https://docs.docker.com/engine/storage/volumes/ **[Verified-source]**). mmap coherence between containers relies on all of them sharing one kernel page cache **[Assumption, strongly expected → M0]**. A tmpfs-backed volume (`--opt type=tmpfs`) is not documented on that page and is an optional M0 experiment only **[Assumption]**.
* Docker Desktop for Mac shares host files into the VM using VirtioFS or gRPC FUSE (https://docs.docker.com/desktop/settings-and-maintenance/settings/ **[Verified-source]**); keeping the clock page on a VM-local volume avoids depending on mmap semantics across that boundary.
* Shim and controller binary (`.so` files) are delivered on the same volume (controller image copies them in at start) — no host file mounts needed at all. This removes macOS file-sharing configuration problems.

**Memory layout and ordering.** The original plan used separate `AtomicI64 base_real_ns` and `base_virtual_ns`. A reader may load `base_real` from update N and `base_virtual` from update N+1 → a torn, wrong timestamp. **[Verified design flaw]**

Recommended page (64-byte aligned, versioned):

```
struct chronostack_clock_v1 {        // little-endian, naturally aligned
    u64 magic;            // "CHRONOST"
    u32 layout_version;   // 1
    u32 page_size;        // sizeof struct, for forward compatibility
    u64 seq;              // seqlock counter: odd = write in progress
    i64 offset_ns;        // virtual = real(REALTIME) + offset   (when not frozen)
    i64 frozen_virtual_ns;// valid when frozen == 1
    u32 frozen;
    u32 flags;            // reserved
    u64 generation;       // increments on every committed change (for acks/logging)
    u64 session_id;       // random per `chronostack up`; detects stale pages
};
```

Writer (single: controller): `seq++` (odd, release) → write fields → `seq++` (even, release). Reader (shim): load `seq` (acquire); if odd retry; read fields; acquire fence; reload `seq`; retry if changed. Bounded retries (e.g. 64) then fall back to last good value. **[Verified pattern — Linux kernel seqlock]**

Why offset rather than `base_real/base_virtual` pairs: both containers and the controller read `CLOCK_REALTIME` from the *same VM kernel*, so a single offset is sufficient and needs only one value to be consistent.

**Acknowledgement.** Readers do not cache; every call reads the page, so a committed update is visible to all processes on their next clock read (subject to the seqlock). Visibility is verified by **probes** (§8 in ARCHITECTURE.md), not by per-process acks.

**Multiple sessions.** Volume name, controller container name and HTTP port include the Compose project name → independent sessions do not collide. `session_id` detects a stale volume from a crashed run.

**Crash recovery.** If the controller dies, the page persists; time continues to flow at real rate with the last offset. On restart the controller reattaches and verifies `session_id`. `chronostack down` removes the volume (or `--keep-clock`).

**Reset.** `chronostack reset` writes `offset = 0, frozen = 0`. Because target services may have persisted future timestamps (Redis absolute expiries, Postgres rows), moving time backward is **not supported** in v0.1 except via full `down -v` + `up`. **[Recommendation]**

---

## 3.3 PostgreSQL 16

How Postgres reads time: `clock_timestamp()` returns `GetCurrentTimestamp()` and `now()` returns `GetCurrentTransactionStartTimestamp()` **[Verified-source]**; `GetCurrentTimestamp()` calls `gettimeofday()` **[Partially verified — see SOURCE_VERIFICATION §2]**. https://github.com/postgres/postgres/blob/REL_16_STABLE/src/backend/utils/adt/timestamp.c Elapsed-time instrumentation (`instr_time`) uses `CLOCK_MONOTONIC`. https://github.com/postgres/postgres/blob/REL_16_STABLE/src/include/portability/instr_time.h **[Verified-source]**

Function semantics (https://www.postgresql.org/docs/16/functions-datetime.html#FUNCTIONS-DATETIME-CURRENT) **[Verified-source]**:

| Function | Value | Expected under ChronoStack |
|---|---|---|
| `now()`, `CURRENT_TIMESTAMP`, `transaction_timestamp()` | Start of current **transaction** (cached) | Reflects virtual time **at transaction start**. A transaction opened before an advance keeps the old value. |
| `statement_timestamp()` | Start of current statement | Reflects virtual time at statement start. |
| `clock_timestamp()` | Live | Reflects virtual time immediately. Best probe. |
| `timeofday()` | Live, text | Reflects virtual time immediately. |
| `CURRENT_DATE`, `LOCALTIMESTAMP` | Derived from transaction start | As `now()`. |

Background behaviour:

| Area | Expectation | Label |
|---|---|---|
| Autovacuum / checkpoints / stats | Use timestamps for logging/thresholds; constant offset harmless; a 14-day jump may trigger "time since last" heuristics (e.g. immediate autovacuum consideration, checkpoint_timeout comparisons use monotonic or wall?) | **[Unresolved]** — observe in M0 |
| `statement_timeout`, `lock_timeout`, `idle_in_transaction_session_timeout` | `timeout.c` stores `fin_time` from `GetCurrentTimestamp()` and arms `setitimer` with a relative interval. With a constant offset, deltas are correct. **If an advance happens while a timeout is pending**, the handler compares `now >= fin_time` with the new virtual `now` and fires early. https://github.com/postgres/postgres/blob/REL_16_STABLE/src/backend/utils/misc/timeout.c | **[Verified-source]** |
| Latches / `WaitLatch` | Relative timeouts → unaffected | **[Verified-source]** |
| Connection-level `authentication_timeout` | Same as timeout.c | **[Verified-source]** |
| pg_cron | Launcher loop reads `GetCurrentTimestamp()` (virtualized) and blocks ≤ `MaxWait = 1000` ms. `StartAllPendingRuns` "use[s] Vixie cron logic for clock jumps": **> 3 h forward → `CLOCK_CHANGE`, only the current minute is evaluated (intermediate runs skipped)**; 5 min–3 h → `CLOCK_JUMP_FORWARD`, fixed-time jobs caught up per missed minute, wildcard jobs once; 1–5 min → full catch-up. `cron.timezone` defaults to GMT. https://github.com/citusdata/pg_cron/blob/main/src/pg_cron.c **[Verified-source]** (MINUTE_COUNT=60 assumed). **Consequence: ChronoStack must sub-step advances to ≤ 1 h when pg_cron is present (ADR-005).** |
| Replication / WAL timestamps | Virtual timestamps written into WAL; harmless for single-node dev. | **[Assumption]** |
| `initdb` / entrypoint | `docker-entrypoint.sh` runs `initdb` and `gosu postgres`; gosu is not setuid → `LD_PRELOAD` inherited. | **[Assumption → M0]** |

**Integration test matrix (M0/M1):**

| # | Test | Pass criterion |
|---|---|---|
| PG-1 | `SELECT clock_timestamp()` after `advance 7d` | within ±1 s of controller virtual time |
| PG-2 | `now()` inside a transaction opened *before* advance | equals pre-advance value (documents caching) |
| PG-3 | `now()` in new transaction after advance | ≈ virtual time |
| PG-4 | `INSERT … DEFAULT now()` then `advance 14d` then `WHERE created_at < now() - interval '14 days'` | row matched |
| PG-5 | `SET statement_timeout='2s'; SELECT pg_sleep(1)` under +14 d offset | completes, no timeout |
| PG-6 | `pg_sleep(5)` while another client advances 1 d | **documents** early timeout behaviour with statement_timeout set |
| PG-7 | pg_cron job `* * * * *` → `advance 1h --step 1m` | 60 runs |
| PG-8 | pg_cron daily job `0 3 * * *` → `advance 14d` as one raw jump (sub-stepping disabled) | ≤ 1 run (expected per source: skipped unless landing on 03:00) — documents the hazard |
| PG-9 | pg_cron daily job → `advance 14d --step 1h` with barrier | 14 runs |
| PG-10 | `postgres:16` and `postgres:16-alpine` | both pass PG-1..5 |
| PG-11 | Restart container with shim, existing data dir | starts, timestamps consistent |

---

## 3.4 Redis 7

How Redis reads time: `ustime()` → `gettimeofday()`, `mstime()` = `ustime()/1000` **[Verified-source]**; command execution uses a cached `commandTimeSnapshot()` **[Partially verified]**; active expiry uses `data.now = mstime()` and `if (now > t)` (`expire.c`) **[Verified-source]**. The official EXPIRE docs state expiries are absolute Unix timestamps and that running instances "always check the computer clock" — moving the clock ahead expires keys **[Verified-source]**. https://github.com/redis/redis/blob/7.2/src/server.c, https://github.com/redis/redis/blob/7.2/src/expire.c

Expiry semantics (https://redis.io/docs/latest/commands/expire/ — "How Redis expires keys") **[Verified-source]**:

* Expiry is stored as an **absolute Unix ms timestamp** (also persisted that way in RDB/AOF).
* **Lazy expiration**: on access, key is expired if `now > expire_at`.
* **Active expiration**: `activeExpireCycle` in `serverCron` (runs `hz` times/s, default 10) samples volatile keys and deletes expired ones. https://github.com/redis/redis/blob/7.2/src/expire.c

| Command | Expected | Label |
|---|---|---|
| `SET k v EX 86400` then `advance 25h`, `GET k` | nil (lazy expiry) | **[Assumption, high confidence]** |
| `TTL` / `PTTL` | `expire_at − virtual_now` | **[Assumption, high confidence]** |
| `EXPIREAT k <virtual ts>` | interpreted against virtual now | **[Assumption, high confidence]** |
| `TIME` | virtual time | **[Assumption, high confidence]** |
| `DBSIZE` / keyspace events after advance without access | keys removed within ~100 ms–seconds by active cycle; `expired` keyspace notifications fire | **[Assumption → M0]** |
| Restart Redis *without* shim after virtual-future writes | keys with virtual-future expiry live much longer than intended | **[Verified reasoning]** — documented limitation |
| Restart *with* shim, same offset | consistent | **[Assumption]** |
| Replica / `WAIT` | out of scope (single node) | — |

**Verdict:** Redis expiry works with wall-clock virtualization (high confidence). Keyspace-notification-driven workers will see expirations after an advance, which is actually helpful.

---

## 3.5 Application runtimes

### Summary matrix

| Runtime | Wall clock via LD_PRELOAD | Timers fire on advance? | Deadline-hang risk | SDK needed? | v0.1 status |
|---|---|---|---|---|---|
| Node.js 20/22 (glibc, musl) | Yes — `Date.now()` via V8 → libc **[Assumption → M0]** | **No** (`setTimeout` is monotonic) **[Verified-source]** | Low (libuv uses relative timeouts) **[Assumption]** | For schedulers: trigger or SDK | **Supported (wall clock)** |
| Node.js native on macOS/Windows host | No (no `LD_PRELOAD`; macOS `DYLD_INSERT_LIBRARIES` restricted by SIP for system binaries) | No | — | **Yes**: `-r chronostack/register` patches `Date` | **v0.1 late (Node register)** |
| Python 3.11+ (glibc) | Yes — `time.time()` uses `clock_gettime(CLOCK_REALTIME)` https://docs.python.org/3/library/time.html **[Verified-source]** | No (`asyncio` uses `time.monotonic()` **[Verified-source]**) | Believed low on 3.11+ (monotonic lock timeouts); the threading docs do **not** state the clock **[Assumption → NH-PY1]**; background: https://github.com/python/cpython/issues/67616 (not opened) | For schedulers | **Supported target (wall clock), gated on NH-PY1** |
| Python < 3.11 | Yes | No | **High**: lock timeouts may use `sem_timedwait` with REALTIME deadlines → hang without translation **[Assumption → M0]** | — | Experimental |
| JVM 17/21 | Yes — `currentTimeMillis` / `Instant.now` via libc **[Assumption → M0]** | No (`ScheduledExecutorService` is nanoTime-based) | Medium (libfaketime history of Java hangs) **[Search-confirmed]** | For Spring `@Scheduled(cron)`/Quartz | Experimental |
| Ruby 3.x | Yes — `Time.now` → `clock_gettime(REALTIME)` **[Assumption]** | Depends: Sidekiq poller re-reads wall time on a poll interval **[Verified-source]** https://github.com/sidekiq/sidekiq/wiki/Scheduled-Jobs | Low **[Assumption]** | For sidekiq-cron etc. | Experimental |
| Go | **No** — runtime calls vDSO directly | No | — | Only app-level clock injection | **Unsupported** |
| Static binaries / distroless static | **No** | No | — | — | **Unsupported** |

### Scheduler libraries

| Library | How it waits | Notices wall jump? | ChronoStack approach | Label |
|---|---|---|---|---|
| BullMQ (delayed jobs, job schedulers) | Public API takes a **relative** `delay` (ms) and places the job in a "delayed set"; docs say exact-time execution is not guaranteed https://docs.bullmq.io/guide/jobs/delayed **[Verified-source]**. Internal score encoding and worker wake bound (`drainDelay`) are **not documented** on the opened pages. | Unknown — worker may sleep until its previously computed wake time | Trigger (`queue.promoteJobs()` / admin endpoint) **or** measured dwell | **[Unresolved → read source + SCH-BULL-1 in M0]** |
| node-cron / croner | `setTimeout` to next match | **No** until timeout expires | Trigger or Node SDK `onAdvance` re-arm | **[Assumption → M0]** |
| Celery beat | Sleeps ≤ `beat_max_loop_interval` then re-checks schedule. Setting default is 0 = scheduler decides: 300 s for the default scheduler, 5 s for django-celery-beat. https://docs.celeryq.dev/en/stable/userguide/configuration.html#beat-max-loop-interval | Yes, within the interval (set to 1 s in dev) | Recommend `beat_max_loop_interval=1` + dwell; or trigger | **[Verified-source]** |
| APScheduler 3.x | Computes wait to next run, waits on event | **No** until wait expires (unless `wakeup()`) | Trigger | **[Assumption → M0]** |
| pg_cron | ≤ 1 s loop on wall time; Vixie jump logic (§3.3) | Yes for steps ≤ 3 h (fixed-time jobs) / ≤ 5 min (all jobs); **skips** on larger jumps | Automatic sub-stepping (≤ 1 h) + barrier | **[Verified-source]** |
| Sidekiq scheduled/retry sets | Polls "approximately every 5 seconds by default" https://github.com/sidekiq/sidekiq/wiki/Scheduled-Jobs | Yes within the interval | dwell ≥ poll interval or trigger | **[Verified-source]** |
| Spring `@Scheduled(cron)` | Delay from wall clock, scheduled on monotonic executor | **No** | Trigger | **[Assumption]** |
| Kubernetes CronJob / host cron | Not in compose scope | — | — | — |

### Runtime conclusions

* **Do not claim "Node supported" because `Date.now()` moves.** Support is per (runtime × scheduler) row and must be demonstrated by a scheduler execution test.
* v0.1 targets **Node and Python** as first-class wall-clock runtimes, Postgres and Redis as first-class data stores, and **triggers + barriers** as the universal scheduling mechanism.

---

## 3.6 Docker and cross-platform

| Topic | Finding | Recommendation |
|---|---|---|
| Override merging | Compose merges multiple `-f` files; `environment` maps merge by key, `volumes` merge by target path. https://docs.docker.com/compose/how-tos/multiple-compose-files/merge/ **[Verified-source]** | Generate `.chronostack/compose.override.yml`; never edit user files. |
| Parsing the project | `docker compose config --format json` returns the fully resolved model (interpolation, `extends`, profiles, multiple files). https://docs.docker.com/reference/cli/docker/compose/config/ **[Verified-source]** | Use this instead of `serde_yaml` on raw files. |
| Existing `LD_PRELOAD` | An override replaces the key value. | Read resolved env; emit `LD_PRELOAD=/chronostack/lib…so:<existing>`. Image-level `ENV LD_PRELOAD` (e.g. jemalloc) must be read via `docker image inspect`. |
| Entrypoints | `LD_PRELOAD` is inherited across `exec` and by child processes; shells, `gosu`, `tini` all load it. | Fine; shim must be tiny and safe in every process (`bash`, `initdb`, `pg_ctl`). |
| Architecture | Image arch ≠ host arch possible (`platform: linux/amd64` under emulation on Apple Silicon). | Detect per service from `docker image inspect` (`Architecture`). Under QEMU/Rosetta emulation, an amd64 shim loads in the emulated process. **[Assumption → M0]** |
| libc detection | Image may be glibc, musl, or none (distroless static / scratch). | Probe: `docker run --rm --entrypoint "" <image> ls /lib/ld-musl-*` vs `/lib*/ld-linux-*`; cache per image digest. If neither → mark service "not virtualizable", warn, continue. |
| Read-only root FS | Shim is on a mounted volume, so `read_only: true` is fine. | Mount volume read-only into services. |
| Privileges | None required. ChronoStack never needs `CAP_SYS_TIME`, `privileged`, or host PID. | Enforce. |
| Networking (Docker Desktop) | Host → container via published ports on 127.0.0.1 works on all platforms. | Controller publishes HTTP API on `127.0.0.1:<random>`. |
| macOS ARM64 | Docker Desktop / OrbStack / Colima all run a Linux VM; same model applies. | Test on Docker Desktop and OrbStack. **[Assumption]** |
| Windows | Docker Desktop (WSL2). Host CLI only talks HTTP and `docker` CLI → works. | Windows host CLI binary. Native Windows app processes cannot be virtualized except via SDK. |
| Podman | Compose compatibility varies. | Out of scope for v0.1. |

**Unavoidable limitations:** static binaries, Go, setuid binaries, processes started outside Compose, services on remote Docker hosts (volume semantics fine, but host CLI port forwarding may differ), and external SaaS dependencies (e.g. Stripe itself — combine with Stripe Test Clocks).

---

## 3.7 Existing alternatives

See [COMPETITIVE_ANALYSIS.md](COMPETITIVE_ANALYSIS.md).

---

## Negative tests (expected failures, must exist in CI)

| # | Test | Expected |
|---|---|---|
| NEG-1 | Go binary `time.Now()` under shim | Real time (documents unsupported) |
| NEG-2 | Static musl binary | Real time |
| NEG-3 | `setTimeout(fn, 60_000)` + `advance 1h` | `fn` **not** executed before ~60 s real |
| NEG-4 | node-cron daily job + `advance 1d` without trigger | job **not** executed within dwell |
| NEG-5 | Postgres `now()` in transaction opened before advance | old value |
| NEG-6 | Shim missing volume (`CHRONOSTACK_STRICT` unset) | real time + one stderr warning |
| NEG-7 | Same with `CHRONOSTACK_STRICT=1` | process aborts with clear message |
| NEG-8 | Raw `syscall(SYS_clock_gettime)` | real time |
| NEG-9 | `advance -1d` | rejected by CLI |

---

## Unresolved questions (to close in M0)

1. ~~Does pg_cron implement Vixie cron's >3 h jump "correction" logic?~~ **Resolved (source-verified): yes.** Remaining: confirm `MINUTE_COUNT = 60` and observe behaviour experimentally (PG-7..9).
2. Exact BullMQ worker wake bound after a wall-clock jump (v5 marker logic) — not in public docs; read source.
3. Which CPython versions use REALTIME deadlines for `Lock.acquire(timeout)` on glibc/musl in official images?
4. Does a tmpfs-backed local named volume share one mount across containers on Docker Desktop?
5. Behaviour of an amd64 shim inside Rosetta-emulated containers on Apple Silicon.
6. Whether glibc-internal time reads affect anything user-visible (e.g. `pthread_cond_timedwait` internals, `getaddrinfo` timeouts).
7. JVM: does HotSpot use REALTIME condvars anywhere (`Object.wait` was moved to monotonic in JDK 8u/9)?
8. Postgres autovacuum/checkpointer reaction to a large forward jump.
