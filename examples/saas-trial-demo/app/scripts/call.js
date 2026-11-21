// SPDX-License-Identifier: MIT OR Apache-2.0
// Helper for demos and tests (runs inside the api container):
//   node scripts/call.js signup alice@example.com   → prints the session token
//   node scripts/call.js me <token>                 → prints {status, body}
//   node scripts/call.js users                      → prints all users
//   node scripts/call.js received <run_id> <n>      → checks that the trial.expired webhook of
//     each user signed up as …-<run_id>@example.com reached the receiver exactly once, for n users
const http = require('http');
const { pool, redis } = require('../src/db');

function request(method, path, headers = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port: 3000, method, path, headers }, (res) => {
      let data = '';
      res.on('data', (c) => (data += c));
      res.on('end', () => resolve({ status: res.statusCode, body: JSON.parse(data || 'null') }));
    });
    req.on('error', reject);
    req.end();
  });
}

(async () => {
  const [cmd, arg] = process.argv.slice(2);
  if (cmd === 'signup') {
    const r = await request('POST', `/signup?email=${encodeURIComponent(arg)}`);
    if (r.status !== 201) throw new Error(JSON.stringify(r));
    console.log(r.body.token);
  } else if (cmd === 'me') {
    console.log(JSON.stringify(await request('GET', '/me', { 'x-session': arg })));
  } else if (cmd === 'users') {
    console.log(JSON.stringify((await request('GET', '/users')).body));
  } else if (cmd === 'received') {
    const want = Number(process.argv[4]);
    const { rows } = await pool.query("SELECT id FROM users WHERE email LIKE '%-' || $1 || '@example.com'", [arg]);
    const r = await redis();
    let once = 0;
    for (const { id } of rows) {
      if ((await r.hGet('webhook:received', `trial.expired:${id}`)) === '1') once += 1;
    }
    await r.quit();
    await pool.end();
    console.log(`${once} of ${rows.length} users' trial.expired webhooks arrived exactly once`);
    if (rows.length !== want || once !== want) process.exit(1);
  } else {
    throw new Error('usage: call.js signup <email> | me <token> | users | received <run_id> <n>');
  }
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
