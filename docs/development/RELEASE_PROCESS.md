# Release & Maintenance Strategy

**Status (M5, 2026-11-14):** the release is automated in [`.github/workflows/release.yml`](../../.github/workflows/release.yml), in **draft mode**. A tag never publishes a finished release by itself: the workflow creates a *draft* GitHub Release that a maintainer reviews and publishes by hand. Run the workflow manually (`workflow_dispatch`) for a **dry run**, which builds and smoke-tests everything and publishes nothing.

### How to cut a release

1. Set `version` in the root `Cargo.toml` (and `sdk/node/package.json`). Rename `## [Unreleased]` in `CHANGELOG.md` to `## [X.Y.Z[-pre]] - YYYY-MM-DD`.
2. Run the workflow by hand (dry run) and wait for it to pass.
3. Push an annotated tag `vX.Y.Z[-pre]`. The workflow checks the tag matches `Cargo.toml`, then:
   - builds the controller image natively on x86_64 and arm64, pushes `ghcr.io/asmodeus14/chronostack-clockd:X.Y.Z-amd64/-arm64` and the multi-arch `:X.Y.Z`;
   - builds the CLI for linux-x64/arm64 (static musl), darwin-arm64/x64 and win32-x64, with the image **pinned by digest** as the default;
   - extracts the 4 preload libraries, writes `SHA256SUMS` and an SPDX SBOM;
   - installs with `install.sh` on fresh Linux x86_64 and arm64 runners and runs the demo scenario (must finish in under 5 minutes);
   - creates a **draft** release (marked pre-release for `-alpha/-beta/-rc`) with all files and notes from `scripts/release_notes.py`.
4. **First release only:** GHCR creates the `chronostack-clockd` package as **private**. Make it public in GitHub → Packages → chronostack-clockd → Package settings → Change visibility, otherwise users can't pull the image.
5. Review the draft and publish it.

## Versioning

- [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).
- One project version is shared by:
  - the CLI;
  - `clockd` (container image tag);
  - the preload library;
  - the clock-page layout compatibility (see below).
- **0.x rules:**
  - a minor bump (0.1 → 0.2) may break things, and the CHANGELOG lists each break;
  - a patch bump is fixes only.
- **Pre-releases:** `0.1.0-alpha.N`, `0.1.0-beta.N`, `0.1.0-rc.N`.
- **Compatibility contracts**, versioned separately inside the project:
  - clock-page `layout_version`: the preload library rejects layouts it doesn't know;
  - controller API path prefix (`/v1`);
  - `chronostack.yml` `version:` field.

## Release stages

| Stage | Entry criteria |
|---|---|
| **Proof of concept (M0)** | No release. Results are published in `docs/M0_RESULTS.md`. |
| **Alpha (`0.1.0-alpha.N`)** | M1 + M2 are complete:<br>• `up` / `advance` / `doctor` work on 3 external Compose projects;<br>• the no-hang suite is green on Linux;<br>• macOS checked by hand;<br>• known limitations documented;<br>• license chosen;<br>• SECURITY contact live. |
| **Beta (`0.1.0-beta.N`)** | M3 + M4 are complete:<br>• the demo scenario passes 10/10 runs on Linux and 3/3 on macOS;<br>• the config schema is frozen for 0.1;<br>• the README compatibility table is generated from CI. |
| **Stable `0.1.0`** | No open P0/P1 bugs, release smoke test on fresh Linux and macOS machines, docs complete. |

## Git tags and GitHub Releases

- Tags use the form `vX.Y.Z[-pre]` and are annotated, and signed where possible.
- A GitHub Release per tag carries:
  - CLI binaries (linux-x64, linux-arm64, darwin-arm64, darwin-x64, win32-x64);
  - shim `.so` files (4 targets);
  - `SHA256SUMS`;
  - an SBOM;
  - release notes.
- The `clockd` multi-arch image goes to GHCR, tagged with the version and referenced **by digest** from the CLI build.
- Windows CLI binaries are built on a native Windows runner, because cargo-zigbuild supports only Linux and macOS targets ([verified](../SOURCE_VERIFICATION.md#7-tools-and-competitors)).

## Release notes and changelog

- Maintain `CHANGELOG.md` by hand under `[Unreleased]` in every PR with user-visible changes, following Keep a Changelog.
- At release, rename `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD`. The GitHub Release notes are that section, plus a manual-verification checklist (macOS / Windows Docker Desktop).

## Release tooling evaluation

| Tool | Fit | Recommendation |
|---|---|---|
| [release-plz](https://github.com/release-plz/release-plz) | Automates release PRs, changelog (git-cliff) and crates.io publishing. Good for multi-crate workspaces publishing to crates.io. | **Later.** ChronoStack's main artifacts are binaries and a container image, not crates. Revisit if crates are published. |
| [cargo-release](https://github.com/crate-ci/cargo-release) | Local CLI for version bump, tag and push; dry-run by default. | **Use for alpha/beta.** It is explicit, human-driven and safe. |
| GitHub Actions release workflow (native runners + Docker) | Builds and attaches artifacts on tag push | **Adopted in M5, in draft-release mode.** Native runners for every target instead of cargo-zigbuild (Linux uses the static musl target; macOS builds both architectures on one runner). |

## CI requirements (once code exists)

- Required on every PR:
  - fmt, clippy (`-D warnings`), unit tests;
  - shim tests on 4 targets;
  - Docker integration tests on `ubuntu-latest` and an arm64 Ubuntu runner;
  - documentation link check.
- Nightly: full runtime compatibility matrix, scheduler tests, the long no-hang suite, performance benchmarks.
- Release: smoke test from published artifacts on clean VMs.

## Supported platforms policy

| Item | Policy (proposed) |
|---|---|
| Rust | MSRV declared in `Cargo.toml` `rust-version`. Raising the MSRV counts as a minor change and is noted in the CHANGELOG. Target: N-2 stable at the time of 0.1. |
| Docker | Docker Engine and Docker Desktop releases from the last 12 months; Compose v2 with `config --format json`. Exact minimums are set during M1 testing. |
| Host OS | Linux x86_64/arm64; macOS 14+ arm64 (x64 best-effort); Windows 11 x64 with WSL2 Docker Desktop. |
| Container libc | glibc ≥ 2.17 (to be confirmed) and musl (Alpine 3.19+). |

## Backward compatibility and deprecation

- Before 1.0: breaking changes are allowed in minor releases, but they must be listed in the CHANGELOG under **Changed** and **Removed**, with migration notes.
- Deprecations are announced at least **one minor release** before removal and produce CLI warnings.
- The CLI refuses to talk to a `clockd` or clock-page layout from a different major/minor version and suggests running `chronostack down && chronostack up`.

## Bug triage

Labels:

- `bug`, `hang`, `compat:<runtime>`, `scheduler`, `docs`, `security`;
- `good first issue`, `help wanted`, `needs-repro`;
- `architecture`.

Priority levels:

| Priority | Meaning | Response target |
|---|---|---|
| P0 | Hangs, crashes or data loss in target services, or a security issue | Fix in the next patch release |
| P1 | Wrong time in a supported component | Next minor release |
| P2 | Everything else | Best effort |

Issues waiting on the reporter are closed after 30 days without a response.

## Security patches

Follow [SECURITY.md](../../SECURITY.md):

- coordinate privately;
- release the fix as a patch on the latest minor;
- publish a GitHub Security Advisory;
- credit the reporter.

## Dependency updates

- Dependabot handles `github-actions`, `cargo` and `docker` (the Node SDK has no dependencies).
- Updates are grouped weekly.
- Security updates are reviewed within 7 days.
