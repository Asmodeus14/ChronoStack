# Using ChronoStack on your own project

This guide takes you from "installed" to "my time-dependent flow is tested", and covers what to do when something doesn't behave. It assumes a project that already runs with `docker compose up`.

- [1. Start the copy](#1-start-the-copy)
- [2. Read `doctor`](#2-read-doctor)
- [3. Move time](#3-move-time)
- [4. Make your scheduled jobs run](#4-make-your-scheduled-jobs-run)
- [5. Wait for the work to finish: barriers](#5-wait-for-the-work-to-finish-barriers)
- [6. Outages for HTTP dependencies](#6-outages-for-http-dependencies)
- [7. Turn it into a test: scenarios](#7-turn-it-into-a-test-scenarios)
- [8. In CI](#8-in-ci)
- [9. Troubleshooting](#9-troubleshooting)

## 1. Start the copy

(Installing: see the [README](GETTING_STARTED.md#1-install). Updating later: `chronostack update`.)

```sh
cd your-project            # where your compose.yml / docker-compose.yml lives
chronostack up --wait
```

- ChronoStack starts a **separate copy** of your project, named `<project>-chronostack`, with its own volumes. Your normal stack and its data are not touched, and no file in your project is changed. The generated override lives in `.chronostack/`, which ignores itself in git.
- Host ports and `container_name` are dropped in the copy, so it can run next to your normal stack. To reach the copy from your browser, use `chronostack up --keep-ports`; it then conflicts with your normal stack's ports.
- `--wait` returns once every virtualized service answers a time probe.
- Several compose files work as with Compose: `chronostack -f compose.yml -f compose.dev.yml up`.

## 2. Read `doctor`

```sh
chronostack doctor
```

Each service gets one row:

| Column | Meaning |
|---|---|
| PLATFORM | CPU architecture and C library of the image: `x86_64/gnu`, `aarch64/musl`, …, or `…/static` |
| VIRTUALIZED | `yes`, or `no — <reason>` |
| SHIM IN PID1 | whether the preload library is loaded in the container's main process |
| PROBE | `ok` (sees virtual time), `skew` (sees another time), `unavailable` (the probe command failed) |

Reasons a service is **not virtualized**, and what to do:

| `doctor` says | Why | What you can do |
|---|---|---|
| `no dynamic loader in image (static binary, e.g. Go or scratch)` | `LD_PRELOAD` needs a dynamic loader; static and `scratch` images have none | Usually fine if the service doesn't use time itself (proxies, static file servers). Go services are unsupported in v0.1. |
| `Go process(es) … read the clock directly (vDSO)` | Go doesn't call libc for time | Same as above. The service runs, but on real time. |
| `shim not loaded in PID 1` | The main process is static or setuid | Check what the image's entrypoint really runs. |
| `image is arm64 but the Docker engine is x86_64` | Emulated images aren't supported | Use an image built for your machine's architecture. |
| `image could not be inspected` | Pull or build failed | Run `docker compose pull` / `build` and read the error. |

A probe of `unavailable` right after `up` usually means the service is still starting; `up --wait` waits for it. If `cache` or `db` probes say `skew`, open an issue with the `doctor` output.

## 3. Move time

```sh
chronostack now              # virtual time, real time, offset
chronostack advance 25h      # sessions with a 24 h TTL expire
chronostack advance --to 2026-12-01T00:00:00Z
chronostack freeze           # stop the clock (constant wall time)
chronostack unfreeze
```

Time only moves forward. Every service sees the same virtual time:

- Postgres `now()`;
- Redis key expiry;
- `Date.now()` in Node, `time.time()` in Python, the JVM, PHP, Ruby, …

After each advance, every service is probed.

## 4. Make your scheduled jobs run

**Moving the clock does not, by itself, run your timers.** `setTimeout`, `asyncio.sleep` and most in-process schedulers wait on the monotonic clock, which ChronoStack deliberately leaves alone (otherwise deadlines would break). What happens depends on the scheduler:

| Scheduler | After an advance | What to do |
|---|---|---|
| **BullMQ** (delayed jobs, job schedulers) | Fires within about 1 s | Nothing |
| **Celery beat** with `beat_max_loop_interval ≤ 1` | Fires within about 2 s | Set that option in test/dev |
| **pg_cron** | Detected automatically; ChronoStack stops the clock on each job's minute | Nothing. Jobs like `*/5 * * * *` run once per stop; `advance --pg-cron-all` runs every occurrence. |
| A worker that polls the database (`WHERE run_at <= now()`) | Runs at its next poll | A barrier (section 5) so ChronoStack waits for it |
| **node-cron**, **APScheduler**, `setTimeout`-based loops, Spring `@Scheduled` | Does not fire | Declare a **trigger** |

A trigger runs your job's code **at its virtual time**, in the container, like the scheduler would:

```yaml
# chronostack.yml (next to your compose file)
version: 1
triggers:
  - name: expire-trials
    schedule: { every: 1d, at: "03:00" }          # UTC, virtual time
    exec: { service: worker, command: [node, src/jobs/expire-trials.js] }
  - name: nightly-report
    schedule: { every: 1d, at: "01:30" }
    http: { url: "http://localhost:8080/internal/run-report" }   # from the host
```

Then `chronostack advance 14d` runs `expire-trials` 14 times, each at its own virtual 03:00. `chronostack advance 14d --dry-run` prints the plan first. A trigger without `schedule` runs at the end of every `--step`.

To find the command for a job, look for how your scheduler calls it. Most projects can run a job once from the command line (`node jobs/x.js`, `python -m app.jobs.x`, `rails runner`, `php artisan schedule:run`). If yours can't, add a tiny entry point; it's useful outside ChronoStack too.

## 5. Wait for the work to finish: barriers

Without barriers, ChronoStack moves on as soon as a trigger's command returns. If the job only *enqueues* work for a worker, wait for that work too:

```yaml
barriers:
  - name: queue-drained
    redis: { service: cache, command: [LLEN, "bull:emails:wait"], equals: "0" }
  - name: outbox-settled
    sql:
      service: db
      query: "select count(*) = 0 from outbox where sent_at is null and next_attempt_at <= now()"
```

Barriers are checked at every clock stop, with a timeout (default 30 s; `advance.barrier_timeout` changes it). A barrier that never becomes true stops the advance with exit code 5. `exec` and `http` barriers work too. See the [schema](schema/chronostack.schema.json) for every field.

## 6. Outages for HTTP dependencies

To test retries when a partner API or webhook receiver is down, route it through ChronoStack's proxy:

```yaml
proxies:
  - name: payments
    upstream: https://api.payments.example    # http:// or https://
    env: { worker: PAYMENTS_URL }              # the variable in your compose file holding that URL
```

On `up`, the worker's `PAYMENTS_URL` points at the proxy instead; your compose file is unchanged. Then:

```sh
chronostack fault set payments --from +2d --for 2h   # down on day 2, in virtual time (503)
chronostack fault list
chronostack events                                    # every proxied request: forwarded or blocked
```

The URL must come from an environment variable set in the compose file. Hard-coded URLs can't be routed.

## 7. Turn it into a test: scenarios

```yaml
# scenarios/trial.yml
version: 1
name: Trial expires on day 14
steps:
  - advance: { to: next 00:00 }
  - exec: { service: api, command: [node, scripts/signup.js, "user-${run_id}@example.com"] }
  - exec: { service: db, command: [psql, -U, app, -Atc, "select id from users where email = 'user-${run_id}@example.com'"] }
    save: user_id
  - advance: 14d
  - advance: { by: 6h, step: 1h }
  - name: the user was downgraded
    assert: { sql: { service: db, query: "select plan from users where id = ${user_id}", equals: free } }
```

```sh
chronostack run scenarios/trial.yml      # exit 0 = passed, 6 = a step failed
```

- Each run writes `.chronostack/reports/<time>-<name>.md`, which explains what failed, and `.json`, which has everything.
- Secrets in command output are redacted.
- `${run_id}` makes each run unique, so a scenario can run again on the same stack.
- Every step type: [scenario schema](schema/scenario.schema.json). A complete example: [the demo](../examples/saas-trial-demo/scenarios/trial.yml).

## 8. In CI

On GitHub Actions (a Linux runner; GitHub's macOS and Windows runners can't run Linux containers):

```yaml
jobs:
  lifecycle:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: Asmodeus14/ChronoStack@v0.1.0-alpha.2    # installs the CLI (checksum-verified)
      - run: chronostack up --wait
      - run: chronostack run scenarios/trial.yml
      - if: always()
        uses: actions/upload-artifact@v4
        with: { name: chronostack-reports, path: .chronostack/reports/ }
```

Any other CI with Docker:

```sh
curl -fsSL https://github.com/Asmodeus14/ChronoStack/releases/download/v0.1.0-alpha.2/install.sh | CHRONOSTACK_VERSION=0.1.0-alpha.2 sh
export PATH="$HOME/.local/bin:$PATH"
chronostack up --wait && chronostack run scenarios/trial.yml
```

Keep `.chronostack/reports/` as a build artifact, so a failed run can be read later.

## 9. Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| A job didn't run after `advance` | Its scheduler doesn't notice wall-clock jumps (section 4) | Add a trigger, or a barrier if it polls |
| `advance` stops with exit 5 | A barrier stayed false | `chronostack advance … --json` shows the last value; raise its `timeout` or fix the condition |
| `advance` stops with exit 4 | A trigger failed or timed out | The output shows the command's error; timed-out commands are stopped inside the container |
| Exit 3 after `advance` | A service doesn't see the virtual time | `chronostack doctor` |
| "port is already allocated" | You used `--keep-ports` with your normal stack running | Stop one, or drop `--keep-ports` |
| Ports or `container_name` kept despite the default | Docker Compose older than 2.24 | Update Compose |
| A pg_cron job like `*/5 * * * *` ran fewer times than expected | It runs once per clock stop | `advance --pg-cron-all`, or `--step` |
| `doctor` warns the CLI and controller versions differ | An old stack from another CLI version | `chronostack down`, then `up` |
| Data looks "from the future" | It was written in virtual time | It's in the copy's own volumes; `chronostack down -v` removes them |
| `chronostack: command not found` / "not recognized" right after installing | The terminal was opened before the install (on Linux and macOS, `curl … \| sh` can't change the shell that ran it) | Run the `export PATH=…` line the installer printed, or open a new terminal. On Windows the installer updates the current session itself. |
| macOS: "cannot be opened because the developer cannot be verified" | The archive was downloaded in a browser (binaries aren't notarized yet) | Install with the script, or run `xattr -d com.apple.quarantine ~/.local/bin/chronostack` |

Still stuck? Open an issue with your compose snippet, `chronostack.yml`, and the output of `chronostack doctor`.
