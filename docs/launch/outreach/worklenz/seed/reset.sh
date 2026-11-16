#!/bin/sh
# Fresh stack: wipe volumes, start under ChronoStack, run Worklenz's migrations,
# then seed the user, project and two recurring tasks.
# The published 3.3.1 image lacks database/pg-migrations-support, which one
# migration requires, so it is copied in from the v3.3.1 source first
# (seed/fetch-schema.sh). 15 later migrations still fail on a fresh database;
# none of them touch recurring tasks.
set -e
cd "$(dirname "$0")/.."
[ -d database/pg-migrations-support ] || sh seed/fetch-schema.sh
chronostack down -v >/dev/null 2>&1 || true
chronostack up --wait >/dev/null
B=worklenz-chronostack-backend-1
MSYS_NO_PATHCONV=1 docker cp database/pg-migrations-support $B:/app/database/pg-migrations-support
MSYS_NO_PATHCONV=1 docker exec -e WORKLENZ_DEPLOYMENT_MODE=self_hosted $B npm run migrate:up >/dev/null 2>&1 || true
MSYS_NO_PATHCONV=1 docker exec worklenz-chronostack-db-1 psql -U postgres -d worklenz_db -v ON_ERROR_STOP=1 -q -f /seed/seed.sql >/dev/null
echo "seeded: $(docker exec worklenz-chronostack-db-1 psql -U postgres -d worklenz_db -Atc 'select count(*) from task_recurring_templates') recurring tasks"
