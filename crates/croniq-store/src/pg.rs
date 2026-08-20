//! PostgreSQL store implementation.
//!
//! Requires the `postgres` feature. Uses the synchronous `postgres` crate
//! to match the synchronous store trait signatures.

use crate::models::*;
use crate::retention_sql::DELETABLE_EXECUTION;
use crate::traits::*;
use chrono::{DateTime, Utc};
use postgres::{Client, NoTls};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// PostgreSQL-backed store.
pub struct PgStore {
    client: Mutex<Client>,
}

impl PgStore {
    /// Connect to a PostgreSQL database.
    ///
    /// TLS is negotiated according to [`crate::pg_tls`]: demanded by default
    /// for a remote host, preferred for loopback and unix sockets, and
    /// overridable through `sslmode=` in the connection string or
    /// `CRONIQ_PG_SSLMODE`. Before #431 this was unconditionally `NoTls`.
    pub fn connect(connection_string: &str) -> Result<Self, StoreError> {
        let client = Self::connect_client(connection_string)?;
        let store = Self {
            client: Mutex::new(client),
        };
        store.migrate()?;
        Ok(store)
    }

    fn connect_client(connection_string: &str) -> Result<Client, StoreError> {
        let mut config: postgres::Config = connection_string.parse().map_err(|e| {
            StoreError::Database(format!("invalid Postgres connection string: {e}"))
        })?;
        let mode = crate::pg_tls::resolve_ssl_mode(connection_string, &config)?;

        match mode {
            crate::pg_tls::SslMode::Disable => {
                config.ssl_mode(postgres::config::SslMode::Disable);
                config.connect(NoTls)
            }
            crate::pg_tls::SslMode::Prefer => {
                config.ssl_mode(postgres::config::SslMode::Prefer);
                config.connect(crate::pg_tls::make_connector()?)
            }
            crate::pg_tls::SslMode::Require => {
                config.ssl_mode(postgres::config::SslMode::Require);
                config.connect(crate::pg_tls::make_connector()?)
            }
        }
        .map_err(|e| {
            // A handshake failure here is the single most likely upgrade
            // symptom, so say what changed rather than surfacing a bare
            // driver error.
            StoreError::Database(format!(
                "Postgres connection failed with sslmode={mode:?}: {e}. \
                 If this database has no TLS, set CRONIQ_PG_SSLMODE=prefer (or \
                 sslmode=disable in the connection string). If it has a private \
                 CA, point CRONIQ_PG_ROOT_CERT at its PEM bundle."
            ))
        })
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.batch_execute(
            "CREATE TABLE IF NOT EXISTS _migrations (
                    name TEXT PRIMARY KEY,
                    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
                );",
        )
        .map_err(map_err)?;

        let applied: Vec<String> = db
            .query("SELECT name FROM _migrations", &[])
            .map_err(map_err)?
            .iter()
            .map(|row| row.get(0))
            .collect();

        // Numbers mirror the SQLite migration set (see
        // `migrations/mod.rs`). Postgres has no existing deployments to
        // upgrade, so each table is created in its current/final shape
        // (later SQLite ALTERs are folded into the CREATE) rather than
        // replayed incrementally. Every statement is `IF NOT EXISTS`-guarded
        // so re-running is a no-op. Order matters for the FK chain
        // (api_clients → api_keys, users → invitations/resets/totp/pat/oidc).
        for &(name, sql) in PG_MIGRATIONS {
            if applied.iter().any(|a| a == name) {
                continue;
            }
            db.batch_execute(sql).map_err(map_err)?;
            db.execute("INSERT INTO _migrations (name) VALUES ($1)", &[&name])
                .map_err(map_err)?;
        }

        Ok(())
    }
}

/// Embedded Postgres migrations, applied in listed order. Kept in numeric
/// lock-step with the SQLite set so the two backends stay behaviourally
/// equivalent.
const PG_MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial", PG_MIGRATION_001),
    ("002_auth", PG_MIGRATION_002),
    ("003_definitions", PG_MIGRATION_003),
    ("005_perf_indexes", PG_MIGRATION_005),
    ("007_dsl_adoptions", PG_MIGRATION_007),
    ("011_users", PG_MIGRATION_011),
    ("012_invitations_and_resets", PG_MIGRATION_012),
    ("013_totp_and_recovery", PG_MIGRATION_013),
    ("014_personal_access_tokens", PG_MIGRATION_014),
    ("015_oidc", PG_MIGRATION_015),
    ("016_audit_log", PG_MIGRATION_016),
    ("017_alert_deliveries", PG_MIGRATION_017),
    ("018_alert_rule_overrides", PG_MIGRATION_018),
    ("019_trigger_idempotency", PG_MIGRATION_019),
    ("020_maintenance", PG_MIGRATION_020),
    ("021_execution_retention_indexes", PG_MIGRATION_021),
    ("022_scheduled_for", PG_MIGRATION_022),
    ("023_dead_letter_policy", PG_MIGRATION_023),
    ("024_runner_identities", PG_MIGRATION_024),
    ("025_token_generation", PG_MIGRATION_025),
    ("026_api_client_managed_by", PG_MIGRATION_026),
    ("027_dead_letter_execution_index", PG_MIGRATION_027),
];

