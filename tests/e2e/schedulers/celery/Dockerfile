# SPDX-License-Identifier: MIT OR Apache-2.0
FROM python:3.12-alpine
RUN pip install --no-cache-dir "celery[redis]>=5.5" packaging
WORKDIR /app
COPY tasks.py ./
