-- How many runs of a job in a row have failed for good: exhausted their
-- retries and were dead-lettered or dropped. A run that fails and then
-- succeeds on retry is not counted; it never failed for good.
--
-- One row per job while it is failing. A permanent failure creates or bumps
-- it; the job's next successful run deletes it, and that deletion is the
-- moment a `job_recovered` rule fires. No row means the job's latest finished
-- run did not fail for good.
--
--   consecutive        permanent failures in a row
--   since              when the first of them happened
--   last_execution_id  the latest of them, its error, and when
--
-- What `min_consecutive` on a rule compares against: a `job_failed` rule
-- fires from the Nth failure in a row on, a `job_recovered` rule announces
-- the end of a streak at least N long. Kept in the store rather than in
-- memory so a restart in the middle of an outage neither restarts the count
-- nor forgets that a recovery is owed.

CREATE TABLE IF NOT EXISTS job_failure_streaks (
    job_key            TEXT PRIMARY KEY,
    consecutive        INTEGER NOT NULL,
    since              TEXT NOT NULL,
    last_execution_id  TEXT NOT NULL,
    last_error         TEXT NOT NULL,
    last_failed_at     TEXT NOT NULL
);
