# Getting started with ChronoStack

This page takes you from nothing to "I tested what happens after 14 days", in about 10 minutes. No prior knowledge of ChronoStack is assumed. For the full reference, see the [guide](GUIDE.md).

- [What ChronoStack is, in one minute](#what-chronostack-is-in-one-minute)
- [1. Install](#1-install)
- [2. Try the demo](#2-try-the-demo)
- [3. Use it on your own project](#3-use-it-on-your-own-project)
- [Command cheat sheet](#command-cheat-sheet)
- [Key ideas](#key-ideas)
- [FAQ](#faq)

## What ChronoStack is, in one minute

Your app has rules that depend on time passing: a trial ends after 14 days, a session expires after 24 hours, a job runs every night at 03:00, a failed webhook is retried an hour later. Testing them usually means editing dates in the database by hand, or waiting.

ChronoStack lets you **move time forward** for your whole Docker Compose stack instead:

```text
your compose.yml ──► chronostack up ──► a copy of your stack, on one virtual clock
                                          Postgres now(), Redis expiry, Node, Python, …
                     chronostack advance 14d
                                          ──► every service is 14 days later,
                                              and your daily job ran 14 times
```

- It runs a **copy** of your stack. Your normal containers and data are never touched, and your files are never modified.
- It **doesn't change your computer's clock.** Only the processes inside the copy see the virtual time.
- **Time only moves forward.** To start again: `chronostack down -v` and `chronostack up`.

## 1. Install

You need **Docker with Compose v2**: Docker Desktop on Windows or macOS, Docker Engine on Linux. No admin rights are needed for ChronoStack itself.

**Linux / macOS**

```sh
curl -fsSL https://github.com/Asmodeus14/ChronoStack/releases/download/v0.1.0-alpha.2/install.sh | CHRONOSTACK_VERSION=0.1.0-alpha.2 sh
```

The installer prints one `export PATH=…` line. Run it, or open a new terminal.

**Windows (PowerShell)**

```powershell
$env:CHRONOSTACK_VERSION = "0.1.0-alpha.2"; irm https://github.com/Asmodeus14/ChronoStack/releases/download/v0.1.0-alpha.2/install.ps1 | iex
```

It works in the same window straight away.

**Check it worked:**

```sh
chronostack --version      # chronostack 0.1.0-alpha.2
```

**Update later:** `chronostack update` (or `chronostack update --check` to only look).

<details>
<summary><b>Uninstall</b></summary>

- **Linux / macOS:** `rm ~/.local/bin/chronostack`, then delete the two lines starting with `# Added by the ChronoStack installer` from your shell's startup file (`~/.zshrc`, `~/.bashrc`, `~/.bash_profile`, `~/.profile`, or `~/.config/fish/conf.d/chronostack.fish`).
- **Windows:** delete `%LOCALAPPDATA%\chronostack`, then remove that folder from your user PATH (Settings → "Edit environment variables for your account").
- **Controller image:** `docker image rm ghcr.io/asmodeus14/chronostack-clockd:0.1.0-alpha.2`.

</details>

## 2. Try the demo

The repository has a small SaaS app (Postgres, Redis, an API and a worker) with a 14-day trial, 24-hour sessions, a nightly job and webhooks.

```sh
git clone https://github.com/Asmodeus14/ChronoStack
cd ChronoStack/examples/saas-trial-demo
chronostack up --wait          # first time: about a minute (images download)
```

Look around:

```sh
chronostack doctor             # every service: is it on the virtual clock?
chronostack now                # virtual time vs real time
chronostack advance 1d         # one day later; the 03:00 job runs once
chronostack now
```

Then run the full story as a test. A user signs up, the partner's webhook endpoint is down on day 14, the trial expires, and the webhook is retried and delivered exactly once:

```sh
chronostack run scenarios/trial.yml     # about 40 s; ends with PASSED — 12/12 steps
```

Open the report it prints (`.chronostack/reports/….md`). To see a failure, start the demo with a deliberate bug:

```sh
chronostack down -v
chronostack -f compose.yml -f compose.bug.yml up --wait
chronostack run scenarios/trial.yml     # FAILED, and the report says why
chronostack down -v                     # clean up
```

## 3. Use it on your own project

**Step 1: start the copy.** In the folder with your `compose.yml`:

```sh
chronostack up --wait
```

**Step 2: check it.**

```sh
chronostack doctor
```

Every service that matters should say `VIRTUALIZED yes` and `PROBE ok`. A `no` usually means a Go or static binary, which ChronoStack can't reach. That's fine if the service doesn't care about time. [What each line means](GUIDE.md#2-read-doctor).

**Step 3: move time and look.**

```sh
chronostack advance 25h        # did sessions expire? check your app or database
```

**Step 4: make your scheduled jobs run.** Most in-process schedulers (`setTimeout`, node-cron, APScheduler, `@Scheduled`) **don't** notice the clock jumping. Tell ChronoStack how to run the job at its virtual time. Create `chronostack.yml` next to your compose file:

```yaml
version: 1
triggers:
  - name: nightly-job
    schedule: { every: 1d, at: "03:00" }                  # UTC, virtual time
    exec: { service: worker, command: [node, jobs/nightly.js] }
```

Now `chronostack advance 7d` runs `nightly-job` 7 times, each at its own virtual 03:00. Preview with `--dry-run`. BullMQ, Celery beat and pg_cron are handled without a trigger; see [which schedulers need what](GUIDE.md#4-make-your-scheduled-jobs-run).

**Step 5: turn it into a test.** Write the steps and checks in a scenario file and run it with `chronostack run`. It exits with 0 if everything passed, so it can run in CI. [How to write scenarios](GUIDE.md#7-turn-it-into-a-test-scenarios) · [CI setup](GUIDE.md#8-in-ci)

**When you're done:** `chronostack down -v` removes the copy and its data.

## Command cheat sheet

| Command | What it does |
|---|---|
| `chronostack up --wait` | Start the copy of your stack on a virtual clock and wait until it's ready |
| `chronostack doctor` | Show which services see virtual time, and why not |
| `chronostack now` | Virtual time, real time and the difference |
| `chronostack advance 14d` | Move forward 14 days, running your triggers at their virtual times |
| `chronostack advance 14d --dry-run` | Show what would run, without moving the clock |
| `chronostack advance --to 2026-12-01T00:00:00Z` | Move forward to an exact time (UTC) |
| `chronostack freeze` / `unfreeze` | Stop and restart virtual time |
| `chronostack fault set <proxy> --from +1d --for 2h` | Make an HTTP dependency "down" during a virtual window ([how](GUIDE.md#6-outages-for-http-dependencies)) |
| `chronostack events` | What happened: clock moves, outages, proxied requests |
| `chronostack run <scenario.yml>` | Run a scripted test and write a report |
| `chronostack down -v` | Remove the copy and its data |
| `chronostack update` | Update ChronoStack itself |
| `chronostack <command> --help` | Every option of a command |

Durations look like `90s`, `25h`, `14d`, `1h30m`.

## Key ideas

| Term | Meaning |
|---|---|
| **The copy** | Your stack started under the project name `<name>-chronostack`, with its own containers and volumes. |
| **Virtual time** | The time every process in the copy sees. It flows normally between your commands and jumps when you `advance`. |
| **Trigger** | A command or HTTP call ChronoStack runs at a given virtual time: "run the nightly job at 03:00". It's needed because timers don't fire when the clock jumps. |
| **Barrier** | A condition ChronoStack waits for before moving on, e.g. "the email queue is empty". |
| **Probe** | A check that a service really sees the virtual time (`doctor`, and after each advance). |
| **Proxy / outage** | A small proxy in front of an HTTP dependency, so it can be made to fail during a window of virtual time. |
| **Scenario** | A YAML file of steps and checks, run with `chronostack run`. It produces a report. |

## FAQ

**Does it change my computer's clock?**
No. Only processes inside ChronoStack's copy of your stack see virtual time. Your machine and your normal containers keep real time.

**Will it touch my data or my compose files?**
No. The copy has its own volumes, and the extra settings live in `.chronostack/` (which ignores itself in git). Your compose files are never edited. `chronostack down -v` deletes the copy's data.

**I advanced 7 days, but my cron job didn't run. Why?**
Most schedulers wait on a timer that doesn't notice the wall clock jumping. Declare a trigger for the job ([step 4](#3-use-it-on-your-own-project)). BullMQ, Celery beat (with `beat_max_loop_interval: 1`) and pg_cron are the exceptions; they work on their own.

**Can I go back in time?**
Not in v0.1. Start fresh with `chronostack down -v` and `chronostack up`.

**`chronostack` is "not found" or "not recognized" after installing.**
On Linux and macOS, run the `export PATH=…` line the installer printed, or open a new terminal. On Windows, open a new terminal if the installer window was a different one.

**One of my services says `VIRTUALIZED no`.**
It's most likely a Go or static binary, which ChronoStack can't reach. Services that don't depend on time are fine as they are. The [guide](GUIDE.md#2-read-doctor) lists every reason and what to do.

**Does it work on macOS?**
It should: everything runs in Docker's Linux VM, the same as on Windows. It hasn't been tested yet, though. If you have a Mac, [issue #5](https://github.com/Asmodeus14/ChronoStack/issues/5) has a 5-minute test.

**Can I use it in CI?**
Yes, on Linux runners. There's a GitHub Action (`uses: Asmodeus14/ChronoStack@v0.1.0-alpha.2`); see the [CI section](GUIDE.md#8-in-ci).

**Can an AI agent drive it?**
Yes. `chronostack mcp` is an MCP server with a fixed, safe set of tools: `claude mcp add chronostack -- chronostack mcp --project-dir <your project>`.

**Something else is wrong.**
Open an [issue](https://github.com/Asmodeus14/ChronoStack/issues) with your compose snippet and the output of `chronostack doctor`.
