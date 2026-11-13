# M2 — Step / trigger / barrier engine + reference demo: results

**Date:** 2026-11-13
**Exit gate** ([IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md#m2--step--trigger--barrier--reference-app-weeks-57)): *10/10 identical runs on Linux and 3/3 on macOS.*

**Status: substantially met, with documented gaps.**

- **Local:** 3/3 identical runs on Docker Desktop (WSL2). Ten local runs were not attempted because the machine is memory-constrained.
- **CI:** one run per push on native Linux x86_64 and arm64.
- **macOS:** untested; no Mac was available.

## What was built

- **`chronostack.yml`** (`crates/cli/src/config.rs`), validated with clear errors, plus a JSON Schema for editor completion ([schema](schema/chronostack.schema.json)). It holds:
  - triggers (`exec` in a service, or `http`), either **scheduled in virtual time** (`schedule: {every: 1d, at: "03:00"}`, UTC) or run after every `--step`;
  - barriers (`sql`, `redis`, `exec`, `http`) with timeouts;
  - `advance` settings (`max_jump`, `dwell`, `barrier_timeout`).
- **Planner** (`crates/core/src/schedule.rs`, pure and unit-tested). It computes every stop before the clock moves:
  - each `--step` end;
  - each trigger occurrence;
  - extra sub-steps so no jump exceeds `max_jump`.
- **Engine** (`crates/cli/src/engine.rs`). At each stop it:
  1. moves the clock to that exact virtual time;
  2. runs the due triggers;
  3. waits on barriers (or `dwell`).

  It streams progress and probes all services at the end. Exit codes: 3 = probe skew, 4 = trigger failed, 5 = barrier timed out.
- **pg_cron awareness.** pg_cron is detected through `shared_preload_libraries`, which sets `max_jump` to 1 h and `dwell` to 1.2 s automatically.
- **CLI additions:**
  - `advance --to <UTC>`;
  - `--step`;
  - `--max-jump auto|none|<dur>`;
  - `--dwell`;
  - `--dry-run`;
  - `chronostack config`.
- **Reference demo** [`examples/saas-trial-demo`](../examples/saas-trial-demo/README.md):
  - Node API and worker, Postgres, Redis;
  - 14-day trial, 24 h sessions, a daily 03:00 job;
  - no ChronoStack-specific code in the app.

**Design refinement:** scheduled triggers make `--step` optional. A plain `advance 14d` runs a daily 03:00 trigger 14 times, each at its own virtual time. `--step` remains for pacing schedulers that poll the clock.

## Results (e2e `tests/e2e/run_m2.py`)

| Check | Result |
|---|---|
| Session (Redis `EX 86400`) after `advance 25h` | Expired (401); trial still active |
| `advance 14d` | **14** trigger runs, all triggers and barriers ok |
| Real time for `advance 14d` | **17.3–17.6 s** (target < 60 s) |
| Trial downgrade time | **Day 14, 03:00:00.69–0.76** virtual. That is the scheduled run plus about 0.7 s of `docker exec` latency. |
| Job runs since day 0 | 15, with exactly 1 downgrade |
| `advance 3d --step 1d` | 3 scheduled runs, 6 stops |
| `--dry-run` | Plan matches execution (one 03:00 stop in 25 h) |
| Determinism | 3/3 runs identical: `('free', day 14 03:00, '15/1')` |
| Demo compose file | Unchanged |

### pg_cron through the product (reproduces M0 with the real CLI)

| Check | Result |
|---|---|
| Detection | `pg_cron` found in `db` → max jump 1 h, 72 stops for 3 days |
| `advance 3d --max-jump none` (one raw jump) | **0** runs of a daily `0 3 * * *` job (skipped, as pg_cron's source predicts) |
| `advance 3d` (automatic sub-steps) | **3** runs, in 92 s of real time |

## Known gaps and costs

Follow-ups after the M2 review (2026-11-13):

1. ~~pg_cron is slow to fast-forward~~ **Fixed.** ChronoStack reads `cron.job` and stops at each fixed-time job's minute (UTC). 3 virtual days: **92 s → 6.4 s**, still 3 runs. This also confirms empirically that pg_cron runs the current minute's jobs after a large jump. It falls back to hourly sub-steps for unsupported schedules (e.g. `30 seconds`) or a non-UTC `cron.timezone`.
2. ~~Scheduler matrix not automated~~ **Done:** `tests/e2e/run_schedulers.py`, run nightly. BullMQ job schedulers and delayed jobs fire on their own (about 1 s). Celery beat with `beat_max_loop_interval=1` fires (about 2 s). node-cron needs a trigger.
3. ~~R13~~ **Fixed:** published host ports and `container_name` are reset with `!reset` by default (`up --keep-ports` to opt out).
4. ~~YAML parser~~ **Replaced** with `serde_norway` 0.9.42, a maintained fork with the same API.
5. **Determinism:** nightly CI runs the demo 10× on Linux x86_64 and arm64 (`.github/workflows/nightly.yml`). macOS remains a manual pre-alpha check.
6. Still open: a timed-out `exec` trigger's process may keep running inside the container.
