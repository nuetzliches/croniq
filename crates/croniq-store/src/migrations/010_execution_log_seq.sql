-- Add a per-execution monotonic sequence number to execution_logs so the
-- per-line log stream (#108) preserves the order events were captured even
-- when many events share the same millisecond timestamp.
--
-- Existing rows get seq = 0 by default — they predate per-line emission
-- and consist of at most one stdout blob plus one stderr blob, so the
-- (timestamp, seq) order falls back to timestamp-only and matches the
-- old behaviour. New per-line inserts assign seq = MAX(seq) + 1 within
-- the execution.

ALTER TABLE execution_logs ADD COLUMN seq INTEGER NOT NULL DEFAULT 0;

-- Composite index on (execution_id, seq) — read_logs orders by
-- (timestamp, seq) but the WHERE clause is execution_id-only, so the
-- planner uses this index for both filter and the secondary sort.
CREATE INDEX IF NOT EXISTS idx_execution_logs_execution_seq
    ON execution_logs(execution_id, seq);
