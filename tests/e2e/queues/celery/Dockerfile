# SPDX-License-Identifier: MIT OR Apache-2.0
# Celery worker for run_queues.py. Installed at build time (cached, with retries)
# rather than when the container starts.
FROM python:3.12-slim
# celery 5.4.0 doesn't cap kombu, and kombu >= 5.6 needs redis-py >= 6.1, which
# celery 5.4.0 doesn't allow: pin a set that works together.
RUN pip install --no-cache-dir --retries 10 --timeout 120 'celery==5.4.0' 'kombu==5.4.2' 'redis==5.2.1'
