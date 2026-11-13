# Open-Source Readiness

**Date:** 2026-11-13 (updated after M2)
**Repository state:** public on GitHub; M0–M2 implemented and tested in CI; usable from source; **no release**.

**Classification:**

- ✅ **Ready**
- ⚠️ **Needs attention**
- ⛔ **Blocked**, pending implementation or an owner decision

## 1. Checklist

### Project documentation

| Item | Status | Notes |
|---|---|---|
| README: value proposition in the first two lines | ✅ | Tagline, plus a pre-alpha status banner |
| README: accurate status | ✅ | Implemented commands are described as such; M3–M4 features are marked *(planned)*; the compatibility table lists only what tests cover |
| README: install / quick start | ✅ | "Try it from source" (build the controller image, `cargo install`). Binary install waits for M5. |
| README: screenshots / GIF | ⚠️ | None yet; none fabricated. A mermaid diagram is included. A GIF of the demo is an M5 task. |
| Documentation index (`docs/README.md`) | ✅ | |
| Internal Markdown links and anchors | ✅ | Checked by the `docs` workflow on every push (passing) |
| External URLs | ⚠️ | Key technical URLs were opened in the second pass ([SOURCE_VERIFICATION.md](SOURCE_VERIFICATION.md)); the §8 list there is still unopened |
| Consistency across the 12 planning docs | ✅ | Resolved: engine location (ADR-016), pg_cron sub-stepping, Node SDK as a stretch goal, demo runtime (Node), API endpoints, MCP tool list, CI-mode strictness, persona naming |
| Acronyms explained | ✅ | Glossary in [docs/README.md](README.md#glossary) |
| CHANGELOG (Keep a Changelog) | ✅ | `[Unreleased]` lists M0–M2 work |

### Community and governance

| Item | Status | Notes |
|---|---|---|
| CONTRIBUTING.md | ✅ | Build, test and e2e commands; contribution paths for a pre-alpha project |
| CODE_OF_CONDUCT.md | ⛔ Owner | Adopts Contributor Covenant 3.0 by reference. **The owner must paste the full official text and provide a reporting contact.** |
| SECURITY.md | ✅ | GitHub private vulnerability reporting enabled (2026-11-13) |
| LICENSE | ✅ | `MIT OR Apache-2.0`, decided by the owner on 2026-11-13 ([LICENSING.md](development/LICENSING.md)). MIT copyright line is "The ChronoStack Contributors"; the owner may change it. |
| Issue templates (bug, feature) | ✅ | |
| PR template (testing + docs checklists) | ✅ | |
| Discussions form (`ideas.yml`) | ✅ | Discussions enabled; the **Ideas** category (slug `ideas`) exists |
| Maintainers / governance | ⛔ Owner | No maintainers are listed. Add a `MAINTAINERS` section or `CODEOWNERS` once known. |

### CI and tooling

| Item | Status | Notes |
|---|---|---|
| `documentation.yml` (offline link check) | ✅ | Runs on every push; passing |
| `ci.yml` | ✅ | fmt, clippy (`-D warnings`), unit tests on Linux x86_64/arm64, Windows and macOS, MSRV 1.85, and e2e on native Linux x86_64 and arm64 |
| `nightly.yml` | ✅ | Demo ×10 (determinism), scheduler matrix and no-hang suite on x86_64 and arm64 |
| Dependabot | ✅ | `github-actions`, `cargo` and `docker`. An open cargo PR (major `getrandom`/`ureq` bumps) fails CI and needs a code change. |
| `.gitignore`, `.gitattributes`, `.editorconfig` | ✅ | |
| rustfmt / clippy | ✅ | Default rustfmt style; clippy with `-D warnings` in CI; workspace lint `unsafe_code = "deny"` |
| Cargo workspace metadata | ✅ | Version `0.1.0-alpha.0`, edition 2024, MSRV 1.85, license and repository URL set |
| npm package metadata | ⛔ Implementation | Plan only; package name not reserved |
| Release automation | ⛔ Implementation | Intentionally not configured ([RELEASE_PROCESS.md](development/RELEASE_PROCESS.md)) |

### GitHub presentation

| Item | Status | Notes |
|---|---|---|
| Repository description and topics | ✅ | Applied 2026-11-13: description plus 14 topics; Wiki off |
| Visibility | ✅ | **Public** since 2026-11-13: https://github.com/Asmodeus14/ChronoStack |
| Social preview image | ⛔ Owner | Concept only |
| Branch protection | ⚠️ Owner | Not enabled (checked 2026-11-13); recommended settings are in REPOSITORY_SETUP.md |
| Git repository | ✅ | Initialized; `main` pushed |

## 2. Positioning review

| Candidate | Assessment |
|---|---|
| **"Stripe Test Clocks for your Docker Compose stack."** | **Recommended.** It names a known reference experience and the exact scope (Compose), and says *what* you can do (move time) without overclaiming. Weakness: it means little to people who don't know Stripe, so the second README line explains it in plain words. |
| "Deterministic lifecycle and failure testing for distributed applications." | **Rejected as the headline.** ChronoStack cannot make real thread scheduling, I/O or schedulers deterministic (ADR-012). "Distributed applications" also overstates the scope (no Kubernetes, single host), and the line invites comparison with deterministic-simulation products. |

Changes applied:

- the tagline and the second line in the README now say "run the jobs that depend on it, and check the results";
- the README uses "reproducible", never "deterministic";
- the comparison table in the README admits that the faking mechanism is not new.

## 3. Audit findings, and what was done

| Finding | Action |
|---|---|
| ARCHITECTURE put `exec` triggers in clockd (needing the Docker socket), while SECURITY preferred the host | ADR-016 added; ARCHITECTURE, IMPLEMENTATION_PLAN and SECURITY aligned |
| pg_cron described as "unresolved", with tests expecting 14 runs from `--step 1d` | Source verified: jumps over 3 h skip runs. Sub-stepping ADR amendment added; tests corrected; risk R5 re-rated |
| Claims cited as verified without having been opened (Chaos Mesh ptrace, libfaketime GPL-2.0 and `FAKETIME_SHARED`, BullMQ scoring, tmpfs volumes, CPython 3.11 locks) | Corrected or downgraded to assumptions |
| ADR-008 referenced a nonexistent `chronostack test` command | Changed to `chronostack run` / `CI=true` |
| API endpoint names differed between ARCHITECTURE and IMPLEMENTATION_PLAN | Unified (`/v1/clock/*`) |
| Node SDK status was inconsistent (v0.1 late vs v0.2) | Unified: v0.1 stretch goal, moving to v0.2 if it slips |
| Persona named "Dev" was ambiguous | Renamed to "Jordan" |
| Windows builds were assumed to use cargo-zigbuild | Corrected: native Windows runner |

## 4. Verdicts

- **Ready for external contributors?** **Mostly.** The license, git repository, CI and contributor docs are in place. The remaining blocker is the Code of Conduct reporting contact (owner decision).
- **Ready for a public alpha?** **No.** M3–M5 are not done, there are no release binaries, and macOS is untested. The alpha criteria are in [RELEASE_PROCESS.md](development/RELEASE_PROCESS.md).
- **Ready to be public as a pre-alpha project?** Yes; it has been public since 2026-11-13.
