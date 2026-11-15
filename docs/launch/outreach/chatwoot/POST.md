<!-- Posted as https://github.com/chatwoot/chatwoot/issues/16667 on 2026-11-15. -->

**Title:** Snoozed conversations are never reopened if the snooze ended more than 3 days before the job runs

### Describe the bug

`Conversations::ReopenSnoozedConversationsJob` only reopens conversations whose `snoozed_until` is in the last 3 days:

```ruby
Conversation.where(status: :snoozed).where(snoozed_until: 3.days.ago..Time.current).all.find_each(batch_size: 100, &:open!)
```

If the job doesn't run for more than 3 days, for example a self-hosted instance whose Sidekiq is down over a long weekend, every conversation whose snooze ended during that gap stays snoozed permanently. The job runs every 5 minutes afterwards, but those conversations are outside the window, so the agent's "snooze until" reminder never fires. The same applies to a conversation snoozed (through the API) until a time more than 3 days in the past, since `snoozed_until` isn't validated to be in the future.

`TriggerScheduledItemsJob` uses the same `3.days.ago..Time.current` window for one-off campaigns, so a campaign scheduled during such a gap is never sent either (not tested).

### To Reproduce

1. Snooze a conversation until a time T.
2. Stop Sidekiq before T and start it again more than 3 days after T.
3. The reopen job runs every 5 minutes, but the conversation stays snoozed.

I reproduced this on the published `chatwoot/chatwoot:v4.19.1` image (Sidekiq with the schedule in `config/schedule.yml`, Postgres, Redis), moving the whole stack's clock forward to simulate the gaps. Sidekiq's log confirms the job didn't run during each gap and ran every 5 minutes after it:

| Case | Snooze ended | Job next ran | Result |
|---|---|---|---|
| Normal | 02:10 | 02:15 | reopened |
| 2-day gap | day 0, 02:25 | day 2, 02:20 | reopened |
| 4-day gap | day 2, 02:31 | day 6, 02:25 | **still snoozed**, and still snoozed after another week of the job running every 5 minutes |

### Expected behavior

A snoozed conversation is reopened on the first run of the job after its snooze ends, however long ago that was.

Suggested fix (verified): drop the lower bound.

```ruby
Conversation.where(status: :snoozed).where(snoozed_until: ..Time.current).all.find_each(batch_size: 100, &:open!)
```

With this change applied in the Sidekiq container, the 4-day case is reopened on the first run after the gap, and the other cases behave as before. If the 3-day bound is there to limit the query, an index on `(status, snoozed_until)` would keep it cheap.

### Environment

Self-hosted, Docker: `chatwoot/chatwoot:v4.19.1`, `pgvector/pgvector:pg16`, `redis:7.4-alpine`. The code is unchanged on `develop` as of 2026-11-15.

### Additional context

Compose file, seed script, scenarios and run reports: https://github.com/Asmodeus14/ChronoStack/tree/main/docs/launch/outreach/chatwoot

Happy to open a PR with the fix and a spec.
