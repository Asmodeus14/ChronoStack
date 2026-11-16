#!/bin/sh
# Fetches Worklenz v3.3.1's database init files (the same ones its compose file
# mounts into Postgres) into ../database (gitignored).
set -e
cd "$(dirname "$0")/.."
rm -rf database && mkdir database
curl -fsSL https://codeload.github.com/Worklenz/worklenz/tar.gz/refs/tags/v3.3.1 \
  | tar xz -C database --strip-components=3 worklenz-3.3.1/worklenz-backend/database/sql worklenz-3.3.1/worklenz-backend/database/00_init.sh worklenz-3.3.1/worklenz-backend/database/pg-migrations-support
sed -i 's/\r$//' database/00_init.sh
ls database
