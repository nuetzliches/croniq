-- The trigger's original logical fire time, constant across the retry
-- chain and across dead-letter replay (fire_at tracks queue due time and
-- gets reset on retry/replay). Nullable: rows written by older binaries
-- fall back to fire_at in the row mappers.
ALTER TABLE executions ADD COLUMN scheduled_for TEXT DEFAULT NULL;
UPDATE executions SET scheduled_for = fire_at WHERE scheduled_for IS NULL;

ALTER TABLE dead_letters ADD COLUMN scheduled_for TEXT DEFAULT NULL;
UPDATE dead_letters SET scheduled_for = fire_at WHERE scheduled_for IS NULL;
