// SPDX-License-Identifier: MIT OR Apache-2.0
import { Worker } from 'bullmq';
import IORedis from 'ioredis';
const connection = { host: 'redis', port: 6379, maxRetriesPerRequest: null };
const r = new IORedis(connection);
new Worker('emails', async () => { await new Promise(res => setTimeout(res, 4000)); await r.incr('done:bullmq'); }, { connection, concurrency: 5 });
