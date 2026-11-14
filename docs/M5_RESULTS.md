# M5 — Release: results

**Date:** 2026-11-14
**Exit gate** ([IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md#m5--release-weeks-1112)): *tag v0.1.0.*

**Status: first pre-release `v0.1.0-alpha.2`.** (`v0.1.0-alpha.1` was tagged and built, run 43563492172, but not published: it predated `chronostack update`, the GitHub Action and the MCP bundles.)

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

## Published release, checked from the outside (2026-11-14)

[`v0.1.0-alpha.2`](https://github.com/Asmodeus14/ChronoStack/releases/tag/v0.1.0-alpha.2), release run 43571507866: every job green, 19 files.

| Check | Result |
|---|---|
| Linux (clean `ubuntu:24.04`): README `curl … install.sh \| sh` command | ✅ checksum ok; PATH line added to `~/.bashrc`; a new shell finds `chronostack`; `update --check` reports up to date |
| Linux (clean `alpine:3.20`, busybox wget), no version given | ✅ picks the newest release (pre-releases included) |
| Windows 11: README `irm … install.ps1 \| iex` command | ✅ works in the same terminal right away, and in a new one; `update --check` up to date |
| Windows 11 + Docker Desktop: released CLI, demo from zero | ✅ pulled `ghcr.io/asmodeus14/chronostack-clockd:0.1.0-alpha.2@sha256:e4305a12…` without logging in; all 5 services see virtual time; scenario 12/12 passed; 59 s total; `doctor` shows matching versions |
| GitHub Action (`action-test`, run 43572375602) | ✅ installs and runs the demo on ubuntu-latest and ubuntu-24.04-arm |
| MCP bundles listed in `server.json` | ✅ all 5 URLs download and match their `fileSha256` |

## After the dry run

- **`chronostack update`** (added before publishing): finds the newest release (pre-releases for pre-release builds), downloads this platform's archive, verifies it against `SHA256SUMS`, and swaps the running binary using `self-replace`, which works on Windows. Tested locally on Windows: a debug build replaced itself with the release build and no temporary file was left; a wrong checksum was refused and the binary stayed byte-identical. The CLI now includes rustls for HTTPS; MSRV 1.85 still builds.
- **No admin rights anywhere:** the scripts install into user directories, and `update` writes only where the binary is.
- **Not code-signed.** The install scripts use curl or `Invoke-WebRequest`, which don't mark files as downloaded from the internet, so Gatekeeper and SmartScreen don't prompt. Browser downloads of the archives can. Signing needs an Apple Developer ID with notarization, and a Windows code-signing certificate (e.g. Azure Trusted Signing). That's an owner decision, since both cost money.

## Not done

- **macOS smoke test.** GitHub's macOS runners can't run Docker. The macOS binaries are built and run `--version` in CI, but have not been used against a real Docker. Needed for `v0.1.0` stable (issue #5).
- ~~20-second terminal recording~~ **Done:** `docs/assets/demo.gif`, rendered with vhs by the `demo-gif` workflow.
- **README compatibility table generated from CI.** Still maintained by hand; it lists only what the tests cover.
