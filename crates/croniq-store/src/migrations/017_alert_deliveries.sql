-- Alert delivery log (issue #140, PR-1).
--
-- One row per (rule, channel, fire-event). The evaluator inserts a row
-- the moment it decides to fire (or skip due to throttle); the channel
-- handler updates the row on completion.
--
-- State values:
--   delivered  — channel handler returned success
--   failed     — channel handler returned an error (error column set)
--   throttled  — rule matched but the per-(rule, job_key) throttle
--                window suppressed the fire. We still record it so
--                operators can see what would have fired without the
--                throttle.
--
-- Retention is not enforced by the DB — operators can prune via the
-- future admin endpoint (PR-2/3) or by direct DELETE. Keeping ~30
-- days on a 10-job-per-day install is well under a megabyte.

CREATE TABLE IF NOT EXISTS alert_deliveries (
    delivery_id   TEXT PRIMARY KEY,
    rule_name     TEXT NOT NULL,
    channel_name  TEXT NOT NULL,
    job_key       TEXT NOT NULL,
    execution_id  TEXT,
    state         TEXT NOT NULL,
    error         TEXT,
    fired_at      TEXT NOT NULL,
    delivered_at  TEXT
);

CREATE INDEX IF NOT EXISTS idx_alert_deliveries_job_key ON alert_deliveries(job_key);
CREATE INDEX IF NOT EXISTS idx_alert_deliveries_fired_at ON alert_deliveries(fired_at);
CREATE INDEX IF NOT EXISTS idx_alert_deliveries_rule_name ON alert_deliveries(rule_name);
