# Source Verification (second pass)

**Date:** 2026-11-13
**Method:** Every source marked **Opened** below was fetched during this pass and read for the specific claim. Some large source files were truncated by the fetch tool; when part of a finding came from the tool's summary of the unread text and not from verbatim text, the status says so.
**Scope:** Documentation and source code only. **No experiments were run.** Nothing here is a runtime test result.

## Status legend

| Status | Meaning |
|---|---|
| **Verified** | The opened primary source states the claim (verbatim text or code). |
| **Partially verified** | The source supports part of the claim, or support came from the tool's summary of unread text. |
| **Corrected** | The source contradicts or refines what the first-pass docs said; the docs have been updated. |
| **Not found** | The opened source does not state the claim. The claim is now labeled an assumption. |
| **Not opened** | Cited from knowledge only; still unverified. |

## 1. Linux and POSIX time interfaces

| Claim | Source | Status | Findings | Remaining uncertainty |
|---|---|---|---|---|
| Time namespaces cannot virtualize `CLOCK_REALTIME` | https://man7.org/linux/man-pages/man7/time_namespaces.7.html | **Verified** | "Note that time namespaces do not virtualize the CLOCK_REALTIME clock." They cover MONOTONIC (+COARSE, RAW) and BOOTTIME (+ALARM). | None |
| libc reaches time functions through the vDSO | https://man7.org/linux/man-pages/man7/vdso.7.html | **Partially verified** | "the C library will take care of using any functionality that is available via the vDSO." x86_64 exports `__vdso_clock_gettime`, `__vdso_gettimeofday`, `__vdso_time`, `__vdso_getcpu`. aarch64 exports `__kernel_clock_gettime`, `__kernel_gettimeofday`, `__kernel_clock_getres`, and **no `time`**. | glibc is not named specifically; musl behaviour is not covered. The shim must still interpose the exported `time` symbol on both architectures. |
| `LD_PRELOAD` is ignored for setuid/setgid programs | https://man7.org/linux/man-pages/man8/ld.so.8.html | **Verified** | "In secure-execution mode, preload pathnames containing slashes are ignored." Set-UID preloads load only from standard directories. | None |
| `sem_timedwait` deadline is absolute `CLOCK_REALTIME` | https://man7.org/linux/man-pages/man3/sem_timedwait.3.html | **Partially verified** | The deadline is absolute "since the Epoch"; the page's example builds it with `clock_gettime(CLOCK_REALTIME)`. `sem_clockwait` is not mentioned. | The page states no clock explicitly. Prove the hang and the fix in the M0 test NH-C1. |
| `pthread_mutex_timedlock` uses `CLOCK_REALTIME` | https://man7.org/linux/man-pages/man3/pthread_mutex_timedlock.3p.html | **Verified** (POSIX) | "The timeout shall be based on the CLOCK_REALTIME clock." | The POSIX page notes Linux may differ; confirm with M0 test NH-C2. |
| A condition variable's clock is configurable; the default is "the system clock" | https://man7.org/linux/man-pages/man3/pthread_condattr_getclock.3p.html | **Partially verified** | "The default value of the clock attribute shall refer to the system clock." Only CPU-time clocks are rejected. | `CLOCK_REALTIME` is not named as the default on this page. The shim must track each condition variable's clock (ADR-002). |
| `clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME)` is affected by the realtime clock | https://man7.org/linux/man-pages/man2/clock_nanosleep.2.html | **Verified** | Setting the clock changes when an absolute sleep wakes. If the new value is past the deadline, the call returns immediately. Relative sleeps are unaffected. | How a virtual offset interacts with this API is the subject of M0 test NH-C5. |
| `timerfd` supports `CLOCK_REALTIME` with `TFD_TIMER_ABSTIME` and `TFD_TIMER_CANCEL_ON_SET` | https://man7.org/linux/man-pages/man2/timerfd_create.2.html | **Verified** | Both flags are documented; `CANCEL_ON_SET` makes `read` return `ECANCELED` when the clock is set. | ChronoStack never sets the clock, so `CANCEL_ON_SET` is never triggered. Note this in the shim design. |
| `CLOCK_REALTIME` jumps; `CLOCK_MONOTONIC` does not | https://man7.org/linux/man-pages/man2/clock_gettime.2.html | **Verified** | REALTIME "is affected by discontinuous jumps"; MONOTONIC "is not affected by discontinuous jumps". `CLOCK_TAI` is "settable only indirectly by setting CLOCK_REALTIME". | Whether TAI jumps is not stated. The shim treats TAI as wall-clock time (ADR-001). |

