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

### Changed

- Architecture: the step / trigger / barrier engine moves to the host CLI, and no container receives the Docker socket (ADR-016).
- Stepped advances are split into sub-steps of at most 1 hour when pg_cron is present. Verified pg_cron source shows that clock jumps over 3 hours skip scheduled runs.
