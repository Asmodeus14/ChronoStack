# Lago v1.57.0: thirteen months of billing, no bugs found

Run on 2026-11-15 with ChronoStack v0.1.0-alpha.4 against the published `getlago/api:v1.57.0` image (API, Sidekiq worker, Clockwork clock, `getlago/postgres-partman:15.0-alpine`, Redis 7). Setup: [compose.yml](compose.yml), [seed/seed.sh](seed/seed.sh), [chronostack.yml](chronostack.yml) (a Sidekiq-idle barrier), scenario [scenarios/billing-15-months.yml](scenarios/billing-15-months.yml). Run `seed/make-env.sh` once first; it writes a throwaway RSA key to `.env`.

## What was tested

Six subscriptions in arrears, 10.00 USD per period, created through Lago's REST API:

| Subscription | Plan | Billing | Starts |
|---|---|---|---|
| m31-utc | monthly | anniversary | 2026-12-31 |
| m30-ny | monthly | anniversary | 2026-12-30 |
| m29-syd | monthly | anniversary | 2026-12-29 |
| cal-ny | monthly | calendar | 2026-12-15 |
| q30-utc | quarterly | anniversary | 2026-11-30 |
| y31-utc | yearly | anniversary | 2026-12-31 |

Customer time zones (`America/New_York`, `Australia/Sydney`) are a premium feature and were ignored, so every customer billed in UTC.

Lago's own Clockwork ran the hourly billing jobs. The clock landed every day at 12:10 UTC, just after the `*:10` billing run, and a barrier waited until Sidekiq was idle at each stop.

## Result

The run was meant to reach 2028-03-05. It was stopped at **2028-01-23** because the host ran low on memory (not a test failure). The invariants were checked on the data it produced:

| Check | Result |
|---|---|
| Each subscription's billing periods join up: no gaps, overlaps or duplicates | 0 problems |
| Every anniversary period billed in full (no stray proration) | 0 wrong amounts |
| Invoices per subscription at 2028-01-23 | m29, m30, m31: 12 each (January to December 2027); cal-ny: 13; q30-utc: 4 (Feb, May, Aug, Nov 2027); y31-utc: 1. All as expected |

Edge cases that behaved correctly:

- The calendar plan's first, partial month (15 to 31 December) was prorated: 5.48 USD for 17 of 31 days.
- Anniversary on the 29th in a non-leap February: period 29 January to 27 February, next period from 28 February. Nothing skipped or double-billed.
- Quarterly anniversary on the 30th: billed on 28 February, 30 May, 30 August and 30 November.

**Not covered:** billing on 29 February 2028 (leap year), and per-customer time zones.
