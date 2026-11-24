# SPDX-License-Identifier: MIT OR Apache-2.0
"""Print GitHub Release notes for a version: its CHANGELOG section plus install
instructions and a manual-verification checklist.

Usage: python scripts/release_notes.py 0.1.0-alpha.3
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def section(changelog, version):
    """The `## [version]` section, or `## [Unreleased]` if there is none yet."""
    for name in (version, "Unreleased"):
        m = re.search(rf"^## \[{re.escape(name)}\][^\n]*\n(.*?)(?=^## \[|\Z)", changelog, re.S | re.M)
        if m:
            return m.group(1).strip()
    return "(no CHANGELOG entry)"


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    version = sys.argv[1]
    changelog = open(os.path.join(ROOT, "CHANGELOG.md"), encoding="utf-8").read()
    # The stage from the version (0.1.0-alpha.9 -> alpha); nothing for a stable release.
    stage = version.split("-", 1)[1].split(".")[0] if "-" in version else ""
    print(f"""ChronoStack {version}{f" ({stage})" if stage else ""}: Stripe Test Clocks for your Docker Compose stack.

## Install

Linux / macOS:

```sh
curl -fsSL https://github.com/Asmodeus14/ChronoStack/releases/download/v{version}/install.sh | CHRONOSTACK_VERSION={version} sh
```

Windows (PowerShell):

```powershell
$env:CHRONOSTACK_VERSION = "{version}"; irm https://github.com/Asmodeus14/ChronoStack/releases/download/v{version}/install.ps1 | iex
```

Homebrew (macOS, Linux) or Scoop (Windows); both are updated a few minutes after this release is published:

```sh
brew install asmodeus14/tap/chronostack
```

```powershell
scoop bucket add chronostack https://github.com/Asmodeus14/scoop-bucket
scoop install chronostack/chronostack
```

crates.io: `cargo install chronostack@{version}`.

Node SDK and Playwright helper on npm: `npm install --save-dev chronostack@{version}`.

The CLI pulls the matching controller image (`ghcr.io/asmodeus14/chronostack-clockd:{version}`, pinned by digest) on first `chronostack up`. Every file below is listed in `SHA256SUMS`; the SBOM is SPDX JSON.

## Verified for this release

- Automated: unit tests on Linux, Windows and macOS; end-to-end tests (M1–M4, `init`, shared mounts, `expose` and simulated users) on Linux x86_64 and arm64; a nightly check that simulated users on the virtual clock give the same timeline run after run; nightly queue-barrier tests against real BullMQ, Sidekiq and Celery workers; M1 and M2 with the native macOS CLI on an Intel macOS runner (Docker in a Linux VM); a smoke test that installs from these files on fresh Linux x86_64 and arm64 runners and runs the demo scenario; an install test on Apple-silicon and Intel macOS runners (installer, native binary, `init` and `config`; no Docker engine).
- Manual checklist (not automated):
  - [ ] macOS on Apple silicon with a running Docker engine (Docker Desktop or OrbStack): not yet tested, see issue #5
  - [x] Windows 11 + Docker Desktop (WSL2): tested by the maintainers during development

## Changes

{section(changelog, version)}
""")


if __name__ == "__main__":
    main()
