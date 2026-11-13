# M0 feasibility spike

**Throwaway code.** The purpose of this directory is to prove or disprove the hardest assumptions in [docs/IMPLEMENTATION_PLAN.md § M0](../docs/IMPLEMENTATION_PLAN.md#m0--feasibility-spike-weeks-12) before any product code is written. Results are in [docs/M0_RESULTS.md](../docs/M0_RESULTS.md).

## What's here

| Path | What |
|---|---|
| `shim/chronoclock.c` | Preload library (C, libc only). It virtualizes wall-clock reads and converts absolute `CLOCK_REALTIME` deadlines back to real time. |
| `shim/chronoclock.h` | Clock page layout v1 (seqlock) |
| `shim/chronoctl.c` | Clock writer; stands in for the future Rust `clockd` |
| `tests/c/cstest.c` | C probes: every clock API, the no-hang suite, a cross-container stress test, a benchmark |
| `tests/{node,python,java}` | Runtime probes: wall-clock read plus wait primitives |
| `tests/go`, `tests/c/staticnow.c` | Negative tests (cannot be intercepted) |
| `docker/` | Multi-stage build: glibc shim built on Debian bookworm (glibc 2.36), musl shim built on Alpine |
| `compose.yml` | Hand-written stack: clockd, Postgres 16 (+ pg_cron) and 16-alpine, Redis 7 and 7-alpine, Node 22, Python 3.10/3.12/3.12-alpine, Temurin 21, tool containers |
| `run_m0.py` | Harness. Writes `results/m0-results.{json,md}`. |

## Run

Requires Docker with Compose v2 and Python 3.8+.

```sh
docker compose -f spike/compose.yml -p cs-m0 build
python spike/run_m0.py                      # all sections, then tears down
python spike/run_m0.py --only reads,redis   # subset
python spike/run_m0.py --keep               # leave the stack running
```

The `arm64` section builds the aarch64 images with `docker buildx` and runs them under QEMU emulation. That needs binfmt support, which Docker Desktop includes.