const PG_MIGRATION_001: &str = r#"
CREATE TABLE IF NOT EXISTS job_states (
    job_key     TEXT PRIMARY KEY,
    next_fire_at TIMESTAMPTZ,
    last_fired_at TIMESTAMPTZ,
    fire_count  BIGINT NOT NULL DEFAULT 0,
    status      TEXT NOT NULL DEFAULT 'active',
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS executions (
    id           UUID PRIMARY KEY,
    job_key      TEXT NOT NULL,
    fire_at      TIMESTAMPTZ NOT NULL,
    attempt      INTEGER NOT NULL DEFAULT 1,
    state        TEXT NOT NULL DEFAULT 'queued',
    runner_id    TEXT,
    claimed_at   TIMESTAMPTZ,
    started_at   TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    duration_ms  BIGINT,
    error        TEXT,
    dead_reason  TEXT,
    metadata     JSONB NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_executions_state ON executions(state);
CREATE INDEX IF NOT EXISTS idx_executions_job_key ON executions(job_key);
CREATE INDEX IF NOT EXISTS idx_executions_fire_at ON executions(fire_at);
CREATE INDEX IF NOT EXISTS idx_executions_runner_id ON executions(runner_id);

CREATE TABLE IF NOT EXISTS runners (
    runner_id     TEXT PRIMARY KEY,
    capabilities  JSONB NOT NULL DEFAULT '[]',
    max_inflight  INTEGER NOT NULL DEFAULT 1,
    last_poll_at  TIMESTAMPTZ NOT NULL,
    inflight      JSONB NOT NULL DEFAULT '[]',
    status        TEXT NOT NULL DEFAULT 'online',
    registered_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS dead_letters (
    id            UUID PRIMARY KEY,
    execution_id  UUID NOT NULL,
    job_key       TEXT NOT NULL,
    fire_at       TIMESTAMPTZ NOT NULL,
    attempt       INTEGER NOT NULL,
    error         TEXT NOT NULL,
    dead_reason   TEXT NOT NULL,
    metadata      JSONB NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL,
    expires_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_dead_letters_job_key ON dead_letters(job_key);
CREATE INDEX IF NOT EXISTS idx_dead_letters_expires_at ON dead_letters(expires_at);
"#;

const PG_MIGRATION_005: &str = r#"
DROP INDEX IF EXISTS idx_executions_state;
CREATE INDEX IF NOT EXISTS idx_executions_state_fire_at
    ON executions(state, fire_at);
CREATE INDEX IF NOT EXISTS idx_executions_created_at
    ON executions(created_at);
"#;

const PG_MIGRATION_019: &str = r#"
ALTER TABLE executions ADD COLUMN IF NOT EXISTS idempotency_key TEXT;
CREATE INDEX IF NOT EXISTS idx_executions_job_key_idempotency_key
    ON executions(job_key, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
"#;

// Execution retention (issue #344). Partial indexes on the terminal
// timestamp back the age sweep and per-job keep_last prune. Mirrors
// migrations/021_execution_retention_indexes.sql.
const PG_MIGRATION_021: &str = r#"
CREATE INDEX IF NOT EXISTS idx_executions_completed_at
    ON executions(completed_at)
    WHERE completed_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_executions_job_key_completed_at
    ON executions(job_key, completed_at)
    WHERE completed_at IS NOT NULL;
"#;

// Logical fire time carried unchanged through the retry chain and replay
// (fire_at is reset on retry/replay). Nullable; row mappers fall back to
// fire_at. Mirrors migrations/022_scheduled_for.sql.
const PG_MIGRATION_022: &str = r#"
ALTER TABLE executions ADD COLUMN IF NOT EXISTS scheduled_for TIMESTAMPTZ;
UPDATE executions SET scheduled_for = fire_at WHERE scheduled_for IS NULL;
ALTER TABLE dead_letters ADD COLUMN IF NOT EXISTS scheduled_for TIMESTAMPTZ;
UPDATE dead_letters SET scheduled_for = fire_at WHERE scheduled_for IS NULL;
"#;

// Per-job dead-letter policy for API-registered jobs (parity with the DSL
// `dead_letter { … }` block). NULL = system default (retention 30d, no hint,
// no stale-replay guard). Mirrors migrations/023_dead_letter_policy.sql.
const PG_MIGRATION_023: &str = r#"
ALTER TABLE job_definitions ADD COLUMN IF NOT EXISTS dead_letter_retention      TEXT;
ALTER TABLE job_definitions ADD COLUMN IF NOT EXISTS dead_letter_operator_hint  TEXT;
ALTER TABLE job_definitions ADD COLUMN IF NOT EXISTS dead_letter_replay_max_age TEXT;
"#;

// Binds a work-protocol `runner_id` to the credential that first claimed it,
// so the work handlers can refuse requests that name someone else's runner.
// See `migrations/024_runner_identities.sql` for the full rationale.
const PG_MIGRATION_024: &str = r#"
CREATE TABLE IF NOT EXISTS runner_identities (
    runner_id TEXT PRIMARY KEY,
    owner_id  TEXT NOT NULL,
    bound_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runner_identities_owner
    ON runner_identities(owner_id);
"#;

// Credential generation counter, stamped into every access token and checked
// on each authenticated request so a password change / reset / deactivation
// invalidates tokens already issued.
// See `migrations/025_token_generation.sql` for the full rationale.
const PG_MIGRATION_025: &str = r#"
ALTER TABLE users ADD COLUMN IF NOT EXISTS token_generation BIGINT NOT NULL DEFAULT 0;
"#;

// Ownership marker on api_clients: 'env' rows are declared by
// CRONIQ_API_CLIENT_<NAME>_KEY and owned by the environment, 'api' rows by
// whoever created them through the API. See
// `migrations/026_api_client_managed_by.sql`.
const PG_MIGRATION_026: &str = r#"
ALTER TABLE api_clients ADD COLUMN IF NOT EXISTS managed_by TEXT NOT NULL DEFAULT 'api';
"#;

// Index for the retention reachability probe added by #470 — `dead_letters`
// was indexed by job_key and expires_at only, never by the column
// `NOT EXISTS (… WHERE dl.execution_id = e.id)` joins on. See
// `migrations/027_dead_letter_execution_index.sql`.
const PG_MIGRATION_027: &str = r#"
CREATE INDEX IF NOT EXISTS idx_dead_letters_execution_id ON dead_letters(execution_id);
"#;

const PG_MIGRATION_002: &str = r#"
CREATE TABLE IF NOT EXISTS api_clients (
    client_id   TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    scopes      TEXT NOT NULL DEFAULT '[]',
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
    key_id      TEXT PRIMARY KEY,
    client_id   TEXT NOT NULL REFERENCES api_clients(client_id),
    key_hash    TEXT NOT NULL,
    key_prefix  TEXT NOT NULL,
    expires_at  TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_client_id ON api_keys(client_id);

CREATE TABLE IF NOT EXISTS password_credentials (
    user_id         TEXT PRIMARY KEY,
    username        TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_password_credentials_username
    ON password_credentials(username);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    token_hash  TEXT PRIMARY KEY,
    client_id   TEXT NOT NULL,
    user_id     TEXT,
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_client_id ON refresh_tokens(client_id);
"#;

// Definitions in final shape: the job-policy columns (SQLite 004), calendar
// `managed_by` (006), job `tags` (008) and execution-log `seq` (010) are
// folded straight into the CREATE. `window` is a reserved word in Postgres,
// so it is double-quoted here and in every statement that touches the column.
const PG_MIGRATION_003: &str = r#"
CREATE TABLE IF NOT EXISTS job_definitions (
    job_key             TEXT PRIMARY KEY,
    description         TEXT,
    assigned_runner_id  TEXT,
    is_active           BOOLEAN NOT NULL DEFAULT TRUE,
    metadata            TEXT NOT NULL DEFAULT '{}',
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    timeout             TEXT,
    max_retries         INTEGER,
    dead_letter_enabled BOOLEAN,
    tags                TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS trigger_definitions (
    trigger_id       TEXT PRIMARY KEY,
    job_key          TEXT NOT NULL,
    cron_expression  TEXT,
    timezone         TEXT,
    calendar         TEXT,
    "window"         TEXT,
    not_before       TIMESTAMPTZ,
    not_after        TIMESTAMPTZ,
    enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    managed_by       TEXT NOT NULL DEFAULT 'api',
    created_at       TIMESTAMPTZ NOT NULL,
    updated_at       TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_trigger_definitions_job_key
    ON trigger_definitions(job_key);

CREATE TABLE IF NOT EXISTS calendar_definitions (
    calendar_id  TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    timezone     TEXT,
    -- Line-separated Croniqfile DSL text (one include/exclude directive per
    -- line), not JSON. The default is unused — every INSERT supplies rules
    -- explicitly — but '' is the only valid empty DSL; '[]' was a leftover
    -- from when rules were a JSON array.
    rules        TEXT NOT NULL DEFAULT '',
    managed_by   TEXT NOT NULL DEFAULT 'api',
    created_at   TIMESTAMPTZ NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS execution_logs (
    id            UUID PRIMARY KEY,
    execution_id  UUID NOT NULL,
    timestamp     TIMESTAMPTZ NOT NULL,
    level         TEXT NOT NULL DEFAULT 'info',
    message       TEXT NOT NULL,
    fields        TEXT NOT NULL DEFAULT '{}',
    seq           BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_execution_logs_execution_id
    ON execution_logs(execution_id);
CREATE INDEX IF NOT EXISTS idx_execution_logs_timestamp
    ON execution_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_execution_logs_execution_seq
    ON execution_logs(execution_id, seq);
"#;

const PG_MIGRATION_007: &str = r#"
CREATE TABLE IF NOT EXISTS dsl_adoptions (
    resource_type TEXT NOT NULL,
    resource_key  TEXT NOT NULL,
    adopted_at    TIMESTAMPTZ NOT NULL,
    adopted_by    TEXT,
    PRIMARY KEY (resource_type, resource_key)
);
"#;

const PG_MIGRATION_011: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    user_id       TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    email         TEXT,
    display_name  TEXT,
    role          TEXT NOT NULL CHECK (role IN ('admin','operator','viewer')),
    is_active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL,
    last_login_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
"#;

const PG_MIGRATION_012: &str = r#"
CREATE TABLE IF NOT EXISTS invitations (
    invitation_id TEXT PRIMARY KEY,
    email         TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('admin','operator','viewer')),
    token_hash    TEXT NOT NULL UNIQUE,
    invited_by    TEXT NOT NULL REFERENCES users(user_id),
    expires_at    TIMESTAMPTZ NOT NULL,
    accepted_at   TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_invitations_token ON invitations(token_hash);
CREATE INDEX IF NOT EXISTS idx_invitations_email ON invitations(email);
CREATE INDEX IF NOT EXISTS idx_invitations_invited_by ON invitations(invited_by);

CREATE TABLE IF NOT EXISTS password_resets (
    reset_id    TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    token_hash  TEXT NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_password_resets_token ON password_resets(token_hash);
CREATE INDEX IF NOT EXISTS idx_password_resets_user ON password_resets(user_id);
"#;

const PG_MIGRATION_013: &str = r#"
CREATE TABLE IF NOT EXISTS totp_secrets (
    user_id      TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    secret_enc   TEXT NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT FALSE,
    confirmed_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS recovery_codes (
    code_id    TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    code_hash  TEXT NOT NULL,
    used_at    TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_recovery_codes_user ON recovery_codes(user_id);
CREATE INDEX IF NOT EXISTS idx_recovery_codes_hash ON recovery_codes(code_hash);
"#;

const PG_MIGRATION_014: &str = r#"
CREATE TABLE IF NOT EXISTS personal_access_tokens (
    token_id      TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    token_hash    TEXT NOT NULL UNIQUE,
    token_prefix  TEXT NOT NULL,
    scopes        TEXT NOT NULL DEFAULT '[]',
    expires_at    TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ,
    last_used_at  TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pat_hash ON personal_access_tokens(token_hash);
CREATE INDEX IF NOT EXISTS idx_pat_user ON personal_access_tokens(user_id);
"#;

const PG_MIGRATION_015: &str = r#"
CREATE TABLE IF NOT EXISTS oidc_identities (
    provider      TEXT NOT NULL,
    subject       TEXT NOT NULL,
    user_id       TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    email         TEXT,
    linked_at     TIMESTAMPTZ NOT NULL,
    last_login_at TIMESTAMPTZ,
    PRIMARY KEY (provider, subject)
);
CREATE INDEX IF NOT EXISTS idx_oidc_identities_user ON oidc_identities(user_id);

CREATE TABLE IF NOT EXISTS oidc_pending_logins (
    state         TEXT PRIMARY KEY,
    nonce         TEXT NOT NULL,
    redirect_to   TEXT,
    created_at    TIMESTAMPTZ NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_oidc_pending_expires ON oidc_pending_logins(expires_at);
"#;

const PG_MIGRATION_016: &str = r#"
CREATE TABLE IF NOT EXISTS audit_log (
    event_id    TEXT PRIMARY KEY,
    actor_type  TEXT NOT NULL,
    actor_id    TEXT,
    action      TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id   TEXT,
    diff_json   TEXT,
    ip_address  TEXT,
    user_agent  TEXT,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_target ON audit_log(target_type, target_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_type, actor_id);
"#;

const PG_MIGRATION_017: &str = r#"
CREATE TABLE IF NOT EXISTS alert_deliveries (
    delivery_id   TEXT PRIMARY KEY,
    rule_name     TEXT NOT NULL,
    channel_name  TEXT NOT NULL,
    job_key       TEXT NOT NULL,
    execution_id  TEXT,
    state         TEXT NOT NULL,
    error         TEXT,
    fired_at      TIMESTAMPTZ NOT NULL,
    delivered_at  TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_alert_deliveries_job_key ON alert_deliveries(job_key);
CREATE INDEX IF NOT EXISTS idx_alert_deliveries_fired_at ON alert_deliveries(fired_at);
CREATE INDEX IF NOT EXISTS idx_alert_deliveries_rule_name ON alert_deliveries(rule_name);
"#;

const PG_MIGRATION_018: &str = r#"
CREATE TABLE IF NOT EXISTS alert_rule_overrides (
    rule_name       TEXT PRIMARY KEY,
    enabled         BOOLEAN,
    snooze_until    TIMESTAMPTZ,
    throttle_secs   BIGINT,
    note            TEXT NOT NULL,
    set_by_user_id  TEXT NOT NULL,
    set_at          TIMESTAMPTZ NOT NULL,
    expires_at      TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_alert_rule_overrides_expires_at
    ON alert_rule_overrides(expires_at);
"#;

const PG_MIGRATION_020: &str = r#"
CREATE TABLE IF NOT EXISTS maintenance (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    manual_active BOOLEAN NOT NULL DEFAULT FALSE,
    window_start  TIMESTAMPTZ,
    window_end    TIMESTAMPTZ,
    note          TEXT,
    updated_by    TEXT,
    updated_at    TIMESTAMPTZ
);
"#;

fn map_err(e: postgres::Error) -> StoreError {
    StoreError::Database(e.to_string())
}

fn metadata_to_json(meta: &HashMap<String, String>) -> serde_json::Value {
    serde_json::Value::Object(
        meta.iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect(),
    )
}

fn json_to_metadata(val: serde_json::Value) -> HashMap<String, String> {
    match val {
        serde_json::Value::Object(map) => map
            .into_iter()
            .map(|(k, v)| match v {
                serde_json::Value::String(s) => (k, s),
                _ => (k, v.to_string()),
            })
            .collect(),
        _ => HashMap::new(),
    }
}

// ─── JobStore ───

impl JobStore for PgStore {
    fn get_job_state(&self, job_key: &str) -> Result<Option<JobState>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT job_key, next_fire_at, last_fired_at, fire_count, status, updated_at FROM job_states WHERE job_key = $1",
                &[&job_key],
            )
            .map_err(map_err)?;

        Ok(rows.first().map(|row| JobState {
            job_key: row.get(0),
            next_fire_at: row.get(1),
            last_fired_at: row.get(2),
            fire_count: row.get::<_, i64>(3) as u64,
            status: parse_job_status(&row.get::<_, String>(4)),
            updated_at: row.get(5),
        }))
    }

    fn upsert_job_state(&self, state: &JobState) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        let status = job_status_to_str(state.status);
        client
            .execute(
                "INSERT INTO job_states (job_key, next_fire_at, last_fired_at, fire_count, status, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT(job_key) DO UPDATE SET
                   next_fire_at = EXCLUDED.next_fire_at,
                   last_fired_at = EXCLUDED.last_fired_at,
                   fire_count = EXCLUDED.fire_count,
                   status = EXCLUDED.status,
                   updated_at = EXCLUDED.updated_at",
                &[
                    &state.job_key,
                    &state.next_fire_at,
                    &state.last_fired_at,
                    &(state.fire_count as i64),
                    &status,
                    &state.updated_at,
                ],
            )
            .map_err(map_err)?;
        Ok(())
    }

    fn list_job_states(&self) -> Result<Vec<JobState>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT job_key, next_fire_at, last_fired_at, fire_count, status, updated_at FROM job_states ORDER BY job_key",
                &[],
            )
            .map_err(map_err)?;

        Ok(rows
            .iter()
            .map(|row| JobState {
                job_key: row.get(0),
                next_fire_at: row.get(1),
                last_fired_at: row.get(2),
                fire_count: row.get::<_, i64>(3) as u64,
                status: parse_job_status(&row.get::<_, String>(4)),
                updated_at: row.get(5),
            })
            .collect())
    }

    fn delete_job_state(&self, job_key: &str) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        client
            .execute("DELETE FROM job_states WHERE job_key = $1", &[&job_key])
            .map_err(map_err)?;
        Ok(())
    }
}

// ─── ExecutionStore ───

impl ExecutionStore for PgStore {
    fn create_execution(&self, exec: &Execution) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        pg_insert_execution(&mut client, exec)
    }

    fn create_execution_and_advance_job_state(
        &self,
        exec: &Execution,
        job_state: &JobState,
    ) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut tx = client.transaction().map_err(map_err)?;
        pg_insert_execution_tx(&mut tx, exec)?;
        pg_upsert_job_state_tx(&mut tx, job_state)?;
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn get_execution(&self, id: Uuid) -> Result<Option<Execution>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for FROM executions WHERE id = $1",
                &[&id],
            )
            .map_err(map_err)?;

        Ok(rows.first().map(row_to_execution))
    }

    fn claim_execution(
        &self,
        id: Uuid,
        runner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Execution, StoreError> {
        let mut client = self.client.lock().unwrap();
        let affected = client
            .execute(
                "UPDATE executions SET state = 'claimed', runner_id = $1, claimed_at = $2, started_at = $2 WHERE id = $3 AND state = 'queued'",
                &[&runner_id, &now, &id],
            )
            .map_err(map_err)?;

        if affected == 0 {
            return Err(StoreError::Conflict(format!(
                "execution {id} is not in queued state"
            )));
        }

        let rows = client
            .query(
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for FROM executions WHERE id = $1",
                &[&id],
            )
            .map_err(map_err)?;

        rows.first()
            .map(row_to_execution)
            .ok_or_else(|| StoreError::NotFound(format!("execution {id}")))
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_execution(
        &self,
        id: Uuid,
        runner_id: Option<&str>,
        state: ExecutionState,
        duration_ms: Option<i64>,
        error: Option<&str>,
        dead_reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let mut client = self.client.lock().unwrap();
        let affected = client
            .execute(
                "UPDATE executions SET state = $1, completed_at = $2, duration_ms = $3, error = $4, dead_reason = $5 WHERE id = $6 AND state = 'claimed' AND ($7::TEXT IS NULL OR runner_id = $7)",
                &[&state_to_str(state), &now, &duration_ms, &error, &dead_reason, &id, &runner_id],
            )
            .map_err(map_err)?;
        Ok(affected > 0)
    }

    fn find_queued_executions(
        &self,
        capabilities: &[String],
        limit: u32,
    ) -> Result<Vec<Execution>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let fetch_limit = if capabilities.is_empty() {
            limit
        } else {
            limit * 4
        };
        let rows = client
            .query(
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for FROM executions WHERE state = 'queued' ORDER BY fire_at ASC LIMIT $1",
                &[&(fetch_limit as i64)],
            )
            .map_err(map_err)?;

        let all: Vec<Execution> = rows.iter().map(row_to_execution).collect();

        if capabilities.is_empty() {
            return Ok(all);
        }

        Ok(all
            .into_iter()
            .filter(|exec| {
                let require: Vec<String> = exec
                    .metadata
                    .get("__require")
                    .and_then(|v| serde_json::from_str(v).ok())
                    .unwrap_or_default();
                require.iter().all(|req| capabilities.contains(req))
            })
            .take(limit as usize)
            .collect())
    }

    fn list_executions(&self, filter: &ExecutionFilter) -> Result<Vec<Execution>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for FROM executions WHERE true",
        );
        let mut params: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();
        let mut idx = 1;

        if let Some(ref jk) = filter.job_key {
            params.push(Box::new(jk.clone()));
            sql.push_str(&format!(" AND job_key = ${idx}"));
            idx += 1;
        }
        if let Some(state) = filter.state {
            params.push(Box::new(state_to_str(state).to_string()));
            sql.push_str(&format!(" AND state = ${idx}"));
            idx += 1;
        }
        if let Some(ref rid) = filter.runner_id {
            params.push(Box::new(rid.clone()));
            sql.push_str(&format!(" AND runner_id = ${idx}"));
            idx += 1;
        }
        if let Some(since) = filter.since {
            params.push(Box::new(since));
            sql.push_str(&format!(" AND created_at >= ${idx}"));
            idx += 1;
        }
        if let Some(until) = filter.until {
            params.push(Box::new(until));
            sql.push_str(&format!(" AND created_at <= ${idx}"));
            idx += 1;
        }

        sql.push_str(" ORDER BY created_at DESC");
        let limit = filter.limit.unwrap_or(100);
        params.push(Box::new(limit as i64));
        sql.push_str(&format!(" LIMIT ${idx}"));

        let params_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = client.query(&sql, &params_ref).map_err(map_err)?;

        Ok(rows.iter().map(row_to_execution).collect())
    }

    fn list_claimed_older_than(
        &self,
        cutoff: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Execution>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for
                 FROM executions
                 WHERE state = 'claimed' AND (claimed_at IS NULL OR claimed_at <= $1)
                 ORDER BY claimed_at ASC NULLS FIRST
                 LIMIT $2",
                &[&cutoff, &(limit as i64)],
            )
            .map_err(map_err)?;

        Ok(rows.iter().map(row_to_execution).collect())
    }

    fn find_execution_by_idempotency_key(
        &self,
        job_key: &str,
        idempotency_key: &str,
        window_start: DateTime<Utc>,
    ) -> Result<Option<Execution>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for
                 FROM executions
                 WHERE job_key = $1 AND idempotency_key = $2
                   AND (state IN ('queued', 'claimed') OR created_at >= $3)
                 ORDER BY created_at DESC
                 LIMIT 1",
                &[&job_key, &idempotency_key, &window_start],
            )
            .map_err(map_err)?;

        Ok(rows.first().map(row_to_execution))
    }

    fn requeue_abandoned(
        &self,
        runner_id: &str,
        _now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT id FROM executions WHERE runner_id = $1 AND state = 'claimed'",
                &[&runner_id],
            )
            .map_err(map_err)?;

        let ids: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();

        if !ids.is_empty() {
            client
                .execute(
                    "UPDATE executions SET state = 'queued', runner_id = NULL, claimed_at = NULL, started_at = NULL WHERE runner_id = $1 AND state = 'claimed'",
                    &[&runner_id],
                )
                .map_err(map_err)?;
        }

        Ok(ids)
    }

    fn requeue_if_claimed(&self, id: Uuid, _now: DateTime<Utc>) -> Result<bool, StoreError> {
        let mut client = self.client.lock().unwrap();
        let affected = client
            .execute(
                "UPDATE executions SET state = 'queued', runner_id = NULL, claimed_at = NULL, started_at = NULL WHERE id = $1 AND state = 'claimed'",
                &[&id],
            )
            .map_err(map_err)?;
        Ok(affected > 0)
    }

    fn cancel_execution(&self, id: Uuid, now: DateTime<Utc>) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        client
            .execute(
                "UPDATE executions SET state = 'cancelled', completed_at = $1 WHERE id = $2 AND state IN ('queued', 'claimed')",
                &[&now, &id],
            )
            .map_err(map_err)?;
        Ok(())
    }

    fn count_by_state(&self) -> Result<HashMap<ExecutionState, u64>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query("SELECT state, COUNT(*) FROM executions GROUP BY state", &[])
            .map_err(map_err)?;

        let mut map = HashMap::new();
        for row in &rows {
            let state_str: String = row.get(0);
            let count: i64 = row.get(1);
            map.insert(parse_execution_state(&state_str), count as u64);
        }
        Ok(map)
    }

    fn count_executions_in_states(
        &self,
        job_key: &str,
        states: &[ExecutionState],
    ) -> Result<u64, StoreError> {
        if states.is_empty() {
            return Ok(0);
        }
        let mut client = self.client.lock().unwrap();
        let state_strs: Vec<String> = states
            .iter()
            .map(|s| state_to_str(*s).to_string())
            .collect();
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM executions WHERE job_key = $1 AND state = ANY($2)",
                &[&job_key, &state_strs],
            )
            .map_err(map_err)?;
        let count: i64 = row.get(0);
        Ok(count as u64)
    }

    fn job_execution_metrics(&self) -> Result<Vec<JobExecutionMetrics>, StoreError> {
        let mut client = self.client.lock().unwrap();

        // Cumulative duration-bucket columns are generated from the shared
        // boundary list so the SQL and the Prometheus renderer can't drift.
        // `::bigint` casts keep SUM() out of Postgres `numeric` so each
        // column reads back as i64.
        let bucket_cols: String = JOB_DURATION_BUCKETS_SECONDS
            .iter()
            .map(|secs| {
                let ms = (secs * 1000.0).round() as i64;
                format!(
                    ", SUM(CASE WHEN duration_ms IS NOT NULL AND duration_ms <= {ms} THEN 1 ELSE 0 END)::bigint"
                )
            })
            .collect();

        let sql = format!(
            "SELECT job_key, \
             SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END)::bigint, \
             SUM(CASE WHEN state = 'failed' THEN 1 ELSE 0 END)::bigint, \
             SUM(CASE WHEN state = 'dead' THEN 1 ELSE 0 END)::bigint, \
             SUM(CASE WHEN state = 'cancelled' THEN 1 ELSE 0 END)::bigint, \
             SUM(CASE WHEN duration_ms IS NOT NULL THEN 1 ELSE 0 END)::bigint, \
             COALESCE(SUM(duration_ms), 0)::bigint, \
             MAX(completed_at){bucket_cols} \
             FROM executions GROUP BY job_key"
        );

        let rows = client.query(sql.as_str(), &[]).map_err(map_err)?;
        let n_buckets = JOB_DURATION_BUCKETS_SECONDS.len();

        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut duration_buckets = Vec::with_capacity(n_buckets);
            for i in 0..n_buckets {
                // Bucket columns follow the eight fixed leading columns.
                duration_buckets.push(row.get::<usize, i64>(8 + i) as u64);
            }
            out.push(JobExecutionMetrics {
                job_key: row.get(0),
                completed: row.get::<usize, i64>(1) as u64,
                failed: row.get::<usize, i64>(2) as u64,
                dead: row.get::<usize, i64>(3) as u64,
                cancelled: row.get::<usize, i64>(4) as u64,
                duration_count: row.get::<usize, i64>(5) as u64,
                duration_sum_ms: row.get::<usize, i64>(6),
                last_run_at: row.get::<usize, Option<DateTime<Utc>>>(7),
                duration_buckets,
            });
        }
        Ok(out)
    }

    fn prune_executions_older_than(
        &self,
        cutoff: DateTime<Utc>,
        limit: u32,
    ) -> Result<u64, StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut tx = client.transaction().map_err(map_err)?;
        let limit_i = limit as i64;
        // Child logs first, then parents (mirrors the SQLite path). Both
        // subqueries are identical and deterministic so they match the same
        // batch even though `executions` is read twice.
        let selection = format!(
            "SELECT e.id FROM executions e
             WHERE e.completed_at IS NOT NULL AND e.completed_at <= $1
               AND {DELETABLE_EXECUTION}
             ORDER BY e.completed_at ASC, e.id ASC
             LIMIT $2"
        );
        tx.execute(
            &format!("DELETE FROM execution_logs WHERE execution_id IN ({selection})"),
            &[&cutoff, &limit_i],
        )
        .map_err(map_err)?;
        let affected = tx
            .execute(
                &format!("DELETE FROM executions WHERE id IN ({selection})"),
                &[&cutoff, &limit_i],
            )
            .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(affected)
    }

    fn prune_executions_keep_last(
        &self,
        job_key: &str,
        keep_last: u32,
        limit: u32,
    ) -> Result<u64, StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut tx = client.transaction().map_err(map_err)?;
        let limit_i = limit as i64;
        let keep_i = keep_last as i64;
        let selection = format!(
            "SELECT e.id FROM executions e
             WHERE e.job_key = $1 AND e.completed_at IS NOT NULL
               AND {DELETABLE_EXECUTION}
             ORDER BY e.completed_at DESC, e.id DESC
             LIMIT $2 OFFSET $3"
        );
        tx.execute(
            &format!("DELETE FROM execution_logs WHERE execution_id IN ({selection})"),
            &[&job_key, &limit_i, &keep_i],
        )
        .map_err(map_err)?;
        let affected = tx
            .execute(
                &format!("DELETE FROM executions WHERE id IN ({selection})"),
                &[&job_key, &limit_i, &keep_i],
            )
            .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(affected)
    }
}

