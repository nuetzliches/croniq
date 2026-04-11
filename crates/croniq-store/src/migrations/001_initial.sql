-- Croniq initial schema

CREATE TABLE job_states (
    job_key     TEXT PRIMARY KEY,
    next_fire_at TEXT,
    last_fired_at TEXT,
    fire_count  INTEGER NOT NULL DEFAULT 0,
    status      TEXT NOT NULL DEFAULT 'active',
    updated_at  TEXT NOT NULL
);

CREATE TABLE executions (
    id           TEXT PRIMARY KEY,
    job_key      TEXT NOT NULL,
    fire_at      TEXT NOT NULL,
    attempt      INTEGER NOT NULL DEFAULT 1,
    state        TEXT NOT NULL DEFAULT 'queued',
    runner_id    TEXT,
    claimed_at   TEXT,
    started_at   TEXT,
    completed_at TEXT,
    duration_ms  INTEGER,
    error        TEXT,
    dead_reason  TEXT,
    metadata     TEXT NOT NULL DEFAULT '{}',
    created_at   TEXT NOT NULL
);

CREATE INDEX idx_executions_state ON executions(state);
CREATE INDEX idx_executions_job_key ON executions(job_key);
CREATE INDEX idx_executions_fire_at ON executions(fire_at);
CREATE INDEX idx_executions_runner_id ON executions(runner_id);

CREATE TABLE runners (
    runner_id     TEXT PRIMARY KEY,
    capabilities  TEXT NOT NULL DEFAULT '[]',
    max_inflight  INTEGER NOT NULL DEFAULT 1,
    last_poll_at  TEXT NOT NULL,
    inflight      TEXT NOT NULL DEFAULT '[]',
    status        TEXT NOT NULL DEFAULT 'online',
    registered_at TEXT NOT NULL
);

CREATE TABLE dead_letters (
    id            TEXT PRIMARY KEY,
    execution_id  TEXT NOT NULL,
    job_key       TEXT NOT NULL,
    fire_at       TEXT NOT NULL,
    attempt       INTEGER NOT NULL,
    error         TEXT NOT NULL,
    dead_reason   TEXT NOT NULL,
    metadata      TEXT NOT NULL DEFAULT '{}',
    created_at    TEXT NOT NULL,
    expires_at    TEXT
);

CREATE INDEX idx_dead_letters_job_key ON dead_letters(job_key);
CREATE INDEX idx_dead_letters_expires_at ON dead_letters(expires_at);
