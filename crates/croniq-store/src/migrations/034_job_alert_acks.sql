-- Per-job alert acknowledgements: "we know this job is failing, and we are
-- waiting it out". While a row exists and has not run out, the job's alerts
-- are recorded as `acknowledged` in alert_deliveries instead of being sent.
--
-- One row per job, keyed by job key. It ends by itself: the job's first
-- successful run deletes it (and sends a recovery), and the watchdog deletes
-- it at expires_at (and alerts that the job has still not recovered). That is
-- the difference to a rule snooze in alert_rule_overrides, which silences
-- every job a rule matches until a time someone guessed.
--
--   note                mandatory, why the job was acknowledged
--   acked_by            user_id / api_client_id
--   acked_by_name       display name at that moment, kept for when the user
--                       is renamed or removed
--   expires_at          always set; the API caps it
--   suppressed_count    failed runs whose alerts were held back, counted per
--                       execution rather than per rule
--   last_execution_id   the run held back last, its error, and when
--
-- `kind` on alert_deliveries tells a recovery from an alert. A recovery does
-- not count as a fire for the throttle, and the throttle seed at boot skips
-- the alerts that came before one, so a failure after a recovery alerts at
-- once instead of waiting out the old incident's window.

CREATE TABLE IF NOT EXISTS job_alert_acks (
    job_key             TEXT PRIMARY KEY,
    note                TEXT NOT NULL,
    acked_by            TEXT NOT NULL,
    acked_by_name       TEXT NOT NULL,
    acked_at            TEXT NOT NULL,
    expires_at          TEXT NOT NULL,
    suppressed_count    INTEGER NOT NULL DEFAULT 0,
    last_execution_id   TEXT,
    last_error          TEXT,
    last_suppressed_at  TEXT
);

CREATE INDEX IF NOT EXISTS idx_job_alert_acks_expires_at
    ON job_alert_acks(expires_at);

ALTER TABLE alert_deliveries ADD COLUMN kind TEXT NOT NULL DEFAULT 'alert';