// ─── RunnerStore ───

impl RunnerStore for PgStore {
    fn upsert_runner(&self, runner: &Runner) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        let caps = serde_json::to_value(&runner.capabilities).unwrap();
        let inflight = serde_json::to_value(&runner.inflight).unwrap();
        client
            .execute(
                "INSERT INTO runners (runner_id, capabilities, max_inflight, last_poll_at, inflight, status, registered_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT(runner_id) DO UPDATE SET
                   capabilities = EXCLUDED.capabilities,
                   max_inflight = EXCLUDED.max_inflight,
                   last_poll_at = EXCLUDED.last_poll_at,
                   inflight = EXCLUDED.inflight,
                   status = EXCLUDED.status",
                &[
                    &runner.runner_id,
                    &caps,
                    &(runner.max_inflight as i32),
                    &runner.last_poll_at,
                    &inflight,
                    &runner_status_to_str(runner.status),
                    &runner.registered_at,
                ],
            )
            .map_err(map_err)?;
        Ok(())
    }

    fn get_runner(&self, runner_id: &str) -> Result<Option<Runner>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT runner_id, capabilities, max_inflight, last_poll_at, inflight, status, registered_at FROM runners WHERE runner_id = $1",
                &[&runner_id],
            )
            .map_err(map_err)?;

        Ok(rows.first().map(row_to_runner))
    }

    fn list_runners(&self) -> Result<Vec<Runner>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT runner_id, capabilities, max_inflight, last_poll_at, inflight, status, registered_at FROM runners ORDER BY runner_id",
                &[],
            )
            .map_err(map_err)?;

        Ok(rows.iter().map(row_to_runner).collect())
    }

    fn remove_runner(&self, runner_id: &str) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        client
            .execute("DELETE FROM runners WHERE runner_id = $1", &[&runner_id])
            .map_err(map_err)?;
        Ok(())
    }

    fn update_poll(
        &self,
        runner_id: &str,
        inflight: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        let inflight_json = serde_json::to_value(inflight).unwrap();
        client
            .execute(
                "UPDATE runners SET last_poll_at = $1, inflight = $2, status = 'online' WHERE runner_id = $3",
                &[&now, &inflight_json, &runner_id],
            )
            .map_err(map_err)?;
        Ok(())
    }

    fn runner_identity_bind(
        &self,
        runner_id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<String, StoreError> {
        // One statement, so the insert-or-read is atomic even across
        // connections. The no-op `DO UPDATE SET` (rather than `DO NOTHING`)
        // is what makes `RETURNING` yield the existing row on conflict.
        let mut client = self.client.lock().unwrap();
        let row = client
            .query_one(
                "INSERT INTO runner_identities (runner_id, owner_id, bound_at)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (runner_id)
                   DO UPDATE SET owner_id = runner_identities.owner_id
                 RETURNING owner_id",
                &[&runner_id, &owner_id, &now],
            )
            .map_err(map_err)?;
        Ok(row.get(0))
    }

    fn runner_identity_owner(&self, runner_id: &str) -> Result<Option<String>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT owner_id FROM runner_identities WHERE runner_id = $1",
                &[&runner_id],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| row.get(0)))
    }

    fn runner_identity_release(&self, runner_id: &str) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        client
            .execute(
                "DELETE FROM runner_identities WHERE runner_id = $1",
                &[&runner_id],
            )
            .map_err(map_err)?;
        Ok(())
    }
}

