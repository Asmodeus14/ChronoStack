// SPDX-License-Identifier: MIT OR Apache-2.0
// The worker does two things.
//
// 1. Runs expire-trials every day at 03:00 UTC, with a typical in-process
//    scheduler: it computes "time until 03:00" from Date.now() and sleeps with
//    setTimeout. setTimeout runs on the monotonic clock, so when ChronoStack
//    moves the wall clock forward this timer does NOT fire early. That is why
//    chronostack.yml declares a trigger that runs the same job at each virtual
//    03:00.
//
// 2. Delivers queued webhooks (the `webhook_outbox` table). Every 500 ms it
//    asks Postgres which rows are due (`next_attempt_at <= now()`), so it reacts
//    to virtual time on its own. Failed deliveries are retried with exponential
//    backoff: 20 min, 40 min, 80 min, … and given up after 8 attempts.
const { expireTrials } = require('./jobs/expire-trials');
const { pool } = require('./db');

const WEBHOOK_URL = process.env.WEBHOOK_URL || 'http://receiver:4000/hooks';
// A deliberate regression for the ChronoStack demo (compose.bug.yml): treat any
// HTTP response, even a 503, as a successful delivery.
const BUG_IGNORE_STATUS = process.env.WEBHOOK_BUG === 'ignore-status';
const MAX_ATTEMPTS = 8;

function msUntilNext0300() {
  const now = new Date();
  const next = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), 3, 0, 0));
  if (next <= now) next.setUTCDate(next.getUTCDate() + 1);
  return next - now;
}

function scheduleExpiry() {
  const ms = msUntilNext0300();
  console.log(`worker: next expire-trials run in ${Math.round(ms / 60000)} min (by its own timer)`);
  setTimeout(async () => {
    try {
      console.log(`worker: expired ${await expireTrials()} trial(s)`);
    } catch (e) {
      console.error('worker: job failed', e);
    }
    scheduleExpiry();
  }, ms);
}

async function deliverDue() {
  const { rows } = await pool.query(
    `SELECT id, event_id, type, payload FROM webhook_outbox
      WHERE delivered_at IS NULL AND failed_at IS NULL AND next_attempt_at <= now()
      ORDER BY id LIMIT 10`,
  );
  for (const row of rows) {
    let error = null;
    try {
      const res = await fetch(WEBHOOK_URL, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'idempotency-key': row.event_id },
        body: JSON.stringify({ id: row.event_id, type: row.type, data: row.payload }),
        signal: AbortSignal.timeout(5000),
      });
      if (!res.ok && !BUG_IGNORE_STATUS) error = `HTTP ${res.status}`;
    } catch (e) {
      error = e.message;
    }
    if (error === null) {
      await pool.query(
        'UPDATE webhook_outbox SET attempts = attempts + 1, delivered_at = now(), last_error = NULL WHERE id = $1',
        [row.id],
      );
      console.log(`worker: delivered ${row.event_id}`);
    } else {
      await pool.query(
        `UPDATE webhook_outbox
            SET attempts = attempts + 1,
                last_error = $2,
                next_attempt_at = now() + interval '20 minutes' * power(2, attempts),
                failed_at = CASE WHEN attempts + 1 >= $3 THEN now() END
          WHERE id = $1`,
        [row.id, error, MAX_ATTEMPTS],
      );
      console.log(`worker: delivery of ${row.event_id} failed (${error}); will retry`);
    }
  }
}

function deliveryLoop() {
  let running = false;
  setInterval(async () => {
    if (running) return;
    running = true;
    try {
      await deliverDue();
    } catch (e) {
      console.error('worker: delivery loop error', e.message);
    } finally {
      running = false;
    }
  }, 500);
}

scheduleExpiry();
deliveryLoop();
