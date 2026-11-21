# Using ChronoStack on your own project

This guide takes you from "installed" to "my time-dependent flow is tested", and covers what to do when something doesn't behave. It assumes a project that already runs with `docker compose up`.

- [1. Start the copy](#1-start-the-copy)
- [2. Read `doctor`](#2-read-doctor)
- [3. Move time](#3-move-time)
- [4. Make your scheduled jobs run](#4-make-your-scheduled-jobs-run)
- [5. Wait for the work to finish: barriers](#5-wait-for-the-work-to-finish-barriers)
- [6. Outages for HTTP dependencies](#6-outages-for-http-dependencies)
- [7. Turn it into a test: scenarios](#7-turn-it-into-a-test-scenarios)
- [8. Simulated users](#8-simulated-users)
- [9. In CI](#9-in-ci)
- [10. Troubleshooting](#10-troubleshooting)

## 1. Start the copy

(Installing: see the [README](GETTING_STARTED.md#1-install). Updating later: `chronostack update`.)

```sh
cd your-project            # where your compose.yml / docker-compose.yml lives
chronostack up --wait
```

- ChronoStack starts a **separate copy** of your project, named `<project>-chronostack`, with its own named volumes. No file in your project is changed. The generated override lives in `.chronostack/`, which ignores itself in git.
- **Some storage can't be copied:** bind mounts (`./data:/var/lib/postgresql/data`), volumes with `external: true`, and volumes with a fixed `name:` are the same folder or volume in the copy as in your normal stack. A job that runs during `advance` (say, "delete records older than 7 days") would change your real data there. So `up` refuses to start when one of them is writable, and names it. Switch it to an ordinary named volume, mount it read-only (`:ro`, which only gets a note), or pass `--allow-shared-mounts` if sharing is what you want, e.g. for a config folder. `doctor` lists these mounts too.
- Host ports and `container_name` are dropped in the copy, so it can run next to your normal stack. To reach a service of the copy from your machine, list it under `expose:` in `chronostack.yml`:

  ```yaml
  expose:
    - app:3000                                  # a free port on 127.0.0.1, shown by `up` and `doctor`
    - { service: web, port: 80, host_port: 8080 }   # always http://127.0.0.1:8080
  ```

  Only these ports are published, and only on 127.0.0.1. In triggers, barriers and scenario `http` steps, `service://app:3000/path` goes to that port, wherever it landed. `chronostack up --keep-ports` keeps all your own published ports instead; it then conflicts with your normal stack's ports.
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
| PROBE | `ok` (sees virtual time), `skew` (sees another time), `unavailable` (the probe command failed), `completed` (a one-shot container such as a migrator exited with code 0; not a problem) |

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

`chronostack init` applies this table to your compose file: it writes a starter `chronostack.yml` with what it can detect (Sidekiq barriers, pg_cron, Celery beat settings) and commented suggestions for the rest.

| Scheduler | After an advance | What to do |
|---|---|---|
| **BullMQ** (delayed jobs, job schedulers) | Fires within up to about 10 s (BullMQ's longest wait between checks) | A `bullmq` barrier (section 5) |
| **Celery beat** with `beat_max_loop_interval ≤ 1` (or `celery beat --max-interval 1`) | Fires within about 2 s | Set that option in test/dev |
| **Celery beat** with django-celery-beat's `DatabaseScheduler` (or a subclass), crontab tasks | Interval tasks fire as above, but crontab tasks may never be sent after a jump, even when the scheduler reports them due. Restarting beat sends them once. ([#9](https://github.com/Asmodeus14/ChronoStack/issues/9)) | Declare a **trigger** for each crontab task |
| **sidekiq-cron** (Ruby) | Fires if its 30 s poll lands within 60 s after the job's time (`reschedule_grace_period`); otherwise that run is skipped | Land each stop just after the job's minute (`advance: { to: next 02:15 }`) with a `sidekiq` barrier and `settle: 35s`, or declare a trigger |
| **pg_cron** | Detected automatically; ChronoStack stops the clock on each job's minute | Nothing. Jobs like `*/5 * * * *` run once per stop; `advance --pg-cron-all` runs every occurrence. |
| A worker that polls the database (`WHERE run_at <= now()`) | Runs at its next poll | A barrier (section 5) so ChronoStack waits for it |
| **Laravel scheduler** (`php artisan schedule:work`) | Does not fire: `schedule:run` only runs jobs due in the current minute, so a jump past a job's minute skips it | Declare a **trigger** that runs `php artisan schedule:run` at the job's time |
| **node-cron**, **APScheduler**, `setTimeout`-based loops, Spring `@Scheduled` | Does not fire | Declare a **trigger** |

**Schedulers that fire on their own run a missed job when the clock lands, not at the job's own time.** After `advance 1d` from 01:30, a 01:00 Celery beat job runs at 01:30 plus a second or two, so its `now()` is 01:30. If a test depends on a job's exact time (for example "exactly 30 days after creation"), land each step just after the job's time (`advance: { to: next 01:01 }`, then `step: 1d`), or declare a trigger, which always runs at the scheduled time.

**They also run a missed job only once per jump.** A single `advance 3d` runs a daily Celery beat task once, not three times (measured with the scheduler test fixture). To get one run per day, stop the clock each day with `advance 3d --step 1d`, or declare a trigger with a `schedule`, which runs at every scheduled time in the span.

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
    http: { url: "service://api:8080/internal/run-report" }   # api:8080 listed under expose:
```

Then `chronostack advance 14d` runs `expire-trials` 14 times, each at its own virtual 03:00. `chronostack advance 14d --dry-run` prints the plan first. A trigger without `schedule` runs at the end of every `--step`.

To find the command for a job, look for how your scheduler calls it. Most projects can run a job once from the command line (`node jobs/x.js`, `python -m app.jobs.x`, `rails runner`, `php artisan schedule:run`). If yours can't, add a tiny entry point; it's useful outside ChronoStack too.

## 5. Wait for the work to finish: barriers

Without barriers, ChronoStack moves on as soon as a trigger's command returns. If the job only *enqueues* work for a worker, wait for that work too. For BullMQ and Sidekiq there are built-in barriers:

```yaml
barriers:
  - name: jobs-done
    bullmq: { service: redis, queues: [emails, reports] }    # prefix: "bull" by default
  - name: sidekiq-idle
    sidekiq: { service: redis }                               # namespace: myapp, if you use redis-namespace
```

- **`bullmq`** (also Bull) waits until the queues have nothing waiting, paused, running or prioritized, and no delayed job that is already due. It reads Redis's own clock, so jobs that became due with the jump count until a worker has finished them. Set `prefix` if your queues use BullMQ's `prefix` option.
- **`sidekiq`** waits until every queue is empty, no scheduled or retry job is due, and no job is running. Sidekiq only reports running jobs on its heartbeat (every 10 s), so once the queues look empty the barrier waits for one more heartbeat before it trusts "nothing running". Expect up to about 10 s per stop.
- **`settle: 3s`** on any barrier waits that long at each stop before the first check, for schedulers that only enqueue work when they next poll (sidekiq-cron, Clockwork).

For anything else, use a query:

```yaml
barriers:
  - name: queue-drained
    redis: { service: cache, command: [LLEN, "jobs"], equals: "0" }
  - name: outbox-settled
    sql:
      service: db
      query: "select count(*) = 0 from outbox where sent_at is null and next_attempt_at <= now()"
```

A barrier also covers slow jobs. A queue that runs one job at a time is held up in *real* time, so a job that takes a minute would otherwise span several virtual days of short stops.

Barriers are checked at every clock stop, and also **before the first jump** of each `advance`, so no job is running when the clock moves. That matters: a job in flight during a jump can see its Redis lock expire with the jump (BullMQ then treats it as stalled and runs it again) or time out against the new time. Each check has a timeout (default 30 s; `advance.barrier_timeout` changes it, or `timeout` on one barrier). A barrier that never becomes true stops the advance with exit code 5. `exec` and `http` barriers work too. See the [schema](schema/chronostack.schema.json) for every field.

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
- A value saved by an earlier step works as an advance target, for times that depend on data (calendar months differ in length): save `ends_at` as a UTC timestamp like `2026-12-14T17:10:00Z`, then `advance: { to: "${ends_at}" }`.
- Every step type: [scenario schema](schema/scenario.schema.json). A complete example: [the demo](../examples/saas-trial-demo/scenarios/trial.yml).

## 8. Simulated users

A scenario is one script. `chronostack users` runs many simulated users at once, each following a **journey**, with every random choice fixed by a seed so a run can be repeated.

```yaml
# journeys.yml
version: 1
seed: 42                          # same seed: same users, same arrivals, same choices
max_parallel: 32                  # most users active at once (up to 256)
targets:
  api: service://api:8080         # a port under `expose:` in chronostack.yml, or any http(s):// URL
cohorts:
  - name: trialists
    users: 50
    arrive: { over: 5m }          # evenly; add `model: poisson` for random gaps
    journey: trial_to_paid
journeys:
  trial_to_paid:
    - name: sign up
      http: { method: POST, url: "${api}/signup", json: { email: "u${user}+${run_id}@example.com" } }
      expect: { status: 201, json: { /plan: trial } }
      save: { token: /token }     # a JSON pointer into the response
    - think: { between: [2s, 10s] }
    - branch: { p: 0.3 }          # 30% of users go on; the others stop here
    - name: add a card
      http: { method: POST, url: "${api}/billing/card", headers: { Authorization: "Bearer ${token}" } }
      expect: { status: [200, 204] }
    - repeat:
        times: 3
        steps:
          - http: { url: "${api}/dashboard", headers: { Authorization: "Bearer ${token}" } }
          - think: 1s
```

```sh
chronostack users validate journeys.yml    # checks the file, runs nothing
chronostack users run journeys.yml         # exit 0 = passed, 6 = a step failed
```

- **Steps:** `http` (with `expect` and `save`), `think` (fixed or `between`), `branch` and `repeat`. There's no scripting language; use a scenario `exec` step for anything else.
- **Variables:** `${user}` (1, 2, … within a cohort), `${cohort}`, `${run_id}`, each target, and values saved by earlier steps. A variable used before it's saved is an error when the file is checked.
- **A failed step stops that user:** an unexpected status, a JSON value that doesn't match, or a missing value to save. Other users carry on. The summary lists runs, failures and p50/p90/p99/max latency per step, and the errors.
- **Reports:** `--json` prints the report, `--junit out.xml` writes one test case per step, and `--trace` adds every user's steps to the JSON.
- **Reproducible:** `--seed` (or `seed:`) fixes arrivals, `between` thinks and branches. Without one, a seed is picked and printed, so you can repeat that run.
- **`users run` is real time:** users run on your machine's clock, and each think is capped at `max_think` (default 30s), so `think: 13d` doesn't hang. `--duration 10m` stops a run early. To let thirteen days pass in seconds, put the users in a scenario (below).
- **Not a load tester:** it simulates behaviour, with up to 256 users active at once. Against a trivial local server it drives about 8,000–24,000 requests per second depending on the machine ([benchmark](../bench/README.md#b3-simulated-users)); for high request rates against a real app, use k6 or similar.

### On the virtual clock: users in a scenario

Add `users:` to a scenario and the same journeys run on the **virtual** clock while the scenario advances. Arrivals and thinks count from the scenario's start, so a user who thinks for 13 days costs no real time.

```yaml
# scenarios/cohort.yml
version: 1
name: 20 trialists over 30 days
users: ../journeys/trialists.yml      # or { file: ../journeys/trialists.yml, resolution: 5s }
steps:
  - advance: { to: next 00:00 }
  - advance: 30d                      # users arrive and act during the advance
  - name: every trialist was downgraded
    assert: { sql: { service: db, query: "select count(*) from users where email like '%-${run_id}@example.com' and plan = 'free'", equals: "20" } }
```

- **During each `advance`, the clock also stops wherever users are due.** At each such stop, the due users act (up to `max_parallel` at once), then every barrier in `chronostack.yml` must hold again so their work settles, and time moves on. Triggers, pg_cron and outages run as in any advance. Stops between the users skip the probe step; the last one probes as usual.
- **Times are rounded up to `resolution`** (default 1 minute, at least 1 second), so users due close together share a stop. Each stop costs real time; a coarser grid makes long runs faster.
- **The same seed gives the same timeline.** A user's next action time is the stop's virtual time plus its think, never "whenever the requests finished". So who does which step at which virtual time depends only on the seed. How long each request took in real time can differ between runs.
- **Reports:** the scenario report gets a users table; the JSON report has the full timeline under `users.timeline` (redacted); JUnit gets a test case per journey step. A failed user step fails the scenario (exit 6). Users still mid-journey when the scenario ends are reported as stopped, and users who never arrived as not started.
- **`service://` targets** need the port under `expose:` in `chronostack.yml`.
- A complete example: [the demo's cohort scenario](../examples/saas-trial-demo/scenarios/cohort.yml), with 20 trialists over 30 days and a webhook outage. It takes about 3 minutes of real time.

## 9. In CI

On GitHub Actions, use a Linux runner. GitHub's macOS and Windows runners don't come with Docker; ChronoStack's own macOS CI starts Docker in a Linux VM with Colima on the Intel runners, which works but is slow.

```yaml
jobs:
  lifecycle:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: Asmodeus14/ChronoStack@v0.1.0-alpha.7    # installs the CLI (checksum-verified)
        with: { version: 0.1.0-alpha.7 }
      - run: chronostack up --wait
      - run: chronostack run scenarios/trial.yml --junit .chronostack/reports/junit.xml
      - if: always()
        uses: actions/upload-artifact@v4
        with: { name: chronostack-reports, path: .chronostack/reports/ }
```

`--junit <path>` also writes the result as JUnit XML, which most CI test reporters read: each step is a test case, a failed step is a `<failure>` with the evidence, and the steps after it are marked skipped. The JSON and Markdown reports are still written to `.chronostack/reports/`.

Any other CI with Docker:

```sh
curl -fsSL https://github.com/Asmodeus14/ChronoStack/releases/download/v0.1.0-alpha.7/install.sh | CHRONOSTACK_VERSION=0.1.0-alpha.7 sh
export PATH="$HOME/.local/bin:$PATH"
chronostack up --wait && chronostack run scenarios/trial.yml --junit junit.xml
```

Keep `.chronostack/reports/` as a build artifact, so a failed run can be read later.

## 10. Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| A job didn't run after `advance` | Its scheduler doesn't notice wall-clock jumps (section 4) | Add a trigger, or a barrier if it polls |
| A file you mounted at `/chronostack` is missing in the container | ChronoStack mounts its own files at `/chronostack` in every virtualized service | Mount yours at another path |
| `advance` stops with exit 5 | A barrier stayed false | `chronostack advance … --json` shows the last value; raise its `timeout` or fix the condition |
| `advance` stops with exit 4 | A trigger failed or timed out | The output shows the command's error; timed-out commands are stopped inside the container |
| Exit 3 after `advance` | A service doesn't see the virtual time | `chronostack doctor` |
| "port is already allocated" | You used `--keep-ports` with your normal stack running | Stop one, or drop `--keep-ports` |
| Ports or `container_name` kept despite the default | Docker Compose older than 2.24 | Update Compose |
| A pg_cron job like `*/5 * * * *` ran fewer times than expected | It runs once per clock stop | `advance --pg-cron-all`, or `--step` |
| `doctor` warns the CLI and controller versions differ | An old stack from another CLI version | `chronostack down`, then `up` |
| Data looks "from the future" | It was written in virtual time | It's in the copy's own volumes; `chronostack down -v` removes them |
| `up` says the copy "would share storage with your own stack" | A writable bind mount, `external` volume or fixed-name volume (section 1) | Use a named volume or `:ro`, or pass `--allow-shared-mounts` if sharing is intended |
| `chronostack: command not found` / "not recognized" right after installing | The terminal was opened before the install (on Linux and macOS, `curl … \| sh` can't change the shell that ran it) | Run the `export PATH=…` line the installer printed, or open a new terminal. On Windows the installer updates the current session itself. |
| macOS: "cannot be opened because the developer cannot be verified" | The archive was downloaded in a browser (binaries aren't notarized yet) | Install with the script, or run `xattr -d com.apple.quarantine ~/.local/bin/chronostack` |

Still stuck? Open an issue with your compose snippet, `chronostack.yml`, and the output of `chronostack doctor`.