// ─── DeadLetterStore ───

impl DeadLetterStore for PgStore {
    fn add_dead_letter(&self, dl: &DeadLetter) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        let metadata = metadata_to_json(&dl.metadata);
        let attempt = dl.attempt as i32;
        client
            .execute(
                PG_INSERT_DEAD_LETTER_SQL,
                &[
                    &dl.id,
                    &dl.execution_id,
                    &dl.job_key,
                    &dl.fire_at,
                    &attempt,
                    &dl.error,
                    &dl.dead_reason,
                    &metadata,
                    &dl.created_at,
                    &dl.expires_at,
                    &dl.scheduled_for,
                ],
            )
            .map_err(map_err)?;
        Ok(())
    }

    fn complete_as_dead(
        &self,
        execution_id: Uuid,
        runner_id: Option<&str>,
        duration_ms: Option<i64>,
        error: Option<&str>,
        dead_letter: &DeadLetter,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut tx = client.transaction().map_err(map_err)?;
        let dead_reason = dead_letter.dead_reason.as_str();
        let updated = tx.execute(
            "UPDATE executions SET state = 'dead', completed_at = $1, duration_ms = $2, error = $3, dead_reason = $4 WHERE id = $5 AND state IN ('claimed', 'failed') AND ($6::TEXT IS NULL OR runner_id = $6)",
            &[&now, &duration_ms, &error, &dead_reason, &execution_id, &runner_id],
        )
        .map_err(map_err)?;
        if updated == 0 {
            // Dropping the uncommitted transaction rolls it back — the
            // dead-letter row must not exist for a run we didn't kill.
            return Ok(false);
        }
        let metadata = metadata_to_json(&dead_letter.metadata);
        let attempt = dead_letter.attempt as i32;
        tx.execute(
            PG_INSERT_DEAD_LETTER_SQL,
            &[
                &dead_letter.id,
                &dead_letter.execution_id,
                &dead_letter.job_key,
                &dead_letter.fire_at,
                &attempt,
                &dead_letter.error,
                &dead_letter.dead_reason,
                &metadata,
                &dead_letter.created_at,
                &dead_letter.expires_at,
                &dead_letter.scheduled_for,
            ],
        )
        .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(true)
    }

    fn replay_dead_letter(
        &self,
        dead_letter_id: Uuid,
        execution: &Execution,
    ) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut tx = client.transaction().map_err(map_err)?;
        let affected = tx
            .execute("DELETE FROM dead_letters WHERE id = $1", &[&dead_letter_id])
            .map_err(map_err)?;
        if affected == 0 {
            // Dropping the uncommitted transaction rolls it back.
            return Err(StoreError::NotFound(format!(
                "dead letter {dead_letter_id}"
            )));
        }
        pg_insert_execution_tx(&mut tx, execution)?;
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn get_dead_letter(&self, id: Uuid) -> Result<Option<DeadLetter>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at, scheduled_for FROM dead_letters WHERE id = $1",
                &[&id],
            )
            .map_err(map_err)?;

        Ok(rows.first().map(row_to_dead_letter))
    }

    fn list_dead_letters(&self, filter: &DeadLetterFilter) -> Result<Vec<DeadLetter>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at, scheduled_for FROM dead_letters WHERE true",
        );
        let mut params: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();
        let mut idx = 1;

        if let Some(ref jk) = filter.job_key {
            params.push(Box::new(jk.clone()));
            sql.push_str(&format!(" AND job_key = ${idx}"));
            idx += 1;
        }

        sql.push_str(" ORDER BY created_at DESC");
        let limit = filter.limit.unwrap_or(100);
        params.push(Box::new(limit as i64));
        sql.push_str(&format!(" LIMIT ${idx}"));

        let params_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = client.query(&sql, &params_ref).map_err(map_err)?;

        Ok(rows.iter().map(row_to_dead_letter).collect())
    }

    fn remove_dead_letters(&self, ids: &[Uuid]) -> Result<u64, StoreError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut client = self.client.lock().unwrap();
        let affected = client
            .execute("DELETE FROM dead_letters WHERE id = ANY($1)", &[&ids])
            .map_err(map_err)?;
        Ok(affected)
    }

    fn clear_dead_letters(&self, job_key: Option<&str>) -> Result<u64, StoreError> {
        let mut client = self.client.lock().unwrap();
        let affected = match job_key {
            Some(jk) => client
                .execute("DELETE FROM dead_letters WHERE job_key = $1", &[&jk])
                .map_err(map_err)?,
            None => client
                .execute("DELETE FROM dead_letters", &[])
                .map_err(map_err)?,
        };
        Ok(affected)
    }

    fn remove_dead_letter(&self, id: Uuid) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        client
            .execute("DELETE FROM dead_letters WHERE id = $1", &[&id])
            .map_err(map_err)?;
        Ok(())
    }

    fn purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        let mut client = self.client.lock().unwrap();
        let affected = client
            .execute(
                "DELETE FROM dead_letters WHERE expires_at IS NOT NULL AND expires_at <= $1",
                &[&now],
            )
            .map_err(map_err)?;
        Ok(affected)
    }
}

// ─── AuthStore ───

