// SPDX-License-Identifier: MIT OR Apache-2.0
// node-cron: daily at 03:00 UTC; counts runs in Redis (nodecron:runs).
const cron = require('node-cron');
const IORedis = require('ioredis');
const r = new IORedis({ host: 'redis', port: 6379 });
cron.schedule('0 3 * * *', () => r.incr('nodecron:runs'), { timezone: 'UTC' });
console.log('node-cron ready');
