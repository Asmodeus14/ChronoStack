# SaaS trial demo

A small app with the time-dependent behaviour most SaaS products have:

| Rule | Where | Time source |
|---|---|---|
| A trial lasts **14 days** | `users.trial_ends_at DEFAULT now() + 14 days` | Postgres `now()` |
| A session lasts **24 hours** | `SET session:<token> … EX 86400` | Redis TTL |
| A **daily job at 03:00 UTC** downgrades expired trials | `src/jobs/expire-trials.js`, scheduled by `src/worker.js` with `setTimeout` | Node + Postgres |

Nothing in the app knows about ChronoStack. [`chronostack.yml`](chronostack.yml) adds one trigger that runs the daily job at every virtual 03:00. The worker's own `setTimeout` can't do this: it runs on the monotonic clock, and ChronoStack deliberately leaves that clock alone.

## Run it

From this directory, after building the controller image and installing the CLI (see the main [README](../../README.md#try-it-from-source-pre-alpha)):

```sh
chronostack up
chronostack doctor

# sign up a user (the helper runs inside the api container)
TOKEN=$(docker compose -f compose.yml -f .chronostack/compose.override.json -p saas-trial-demo-chronostack \
          exec -T api node scripts/call.js signup alice@example.com)

chronostack advance 25h     # the 24h session expires; the trial is still active
chronostack advance 14d     # expire-trials runs at each virtual 03:00; the trial ends on day 14
chronostack advance 14d --dry-run   # show the plan without running it

chronostack down -v
```

The end-to-end test [`tests/e2e/run_m2.py`](../../tests/e2e/run_m2.py) runs exactly this flow and checks the results.
