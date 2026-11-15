# Scenario report: Snoozed conversations after an outage, as expected

sidekiq-cron runs TriggerScheduledItemsJob every 5 minutes, which runs Conversations::ReopenSnoozedConversationsJob: it reopens snoozed conversations whose snoozed_until is between 3 days ago and now. sidekiq-cron polls every 30 s and skips a run it notices more than 60 s late, so each check lands just after a 5-minute mark and waits 45 s. A jump with no landing near a 5-minute mark is an outage: the job doesn't run. Status 0 = open, 3 = snoozed. Expected: a snoozed conversation reopens on the first run of the job after its snooze ends, however long the job was down.

**Result: PASSED ✅**  
Virtual time: 2026-11-15T15:51:15Z → 2026-11-22T02:25:46Z  
Real time: 178.8 s (started 2026-11-15T15:51:15Z)  
Scenario file: `scenarios/snooze-reopen-as-expected.yml` · ChronoStack project `chatwoot-chronostack`

## Steps

| # | Step | Virtual time (after) | Result | Details |
|---|---|---|---|---|
| 1 | day 0, 02:00 UTC | 2026-11-16T02:00:00Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 2 | snooze conversation A for 10 minutes | 2026-11-16T02:00:11Z | ✅ passed | 1 (saved as ${a}) |
| 3 | day 0, 02:15 (the job runs normally) | 2026-11-16T02:15:45Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 4 | A reopened on time | 2026-11-16T02:15:46Z | ✅ passed | = 0 |
| 5 | snooze conversation B for 10 minutes | 2026-11-16T02:15:56Z | ✅ passed | 2 (saved as ${b}) |
| 6 | a 2-day outage (B's snooze ends during it) | 2026-11-18T02:15:57Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 7 | day 2, 02:20 — the job runs again | 2026-11-18T02:20:45Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 8 | B (snooze ended 1 d 23 h 54 min ago) is reopened | 2026-11-18T02:20:46Z | ✅ passed | = 0 |
| 9 | snooze conversation C for 10 minutes | 2026-11-18T02:20:56Z | ✅ passed | 3 (saved as ${c}) |
| 10 | a 4-day outage (C's snooze ends during it) | 2026-11-22T02:20:57Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 11 | day 6, 02:25 — the job runs again | 2026-11-22T02:25:45Z | ✅ passed | 1 stop(s), 0 trigger run(s) |
| 12 | C (snooze ended 3 d 23 h 54 min ago) is reopened | 2026-11-22T02:25:46Z | ✅ passed | = 0 |

## Requests through the fault proxy

None.

## Outage windows

None.

## Services at the end

| Service | Sees virtual time? | Detail |
|---|---|---|
| migrate | completed | one-shot container exited with code 0; nothing to probe |
| postgres | ok |  |
| redis | ok |  |
| sidekiq | ok |  |

## Reproduce

```sh
chronostack run scenarios/snooze-reopen-as-expected.yml
```

The JSON version of this report has every clock stop, trigger output and event.