impl AuthStore for PgStore {
    fn create_client(&self, client: &ApiClient) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let scopes = serde_json::to_string(&client.scopes).unwrap_or_default();
        db.execute(
            "INSERT INTO api_clients (client_id, name, scopes, is_active, created_at, managed_by)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(client_id) DO UPDATE SET
               name = EXCLUDED.name, scopes = EXCLUDED.scopes, is_active = EXCLUDED.is_active,
               managed_by = EXCLUDED.managed_by",
            &[
                &client.client_id,
                &client.name,
                &scopes,
                &client.is_active,
                &client.created_at,
                &client.managed_by,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_client(&self, client_id: &str) -> Result<Option<ApiClient>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT client_id, name, scopes, is_active, created_at, managed_by FROM api_clients WHERE client_id = $1",
                &[&client_id],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_api_client))
    }

    fn list_clients(&self) -> Result<Vec<ApiClient>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT client_id, name, scopes, is_active, created_at, managed_by FROM api_clients ORDER BY name",
                &[],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(row_to_api_client).collect())
    }

    fn delete_client(&self, client_id: &str) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute("DELETE FROM api_keys WHERE client_id = $1", &[&client_id])
            .map_err(map_err)?;
        db.execute(
            "DELETE FROM api_clients WHERE client_id = $1",
            &[&client_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn create_api_key(&self, key: &ApiKey) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO api_keys (key_id, client_id, key_hash, key_prefix, expires_at, revoked_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                &key.key_id,
                &key.client_id,
                &key.key_hash,
                &key.key_prefix,
                &key.expires_at,
                &key.revoked_at,
                &key.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT key_id, client_id, key_hash, key_prefix, expires_at, revoked_at, created_at FROM api_keys WHERE key_hash = $1",
                &[&key_hash],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_api_key))
    }

    fn revoke_api_key(&self, key_id: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE api_keys SET revoked_at = $1 WHERE key_id = $2",
            &[&now, &key_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn set_api_key_expiry(
        &self,
        key_id: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE api_keys SET expires_at = $1 WHERE key_id = $2",
            &[&expires_at, &key_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn list_api_keys(&self, client_id: &str) -> Result<Vec<ApiKey>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT key_id, client_id, key_hash, key_prefix, expires_at, revoked_at, created_at FROM api_keys WHERE client_id = $1 ORDER BY created_at DESC",
                &[&client_id],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(row_to_api_key).collect())
    }

    fn get_credentials(&self, username: &str) -> Result<Option<PasswordCredential>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT user_id, username, password_hash, failed_attempts, locked_until, created_at FROM password_credentials WHERE username = $1",
                &[&username],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| PasswordCredential {
            user_id: row.get(0),
            username: row.get(1),
            password_hash: row.get(2),
            failed_attempts: row.get::<_, i32>(3) as u32,
            locked_until: row.get(4),
            created_at: row.get(5),
        }))
    }

    fn upsert_credentials(&self, cred: &PasswordCredential) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO password_credentials (user_id, username, password_hash, failed_attempts, locked_until, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(user_id) DO UPDATE SET
               username = EXCLUDED.username,
               password_hash = EXCLUDED.password_hash,
               failed_attempts = EXCLUDED.failed_attempts,
               locked_until = EXCLUDED.locked_until",
            &[
                &cred.user_id,
                &cred.username,
                &cred.password_hash,
                &(cred.failed_attempts as i32),
                &cred.locked_until,
                &cred.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn create_refresh_token(&self, token: &RefreshToken) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO refresh_tokens (token_hash, client_id, user_id, expires_at, revoked_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &token.token_hash,
                &token.client_id,
                &token.user_id,
                &token.expires_at,
                &token.revoked_at,
                &token.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn validate_refresh_token(&self, token_hash: &str) -> Result<Option<RefreshToken>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT token_hash, client_id, user_id, expires_at, revoked_at, created_at FROM refresh_tokens WHERE token_hash = $1 AND revoked_at IS NULL",
                &[&token_hash],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| RefreshToken {
            token_hash: row.get(0),
            client_id: row.get(1),
            user_id: row.get(2),
            expires_at: row.get(3),
            revoked_at: row.get(4),
            created_at: row.get(5),
        }))
    }

    fn revoke_refresh_token(&self, token_hash: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE refresh_tokens SET revoked_at = $1 WHERE token_hash = $2",
            &[&now, &token_hash],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn users_create(&self, user: &User) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let role = user.role.as_str();
        db.execute(
            "INSERT INTO users (user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT(user_id) DO UPDATE SET
                username = EXCLUDED.username,
                email = EXCLUDED.email,
                display_name = EXCLUDED.display_name,
                role = EXCLUDED.role,
                is_active = EXCLUDED.is_active,
                updated_at = EXCLUDED.updated_at",
            &[
                &user.user_id,
                &user.username,
                &user.email,
                &user.display_name,
                &role,
                &user.is_active,
                &user.created_at,
                &user.updated_at,
                &user.last_login_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn users_get_by_id(&self, user_id: &str) -> Result<Option<User>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at FROM users WHERE user_id = $1",
                &[&user_id],
            )
            .map_err(map_err)?;
        rows.first().map(row_to_user).transpose()
    }

    fn users_get_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at FROM users WHERE username = $1",
                &[&username],
            )
            .map_err(map_err)?;
        rows.first().map(row_to_user).transpose()
    }

    fn users_list(&self) -> Result<Vec<User>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at FROM users ORDER BY username",
                &[],
            )
            .map_err(map_err)?;
        rows.iter().map(row_to_user).collect()
    }

    fn users_update(&self, user: &User) -> Result<(), StoreError> {
        // users_create is upsert-on-user_id, so update is the same write.
        // The last-admin-demotion check lives in the API layer (calls
        // users_count_active_admins before mutating).
        self.users_create(user)
    }

    fn users_delete(&self, user_id: &str) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute("DELETE FROM users WHERE user_id = $1", &[&user_id])
            .map_err(map_err)?;
        Ok(())
    }

    fn users_set_last_login(&self, user_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE users SET last_login_at = $1, updated_at = $1 WHERE user_id = $2",
            &[&at, &user_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn users_count_active_admins(&self) -> Result<u64, StoreError> {
        let mut db = self.client.lock().unwrap();
        let row = db
            .query_one(
                "SELECT COUNT(*) FROM users WHERE role = 'admin' AND is_active = TRUE",
                &[],
            )
            .map_err(map_err)?;
        Ok(row.get::<_, i64>(0) as u64)
    }

    fn users_token_generation(&self, user_id: &str) -> Result<Option<i64>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT token_generation FROM users WHERE user_id = $1",
                &[&user_id],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|r| r.get::<_, i64>(0)))
    }

    fn users_bump_token_generation(&self, user_id: &str) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        // Single-statement read-modify-write so concurrent bumps serialise.
        db.execute(
            "UPDATE users SET token_generation = token_generation + 1 WHERE user_id = $1",
            &[&user_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn invitations_create(&self, invite: &Invitation) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let role = invite.role.as_str();
        db.execute(
            "INSERT INTO invitations (invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            &[
                &invite.invitation_id,
                &invite.email,
                &role,
                &invite.token_hash,
                &invite.invited_by,
                &invite.expires_at,
                &invite.accepted_at,
                &invite.revoked_at,
                &invite.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn invitations_get(&self, invitation_id: &str) -> Result<Option<Invitation>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at FROM invitations WHERE invitation_id = $1",
                &[&invitation_id],
            )
            .map_err(map_err)?;
        rows.first().map(row_to_invitation).transpose()
    }

    fn invitations_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Invitation>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at FROM invitations WHERE token_hash = $1",
                &[&token_hash],
            )
            .map_err(map_err)?;
        rows.first().map(row_to_invitation).transpose()
    }

    fn invitations_list(&self) -> Result<Vec<Invitation>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at FROM invitations ORDER BY created_at DESC",
                &[],
            )
            .map_err(map_err)?;
        rows.iter().map(row_to_invitation).collect()
    }

    fn invitations_mark_accepted(
        &self,
        invitation_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE invitations SET accepted_at = $1 WHERE invitation_id = $2",
            &[&at, &invitation_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn invitations_revoke(&self, invitation_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE invitations SET revoked_at = $1 WHERE invitation_id = $2",
            &[&at, &invitation_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn password_resets_create(&self, reset: &PasswordReset) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO password_resets (reset_id, user_id, token_hash, expires_at, used_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &reset.reset_id,
                &reset.user_id,
                &reset.token_hash,
                &reset.expires_at,
                &reset.used_at,
                &reset.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn password_resets_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordReset>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT reset_id, user_id, token_hash, expires_at, used_at, created_at FROM password_resets WHERE token_hash = $1",
                &[&token_hash],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| PasswordReset {
            reset_id: row.get(0),
            user_id: row.get(1),
            token_hash: row.get(2),
            expires_at: row.get(3),
            used_at: row.get(4),
            created_at: row.get(5),
        }))
    }

    fn password_resets_mark_used(
        &self,
        reset_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE password_resets SET used_at = $1 WHERE reset_id = $2",
            &[&at, &reset_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn totp_upsert(&self, secret: &TotpSecret) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO totp_secrets (user_id, secret_enc, enabled, confirmed_at, created_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT(user_id) DO UPDATE SET
                secret_enc = EXCLUDED.secret_enc,
                enabled = EXCLUDED.enabled,
                confirmed_at = EXCLUDED.confirmed_at",
            &[
                &secret.user_id,
                &secret.secret_enc,
                &secret.enabled,
                &secret.confirmed_at,
                &secret.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn totp_get(&self, user_id: &str) -> Result<Option<TotpSecret>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT user_id, secret_enc, enabled, confirmed_at, created_at FROM totp_secrets WHERE user_id = $1",
                &[&user_id],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| TotpSecret {
            user_id: row.get(0),
            secret_enc: row.get(1),
            enabled: row.get(2),
            confirmed_at: row.get(3),
            created_at: row.get(4),
        }))
    }

    fn totp_set_enabled(
        &self,
        user_id: &str,
        enabled: bool,
        confirmed_at: Option<DateTime<Utc>>,
    ) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE totp_secrets SET enabled = $1, confirmed_at = $2 WHERE user_id = $3",
            &[&enabled, &confirmed_at, &user_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn totp_delete(&self, user_id: &str) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        // recovery_codes cascade via FK ON DELETE CASCADE, but delete both
        // explicitly to keep the contract identical across backends.
        db.execute("DELETE FROM totp_secrets WHERE user_id = $1", &[&user_id])
            .map_err(map_err)?;
        db.execute("DELETE FROM recovery_codes WHERE user_id = $1", &[&user_id])
            .map_err(map_err)?;
        Ok(())
    }

    fn recovery_codes_replace_all(
        &self,
        user_id: &str,
        codes: &[RecoveryCode],
    ) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let mut tx = db.transaction().map_err(map_err)?;
        tx.execute("DELETE FROM recovery_codes WHERE user_id = $1", &[&user_id])
            .map_err(map_err)?;
        for code in codes {
            tx.execute(
                "INSERT INTO recovery_codes (code_id, user_id, code_hash, used_at, created_at)
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &code.code_id,
                    &code.user_id,
                    &code.code_hash,
                    &code.used_at,
                    &code.created_at,
                ],
            )
            .map_err(map_err)?;
        }
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn recovery_codes_find_unused(
        &self,
        user_id: &str,
        code_hash: &str,
    ) -> Result<Option<RecoveryCode>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT code_id, user_id, code_hash, used_at, created_at FROM recovery_codes
                 WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL",
                &[&user_id, &code_hash],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| RecoveryCode {
            code_id: row.get(0),
            user_id: row.get(1),
            code_hash: row.get(2),
            used_at: row.get(3),
            created_at: row.get(4),
        }))
    }

    fn recovery_codes_mark_used(&self, code_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE recovery_codes SET used_at = $1 WHERE code_id = $2",
            &[&at, &code_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn recovery_codes_count_unused(&self, user_id: &str) -> Result<u64, StoreError> {
        let mut db = self.client.lock().unwrap();
        let row = db
            .query_one(
                "SELECT COUNT(*) FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL",
                &[&user_id],
            )
            .map_err(map_err)?;
        Ok(row.get::<_, i64>(0) as u64)
    }

    fn pat_create(&self, pat: &PersonalAccessToken) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let scopes =
            serde_json::to_string(&pat.scopes).map_err(|e| StoreError::Database(e.to_string()))?;
        db.execute(
            "INSERT INTO personal_access_tokens (token_id, user_id, name, token_hash, token_prefix, scopes, expires_at, revoked_at, last_used_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            &[
                &pat.token_id,
                &pat.user_id,
                &pat.name,
                &pat.token_hash,
                &pat.token_prefix,
                &scopes,
                &pat.expires_at,
                &pat.revoked_at,
                &pat.last_used_at,
                &pat.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn pat_find_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PersonalAccessToken>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT token_id, user_id, name, token_hash, token_prefix, scopes, expires_at, revoked_at, last_used_at, created_at FROM personal_access_tokens WHERE token_hash = $1",
                &[&token_hash],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_pat))
    }

    fn pat_list(&self, user_id: &str) -> Result<Vec<PersonalAccessToken>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT token_id, user_id, name, token_hash, token_prefix, scopes, expires_at, revoked_at, last_used_at, created_at FROM personal_access_tokens WHERE user_id = $1 ORDER BY created_at DESC",
                &[&user_id],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(row_to_pat).collect())
    }

    fn pat_revoke(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE personal_access_tokens SET revoked_at = $1 WHERE token_id = $2",
            &[&at, &token_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn pat_touch_last_used(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE personal_access_tokens SET last_used_at = $1 WHERE token_id = $2",
            &[&at, &token_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn oidc_link(&self, identity: &OidcIdentity) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO oidc_identities (provider, subject, user_id, email, linked_at, last_login_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(provider, subject) DO UPDATE SET
                user_id = EXCLUDED.user_id,
                email = EXCLUDED.email,
                last_login_at = EXCLUDED.last_login_at",
            &[
                &identity.provider,
                &identity.subject,
                &identity.user_id,
                &identity.email,
                &identity.linked_at,
                &identity.last_login_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn oidc_get_by_subject(
        &self,
        provider: &str,
        subject: &str,
    ) -> Result<Option<OidcIdentity>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT provider, subject, user_id, email, linked_at, last_login_at FROM oidc_identities WHERE provider = $1 AND subject = $2",
                &[&provider, &subject],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_oidc_identity))
    }

    fn oidc_touch_last_login(
        &self,
        provider: &str,
        subject: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "UPDATE oidc_identities SET last_login_at = $1 WHERE provider = $2 AND subject = $3",
            &[&at, &provider, &subject],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn oidc_pending_create(&self, pending: &OidcPendingLogin) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO oidc_pending_logins (state, nonce, redirect_to, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
            &[
                &pending.state,
                &pending.nonce,
                &pending.redirect_to,
                &pending.created_at,
                &pending.expires_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn oidc_pending_take(&self, state: &str) -> Result<Option<OidcPendingLogin>, StoreError> {
        let mut db = self.client.lock().unwrap();
        // Single-statement atomic read+delete via RETURNING so a state param
        // can be consumed exactly once even across server instances.
        let rows = db
            .query(
                "DELETE FROM oidc_pending_logins WHERE state = $1
                 RETURNING state, nonce, redirect_to, created_at, expires_at",
                &[&state],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| OidcPendingLogin {
            state: row.get(0),
            nonce: row.get(1),
            redirect_to: row.get(2),
            created_at: row.get(3),
            expires_at: row.get(4),
        }))
    }

    fn oidc_pending_purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        let mut db = self.client.lock().unwrap();
        let affected = db
            .execute(
                "DELETE FROM oidc_pending_logins WHERE expires_at < $1",
                &[&now],
            )
            .map_err(map_err)?;
        Ok(affected)
    }

    fn audit_log(&self, event: &AuditEvent) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO audit_log (event_id, actor_type, actor_id, action, target_type, target_id, diff_json, ip_address, user_agent, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            &[
                &event.event_id,
                &event.actor_type,
                &event.actor_id,
                &event.action,
                &event.target_type,
                &event.target_id,
                &event.diff_json,
                &event.ip_address,
                &event.user_agent,
                &event.created_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn audit_list(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let mut sql = String::from(
            "SELECT event_id, actor_type, actor_id, action, target_type, target_id, diff_json, ip_address, user_agent, created_at FROM audit_log WHERE true",
        );
        let mut params: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();
        let mut idx = 1;
        if let Some(v) = &filter.actor_type {
            params.push(Box::new(v.clone()));
            sql.push_str(&format!(" AND actor_type = ${idx}"));
            idx += 1;
        }
        if let Some(v) = &filter.actor_id {
            params.push(Box::new(v.clone()));
            sql.push_str(&format!(" AND actor_id = ${idx}"));
            idx += 1;
        }
        if let Some(v) = &filter.action {
            params.push(Box::new(v.clone()));
            sql.push_str(&format!(" AND action = ${idx}"));
            idx += 1;
        }
        if let Some(v) = &filter.target_type {
            params.push(Box::new(v.clone()));
            sql.push_str(&format!(" AND target_type = ${idx}"));
            idx += 1;
        }
        if let Some(v) = &filter.target_id {
            params.push(Box::new(v.clone()));
            sql.push_str(&format!(" AND target_id = ${idx}"));
            idx += 1;
        }
        if let Some(t) = filter.since {
            params.push(Box::new(t));
            sql.push_str(&format!(" AND created_at >= ${idx}"));
            idx += 1;
        }
        if let Some(t) = filter.until {
            params.push(Box::new(t));
            sql.push_str(&format!(" AND created_at <= ${idx}"));
            idx += 1;
        }
        let limit = filter.limit.unwrap_or(200).min(1000);
        params.push(Box::new(limit as i64));
        sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ${idx}"));

        let params_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = db.query(&sql, &params_ref).map_err(map_err)?;
        Ok(rows
            .iter()
            .map(|row| AuditEvent {
                event_id: row.get(0),
                actor_type: row.get(1),
                actor_id: row.get(2),
                action: row.get(3),
                target_type: row.get(4),
                target_id: row.get(5),
                diff_json: row.get(6),
                ip_address: row.get(7),
                user_agent: row.get(8),
                created_at: row.get(9),
            })
            .collect())
    }
}

// ─── JobDefinitionStore ───

impl JobDefinitionStore for PgStore {
    fn create_job_definition(&self, job: &JobDefinition) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let metadata = serde_json::to_string(&job.metadata).unwrap_or_default();
        let tags = serde_json::to_string(&job.tags).unwrap_or_else(|_| "[]".into());
        let max_retries = job.max_retries.map(|n| n as i32);
        db.execute(
            "INSERT INTO job_definitions
                (job_key, description, assigned_runner_id, is_active, metadata,
                 created_at, updated_at, timeout, max_retries, dead_letter_enabled, tags,
                 dead_letter_retention, dead_letter_operator_hint, dead_letter_replay_max_age)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT(job_key) DO UPDATE SET
                description=EXCLUDED.description,
                assigned_runner_id=EXCLUDED.assigned_runner_id,
                is_active=EXCLUDED.is_active,
                metadata=EXCLUDED.metadata,
                updated_at=EXCLUDED.updated_at,
                timeout=EXCLUDED.timeout,
                max_retries=EXCLUDED.max_retries,
                dead_letter_enabled=EXCLUDED.dead_letter_enabled,
                tags=EXCLUDED.tags,
                dead_letter_retention=EXCLUDED.dead_letter_retention,
                dead_letter_operator_hint=EXCLUDED.dead_letter_operator_hint,
                dead_letter_replay_max_age=EXCLUDED.dead_letter_replay_max_age",
            &[
                &job.job_key,
                &job.description,
                &job.assigned_runner_id,
                &job.is_active,
                &metadata,
                &job.created_at,
                &job.updated_at,
                &job.timeout,
                &max_retries,
                &job.dead_letter_enabled,
                &tags,
                &job.dead_letter_retention,
                &job.dead_letter_operator_hint,
                &job.dead_letter_replay_max_age,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_job_definition(&self, job_key: &str) -> Result<Option<JobDefinition>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT job_key, description, assigned_runner_id, is_active, metadata,
                        created_at, updated_at, timeout, max_retries, dead_letter_enabled, tags,
                        dead_letter_retention, dead_letter_operator_hint, dead_letter_replay_max_age
                 FROM job_definitions WHERE job_key = $1",
                &[&job_key],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_job_def))
    }

    fn list_job_definitions(&self) -> Result<Vec<JobDefinition>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT job_key, description, assigned_runner_id, is_active, metadata,
                        created_at, updated_at, timeout, max_retries, dead_letter_enabled, tags,
                        dead_letter_retention, dead_letter_operator_hint, dead_letter_replay_max_age
                 FROM job_definitions ORDER BY job_key",
                &[],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(row_to_job_def).collect())
    }

    fn delete_job_definition(&self, job_key: &str) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "DELETE FROM trigger_definitions WHERE job_key = $1",
            &[&job_key],
        )
        .map_err(map_err)?;
        db.execute(
            "DELETE FROM job_definitions WHERE job_key = $1",
            &[&job_key],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

// ─── TriggerDefinitionStore ───

impl TriggerDefinitionStore for PgStore {
    fn create_trigger(&self, t: &TriggerDefinition) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO trigger_definitions (trigger_id, job_key, cron_expression, timezone, calendar, \"window\", not_before, not_after, enabled, managed_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT(trigger_id) DO UPDATE SET
                job_key=EXCLUDED.job_key, cron_expression=EXCLUDED.cron_expression, timezone=EXCLUDED.timezone,
                calendar=EXCLUDED.calendar, \"window\"=EXCLUDED.\"window\", not_before=EXCLUDED.not_before,
                not_after=EXCLUDED.not_after, enabled=EXCLUDED.enabled, managed_by=EXCLUDED.managed_by,
                updated_at=EXCLUDED.updated_at",
            &[
                &t.trigger_id,
                &t.job_key,
                &t.cron_expression,
                &t.timezone,
                &t.calendar,
                &t.window,
                &t.not_before,
                &t.not_after,
                &t.enabled,
                &t.managed_by,
                &t.created_at,
                &t.updated_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_trigger(&self, trigger_id: &str) -> Result<Option<TriggerDefinition>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT trigger_id, job_key, cron_expression, timezone, calendar, \"window\", not_before, not_after, enabled, managed_by, created_at, updated_at FROM trigger_definitions WHERE trigger_id = $1",
                &[&trigger_id],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_trigger_def))
    }

    fn list_triggers(&self, job_key: Option<&str>) -> Result<Vec<TriggerDefinition>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = if let Some(jk) = job_key {
            db.query(
                "SELECT trigger_id, job_key, cron_expression, timezone, calendar, \"window\", not_before, not_after, enabled, managed_by, created_at, updated_at FROM trigger_definitions WHERE job_key = $1 ORDER BY created_at",
                &[&jk],
            )
            .map_err(map_err)?
        } else {
            db.query(
                "SELECT trigger_id, job_key, cron_expression, timezone, calendar, \"window\", not_before, not_after, enabled, managed_by, created_at, updated_at FROM trigger_definitions ORDER BY created_at",
                &[],
            )
            .map_err(map_err)?
        };
        Ok(rows.iter().map(row_to_trigger_def).collect())
    }

    fn delete_trigger(&self, trigger_id: &str) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "DELETE FROM trigger_definitions WHERE trigger_id = $1",
            &[&trigger_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn update_trigger(&self, t: &TriggerDefinition) -> Result<bool, StoreError> {
        // Scoped to `managed_by != 'dsl'` so Croniqfile-owned rows can never
        // be edited through this path; rows_affected is the found-flag the
        // handler maps to 404. Mirrors the SQLite impl.
        let mut db = self.client.lock().unwrap();
        let n = db
            .execute(
                "UPDATE trigger_definitions
                 SET cron_expression = $2, timezone = $3, calendar = $4, enabled = $5, updated_at = $6
                 WHERE trigger_id = $1 AND managed_by != 'dsl'",
                &[
                    &t.trigger_id,
                    &t.cron_expression,
                    &t.timezone,
                    &t.calendar,
                    &t.enabled,
                    &t.updated_at,
                ],
            )
            .map_err(map_err)?;
        Ok(n > 0)
    }
}

// ─── CalendarDefinitionStore ───

impl CalendarDefinitionStore for PgStore {
    fn create_calendar(&self, cal: &CalendarDefinition) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO calendar_definitions (calendar_id, name, timezone, rules, managed_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT(calendar_id) DO UPDATE SET
                name=EXCLUDED.name, timezone=EXCLUDED.timezone, rules=EXCLUDED.rules,
                managed_by=EXCLUDED.managed_by, updated_at=EXCLUDED.updated_at",
            &[
                &cal.calendar_id,
                &cal.name,
                &cal.timezone,
                &cal.rules,
                &cal.managed_by,
                &cal.created_at,
                &cal.updated_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_calendar(&self, calendar_id: &str) -> Result<Option<CalendarDefinition>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT calendar_id, name, timezone, rules, managed_by, created_at, updated_at FROM calendar_definitions WHERE calendar_id = $1",
                &[&calendar_id],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_calendar))
    }

    fn list_calendars(&self) -> Result<Vec<CalendarDefinition>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT calendar_id, name, timezone, rules, managed_by, created_at, updated_at FROM calendar_definitions ORDER BY name",
                &[],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(row_to_calendar).collect())
    }

    fn delete_calendar(&self, calendar_id: &str) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "DELETE FROM calendar_definitions WHERE calendar_id = $1 AND managed_by != 'dsl'",
            &[&calendar_id],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

