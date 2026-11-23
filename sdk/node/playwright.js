// SPDX-License-Identifier: MIT OR Apache-2.0
'use strict';
// Playwright: make the browser's clock follow the stack's virtual clock.
//
//   const { syncClock } = require('chronostack/playwright');
//   test.beforeEach(async ({ page }) => { await syncClock(page); });
//
// Uses Playwright's own clock API (page.clock / context.clock, Playwright 1.45+):
// `setSystemTime` while virtual time runs, `setFixedTime` while it is frozen.
// Like everywhere else in ChronoStack, only the wall clock changes: the page's
// timers (setTimeout, setInterval, requestAnimationFrame) keep real time.

const cs = require('./index.js');

async function apply(target) {
  const t = Math.floor(cs.now());
  if (cs.frozen) await target.clock.setFixedTime(t);
  else await target.clock.setSystemTime(t);
}

/**
 * Set the browser clock of a Playwright `Page` or `BrowserContext` to the
 * virtual time, and keep it in sync after every `advance`, `freeze` or
 * `unfreeze`. Returns a function that stops following (the browser keeps the
 * time it has). Without a running stack it changes nothing, returns a no-op,
 * and the browser keeps real time (with CHRONOSTACK_STRICT=1, it throws).
 */
async function syncClock(target) {
  if (!target || !target.clock || typeof target.clock.setSystemTime !== 'function') {
    throw new TypeError('syncClock needs a Playwright Page or BrowserContext (Playwright 1.45 or later)');
  }
  if (!cs.install()) return () => {};
  await apply(target);
  let pending = Promise.resolve();
  const off = cs.onAdvance(() => {
    // Serialise updates; a page that has closed just stops following.
    pending = pending.then(() => apply(target)).catch(() => {});
  });
  if (typeof target.once === 'function') target.once('close', off);
  return off;
}

module.exports = { syncClock };
