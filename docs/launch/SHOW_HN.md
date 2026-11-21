# Draft: Show HN post

*Post from your own account. Best times: Tuesday–Thursday, about 8–10 am US Eastern (7–9 pm in India). Stay in the thread for the day. HN guidelines: https://news.ycombinator.com/showhn.html*

**Title** (80 characters max):

> Show HN: ChronoStack – Stripe Test Clocks for your Docker Compose stack

**URL:** `https://github.com/Asmodeus14/ChronoStack`

**Text (or first comment):**

> I kept testing trial expiry and webhook retries by editing timestamps in Postgres, so I built a tool that moves the clock for a whole Compose stack: Postgres `now()`, Redis TTLs, Node and Python services (JVM, Ruby, PHP and .NET pass the nightly clock checks but haven't been tried on real apps yet), all at once, with no code changes (an `LD_PRELOAD` shim plus a small controller container).
>
> The fake clock was the easy part. What took the time:
>
> - advancing wall-clock time doesn't fire timers (they run on the monotonic clock), so `advance` stops at each job's virtual time, runs a declared trigger, and waits on a barrier;
> - absolute wall-clock deadlines (`sem_timedwait`, `pthread_cond_timedwait`, Java's `parkUntil`, …) hang once the clock is ahead, so the shim translates them back to real time (checked every night in C, Python, Node, JVM, Ruby, PHP and .NET, on x86_64 and arm64);
> - pg_cron skips jobs after a jump of more than 3 h (Vixie cron logic), so the clock stops on each job's minute;
> - job queues hide work from you: built-in BullMQ and Sidekiq barriers hold each step until the jobs are done, and running jobs finish before the clock moves (otherwise a jump expires BullMQ's locks and it runs them twice);
> - the copy of your stack gets its own named volumes, but a bind mount like `./data` can't be copied, so `up` refuses to start if one is writable: otherwise a "delete rows older than 7 days" job during `advance 15d` would run against your real data;
> - there's a fault proxy for outages in virtual time, scenarios with readable failure reports (JUnit too), and an MCP server so agents can drive it.
>
> The shim adds about 12–16 ns per clock read in C and 10–26 ns in Python and Node, measured on three machines (`bench/`).
>
> The demo: 15 virtual days, a webhook outage on day 14 and retries, checked in about 15 s of real time.
>
> Before posting this I ran it against a few open-source projects' published images, testing their real time-based features, and found six date bugs: Saleor's unpaid-order expiry fired far too late for settings of a day or more, Twenty's trash cleanup deleted records up to a day early, Chatwoot never reopened snoozed conversations after a background-job outage of more than 3 days, Formbricks scheduled surveys a day late in UTC+12 zones, tududi moved monthly recurring tasks to the wrong day, and Worklenz's "1st Monday" recurring tasks moved into the previous month and then stopped recurring. All are reported upstream (links on the site and in the README); Twenty and Worklenz have merged the fixes, Chatwoot's team has confirmed theirs as a bug, and two more fix PRs are open.
>
> Limits: Go binaries aren't covered (no libc), time only moves forward, and Apple-silicon Macs are untested (Linux and Intel macOS run in CI). If you have an M-series Mac, issue #5 is a 5-minute test. It's an alpha (0.1.0-alpha.8), MIT/Apache-2.0. Install with a script, or `brew install asmodeus14/tap/chronostack`.
>
> Site, with an interactive demo: https://chronostack-dev.vercel.app. I'd especially like to hear what breaks on your stack (`chronostack doctor` output helps).

**Before posting:** check each upstream item's status (as of 2026-11-20: Twenty and Worklenz fixed and merged; Saleor and tududi fix PRs open; Chatwoot labelled Bug by their team, with a community fix PR open (#16679); Formbricks labelled bug) and match the site's "Found in real projects" table. State facts only: no project's logo, no implied endorsement, and Twenty's "critical" label came from their triage bot, so don't quote it as their verdict.

**Answers to have ready:**

- *"Will it touch my real database?"* Not named volumes: the copy runs as `<project>-chronostack` with its own. Bind mounts and `external` volumes can't be copied, so `up` stops if one is writable and names it; `--allow-shared-mounts` overrides, read-only mounts only get a note, and `doctor` lists them.

- *"Did you find those bugs with the tool, or by reading code?"* Both: reading the time-related code points to a likely bug; the tool is what proves it on the real published image (for example, a 4-day outage or two weeks of daily jobs, checked in a few minutes), and checks the fix the same way.

- *"Why not inject a clock in the app?"* That works for unit tests. This is for integration: the database's `now()`, Redis expiry and every service agreeing, with no code changes.
- *"How is this different from libfaketime?"* libfaketime provides the mechanism (and inspired this). ChronoStack adds live control across containers, deadline translation, scheduler-aware stepping, verification probes, outages and scenarios.
- *"LD_PRELOAD is scary."* The C shim is small and auditable. It reads a read-only page, fails open (strict mode for CI), and never changes the host clock or uses privileged containers.
- *"Will it work on my Mac?"* Intel Macs: the native CLI passes the end-to-end suites in CI (Docker in a Linux VM, the same setup as Docker Desktop). Apple silicon should work the same way, since containers run in a Linux VM there too, but it's untested: the risky part is amd64 images under Rosetta. Issue #5 has the 5-minute test.
- *"Why C for the shim?"* It has to load into every libc (glibc 2.17+ and musl) without a runtime; the rest is Rust.
