# SPDX-License-Identifier: MIT OR Apache-2.0
# Celery worker for run_queues.py. Installed at build time (cached, with retries)
# rather than when the container starts.
FROM python:3.12-slim
# kombu's Redis transport imports `packaging`, which celery 5.4.0 doesn't declare.
RUN pip install --no-cache-dir --retries 10 --timeout 120 'celery[redis]==5.4.0' 'packaging==24.2'