## 2. PostgreSQL 16

| Claim | Source | Status | Findings | Remaining uncertainty |
|---|---|---|---|---|
| `now()` is the transaction start; `clock_timestamp()` is live | https://www.postgresql.org/docs/16/functions-datetime.html (§9.9.5) | **Verified** | `now()` ≡ `transaction_timestamp()` = "start of current transaction". `clock_timestamp()` "returns the actual current time … changes even within a single SQL statement". `timeofday()` is like `clock_timestamp()`. | None |
| `clock_timestamp()` → `GetCurrentTimestamp()`; `now()` → transaction start | https://raw.githubusercontent.com/postgres/postgres/REL_16_STABLE/src/backend/utils/adt/timestamp.c | **Verified** | `PG_RETURN_TIMESTAMPTZ(GetCurrentTimestamp())` in `clock_timestamp`; `GetCurrentTransactionStartTimestamp()` in `now`. | — |
| `GetCurrentTimestamp()` calls `gettimeofday()` | same file | **Partially verified** | The tool's summary reported `gettimeofday(&tp, NULL)` in `GetCurrentTimestamp` and `timeofday`; that part of the file was not read verbatim. | Re-read that section during M0. The behaviour will be confirmed by test PG-1 either way. |
| Timeouts store an absolute `fin_time`, arm a relative `setitimer`, and the handler compares `now >= fin_time` | https://raw.githubusercontent.com/postgres/postgres/REL_16_STABLE/src/backend/utils/misc/timeout.c | **Verified** | `enable_timeout_after`: `now = GetCurrentTimestamp(); fin_time = TimestampTzPlusMilliseconds(now, delay_ms)`. `schedule_alarm`: `TimestampDifference(...)` then `setitimer(ITIMER_REAL, ...)`. `handle_sig_alarm`: `now >= active_timeouts[0]->fin_time`. | This confirms that advancing time during a statement can fire pending timeouts early (PG-6). |
| Elapsed-time instrumentation uses `CLOCK_MONOTONIC` on Linux | https://raw.githubusercontent.com/postgres/postgres/REL_16_STABLE/src/include/portability/instr_time.h | **Verified** | `#elif defined(CLOCK_MONOTONIC)` → `#define PG_INSTR_CLOCK CLOCK_MONOTONIC`; `clock_gettime(PG_INSTR_CLOCK, &tmp)`. | None |
| **pg_cron handles clock jumps the way Vixie cron does** | https://raw.githubusercontent.com/citusdata/pg_cron/main/src/pg_cron.c | **Verified, and it corrects the docs** (it was *Unresolved* in the first pass) | `StartAllPendingRuns`: `/* use Vixie cron logic for clock jumps */`. Behaviour by minutes passed:<br>• **> 3·MINUTE_COUNT** → `CLOCK_CHANGE`: only the **current minute** is evaluated; intermediate runs are skipped.<br>• **> 5** → `CLOCK_JUMP_FORWARD`: every missed minute is checked for **fixed-time** jobs; wildcard jobs run once.<br>• **1–5** → `CLOCK_PROGRESSED`: full catch-up.<br>• Backward < 3 h → `CLOCK_JUMP_BACKWARD`: wildcard jobs only, and `lastMinute` is not advanced.<br>Other details: the main loop blocks at most `MaxWait = 1000` ms; time comes from `GetCurrentTimestamp()`; schedules are matched with `pg_localtime(..., pg_tzset(cron_timezone))`, and `cron.timezone` defaults to `"GMT"`. | `MINUTE_COUNT` is defined in a header that was not opened (assumed 60, so 3 h = 180 min). Part of the file was summarized only. Consequence: **`advance 14d --step 1d` skips pg_cron daily jobs.** Steps must be ≤ 3 h, or ≤ 5 min for full wildcard catch-up. See ADR-005. |

