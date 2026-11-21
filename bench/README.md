# Benchmarks

These are reproducible measurements behind the performance numbers in the README. Every number published from here links to a results folder with the raw samples and the environment it was measured on.

## Results

| Folder | Machine | B1 | B2 | B3 |
|---|---|---|---|---|
| [2026-11-18-windows11-wsl2-ryzen7840hs](results/2026-11-18-windows11-wsl2-ryzen7840hs/README.md) | Windows 11 laptop, Docker Desktop (WSL2), AMD Ryzen 7 7840HS | ✅ | ✅ | |
| [2026-11-18-github-ubuntu-latest](results/2026-11-18-github-ubuntu-latest/README.md) | GitHub-hosted runner, x86_64, 4 vCPUs, Intel Xeon Platinum 8573C | ✅ | ✅ | |
| [2026-11-18-github-ubuntu-24.04-arm](results/2026-11-18-github-ubuntu-24.04-arm/README.md) | GitHub-hosted runner, arm64, 4 vCPUs, Arm Neoverse N2 (CPU part `0xd49`) | ✅ | ✅ | |
| [2026-11-21-windows11-wsl2-ryzen7840hs](results/2026-11-21-windows11-wsl2-ryzen7840hs/README.md) | Windows 11 laptop, Docker Desktop (WSL2), AMD Ryzen 7 7840HS | | | ✅ |
| [2026-11-21-github-ubuntu-latest](results/2026-11-21-github-ubuntu-latest/README.md) | GitHub-hosted runner, x86_64, 4 vCPUs, Intel Xeon Platinum 8573C | | | ✅ |
| [2026-11-21-github-ubuntu-24.04-arm](results/2026-11-21-github-ubuntu-24.04-arm/README.md) | GitHub-hosted runner, arm64, 4 vCPUs, Arm Neoverse N2 (CPU part `0xd49`) | | | ✅ |

**Summary across the three machines** (median overhead per wall-clock read, compared with native in the same container):

| Runtime | ChronoStack | libfaketime (cached) | libfaketime (file, no cache) |
|---|---|---|---|
| C | +12 to +16 ns | +41 to +60 ns | +3.7 to +14 µs |
| Python | +10 to +26 ns | +40 to +71 ns | +3.7 to +14 µs |
| Node | +12 to +22 ns | +44 to +68 ns | +3.8 to +15 µs |

`advance 1h` without probes takes 3 ms on the Linux runners and 28 ms under Docker Desktop. With probes it takes 0.15–0.56 s for one service and 0.64–1.16 s for 20.

`chronostack users run` with 200 users against a local echo server handles about 8,000 (laptop, Docker Desktop), 10,700 (arm64 runner) and 24,000 (x86_64 runner) requests per second at `max_parallel` 32, with a median request latency of 0.8–3.6 ms. At `max_parallel` 200 the rate stays about the same and latency goes up, as requests queue at the server.

Runs on GitHub's Linux runners (x86_64 and arm64) can be started from the **Benchmarks** workflow (`.github/workflows/bench.yml`). Each run uploads its results folder as an artifact.

## B1: clock-read overhead

**What it measures:** how many nanoseconds one wall-clock read costs in three runtimes:
- **C:** `clock_gettime(CLOCK_REALTIME)`.
- **Python:** `time.time()`.
- **Node:** `Date.now()`.

**Configurations** (all in the same container; only the preload changes between runs):

| Configuration | How |
|---|---|
| Native | `LD_PRELOAD=` (nothing preloaded) |
| ChronoStack | The service as `chronostack up` starts it, with the clock moved +2 days |
| libfaketime (cached) | `LD_PRELOAD=libfaketime.so.1 FAKETIME=+1d`. This is libfaketime's default mode. Settings are re-read about every 10 s, so a change takes up to 10 s to apply. |
| libfaketime (file, no cache) | `FAKETIME_TIMESTAMP_FILE` + `FAKETIME_NO_CACHE=1`. This is how libfaketime documents changing time in a running process immediately, which is what ChronoStack's shared clock page does. |

**Method:**
- **Timing loops:**
  - C uses `CLOCK_MONOTONIC_RAW` through the raw syscall, so no preload can affect the timer.
  - Python uses `perf_counter_ns` and subtracts an empty loop of the same length.
  - Node uses `process.hrtime.bigint()`, and the result includes the loop overhead.
- **Iterations:** 10 million (C, Node) or 2 million (Python) per run. The file-backed libfaketime configuration uses 100,000–200,000, because each call reads a file.
- **Verification:** each run also prints the time it read. The harness checks it against the expected offset: about 0 for native, +2 days for ChronoStack, +1 day for libfaketime. If a configuration didn't take effect, the harness fails rather than recording a misleading number.
- **Repetitions:** 10 runs per configuration. The order of configurations rotates each repetition to spread out drift. The tables report the median and p10–p90.

**Caveats:**
- Absolute nanoseconds depend heavily on the machine and on how the VM exposes clocks. Docker Desktop's WSL2 kernel has a slower native `clock_gettime` than bare-metal Linux. Compare configurations *within* one results folder, not across machines.
- libfaketime is the version Debian bookworm ships (0.9.10). Upstream is newer.
- Only the cost of reading the clock is measured. Deadline translation for timed waits is tested for correctness by the no-hang suite (`tests/e2e/run_nohang.py`), not timed here.

## B2: advance latency

**What it measures:** the real time `chronostack advance 1h` takes on an idle stack of 1, 5 or 20 virtualized `debian:bookworm-slim` services, with no triggers or barriers.
- **With probes** (the default): every service's clock is checked after the move.
- **Without probes:** `--no-probe`.

**Method:**
- One warm-up advance per stack.
- 10 timed advances per configuration.
- Each one is checked to have moved virtual time by one hour.

The results show the floor of the advance cost. Real projects add the time their triggers and barriers take.

## B3: simulated users

**What it measures:** how fast `chronostack users run` drives requests, and the request latency it reports. 200 users each make 20 GET requests (4,000 per run) to `traefik/whoami`, a tiny Go server, published on 127.0.0.1. It runs at `max_parallel` 32 and 200.

**Method:**
- One warm-up run per configuration.
- 10 timed runs. Each must finish with every request made and none failed.
- Requests per second = 4,000 / the run's real time, including starting the worker threads.
- Latency is the engine's own per-request measurement: the p50 and p99 of each run, then the median across runs.

**What it doesn't show:** the server does almost nothing and the requests stay on one machine, so this is a ceiling for the engine, not a load-testing result. Real applications are slower than whoami. The two runs on GitHub's shared runners gave 19,400 and 24,000 requests per second on x86_64; compare configurations within one run.

## Running it

**Requirements:** Docker with Compose v2 and Python 3.

```sh
docker build -f docker/clockd.Dockerfile -t chronostack/clockd:dev .
cargo build --release -p chronostack
python bench/run_bench.py --label my-machine            # all benchmarks, 10 repetitions
python bench/run_bench.py --only b1 --reps 20           # one benchmark, more repetitions
```

Results are written to `bench/results/<date>-<label>/`:
- `b1-clock.json`, `b2-advance.json` and `b3-users.json` hold the raw samples, summaries, environment and clock checks.
- `README.md` holds the tables.

A full run takes about 15 minutes on a laptop.
