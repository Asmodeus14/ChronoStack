<!-- Draft GitHub issue for https://github.com/saleor/saleor/issues (bug report template). Not posted yet. -->

**Title:** Bug: unpaid orders expire far too late when expireOrdersAfter is 24 hours or more

### What are you trying to achieve?

Expire unconfirmed, unpaid orders after the time set in the channel's `orderSettings.expireOrdersAfter` (minutes). With values of one day or more, orders expire much later than configured: 24 h expires after about 47.6 h, and 48 h after about 62 days.

### Steps to reproduce the problem

The cause is in `saleor/order/tasks.py`, `_expire_orders`:

```python
time_diff_func_in_minutes = (
    Func(Value("day"), now - OuterRef("created_at"), function="DATE_PART") * 24
    + Func(Value("hour"), now - OuterRef("created_at"), function="DATE_PART") * 60
) + Func(Value("minute"), now - OuterRef("created_at"), function="DATE_PART")
```

This computes `days*24 + hours*60 + minutes`. The days term is converted to hours, not minutes, so it only equals the age in minutes while the order is under a day old. Once an order is a day old the value drops (23 h 59 min is 1439, 24 h 0 min is 24), and the comparison `expire_orders_after <= time_diff` is reached much later:

| `expireOrdersAfter` | Expires after (computed) | Observed on 3.23.49 |
|---|---|---|
| 60 | 1 h | 1 h ✓ |
| 1440 (24 h) | 47 h 36 min | not expired at 24 h 5 min and 47 h 30 min; expired by 47 h 40 min |
| 2880 (48 h) | 61 d 23 h 36 min | not expired at 48 h 5 min and 61 d 23 h 30 min; expired by 61 d 23 h 40 min |

To reproduce:

1. Create a channel with `expireOrdersAfter: 1440`.
2. Create an unconfirmed order in it (for example from a checkout with no payment).
3. 24 h later, `expire_orders_task` runs and leaves it `unconfirmed`. It expires only on the run after 47 h 36 min.

The existing tests in `saleor/order/tests/test_tasks.py` use orders 10 or 120 minutes old, so the "over a day old" path isn't covered.

**Suggested fix** (verified, see below): multiply the day and hour part by 60:

```python
time_diff_func_in_minutes = (
    Func(Value("day"), now - OuterRef("created_at"), function="DATE_PART") * 24
    + Func(Value("hour"), now - OuterRef("created_at"), function="DATE_PART")
) * 60 + Func(Value("minute"), now - OuterRef("created_at"), function="DATE_PART")
```

Alternatively, compare `created_at` directly against `now - expire_orders_after * interval '1 minute'`. A test with an order created `timedelta(days=1, hours=1)` ago and `expire_orders_after = 1440` would cover it.

### What did you expect to happen?

An order expires on the first `expire_orders_task` run after `created_at + expireOrdersAfter` minutes, whatever the value.

### Logs

`expire_orders_task` runs and succeeds every time (`succeeded in 0.12s`) without expiring the order. There are no errors.

### Environment

Saleor version: 3.23.49 (`ghcr.io/saleor/saleor:latest`, image built 2026-11-12); the code is unchanged on `main` as of 2026-11-14.
OS and version: Docker (Linux), Postgres 15.7, Redis 7.

**How it was tested.** I ran the published image (Celery worker and beat with your `DatabaseScheduler`, Postgres, Redis) under [ChronoStack](https://github.com/Asmodeus14/ChronoStack), an open-source tool that moves a Docker Compose stack forward in virtual time, so beat's own 5-minute `expire_orders_task` handled each order with no mocks:

- One scenario pins the current behaviour above to the minute (15/15 checks pass on 3.23.49).
- A second scenario expects documented behaviour (expiry after 60 min, 24 h and 48 h). It fails on 3.23.49 at the 24 h check, and passes 8/8 with the one-line fix above applied in the worker.

Compose file, seed script, both scenarios and the three run reports: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/saleor

Happy to open a PR with the fix and a test if that helps.