## 3. Redis 7

| Claim | Source | Status | Findings | Remaining uncertainty |
|---|---|---|---|---|
| `ustime()`/`mstime()` use `gettimeofday()` | https://raw.githubusercontent.com/redis/redis/7.2/src/server.c | **Verified** | `long long ustime(void) { … gettimeofday(&tv, NULL);`; `mstime()` returns `ustime()/1000`. | — |
| Command execution uses a cached time snapshot | same | **Partially verified** | `commandTimeSnapshot()` returns `server.cmd_time_snapshot`, "updated before each call in the call() function". | Where the snapshot is assigned was summarized only. |
| Event-loop timers use a monotonic clock | https://raw.githubusercontent.com/redis/redis/7.2/src/monotonic.c | **Partially verified** | `getMonotonicUs_posix` uses `clock_gettime(CLOCK_MONOTONIC)`. The TSC path is off by default. | That the event loop (`ae.c`) uses this clock was **not found** in this file. |
| Active expiry compares wall-clock milliseconds | https://raw.githubusercontent.com/redis/redis/7.2/src/expire.c | **Verified** | `data.now = mstime();` … `if (now > t)` in `activeExpireCycleTryExpire`. `checkAlreadyExpired` uses `commandTimeSnapshot()`. | — |
| Expiry is stored as absolute timestamps, and clock changes affect expiry | https://redis.io/docs/latest/commands/expire/ | **Verified** (latest docs, not 7.2-specific) | "stored as absolute Unix timestamps"; "If you move an RDB file … big desync in their clocks, funny things may happen". Running instances "always check the computer clock", so setting the time 2000 s ahead expires a 1000 s key. | This is the strongest official evidence that virtual wall time drives Redis expiry. |

## 4. Runtimes and schedulers

| Claim | Source | Status | Findings | Remaining uncertainty |
|---|---|---|---|---|
| libuv timers use `CLOCK_MONOTONIC` | https://raw.githubusercontent.com/libuv/libuv/v1.x/src/unix/linux.c | **Verified** (corrects the file: `linux.c`, not `core.c`) | `uv__hrtime`: `clock_id = CLOCK_MONOTONIC`, or `CLOCK_MONOTONIC_COARSE` for `UV_CLOCK_FAST` when its resolution is ≤ 1 ms. | — |
| Node `MockTimers` mocks timers and `Date`, with `tick()` | https://nodejs.org/api/test.html | **Verified** | Supported values: `'setInterval'`, `'setTimeout'`, `'setImmediate'`, `'Date'`. `tick()` "Advances time for all mocked timers." | — |
| Python `time.time()` uses `CLOCK_REALTIME`; `monotonic()` uses `CLOCK_MONOTONIC` | https://docs.python.org/3/library/time.html | **Verified** | "Call `clock_gettime(CLOCK_REALTIME)` if available." / "call `clock_gettime(CLOCK_MONOTONIC)`." | — |
| CPython ≥ 3.11 lock timeouts use a monotonic clock | https://docs.python.org/3/library/threading.html | **Not found** | The threading docs do not describe the clock used by timeouts. | Still an **assumption**. Test NH-PY1 decides it. |
| The Go runtime calls the vDSO directly | https://raw.githubusercontent.com/golang/go/master/src/runtime/vdso_linux_amd64.go | **Partially verified** | The runtime resolves `__vdso_clock_gettime` itself (`vdsoClockgettimeSym`). The libfaketime README also says some runtimes "use vDSO or direct system calls and can bypass" it and mentions golang. | "Not through libc" is inferred, not stated in that file. Negative test NEG-1 confirms behaviour. |
| Celery beat re-checks its schedule at a bounded interval | https://docs.celeryq.dev/en/stable/userguide/configuration.html | **Verified, with a refinement** | `beat_max_loop_interval` "Default: 0", which means the scheduler decides: **300 s** for the default scheduler and **5 s** for django-celery-beat. | — |
| Sidekiq polls the scheduled set about every 5 s | https://github.com/sidekiq/sidekiq/wiki/Scheduled-Jobs | **Verified** | "approximately every 5 seconds by default (15 seconds before Sidekiq 5.1)". | — |
| BullMQ scores delayed jobs with `Date.now()+delay`; drain wait is bounded | https://docs.bullmq.io/guide/jobs/delayed, https://docs.bullmq.io/guide/workers | **Partially verified; `drainDelay` not found** | The public API takes a **relative** `delay` in ms; the docs say execution at the exact time is not guaranteed. Neither page documents `drainDelay`. | The internal score encoding and the worker wake-up bound remain **unresolved**. Read BullMQ source in M0 (SCH-BULL-1). |

