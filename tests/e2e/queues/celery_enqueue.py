# SPDX-License-Identifier: MIT OR Apache-2.0
from celery_tasks import later, slow

for i in range(3):
    slow.delay(i)
later.apply_async((0,), countdown=2 * 3600)
print("queued 3 slow celery tasks and 1 due in 2 h")