// ─── ExecutionLogStore ───

impl ExecutionLogStore for PgStore {
    fn append_log(&self, entry: &ExecutionLogEntry) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let next: Option<i64> = db
            .query_one(
                "SELECT MAX(seq) FROM execution_logs WHERE execution_id = $1",
                &[&entry.execution_id],
            )
            .map_err(map_err)?
            .get(0);
        let seq = next.map(|m| m + 1).unwrap_or(0);
        let fields = serde_json::to_string(&entry.fields).unwrap_or_default();
        db.execute(
            "INSERT INTO execution_logs (id, execution_id, timestamp, level, message, fields, seq)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                &entry.id,
                &entry.execution_id,
                &entry.timestamp,
                &entry.level,
                &entry.message,
                &fields,
                &seq,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn append_logs_batch(&self, entries: &[ExecutionLogEntry]) -> Result<(), StoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut db = self.client.lock().unwrap();
        let mut tx = db.transaction().map_err(map_err)?;
        // Group by execution_id so each group gets a contiguous seq range.
        let mut next_seq_by_exec: HashMap<Uuid, i64> = HashMap::new();
        for entry in entries {
            let seq = match next_seq_by_exec.get(&entry.execution_id).copied() {
                Some(n) => n,
                None => {
                    let max: Option<i64> = tx
                        .query_one(
                            "SELECT MAX(seq) FROM execution_logs WHERE execution_id = $1",
                            &[&entry.execution_id],
                        )
                        .map_err(map_err)?
                        .get(0);
                    max.map(|m| m + 1).unwrap_or(0)
                }
            };
            let fields = serde_json::to_string(&entry.fields).unwrap_or_default();
            tx.execute(
                "INSERT INTO execution_logs (id, execution_id, timestamp, level, message, fields, seq)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &entry.id,
                    &entry.execution_id,
                    &entry.timestamp,
                    &entry.level,
                    &entry.message,
                    &fields,
                    &seq,
                ],
            )
            .map_err(map_err)?;
            next_seq_by_exec.insert(entry.execution_id, seq + 1);
        }
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn read_logs(
        &self,
        execution_id: Uuid,
        limit: u32,
    ) -> Result<Vec<ExecutionLogEntry>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT id, execution_id, timestamp, level, message, fields, seq
                 FROM execution_logs WHERE execution_id = $1
                 ORDER BY timestamp ASC, seq ASC LIMIT $2",
                &[&execution_id, &(limit as i64)],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(row_to_execution_log).collect())
    }
}

