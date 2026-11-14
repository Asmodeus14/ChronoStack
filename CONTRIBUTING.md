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

Code layout (see [ARCHITECTURE.md §9](docs/ARCHITECTURE.md#9-repository-layout)):

```
crates/core                Shared types: clock page layout, API, Compose model, durations
crates/clockd              Controller (single writer of the clock page, HTTP API)
crates/cli                 `chronostack` CLI
shim/                      C preload library (libchronoclock)
docker/clockd.Dockerfile   Controller image (clockd + preload libraries)
tests/e2e/                 End-to-end tests against real Docker
examples/saas-trial-demo/  Reference demo (Node, Postgres, Redis)
spike/                     M0 feasibility spike (frozen, reference only)
sdk/node/                  Node register for host-native processes (M4)
```

## Development environment

| Tool | Purpose |
|---|---|
| Rust (stable; minimum supported version 1.85) | CLI, controller |
| Zig | Not needed locally: the controller image builds the glibc shim with zig inside Docker |
| Docker Engine or Docker Desktop with Compose v2 | Integration tests |
| Python 3.8+ | End-to-end test scripts |

### Build and test

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # clockd's tests only run on Linux
docker build -f docker/clockd.Dockerfile -t chronostack/clockd:dev .   # also runs clockd tests
python tests/e2e/run_m1.py        # end-to-end: controller, CLI, doctor (real Docker, real services)
python tests/e2e/run_m2.py        # engine, demo lifecycle, trigger timeouts, pg_cron
python tests/e2e/run_schedulers.py  # scheduler matrix (nightly in CI)
python tests/e2e/run_nohang.py    # no-hang suite across runtimes (nightly in CI)
```

The M0 spike harness (`spike/run_m0.py`) is kept for reference. It is not part of CI.

## Coding conventions

- **Rust:** `cargo fmt` and `cargo clippy -- -D warnings` must pass. The workspace denies `unsafe` code.
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
- the output of `chronostack doctor`.

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
