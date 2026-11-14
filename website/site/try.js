// "Try it here": a simulated chronostack session, modelled on the demo app.
// One user on a 14-day trial, a nightly job at 03:00 that ends expired trials and
// sends a webhook. Everything happens in the browser; nothing talks to Docker.

import { handsSVG, setHands } from './hands.js';

const $ = (s) => document.querySelector(s);
const log = $('#try-log');
const form = $('#try-form');
const input = $('#try-input');
if (log && form && input) init();

function init() {
  const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const TRIAL_END = 14 * 1440;     // minutes after signup
  const NIGHTLY = 3 * 60;          // 03:00
  const out = {
    time: $('#try-time'), pg: $('#try-pg'), redis: $('#try-redis'), api: $('#try-api'), worker: $('#try-worker'),
  };
  const handsHost = $('.try-hands');
  if (!handsHost.querySelector('svg')) handsHost.innerHTML = handsSVG();
  const hands = handsHost.querySelector('svg');

  let S = fresh();
  let busy = false;
  const history = [];
  let hIndex = 0;

  function fresh() {
    return { up: false, minutes: 0, runs: 0, expired: false, webhooks: 0 };
  }

  // ---- output
  const esc = (s) => s.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));
  function line(text, cls = '') {
    const el = document.createElement('div');
    if (cls) el.className = cls;
    el.innerHTML = esc(text);
    log.appendChild(el);
    while (log.children.length > 200) log.firstChild.remove();
    log.scrollTop = log.scrollHeight;
  }
  const fmt = (minutes) => {
    const m = Math.max(0, Math.floor(minutes + 1e-6));
    const d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60), mm = m % 60;
    return `+${String(d).padStart(2, '0')}d ${String(h).padStart(2, '0')}:${String(mm).padStart(2, '0')}`;
  };

  function set(el, text, changed) {
    if (el.textContent === text) return;
    el.textContent = text;
    if (changed) {
      el.classList.add('is-changed');
      clearTimeout(el._t);
      el._t = setTimeout(() => el.classList.remove('is-changed'), 1600);
    }
  }
  function paint(flash = false) {
    set(out.time, S.up ? fmt(S.minutes) : 'stack is down', false);
    setHands(hands, S.up ? S.minutes : 0);
    set(out.pg, `trial_ends_at ${fmt(TRIAL_END)}${S.minutes >= TRIAL_END ? ' (passed)' : ''}`, flash);
    set(out.redis, `webhook:received ${S.webhooks}`, flash);
    set(out.api, `plan: ${S.expired ? 'free' : 'trial'}`, flash);
    set(out.worker, `nightly job ran ${S.runs} time${S.runs === 1 ? '' : 's'}`, flash);
  }

  // ---- durations: 15d, 12h, 90m, 1d12h, 2w
  function parseDuration(s) {
    const re = /(\d+(?:\.\d+)?)\s*(w|d|h|m)/g;
    let total = 0, matched = '', m;
    while ((m = re.exec(s))) {
      total += parseFloat(m[1]) * { w: 10080, d: 1440, h: 60, m: 1 }[m[2]];
      matched += m[0];
    }
    if (!total || matched.replace(/\s/g, '') !== s.replace(/\s/g, '')) return null;
    return Math.round(total);
  }

  const wait = (ms) => new Promise((r) => setTimeout(r, REDUCED ? 0 : ms));

  // Move the clock forward, stopping at each 03:00 to run the nightly job.
  async function advance(by, quiet = false) {
    const target = S.minutes + by;
    const stops = [];
    let next = Math.floor((S.minutes - NIGHTLY) / 1440 + 1) * 1440 + NIGHTLY;
    if (next <= S.minutes) next += 1440;
    for (; next <= target; next += 1440) stops.push(next);

    const duration = REDUCED ? 0 : Math.min(1800, 250 + stops.length * 110);
    const from = S.minutes;
    const t0 = performance.now();
    let si = 0;
    await new Promise((resolve) => {
      function frame(now) {
        const k = duration ? Math.min(1, (now - t0) / duration) : 1;
        const eased = 1 - Math.pow(1 - k, 3);
        S.minutes = from + (target - from) * eased;
        while (si < stops.length && S.minutes >= stops[si]) nightly(stops[si++], quiet);
        paint(true);
        if (k < 1) requestAnimationFrame(frame); else resolve();
      }
      requestAnimationFrame(frame);
    });
    while (si < stops.length) nightly(stops[si++], quiet);
    S.minutes = target;
    paint(true);
  }

  function nightly(at, quiet) {
    S.runs++;
    if (!S.expired && at >= TRIAL_END) {
      S.expired = true;
      S.webhooks = 1;
      line(`→ ${fmt(at)}  nightly job: 1 trial expired, plan → free, webhook sent`, 'l-hit');
    } else if (!quiet) {
      line(`→ ${fmt(at)}  nightly job: no trials to expire`);
    }
  }

  // ---- commands
  const HELP = [
    'up --wait            start the demo stack on a virtual clock',
    'advance <duration>   move time forward, e.g. advance 3d, advance 12h',
    'status               show the virtual time',
    'doctor               check that every service sees the same time',
    'run scenarios/trial.yml   run the trial scenario from the start',
    'down                 stop the stack',
    'clear                clear the screen',
  ];

  async function exec(raw) {
    const cmd = raw.trim().replace(/^chronostack\s+/, '').replace(/\s+/g, ' ');
    if (!cmd) return;
    line(cmd, 'l-cmd');
    const [verb, ...rest] = cmd.split(' ');
    const arg = rest.join(' ');

    switch (verb) {
      case 'help': case '--help': case '-h':
        HELP.forEach((h) => line(h));
        break;
      case 'clear':
        log.innerHTML = '';
        break;
      case 'up':
        if (S.up) { line('the stack is already up'); break; }
        line('starting a copy of the demo stack on a virtual clock…');
        await wait(500);
        S = { ...fresh(), up: true };
        paint(true);
        line('postgres, redis, api, worker: ready at +00d 00:00', 'l-ok');
        line('the demo user signed up; their trial ends at +14d 00:00');
        break;
      case 'advance': {
        if (!S.up) { line('the stack is down; run: up --wait', 'l-err'); break; }
        const by = parseDuration(arg.replace(/--\S+/g, '').trim());
        if (!by) { line('usage: advance <duration>, e.g. advance 3d or advance 1d12h', 'l-err'); break; }
        if (by > 365 * 1440) { line('that is more than a year; try a shorter step', 'l-err'); break; }
        await advance(by);
        line(`advanced to ${fmt(S.minutes)}; all 4 services agree`, 'l-ok');
        break;
      }
      case 'status':
        line(S.up ? `virtual time ${fmt(S.minutes)}` : 'the stack is down');
        break;
      case 'doctor':
        if (!S.up) { line('the stack is down; run: up --wait', 'l-err'); break; }
        for (const s of ['postgres', 'redis', 'api', 'worker']) {
          await wait(160);
          line(`${s.padEnd(9)} sees ${fmt(S.minutes)}  ok`);
        }
        line('everything is on one clock', 'l-ok');
        break;
      case 'run': {
        if (!S.up) { line('the stack is down; run: up --wait', 'l-err'); break; }
        if (!/trial\.ya?ml$/.test(arg)) { line('try: run scenarios/trial.yml', 'l-err'); break; }
        line('scenario trial.yml: resetting to +00d 00:00');
        S = { ...fresh(), up: true };
        paint(true);
        await wait(300);
        line('step: advance 14d');
        await advance(14 * 1440, true);
        line('step: advance 12h');
        await advance(12 * 60, true);
        await wait(250);
        line(`assert plan = free            ${S.expired ? 'ok' : 'FAILED'}`);
        line(`assert webhook:received = 1   ${S.webhooks === 1 ? 'ok' : 'FAILED'}`);
        line('PASSED (simulated)', 'l-ok');
        break;
      }
      case 'down':
        if (!S.up) { line('the stack is already down'); break; }
        S = fresh();
        paint();
        line('stopped the copy; your own containers were never touched', 'l-ok');
        break;
      default:
        line(`unknown command: ${verb}. Type help to see what this simulation knows.`, 'l-err');
    }
  }

  async function submit(text) {
    if (busy) return;
    busy = true;
    input.disabled = true;
    try { await exec(text); } finally {
      busy = false;
      input.disabled = false;
    }
  }

  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const text = input.value;
    if (text.trim()) { history.push(text); hIndex = history.length; }
    input.value = '';
    submit(text).then(() => input.focus({ preventScroll: true }));
  });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowUp' && history.length) {
      hIndex = Math.max(0, hIndex - 1); input.value = history[hIndex]; e.preventDefault();
    } else if (e.key === 'ArrowDown' && history.length) {
      hIndex = Math.min(history.length, hIndex + 1); input.value = history[hIndex] || ''; e.preventDefault();
    }
  });
  document.querySelectorAll('.try-chips [data-cmd]').forEach((b) => {
    b.addEventListener('click', () => submit(b.dataset.cmd));
  });
  // clicking anywhere on the terminal focuses the prompt, like a real one
  document.querySelector('.try-term').addEventListener('click', (e) => {
    if (!e.target.closest('a, button') && !getSelection().toString()) input.focus({ preventScroll: true });
  });

  // The stack starts already running, so advance works straight away.
  S = { ...fresh(), up: true };
  line('up --wait', 'l-cmd');
  line('postgres, redis, api, worker: ready at +00d 00:00', 'l-ok');
  line('the demo user signed up; their trial ends at +14d 00:00');
  line('Try: advance 15d   (or type help)');
  paint();
}
