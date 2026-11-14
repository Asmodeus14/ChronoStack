# Draft: "Moving the clock doesn't run your cron jobs"

*Status: draft for the launch blog post (dev.to / personal blog), to be linked from Show HN. Every number below comes from the project's test results; links point to them. Edit freely; keep the numbers honest.*

---

## Moving the clock doesn't run your cron jobs

*What I learned building "Stripe Test Clocks" for a whole Docker Compose stack.*

Every SaaS app has code that only runs when time passes: trials that expire after 14 days, sessions that time out after 24 hours, a nightly job that downgrades accounts, webhook retries with backoff. That code is usually tested by editing timestamps in the database, by shortening durations with an environment flag, or not at all.

Stripe solved this for its own objects with [Test Clocks](https://docs.stripe.com/billing/testing/test-clocks): create a clock, advance it a month, and watch subscriptions renew. I wanted the same for my *own* stack: Postgres `now()`, Redis key expiry, a Node API and a worker, all moved forward together, with the jobs that depend on time actually running.

The obvious approach is a fake clock. It's an old idea ([libfaketime](https://github.com/wolfcw/libfaketime) has done it with `LD_PRELOAD` for years). The interesting part was everything the fake clock breaks or doesn't do. Here are four things I learned building [ChronoStack](https://github.com/Asmodeus14/ChronoStack).

### 1. Advancing wall-clock time does not fire timers

The first demo looked perfect: `advance 14d`, and Postgres, Redis and Node all reported a date two weeks ahead. The 03:00 daily job didn't run once.

The reason: Node's `setTimeout`, Python's `asyncio.sleep`, Java's `ScheduledExecutorService`, Postgres latches and Redis's event loop all wait on the **monotonic** clock, with relative timeouts. A job scheduled "in 7 hours" sleeps 7 *real* hours, whatever the wall clock says. And you don't want to virtualize the monotonic clock: every timeout in every library would fire at once.

So the design became **step → trigger → barrier**:

1. move the clock to the next interesting virtual time (for example 03:00 on each day);
2. run the job you declared, as a command in the container or an HTTP call: *a trigger*;
3. wait until the system has settled (for example "no webhook is due and unsent"): *a barrier*;
4. repeat.

```yaml
triggers:
  - name: expire-trials
    schedule: { every: 1d, at: "03:00" }      # virtual time
    exec: { service: worker, command: [node, src/jobs/expire-trials.js] }
```

`advance 14d` then runs the job 14 times, each at its own virtual 03:00. Schedulers that *poll* wall-clock time behave differently. They notice the jump on their own, and I measured which ones do: BullMQ fires within about 1 s and Celery beat within about 2 s (with `beat_max_loop_interval=1`). node-cron and APScheduler never fire, so they need a trigger ([measurements](../MVP_SCOPE.md)).

### 2. Faking the clock can hang your process

This one surprised me most. Many APIs take an **absolute** deadline in wall-clock time, for example `sem_timedwait`, `pthread_cond_timedwait`, `pthread_mutex_timedlock`, `clock_nanosleep(..., TIMER_ABSTIME)`, `timerfd` and Java's `LockSupport.parkUntil`. A program computes "now + 1 s" from the *fake* clock and hands that deadline to the kernel. The kernel compares it with the *real* clock. Fourteen days ahead, a one-second wait becomes a fourteen-day wait.

I measured it: with deadline translation off, every one of those APIs hung at +1 day. With the preload library translating absolute deadlines back to real time, all of them return in about a second. That holds in C (glibc and musl), Python, Node and the JVM, at +1 day, +30 days, +365 days and with the clock frozen: 272 of 272 checks, on x86_64 and arm64, every night in CI ([results](../M0_RESULTS.md)). The overhead of the shim is about 12–14 ns per clock read.

### 3. pg_cron silently skips jobs after a big jump

pg_cron reuses Vixie cron's logic. If the clock jumps more than 3 hours, Vixie cron treats it as a clock *correction* and runs only the jobs due in the current minute. So `advance 3d` over a daily `0 3 * * *` job ran it **zero** times.

The first fix was to stop every hour (hourly sub-steps), which ran the job 3 times but took 92 s for 3 virtual days. The better fix: read `cron.job`, compute each job's next minutes, and stop the clock exactly there. That gave 3 runs in **6.4 s** ([details](../M2_RESULTS.md)). Jobs like `*/5 * * * *` still run once per stop, and ChronoStack now tells you so ("due 4320 times, will run about 4") instead of letting it happen silently.

### 4. Even the real clock isn't monotonic

The first version computed virtual time as `CLOCK_REALTIME + offset`. Under WSL2 the host's real clock stepped **backwards** by about 2 ms in 4 of 6 twenty-second test windows, and those corrections leaked into virtual time. Virtual time is now anchored to `CLOCK_MONOTONIC` (`virtual = monotonic + offset`), stored in a 64-byte shared page that one controller writes and every process reads through a seqlock ([ADR-017](../ARCHITECTURE_DECISIONS.md)).

### What it looks like now

On a small SaaS demo (Postgres, Redis, a Node API and a worker), one scenario:

- signs a user up;
- takes the partner's webhook endpoint down on day 14 from 02:00 to 06:00 (through a small fault proxy, in *virtual* time);
- advances 14 days;
- checks that the trial expired at the 03:00 job, and that the `trial.expired` webhook was retried during the outage and delivered **exactly once**, at 07:00.

That's fifteen virtual days in 10–15 s of real time on CI machines. When the worker has a bug (treating a 503 as delivered), the same scenario fails, and the report says so in plain words: the receiver never got the webhook, and the only request was a 503 during the outage.

AI agents can drive it too, through an MCP server ("verify webhook retries during a 2-hour outage"), and Node apps running outside Docker can follow the virtual clock with the Node register (`node -r sdk/node/register.js app.js`).

### What it can't do (yet)

- **Go binaries** read the clock without libc, so `LD_PRELOAD` can't reach them. `doctor` flags them, including Go servers hidden behind a C supervisor.
- **macOS** hasn't been tested yet. If you have a Mac, [issue #5](https://github.com/Asmodeus14/ChronoStack/issues/5) has a 5-minute test.
- Time only moves forward.

It's open source (MIT OR Apache-2.0) and pre-alpha: [github.com/Asmodeus14/ChronoStack](https://github.com/Asmodeus14/ChronoStack). I'd love to hear how you test time-dependent code today, and whether this works on your stack (`chronostack doctor` output welcome).