## 5. libfaketime

| Claim | Source | Status | Findings | Remaining uncertainty |
|---|---|---|---|---|
| License | https://raw.githubusercontent.com/wolfcw/libfaketime/master/README | **Partially verified** | "released under the GNU General Public License, GPL". The README defers the version to `COPYING`. | `COPYING` was not opened; "GPL-2.0" is unconfirmed, so the docs now say "GPL". |
| `FAKETIME_TIMESTAMP_FILE` exists, with cache controls | same | **Verified** | `FAKETIME_TIMESTAMP_FILE`, `FAKETIME_CACHE_DURATION`, `FAKETIME_NO_CACHE=1`. | — |
| Java hangs unless monotonic faking is disabled | same | **Verified** | "(FAKETIME_DONT_FAKE_MONOTONIC). Without this argument the java command usually hangs." | — |
| libfaketime intercepts timed waits | same | **Partially verified** | It mentions "intercepted functions such as pthread_cond_timedwait()" that can "return with a time-out too early or too late". `sem_timedwait` is not mentioned. | — |
| Static binaries and Go are not supported | same | **Verified** | "cannot work with statically linked binaries"; golang is mentioned. | — |
| Cross-process shared state | same | **Corrected** | It uses "semaphores and shared memory"; the switch is `FAKETIME_DISABLE_SHM`. The name `FAKETIME_SHARED` was **not found**. | — |
| macOS via `DYLD_INSERT_LIBRARIES` | same | **Verified** | — | — |

## 6. Docker and Docker Compose

| Claim | Source | Status | Findings | Remaining uncertainty |
|---|---|---|---|---|
| `environment` merges by key; `volumes` merge by container path | https://docs.docker.com/compose/how-tos/multiple-compose-files/merge/ | **Verified** | Both statements are on the page. | `!reset`/`!override` are not on this page; check the Compose spec if they are needed. |
| `docker compose config --format json` renders the resolved model | https://docs.docker.com/reference/cli/docker/compose/config/ | **Verified** | `--format` accepts `yaml` or `json`; the command "resolves variables … expands short-notation into the canonical format". | Minimum Compose version is not documented here. |
| A named volume can be mounted into multiple containers | https://docs.docker.com/engine/storage/volumes/ | **Verified** | "A given volume can be mounted into multiple containers simultaneously." | — |
| A tmpfs-backed local volume (`--opt type=tmpfs`) is shared across containers | same | **Not found** | No tmpfs example on the page. | **Assumption.** The default is now a disk-backed named volume; tmpfs is an M0 experiment. |
| Docker Desktop for Mac shares host files into the VM through VirtioFS or gRPC FUSE | https://docs.docker.com/desktop/settings-and-maintenance/settings/ | **Verified** | "share files using VirtioFS, or gRPC FUSE" (VirtioFS is the default). | That mmap is incoherent across this boundary is still a **reasoned assumption**. It is one reason, among others, for ADR-004. |

## 7. Tools and competitors

