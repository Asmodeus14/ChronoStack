# SPDX-License-Identifier: MIT OR Apache-2.0
FROM python:3.12-slim
RUN pip install --no-cache-dir "celery[redis]>=5.5" "psycopg[binary]>=3.2"
WORKDIR /app
COPY tasks.py ./
