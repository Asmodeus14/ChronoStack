# Open-Source Readiness

**Date:** 2026-11-13
**Repository state:** documentation only; not yet a git repository; not published.

**Classification:**

- ✅ **Ready**
- ⚠️ **Needs attention**
- ⛔ **Blocked**, pending implementation or an owner decision

## 1. Checklist

### Project documentation

| Item | Status | Notes |
|---|---|---|
| README: value proposition in the first two lines | ✅ | Tagline, plus a design-phase status banner |
| README: accurate status (no implemented-feature claims) | ✅ | All commands labelled *proposed* |
| README: install / quick start | ⛔ Implementation | Deliberately absent; nothing is runnable |
| README: screenshots / GIF | ⛔ Implementation | None exist; none fabricated. A mermaid diagram is included. |
| Documentation index (`docs/README.md`) | ✅ | |
| Internal Markdown links and anchors | ✅ | Checked locally with a script (0 problems across 22 files) at the time of writing. The CI link-check workflow has **not** been run yet. |
| External URLs | ⚠️ | Key technical URLs were opened in the second pass ([SOURCE_VERIFICATION.md](SOURCE_VERIFICATION.md)); the §8 list there is still unopened |
| Consistency across the 12 planning docs | ✅ | Resolved: engine location (ADR-016), pg_cron sub-stepping, Node SDK as a stretch goal, demo runtime (Node), API endpoints, MCP tool list, CI-mode strictness, persona naming |
| Acronyms explained | ✅ | Glossary in [docs/README.md](README.md#glossary) |
| CHANGELOG (Keep a Changelog) | ✅ | Only `[Unreleased]` documentation changes |

### Community and governance

| Item | Status | Notes |
|---|---|---|
| CONTRIBUTING.md | ✅ | Design-phase contribution paths; build/test commands deferred honestly |
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
| `documentation.yml` (offline link check) | ⚠️ | Written but **not executed**. It will run on the first push. |
| `ci.yml` skeleton | ✅ | Runs Rust jobs only once `Cargo.toml` exists; no fake tests |
| Dependabot | ✅ | `github-actions` only; cargo/npm/docker commented out until their manifests exist |
| `.gitignore`, `.gitattributes`, `.editorconfig` | ✅ | |
| rustfmt / clippy configuration | ⛔ Implementation | Deferred to M1 (proposal in [REPOSITORY_SETUP.md](development/REPOSITORY_SETUP.md)) |
| Cargo workspace metadata | ⛔ Implementation and owner | Proposal written. Needs the license and repository URL. |
| npm package metadata | ⛔ Implementation | Plan only; package name not reserved |
| Release automation | ⛔ Implementation | Intentionally not configured ([RELEASE_PROCESS.md](development/RELEASE_PROCESS.md)) |

### GitHub presentation

| Item | Status | Notes |
|---|---|---|
| Repository description and topics | ✅ | Applied 2026-11-13: description plus 14 topics; Wiki off |
| Visibility | ✅ | **Public** since 2026-11-13: https://github.com/Asmodeus14/ChronoStack |
| Social preview image | ⛔ Owner | Concept only |
| Branch protection | ⚠️ Owner | Not enabled yet; recommended settings are in REPOSITORY_SETUP.md |
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

- **Ready for external contributors?** **Not yet.** There is no license (contributions can't be accepted), no CoC reporting contact, and no git repository. Design review through Discussions becomes possible as soon as those three owner actions are done.
- **Ready for a public alpha?** **No.** No code exists. The alpha criteria are in [RELEASE_PROCESS.md](development/RELEASE_PROCESS.md).
- **Ready to publish the repository as a design-phase project?** Yes, once the owner completes the ⛔ Owner items above.
