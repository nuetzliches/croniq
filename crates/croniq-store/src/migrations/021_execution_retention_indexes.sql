-- Execution retention (issue #344).
--
-- Both prune paths — the global `server { execution_retention }` age sweep
-- and per-job `keep_last` caps — select terminal executions by
-- `completed_at` (queued/claimed rows have NULL completed_at and are
-- excluded). Without an index the 30 s watchdog would full-scan the
-- executions table every tick. Partial indexes keep the entries limited to
-- terminal rows.
--
-- `idx_executions_completed_at` serves the age sweep
-- (`WHERE completed_at <= cutoff`); `idx_executions_job_key_completed_at`
-- serves per-job keep_last (`WHERE job_key = ? ORDER BY completed_at`).

CREATE INDEX IF NOT EXISTS idx_executions_completed_at
    ON executions(completed_at)
    WHERE completed_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_executions_job_key_completed_at
    ON executions(job_key, completed_at)
    WHERE completed_at IS NOT NULL;
