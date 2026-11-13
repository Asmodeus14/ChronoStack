# SPDX-License-Identifier: MIT OR Apache-2.0
# postgres:16 (Debian) + pg_cron from the PGDG apt repository already configured in the image.
FROM postgres:16
RUN apt-get update && apt-get install -y --no-install-recommends postgresql-16-cron && rm -rf /var/lib/apt/lists/*
