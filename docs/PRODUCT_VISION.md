# ChronoStack — Product Vision

## One-liner

**Stripe Test Clocks for your docker-compose stack.**
Advance time across Postgres, Redis and your services — and actually run the jobs that depend on it.

## Positioning decision

| Candidate | Verdict | Why |
|---|---|---|
| "Stripe Test Clocks for your entire Docker Compose stack" | **Lead with this** | Concrete, instantly understood by SaaS developers, searchable, sets correct expectations (lifecycle testing). |
| "Deterministic lifecycle and failure testing for distributed applications" | Reject as headline | Overclaims determinism (real concurrency isn't controlled), vague, invites comparison with Antithesis/FoundationDB simulation. |
| "Time travel for local dev" | Use as secondary copy | Friendly, but sounds like a toy without the lifecycle angle. |

## Problem

Time-dependent behaviour — trials, subscriptions, dunning, token/session expiry, retention purges, scheduled reports, retries with backoff — is among the least tested code in SaaS products. Today teams:

* edit timestamps in the database by hand (`UPDATE users SET trial_ends_at = now() - interval '1 day'`),
* add test-only endpoints or env flags to shorten durations,
* inject clocks in unit tests (doesn't cover DB `now()`, Redis TTL, cron),
* or wait real days in staging.

None of these exercise the real interaction of database time, cache TTLs, schedulers and services together.

## Vision

A developer points ChronoStack at an existing `docker-compose.yml` — no code or Dockerfile changes — and can:

1. Move the whole stack's wall-clock time forward, consistently and verifiably.
2. Step through days with scheduled work actually executed (polling schedulers, declared triggers) and wait for it to finish (barriers).
3. Schedule outages of downstream services on the virtual timeline to test retries.
4. Run a declarative scenario and get a reproducible pass/fail report.
5. Let an AI coding agent do all of the above through MCP.

## Principles

1. **Honest compatibility.** Every "supported" claim is backed by an automated test; limitations are listed first-class.
2. **Never touch the real clock.** No privileges.
3. **Zero changes for the common case;** one line of YAML when a scheduler needs a trigger.
4. **Verifiable.** Every advance is followed by probes that prove each service saw it.
5. **Reproducible, not "deterministic".**

## Non-goals (v0.x)

Production chaos engineering, Kubernetes, deterministic simulation, replacing unit-level fake timers, Go support (until v1.0).