| Claim | Source | Status | Findings | Remaining uncertainty |
|---|---|---|---|---|
| Toxiproxy toxics | https://github.com/Shopify/toxiproxy | **Corrected** | Toxics: latency, bandwidth, slow_close, timeout, reset_peer, slicer, limit_data, **plus packet_loss**. "Down" is not a toxic: a proxy is disabled with `enabled: false`. | — |
| Chaos Mesh TimeChaos patches the vDSO via ptrace | https://chaos-mesh.org/docs/simulate-time-chaos-on-kubernetes/ | **Not found / Corrected** | The page does not describe the mechanism. It shows `clockIds` (default `CLOCK_REALTIME`, can include `CLOCK_MONOTONIC`), Kubernetes CRDs, and: "only affects the PID 1 process … and child processes". | The mechanism claim was removed from the docs. |
| Stripe Test Clocks scope and limits | https://docs.stripe.com/billing/testing/test-clocks | **Verified, with a refinement** | Test clocks affect Billing objects (subscriptions, invoices, customer balances) in **sandboxes** ("doesn't affect live mode"). Each advance can move "up to two subscription intervals at a time". | — |
| MCP uses a stdio transport, JSON-RPC 2.0, and `tools/list`/`tools/call` | https://modelcontextprotocol.io/specification/2025-06-18 | **Verified** | All four are confirmed in the 2025-06-18 spec. The index also lists a newer **2026-07-28** revision (described as stateless). | Implementation must target whichever spec revision is current at M4 (ADR-011). |
| cargo-zigbuild cross-compiles Linux (with glibc version suffix) and macOS targets | https://github.com/rust-cross/cargo-zigbuild | **Verified** | "Currently only Linux and macOS targets are supported"; glibc pinning via `--target x86_64-unknown-linux-gnu.2.17`. | **Windows targets are not supported by zigbuild**: build them on a Windows runner. |
| Rust ecosystem uses `MIT OR Apache-2.0` | https://rust-lang.github.io/api-guidelines/necessities.html | **Verified** | The Rust project is dual-licensed MIT/Apache-2.0, and crates wanting maximum compatibility "are recommended to do the same". | — |
| release-plz / cargo-release | https://github.com/release-plz/release-plz, https://github.com/crate-ci/cargo-release | **Verified** | release-plz automates release PRs, changelogs (git-cliff), and publishing. cargo-release is a local CLI for version, tag, and publish, with dry-run by default. | — |
| Keep a Changelog categories | https://keepachangelog.com/en/1.1.0/ | **Verified** | Added, Changed, Deprecated, Removed, Fixed, Security, plus an `Unreleased` section. | — |
| Contributor Covenant latest version | https://www.contributor-covenant.org/version/3/0/code_of_conduct/ | **Verified** | **3.0** is the latest. It has a reporting placeholder: "[NOTE: describe your means of reporting here.]" | Reporting contact must come from the owner. |
| Dependabot ecosystems `cargo`, `npm`, `github-actions`, `docker` | https://docs.github.com/en/code-security/dependabot/working-with-dependabot/dependabot-options-reference | **Verified** (`cargo` and `github-actions` via the tool's summary) | — | — |
| Discussion forms live at `.github/DISCUSSION_TEMPLATE/<category-slug>.yml` | https://docs.github.com/en/discussions/managing-discussions-for-your-community/creating-discussion-category-forms | **Verified** | The file name must match a category slug. | Categories must exist in repository settings. |

## 8. Sources still not opened

These are knowledge-based only and remain labeled as assumptions in the docs:

- POSIX `sem_timedwait` normative text
- glibc source for `pthread_cond_timedwait` symbol versions and `dlvsym` requirements
- CPython issue #67616 (first-pass web search only)
- libfaketime issue #469
- Temporal time-skipping docs
- Antithesis
- Pumba
- WireMock fault docs
- testcontainers.com
- freezegun and time-machine
- @sinonjs/fake-timers
- Redis `ae.c`
- Postgres autovacuum and checkpointer timing

## 9. Net effect on the plan

1. **pg_cron has moved from unresolved to verified**, and the finding changes the design. A daily pg_cron job is **skipped** by single steps of more than 3 hours. ChronoStack must automatically sub-step large advances (ADR-005 update). The `--step 1d` example is wrong for pg_cron.
2. **Redis expiry under wall-clock shifts is officially documented.** Confidence in Redis support is high.
3. **Postgres timeouts can fire early** if time advances while a statement is running. This is verified from source; advance only between steps, after barriers.
4. **The libfaketime Java hang is confirmed by its own README**, which supports ADR-001 and ADR-002.
5. **Several claims were downgraded or corrected:**
   - Chaos Mesh's mechanism
   - the BullMQ scoring and `drainDelay` details
   - the libfaketime license version and `FAKETIME_SHARED`
   - the tmpfs volume claim
   - the CPython ≥ 3.11 monotonic-lock claim
6. **Windows CLI builds need a native runner**, because cargo-zigbuild does not target Windows.
