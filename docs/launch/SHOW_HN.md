# Draft: Show HN post

*Post from your own account. Best times: Tuesday–Thursday, about 8–10 am US Eastern. Link the write-up in the first comment, or use it as the URL. HN guidelines: https://news.ycombinator.com/showhn.html*

**Title** (80 characters max):

> Show HN: ChronoStack – Stripe Test Clocks for your Docker Compose stack

**URL:** `https://github.com/Asmodeus14/ChronoStack`

**Text (or first comment):**

> I kept testing trial expiry and webhook retries by editing timestamps in Postgres, so I built a tool that moves the clock for a whole Compose stack: Postgres `now()`, Redis TTLs, Node/Python/JVM services, all at once, with no code changes (an `LD_PRELOAD` shim plus a small controller container).
>
> The fake clock was the easy part. What took the time:
>
> - advancing wall-clock time doesn't fire timers (they run on the monotonic clock), so `advance` stops at each job's virtual time, runs a declared trigger, and waits on a barrier;
> - absolute wall-clock deadlines (`sem_timedwait`, `pthread_cond_timedwait`, Java's `parkUntil`, …) hang once the clock is ahead, so the shim translates them back to real time (272/272 no-hang checks on x86_64 and arm64);
> - pg_cron skips jobs after a jump of more than 3 h (Vixie cron logic), so the clock stops on each job's minute;
> - there's a fault proxy for outages in virtual time, scenarios with readable failure reports, and an MCP server so agents can drive it.
>
> The demo: 15 virtual days, a webhook outage on day 14 and retries, checked in about 15 s of real time.
>
> Before posting this I ran it against a few open-source projects' published images, testing their real time-based features, and found five date bugs: Saleor's unpaid-order expiry fired far too late for settings of a day or more, Twenty's trash cleanup deleted records up to a day early, Chatwoot never reopened snoozed conversations after a background-job outage of more than 3 days, Formbricks scheduled surveys a day late in UTC+12 zones, and tududi moved monthly recurring tasks to the wrong day. All are reported upstream (links on the site), two with fixes proposed.
>
> Limits: Go binaries aren't covered (no libc), time only moves forward, and **macOS is untested**. If you have a Mac, issue #5 is a 5-minute test. Pre-alpha, MIT/Apache-2.0.
>
> Write-up of the problems above: <link>. I'd especially like to hear what breaks on your stack (`chronostack doctor` output helps).

**Before posting:** update the bug paragraph with each item's current status (merged, fixed, open) and match the site's "Found in real projects" table. State facts only: no project's logo, no implied endorsement, and Twenty's "critical" label came from their triage bot, so don't quote it as their verdict.

**Answers to have ready:**

- *"Did you find those bugs with the tool, or by reading code?"* Both: reading the time-related code points to a likely bug; the tool is what proves it on the real published image (for example, a 4-day outage or two weeks of daily jobs, checked in a few minutes), and checks the fix the same way.

- *"Why not inject a clock in the app?"* That works for unit tests. This is for integration: the database's `now()`, Redis expiry and every service agreeing, with no code changes.
- *"How is this different from libfaketime?"* libfaketime provides the mechanism (and inspired this). ChronoStack adds live control across containers, deadline translation, scheduler-aware stepping, verification probes, outages and scenarios.
- *"LD_PRELOAD is scary."* The C shim is small and auditable. It reads a read-only page, fails open (strict mode for CI), and never changes the host clock or uses privileged containers.
- *"Why C for the shim?"* It has to load into every libc (glibc 2.17+ and musl) without a runtime; the rest is Rust.
