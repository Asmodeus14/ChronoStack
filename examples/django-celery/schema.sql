-- SPDX-License-Identifier: MIT OR Apache-2.0
CREATE TABLE task_runs (
  id     serial PRIMARY KEY,
  task   text NOT NULL,
  ran_at timestamptz NOT NULL DEFAULT now()
);
