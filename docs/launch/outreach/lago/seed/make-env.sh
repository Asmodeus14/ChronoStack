#!/bin/sh
# Writes ../.env with a fresh throwaway RSA key for Lago (LAGO_RSA_PRIVATE_KEY).
set -e
cd "$(dirname "$0")/.."
key=$(docker run --rm alpine/openssl genrsa 2048 2>/dev/null | base64 | tr -d '\n')
printf 'LAGO_RSA_PRIVATE_KEY=%s\n' "$key" > .env
echo "wrote .env"
