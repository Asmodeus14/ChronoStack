# Contributing to ChronoStack

Thanks for your interest. ChronoStack is in its **design phase**. There is no code yet, and right now the most valuable contributions are technical review and real-world input.

> [!NOTE]
> **License.** ChronoStack is dual-licensed under `MIT OR Apache-2.0` ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)). Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion is dual-licensed the same way, without additional terms or conditions.

## Ways to contribute today

1. **Challenge the technical claims.** [TECHNICAL_FEASIBILITY.md](docs/TECHNICAL_FEASIBILITY.md) and [SOURCE_VERIFICATION.md](docs/SOURCE_VERIFICATION.md) label every claim as Verified, Partially verified, Assumption or Unresolved. If you know the answer to an open question, open an issue and cite the source code or documentation.
2. **Review the architecture.** Comment on the [Architecture Decision Records](docs/ARCHITECTURE_DECISIONS.md).
3. **Share your use case.** Tell us how you test trials, expiry, retention or retries today. Use the feature request template or Discussions.
4. **Improve the docs.** Fix unclear wording, broken links or inconsistencies.

## Repository structure

```
README.md            Project overview
docs/                Research, architecture and planning (start at docs/README.md)
docs/development/    Release process, licensing decision, repository setup
.github/             Issue/PR templates, CI workflows, Dependabot config
```

Planned once implementation is authorized (see [ARCHITECTURE.md §9](docs/ARCHITECTURE.md#9-repository-layout-planned-not-created)):

```
crates/{cli,clockd,core}   Rust workspace
shim/                      C preload library (libchronoclock)
sdk/node/                  Node SDK (stretch goal)
examples/saas-trial-demo/  Reference application
tests/                     Docker-based integration tests
```

## Development environment (planned)

These requirements apply once code exists:

| Tool | Purpose |
|---|---|
| Rust (stable; minimum supported version to be set at M1) | CLI, controller |
| Zig | Cross-compiling the C shim for x86_64/aarch64 × glibc/musl |
| Docker Engine or Docker Desktop with Compose v2 | Integration tests |
| Node.js LTS | Node SDK and demo (later milestones) |

Build and test commands will be documented here when the Cargo workspace is created. They are not listed now, so none of them are invented.

## Coding conventions (planned)

- **Rust:** `cargo fmt` and `cargo clippy -- -D warnings` must pass. Use `unsafe` only in clearly marked, documented modules.
- **C shim:**
  - C99, no dependencies beyond libc and `dlfcn`;
  - no allocation, locks or logging on the hot path;
  - every interposed function has a test.
- Every compatibility claim needs an automated test ([TESTING_STRATEGY.md](docs/TESTING_STRATEGY.md)).

## Commit conventions

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `ci:`, `chore:`. Mark breaking changes with `!` or a `BREAKING CHANGE:` footer. This keeps release-note generation possible later ([RELEASE_PROCESS.md](docs/development/RELEASE_PROCESS.md)).

## Pull request process

1. For anything larger than a small fix, open an issue or discussion first.
2. Keep each pull request focused on one concern.
3. Fill in the pull request template, including the testing and documentation checklists.
4. Update `CHANGELOG.md` under `[Unreleased]` for user-visible changes.
5. CI must pass. A maintainer reviews; at least one approval is required before merging.

## Reporting bugs

Use the **Bug report** issue template. Include:

- your OS and Docker version;
- the relevant Compose snippet;
- the output of `chronostack doctor`, once that command exists.

**Do not report security problems in public issues.** See [SECURITY.md](SECURITY.md).

## Documentation contributions

- Keep the evidence labels (Verified / Assumption / Unresolved) accurate. Never mark a claim verified without opening the source; add it to [SOURCE_VERIFICATION.md](docs/SOURCE_VERIFICATION.md).
- Never describe a planned feature as implemented.
- Run the link check locally if you can (see `.github/workflows/documentation.yml`).

## Proposing architectural changes

1. Open a discussion in the **Ideas** category (or an issue labelled `architecture`). Describe the problem, the options and the evidence.
2. If there's agreement, submit a pull request that adds or amends an ADR in [ARCHITECTURE_DECISIONS.md](docs/ARCHITECTURE_DECISIONS.md) (Context → Decision → Consequences).
3. Update every document affected, so the docs never contradict each other.

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
