<!-- Draft for https://github.com/lukevella/rallly/discussions, category: General. Not posted yet. -->

**Title:** Tested Rallly's housekeeping crons across 6 weeks of virtual time (scenario included)

Hi! I'm building [ChronoStack](https://github.com/Asmodeus14/ChronoStack), an open-source tool that moves a whole Docker Compose stack forward in time together (Postgres `now()`, Node's `Date.now()`, …) and runs scheduled jobs at their virtual times.

Rallly's house-keeping jobs are a nice real-world case, because `autoClosePolls` decides with Postgres's `now()` while `deleteInactivePolls` and `removeDeletedPolls` decide with Node's `Date.now()`. Both clocks have to agree for the poll lifecycle to work. So I ran the published `lukevella/rallly:latest` image, unmodified, with the crons from `apps/web/vercel.json` firing at their virtual times, and checked one poll through its whole life:

| Virtual time | Expected | Result |
|---|---|---|
| Day 0 | Poll created, only option on day 2, 10:00–11:00 | open ✓ |
| Day 2, 11:00 | Option just ended, next auto-close run is 11:15 | still open ✓ |
| Day 2, 11:15 | `auto-close-polls` closes it (`poll_closed` activity at 11:15) | closed ✓ |
| Day 33 | 30 days inactive: `delete-inactive-polls` | `deleted = true` ✓ |
| Day 41 | 7 days after soft delete: `remove-deleted-polls` | row gone ✓ |

12/12 checks passed. About 1,000 cron runs over 6 virtual weeks took under 7 minutes of real time on my machine. Nothing was broken; I'm sharing it in case it's useful as a regression test for those jobs, e.g. in CI.

<details>
<summary>The config (cron triggers)</summary>

```yaml
# chronostack.yml, next to a compose file running lukevella/rallly + postgres
version: 1
triggers:
  - name: auto-close-polls          # vercel.json: "15 * * * *"
    schedule: { every: 1h, at: "00:15" }
    exec: { service: rallly, command: [curl, -fsS, --noproxy, "*", -H, "Authorization: Bearer <CRON_SECRET>", "http://localhost:3000/api/house-keeping/auto-close-polls"] }
  - name: delete-inactive-polls     # "0 6 * * *"
    schedule: { every: 1d, at: "06:00" }
    exec: { service: rallly, command: [curl, -fsS, --noproxy, "*", -H, "Authorization: Bearer <CRON_SECRET>", "http://localhost:3000/api/house-keeping/delete-inactive-polls"] }
  - name: remove-deleted-polls      # "30 6 * * *"
    schedule: { every: 1d, at: "06:30" }
    exec: { service: rallly, command: [curl, -fsS, --noproxy, "*", -H, "Authorization: Bearer <CRON_SECRET>", "http://localhost:3000/api/house-keeping/remove-deleted-polls"] }
```

</details>

The full scenario file, compose file and run report are here: <LINK TO docs/launch/outreach/rallly ONCE PUSHED>

If this would be useful in Rallly's CI, I'm happy to open a PR with it. And if anyone here is on an Apple-silicon Mac, `chronostack doctor` output from that setup would help me a lot: macOS is the one platform I haven't been able to test yet.
