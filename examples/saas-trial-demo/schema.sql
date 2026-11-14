-- SPDX-License-Identifier: MIT OR Apache-2.0
CREATE TABLE users (
  id            serial PRIMARY KEY,
  email         text UNIQUE NOT NULL,
  plan          text NOT NULL DEFAULT 'trial',          -- trial | free
  created_at    timestamptz NOT NULL DEFAULT now(),
  trial_ends_at timestamptz NOT NULL DEFAULT now() + interval '14 days',
  downgraded_at timestamptz
);

CREATE TABLE job_runs (
  id       serial PRIMARY KEY,
  name     text NOT NULL,
  ran_at   timestamptz NOT NULL DEFAULT now(),
  affected integer NOT NULL
);

-- Outgoing webhooks (transactional outbox). The worker delivers due rows and
-- retries failures with exponential backoff: 20 min, 40 min, 80 min, …
CREATE TABLE webhook_outbox (
  id              serial PRIMARY KEY,
  event_id        text UNIQUE NOT NULL,        -- also sent as Idempotency-Key
  type            text NOT NULL,
  payload         jsonb NOT NULL,
  created_at      timestamptz NOT NULL DEFAULT now(),
  attempts        integer NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  last_error      text,
  delivered_at    timestamptz,
  failed_at       timestamptz                  -- gave up after 8 attempts
);
