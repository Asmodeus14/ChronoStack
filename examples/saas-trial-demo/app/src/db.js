// SPDX-License-Identifier: MIT OR Apache-2.0
const { Pool } = require('pg');
const { createClient } = require('redis');

// Connection settings come from PGHOST/PGUSER/... environment variables.
const pool = new Pool({ max: 4 });

async function redis() {
  const client = createClient({ url: process.env.REDIS_URL });
  client.on('error', (e) => console.error('redis error', e.message));
  await client.connect();
  return client;
}

module.exports = { pool, redis };
