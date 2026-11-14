// SPDX-License-Identifier: MIT OR Apache-2.0
// Daily job: downgrade every user whose trial has ended, and queue a
// `trial.expired` webhook for each one (delivered by the worker). Uses the
// database's now().
const { pool } = require('../db');

async function expireTrials() {
  const res = await pool.query(
    `WITH expired AS (
       UPDATE users SET plan = 'free', downgraded_at = now()
        WHERE plan = 'trial' AND trial_ends_at <= now()
        RETURNING id, email
     )
     INSERT INTO webhook_outbox (event_id, type, payload)
     SELECT 'trial.expired:' || id, 'trial.expired', json_build_object('user_id', id, 'email', email)
       FROM expired
     RETURNING id`,
  );
  await pool.query('INSERT INTO job_runs (name, affected) VALUES ($1, $2)', ['expire-trials', res.rowCount]);
  return res.rowCount;
}

module.exports = { expireTrials };

// Run directly: `node src/jobs/expire-trials.js` (this is what the ChronoStack trigger calls).
if (require.main === module) {
  expireTrials()
    .then((n) => {
      console.log(JSON.stringify({ job: 'expire-trials', expired: n, at: new Date().toISOString() }));
      return pool.end();
    })
    .catch((e) => {
      console.error(e);
      process.exit(1);
    });
}
