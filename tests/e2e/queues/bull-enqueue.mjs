// SPDX-License-Identifier: MIT OR Apache-2.0
import { Queue } from 'bullmq';
const q = new Queue('emails', { connection: { host: 'redis', port: 6379 } });
for (let i = 0; i < 3; i++) await q.add('slow', { i });
console.log('queued 3 bullmq jobs'); await q.close();
