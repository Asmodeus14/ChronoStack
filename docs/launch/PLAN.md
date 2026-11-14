# Launch plan (v0.1 pre-releases)

**Goal (owner, 2026-11-14):** adoption as open source. Real teams running it on their own Compose projects, and CI usage. Business questions come later.

**macOS (owner):** wait for a contributor (issue #5) instead of renting a Mac. Until someone reports back, every post says macOS is untested and links the issue.

## Before posting anywhere

- [x] Publish `v0.1.0-alpha.2` (2026-11-14); the GHCR package is public (anonymous pull verified).
- [x] `action-test` passes against the published release (x86_64 and arm64).
- [x] README GIFs: quickstart, scenario and failure report (`demo-gif` workflow, rendered 2026-11-14).
- [x] Guide for your own project: [GUIDE.md](../GUIDE.md).

## 1. Quiet beta (2–3 weeks)

Ask 10–20 developers (friends, communities you're in, X/Bluesky) to run `chronostack up --wait && chronostack doctor` on their own Compose project and report back:

- did it work first try?
- the `doctor` output;
- which scheduler their jobs use.

Fix what breaks. The share that works first try is the launch-readiness signal.

## 2. Launch

1. Publish the write-up ([WRITEUP.md](WRITEUP.md)) on dev.to or a blog.
2. Show HN ([SHOW_HN.md](SHOW_HN.md)), Tuesday–Thursday morning US time. Stay in the thread for the day.
3. Over the next days: r/rust (architecture), r/node and r/Python (the trial-expiry demo), and a dev.to tutorial ("Test your Stripe-style trial flow in 60 seconds").
4. MCP Registry and community lists ([MCP_LISTING.md](MCP_LISTING.md)).

## 3. After launch

- Track release downloads and GHCR pulls (GitHub API), and issues by type: `scheduler-didnt-run`, `hang`, `unsupported-image`.
- Turn every "my scheduler didn't fire" report into a row in the scheduler table in the [guide](../GUIDE.md#4-make-your-scheduled-jobs-run).
- Homebrew tap and the npm package for the Node SDK, once macOS is confirmed.
