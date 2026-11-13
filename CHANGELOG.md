# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project will adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once the first release is tagged.

No versions have been released. ChronoStack contains no runnable code yet.

## [Unreleased]

### Added

- Research and planning documentation in `docs/`:
  - product vision
  - competitive analysis
  - technical feasibility
  - architecture
  - architecture decision records
  - MVP scope
  - feature roadmap
  - implementation plan
  - testing strategy
  - security and reliability
  - risks and limitations
  - go-to-market
- `docs/SOURCE_VERIFICATION.md`: second-pass check of the technical claims against primary sources.
- `docs/OPEN_SOURCE_READINESS.md`, `docs/README.md` (documentation index), and `docs/development/` covering release process, licensing decision and repository setup.
- Community files: README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, issue and pull request templates, a discussion form.
- CI workflows:
  - documentation link check;
  - a Rust CI skeleton that only runs once a Cargo workspace exists.
- Dependabot for GitHub Actions.
- **M0 feasibility spike** (`spike/`, throwaway):
  - C preload shim (glibc and musl) with seqlock clock page v2 and absolute-deadline translation;
  - `chronoctl` writer;
  - Docker-based test matrix and harness.
  - Results in `docs/M0_RESULTS.md`: 137/137 pass on Docker Desktop/WSL2 x86_64.
- `LICENSE-MIT` and `LICENSE-APACHE`: the project is dual-licensed `MIT OR Apache-2.0`.
- ADR-017: virtual time is anchored to `CLOCK_MONOTONIC`, because M0 measured host `CLOCK_REALTIME` stepping backwards.

- **M1 — controller and core CLI** (pre-alpha, from source):
  - `crates/core`: clock page layout (asserted against the C header), API types, Compose model, override generator, durations.
  - `crates/clockd`: single writer of the clock page (ADR-017 protocol) and a token-authenticated localhost HTTP API.
  - `crates/cli` (`chronostack`): `up`, `down`, `now`, `advance`, `freeze`, `unfreeze`, `probe`, `doctor`.
  - `shim/`: the preload library promoted from the spike. The glibc build targets glibc 2.17 via zig.
  - `docker/clockd.Dockerfile`, and the end-to-end test `tests/e2e/run_m1.py`.
- CI:
  - Rust checks on Linux (x86_64 and arm64), Windows and macOS;
  - an MSRV check;
  - the e2e test on Linux x86_64 and arm64.
  - Dependabot now covers cargo and docker.

- `doctor` checks every process in each container and flags Go binaries (they bypass the shim), e.g. Gitea's server behind its C supervisor. Covered by a new e2e case (`caddy:2-alpine`).
- `docs/M1_RESULTS.md`: M1 exit gate passed on 3 unmodified docker/awesome-compose projects.

### Changed

- Architecture: the step / trigger / barrier engine moves to the host CLI, and no container receives the Docker socket (ADR-016).
- Stepped advances are split into sub-steps of at most 1 hour when pg_cron is present. Verified pg_cron source shows that clock jumps over 3 hours skip scheduled runs.
