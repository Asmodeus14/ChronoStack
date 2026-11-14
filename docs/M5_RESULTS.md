# M5 — Release: results

**Date:** 2026-11-14
**Exit gate** ([IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md#m5--release-weeks-1112)): *tag v0.1.0.*

**Status: release pipeline built and dry-run green; not tagged yet.**

The first tag will be **`v0.1.0-alpha.1`**, not `v0.1.0`. [RELEASE_PROCESS](development/RELEASE_PROCESS.md) requires smoke tests on fresh Linux **and macOS** machines for a stable 0.1.0, and macOS is untested ([#5](https://github.com/Asmodeus14/ChronoStack/issues/5)). Tagging is a public, outward-facing step, so it waits for the owner's go-ahead.

## What was built

- **[`.github/workflows/release.yml`](../.github/workflows/release.yml)**, in draft mode:
  - **Controller image:** built natively on x86_64 and arm64 runners, which also runs its unit tests. On a tag it is pushed to `ghcr.io/asmodeus14/chronostack-clockd:<version>-amd64/-arm64` and joined into a multi-arch `:<version>`.
  - **CLI for 5 targets:**
    - linux-x64 and linux-arm64: static musl;
    - darwin-arm64 and darwin-x64: one macOS runner;
    - win32-x64: native Windows.

    The default controller image is compiled in **pinned by digest** (`…:<version>@sha256:…`).
  - **Release files:** the 4 preload libraries (extracted from the images), `SHA256SUMS`, an SPDX SBOM, both install scripts, and release notes generated from `CHANGELOG.md` by `scripts/release_notes.py`.
  - **Smoke test** on fresh Linux x86_64 and arm64 runners. It serves the release files locally, installs with `install.sh` (checksum-verified), copies the demo into an empty directory, and runs `up --wait` and `run scenarios/trial.yml` in under 5 minutes.
  - **Draft GitHub Release** on tags only, marked pre-release for `-alpha/-beta/-rc`. A maintainer reviews and publishes it.
  - A manual run (`workflow_dispatch`) is a **dry run**: everything is built and tested, and nothing is pushed or released.
- **Install scripts:**
  - `install/install.sh` (Linux, macOS; curl or wget) and `install/install.ps1` (Windows).
  - Both pick the newest release (pre-releases included) or `CHRONOSTACK_VERSION`, verify `SHA256SUMS`, and install to a user directory.
  - No sudo and no shell-profile edits. The PowerShell script adds its folder to the user PATH; `CHRONOSTACK_NO_MODIFY_PATH=1` turns that off.
- **CLI:**
  - Release builds pull the controller image on first `up`.
  - `/v1/health` reports the controller version. `doctor` shows it and warns if it differs from the CLI.
- **README:** install instructions, and a demo transcript copied from a real run.
- **Version:** `0.1.0-alpha.1`.

## Results

| Check | Result |
|---|---|
| Release dry run ([run 43562887606](https://github.com/Asmodeus14/ChronoStack/actions/runs/43562887606)) | ✅ every job; publishing steps skipped as designed |
| CLI builds: linux-x64, linux-arm64, darwin-arm64, darwin-x64, win32-x64 | ✅ (`--version` runs on each native target) |
| Smoke test, fresh Ubuntu x86_64: install + `up --wait` + demo scenario | ✅ 26 s (scenario 10.0 s), 12/12 steps |
| Smoke test, fresh Ubuntu arm64 | ✅ 30 s (scenario 13.8 s), 12/12 steps |
| `install.sh` locally (Alpine, busybox wget): installs; a wrong checksum is refused and nothing is installed | ✅ |
| `install.ps1` locally (Windows 11): installs; a wrong checksum is refused | ✅ |
| `SHA256SUMS` covers 5 archives, 4 libraries, SBOM, 2 install scripts | ✅ |

## Not done

- **Tag `v0.1.0-alpha.1`.** Waiting for the owner. After the tag:
  1. make the GHCR package `chronostack-clockd` **public** (GitHub creates it private);
  2. review and publish the draft release.
- **macOS smoke test.** GitHub's macOS runners can't run Docker. The macOS binaries are built and run `--version` in CI, but have not been used against a real Docker. Needed for `v0.1.0` stable (issue #5).
- **20-second terminal recording.** Not made. The README has a transcript copied from a real run instead. A GIF needs a recorder (asciinema or vhs) on a machine with a display.
- **README compatibility table generated from CI.** Still maintained by hand; it lists only what the tests cover.
