#!/bin/sh
# Fresh stack: wipe volumes, start under ChronoStack, seed Twenty's demo "Apple" workspace.
set -e
cd "$(dirname "$0")/.."
chronostack down -v >/dev/null 2>&1 || true
chronostack up --wait >/dev/null
docker exec twenty-chronostack-server-1 yarn command:prod workspace:seed:dev --light >/dev/null 2>&1
echo "seeded: $(docker exec twenty-chronostack-db-1 psql -U postgres -d default -Atc 'select count(*) from workspace_1wgvd1injqtife6y4rvfbu3h5.person') people"
