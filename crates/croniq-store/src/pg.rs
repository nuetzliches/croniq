//! PostgreSQL store implementation.
//!
//! Requires the `postgres` feature. Uses the synchronous `postgres` crate
//! to match the synchronous store trait signatures.

use crate::models::*;
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
    pub fn connect(connection_string: &str) -> Result<Self, StoreError> {
        let client = Client::connect(connection_string, NoTls)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let store = Self {
            client: Mutex::new(client),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS _migrations (
                    name TEXT PRIMARY KEY,
                    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
                );",
            )
            .map_err(map_err)?;

        let applied: Vec<String> = client
            .query("SELECT name FROM _migrations", &[])
            .map_err(map_err)?
            .iter()
            .map(|row| row.get(0))
            .collect();

        if !applied.contains(&"001_initial".to_string()) {
            client.batch_execute(PG_MIGRATION_001).map_err(map_err)?;
            client
                .execute(
                    "INSERT INTO _migrations (name) VALUES ($1)",
                    &[&"001_initial"],
                )
                .map_err(map_err)?;
        }

        if !applied.contains(&"005_perf_indexes".to_string()) {
            // Mirrors the SQLite migration: drop the single-column state
            // index, replace it with a (state, fire_at) composite, and add
            // a created_at index for the list-executions endpoint. See the
            // SQLite migration for the rationale.
            client.batch_execute(PG_MIGRATION_005).map_err(map_err)?;
            client
                .execute(
                    "INSERT INTO _migrations (name) VALUES ($1)",
                    &[&"005_perf_indexes"],
                )
                .map_err(map_err)?;
        }

        Ok(())
    }
}

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
            .filter_map(|(k, v)| match v {
                serde_json::Value::String(s) => Some((k, s)),
                _ => Some((k, v.to_string())),
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
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at FROM executions WHERE id = $1",
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
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at FROM executions WHERE id = $1",
                &[&id],
            )
            .map_err(map_err)?;

        rows.first()
            .map(row_to_execution)
            .ok_or_else(|| StoreError::NotFound(format!("execution {id}")))
    }

    fn complete_execution(
        &self,
        id: Uuid,
        state: ExecutionState,
        duration_ms: Option<i64>,
        error: Option<&str>,
        dead_reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        client
            .execute(
                "UPDATE executions SET state = $1, completed_at = $2, duration_ms = $3, error = $4, dead_reason = $5 WHERE id = $6",
                &[&state_to_str(state), &now, &duration_ms, &error, &dead_reason, &id],
            )
            .map_err(map_err)?;
        Ok(())
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
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at FROM executions WHERE state = 'queued' ORDER BY fire_at ASC LIMIT $1",
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
            "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at FROM executions WHERE true",
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
}

// ─── DeadLetterStore ───

impl DeadLetterStore for PgStore {
    fn add_dead_letter(&self, dl: &DeadLetter) -> Result<(), StoreError> {
        let mut client = self.client.lock().unwrap();
        let metadata = metadata_to_json(&dl.metadata);
        client
            .execute(
                "INSERT INTO dead_letters (id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                &[
                    &dl.id,
                    &dl.execution_id,
                    &dl.job_key,
                    &dl.fire_at,
                    &(dl.attempt as i32),
                    &dl.error,
                    &dl.dead_reason,
                    &metadata,
                    &dl.created_at,
                    &dl.expires_at,
                ],
            )
            .map_err(map_err)?;
        Ok(())
    }

    fn get_dead_letter(&self, id: Uuid) -> Result<Option<DeadLetter>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let rows = client
            .query(
                "SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at FROM dead_letters WHERE id = $1",
                &[&id],
            )
            .map_err(map_err)?;

        Ok(rows.first().map(row_to_dead_letter))
    }

    fn list_dead_letters(&self, filter: &DeadLetterFilter) -> Result<Vec<DeadLetter>, StoreError> {
        let mut client = self.client.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at FROM dead_letters WHERE true",
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

impl Store for PgStore {}

// ─── Row mappers ───

fn row_to_execution(row: &postgres::Row) -> Execution {
    let metadata_json: serde_json::Value = row.get(12);
    Execution {
        id: row.get(0),
        job_key: row.get(1),
        fire_at: row.get(2),
        attempt: row.get::<_, i32>(3) as u32,
        state: parse_execution_state(&row.get::<_, String>(4)),
        runner_id: row.get(5),
        claimed_at: row.get(6),
        started_at: row.get(7),
        completed_at: row.get(8),
        duration_ms: row.get(9),
        error: row.get(10),
        dead_reason: row.get(11),
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
    DeadLetter {
        id: row.get(0),
        execution_id: row.get(1),
        job_key: row.get(2),
        fire_at: row.get(3),
        attempt: row.get::<_, i32>(4) as u32,
        error: row.get(5),
        dead_reason: row.get(6),
        metadata: json_to_metadata(metadata_json),
        created_at: row.get(8),
        expires_at: row.get(9),
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

const PG_INSERT_EXECUTION_SQL: &str = "INSERT INTO executions (id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)";

const PG_UPSERT_JOB_STATE_SQL: &str =
    "INSERT INTO job_states (job_key, next_fire_at, last_fired_at, fire_count, status, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6)
     ON CONFLICT(job_key) DO UPDATE SET
       next_fire_at = EXCLUDED.next_fire_at,
       last_fired_at = EXCLUDED.last_fired_at,
       fire_count = EXCLUDED.fire_count,
       status = EXCLUDED.status,
       updated_at = EXCLUDED.updated_at";
