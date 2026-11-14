// SPDX-License-Identifier: MIT OR Apache-2.0
'use strict';
// Unit tests against a fake controller (no Docker needed): `node --test test/`.
const test = require('node:test');
const assert = require('node:assert');
const http = require('node:http');
const path = require('node:path');
const { spawn } = require('node:child_process');

const REGISTER = path.join(__dirname, '..', 'register.js');
const DAY_MS = 86_400_000;
const TOKEN = 't'.repeat(64);

/** A fake clockd: GET /v1/clock and long-poll GET /v1/clock/wait. */
function fakeClockd() {
  let st = { offsetMs: 14 * DAY_MS, frozen: false, frozenMs: 0, generation: 1 };
  const waiters = new Set();
  const body = () => {
    const real = Date.now();
    const virt = st.frozen ? st.frozenMs : real + st.offsetMs;
    return JSON.stringify({
      virtual_ns: virt * 1e6, real_ns: real * 1e6, offset_ns: (virt - real) * 1e6,
      frozen: st.frozen, generation: st.generation, session_id: 'x', layout_version: 2,
    });
  };
  const seenAuth = [];
  const server = http.createServer((req, res) => {
    seenAuth.push(req.headers.authorization);
    if (req.headers.authorization !== `Bearer ${TOKEN}`) {
      res.writeHead(401).end('{"error":"token"}');
      return;
    }
    const url = new URL(req.url, 'http://x');
    if (url.pathname === '/v1/clock') {
      res.end(body());
    } else if (url.pathname === '/v1/clock/wait') {
      const after = Number(url.searchParams.get('after'));
      if (after !== st.generation) {
        res.end(body());
        return;
      }
      const w = () => {
        waiters.delete(w);
        res.end(body());
      };
      waiters.add(w);
      req.on('close', () => waiters.delete(w));
    } else {
      res.writeHead(404).end();
    }
  });
  return new Promise((resolve) =>
    server.listen(0, '127.0.0.1', () =>
      resolve({
        url: `http://127.0.0.1:${server.address().port}`,
        seenAuth,
        change(patch) {
          st = { ...st, ...patch, generation: st.generation + 1 };
          for (const w of [...waiters]) w();
        },
        close: () => new Promise((r) => { server.closeAllConnections(); server.close(r); }),
      }),
    ),
  );
}

