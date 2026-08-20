-- Index dead_letters(execution_id) for the retention reachability probe
-- (issue #485).
--
-- Retention learned to reach `dead` executions that nothing references
-- (issue #470) by adding `OR NOT EXISTS (SELECT 1 FROM dead_letters dl WHERE
-- dl.execution_id = e.id)` to both prune paths. That correlated subquery had
-- no index to stand on: `dead_letters` was indexed by `job_key` and
-- `expires_at` only, never by the column the probe joins on. Every candidate
-- execution therefore cost a scan of `dead_letters`, on every 30 s watchdog
-- tick, and the cost grows with the dead-letter backlog — worst exactly where
-- retention matters most.
--
-- Not partial: unlike the `completed_at` indexes of migration 021 there is no
-- subset to restrict to. `execution_id` is NOT NULL and every row is a
-- possible match for the probe.

CREATE INDEX IF NOT EXISTS idx_dead_letters_execution_id
    ON dead_letters(execution_id);
