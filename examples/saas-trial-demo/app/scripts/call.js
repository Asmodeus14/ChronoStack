// SPDX-License-Identifier: MIT OR Apache-2.0
// Helper for demos and tests (runs inside the api container):
//   node scripts/call.js signup alice@example.com   → prints the session token
//   node scripts/call.js me <token>                 → prints {status, body}
//   node scripts/call.js users                      → prints all users
const http = require('http');

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
  } else {
    throw new Error('usage: call.js signup <email> | me <token> | users');
  }
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
