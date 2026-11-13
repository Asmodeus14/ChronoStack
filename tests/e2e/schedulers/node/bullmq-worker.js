// SPDX-License-Identifier: MIT OR Apache-2.0
// BullMQ worker: counts processed jobs per job name in Redis (done:<name>).
const { Worker } = require('bullmq');
const IORedis = require('ioredis');
const connection = { host: 'redis', port: 6379, maxRetriesPerRequest: null };
const counters = new IORedis(connection);
new Worker('q', async (job) => { await counters.incr(`done:${job.name}`); }, { connection });
console.log('bullmq worker ready');
