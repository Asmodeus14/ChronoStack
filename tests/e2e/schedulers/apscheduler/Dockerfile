# SPDX-License-Identifier: MIT OR Apache-2.0
FROM python:3.12-alpine
RUN pip install --no-cache-dir "apscheduler>=3.11,<4" "redis>=5"
WORKDIR /app
COPY job.py ./
