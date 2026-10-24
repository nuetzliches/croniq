-- In-flight executions per job, for the per-job concurrency guard
-- (review 2026-10-24, S-3).
--
-- `count_executions_in_states(job_key, [claimed])` runs on every poll that
-- carries a `singleton` / `max_concurrent` item, under the queue write lock.
-- Before this index its only access path was `idx_executions_job_key`, which
-- walks the job's whole history: a job with 500k terminal rows cost a 500k-row
-- scan per poll, and retention is opt-in. The retention index of 021 only
-- holds terminal rows and the one of 029 only grouped rows.
--
-- Partial, so it stays as small as the queue rather than the history. SQLite
-- uses a partial index only when the query repeats its WHERE term verbatim,
-- so the store adds `state IN ('queued', 'claimed')` to the count whenever
-- every requested state lies inside it.
CREATE INDEX IF NOT EXISTS idx_executions_job_key_inflight
    ON executions(job_key, state)
    WHERE state IN ('queued', 'claimed');
