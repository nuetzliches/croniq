-- Per-job dead-letter policy fields for API-registered jobs (parity with
-- the DSL `dead_letter { … }` block; follows the migration-004 pattern for
-- `dead_letter_enabled`). All columns are nullable: NULL means "use the
-- system default" (retention 30d, no operator hint, no stale-replay guard).
--
-- `dead_letter_replay_max_age` is the opt-in stale-replay guard (PR #359):
-- a duration string ("7d", "12h"); replaying a dead letter whose original
-- `scheduled_for` is older than this is rejected with 409 unless forced.

ALTER TABLE job_definitions ADD COLUMN dead_letter_retention      TEXT DEFAULT NULL;
ALTER TABLE job_definitions ADD COLUMN dead_letter_operator_hint  TEXT DEFAULT NULL;
ALTER TABLE job_definitions ADD COLUMN dead_letter_replay_max_age TEXT DEFAULT NULL;
