#!/bin/sh
# SPDX-License-Identifier: MIT OR Apache-2.0
# Install the ChronoStack CLI on Linux or macOS.
#
#   curl -fsSL https://github.com/Asmodeus14/ChronoStack/releases/latest/download/install.sh | sh
#
# Options (environment variables):
#   CHRONOSTACK_VERSION        version to install, e.g. 0.1.0-alpha.1 (default: the newest release,
#                              including pre-releases)
#   CHRONOSTACK_INSTALL_DIR    where to put the binary (default: $HOME/.local/bin)
#   CHRONOSTACK_DOWNLOAD_BASE  download from here instead of GitHub Releases (testing)
#
# The archive is verified against the release's SHA256SUMS before anything is installed.
# Nothing else on the machine is changed: no sudo, no shell profile edits.
set -eu

REPO="Asmodeus14/ChronoStack"
DEST="${CHRONOSTACK_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "chronostack-install: $*" >&2; }
die() { say "error: $*"; exit 1; }

fetch() { # url dest
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    die "curl or wget is required"
  fi
}

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) die "unsupported OS $(uname -s); on Windows use install.ps1" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x64 ;;
  aarch64 | arm64) arch=arm64 ;;
  *) die "unsupported CPU $(uname -m)" ;;
esac

version="${CHRONOSTACK_VERSION:-}"
if [ -z "$version" ]; then
  tmpv=$(mktemp)
  fetch "https://api.github.com/repos/$REPO/releases?per_page=1" "$tmpv" || die "cannot query GitHub releases"
  version=$(sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' "$tmpv" | head -1)
  rm -f "$tmpv"
  [ -n "$version" ] || die "no release found"
fi
version="${version#v}"

base="${CHRONOSTACK_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download/v$version}"
file="chronostack-$version-$os-$arch.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
say "downloading $file"
fetch "$base/$file" "$tmp/$file" || die "download failed: $base/$file"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "download failed: $base/SHA256SUMS"

expected=$(awk -v f="$file" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || die "$file is not listed in SHA256SUMS"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$file" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$tmp/$file" | awk '{ print $1 }')
fi
[ "$expected" = "$actual" ] || die "checksum mismatch for $file (expected $expected, got $actual)"
say "checksum ok"

tar -xzf "$tmp/$file" -C "$tmp" chronostack
mkdir -p "$DEST"
install -m 0755 "$tmp/chronostack" "$DEST/chronostack" 2>/dev/null || {
  cp "$tmp/chronostack" "$DEST/chronostack" && chmod 0755 "$DEST/chronostack"
}
say "installed $("$DEST/chronostack" --version) to $DEST/chronostack"

case ":$PATH:" in
  *":$DEST:"*) ;;
  *) say "note: $DEST is not on your PATH; add it, e.g. export PATH=\"$DEST:\$PATH\"" ;;
esac
say "next: cd into a Docker Compose project and run 'chronostack up --wait'"
