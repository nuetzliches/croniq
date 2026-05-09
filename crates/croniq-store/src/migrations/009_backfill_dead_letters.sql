-- Backfill dead_letters rows for orphan executions in state='dead'.
--
-- Before #104's fix, the completion processor wrote `state='dead'` and the
-- matching `dead_letters` row in two non-transactional store calls and
-- swallowed the second call's error. Some deployments therefore accumulated
-- `state='dead'` rows in the executions table with no corresponding
-- `dead_letters` row, leaving the Dead Letters UI page empty even though
-- actionable failures existed.
--
-- This migration populates dead_letters for those orphans so they show up
-- in the UI as soon as the operator updates. We deliberately set
-- `expires_at = NULL` (no TTL) instead of guessing a retention — a future
-- run's purge sweep will not delete these rows on its own, and an operator
-- who wants them gone can use DELETE /v1/dead-letters/{id}.
--
-- ID generation uses SQLite's randomblob in the standard 8-4-4-4-12 UUID
-- text format. Uuid::parse_str at the read path accepts any 32 hex chars
-- in this layout regardless of version/variant bits.

INSERT INTO dead_letters (
    id,
    execution_id,
    job_key,
    fire_at,
    attempt,
    error,
    dead_reason,
    metadata,
    created_at,
    expires_at
)
SELECT
    lower(
        substr(hex(randomblob(4)), 1, 8) || '-' ||
        substr(hex(randomblob(2)), 1, 4) || '-' ||
        substr(hex(randomblob(2)), 1, 4) || '-' ||
        substr(hex(randomblob(2)), 1, 4) || '-' ||
        substr(hex(randomblob(6)), 1, 12)
    ),
    e.id,
    e.job_key,
    e.fire_at,
    e.attempt,
    COALESCE(e.error, ''),
    COALESCE(e.dead_reason, 'recovered from orphaned dead state'),
    e.metadata,
    COALESCE(e.completed_at, e.created_at),
    NULL
FROM executions e
WHERE e.state = 'dead'
  AND e.id NOT IN (SELECT execution_id FROM dead_letters);
