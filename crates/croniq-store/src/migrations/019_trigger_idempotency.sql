-- Caller-supplied dedup key for POST /v1/trigger (issue #279).
--
-- Event-driven producers operate under at-least-once semantics (event
-- redelivery, client retries, concurrent producers) and can fire the same
-- logical event more than once. The trigger endpoint stores the caller's
-- idempotency_key on the execution row so a repeat trigger with the same
-- (job_key, idempotency_key) can coalesce to the existing execution while
-- it is in-flight or still inside the configured dedup window.
--
-- NULL for scheduler-fired executions and for triggers without a key —
-- the vast majority of rows — hence the partial index below.

ALTER TABLE executions ADD COLUMN idempotency_key TEXT;

-- Partial composite index: the dedup lookup filters on
-- (job_key, idempotency_key) and only rows that actually carry a key can
-- ever match, so NULL rows are excluded to keep the index tiny.
CREATE INDEX IF NOT EXISTS idx_executions_job_key_idempotency_key
    ON executions(job_key, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
