# Quiet beta kit

**Window:** about 3 weeks, from 2026-11-16 to 2026-12-06. Launch (Show HN, Reddit) comes after, per [PLAN.md](PLAN.md).

**Goal:** real developers run ChronoStack on *their own* Compose project and tell us what happened. We fix what breaks before the one-shot public launch.

## Exit criteria (ready to launch)

- [ ] At least 15 beta reports from people other than the maintainer.
- [ ] At least 3 reports from macOS, Apple silicon, with `doctor` all green. This closes [issue #5](https://github.com/Asmodeus14/ChronoStack/issues/5).
- [ ] At least 70% of reports say "worked on the first try".
- [ ] Every "No" report is fixed, or documented as a known limitation in the guide.

## Links to hand out

- Website: https://chronostack-dev.vercel.app
- Beta report form: https://github.com/Asmodeus14/ChronoStack/issues/new?template=beta_report.yml
- Getting started: https://github.com/Asmodeus14/ChronoStack/blob/main/docs/GETTING_STARTED.md

What we ask testers to do (5 minutes):

```sh
# install: the one-liner from the website for their OS, then in their own compose project:
chronostack up --wait
chronostack doctor
chronostack advance 1d     # optional: did your scheduled jobs run?
chronostack down
```

## Where to find testers

Ranked by how likely a request turns into a report.

1. **People you know** who run Docker Compose at work or on side projects. Ask one at a time, by name. Highest yield by far.
2. **People who already have the problem.** Search these, and reply only where ChronoStack genuinely helps. Say that you built it.
   - X / Bluesky / Mastodon: `"stripe test clocks"`, `"test trial expiry"`, `"libfaketime" docker`, `"freezegun" postgres`, `"time travel" testing docker`.
   - GitHub: issues on libfaketime, freezegun and time-machine that mention Docker, Compose, Postgres `now()` or multiple services.
   - Stack Overflow and Reddit: "how to test subscription expiration", "test cron job docker compose", "mock postgres now()".
3. **Communities you're already part of** (Discord, Slack, forums): post once, in the channel meant for sharing projects, following each community's rules. Good fits are Node.js, Python, Rust and SaaS-builder communities, and Indie Hackers.
4. **Mac users, specifically.** Every message below has a variant for them. Until three Mac reports are in, this is the priority.

Keep Reddit, Hacker News and Product Hunt for the launch itself. A beta post there spends the launch.

## Messages

Edit freely; keep them short and personal. Each one asks for a single concrete thing.

**A. To someone you know**

> Hey [name], I've built an open-source tool that moves a Docker Compose stack forward in time (Postgres `now()`, Redis expiry, your services), so you can test things like trial expiry without waiting 14 days: https://chronostack-dev.vercel.app
>
> Would you try it on one of your compose projects? It takes about 5 minutes: install, then `chronostack up --wait` and `chronostack doctor`. Whether it works or breaks, the doctor output would help me a lot: [beta form link]. It runs a separate copy and never touches your containers, data or your machine's clock.

**A-Mac. Add for Mac users**

> You'd be one of the first to run it on macOS. It's untested there, so even "it failed at step 1" is useful.

**B. Community post (project-sharing channel)**

> I'm looking for 10–20 people to try a pre-alpha tool on their own Docker Compose project. ChronoStack is "Stripe Test Clocks for your stack": advance Postgres, Redis and your services in virtual time together, and the cron jobs that depend on it actually run. Open source (MIT/Apache-2.0), no code changes, never touches your host clock.
>
> Ask: install, run `chronostack up --wait && chronostack doctor` on your project, and fill in a short form, especially if it breaks. Mac users are extra welcome (macOS is untested).
> Site: https://chronostack-dev.vercel.app · Form: [beta form link]

**C. Reply to someone who has the problem**

> I ran into the same thing testing trial expiry across Postgres and a job queue, and ended up building an open-source tool for it: it advances the whole Compose stack in virtual time and runs the scheduled jobs at the right moments. It's early, but it might fit what you described: https://chronostack-dev.vercel.app. Happy to help set it up if you try it.

**D. Thank-you, after a report**

> Thank you, this is exactly what I needed. [What you learned / what you'll change.] I'll ping you here when it's fixed so you can try again.

**E. After a fix ships**

> Fixed in [version]: [one line]. If you have 5 minutes, `chronostack update` and the same two commands should now go green. Thanks again for the report.

## Tracker

One row per person asked. Fill in "report" with the issue number when they post.

| Date | Who / where | Host | Asked via | Report | First try | Status |
|---|---|---|---|---|---|---|
| | | | | | | |

## Weekly loop (every Monday)

1. Triage new `beta-report` issues. Add a label for the cause: `macos`, `scheduler-didnt-run`, `hang`, `unsupported-image`, `docs`.
2. Fix the most common cause first. Ship an alpha (`chronostack update` picks it up).
3. Ping each affected reporter with message E.
4. Update the counts against the exit criteria above.
5. Turn every scheduler report into a row in the scheduler table in the [guide](../GUIDE.md#4-make-your-scheduled-jobs-run).
