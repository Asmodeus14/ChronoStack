# ChronoStack — Go-to-Market & Product Validation

No market research data (surveys, interviews) was collected in this phase. The assessments below are reasoned judgments, to be validated with real users before and after launch.

## Is the problem real and painful?

**Yes, for a specific segment.** Evidence-by-reasoning:
* Every subscription product has trial/renewal/dunning/expiry logic; Stripe built Test Clocks precisely because waiting real time is untestable.
* Common workarounds (manual DB timestamp edits, test-only endpoints, shortened durations via env) are widely used and test the wrong thing.
* The pain is **episodic**: felt intensely when a billing/expiry bug hits production, otherwise tolerated. Adoption must therefore be easy enough to try in 5 minutes during that moment.

## Best initial audience

SaaS backend teams (2–30 engineers) using **Node or Python + Postgres + Redis + a job queue** in docker-compose, with subscription/trial/expiry logic. Secondary: platform/QA engineers building lifecycle tests in CI.

## Personas

1. **Maya — SaaS backend engineer.** Owns billing & trials in a Node/BullMQ/Postgres app. Just shipped a bug where trial-expiry emails went out a day late. Wants to run "14 days" before each release. Values: zero app changes, fast feedback.
2. **Jordan — platform/QA engineer.** Maintains CI for a Python/Celery product. Wants nightly lifecycle tests (renewals, retention purges) with artifacts when they fail. Values: reliability, strict mode, reports, no flakiness.
3. **Sam — solo/indie dev working with AI agents.** Uses Claude Code/Cursor heavily. Wants the agent to verify "what happens after 30 days" on its own. Values: MCP, one-command install, clear output the agent can read.

## Usage scenarios

1. **Trial expiry**: sign up → advance 14 d → downgraded, email job enqueued once.
2. **Session / token expiry**: Redis sessions and JWT refresh windows after 25 h / 30 d.
3. **Dunning + webhook retry during outage**: payment-failed webhook receiver down for 2 h on day 3; retries with backoff; delivered exactly once.
4. **Data retention purge**: GDPR deletion after 30 d; assert rows gone and audit log written.
5. **Month-end batch**: invoices generated at month boundary; advance across Jan 31 → Feb 1 and DST change.
6. **API key / certificate expiry warnings**: notifications at 7 d and 1 d before expiry.
7. **Agent-driven exploration**: "check what happens to inactive accounts after 90 days" executed via MCP.

## Adoption barriers & responses

| Barrier | Response |
|---|---|
| "LD_PRELOAD is hacky/dangerous" | Small auditable C shim, read-only, no privileges, fail-open, doctor output; write-up explaining the design |
| "Our scheduler didn't fire" | Clear CLI warnings + trigger docs + per-library table (the #1 support issue to pre-empt) |
| Go services | Honest unsupported label; roadmap |
| Installation friction | Single binary + controller image pulled automatically; works on Docker Desktop/OrbStack |
| Security review in companies | No network egress, no telemetry, local-only API with token |
| "We already inject clocks" | Integration-level coverage across DB/cache/cron |

## Channel assessment (no promises)

| Channel | Likely angle | Realistic expectation |
|---|---|---|
| Hacker News (Show HN) | Technical write-up: "Why advancing the clock doesn't run your cron jobs" / deadline-hang problem / Docker Desktop VM boundary | Interesting technical posts about time get engagement; a bare tool announcement is a coin flip. |
| r/rust | Architecture write-up (seqlock, controller, why the shim is C) | Moderate interest; expect scrutiny over the C shim choice. |
| r/docker, r/selfhosted | Demo GIF, compose-only setup | r/docker moderate; r/selfhosted low relevance (not a self-hosted app). |
| Dev.to / blogs | Tutorial: "Test your Stripe-style trial flow in 60 seconds" | Good for long-tail search traffic. |
| GitHub | README quality, demo GIF, honest compatibility table | Stars follow launch posts; retention follows "works first try". |

What makes developers notice: a 20-second demo of 14 days passing with real Postgres/Redis/worker output. What makes them adopt: it works on their own compose project in under 5 minutes, and failures explain themselves.

## Launch checklist (v0.1)

README leading with one-liner + GIF; compatibility table generated from CI; limitations section above the fold; example repo; Show HN technical post; MCP config snippet for Claude Code/Cursor; issue templates asking for compose snippet + `chronostack doctor` output.

## Validation before/after launch

* Before M2: 5 short conversations with SaaS developers — "how do you test trial expiry today?" — record answers.
* After launch: track issues labeled `scheduler-didnt-run`, `hang`, `unsupported-image` to prioritize v0.2.
