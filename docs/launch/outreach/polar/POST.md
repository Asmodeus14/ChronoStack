<!-- Draft bug report for Polar. Not sent. Polar takes bug reports through the dashboard's support (https://polar.sh/dashboard) or their Discord; GitHub issues are closed to blank reports, and code PRs need an assigned issue first (CONTRIBUTING.md). -->

**Subject:** DebounceMiddleware raises KeyError when a debounce key has expired (one-line fix)

Hi Polar team, a small bug report from testing Polar's backend (open-source `server/`, main as of 2026-11-14).

**What happens.** In `polar/worker/_debounce.py`, `DebounceMiddleware.after_process_message` does:

```python
enqueue_timestamp: int | None = message.options.pop(
    "debounce_enqueue_timestamp"
)
```

`before_process_message` only sets that option when the debounce key still exists in Redis. If the key has expired (`DEBOUNCE_KEY_TTL` is 1 hour), it returns early, and the `pop` without a default raises `KeyError: 'debounce_enqueue_timestamp'`. Dramatiq catches it and logs `Unexpected failure in after_process_message of <DebounceMiddleware>` at fatal level, so the job itself isn't lost. But the delay metric is skipped, and every occurrence is a fatal log or Sentry event.

**When it happens in production.** Whenever a debounced message is processed more than an hour after it was enqueued: a queue backlog, or a retry whose backoff pushes it past the TTL. I saw it on `customer_meter.update_customer`.

**Fix.** The annotation and the `is not None` check below it suggest the intent was:

```python
enqueue_timestamp: int | None = message.options.pop(
    "debounce_enqueue_timestamp", None
)
```

(`after_skip_message` already pops with `None`.) The existing test at `tests/worker/test_debounce.py:251` passes the option explicitly as `None`, so the "key expired, option never set" path isn't covered.

**How I found it.** I was running Polar's worker under [ChronoStack](https://github.com/Asmodeus14/ChronoStack), an open-source tool that moves a Docker Compose stack forward in time (Postgres, Redis TTLs and Python's clock together). I was testing subscription period ends. A 31-day jump expired the debounce keys of messages already in the queue. The test itself passed, and Polar's own APScheduler job store handled the jump well:

- A subscription canceled "at period end" was ended by the scheduler within about 10 s (real time) of the clock passing `ends_at`, without any triggers.
- A free monthly subscription was renewed into its next period.

The test setup (a minimal image built from `server/`, a seed script using your own `tests/fixtures` factories, and the scenario) is here: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/polar

Happy to open the one-line PR with a test if you assign me an issue.
