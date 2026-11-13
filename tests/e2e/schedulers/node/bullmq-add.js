// SPDX-License-Identifier: MIT OR Apache-2.0
// node bullmq-add.js delayed    -> job "delayed", due in 2 hours (virtual)
// node bullmq-add.js scheduler  -> job scheduler "daily", cron 0 3 * * * (UTC)
const { Queue } = require('bullmq');
const q = new Queue('q', { connection: { host: 'redis', port: 6379 } });
(async () => {
  if (process.argv[2] === 'delayed') await q.add('delayed', {}, { delay: 2 * 3600 * 1000 });
  else await q.upsertJobScheduler('daily', { pattern: '0 3 * * *', tz: 'UTC' }, { name: 'daily' });
  await q.close();
})().catch((e) => { console.error(e); process.exit(1); });
