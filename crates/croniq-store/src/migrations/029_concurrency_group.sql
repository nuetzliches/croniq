-- Shared concurrency budget an execution draws on (issue #546).
--
-- Denormalised from the execution's own `__concurrency_group` metadata key by
-- the store's insert helper, not by its callers: `insert_execution_with` is the
-- single insert path behind `create_execution` and
-- `create_execution_and_advance_job_state`, so the scheduler fire, the manual
-- trigger, the retry chain, the MCP tools and the API paths all stamp it
-- without a line of their own.
--
-- Why a column and not a join against `dsl_jobs` at dispatch time: an
-- API-registered job whose metadata carries the group key would be *blocked
-- by* the group yet not *counted into* it, because `dsl_jobs` does not know
-- it. With the column, blocking and counting read the same per-execution
-- stamp.
--
-- Nullable, and NULL for every row written before this migration and for
-- every ungrouped job. The claim-path guard only ever counts rows whose group
-- matches a non-empty name, so NULL rows can never satisfy it.
ALTER TABLE executions ADD COLUMN concurrency_group TEXT DEFAULT NULL;

-- The guard's only query: count in-flight (`claimed`) rows of one group, on
-- every poll that carries a grouped item. Partial, like the retention indexes
-- of migration 021: ungrouped rows are the overwhelming majority and can
-- never match.
CREATE INDEX IF NOT EXISTS idx_executions_concurrency_group_state
    ON executions(concurrency_group, state)
    WHERE concurrency_group IS NOT NULL;
