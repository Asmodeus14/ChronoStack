<!-- Opened as https://github.com/saleor/saleor/pull/20020 on 2026-11-15 (branch fix/expire-orders-after-day on Asmodeus14/saleor). -->

**Title:** Fix order expiry when expireOrdersAfter is a day or more

I want to merge this change because it fixes #20019. Unpaid orders expire far too late when a channel's `orderSettings.expireOrdersAfter` is 24 hours or more.

`_expire_orders` worked out the order age in minutes as `days * 24 + hours * 60 + minutes`. The days term was converted to hours, not minutes, so once an order was a day old it looked much younger than it was. A 24-hour setting expired orders after about 47.6 hours, and a 48-hour setting after about 62 days. The fix multiplies the day and hour part by 60:

```python
(day * 24 + hour) * 60 + minute
```

**Tests.** I added `test_expire_orders_task_expire_after_one_day_or_more`, parametrised over 1-day, 2-day and 1-week settings, with orders just under and over the limit. Without the fix, the 4 cases that should expire fail; the 2 just-under cases pass either way and guard against expiring too early. With the fix, all 25 tests in `saleor/order/tests/test_tasks.py` pass, and `ruff check`, `ruff format --check` and `mypy` are clean on the changed files.

# Impact

- [ ] New migrations
- [ ] New/Updated API fields or mutations
- [ ] Deprecated API fields or mutations
- [ ] Removed API types, fields, or mutations

# Docs

No docs change needed: this makes `expireOrdersAfter` behave as already documented.

- [ ] Link to documentation:

# Pull Request Checklist

- [x] Privileged queries and mutations are either absent or guarded by proper permission checks
- [x] Database queries are optimized and the number of queries is constant
- [x] Database migrations are either absent or optimized for zero downtime
- [x] The changes are covered by test cases
- [x] All new fields/inputs/mutations have proper labels added (`ADDED_IN_X`, `PREVIEW_FEATURE`, etc.)
- [x] All migrations have proper dependencies
- [x] All indexes are added concurrently in migrations
- [x] All RunSql and RunPython migrations have revert option defined
