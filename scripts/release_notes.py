# SPDX-License-Identifier: MIT OR Apache-2.0
"""Print GitHub Release notes for a version: its CHANGELOG section plus install
instructions and a manual-verification checklist.

Usage: python scripts/release_notes.py 0.1.0-alpha.1
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
    pre = "-" in version
    print(f"""ChronoStack {version}{" (pre-release)" if pre else ""}: Stripe Test Clocks for your Docker Compose stack.

## Install

Linux / macOS:

```sh
curl -fsSL https://github.com/Asmodeus14/ChronoStack/releases/download/v{version}/install.sh | CHRONOSTACK_VERSION={version} sh
```

Windows (PowerShell):

```powershell
$env:CHRONOSTACK_VERSION = "{version}"; irm https://github.com/Asmodeus14/ChronoStack/releases/download/v{version}/install.ps1 | iex
```

The CLI pulls the matching controller image (`ghcr.io/asmodeus14/chronostack-clockd:{version}`, pinned by digest) on first `chronostack up`. Every file below is listed in `SHA256SUMS`; the SBOM is SPDX JSON.

## Verified for this release

- Automated: unit tests on Linux, Windows and macOS; end-to-end tests (M1–M4) on Linux x86_64 and arm64; a smoke test that installs from these files on fresh Linux x86_64 and arm64 runners and runs the demo scenario.
- Manual checklist (not automated):
  - [ ] macOS (Docker Desktop or OrbStack): not yet tested, see issue #5
  - [x] Windows 11 + Docker Desktop (WSL2): tested by the maintainers during development

## Changes

{section(changelog, version)}
""")


if __name__ == "__main__":
    main()