// ─── DslAdoptionStore ───

impl DslAdoptionStore for PgStore {
    fn insert_adoption(&self, adoption: &DslAdoption) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO dsl_adoptions (resource_type, resource_key, adopted_at, adopted_by)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT(resource_type, resource_key) DO UPDATE SET
                adopted_at = EXCLUDED.adopted_at,
                adopted_by = EXCLUDED.adopted_by",
            &[
                &adoption.resource_type,
                &adoption.resource_key,
                &adoption.adopted_at,
                &adoption.adopted_by,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn delete_adoption(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError> {
        let mut db = self.client.lock().unwrap();
        let n = db
            .execute(
                "DELETE FROM dsl_adoptions WHERE resource_type = $1 AND resource_key = $2",
                &[&resource_type, &resource_key],
            )
            .map_err(map_err)?;
        Ok(n > 0)
    }

    fn is_adopted(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError> {
        let mut db = self.client.lock().unwrap();
        let row = db
            .query_one(
                "SELECT COUNT(*) FROM dsl_adoptions WHERE resource_type = $1 AND resource_key = $2",
                &[&resource_type, &resource_key],
            )
            .map_err(map_err)?;
        Ok(row.get::<_, i64>(0) > 0)
    }

    fn list_adoptions(&self, resource_type: &str) -> Result<Vec<DslAdoption>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT resource_type, resource_key, adopted_at, adopted_by
                 FROM dsl_adoptions WHERE resource_type = $1 ORDER BY resource_key",
                &[&resource_type],
            )
            .map_err(map_err)?;
        Ok(rows
            .iter()
            .map(|row| DslAdoption {
                resource_type: row.get(0),
                resource_key: row.get(1),
                adopted_at: row.get(2),
                adopted_by: row.get(3),
            })
            .collect())
    }
}

// ─── AlertStore ───

impl AlertStore for PgStore {
    fn record_alert_delivery(&self, d: &AlertDelivery) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO alert_deliveries
                (delivery_id, rule_name, channel_name, job_key, execution_id,
                 state, error, fired_at, delivered_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (delivery_id) DO NOTHING",
            &[
                &d.delivery_id,
                &d.rule_name,
                &d.channel_name,
                &d.job_key,
                &d.execution_id,
                &d.state.as_str(),
                &d.error,
                &d.fired_at,
                &d.delivered_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn list_alert_deliveries(
        &self,
        filter: &AlertDeliveryFilter,
    ) -> Result<Vec<AlertDelivery>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let mut sql = String::from(
            "SELECT delivery_id, rule_name, channel_name, job_key, execution_id,
                    state, error, fired_at, delivered_at
             FROM alert_deliveries WHERE true",
        );
        let mut params: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();
        let mut idx = 1;
        if let Some(ref jk) = filter.job_key {
            params.push(Box::new(jk.clone()));
            sql.push_str(&format!(" AND job_key = ${idx}"));
            idx += 1;
        }
        if let Some(ref rn) = filter.rule_name {
            params.push(Box::new(rn.clone()));
            sql.push_str(&format!(" AND rule_name = ${idx}"));
            idx += 1;
        }
        if let Some(since) = filter.since {
            params.push(Box::new(since));
            sql.push_str(&format!(" AND fired_at >= ${idx}"));
            idx += 1;
        }
        let limit = filter.limit.unwrap_or(200);
        params.push(Box::new(limit as i64));
        sql.push_str(&format!(" ORDER BY fired_at DESC LIMIT ${idx}"));

        let params_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = db.query(&sql, &params_ref).map_err(map_err)?;
        Ok(rows.iter().map(row_to_alert_delivery).collect())
    }

    fn last_alert_fire_at(
        &self,
        rule_name: &str,
        job_key: &str,
    ) -> Result<Option<DateTime<Utc>>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT fired_at FROM alert_deliveries
                 WHERE rule_name = $1 AND job_key = $2
                 ORDER BY fired_at DESC LIMIT 1",
                &[&rule_name, &job_key],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(|row| row.get(0)))
    }

    fn get_alert_delivery(&self, delivery_id: &str) -> Result<Option<AlertDelivery>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT delivery_id, rule_name, channel_name, job_key, execution_id,
                        state, error, fired_at, delivered_at
                 FROM alert_deliveries WHERE delivery_id = $1",
                &[&delivery_id],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_alert_delivery))
    }

    fn upsert_alert_rule_override(&self, ov: &AlertRuleOverride) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        let throttle_secs = ov.throttle_secs.map(|s| s as i64);
        db.execute(
            "INSERT INTO alert_rule_overrides
                (rule_name, enabled, snooze_until, throttle_secs,
                 note, set_by_user_id, set_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT(rule_name) DO UPDATE SET
                enabled        = EXCLUDED.enabled,
                snooze_until   = EXCLUDED.snooze_until,
                throttle_secs  = EXCLUDED.throttle_secs,
                note           = EXCLUDED.note,
                set_by_user_id = EXCLUDED.set_by_user_id,
                set_at         = EXCLUDED.set_at,
                expires_at     = EXCLUDED.expires_at",
            &[
                &ov.rule_name,
                &ov.enabled,
                &ov.snooze_until,
                &throttle_secs,
                &ov.note,
                &ov.set_by_user_id,
                &ov.set_at,
                &ov.expires_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_alert_rule_override(
        &self,
        rule_name: &str,
    ) -> Result<Option<AlertRuleOverride>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT rule_name, enabled, snooze_until, throttle_secs,
                        note, set_by_user_id, set_at, expires_at
                 FROM alert_rule_overrides WHERE rule_name = $1",
                &[&rule_name],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_alert_rule_override))
    }

    fn list_alert_rule_overrides(&self) -> Result<Vec<AlertRuleOverride>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT rule_name, enabled, snooze_until, throttle_secs,
                        note, set_by_user_id, set_at, expires_at
                 FROM alert_rule_overrides ORDER BY set_at DESC",
                &[],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(row_to_alert_rule_override).collect())
    }

    fn delete_alert_rule_override(&self, rule_name: &str) -> Result<bool, StoreError> {
        let mut db = self.client.lock().unwrap();
        let n = db
            .execute(
                "DELETE FROM alert_rule_overrides WHERE rule_name = $1",
                &[&rule_name],
            )
            .map_err(map_err)?;
        Ok(n > 0)
    }

    fn delete_expired_alert_rule_overrides(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<String>, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "DELETE FROM alert_rule_overrides
                 WHERE expires_at IS NOT NULL AND expires_at <= $1
                 RETURNING rule_name",
                &[&now],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    fn prune_alert_rule_overrides(
        &self,
        valid_rule_names: &[String],
    ) -> Result<Vec<String>, StoreError> {
        let mut db = self.client.lock().unwrap();
        // `<> ALL($1)` deletes every override whose rule_name is absent from
        // the valid set; an empty set removes all rows (every rule is an
        // orphan), matching the SQLite impl.
        let valid: Vec<String> = valid_rule_names.to_vec();
        let rows = db
            .query(
                "DELETE FROM alert_rule_overrides WHERE rule_name <> ALL($1) RETURNING rule_name",
                &[&valid],
            )
            .map_err(map_err)?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }
}

impl MaintenanceStore for PgStore {
    fn get_maintenance(&self) -> Result<MaintenanceState, StoreError> {
        let mut db = self.client.lock().unwrap();
        let rows = db
            .query(
                "SELECT manual_active, window_start, window_end, note, updated_by, updated_at
                 FROM maintenance WHERE id = 1",
                &[],
            )
            .map_err(map_err)?;
        Ok(rows.first().map(row_to_maintenance).unwrap_or_default())
    }

