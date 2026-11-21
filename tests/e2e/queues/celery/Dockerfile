# SPDX-License-Identifier: MIT OR Apache-2.0
# Celery worker for run_queues.py. Installed at build time (cached, with retries)
# rather than when the container starts.
FROM python:3.12-slim
RUN pip install --no-cache-dir --retries 10 --timeout 120 'celery[redis]==5.4.0'
