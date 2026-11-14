# Repository Setup & Metadata (plan)

This records what was set up during the documentation phase, what is deliberately deferred, and the metadata the owner should apply on GitHub.

## Added now

| File | Why now |
|---|---|
| `.gitignore` | Avoid committing build output, `.chronostack/` session data (contains a token) and editor files |
| `.gitattributes` | Normalize line endings; the repository is being authored on Windows and will be built on Linux |
| `.editorconfig` | Consistent whitespace across Markdown, YAML, Rust and C |
| `.github/` | Templates, CI skeleton, Dependabot (see [OPEN_SOURCE_READINESS.md](../OPEN_SOURCE_READINESS.md)) |

## Deferred until M1 (no code yet, so these would only be for appearance)

**`Cargo.toml` workspace** (proposed content, to create in M1):

```toml
[workspace]
resolver = "2"
members = ["crates/cli", "crates/clockd", "crates/core"]

[workspace.package]
version = "0.1.0-alpha.0"
edition = "2021"          # re-evaluate edition 2024 at M1
rust-version = "TBD"      # set MSRV at M1
license = "MIT OR Apache-2.0"   # pending owner decision (LICENSING.md)
repository = "TBD"        # owner to supply once the GitHub repo exists
homepage = "TBD"
description = "Stripe Test Clocks for your Docker Compose stack"
keywords = ["testing", "time", "docker-compose", "faketime", "devtools"]
categories = ["development-tools::testing", "command-line-utilities"]

[workspace.lints.rust]
unsafe_code = "deny"       # crates needing unsafe (mmap) opt out explicitly and document why

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
```

- **`rustfmt.toml`:** not needed. The default rustfmt style is enough; add one only if a specific option is agreed on.
- **`clippy.toml`:** not needed. Workspace lints cover the policy, and clippy reads the MSRV from `rust-version` (1.85).

**npm package (v0.2, not published):**

```json
{
  "name": "chronostack",
  "version": "0.0.0",
  "private": true,
  "license": "(MIT OR Apache-2.0)",
  "description": "Node SDK for ChronoStack (planned)",
  "exports": { "./register": "./register.js" }
}
```

- Keep `"private": true` until the release process is validated.
- The package name `chronostack` has **not** been checked or reserved on npm. That is an owner action.

## GitHub repository metadata (owner applies in repository settings)

- **Description** (≤ 350 chars): `Stripe Test Clocks for your Docker Compose stack: advance time across Postgres, Redis and your services, run the jobs that depend on it, and verify the results. (Pre-alpha; first pre-release v0.1.0-alpha.2.)` Applied 2026-11-14.
- **Topics:**
  - `testing`, `integration-testing`, `time-travel`, `faketime`, `ld-preload`
  - `docker-compose`, `postgresql`, `redis`
  - `developer-tools`, `rust`, `mcp`, `saas`
- **Features:**
  - enable Issues and Discussions (categories: Announcements, Q&A, **Ideas**);
  - enable private vulnerability reporting;
  - Wiki off.
- **Branch protection on `main`** (applied 2026-11-13):
  - required status checks: `Rust (ubuntu-latest)`, `Rust (ubuntu-24.04-arm)`, `Rust (windows-latest)`, `Rust (macos-latest)`, `MSRV (1.85)`, `End-to-end (ubuntu-latest)`, `End-to-end (ubuntu-24.04-arm)`;
  - the `docs` link check is **not** required: it only runs when Markdown changes, so requiring it would block other pull requests;
  - no force pushes, no branch deletion;
  - pull requests are not mandatory and admins are not restricted (owner's choice), so maintainers can still push directly.

## Social preview image

Created: [`docs/assets/social-preview.png`](../assets/social-preview.png) with the ChronoStack logo (see [brand](../assets/brand/README.md)), text in Cascadia Mono (SIL Open Font License). Upload it in **Settings → General → Social preview** (GitHub has no API for this). Concept:

- Size: 1280×640.
- Dark background with a horizontal timeline.
- Four service "lanes" (Postgres, Redis, API, Worker) all ticking to the same marker labelled `Day 14 · 03:00`.
- A clock glyph sits on the controller lane.
- Wordmark "ChronoStack" with the tagline "Stripe Test Clocks for your Docker Compose stack".
- No logos of third-party products unless their trademark guidelines allow it.

## Repository initialisation (done 2026-11-13)

The repository was initialised, pushed and made public by the owner on 2026-11-13. The original sequence is kept for reference:

1. `git init -b main`
2. Review the files.
3. Make the initial commit.
4. Create the GitHub repository (keep it **private** until the readiness checklist is complete).
5. Push.
6. Apply the metadata above.
