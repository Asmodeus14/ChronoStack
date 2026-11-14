// SPDX-License-Identifier: MIT OR Apache-2.0
'use strict';
// ChronoStack for Node processes running directly on your machine (not in a
// container): makes `Date` follow the stack's virtual clock.
//
//   node -r chronostack/register app.js        (or NODE_OPTIONS="-r chronostack/register")
//
// The clock is read once, synchronously, at startup, then kept in sync with a
// long-poll to the controller (`/v1/clock/wait`), so an advance reaches this
// process within a few milliseconds. Only wall-clock time changes:
// `performance.now()`, `process.hrtime()` and timers keep real time, exactly
// like services inside the stack.
//
// Where the controller is: CHRONOSTACK_URL + CHRONOSTACK_TOKEN, or
// CHRONOSTACK_SESSION (path to .chronostack/session.json), or the nearest
// .chronostack/session.json above the current directory.
//
// If no session is found the process keeps real time and prints one warning
// (CHRONOSTACK_STRICT=1 makes that an error, for CI).

const fs = require('fs');
const path = require('path');
const http = require('http');
const { execFileSync } = require('child_process');

const RealDate = Date;
const realNow = RealDate.now.bind(RealDate);

const state = {
  installed: false,
  virtualMsAtFetch: 0,
  realMsAtFetch: 0,
  offsetMs: 0,
  frozen: false,
  generation: null,
  base: null,
  token: null,
};
const listeners = new Set();

/** Virtual milliseconds since the epoch. */
function now() {
  if (!state.installed) return realNow();
  return state.frozen ? state.virtualMsAtFetch : realNow() + state.offsetMs;
}

function findSession() {
  if (process.env.CHRONOSTACK_URL && process.env.CHRONOSTACK_TOKEN) {
    return { base: process.env.CHRONOSTACK_URL.replace(/\/+$/, ''), token: process.env.CHRONOSTACK_TOKEN };
  }
  let file = process.env.CHRONOSTACK_SESSION;
  if (!file) {
    let dir = process.cwd();
    for (;;) {
      const candidate = path.join(dir, '.chronostack', 'session.json');
      if (fs.existsSync(candidate)) {
        file = candidate;
        break;
      }
      const up = path.dirname(dir);
      if (up === dir) return null;
      dir = up;
    }
  }
  const s = JSON.parse(fs.readFileSync(file, 'utf8'));
  return { base: `http://127.0.0.1:${s.port}`, token: s.token };
}

function apply(st) {
  const changed = state.generation !== null && st.generation !== state.generation;
  state.virtualMsAtFetch = st.virtual_ns / 1e6;
  state.realMsAtFetch = st.real_ns / 1e6;
  // Map the controller's real clock to ours: virtual = our real now + offset.
  state.offsetMs = (st.virtual_ns - st.real_ns) / 1e6;
  state.frozen = !!st.frozen;
  state.generation = st.generation;
  if (changed) {
    const snapshot = { virtualMs: now(), offsetMs: state.offsetMs, frozen: state.frozen, generation: state.generation };
    for (const cb of listeners) {
      try {
        cb(snapshot);
      } catch (e) {
        process.emitWarning(`chronostack: onAdvance listener threw: ${e && e.message}`);
      }
    }
  }
}

/** Synchronous first read, in a short-lived child process (Node has no sync HTTP). */
function fetchSync(base, token) {
  const script =
    "fetch(process.env.CS_URL + '/v1/clock', { headers: { authorization: 'Bearer ' + process.env.CS_TOKEN } })" +
    ".then(async (r) => { const t = await r.text(); if (!r.ok) { console.error(r.status + ' ' + t); process.exit(2); } process.stdout.write(t); })" +
    '.catch((e) => { console.error(e.message); process.exit(1); });';
  const env = { ...process.env, CS_URL: base, CS_TOKEN: token, NODE_OPTIONS: '', CHRONOSTACK_REGISTER_CHILD: '1' };
  const out = execFileSync(process.execPath, ['-e', script], { env, timeout: 10000, stdio: ['ignore', 'pipe', 'pipe'] });
  return JSON.parse(out.toString('utf8'));
}

/** Long-poll for changes; never keeps the process alive. */
function watch() {
  const url = new URL(`${state.base}/v1/clock/wait?after=${state.generation}&timeout_ms=25000`);
  const req = http.get(url, { headers: { authorization: `Bearer ${state.token}` } }, (res) => {
    let body = '';
    res.setEncoding('utf8');
    res.on('data', (c) => (body += c));
    res.on('end', () => {
      if (res.statusCode === 200) {
        try {
          apply(JSON.parse(body));
          setImmediate(watch);
          return;
        } catch (_) {
          /* fall through to retry */
        }
      }
      retry();
    });
  });
  req.on('socket', (s) => s.unref());
  req.on('error', retry);
}

function retry() {
  setTimeout(watch, 1000).unref();
}

function patchDate() {
  class ChronoDate extends RealDate {
    constructor(...args) {
      if (args.length === 0) super(now());
      else super(...args);
    }
    static now() {
      return Math.floor(now());
    }
    static [Symbol.hasInstance](x) {
      return x instanceof RealDate;
    }
  }
  ChronoDate.parse = RealDate.parse;
  ChronoDate.UTC = RealDate.UTC;
  // `Date()` called without `new` returns a string of the current time.
  globalThis.Date = new Proxy(ChronoDate, {
    apply() {
      return new RealDate(now()).toString();
    },
  });
}

/** Install the virtual clock (idempotent). Returns true when it is active. */
function install() {
  if (state.installed) return true;
  if (process.env.CHRONOSTACK_REGISTER_CHILD) return false;
  const strict = process.env.CHRONOSTACK_STRICT === '1';
  try {
    const s = findSession();
    if (!s) throw new Error('no .chronostack/session.json found (run `chronostack up`, or set CHRONOSTACK_SESSION)');
    state.base = s.base;
    state.token = s.token;
    apply(fetchSync(s.base, s.token));
  } catch (e) {
    const msg = `chronostack: virtual clock not active, using real time: ${(e && e.message) || e}`;
    if (strict) throw new Error(msg);
    process.emitWarning(msg);
    return false;
  }
  state.installed = true;
  patchDate();
  watch();
  return true;
}

/** Call `cb({ virtualMs, offsetMs, frozen, generation })` after every clock change. */
function onAdvance(cb) {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

module.exports = {
  install,
  onAdvance,
  now,
  get active() {
    return state.installed;
  },
  get offsetMs() {
    return state.offsetMs;
  },
  get frozen() {
    return state.frozen;
  },
  RealDate,
};
