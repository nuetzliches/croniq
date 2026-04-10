//! SQLite store implementation with WAL mode.

use crate::migrations;
use crate::models::*;
use crate::traits::*;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

/// SQLite-backed store.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open or create a SQLite database at the given path.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path).map_err(|e| StoreError::Database(e.to_string()))?;
        Self::init(conn)
    }

    /// Create an in-memory SQLite database (useful for testing).
    pub fn in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(|e| StoreError::Database(e.to_string()))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")
            .map_err(|e| StoreError::Database(e.to_string()))?;
        migrations::migrate(&conn).map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

// ─── Helpers ───

fn dt_to_sql(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn opt_dt_to_sql(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.map(|d| d.to_rfc3339())
}

fn sql_to_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .expect("invalid datetime in DB")
        .with_timezone(&Utc)
}

fn sql_to_opt_dt(s: Option<String>) -> Option<DateTime<Utc>> {
    s.map(|v| sql_to_dt(&v))
}

fn map_err(e: rusqlite::Error) -> StoreError {
    StoreError::Database(e.to_string())
}

// ─── JobStore ───

impl JobStore for SqliteStore {
    fn get_job_state(&self, job_key: &str) -> Result<Option<JobState>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT job_key, next_fire_at, last_fired_at, fire_count, status, updated_at FROM job_states WHERE job_key = ?1")
            .map_err(map_err)?;

        let result = stmt
            .query_row(params![job_key], |row| {
                Ok(JobState {
                    job_key: row.get(0)?,
                    next_fire_at: sql_to_opt_dt(row.get(1)?),
                    last_fired_at: sql_to_opt_dt(row.get(2)?),
                    fire_count: row.get::<_, i64>(3)? as u64,
                    status: parse_job_status(&row.get::<_, String>(4)?),
                    updated_at: sql_to_dt(&row.get::<_, String>(5)?),
                })
            })
            .optional()
            .map_err(map_err)?;

