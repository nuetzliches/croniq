-- Add covering indexes that the scheduler-restore and list-executions
-- paths need. With only the existing single-column indexes, SQLite walks
-- one of them and then builds a temporary B-tree to satisfy ORDER BY,
-- which gets expensive once the executions table grows past ~50k rows.
--
-- After this migration:
--   * find_queued_executions(...) walks idx_executions_state_fire_at in
--     order — no temp B-tree, no separate sort step.
--   * GET /v1/executions (ORDER BY created_at DESC LIMIT N) uses
--     idx_executions_created_at instead of full-scanning the table.
--
-- The old single-column idx_executions_state is dropped because the new
-- composite index covers any query that previously hit it (state-only
-- predicates use the leading column of the composite). idx_executions_fire_at
-- stays in case anyone filters purely on fire_at.

DROP INDEX IF EXISTS idx_executions_state;
CREATE INDEX IF NOT EXISTS idx_executions_state_fire_at
    ON executions(state, fire_at);

CREATE INDEX IF NOT EXISTS idx_executions_created_at
    ON executions(created_at);
