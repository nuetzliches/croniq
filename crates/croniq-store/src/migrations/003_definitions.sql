-- Job definitions, trigger definitions, calendar definitions, execution logs

CREATE TABLE job_definitions (
    job_key      TEXT PRIMARY KEY,
    description  TEXT,
    assigned_runner_id TEXT,
    is_active    INTEGER NOT NULL DEFAULT 1,
    metadata     TEXT NOT NULL DEFAULT '{}',
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE TABLE trigger_definitions (
    trigger_id       TEXT PRIMARY KEY,
    job_key          TEXT NOT NULL,
    cron_expression  TEXT,
    timezone         TEXT,
    calendar         TEXT,
    window           TEXT,
    not_before       TEXT,
    not_after        TEXT,
    enabled          INTEGER NOT NULL DEFAULT 1,
    managed_by       TEXT NOT NULL DEFAULT 'api',
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

CREATE INDEX idx_trigger_definitions_job_key ON trigger_definitions(job_key);

CREATE TABLE calendar_definitions (
    calendar_id  TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    timezone     TEXT,
    -- Line-separated Croniqfile DSL text (one include/exclude directive per
    -- line), not JSON. The default is unused — every INSERT supplies rules
    -- explicitly — but '' is the only valid empty DSL; '[]' was a leftover
    -- from when rules were a JSON array. Editing this applied migration only
    -- affects fresh DBs; existing rows keep their real (explicit) values.
    rules        TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE TABLE execution_logs (
    id            TEXT PRIMARY KEY,
    execution_id  TEXT NOT NULL,
    timestamp     TEXT NOT NULL,
    level         TEXT NOT NULL DEFAULT 'info',
    message       TEXT NOT NULL,
    fields        TEXT NOT NULL DEFAULT '{}',
    FOREIGN KEY (execution_id) REFERENCES executions(id)
);

CREATE INDEX idx_execution_logs_execution_id ON execution_logs(execution_id);
CREATE INDEX idx_execution_logs_timestamp ON execution_logs(timestamp);