/** Run `code` under -r register; collect JSON lines it prints. */
function run(code, env, onLine) {
  return new Promise((resolve) => {
    const p = spawn(process.execPath, ['-r', REGISTER, '-e', code], {
      env: { ...process.env, NODE_OPTIONS: '', CHRONOSTACK_SESSION: '', ...env },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let out = '';
    let err = '';
    p.stdout.on('data', (c) => {
      out += c;
      let i;
      while ((i = out.indexOf('\n')) >= 0) {
        const line = out.slice(0, i);
        out = out.slice(i + 1);
        if (onLine) onLine(JSON.parse(line), p);
      }
    });
    p.stderr.on('data', (c) => (err += c));
    p.on('close', (code) => resolve({ code, err }));
  });
}

test('Date follows the virtual clock', async () => {
  const fake = await fakeClockd();
  const lines = [];
  const r = await run(
    `const t = Date.now();
     console.log(JSON.stringify({
       skew: t - require(${JSON.stringify(path.join(__dirname, '..', 'index.js'))}).RealDate.now(),
       newDate: new Date().getTime() - t,
       epoch: new Date(0).getTime(),
       parts: new Date(2020, 0, 2).getFullYear(),
       isDate: new Date() instanceof Date,
       str: typeof Date(),
       utc: Date.UTC(2020, 0, 1),
     }));`,
    { CHRONOSTACK_URL: fake.url, CHRONOSTACK_TOKEN: TOKEN },
    (l) => lines.push(l),
  );
  await fake.close();
  assert.strictEqual(r.code, 0, r.err);
  const l = lines[0];
  assert.ok(Math.abs(l.skew - 14 * DAY_MS) < 50, `skew ${l.skew}`);
  assert.ok(Math.abs(l.newDate) < 50);
  assert.strictEqual(l.epoch, 0, 'explicit timestamps are untouched');
  assert.strictEqual(l.parts, 2020);
  assert.strictEqual(l.isDate, true);
  assert.strictEqual(l.str, 'string');
  assert.strictEqual(l.utc, 1577836800000);
  assert.ok(fake.seenAuth.every((a) => a === `Bearer ${TOKEN}`));
});

test('an advance reaches the process quickly, and onAdvance fires', async () => {
  const fake = await fakeClockd();
  const idx = JSON.stringify(path.join(__dirname, '..', 'index.js'));
  let sentAt = 0;
  const seen = [];
  const r = await run(
    `const cs = require(${idx});
     console.log(JSON.stringify({ ready: true, offset: Date.now() - cs.RealDate.now() }));
     cs.onAdvance((c) => {
       console.log(JSON.stringify({ offset: Date.now() - cs.RealDate.now(), frozen: c.frozen, gen: c.generation, at: cs.RealDate.now() }));
       if (c.frozen) process.exit(0);
     });
     setInterval(() => {}, 1000);`,
    { CHRONOSTACK_URL: fake.url, CHRONOSTACK_TOKEN: TOKEN },
    (l) => {
      seen.push(l);
      if (l.ready) {
        setTimeout(() => { sentAt = Date.now(); fake.change({ offsetMs: 30 * DAY_MS }); }, 200);
      } else if (!l.frozen) {
        setTimeout(() => fake.change({ frozen: true, frozenMs: Date.UTC(2030, 0, 1) }), 100);
      }
    },
  );
  await fake.close();
  assert.strictEqual(r.code, 0, r.err);
  const adv = seen.find((l) => !l.ready && !l.frozen);
  assert.ok(Math.abs(adv.offset - 30 * DAY_MS) < 50, `offset ${adv.offset}`);
  assert.ok(adv.at - sentAt < 200, `took ${adv.at - sentAt} ms`);
  const fr = seen.find((l) => l.frozen);
  assert.ok(fr, 'frozen change observed');
});

test('frozen clock: Date.now() stays constant', async () => {
  const fake = await fakeClockd();
  fake.change({ frozen: true, frozenMs: Date.UTC(2030, 0, 1) });
  const lines = [];
  const r = await run(
    `const a = Date.now(); setTimeout(() => { console.log(JSON.stringify({ a, b: Date.now() })); }, 120);`,
    { CHRONOSTACK_URL: fake.url, CHRONOSTACK_TOKEN: TOKEN },
    (l) => lines.push(l),
  );
  await fake.close();
  assert.strictEqual(r.code, 0, r.err);
  assert.strictEqual(lines[0].a, Date.UTC(2030, 0, 1));
  assert.strictEqual(lines[0].b, lines[0].a);
});

test('no session: real time and one warning; strict mode fails', async () => {
  const lines = [];
  const env = { CHRONOSTACK_URL: '', CHRONOSTACK_TOKEN: '', CHRONOSTACK_SESSION: path.join(__dirname, 'missing.json') };
  const r = await run(`console.log(JSON.stringify({ d: Math.abs(Date.now() - ${Date.now()}) }))`, env, (l) => lines.push(l));
  assert.strictEqual(r.code, 0);
  assert.match(r.err, /virtual clock not active/);
  assert.ok(lines[0].d < 60_000);
  const strict = await run('0', { ...env, CHRONOSTACK_STRICT: '1' });
  assert.notStrictEqual(strict.code, 0);
});

test('wrong token: falls back to real time with a warning', async () => {
  const fake = await fakeClockd();
  const r = await run('0', { CHRONOSTACK_URL: fake.url, CHRONOSTACK_TOKEN: 'wrong' });
  await fake.close();
  assert.strictEqual(r.code, 0);
  assert.match(r.err, /401/);
});
