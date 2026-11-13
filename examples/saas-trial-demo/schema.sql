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
