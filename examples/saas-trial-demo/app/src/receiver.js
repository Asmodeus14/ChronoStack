// SPDX-License-Identifier: MIT OR Apache-2.0
// Stand-in for a partner's webhook endpoint: POST /hooks with a JSON event.
// Counts deliveries per event id in Redis (HINCRBY webhook:received <id>), so a
// test can check that each event arrived exactly once.
const http = require('http');
const { redis } = require('./db');

async function main() {
  const r = await redis();
  http
    .createServer((req, res) => {
      if (req.method !== 'POST' || req.url !== '/hooks') {
        res.writeHead(404).end();
        return;
      }
      let body = '';
      req.on('data', (c) => (body += c));
      req.on('end', async () => {
        try {
          const evt = JSON.parse(body);
          const n = await r.hIncrBy('webhook:received', evt.id, 1);
          console.log(`receiver: ${evt.type} ${evt.id} (delivery #${n})`);
          res.writeHead(200, { 'content-type': 'application/json' });
          res.end(JSON.stringify({ ok: true, duplicate: n > 1 }));
        } catch (e) {
          res.writeHead(400).end();
        }
      });
    })
    .listen(4000, () => console.log('receiver: listening on :4000'));
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
