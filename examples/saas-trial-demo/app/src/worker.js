// SPDX-License-Identifier: MIT OR Apache-2.0
// A typical in-process scheduler: run expire-trials every day at 03:00 UTC.
//
// It computes "time until 03:00" from Date.now() and sleeps with setTimeout.
// setTimeout runs on the monotonic clock, so when ChronoStack moves the wall
// clock forward this timer does NOT fire early — exactly the behaviour ChronoStack
// documents. That is why chronostack.yml declares a trigger that runs the same
// job at each virtual 03:00.
const { expireTrials } = require('./jobs/expire-trials');

function msUntilNext0300() {
  const now = new Date();
  const next = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), 3, 0, 0));
  if (next <= now) next.setUTCDate(next.getUTCDate() + 1);
  return next - now;
}

function schedule() {
  const ms = msUntilNext0300();
  console.log(`worker: next expire-trials run in ${Math.round(ms / 60000)} min (by its own timer)`);
  setTimeout(async () => {
    try {
      console.log(`worker: expired ${await expireTrials()} trial(s)`);
    } catch (e) {
      console.error('worker: job failed', e);
    }
    schedule();
  }, ms);
}

schedule();
