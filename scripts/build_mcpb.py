# SPDX-License-Identifier: MIT OR Apache-2.0
"""Build MCP bundles (.mcpb) from the release archives, and the MCP Registry
`server.json` that lists them.

Usage: python scripts/build_mcpb.py <release_dir> <version>

For each CLI archive in <release_dir> (chronostack-<v>-<target>.tar.gz / .zip)
this writes chronostack-mcp-<v>-<target>.mcpb: a zip with manifest.json at the
root and the binary under server/ (MCPB spec, manifest_version 0.3,
https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md).
The file name contains "mcp", as the MCP Registry requires for mcpb packages.

Then it writes <release_dir>/server.json for `mcp-publisher publish`
(schema 2025-12-11), with one mcpb package per target and its SHA-256.
The spec has no CPU-architecture field, so targets are told apart by file name.
"""
import hashlib
import io
import json
import os
import sys
import tarfile
import zipfile

REPO = "Asmodeus14/ChronoStack"
TARGETS = {  # archive suffix -> (MCPB platform, binary name in the archive)
    "linux-x64": ("linux", "chronostack"),
    "linux-arm64": ("linux", "chronostack"),
    "darwin-arm64": ("darwin", "chronostack"),
    "darwin-x64": ("darwin", "chronostack"),
    "win32-x64": ("win32", "chronostack.exe"),
}
DESCRIPTION = "Move virtual time across a Docker Compose stack and test what happens (trials, jobs, outages)."


def manifest(version, platform):
    return {
        "manifest_version": "0.3",
        "name": "chronostack",
        "display_name": "ChronoStack",
        "version": version,
        "description": DESCRIPTION,
        "author": {"name": "ChronoStack contributors", "url": f"https://github.com/{REPO}"},
        "repository": {"type": "git", "url": f"https://github.com/{REPO}"},
        "homepage": f"https://github.com/{REPO}",
        "documentation": f"https://github.com/{REPO}/blob/main/docs/GUIDE.md",
        "license": "MIT OR Apache-2.0",
        "server": {
            "type": "binary",
            "entry_point": "server/chronostack",
            "mcp_config": {
                # MCPB clients append .exe on Windows.
                "command": "server/chronostack",
                "args": ["mcp", "--project-dir", "${user_config.project_dir}"],
                "env": {},
            },
        },
        "user_config": {
            "project_dir": {
                "type": "directory",
                "title": "Project directory",
                "description": "The Docker Compose project where you ran `chronostack up`.",
                "required": True,
            }
        },
        "compatibility": {"platforms": [platform]},
    }


def read_binary(path, name):
    if path.endswith(".zip"):
        with zipfile.ZipFile(path) as z:
            return z.read(name)
    with tarfile.open(path) as t:
        return t.extractfile(name).read()


def main():
    rel, version = sys.argv[1], sys.argv[2]
    packages = []
    for target, (platform, binname) in TARGETS.items():
        ext = "zip" if target.startswith("win32") else "tar.gz"
        archive = os.path.join(rel, f"chronostack-{version}-{target}.{ext}")
        if not os.path.exists(archive):
            sys.exit(f"missing {archive}")
        out_name = f"chronostack-mcp-{version}-{target}.mcpb"
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
            z.writestr("manifest.json", json.dumps(manifest(version, platform), indent=2) + "\n")
            info = zipfile.ZipInfo(f"server/{binname}")
            info.external_attr = 0o100755 << 16  # executable on Unix
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, read_binary(archive, binname))
            for extra in ("LICENSE-MIT", "LICENSE-APACHE"):
                try:
                    z.writestr(extra, read_binary(archive, extra))
                except (KeyError, AttributeError):
                    pass
        data = buf.getvalue()
        with open(os.path.join(rel, out_name), "wb") as f:
            f.write(data)
        packages.append({
            "registryType": "mcpb",
            "identifier": f"https://github.com/{REPO}/releases/download/v{version}/{out_name}",
            "version": version,
            "fileSha256": hashlib.sha256(data).hexdigest(),
            "transport": {"type": "stdio"},
        })
        print(f"{out_name}  {len(data) // 1024} KiB  ({platform})")
    server = {
        "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
        "name": "io.github.asmodeus14/chronostack",
        "title": "ChronoStack",
        "description": DESCRIPTION[:100],
        "version": version,
        "websiteUrl": f"https://github.com/{REPO}",
        "repository": {"url": f"https://github.com/{REPO}", "source": "github"},
        "packages": packages,
    }
    with open(os.path.join(rel, "server.json"), "w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(server, indent=2) + "\n")
    print("server.json written")


if __name__ == "__main__":
    main()
