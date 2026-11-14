// ChronoStack landing page: scroll is time.
// GSAP + ScrollTrigger + Lenis from CDNs (loaded with defer before this module).

import { handsSVG, setHands, HOUR_PATH, MINUTE_PATH, PIVOT_PATH } from './hands.js';

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const root = document.documentElement;
const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches;
const TOTAL = 14 * 1440 + 180; // the page runs from +00d 00:00 to +14d 03:00

const fmt = (minutes) => {
  const m = Math.max(0, Math.floor(minutes + 1e-6));
  const d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60), mm = m % 60;
  return `+${String(d).padStart(2, '0')}d ${String(h).padStart(2, '0')}:${String(mm).padStart(2, '0')}`;
};

// ------------------------------------------------------------ hands everywhere
$$('[data-hands]').forEach((el) => { el.innerHTML = handsSVG(); });
const navHands = $('.nav-mark .hands');
const clockHands = $('.scroll-clock-hands .hands');
setHands(navHands, 0);
setHands(clockHands, 0);
setHands($('.foot-mark .hands'), 180);         // 03:00, the nightly job
setHands($('.hero-cue-hand .hands'), 0);
$$('.glyph-sync .hands').forEach((h, i) => setHands(h, [130, 470, 260, 610][i]));

// ------------------------------------------------------------ tabs and copy (no animation library needed)
const tabs = $$('[role="tab"]');
function selectTab(tab) {
  tabs.forEach((t) => {
    const on = t === tab;
    t.setAttribute('aria-selected', on);
    t.tabIndex = on ? 0 : -1;
    $('#' + t.getAttribute('aria-controls')).hidden = !on;
  });
}
tabs.forEach((t, i) => {
  t.addEventListener('click', () => selectTab(t));
  t.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    const next = tabs[(i + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length];
    selectTab(next); next.focus();
  });
});
if (/Win/.test(navigator.platform)) selectTab($('#tab-win'));

$$('.copy').forEach((btn) => {
  btn.addEventListener('click', async () => {
    const text = btn.parentElement.querySelector('code').textContent;
    try {
      await navigator.clipboard.writeText(text);
      btn.textContent = 'Copied';
    } catch {
      btn.textContent = 'Select and copy';
    }
    btn.classList.add('is-copied');
    clearTimeout(btn._t);
    btn._t = setTimeout(() => { btn.textContent = 'Copy'; btn.classList.remove('is-copied'); }, 1600);
  });
});

// ------------------------------------------------------------ the timeline diagram (built once, drawn from a day value)
const NS = 'http://www.w3.org/2000/svg';
const svgEl = (tag, attrs = {}, parent) => {
  const n = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  if (parent) parent.appendChild(n);
  return n;
};
const X0 = 140, X1 = 975, DAYS = 15;
const dayX = (d) => X0 + ((X1 - X0) * d) / DAYS;
const ROWS = ['postgres', 'redis', 'api', 'worker'];
const ROW_Y = [70, 160, 250, 340];
const NIGHTLY = Array.from({ length: 15 }, (_, d) => d + 0.125);   // 03:00 every day
const EVENTS = [
  { row: 2, day: 0.02, text: 'trial starts' },
  { row: 1, day: 1.0, text: 'session expires' },
  { row: 0, day: 14.125, text: 'trial_ends_at < now()' },
  { row: 2, day: 14.13, text: 'plan → free' },
];

