-- Job-level execution policy fields.
--
-- These allow API-created jobs to carry their own timeout, retry cap, and
-- dead-letter preference without needing a DSL (Croniqfile) entry. All
-- columns are nullable: NULL means "use the system default" (5 m / 3 attempts
-- / dead-letter enabled).

ALTER TABLE job_definitions ADD COLUMN timeout            TEXT    DEFAULT NULL;
ALTER TABLE job_definitions ADD COLUMN max_retries        INTEGER DEFAULT NULL;
ALTER TABLE job_definitions ADD COLUMN dead_letter_enabled INTEGER DEFAULT NULL;
