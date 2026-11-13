// SPDX-License-Identifier: MIT OR Apache-2.0
// Tiny API: POST /signup?email=…  →  { token }
//           GET  /me  (header x-session: token)  →  user, or 401 when the session expired
const http = require('http');
const crypto = require('crypto');
const { pool, redis } = require('./db');

const SESSION_TTL_SECONDS = 24 * 60 * 60;

async function main() {
  const r = await redis();
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://localhost');
    const send = (code, body) => {
      res.writeHead(code, { 'content-type': 'application/json' });
      res.end(JSON.stringify(body));
    };
    try {
      if (req.method === 'POST' && url.pathname === '/signup') {
        const email = url.searchParams.get('email');
        const { rows } = await pool.query(
          'INSERT INTO users (email) VALUES ($1) RETURNING id, plan, trial_ends_at',
          [email],
        );
        const token = crypto.randomBytes(16).toString('hex');
        await r.set(`session:${token}`, String(rows[0].id), { EX: SESSION_TTL_SECONDS });
        return send(201, { token, user: rows[0] });
      }
      if (req.method === 'GET' && url.pathname === '/me') {
        const userId = await r.get(`session:${req.headers['x-session']}`);
        if (!userId) return send(401, { error: 'session expired' });
        const { rows } = await pool.query(
          'SELECT id, email, plan, trial_ends_at, downgraded_at FROM users WHERE id = $1',
          [userId],
        );
        return send(200, rows[0]);
      }
      if (req.method === 'GET' && url.pathname === '/users') {
        const { rows } = await pool.query('SELECT id, email, plan, trial_ends_at, downgraded_at FROM users ORDER BY id');
        return send(200, rows);
      }
      send(404, { error: 'not found' });
    } catch (e) {
      send(500, { error: e.message });
    }
  });
  server.listen(3000, () => console.log('api listening on :3000'));
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