const tl = $('#timeline');
const T = { rows: [], nightly: [], events: [] };
(function buildTimeline() {
  const axis = svgEl('g', {}, tl);
  for (let d = 0; d <= DAYS; d++) {
    svgEl('line', { x1: dayX(d), x2: dayX(d), y1: 388, y2: d % 5 ? 394 : 400, stroke: 'currentColor', 'stroke-opacity': d % 5 ? 0.3 : 0.7 }, axis);
    if (d % 5 === 0) {
      const t = svgEl('text', { x: dayX(d), y: 424, 'text-anchor': 'middle', 'font-size': 14, 'fill-opacity': 0.7 }, axis);
      t.textContent = `day ${d}`;
    }
  }
  ROWS.forEach((name, i) => {
    const y = ROW_Y[i];
    const g = svgEl('g', {}, tl);
    const label = svgEl('text', { x: 0, y: y + 5, 'font-size': 16 }, g);
    label.textContent = name;
    svgEl('line', { x1: X0, x2: X1, y1: y, y2: y, stroke: 'currentColor', 'stroke-opacity': 0.18, 'stroke-width': 1 }, g);
    const prog = svgEl('line', { x1: X0, x2: X0, y1: y, y2: y, stroke: 'currentColor', 'stroke-width': 3, 'stroke-linecap': 'round' }, g);
    T.rows.push(prog);
  });
  // the worker's nightly job at every virtual 03:00
  NIGHTLY.forEach((day) => {
    const x = dayX(day), y = ROW_Y[3];
    const pulse = svgEl('circle', { cx: x, cy: y, r: 6, fill: 'none', stroke: 'currentColor', 'stroke-width': 1.5, opacity: 0 }, tl);
    const dot = svgEl('circle', { cx: x, cy: y, r: 6, fill: 'var(--paper)', stroke: 'currentColor', 'stroke-width': 1.5 }, tl);
    T.nightly.push({ day, dot, pulse });
  });
  const nl = svgEl('text', { x: dayX(0.125) + 12, y: ROW_Y[3] + 30, 'font-size': 13, 'fill-opacity': 0.75 }, tl);
  nl.textContent = 'nightly job, every 03:00';
  EVENTS.forEach((ev) => {
    const x = dayX(ev.day), y = ROW_Y[ev.row];
    const g = svgEl('g', { opacity: 0.25 }, tl);
    svgEl('path', { d: `M${x},${y - 8}L${x + 8},${y}L${x},${y + 8}L${x - 8},${y}Z`, fill: 'currentColor' }, g);
    const right = ev.day > 10;
    const t = svgEl('text', { x: right ? x - 14 : x + 14, y: y - 16, 'font-size': 14, 'text-anchor': right ? 'end' : 'start' }, g);
    t.textContent = ev.text;
    T.events.push({ ...ev, g });
  });
  // the playhead carries a tiny pair of hands
  const ph = svgEl('g', {}, tl);
  svgEl('line', { x1: 0, x2: 0, y1: 22, y2: 380, stroke: 'currentColor', 'stroke-width': 1.5 }, ph);
  const face = svgEl('g', { transform: 'translate(0,0) scale(0.024)' }, ph);
  svgEl('circle', { r: 960, fill: 'var(--paper)', stroke: 'currentColor', 'stroke-width': 60 }, face);
  T.hour = svgEl('path', { d: HOUR_PATH, 'fill-rule': 'evenodd', fill: 'currentColor' }, face);
  T.min = svgEl('path', { d: MINUTE_PATH, 'fill-rule': 'evenodd', fill: 'currentColor' }, face);
  svgEl('path', { d: PIVOT_PATH, 'fill-rule': 'evenodd', fill: 'currentColor' }, face);
  T.playhead = ph;
})();

const term = $('#term');
const stackTime = $('#stack-time');
const stackNote = $('#stack-note');
const typed = (s, k) => s.slice(0, Math.round(s.length * Math.min(1, Math.max(0, k))));

function drawTimeline(day, termIn) {
  const x = dayX(day);
  T.rows.forEach((l) => l.setAttribute('x2', x));
  T.playhead.setAttribute('transform', `translate(${x},0)`);
  const minutes = day * 1440;
  T.min.setAttribute('transform', `rotate(${(minutes / 60) * 360})`);
  T.hour.setAttribute('transform', `rotate(${(minutes / 720) * 360})`);
  T.nightly.forEach(({ day: d, dot, pulse }) => {
    const k = day - d;
    dot.setAttribute('fill', k >= 0 ? 'currentColor' : 'var(--paper)');
    const pk = k >= 0 && k < 0.6 ? k / 0.6 : -1;
    pulse.setAttribute('opacity', pk >= 0 ? (1 - pk).toFixed(3) : 0);
    pulse.setAttribute('r', pk >= 0 ? 6 + 22 * pk : 6);
  });
  T.events.forEach((ev) => ev.g.setAttribute('opacity', day >= ev.day ? 1 : 0.22));
  stackTime.textContent = fmt(minutes);

  // the terminal tells the same story; it is drawn from scratch so scrolling back works
  const lines = [];
  lines.push(`<span class="t-cmd">${typed('chronostack up --wait', termIn / 0.25)}</span>`);
  if (termIn > 0.3) lines.push('<span class="t-out">postgres, redis, api, worker: one virtual clock</span>');
  if (termIn > 0.45) lines.push(`<span class="t-cmd">${typed('chronostack advance 15d', (termIn - 0.45) / 0.3)}</span>`);
  if (termIn >= 0.8) {
    NIGHTLY.forEach((d) => {
      if (day < d) return;
      const n = Math.floor(d);
      lines.push(n === 14
        ? `<span class="t-hit">→ ${fmt(d * 1440)}  nightly job: trial expired, plan → free</span>`
        : `<span class="t-out">→ ${fmt(d * 1440)}  nightly job</span>`);
    });
    if (day >= DAYS) lines.push('<span class="t-out">advanced 15 days</span>');
  }
  if (day < DAYS) lines.push('<span class="t-caret"></span>');
  term.innerHTML = lines.join('\n');

  stackNote.textContent = day >= 14.125 ? 'day 14, 03:00: the trial expired, on schedule'
    : day > 0 ? 'chronostack advance 15d'
    : 'chronostack up --wait';
}