    fn set_maintenance(&self, state: &MaintenanceState) -> Result<(), StoreError> {
        let mut db = self.client.lock().unwrap();
        db.execute(
            "INSERT INTO maintenance
                (id, manual_active, window_start, window_end, note, updated_by, updated_at)
             VALUES (1, $1, $2, $3, $4, $5, $6)
             ON CONFLICT(id) DO UPDATE SET
                manual_active = EXCLUDED.manual_active,
                window_start  = EXCLUDED.window_start,
                window_end    = EXCLUDED.window_end,
                note          = EXCLUDED.note,
                updated_by    = EXCLUDED.updated_by,
                updated_at    = EXCLUDED.updated_at",
            &[
                &state.manual_active,
                &state.window_start,
                &state.window_end,
                &state.note,
                &state.updated_by,
                &state.updated_at,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

impl Store for PgStore {}

// ─── Row mappers ───

fn row_to_execution(row: &postgres::Row) -> Execution {
    let metadata_json: serde_json::Value = row.get(12);
    let fire_at = row.get(2);
    Execution {
        id: row.get(0),
        job_key: row.get(1),
        fire_at,
        // NULL for rows written before migration 022 — fall back to fire_at.
        scheduled_for: row.get::<_, Option<_>>(15).unwrap_or(fire_at),
        attempt: row.get::<_, i32>(3) as u32,
        state: parse_execution_state(&row.get::<_, String>(4)),
        runner_id: row.get(5),
        claimed_at: row.get(6),
        started_at: row.get(7),
        completed_at: row.get(8),
        duration_ms: row.get(9),
        error: row.get(10),
        dead_reason: row.get(11),
        idempotency_key: row.get(14),
        metadata: json_to_metadata(metadata_json),
        created_at: row.get(13),
    }
}

fn row_to_runner(row: &postgres::Row) -> Runner {
    let caps_json: serde_json::Value = row.get(1);
    let inflight_json: serde_json::Value = row.get(4);
    Runner {
        runner_id: row.get(0),
        capabilities: serde_json::from_value(caps_json).unwrap_or_default(),
        max_inflight: row.get::<_, i32>(2) as u32,
        last_poll_at: row.get(3),
        inflight: serde_json::from_value(inflight_json).unwrap_or_default(),
        status: parse_runner_status(&row.get::<_, String>(5)),
        registered_at: row.get(6),
    }
}

fn row_to_dead_letter(row: &postgres::Row) -> DeadLetter {
    let metadata_json: serde_json::Value = row.get(7);
    let fire_at = row.get(3);
    DeadLetter {
        id: row.get(0),
        execution_id: row.get(1),
        job_key: row.get(2),
        fire_at,
        // NULL for rows written before migration 022 — fall back to fire_at.
        scheduled_for: row.get::<_, Option<_>>(10).unwrap_or(fire_at),
        attempt: row.get::<_, i32>(4) as u32,
        error: row.get(5),
        dead_reason: row.get(6),
        metadata: json_to_metadata(metadata_json),
        created_at: row.get(8),
        expires_at: row.get(9),
    }
}

fn row_to_api_client(row: &postgres::Row) -> ApiClient {
    let scopes_str: String = row.get(2);
    ApiClient {
        client_id: row.get(0),
        name: row.get(1),
        scopes: serde_json::from_str(&scopes_str).unwrap_or_default(),
        is_active: row.get(3),
        created_at: row.get(4),
        managed_by: row.get(5),
    }
}

fn row_to_api_key(row: &postgres::Row) -> ApiKey {
    ApiKey {
        key_id: row.get(0),
        client_id: row.get(1),
        key_hash: row.get(2),
        key_prefix: row.get(3),
        expires_at: row.get(4),
        revoked_at: row.get(5),
        created_at: row.get(6),
    }
}

fn row_to_user(row: &postgres::Row) -> Result<User, StoreError> {
    let role_str: String = row.get(4);
    let role = role_str
        .parse::<Role>()
        .map_err(|_| StoreError::Database(format!("unknown role: {role_str}")))?;
    Ok(User {
        user_id: row.get(0),
        username: row.get(1),
        email: row.get(2),
        display_name: row.get(3),
        role,
        is_active: row.get(5),
        created_at: row.get(6),
        updated_at: row.get(7),
        last_login_at: row.get(8),
    })
}

fn row_to_invitation(row: &postgres::Row) -> Result<Invitation, StoreError> {
    let role_str: String = row.get(2);
    let role = role_str
        .parse::<Role>()
        .map_err(|_| StoreError::Database(format!("unknown role: {role_str}")))?;
    Ok(Invitation {
        invitation_id: row.get(0),
        email: row.get(1),
        role,
        token_hash: row.get(3),
        invited_by: row.get(4),
        expires_at: row.get(5),
        accepted_at: row.get(6),
        revoked_at: row.get(7),
        created_at: row.get(8),
    })
}

fn row_to_pat(row: &postgres::Row) -> PersonalAccessToken {
    let scopes_str: String = row.get(5);
    PersonalAccessToken {
        token_id: row.get(0),
        user_id: row.get(1),
        name: row.get(2),
        token_hash: row.get(3),
        token_prefix: row.get(4),
        scopes: serde_json::from_str(&scopes_str).unwrap_or_default(),
        expires_at: row.get(6),
        revoked_at: row.get(7),
        last_used_at: row.get(8),
        created_at: row.get(9),
    }
}

fn row_to_oidc_identity(row: &postgres::Row) -> OidcIdentity {
    OidcIdentity {
        provider: row.get(0),
        subject: row.get(1),
        user_id: row.get(2),
        email: row.get(3),
        linked_at: row.get(4),
        last_login_at: row.get(5),
    }
}

fn row_to_job_def(row: &postgres::Row) -> JobDefinition {
    let meta_str: String = row.get(4);
    let tags_str: String = row.get(10);
    JobDefinition {
        job_key: row.get(0),
        description: row.get(1),
        assigned_runner_id: row.get(2),
        is_active: row.get(3),
        metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
        created_at: row.get(5),
        updated_at: row.get(6),
        timeout: row.get(7),
        max_retries: row.get::<_, Option<i32>>(8).map(|n| n as u32),
        dead_letter_enabled: row.get(9),
        tags: serde_json::from_str(&tags_str).unwrap_or_default(),
        dead_letter_retention: row.get(11),
        dead_letter_operator_hint: row.get(12),
        dead_letter_replay_max_age: row.get(13),
    }
}

fn row_to_trigger_def(row: &postgres::Row) -> TriggerDefinition {
    TriggerDefinition {
        trigger_id: row.get(0),
        job_key: row.get(1),
        cron_expression: row.get(2),
        timezone: row.get(3),
        calendar: row.get(4),
        window: row.get(5),
        not_before: row.get(6),
        not_after: row.get(7),
        enabled: row.get(8),
        managed_by: row.get(9),
        created_at: row.get(10),
        updated_at: row.get(11),
    }
}

fn row_to_calendar(row: &postgres::Row) -> CalendarDefinition {
    CalendarDefinition {
        calendar_id: row.get(0),
        name: row.get(1),
        timezone: row.get(2),
        rules: row.get(3),
        managed_by: row.get(4),
        created_at: row.get(5),
        updated_at: row.get(6),
    }
}

fn row_to_execution_log(row: &postgres::Row) -> ExecutionLogEntry {
    let fields_str: String = row.get(5);
    ExecutionLogEntry {
        id: row.get(0),
        execution_id: row.get(1),
        timestamp: row.get(2),
        level: row.get(3),
        message: row.get(4),
        fields: serde_json::from_str(&fields_str).unwrap_or_default(),
        seq: row.get(6),
    }
}

fn row_to_alert_delivery(row: &postgres::Row) -> AlertDelivery {
    let state_str: String = row.get(5);
    let state = AlertDeliveryState::parse_db(&state_str).unwrap_or(AlertDeliveryState::Failed);
    AlertDelivery {
        delivery_id: row.get(0),
        rule_name: row.get(1),
        channel_name: row.get(2),
        job_key: row.get(3),
        execution_id: row.get(4),
        state,
        error: row.get(6),
        fired_at: row.get(7),
        delivered_at: row.get(8),
    }
}

fn row_to_alert_rule_override(row: &postgres::Row) -> AlertRuleOverride {
    AlertRuleOverride {
        rule_name: row.get(0),
        enabled: row.get(1),
        snooze_until: row.get(2),
        throttle_secs: row.get::<_, Option<i64>>(3).map(|s| s as u64),
        note: row.get(4),
        set_by_user_id: row.get(5),
        set_at: row.get(6),
        expires_at: row.get(7),
    }
}

fn row_to_maintenance(row: &postgres::Row) -> MaintenanceState {
    MaintenanceState {
        manual_active: row.get(0),
        window_start: row.get(1),
        window_end: row.get(2),
        note: row.get(3),
        updated_by: row.get(4),
        updated_at: row.get(5),
    }
}

// ─── String conversions ───

fn state_to_str(state: ExecutionState) -> &'static str {
    match state {
        ExecutionState::Queued => "queued",
        ExecutionState::Claimed => "claimed",
        ExecutionState::Completed => "completed",
        ExecutionState::Failed => "failed",
        ExecutionState::Dead => "dead",
        ExecutionState::Cancelled => "cancelled",
    }
}

fn parse_execution_state(s: &str) -> ExecutionState {
    match s {
        "queued" => ExecutionState::Queued,
        "claimed" => ExecutionState::Claimed,
        "completed" => ExecutionState::Completed,
        "failed" => ExecutionState::Failed,
        "dead" => ExecutionState::Dead,
        "cancelled" => ExecutionState::Cancelled,
        _ => ExecutionState::Queued,
    }
}

fn job_status_to_str(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Active => "active",
        JobStatus::Paused => "paused",
        JobStatus::Disabled => "disabled",
        JobStatus::Exhausted => "exhausted",
    }
}

fn parse_job_status(s: &str) -> JobStatus {
    match s {
        "active" => JobStatus::Active,
        "paused" => JobStatus::Paused,
        "disabled" => JobStatus::Disabled,
        "exhausted" => JobStatus::Exhausted,
        _ => JobStatus::Active,
    }
}

fn runner_status_to_str(status: RunnerStatus) -> &'static str {
    match status {
        RunnerStatus::Online => "online",
        RunnerStatus::Stale => "stale",
        RunnerStatus::Dead => "dead",
    }
}

fn parse_runner_status(s: &str) -> RunnerStatus {
    match s {
        "online" => RunnerStatus::Online,
        "stale" => RunnerStatus::Stale,
        "dead" => RunnerStatus::Dead,
        _ => RunnerStatus::Online,
    }
}

// ─── Reusable insert/upsert helpers ───
//
// `postgres::Client` and `postgres::Transaction` don't share a trait we can
// use generically without bringing in `GenericClient`, so we duplicate the
// statement bodies into thin wrappers over each. The SQL is identical.

fn pg_insert_execution(client: &mut postgres::Client, exec: &Execution) -> Result<(), StoreError> {
    let metadata = metadata_to_json(&exec.metadata);
    client
        .execute(
            PG_INSERT_EXECUTION_SQL,
            &[
                &exec.id,
                &exec.job_key,
                &exec.fire_at,
                &(exec.attempt as i32),
                &state_to_str(exec.state),
                &exec.runner_id,
                &exec.claimed_at,
                &exec.started_at,
                &exec.completed_at,
                &exec.duration_ms,
                &exec.error,
                &exec.dead_reason,
                &metadata,
                &exec.created_at,
                &exec.idempotency_key,
                &exec.scheduled_for,
            ],
        )
        .map_err(map_err)?;
    Ok(())
}

fn pg_insert_execution_tx(
    tx: &mut postgres::Transaction<'_>,
    exec: &Execution,
) -> Result<(), StoreError> {
    let metadata = metadata_to_json(&exec.metadata);
    tx.execute(
        PG_INSERT_EXECUTION_SQL,
        &[
            &exec.id,
            &exec.job_key,
            &exec.fire_at,
            &(exec.attempt as i32),
            &state_to_str(exec.state),
            &exec.runner_id,
            &exec.claimed_at,
            &exec.started_at,
            &exec.completed_at,
            &exec.duration_ms,
            &exec.error,
            &exec.dead_reason,
            &metadata,
            &exec.created_at,
            &exec.idempotency_key,
            &exec.scheduled_for,
        ],
    )
    .map_err(map_err)?;
    Ok(())
}

fn pg_upsert_job_state_tx(
    tx: &mut postgres::Transaction<'_>,
    state: &JobState,
) -> Result<(), StoreError> {
    let status = job_status_to_str(state.status);
    tx.execute(
        PG_UPSERT_JOB_STATE_SQL,
        &[
            &state.job_key,
            &state.next_fire_at,
            &state.last_fired_at,
            &(state.fire_count as i64),
            &status,
            &state.updated_at,
        ],
    )
    .map_err(map_err)?;
    Ok(())
}

const PG_INSERT_EXECUTION_SQL: &str = "INSERT INTO executions (id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)";

const PG_INSERT_DEAD_LETTER_SQL: &str = "INSERT INTO dead_letters (id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at, scheduled_for)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)";

const PG_UPSERT_JOB_STATE_SQL: &str =
    "INSERT INTO job_states (job_key, next_fire_at, last_fired_at, fire_count, status, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6)
     ON CONFLICT(job_key) DO UPDATE SET
       next_fire_at = EXCLUDED.next_fire_at,
       last_fired_at = EXCLUDED.last_fired_at,
       fire_count = EXCLUDED.fire_count,
       status = EXCLUDED.status,
       updated_at = EXCLUDED.updated_at";
