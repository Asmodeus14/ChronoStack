#!/bin/sh
# SPDX-License-Identifier: MIT OR Apache-2.0
# M0 clockd stand-in: seed the shared volume with the shim and a fresh clock page.
set -eu
mkdir -p /chronostack/lib
cp /opt/chronostack/lib/libchronoclock-*.so /chronostack/lib/
chmod 0644 /chronostack/lib/*.so
rm -f /chronostack/ready
chronoctl /chronostack/clock init >/dev/null
chmod 0644 /chronostack/clock
touch /chronostack/ready
exec sleep infinity
