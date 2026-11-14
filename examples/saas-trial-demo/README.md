# SaaS trial demo

A small app with the time-dependent behaviour most SaaS products have:

| Rule | Where | Time source |
|---|---|---|
| A trial lasts **14 days** | `users.trial_ends_at DEFAULT now() + 14 days` | Postgres `now()` |
| A session lasts **24 hours** | `SET session:<token> … EX 86400` | Redis TTL |
| A **daily job at 03:00 UTC** downgrades expired trials and queues a `trial.expired` webhook | `src/jobs/expire-trials.js`, scheduled by `src/worker.js` with `setTimeout` | Node + Postgres |
| Webhooks are **retried with backoff** (20 min, 40 min, 80 min, …) until delivered | `webhook_outbox` table; `src/worker.js` checks every 500 ms which rows are due | Postgres `now()` |
| A partner endpoint receives them and counts deliveries per event | `src/receiver.js` (`HINCRBY webhook:received <id>`) | — |

Nothing in the app knows about ChronoStack. [`chronostack.yml`](chronostack.yml) adds:

- a **trigger** that runs the daily job at every virtual 03:00. The worker's own `setTimeout` can't do this: it runs on the monotonic clock, and ChronoStack deliberately leaves that clock alone;
- **barriers** so each clock stop waits until the API answers and the worker has handled every webhook that is due;
- a **proxy** named `webhooks`: in the ChronoStack copy, the worker's `WEBHOOK_URL` points at ChronoStack's fault proxy, which forwards to the receiver, so outages can be scheduled in virtual time.

## Run it

From this directory, after building the controller image and installing the CLI (see the main [README](../../README.md#try-it-from-source-pre-alpha)):

```sh
chronostack up --wait
chronostack doctor

chronostack advance 25h     # the 24h session expires; the trial is still active
chronostack advance 14d     # expire-trials runs at each virtual 03:00; the trial ends on day 14
chronostack advance 14d --dry-run   # show the plan without running it

chronostack fault set webhooks --from +1h --for 2h   # the receiver is "down" in virtual time
chronostack fault list
chronostack events          # clock changes, outage windows, proxied requests

chronostack down -v
```

## The scenario

[`scenarios/trial.yml`](scenarios/trial.yml) runs the whole story and checks it: a user signs up, the receiver is down on day 14 from 02:00 to 06:00, the trial expires at the 03:00 job, the webhook is retried during the outage and delivered exactly once at 07:00.

```sh
chronostack down -v && chronostack up --wait     # scenarios expect a fresh stack
chronostack run scenarios/trial.yml              # about 40 s; exit code 0
```

The report is written to `.chronostack/reports/<time>-<name>.md` (for people) and `.json` (everything, for tools).

### See a failure report

[`compose.bug.yml`](compose.bug.yml) adds a realistic regression: the worker treats any HTTP response, even a 503, as delivered.

```sh
chronostack down -v
chronostack -f compose.yml -f compose.bug.yml up --wait
chronostack run scenarios/trial.yml              # exit code 6
```

The report's **What failed** section says the receiver never got the webhook, and the proxy table shows a single attempt that got a 503 during the outage and was never retried.

### Writing your own scenarios: a timing tip

Retries due exactly when the clock stops can land on either side of the stop, depending on sub-second timing. Keep due times away from stop times: this demo steps one hour at a time and retries after 20, 40 and 80 minutes, so every retry is due at least 20 virtual minutes from a stop.

The end-to-end tests [`tests/e2e/run_m2.py`](../../tests/e2e/run_m2.py) and [`tests/e2e/run_m3.py`](../../tests/e2e/run_m3.py) run these flows and check the results.
