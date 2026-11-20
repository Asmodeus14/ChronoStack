# SPDX-License-Identifier: MIT OR Apache-2.0
"""Print the Scoop manifest (bucket/chronostack.json in Asmodeus14/scoop-bucket)
for a published release, using the checksum from its SHA256SUMS file.

Usage: python scripts/scoop_manifest.py 0.1.0-alpha.7 path/to/SHA256SUMS
"""
import json
import sys

REPO = "Asmodeus14/ChronoStack"


def checksums(text):
    sums = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 2:
            sums[parts[1].lstrip("*")] = parts[0].lower()
    return sums


def manifest(version, sums):
    name = f"chronostack-{version}-win32-x64.zip"
    if name not in sums:
        sys.exit(f"{name} is not listed in SHA256SUMS")
    base = f"https://github.com/{REPO}/releases/download/v{version}"
    return {
        "version": version,
        "description": "Stripe Test Clocks for your Docker Compose stack: move time forward "
                       "across Postgres, Redis and your services to test trials, renewals and cron jobs.",
        "homepage": f"https://github.com/{REPO}",
        "license": "MIT|Apache-2.0",
        "notes": [
            "ChronoStack needs Docker Desktop (Compose v2).",
            "Upgrade with `scoop update chronostack` rather than `chronostack update`.",
        ],
        "architecture": {
            "64bit": {"url": f"{base}/{name}", "hash": sums[name]},
        },
        "bin": "chronostack.exe",
        "checkver": {
            "url": f"https://api.github.com/repos/{REPO}/releases",
            "jsonpath": "$[0].tag_name",
            "regex": "v([\\w.-]+)",
        },
        "autoupdate": {
            "architecture": {
                "64bit": {
                    "url": f"https://github.com/{REPO}/releases/download/v$version/chronostack-$version-win32-x64.zip",
                    "hash": {"url": f"https://github.com/{REPO}/releases/download/v$version/SHA256SUMS"},
                },
            },
        },
    }


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    version = sys.argv[1].lstrip("v")
    with open(sys.argv[2], encoding="utf-8") as f:
        text = json.dumps(manifest(version, checksums(f.read())), indent=4) + "\n"
    # bytes, so Windows doesn't turn the newlines into CRLF
    sys.stdout.buffer.write(text.encode("utf-8"))