// ------------------------------------------------------------ no animation library, or reduced motion: show the end state
const gsap = window.gsap;
const ScrollTrigger = window.ScrollTrigger;
function showEverything() {
  $$('[data-reveal], [data-hero]').forEach((el) => { el.style.opacity = 1; });
  drawTimeline(DAYS, 1);
  $('.stack-flood').style.clipPath = 'none';
  $('.install-flood').style.clipPath = 'none';
  $$('.glyph-verify path').forEach((p) => { p.style.strokeDashoffset = 0; });
  $$('.glyph-sync .hands').forEach((h) => setHands(h, 0));
  const update = () => {
    const max = document.documentElement.scrollHeight - innerHeight;
    const m = TOTAL * (max > 0 ? scrollY / max : 0);
    setHands(clockHands, m); setHands(navHands, m);
    $('#scroll-time').textContent = fmt(m);
  };
  addEventListener('scroll', update, { passive: true });
  update();
}

if (!gsap || !ScrollTrigger || REDUCED) {
  showEverything();
} else {
  animate();
}

// ------------------------------------------------------------ the motion
function animate() {
  gsap.registerPlugin(ScrollTrigger);

  // smooth scroll, paused while the intro plays
  let lenis = null;
  if (window.Lenis) {
    lenis = new window.Lenis({ lerp: 0.1, wheelMultiplier: 0.95 });
    lenis.on('scroll', ScrollTrigger.update);
    gsap.ticker.add((t) => lenis.raf(t * 1000));
    gsap.ticker.lagSmoothing(0);
    if (root.classList.contains('is-locked')) lenis.stop();
    $$('a[href^="#"]').forEach((a) => a.addEventListener('click', (e) => {
      const target = $(a.getAttribute('href'));
      if (!target) return;
      e.preventDefault();
      lenis.scrollTo(target, { duration: 1.6, easing: (x) => 1 - Math.pow(1 - x, 4) });
    }));
  }

  // ---- intro hand-off: the hero arrives where the hands folded
  const heroEls = $$('[data-hero]');
  gsap.set(heroEls, { opacity: 0, y: 28, filter: 'blur(10px)' });
  function heroIn() {
    gsap.to(heroEls, { opacity: 1, y: 0, filter: 'blur(0px)', duration: 1.5, ease: 'expo.out', stagger: 0.09, delay: 0.15 });
    lenis?.start();
    ScrollTrigger.refresh();
  }
  function heroOut() {
    gsap.killTweensOf(heroEls);
    gsap.set(heroEls, { opacity: 0, y: 28, filter: 'blur(10px)' });
    lenis?.scrollTo(0, { immediate: true });
    lenis?.stop();
  }
  addEventListener('chronostack:intro-done', heroIn);
  addEventListener('chronostack:intro-start', heroOut);
  if (root.classList.contains('intro-done') || root.classList.contains('no-intro')) heroIn();

  // the hero drifts up and dims as time starts moving
  gsap.to('.hero-inner', {
    yPercent: -18, opacity: 0.25, ease: 'none',
    scrollTrigger: { trigger: '.hero', start: 'top top', end: 'bottom top', scrub: true },
  });

  // ---- the scroll clock: the whole page is fourteen days
  ScrollTrigger.create({
    start: 0, end: 'max',
    onUpdate: (self) => {
      const m = TOTAL * self.progress;
      setHands(clockHands, m);
      setHands(navHands, m);
      $('#scroll-time').textContent = fmt(m);
    },
  });

  // ---- headline and paragraph reveals
  function splitWords(el) {
    const walk = (node) => {
      [...node.childNodes].forEach((c) => {
        if (c.nodeType === 3) {
          const frag = document.createDocumentFragment();
          c.textContent.split(/(\s+)/).forEach((part) => {
            if (!part) return;
            if (/^\s+$/.test(part)) { frag.appendChild(document.createTextNode(part)); return; }
            const m = document.createElement('span');
            m.className = 'wm';
            m.style.cssText = 'display:inline-block;overflow:hidden;vertical-align:top;padding-bottom:.1em;margin-bottom:-.1em';
            const i = document.createElement('span');
            i.className = 'wi';
            i.style.display = 'inline-block';
            i.textContent = part;
            m.appendChild(i);
            frag.appendChild(m);
          });
          c.replaceWith(frag);
        } else if (c.nodeType === 1 && c.tagName !== 'BR') walk(c);
      });
    };
    walk(el);
    return $$('.wi', el);
  }

  $$('[data-reveal]').forEach((el) => {
    const inInstall = el.closest('.install');
    const start = inInstall ? 'top 72%' : 'top 86%';
    if (/^H[23]$/.test(el.tagName)) {
      const words = splitWords(el);
      gsap.set(el, { opacity: 1 });
      gsap.fromTo(words, { yPercent: 110 }, {
        yPercent: 0, duration: 1.2, ease: 'expo.out', stagger: 0.035,
        scrollTrigger: { trigger: el, start, once: true },
      });
    } else {
      gsap.fromTo(el, { opacity: 0, y: 26 }, {
        opacity: 1, y: 0, duration: 1.2, ease: 'expo.out', delay: 0.12,
        scrollTrigger: { trigger: el, start, once: true },
      });
    }
  });

  // ---- the problem: words light up as you read, then the old ways get struck out
  const statement = $('[data-words]');
  const words = splitWords(statement).map((w) => w.parentElement);
  gsap.fromTo(words, { opacity: 0.14 }, {
    opacity: 1, ease: 'none', stagger: 0.1,
    scrollTrigger: { trigger: statement, start: 'top 78%', end: 'bottom 38%', scrub: true },
  });
  $$('.strike-list li').forEach((li) => {
    const span = $('span', li);
    gsap.fromTo(span, { '--strike': 0, opacity: 1 }, {
      '--strike': 1, opacity: 0.4, ease: 'none',
      scrollTrigger: { trigger: li, start: 'top 78%', end: 'top 52%', scrub: true },
    });
  });

  // ---- the stack scene: a drop of white floods the screen, then fifteen days pass
  const stackState = { flood: 0, show: 0, term: 0, day: 0 };
  const content = $('.stack-content');
  const flood = $('.stack-flood');
  const drawStack = () => {
    flood.style.clipPath = `circle(${(stackState.flood * 75).toFixed(2)}% at 50% 50%)`;
    content.style.opacity = stackState.show;
    content.style.transform = `translateY(${(1 - stackState.show) * 24}px)`;
    drawTimeline(stackState.day, stackState.term);
  };
  gsap.timeline({
    defaults: { ease: 'none' },
    onUpdate: drawStack,
    scrollTrigger: { trigger: '.stack', start: 'top top', end: 'bottom bottom', scrub: 0.6 },
  })
    .to(stackState, { flood: 1, duration: 0.14, ease: 'power2.in' }, 0)
    .to(stackState, { show: 1, duration: 0.07, ease: 'power2.out' }, 0.12)
    .to(stackState, { term: 1, duration: 0.12 }, 0.17)
    .to(stackState, { day: DAYS, duration: 0.62, ease: 'power1.inOut' }, 0.3)
    .to({}, { duration: 0.08 });
  drawStack();

  // ---- glyphs
  // four clocks, out of sync, lock together and run as one
  const syncHands = $$('.glyph-sync .hands');
  const sync = { m: [130, 470, 260, 610], common: 0, k: 0 };
  let syncRun = false;
  gsap.ticker.add((t, dt) => {
    if (!syncRun) return;
    sync.common += dt * 0.012 * 60;
    syncHands.forEach((h, i) => setHands(h, sync.m[i] + (sync.common - sync.m[i]) * sync.k));
  });
  ScrollTrigger.create({
    trigger: '.glyph-sync', start: 'top 80%', end: 'bottom top',
    onToggle: (self) => { syncRun = self.isActive; },
    onEnter: () => gsap.to(sync, { k: 1, duration: 1.8, ease: 'expo.inOut', delay: 0.4 }),
  });

  // a hand that stops at each trigger
  const marks = $('.glyph-stops .g-marks');
  for (let i = 0; i < 12; i++) {
    const a = (i / 12) * Math.PI * 2;
    svgEl('line', { x1: Math.sin(a) * 78, y1: -Math.cos(a) * 78, x2: Math.sin(a) * 86, y2: -Math.cos(a) * 86 }, marks);
  }
  const stops = [90, 210, 300, 360];
  const dots = stops.map((deg) => {
    const a = (deg * Math.PI) / 180;
    return svgEl('circle', { cx: Math.sin(a) * 60, cy: -Math.cos(a) * 60, r: 6, opacity: 0.2 }, marks);
  });
  const hand = $('.glyph-stops .g-hand');
  const stopTl = gsap.timeline({ repeat: -1, paused: true, repeatDelay: 0.5 });
  stopTl.set(hand, { rotation: 0, svgOrigin: '0 0' });
  stopTl.set(dots, { opacity: 0.2, scale: 1, transformOrigin: 'center' });
  stops.forEach((deg, i) => {
    stopTl.to(hand, { rotation: deg, duration: 0.9, ease: 'power3.inOut', svgOrigin: '0 0' });
    stopTl.to(dots[i], { opacity: 1, scale: 1.5, duration: 0.18, ease: 'power2.out', transformOrigin: 'center' });
    stopTl.to(dots[i], { scale: 1, duration: 0.4, ease: 'power2.out', transformOrigin: 'center' });
  });
  ScrollTrigger.create({
    trigger: '.glyph-stops', start: 'top 85%', end: 'bottom top',
    onToggle: (self) => (self.isActive ? stopTl.play() : stopTl.pause()),
  });

  // every service probed, one tick at a time
  gsap.to('.glyph-verify path', {
    strokeDashoffset: 0, duration: 0.5, ease: 'power2.out', stagger: 0.28,
    scrollTrigger: { trigger: '.glyph-verify', start: 'top 80%', once: true },
  });

  // ---- lists arrive row by row
  ScrollTrigger.batch('.feat-list > div, .compat-table tbody tr', {
    start: 'top 90%',
    once: true,
    onEnter: (batch) => gsap.fromTo(batch, { opacity: 0, y: 22 }, { opacity: 1, y: 0, duration: 1, ease: 'expo.out', stagger: 0.07 }),
  });
  gsap.set('.feat-list > div, .compat-table tbody tr', { opacity: 0 });

  // ---- code lines type in
  const code = $('.code code');
  const codeLines = code.innerHTML.split('\n').map((l) => `<span class="ln">${l || ' '}</span>`);
  code.innerHTML = codeLines.join('');
  gsap.fromTo($$('.ln', code), { opacity: 0, x: -12 }, {
    opacity: 1, x: 0, duration: 0.8, ease: 'expo.out', stagger: 0.07,
    scrollTrigger: { trigger: code, start: 'top 82%', once: true },
  });

  // ---- recordings drift against the scroll
  $$('[data-parallax]').forEach((el) => {
    gsap.fromTo(el, { y: 70 }, {
      y: -40, ease: 'none',
      scrollTrigger: { trigger: el, start: 'top bottom', end: 'bottom top', scrub: true },
    });
  });

  // ---- the second drop: install floods white from the top edge
  gsap.fromTo('.install-flood', { clipPath: 'circle(0% at 50% 0%)' }, {
    clipPath: 'circle(150% at 50% 0%)', ease: 'power2.in',
    scrollTrigger: { trigger: '.install', start: 'top 92%', end: 'top 8%', scrub: true },
  });

  addEventListener('load', () => ScrollTrigger.refresh());

  // test hook: jump to a scroll position (used by the screenshot script)
  window.__go = (y) => {
    if (lenis) lenis.scrollTo(y, { immediate: true, force: true }); else scrollTo(0, y);
    ScrollTrigger.update();
  };
}