        Ok(result)
    }

    fn upsert_job_state(&self, state: &JobState) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO job_states (job_key, next_fire_at, last_fired_at, fire_count, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(job_key) DO UPDATE SET
               next_fire_at = excluded.next_fire_at,
               last_fired_at = excluded.last_fired_at,
               fire_count = excluded.fire_count,
               status = excluded.status,
               updated_at = excluded.updated_at",
            params![
                state.job_key,
                opt_dt_to_sql(&state.next_fire_at),
                opt_dt_to_sql(&state.last_fired_at),
                state.fire_count as i64,
                format!("{:?}", state.status).to_lowercase(),
                dt_to_sql(&state.updated_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn list_job_states(&self) -> Result<Vec<JobState>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT job_key, next_fire_at, last_fired_at, fire_count, status, updated_at FROM job_states ORDER BY job_key")
            .map_err(map_err)?;

        let rows = stmt
            .query_map([], |row| {
                Ok(JobState {
                    job_key: row.get(0)?,
                    next_fire_at: sql_to_opt_dt(row.get(1)?),
                    last_fired_at: sql_to_opt_dt(row.get(2)?),
                    fire_count: row.get::<_, i64>(3)? as u64,
                    status: parse_job_status(&row.get::<_, String>(4)?),
                    updated_at: sql_to_dt(&row.get::<_, String>(5)?),
                })
            })
            .map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn delete_job_state(&self, job_key: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM job_states WHERE job_key = ?1", params![job_key])
            .map_err(map_err)?;
        Ok(())
    }
}

// ─── ExecutionStore ───

impl ExecutionStore for SqliteStore {
    fn create_execution(&self, exec: &Execution) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let metadata = serde_json::to_string(&exec.metadata).unwrap_or_default();
        conn.execute(
            "INSERT INTO executions (id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                exec.id.to_string(),
                exec.job_key,
                dt_to_sql(&exec.fire_at),
                exec.attempt,
                state_to_str(exec.state),
                exec.runner_id,
                opt_dt_to_sql(&exec.claimed_at),
                opt_dt_to_sql(&exec.started_at),
                opt_dt_to_sql(&exec.completed_at),
                exec.duration_ms,
                exec.error,
                exec.dead_reason,
                metadata,
                dt_to_sql(&exec.created_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_execution(&self, id: Uuid) -> Result<Option<Execution>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at FROM executions WHERE id = ?1")
            .map_err(map_err)?;

        stmt.query_row(params![id.to_string()], |row| row_to_execution(row))
            .optional()
            .map_err(map_err)
    }

    fn claim_execution(
        &self,
        id: Uuid,
        runner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Execution, StoreError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE executions SET state = 'claimed', runner_id = ?1, claimed_at = ?2, started_at = ?2 WHERE id = ?3 AND state = 'queued'",
                params![runner_id, dt_to_sql(&now), id.to_string()],
            )
            .map_err(map_err)?;

        if affected == 0 {
            return Err(StoreError::Conflict(format!(
                "execution {id} is not in queued state"
            )));
        }

        drop(conn);
        self.get_execution(id)?
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE executions SET state = ?1, completed_at = ?2, duration_ms = ?3, error = ?4, dead_reason = ?5 WHERE id = ?6",
            params![
                state_to_str(state),
                dt_to_sql(&now),
                duration_ms,
                error,
                dead_reason,
                id.to_string(),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn find_queued_executions(
        &self,
        capabilities: &[String],
        limit: u32,
    ) -> Result<Vec<Execution>, StoreError> {
        let conn = self.conn.lock().unwrap();
        // Fetch more than limit to allow for post-filtering by capabilities.
        // If capabilities is empty, all executions match (no filtering needed).
        let fetch_limit = if capabilities.is_empty() { limit } else { limit * 4 };
        let mut stmt = conn
            .prepare("SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at FROM executions WHERE state = 'queued' ORDER BY fire_at ASC LIMIT ?1")
            .map_err(map_err)?;

        let rows = stmt
            .query_map(params![fetch_limit], |row| row_to_execution(row))
            .map_err(map_err)?;

        let all: Vec<Execution> = rows.collect::<Result<Vec<_>, _>>().map_err(map_err)?;

        if capabilities.is_empty() {
            return Ok(all);
        }

        // Post-filter: execution matches if its __require caps are all present in runner capabilities
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
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from("SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at FROM executions WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref jk) = filter.job_key {
            param_values.push(Box::new(jk.clone()));
            sql.push_str(&format!(" AND job_key = ?{}", param_values.len()));
        }
        if let Some(state) = filter.state {
            param_values.push(Box::new(state_to_str(state).to_string()));
            sql.push_str(&format!(" AND state = ?{}", param_values.len()));
        }
        if let Some(ref rid) = filter.runner_id {
            param_values.push(Box::new(rid.clone()));
            sql.push_str(&format!(" AND runner_id = ?{}", param_values.len()));
        }
        if let Some(since) = filter.since {
            param_values.push(Box::new(dt_to_sql(&since)));
            sql.push_str(&format!(" AND created_at >= ?{}", param_values.len()));
        }
        if let Some(until) = filter.until {
            param_values.push(Box::new(dt_to_sql(&until)));
            sql.push_str(&format!(" AND created_at <= ?{}", param_values.len()));
        }

        sql.push_str(" ORDER BY created_at DESC");

        let limit = filter.limit.unwrap_or(100);
        param_values.push(Box::new(limit));
        sql.push_str(&format!(" LIMIT ?{}", param_values.len()));

        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_ref.as_slice(), |row| row_to_execution(row))
            .map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn requeue_abandoned(
        &self,
        runner_id: &str,
        _now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM executions WHERE runner_id = ?1 AND state = 'claimed'")
            .map_err(map_err)?;

        let ids: Vec<Uuid> = stmt
            .query_map(params![runner_id], |row| {
                let id_str: String = row.get(0)?;
                Ok(Uuid::parse_str(&id_str).unwrap())
            })
            .map_err(map_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_err)?;

        if !ids.is_empty() {
            conn.execute(
                "UPDATE executions SET state = 'queued', runner_id = NULL, claimed_at = NULL, started_at = NULL WHERE runner_id = ?1 AND state = 'claimed'",
                params![runner_id],
            )
            .map_err(map_err)?;
        }

        Ok(ids)
    }

    fn cancel_execution(&self, id: Uuid, now: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE executions SET state = 'cancelled', completed_at = ?1 WHERE id = ?2 AND state IN ('queued', 'claimed')",
            params![dt_to_sql(&now), id.to_string()],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn count_by_state(&self) -> Result<HashMap<ExecutionState, u64>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT state, COUNT(*) FROM executions GROUP BY state")
            .map_err(map_err)?;

        let rows = stmt
            .query_map([], |row| {
                let state_str: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((parse_execution_state(&state_str), count as u64))
            })
            .map_err(map_err)?;

        let mut map = HashMap::new();
        for row in rows {
            let (state, count) = row.map_err(map_err)?;
            map.insert(state, count);
        }
        Ok(map)
    }
}

// ─── RunnerStore ───

impl RunnerStore for SqliteStore {
    fn upsert_runner(&self, runner: &Runner) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let caps = serde_json::to_string(&runner.capabilities).unwrap();
        let inflight = serde_json::to_string(&runner.inflight).unwrap();
        conn.execute(
            "INSERT INTO runners (runner_id, capabilities, max_inflight, last_poll_at, inflight, status, registered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(runner_id) DO UPDATE SET
               capabilities = excluded.capabilities,
               max_inflight = excluded.max_inflight,
               last_poll_at = excluded.last_poll_at,
               inflight = excluded.inflight,
               status = excluded.status",
            params![
                runner.runner_id,
                caps,
                runner.max_inflight,
                dt_to_sql(&runner.last_poll_at),
                inflight,
                runner_status_to_str(runner.status),
                dt_to_sql(&runner.registered_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_runner(&self, runner_id: &str) -> Result<Option<Runner>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT runner_id, capabilities, max_inflight, last_poll_at, inflight, status, registered_at FROM runners WHERE runner_id = ?1")
            .map_err(map_err)?;

        stmt.query_row(params![runner_id], |row| row_to_runner(row))
            .optional()
            .map_err(map_err)
    }

    fn list_runners(&self) -> Result<Vec<Runner>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT runner_id, capabilities, max_inflight, last_poll_at, inflight, status, registered_at FROM runners ORDER BY runner_id")
            .map_err(map_err)?;

        let rows = stmt
            .query_map([], |row| row_to_runner(row))
            .map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn remove_runner(&self, runner_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM runners WHERE runner_id = ?1", params![runner_id])
            .map_err(map_err)?;
        Ok(())
    }

    fn update_poll(
        &self,
        runner_id: &str,
        inflight: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let inflight_json = serde_json::to_string(&inflight).unwrap();
        conn.execute(
            "UPDATE runners SET last_poll_at = ?1, inflight = ?2, status = 'online' WHERE runner_id = ?3",
            params![dt_to_sql(&now), inflight_json, runner_id],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

// ─── DeadLetterStore ───

impl DeadLetterStore for SqliteStore {
    fn add_dead_letter(&self, dl: &DeadLetter) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let metadata = serde_json::to_string(&dl.metadata).unwrap();
        conn.execute(
            "INSERT INTO dead_letters (id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                dl.id.to_string(),
                dl.execution_id.to_string(),
                dl.job_key,
                dt_to_sql(&dl.fire_at),
                dl.attempt,
                dl.error,
                dl.dead_reason,
                metadata,
                dt_to_sql(&dl.created_at),
                opt_dt_to_sql(&dl.expires_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_dead_letter(&self, id: Uuid) -> Result<Option<DeadLetter>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at FROM dead_letters WHERE id = ?1")
            .map_err(map_err)?;

        stmt.query_row(params![id.to_string()], |row| row_to_dead_letter(row))
            .optional()
            .map_err(map_err)
    }

    fn list_dead_letters(&self, filter: &DeadLetterFilter) -> Result<Vec<DeadLetter>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from("SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at FROM dead_letters WHERE 1=1");

        if let Some(ref jk) = filter.job_key {
            sql.push_str(&format!(" AND job_key = '{jk}'"));
        }

        sql.push_str(" ORDER BY created_at DESC");
        let limit = filter.limit.unwrap_or(100);
        sql.push_str(&format!(" LIMIT {limit}"));

        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let rows = stmt
            .query_map([], |row| row_to_dead_letter(row))
            .map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn remove_dead_letter(&self, id: Uuid) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM dead_letters WHERE id = ?1", params![id.to_string()])
            .map_err(map_err)?;
        Ok(())
    }

    fn purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM dead_letters WHERE expires_at IS NOT NULL AND expires_at <= ?1",
                params![dt_to_sql(&now)],
            )
            .map_err(map_err)?;
        Ok(affected as u64)
    }
}

impl Store for SqliteStore {}

// ─── Row mappers ───

fn row_to_execution(row: &rusqlite::Row<'_>) -> Result<Execution, rusqlite::Error> {
    let id_str: String = row.get(0)?;
    let metadata_str: String = row.get(12)?;
    Ok(Execution {
        id: Uuid::parse_str(&id_str).unwrap(),
        job_key: row.get(1)?,
        fire_at: sql_to_dt(&row.get::<_, String>(2)?),
        attempt: row.get::<_, u32>(3)?,
        state: parse_execution_state(&row.get::<_, String>(4)?),
        runner_id: row.get(5)?,
        claimed_at: sql_to_opt_dt(row.get(6)?),
        started_at: sql_to_opt_dt(row.get(7)?),
        completed_at: sql_to_opt_dt(row.get(8)?),
        duration_ms: row.get(9)?,
        error: row.get(10)?,
        dead_reason: row.get(11)?,
        metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
        created_at: sql_to_dt(&row.get::<_, String>(13)?),
    })
}

fn row_to_runner(row: &rusqlite::Row<'_>) -> Result<Runner, rusqlite::Error> {
    let caps_str: String = row.get(1)?;
    let inflight_str: String = row.get(4)?;
    Ok(Runner {
        runner_id: row.get(0)?,
        capabilities: serde_json::from_str(&caps_str).unwrap_or_default(),
        max_inflight: row.get::<_, u32>(2)?,
        last_poll_at: sql_to_dt(&row.get::<_, String>(3)?),
        inflight: serde_json::from_str(&inflight_str).unwrap_or_default(),
        status: parse_runner_status(&row.get::<_, String>(5)?),
        registered_at: sql_to_dt(&row.get::<_, String>(6)?),
    })
}

fn row_to_dead_letter(row: &rusqlite::Row<'_>) -> Result<DeadLetter, rusqlite::Error> {
    let id_str: String = row.get(0)?;
    let exec_id_str: String = row.get(1)?;
    let metadata_str: String = row.get(7)?;
    Ok(DeadLetter {
        id: Uuid::parse_str(&id_str).unwrap(),
        execution_id: Uuid::parse_str(&exec_id_str).unwrap(),
        job_key: row.get(2)?,
        fire_at: sql_to_dt(&row.get::<_, String>(3)?),
        attempt: row.get::<_, u32>(4)?,
        error: row.get(5)?,
        dead_reason: row.get(6)?,
        metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
        created_at: sql_to_dt(&row.get::<_, String>(8)?),
        expires_at: sql_to_opt_dt(row.get(9)?),
    })
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

/// Extension trait for optional query results.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
