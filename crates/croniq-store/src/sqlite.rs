//! SQLite store implementation with WAL mode.

use crate::migrations;
use crate::models::*;
use crate::traits::*;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params, params_from_iter};
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
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )
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
        upsert_job_state_with(&conn, state)
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
        conn.execute(
            "DELETE FROM job_states WHERE job_key = ?1",
            params![job_key],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

// ─── ExecutionStore ───

impl ExecutionStore for SqliteStore {
    fn create_execution(&self, exec: &Execution) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        insert_execution_with(&conn, exec)
    }

    fn create_execution_and_advance_job_state(
        &self,
        exec: &Execution,
        job_state: &JobState,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(map_err)?;
        insert_execution_with(&tx, exec)?;
        upsert_job_state_with(&tx, job_state)?;
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn get_execution(&self, id: Uuid) -> Result<Option<Execution>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for FROM executions WHERE id = ?1")
            .map_err(map_err)?;

        stmt.query_row(params![id.to_string()], row_to_execution)
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
        let fetch_limit = if capabilities.is_empty() {
            limit
        } else {
            limit * 4
        };
        let mut stmt = conn
            .prepare("SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for FROM executions WHERE state = 'queued' ORDER BY fire_at ASC LIMIT ?1")
            .map_err(map_err)?;

        let rows = stmt
            .query_map(params![fetch_limit], row_to_execution)
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
        let mut sql = String::from(
            "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for FROM executions WHERE 1=1",
        );
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
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_ref.as_slice(), row_to_execution)
            .map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn find_execution_by_idempotency_key(
        &self,
        job_key: &str,
        idempotency_key: &str,
        window_start: DateTime<Utc>,
    ) -> Result<Option<Execution>, StoreError> {
        let conn = self.conn.lock().unwrap();
        // created_at is stored as RFC 3339 text, which compares
        // lexicographically in chronological order (same trick as the
        // since/until filters in list_executions).
        let mut stmt = conn
            .prepare(
                "SELECT id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for
                 FROM executions
                 WHERE job_key = ?1 AND idempotency_key = ?2
                   AND (state IN ('queued', 'claimed') OR created_at >= ?3)
                 ORDER BY created_at DESC
                 LIMIT 1",
            )
            .map_err(map_err)?;

        stmt.query_row(
            params![job_key, idempotency_key, dt_to_sql(&window_start)],
            row_to_execution,
        )
        .optional()
        .map_err(map_err)
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

    fn count_executions_in_states(
        &self,
        job_key: &str,
        states: &[ExecutionState],
    ) -> Result<u64, StoreError> {
        if states.is_empty() {
            return Ok(0);
        }
        let conn = self.conn.lock().unwrap();
        let placeholders: Vec<String> = (0..states.len()).map(|i| format!("?{}", i + 2)).collect();
        let sql = format!(
            "SELECT COUNT(*) FROM executions WHERE job_key = ?1 AND state IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = conn.prepare(&sql).map_err(map_err)?;

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(job_key.to_string())];
        for state in states {
            param_values.push(Box::new(state_to_str(*state).to_string()));
        }
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let count: i64 = stmt
            .query_row(params_ref.as_slice(), |row| row.get(0))
            .map_err(map_err)?;
        Ok(count as u64)
    }

    fn job_execution_metrics(&self) -> Result<Vec<JobExecutionMetrics>, StoreError> {
        let conn = self.conn.lock().unwrap();

        // Cumulative duration-bucket columns are generated from the shared
        // boundary list so the SQL and the Prometheus renderer can't drift.
        let bucket_cols: String = JOB_DURATION_BUCKETS_SECONDS
            .iter()
            .map(|secs| {
                let ms = (secs * 1000.0).round() as i64;
                format!(
                    ", SUM(CASE WHEN duration_ms IS NOT NULL AND duration_ms <= {ms} THEN 1 ELSE 0 END)"
                )
            })
            .collect();

        let sql = format!(
            "SELECT job_key, \
             SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN state = 'failed' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN state = 'dead' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN state = 'cancelled' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN duration_ms IS NOT NULL THEN 1 ELSE 0 END), \
             COALESCE(SUM(duration_ms), 0), \
             MAX(completed_at){bucket_cols} \
             FROM executions GROUP BY job_key"
        );

        let n_buckets = JOB_DURATION_BUCKETS_SECONDS.len();
        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let rows = stmt
            .query_map([], |row| {
                let mut duration_buckets = Vec::with_capacity(n_buckets);
                for i in 0..n_buckets {
                    // Bucket columns follow the eight fixed leading columns.
                    duration_buckets.push(row.get::<_, i64>(8 + i)? as u64);
                }
                Ok(JobExecutionMetrics {
                    job_key: row.get(0)?,
                    completed: row.get::<_, i64>(1)? as u64,
                    failed: row.get::<_, i64>(2)? as u64,
                    dead: row.get::<_, i64>(3)? as u64,
                    cancelled: row.get::<_, i64>(4)? as u64,
                    duration_count: row.get::<_, i64>(5)? as u64,
                    duration_sum_ms: row.get::<_, i64>(6)?,
                    last_run_at: sql_to_opt_dt(row.get::<_, Option<String>>(7)?),
                    duration_buckets,
                })
            })
            .map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn prune_executions_older_than(
        &self,
        cutoff: DateTime<Utc>,
        limit: u32,
    ) -> Result<u64, StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(map_err)?;
        let cutoff_s = dt_to_sql(&cutoff);
        // Child logs first: `execution_logs` has an enforced FK to
        // `executions(id)` (PRAGMA foreign_keys=ON), so the parent delete
        // would fail otherwise. Both subqueries are identical and
        // deterministic (ORDER BY completed_at, id) over an unmodified
        // `executions` table, so they select the exact same batch.
        tx.execute(
            "DELETE FROM execution_logs WHERE execution_id IN (
                 SELECT id FROM executions
                 WHERE completed_at IS NOT NULL AND completed_at <= ?1 AND state <> 'dead'
                 ORDER BY completed_at ASC, id ASC
                 LIMIT ?2
             )",
            params![cutoff_s, limit],
        )
        .map_err(map_err)?;
        let affected = tx
            .execute(
                "DELETE FROM executions WHERE id IN (
                     SELECT id FROM executions
                     WHERE completed_at IS NOT NULL AND completed_at <= ?1 AND state <> 'dead'
                     ORDER BY completed_at ASC, id ASC
                     LIMIT ?2
                 )",
                params![cutoff_s, limit],
            )
            .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(affected as u64)
    }

    fn prune_executions_keep_last(
        &self,
        job_key: &str,
        keep_last: u32,
        limit: u32,
    ) -> Result<u64, StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(map_err)?;
        // Keep the newest `keep_last` terminal rows (OFFSET), delete up to
        // `limit` older ones. Child logs first (see `prune_executions_older_than`).
        tx.execute(
            "DELETE FROM execution_logs WHERE execution_id IN (
                 SELECT id FROM executions
                 WHERE job_key = ?1 AND completed_at IS NOT NULL AND state <> 'dead'
                 ORDER BY completed_at DESC, id DESC
                 LIMIT ?2 OFFSET ?3
             )",
            params![job_key, limit, keep_last],
        )
        .map_err(map_err)?;
        let affected = tx
            .execute(
                "DELETE FROM executions WHERE id IN (
                     SELECT id FROM executions
                     WHERE job_key = ?1 AND completed_at IS NOT NULL AND state <> 'dead'
                     ORDER BY completed_at DESC, id DESC
                     LIMIT ?2 OFFSET ?3
                 )",
                params![job_key, limit, keep_last],
            )
            .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(affected as u64)
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

        stmt.query_row(params![runner_id], row_to_runner)
            .optional()
            .map_err(map_err)
    }

    fn list_runners(&self) -> Result<Vec<Runner>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT runner_id, capabilities, max_inflight, last_poll_at, inflight, status, registered_at FROM runners ORDER BY runner_id")
            .map_err(map_err)?;

        let rows = stmt.query_map([], row_to_runner).map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn remove_runner(&self, runner_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM runners WHERE runner_id = ?1",
            params![runner_id],
        )
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
        insert_dead_letter_with(&conn, dl)
    }

    fn complete_as_dead(
        &self,
        execution_id: Uuid,
        duration_ms: Option<i64>,
        error: Option<&str>,
        dead_letter: &DeadLetter,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(map_err)?;
        update_to_dead_with(
            &tx,
            execution_id,
            duration_ms,
            error,
            Some(&dead_letter.dead_reason),
            now,
        )?;
        insert_dead_letter_with(&tx, dead_letter)?;
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn get_dead_letter(&self, id: Uuid) -> Result<Option<DeadLetter>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at, scheduled_for FROM dead_letters WHERE id = ?1")
            .map_err(map_err)?;

        stmt.query_row(params![id.to_string()], row_to_dead_letter)
            .optional()
            .map_err(map_err)
    }

    fn list_dead_letters(&self, filter: &DeadLetterFilter) -> Result<Vec<DeadLetter>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at, scheduled_for FROM dead_letters WHERE 1=1",
        );

        if let Some(ref jk) = filter.job_key {
            sql.push_str(&format!(" AND job_key = '{jk}'"));
        }

        sql.push_str(" ORDER BY created_at DESC");
        let limit = filter.limit.unwrap_or(100);
        sql.push_str(&format!(" LIMIT {limit}"));

        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let rows = stmt.query_map([], row_to_dead_letter).map_err(map_err)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn remove_dead_letter(&self, id: Uuid) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM dead_letters WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn remove_dead_letters(&self, ids: &[Uuid]) -> Result<u64, StoreError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.conn.lock().unwrap();
        let placeholders = vec!["?"; ids.len()].join(",");
        let sql = format!("DELETE FROM dead_letters WHERE id IN ({placeholders})");
        let affected = conn
            .execute(&sql, params_from_iter(ids.iter().map(|id| id.to_string())))
            .map_err(map_err)?;
        Ok(affected as u64)
    }

    fn clear_dead_letters(&self, job_key: Option<&str>) -> Result<u64, StoreError> {
        let conn = self.conn.lock().unwrap();
        let affected = match job_key {
            Some(jk) => conn
                .execute("DELETE FROM dead_letters WHERE job_key = ?1", params![jk])
                .map_err(map_err)?,
            None => conn
                .execute("DELETE FROM dead_letters", [])
                .map_err(map_err)?,
        };
        Ok(affected as u64)
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

// ─── AuthStore ───

impl AuthStore for SqliteStore {
    fn create_client(&self, client: &ApiClient) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let scopes = serde_json::to_string(&client.scopes).unwrap_or_default();
        conn.execute(
            "INSERT INTO api_clients (client_id, name, scopes, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(client_id) DO UPDATE SET name = excluded.name, scopes = excluded.scopes, is_active = excluded.is_active",
            params![client.client_id, client.name, scopes, client.is_active, dt_to_sql(&client.created_at)],
        ).map_err(map_err)?;
        Ok(())
    }

    fn get_client(&self, client_id: &str) -> Result<Option<ApiClient>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT client_id, name, scopes, is_active, created_at FROM api_clients WHERE client_id = ?1")
            .map_err(map_err)?
            .query_row(params![client_id], |row| {
                let scopes_str: String = row.get(2)?;
                Ok(ApiClient {
                    client_id: row.get(0)?,
                    name: row.get(1)?,
                    scopes: serde_json::from_str(&scopes_str).unwrap_or_default(),
                    is_active: row.get::<_, bool>(3)?,
                    created_at: sql_to_dt(&row.get::<_, String>(4)?),
                })
            })
            .optional()
            .map_err(map_err)
    }

    fn list_clients(&self) -> Result<Vec<ApiClient>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT client_id, name, scopes, is_active, created_at FROM api_clients ORDER BY name").map_err(map_err)?;
        let rows = stmt
            .query_map([], |row| {
                let scopes_str: String = row.get(2)?;
                Ok(ApiClient {
                    client_id: row.get(0)?,
                    name: row.get(1)?,
                    scopes: serde_json::from_str(&scopes_str).unwrap_or_default(),
                    is_active: row.get::<_, bool>(3)?,
                    created_at: sql_to_dt(&row.get::<_, String>(4)?),
                })
            })
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn delete_client(&self, client_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM api_keys WHERE client_id = ?1",
            params![client_id],
        )
        .map_err(map_err)?;
        conn.execute(
            "DELETE FROM api_clients WHERE client_id = ?1",
            params![client_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn create_api_key(&self, key: &ApiKey) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO api_keys (key_id, client_id, key_hash, key_prefix, expires_at, revoked_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![key.key_id, key.client_id, key.key_hash, key.key_prefix, opt_dt_to_sql(&key.expires_at), opt_dt_to_sql(&key.revoked_at), dt_to_sql(&key.created_at)],
        ).map_err(map_err)?;
        Ok(())
    }

    fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT key_id, client_id, key_hash, key_prefix, expires_at, revoked_at, created_at FROM api_keys WHERE key_hash = ?1")
            .map_err(map_err)?
            .query_row(params![key_hash], |row| {
                Ok(ApiKey {
                    key_id: row.get(0)?,
                    client_id: row.get(1)?,
                    key_hash: row.get(2)?,
                    key_prefix: row.get(3)?,
                    expires_at: sql_to_opt_dt(row.get(4)?),
                    revoked_at: sql_to_opt_dt(row.get(5)?),
                    created_at: sql_to_dt(&row.get::<_, String>(6)?),
                })
            })
            .optional()
            .map_err(map_err)
    }

    fn revoke_api_key(&self, key_id: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE api_keys SET revoked_at = ?1 WHERE key_id = ?2",
            params![dt_to_sql(&now), key_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn list_api_keys(&self, client_id: &str) -> Result<Vec<ApiKey>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key_id, client_id, key_hash, key_prefix, expires_at, revoked_at, created_at FROM api_keys WHERE client_id = ?1 ORDER BY created_at DESC").map_err(map_err)?;
        let rows = stmt
            .query_map(params![client_id], |row| {
                Ok(ApiKey {
                    key_id: row.get(0)?,
                    client_id: row.get(1)?,
                    key_hash: row.get(2)?,
                    key_prefix: row.get(3)?,
                    expires_at: sql_to_opt_dt(row.get(4)?),
                    revoked_at: sql_to_opt_dt(row.get(5)?),
                    created_at: sql_to_dt(&row.get::<_, String>(6)?),
                })
            })
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn get_credentials(&self, username: &str) -> Result<Option<PasswordCredential>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT user_id, username, password_hash, failed_attempts, locked_until, created_at FROM password_credentials WHERE username = ?1")
            .map_err(map_err)?
            .query_row(params![username], |row| {
                Ok(PasswordCredential {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    failed_attempts: row.get::<_, u32>(3)?,
                    locked_until: sql_to_opt_dt(row.get(4)?),
                    created_at: sql_to_dt(&row.get::<_, String>(5)?),
                })
            })
            .optional()
            .map_err(map_err)
    }

    fn upsert_credentials(&self, cred: &PasswordCredential) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO password_credentials (user_id, username, password_hash, failed_attempts, locked_until, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, password_hash = excluded.password_hash, failed_attempts = excluded.failed_attempts, locked_until = excluded.locked_until",
            params![cred.user_id, cred.username, cred.password_hash, cred.failed_attempts, opt_dt_to_sql(&cred.locked_until), dt_to_sql(&cred.created_at)],
        ).map_err(map_err)?;
        Ok(())
    }

    fn create_refresh_token(&self, token: &RefreshToken) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO refresh_tokens (token_hash, client_id, user_id, expires_at, revoked_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![token.token_hash, token.client_id, token.user_id, dt_to_sql(&token.expires_at), opt_dt_to_sql(&token.revoked_at), dt_to_sql(&token.created_at)],
        ).map_err(map_err)?;
        Ok(())
    }

    fn validate_refresh_token(&self, token_hash: &str) -> Result<Option<RefreshToken>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT token_hash, client_id, user_id, expires_at, revoked_at, created_at FROM refresh_tokens WHERE token_hash = ?1 AND revoked_at IS NULL")
            .map_err(map_err)?
            .query_row(params![token_hash], |row| {
                Ok(RefreshToken {
                    token_hash: row.get(0)?,
                    client_id: row.get(1)?,
                    user_id: row.get(2)?,
                    expires_at: sql_to_dt(&row.get::<_, String>(3)?),
                    revoked_at: sql_to_opt_dt(row.get(4)?),
                    created_at: sql_to_dt(&row.get::<_, String>(5)?),
                })
            })
            .optional()
            .map_err(map_err)
    }

    fn revoke_refresh_token(&self, token_hash: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE refresh_tokens SET revoked_at = ?1 WHERE token_hash = ?2",
            params![dt_to_sql(&now), token_hash],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn users_create(&self, user: &User) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(user_id) DO UPDATE SET
                username = excluded.username,
                email = excluded.email,
                display_name = excluded.display_name,
                role = excluded.role,
                is_active = excluded.is_active,
                updated_at = excluded.updated_at",
            params![
                user.user_id,
                user.username,
                user.email,
                user.display_name,
                user.role.as_str(),
                user.is_active as i64,
                dt_to_sql(&user.created_at),
                dt_to_sql(&user.updated_at),
                opt_dt_to_sql(&user.last_login_at),
            ],
        ).map_err(map_err)?;
        Ok(())
    }

    fn users_get_by_id(&self, user_id: &str) -> Result<Option<User>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at FROM users WHERE user_id = ?1")
            .map_err(map_err)?
            .query_row(params![user_id], map_user_row)
            .optional()
            .map_err(map_err)
    }

    fn users_get_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at FROM users WHERE username = ?1")
            .map_err(map_err)?
            .query_row(params![username], map_user_row)
            .optional()
            .map_err(map_err)
    }

    fn users_list(&self) -> Result<Vec<User>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT user_id, username, email, display_name, role, is_active, created_at, updated_at, last_login_at FROM users ORDER BY username")
            .map_err(map_err)?;
        let rows = stmt.query_map([], map_user_row).map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn users_update(&self, user: &User) -> Result<(), StoreError> {
        // users_create is upsert-on-user_id, so update is the same write.
        // The last-admin-demotion check lives in the API layer (calls
        // users_count_active_admins before mutating).
        self.users_create(user)
    }

    fn users_delete(&self, user_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM users WHERE user_id = ?1", params![user_id])
            .map_err(map_err)?;
        Ok(())
    }

    fn users_set_last_login(&self, user_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET last_login_at = ?1, updated_at = ?1 WHERE user_id = ?2",
            params![dt_to_sql(&at), user_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn users_count_active_admins(&self) -> Result<u64, StoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE role = 'admin' AND is_active = 1",
                [],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        Ok(count as u64)
    }

    fn invitations_create(&self, invite: &Invitation) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO invitations (invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                invite.invitation_id,
                invite.email,
                invite.role.as_str(),
                invite.token_hash,
                invite.invited_by,
                dt_to_sql(&invite.expires_at),
                opt_dt_to_sql(&invite.accepted_at),
                opt_dt_to_sql(&invite.revoked_at),
                dt_to_sql(&invite.created_at),
            ],
        ).map_err(map_err)?;
        Ok(())
    }

    fn invitations_get(&self, invitation_id: &str) -> Result<Option<Invitation>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at FROM invitations WHERE invitation_id = ?1")
            .map_err(map_err)?
            .query_row(params![invitation_id], map_invitation_row)
            .optional()
            .map_err(map_err)
    }

    fn invitations_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Invitation>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at FROM invitations WHERE token_hash = ?1")
            .map_err(map_err)?
            .query_row(params![token_hash], map_invitation_row)
            .optional()
            .map_err(map_err)
    }

    fn invitations_list(&self) -> Result<Vec<Invitation>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT invitation_id, email, role, token_hash, invited_by, expires_at, accepted_at, revoked_at, created_at FROM invitations ORDER BY created_at DESC")
            .map_err(map_err)?;
        let rows = stmt.query_map([], map_invitation_row).map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn invitations_mark_accepted(
        &self,
        invitation_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE invitations SET accepted_at = ?1 WHERE invitation_id = ?2",
            params![dt_to_sql(&at), invitation_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn invitations_revoke(&self, invitation_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE invitations SET revoked_at = ?1 WHERE invitation_id = ?2",
            params![dt_to_sql(&at), invitation_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn password_resets_create(&self, reset: &PasswordReset) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO password_resets (reset_id, user_id, token_hash, expires_at, used_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                reset.reset_id,
                reset.user_id,
                reset.token_hash,
                dt_to_sql(&reset.expires_at),
                opt_dt_to_sql(&reset.used_at),
                dt_to_sql(&reset.created_at),
            ],
        ).map_err(map_err)?;
        Ok(())
    }

    fn password_resets_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordReset>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT reset_id, user_id, token_hash, expires_at, used_at, created_at FROM password_resets WHERE token_hash = ?1")
            .map_err(map_err)?
            .query_row(params![token_hash], |row| {
                Ok(PasswordReset {
                    reset_id: row.get(0)?,
                    user_id: row.get(1)?,
                    token_hash: row.get(2)?,
                    expires_at: sql_to_dt(&row.get::<_, String>(3)?),
                    used_at: sql_to_opt_dt(row.get(4)?),
                    created_at: sql_to_dt(&row.get::<_, String>(5)?),
                })
            })
            .optional()
            .map_err(map_err)
    }

    fn password_resets_mark_used(
        &self,
        reset_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE password_resets SET used_at = ?1 WHERE reset_id = ?2",
            params![dt_to_sql(&at), reset_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn totp_upsert(&self, secret: &TotpSecret) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO totp_secrets (user_id, secret_enc, enabled, confirmed_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(user_id) DO UPDATE SET
                secret_enc = excluded.secret_enc,
                enabled = excluded.enabled,
                confirmed_at = excluded.confirmed_at",
            params![
                secret.user_id,
                secret.secret_enc,
                secret.enabled as i64,
                opt_dt_to_sql(&secret.confirmed_at),
                dt_to_sql(&secret.created_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn totp_get(&self, user_id: &str) -> Result<Option<TotpSecret>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT user_id, secret_enc, enabled, confirmed_at, created_at FROM totp_secrets WHERE user_id = ?1",
        )
        .map_err(map_err)?
        .query_row(params![user_id], |row| {
            Ok(TotpSecret {
                user_id: row.get(0)?,
                secret_enc: row.get(1)?,
                enabled: row.get::<_, bool>(2)?,
                confirmed_at: sql_to_opt_dt(row.get(3)?),
                created_at: sql_to_dt(&row.get::<_, String>(4)?),
            })
        })
        .optional()
        .map_err(map_err)
    }

    fn totp_set_enabled(
        &self,
        user_id: &str,
        enabled: bool,
        confirmed_at: Option<DateTime<Utc>>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE totp_secrets SET enabled = ?1, confirmed_at = ?2 WHERE user_id = ?3",
            params![enabled as i64, opt_dt_to_sql(&confirmed_at), user_id,],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn totp_delete(&self, user_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        // Recovery codes cascade via FK ON DELETE CASCADE (foreign_keys PRAGMA on),
        // but be explicit to keep the contract test stable across backends.
        conn.execute(
            "DELETE FROM totp_secrets WHERE user_id = ?1",
            params![user_id],
        )
        .map_err(map_err)?;
        conn.execute(
            "DELETE FROM recovery_codes WHERE user_id = ?1",
            params![user_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn recovery_codes_replace_all(
        &self,
        user_id: &str,
        codes: &[RecoveryCode],
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(map_err)?;
        tx.execute(
            "DELETE FROM recovery_codes WHERE user_id = ?1",
            params![user_id],
        )
        .map_err(map_err)?;
        for code in codes {
            tx.execute(
                "INSERT INTO recovery_codes (code_id, user_id, code_hash, used_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    code.code_id,
                    code.user_id,
                    code.code_hash,
                    opt_dt_to_sql(&code.used_at),
                    dt_to_sql(&code.created_at),
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
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT code_id, user_id, code_hash, used_at, created_at FROM recovery_codes
             WHERE user_id = ?1 AND code_hash = ?2 AND used_at IS NULL",
        )
        .map_err(map_err)?
        .query_row(params![user_id, code_hash], |row| {
            Ok(RecoveryCode {
                code_id: row.get(0)?,
                user_id: row.get(1)?,
                code_hash: row.get(2)?,
                used_at: sql_to_opt_dt(row.get(3)?),
                created_at: sql_to_dt(&row.get::<_, String>(4)?),
            })
        })
        .optional()
        .map_err(map_err)
    }

    fn recovery_codes_mark_used(&self, code_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE recovery_codes SET used_at = ?1 WHERE code_id = ?2",
            params![dt_to_sql(&at), code_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn recovery_codes_count_unused(&self, user_id: &str) -> Result<u64, StoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM recovery_codes WHERE user_id = ?1 AND used_at IS NULL",
                params![user_id],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        Ok(count as u64)
    }

    fn pat_create(&self, pat: &PersonalAccessToken) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let scopes_json =
            serde_json::to_string(&pat.scopes).map_err(|e| StoreError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO personal_access_tokens (token_id, user_id, name, token_hash, token_prefix, scopes, expires_at, revoked_at, last_used_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                pat.token_id,
                pat.user_id,
                pat.name,
                pat.token_hash,
                pat.token_prefix,
                scopes_json,
                opt_dt_to_sql(&pat.expires_at),
                opt_dt_to_sql(&pat.revoked_at),
                opt_dt_to_sql(&pat.last_used_at),
                dt_to_sql(&pat.created_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn pat_find_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PersonalAccessToken>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT token_id, user_id, name, token_hash, token_prefix, scopes, expires_at, revoked_at, last_used_at, created_at
             FROM personal_access_tokens WHERE token_hash = ?1",
        )
        .map_err(map_err)?
        .query_row(params![token_hash], map_pat_row)
        .optional()
        .map_err(map_err)
    }

    fn pat_list(&self, user_id: &str) -> Result<Vec<PersonalAccessToken>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT token_id, user_id, name, token_hash, token_prefix, scopes, expires_at, revoked_at, last_used_at, created_at FROM personal_access_tokens WHERE user_id = ?1 ORDER BY created_at DESC")
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![user_id], map_pat_row)
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn pat_revoke(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE personal_access_tokens SET revoked_at = ?1 WHERE token_id = ?2",
            params![dt_to_sql(&at), token_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn pat_touch_last_used(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE personal_access_tokens SET last_used_at = ?1 WHERE token_id = ?2",
            params![dt_to_sql(&at), token_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn oidc_link(&self, identity: &OidcIdentity) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO oidc_identities (provider, subject, user_id, email, linked_at, last_login_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(provider, subject) DO UPDATE SET
                user_id = excluded.user_id,
                email = excluded.email,
                last_login_at = excluded.last_login_at",
            params![
                identity.provider,
                identity.subject,
                identity.user_id,
                identity.email,
                dt_to_sql(&identity.linked_at),
                opt_dt_to_sql(&identity.last_login_at),
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
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT provider, subject, user_id, email, linked_at, last_login_at FROM oidc_identities WHERE provider = ?1 AND subject = ?2",
        )
        .map_err(map_err)?
        .query_row(params![provider, subject], |row| {
            Ok(OidcIdentity {
                provider: row.get(0)?,
                subject: row.get(1)?,
                user_id: row.get(2)?,
                email: row.get(3)?,
                linked_at: sql_to_dt(&row.get::<_, String>(4)?),
                last_login_at: sql_to_opt_dt(row.get(5)?),
            })
        })
        .optional()
        .map_err(map_err)
    }

    fn oidc_touch_last_login(
        &self,
        provider: &str,
        subject: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE oidc_identities SET last_login_at = ?1 WHERE provider = ?2 AND subject = ?3",
            params![dt_to_sql(&at), provider, subject],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn oidc_pending_create(&self, pending: &OidcPendingLogin) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO oidc_pending_logins (state, nonce, redirect_to, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                pending.state,
                pending.nonce,
                pending.redirect_to,
                dt_to_sql(&pending.created_at),
                dt_to_sql(&pending.expires_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn oidc_pending_take(&self, state: &str) -> Result<Option<OidcPendingLogin>, StoreError> {
        let conn = self.conn.lock().unwrap();
        // Atomic read+delete to make state single-use even under
        // concurrent callbacks. SQLite has no RETURNING DELETE in older
        // versions, so we select first, then delete by PK.
        let found: Option<OidcPendingLogin> = conn
            .prepare(
                "SELECT state, nonce, redirect_to, created_at, expires_at FROM oidc_pending_logins WHERE state = ?1",
            )
            .map_err(map_err)?
            .query_row(params![state], |row| {
                Ok(OidcPendingLogin {
                    state: row.get(0)?,
                    nonce: row.get(1)?,
                    redirect_to: row.get(2)?,
                    created_at: sql_to_dt(&row.get::<_, String>(3)?),
                    expires_at: sql_to_dt(&row.get::<_, String>(4)?),
                })
            })
            .optional()
            .map_err(map_err)?;
        if found.is_some() {
            conn.execute(
                "DELETE FROM oidc_pending_logins WHERE state = ?1",
                params![state],
            )
            .map_err(map_err)?;
        }
        Ok(found)
    }

    fn oidc_pending_purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM oidc_pending_logins WHERE expires_at < ?1",
                params![dt_to_sql(&now)],
            )
            .map_err(map_err)?;
        Ok(affected as u64)
    }

    fn audit_log(&self, event: &AuditEvent) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO audit_log (event_id, actor_type, actor_id, action, target_type, target_id, diff_json, ip_address, user_agent, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.event_id,
                event.actor_type,
                event.actor_id,
                event.action,
                event.target_type,
                event.target_id,
                event.diff_json,
                event.ip_address,
                event.user_agent,
                dt_to_sql(&event.created_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn audit_list(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError> {
        let conn = self.conn.lock().unwrap();
        // Build a dynamic WHERE clause. Bind params in the same order
        // they're pushed onto `clauses`/`vals`.
        let mut clauses: Vec<&str> = Vec::new();
        let mut vals: Vec<String> = Vec::new();
        if let Some(v) = &filter.actor_type {
            clauses.push("actor_type = ?");
            vals.push(v.clone());
        }
        if let Some(v) = &filter.actor_id {
            clauses.push("actor_id = ?");
            vals.push(v.clone());
        }
        if let Some(v) = &filter.action {
            clauses.push("action = ?");
            vals.push(v.clone());
        }
        if let Some(v) = &filter.target_type {
            clauses.push("target_type = ?");
            vals.push(v.clone());
        }
        if let Some(v) = &filter.target_id {
            clauses.push("target_id = ?");
            vals.push(v.clone());
        }
        if let Some(t) = filter.since {
            clauses.push("created_at >= ?");
            vals.push(dt_to_sql(&t));
        }
        if let Some(t) = filter.until {
            clauses.push("created_at <= ?");
            vals.push(dt_to_sql(&t));
        }

        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let limit = filter.limit.unwrap_or(200).min(1000);
        let sql = format!(
            "SELECT event_id, actor_type, actor_id, action, target_type, target_id, diff_json, ip_address, user_agent, created_at
             FROM audit_log {where_sql} ORDER BY created_at DESC LIMIT {limit}"
        );
        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let bind_params: Vec<&dyn rusqlite::ToSql> =
            vals.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let rows = stmt
            .query_map(bind_params.as_slice(), |row| {
                Ok(AuditEvent {
                    event_id: row.get(0)?,
                    actor_type: row.get(1)?,
                    actor_id: row.get(2)?,
                    action: row.get(3)?,
                    target_type: row.get(4)?,
                    target_id: row.get(5)?,
                    diff_json: row.get(6)?,
                    ip_address: row.get(7)?,
                    user_agent: row.get(8)?,
                    created_at: sql_to_dt(&row.get::<_, String>(9)?),
                })
            })
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }
}

fn map_pat_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersonalAccessToken> {
    let scopes_json: String = row.get(5)?;
    let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
    Ok(PersonalAccessToken {
        token_id: row.get(0)?,
        user_id: row.get(1)?,
        name: row.get(2)?,
        token_hash: row.get(3)?,
        token_prefix: row.get(4)?,
        scopes,
        expires_at: sql_to_opt_dt(row.get(6)?),
        revoked_at: sql_to_opt_dt(row.get(7)?),
        last_used_at: sql_to_opt_dt(row.get(8)?),
        created_at: sql_to_dt(&row.get::<_, String>(9)?),
    })
}

fn map_invitation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Invitation> {
    use std::str::FromStr;
    let role_str: String = row.get(2)?;
    let role = Role::from_str(&role_str).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("unknown role: {role_str}").into(),
        )
    })?;
    Ok(Invitation {
        invitation_id: row.get(0)?,
        email: row.get(1)?,
        role,
        token_hash: row.get(3)?,
        invited_by: row.get(4)?,
        expires_at: sql_to_dt(&row.get::<_, String>(5)?),
        accepted_at: sql_to_opt_dt(row.get(6)?),
        revoked_at: sql_to_opt_dt(row.get(7)?),
        created_at: sql_to_dt(&row.get::<_, String>(8)?),
    })
}

fn map_user_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    use std::str::FromStr;
    let role_str: String = row.get(4)?;
    let role = Role::from_str(&role_str).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            format!("unknown role: {role_str}").into(),
        )
    })?;
    Ok(User {
        user_id: row.get(0)?,
        username: row.get(1)?,
        email: row.get(2)?,
        display_name: row.get(3)?,
        role,
        is_active: row.get::<_, bool>(5)?,
        created_at: sql_to_dt(&row.get::<_, String>(6)?),
        updated_at: sql_to_dt(&row.get::<_, String>(7)?),
        last_login_at: sql_to_opt_dt(row.get(8)?),
    })
}

// ─── JobDefinitionStore ───

fn map_job_def_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobDefinition> {
    let meta_str: String = row.get(4)?;
    let max_retries: Option<i64> = row.get(8)?;
    let dead_letter_enabled: Option<i64> = row.get(9)?;
    let tags_str: String = row.get(10)?;
    Ok(JobDefinition {
        job_key: row.get(0)?,
        description: row.get(1)?,
        assigned_runner_id: row.get(2)?,
        is_active: row.get::<_, bool>(3)?,
        metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
        created_at: sql_to_dt(&row.get::<_, String>(5)?),
        updated_at: sql_to_dt(&row.get::<_, String>(6)?),
        timeout: row.get(7)?,
        max_retries: max_retries.map(|n| n as u32),
        dead_letter_enabled: dead_letter_enabled.map(|n| n != 0),
        tags: serde_json::from_str(&tags_str).unwrap_or_default(),
    })
}

impl JobDefinitionStore for SqliteStore {
    fn create_job_definition(&self, job: &JobDefinition) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let metadata = serde_json::to_string(&job.metadata).unwrap_or_default();
        let tags = serde_json::to_string(&job.tags).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO job_definitions
                (job_key, description, assigned_runner_id, is_active, metadata,
                 created_at, updated_at, timeout, max_retries, dead_letter_enabled, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(job_key) DO UPDATE SET
                description=excluded.description,
                assigned_runner_id=excluded.assigned_runner_id,
                is_active=excluded.is_active,
                metadata=excluded.metadata,
                updated_at=excluded.updated_at,
                timeout=excluded.timeout,
                max_retries=excluded.max_retries,
                dead_letter_enabled=excluded.dead_letter_enabled,
                tags=excluded.tags",
            params![
                job.job_key,
                job.description,
                job.assigned_runner_id,
                job.is_active,
                metadata,
                dt_to_sql(&job.created_at),
                dt_to_sql(&job.updated_at),
                job.timeout,
                job.max_retries,
                job.dead_letter_enabled,
                tags,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_job_definition(&self, job_key: &str) -> Result<Option<JobDefinition>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT job_key, description, assigned_runner_id, is_active, metadata,
                    created_at, updated_at, timeout, max_retries, dead_letter_enabled, tags
             FROM job_definitions WHERE job_key = ?1",
        )
        .map_err(map_err)?
        .query_row(params![job_key], map_job_def_row)
        .optional()
        .map_err(map_err)
    }

    fn list_job_definitions(&self) -> Result<Vec<JobDefinition>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT job_key, description, assigned_runner_id, is_active, metadata,
                    created_at, updated_at, timeout, max_retries, dead_letter_enabled, tags
             FROM job_definitions ORDER BY job_key",
            )
            .map_err(map_err)?;
        let rows = stmt.query_map([], map_job_def_row).map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn delete_job_definition(&self, job_key: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM trigger_definitions WHERE job_key = ?1",
            params![job_key],
        )
        .map_err(map_err)?;
        conn.execute(
            "DELETE FROM job_definitions WHERE job_key = ?1",
            params![job_key],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

// ─── TriggerDefinitionStore ───

impl TriggerDefinitionStore for SqliteStore {
    fn create_trigger(&self, t: &TriggerDefinition) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO trigger_definitions (trigger_id, job_key, cron_expression, timezone, calendar, window, not_before, not_after, enabled, managed_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(trigger_id) DO UPDATE SET job_key=excluded.job_key, cron_expression=excluded.cron_expression, timezone=excluded.timezone, calendar=excluded.calendar, window=excluded.window, not_before=excluded.not_before, not_after=excluded.not_after, enabled=excluded.enabled, managed_by=excluded.managed_by, updated_at=excluded.updated_at",
            params![t.trigger_id, t.job_key, t.cron_expression, t.timezone, t.calendar, t.window, opt_dt_to_sql(&t.not_before), opt_dt_to_sql(&t.not_after), t.enabled, t.managed_by, dt_to_sql(&t.created_at), dt_to_sql(&t.updated_at)],
        ).map_err(map_err)?;
        Ok(())
    }

    fn get_trigger(&self, trigger_id: &str) -> Result<Option<TriggerDefinition>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT trigger_id, job_key, cron_expression, timezone, calendar, window, not_before, not_after, enabled, managed_by, created_at, updated_at FROM trigger_definitions WHERE trigger_id = ?1")
            .map_err(map_err)?
            .query_row(params![trigger_id], |row| {
                Ok(TriggerDefinition {
                    trigger_id: row.get(0)?,
                    job_key: row.get(1)?,
                    cron_expression: row.get(2)?,
                    timezone: row.get(3)?,
                    calendar: row.get(4)?,
                    window: row.get(5)?,
                    not_before: sql_to_opt_dt(row.get(6)?),
                    not_after: sql_to_opt_dt(row.get(7)?),
                    enabled: row.get::<_, bool>(8)?,
                    managed_by: row.get(9)?,
                    created_at: sql_to_dt(&row.get::<_, String>(10)?),
                    updated_at: sql_to_dt(&row.get::<_, String>(11)?),
                })
            })
            .optional()
            .map_err(map_err)
    }

    fn list_triggers(&self, job_key: Option<&str>) -> Result<Vec<TriggerDefinition>, StoreError> {
        let conn = self.conn.lock().unwrap();
        if let Some(jk) = job_key {
            let mut stmt = conn.prepare("SELECT trigger_id, job_key, cron_expression, timezone, calendar, window, not_before, not_after, enabled, managed_by, created_at, updated_at FROM trigger_definitions WHERE job_key = ?1 ORDER BY created_at").map_err(map_err)?;
            let rows = stmt
                .query_map(params![jk], row_to_trigger_def)
                .map_err(map_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
        } else {
            let mut stmt = conn.prepare("SELECT trigger_id, job_key, cron_expression, timezone, calendar, window, not_before, not_after, enabled, managed_by, created_at, updated_at FROM trigger_definitions ORDER BY created_at").map_err(map_err)?;
            let rows = stmt.query_map([], row_to_trigger_def).map_err(map_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
        }
    }

    fn delete_trigger(&self, trigger_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM trigger_definitions WHERE trigger_id = ?1",
            params![trigger_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn update_trigger(&self, t: &TriggerDefinition) -> Result<bool, StoreError> {
        // Scoped to `managed_by != 'dsl'` so Croniqfile-owned rows can
        // never be edited through this path even if a caller fabricates
        // the trigger_id; everything else (`api`, `runner`, future
        // operator imports) is editable. Returns rows_affected as the
        // found-flag — the handler maps `false` to 404.
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "UPDATE trigger_definitions
             SET cron_expression = ?2, timezone = ?3, calendar = ?4, enabled = ?5, updated_at = ?6
             WHERE trigger_id = ?1 AND managed_by != 'dsl'",
                params![
                    t.trigger_id,
                    t.cron_expression,
                    t.timezone,
                    t.calendar,
                    t.enabled,
                    dt_to_sql(&t.updated_at),
                ],
            )
            .map_err(map_err)?;
        Ok(n > 0)
    }
}

// ─── CalendarDefinitionStore ───

impl CalendarDefinitionStore for SqliteStore {
    fn create_calendar(&self, cal: &CalendarDefinition) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO calendar_definitions (calendar_id, name, timezone, rules, managed_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(calendar_id) DO UPDATE SET name=excluded.name, timezone=excluded.timezone, rules=excluded.rules, managed_by=excluded.managed_by, updated_at=excluded.updated_at",
            params![cal.calendar_id, cal.name, cal.timezone, cal.rules, cal.managed_by, dt_to_sql(&cal.created_at), dt_to_sql(&cal.updated_at)],
        ).map_err(map_err)?;
        Ok(())
    }

    fn get_calendar(&self, calendar_id: &str) -> Result<Option<CalendarDefinition>, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare("SELECT calendar_id, name, timezone, rules, managed_by, created_at, updated_at FROM calendar_definitions WHERE calendar_id = ?1")
            .map_err(map_err)?
            .query_row(params![calendar_id], |row| {
                Ok(CalendarDefinition {
                    calendar_id: row.get(0)?,
                    name: row.get(1)?,
                    timezone: row.get(2)?,
                    rules: row.get(3)?,
                    managed_by: row.get(4)?,
                    created_at: sql_to_dt(&row.get::<_, String>(5)?),
                    updated_at: sql_to_dt(&row.get::<_, String>(6)?),
                })
            })
            .optional()
            .map_err(map_err)
    }

    fn list_calendars(&self) -> Result<Vec<CalendarDefinition>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT calendar_id, name, timezone, rules, managed_by, created_at, updated_at FROM calendar_definitions ORDER BY name").map_err(map_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CalendarDefinition {
                    calendar_id: row.get(0)?,
                    name: row.get(1)?,
                    timezone: row.get(2)?,
                    rules: row.get(3)?,
                    managed_by: row.get(4)?,
                    created_at: sql_to_dt(&row.get::<_, String>(5)?),
                    updated_at: sql_to_dt(&row.get::<_, String>(6)?),
                })
            })
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn delete_calendar(&self, calendar_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM calendar_definitions WHERE calendar_id = ?1 AND managed_by != 'dsl'",
            params![calendar_id],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

// ─── ExecutionLogStore ───

impl ExecutionLogStore for SqliteStore {
    fn append_log(&self, entry: &ExecutionLogEntry) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let next = next_seq_for_execution(&conn, entry.execution_id)?;
        insert_log_with(&conn, entry, next)
    }

    fn append_logs_batch(&self, entries: &[ExecutionLogEntry]) -> Result<(), StoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        // Group by execution_id so each group gets a contiguous seq range.
        // In practice every entry in a single batch shares one execution_id
        // (push_log_events is per-execution), but be defensive.
        let mut next_seq_by_exec: std::collections::HashMap<Uuid, i64> =
            std::collections::HashMap::new();

        let tx = conn.transaction().map_err(map_err)?;
        for entry in entries {
            let next = match next_seq_by_exec.get(&entry.execution_id).copied() {
                Some(n) => n,
                None => next_seq_for_execution(&tx, entry.execution_id)?,
            };
            insert_log_with(&tx, entry, next)?;
            next_seq_by_exec.insert(entry.execution_id, next + 1);
        }
        tx.commit().map_err(map_err)?;
        Ok(())
    }

    fn read_logs(
        &self,
        execution_id: Uuid,
        limit: u32,
    ) -> Result<Vec<ExecutionLogEntry>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, execution_id, timestamp, level, message, fields, seq \
             FROM execution_logs WHERE execution_id = ?1 \
             ORDER BY timestamp ASC, seq ASC LIMIT ?2",
            )
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![execution_id.to_string(), limit], |row| {
                let id_str: String = row.get(0)?;
                let exec_id_str: String = row.get(1)?;
                let fields_str: String = row.get(5)?;
                Ok(ExecutionLogEntry {
                    id: Uuid::parse_str(&id_str).unwrap(),
                    execution_id: Uuid::parse_str(&exec_id_str).unwrap(),
                    timestamp: sql_to_dt(&row.get::<_, String>(2)?),
                    level: row.get(3)?,
                    message: row.get(4)?,
                    fields: serde_json::from_str(&fields_str).unwrap_or_default(),
                    seq: row.get(6)?,
                })
            })
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }
}

fn next_seq_for_execution(
    conn: &rusqlite::Connection,
    execution_id: Uuid,
) -> Result<i64, StoreError> {
    let max: Option<i64> = conn
        .query_row(
            "SELECT MAX(seq) FROM execution_logs WHERE execution_id = ?1",
            params![execution_id.to_string()],
            |row| row.get(0),
        )
        .map_err(map_err)?;
    Ok(max.map(|m| m + 1).unwrap_or(0))
}

fn insert_log_with(
    conn: &rusqlite::Connection,
    entry: &ExecutionLogEntry,
    seq: i64,
) -> Result<(), StoreError> {
    let fields = serde_json::to_string(&entry.fields).unwrap_or_default();
    conn.execute(
        "INSERT INTO execution_logs (id, execution_id, timestamp, level, message, fields, seq) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            entry.id.to_string(),
            entry.execution_id.to_string(),
            dt_to_sql(&entry.timestamp),
            entry.level,
            entry.message,
            fields,
            seq,
        ],
    )
    .map_err(map_err)?;
    Ok(())
}

// ─── DslAdoptionStore ───

impl DslAdoptionStore for SqliteStore {
    fn insert_adoption(&self, adoption: &DslAdoption) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO dsl_adoptions (resource_type, resource_key, adopted_at, adopted_by)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(resource_type, resource_key) DO UPDATE SET
                adopted_at = excluded.adopted_at,
                adopted_by = excluded.adopted_by",
            params![
                adoption.resource_type,
                adoption.resource_key,
                dt_to_sql(&adoption.adopted_at),
                adoption.adopted_by,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn delete_adoption(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "DELETE FROM dsl_adoptions WHERE resource_type = ?1 AND resource_key = ?2",
                params![resource_type, resource_key],
            )
            .map_err(map_err)?;
        Ok(n > 0)
    }

    fn is_adopted(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dsl_adoptions WHERE resource_type = ?1 AND resource_key = ?2",
                params![resource_type, resource_key],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        Ok(n > 0)
    }

    fn list_adoptions(&self, resource_type: &str) -> Result<Vec<DslAdoption>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT resource_type, resource_key, adopted_at, adopted_by
                 FROM dsl_adoptions WHERE resource_type = ?1 ORDER BY resource_key",
            )
            .map_err(map_err)?;
        let rows = stmt
            .query_map(params![resource_type], |row| {
                Ok(DslAdoption {
                    resource_type: row.get(0)?,
                    resource_key: row.get(1)?,
                    adopted_at: sql_to_dt(&row.get::<_, String>(2)?),
                    adopted_by: row.get(3)?,
                })
            })
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }
}

impl AlertStore for SqliteStore {
    fn record_alert_delivery(&self, d: &AlertDelivery) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO alert_deliveries
                (delivery_id, rule_name, channel_name, job_key, execution_id,
                 state, error, fired_at, delivered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                d.delivery_id,
                d.rule_name,
                d.channel_name,
                d.job_key,
                d.execution_id,
                d.state.as_str(),
                d.error,
                dt_to_sql(&d.fired_at),
                opt_dt_to_sql(&d.delivered_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn list_alert_deliveries(
        &self,
        filter: &AlertDeliveryFilter,
    ) -> Result<Vec<AlertDelivery>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT delivery_id, rule_name, channel_name, job_key, execution_id,
                    state, error, fired_at, delivered_at
             FROM alert_deliveries WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(ref jk) = filter.job_key {
            sql.push_str(" AND job_key = ?");
            params.push(Box::new(jk.clone()));
        }
        if let Some(ref rn) = filter.rule_name {
            sql.push_str(" AND rule_name = ?");
            params.push(Box::new(rn.clone()));
        }
        if let Some(since) = filter.since {
            sql.push_str(" AND fired_at >= ?");
            params.push(Box::new(dt_to_sql(&since)));
        }
        sql.push_str(" ORDER BY fired_at DESC");
        let limit = filter.limit.unwrap_or(200);
        sql.push_str(&format!(" LIMIT {limit}"));

        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), row_to_alert_delivery)
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn last_alert_fire_at(
        &self,
        rule_name: &str,
        job_key: &str,
    ) -> Result<Option<DateTime<Utc>>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT fired_at FROM alert_deliveries
                 WHERE rule_name = ?1 AND job_key = ?2
                 ORDER BY fired_at DESC LIMIT 1",
            )
            .map_err(map_err)?;
        stmt.query_row(params![rule_name, job_key], |row| {
            let s: String = row.get(0)?;
            Ok(sql_to_dt(&s))
        })
        .optional()
        .map_err(map_err)
    }

    fn get_alert_delivery(&self, delivery_id: &str) -> Result<Option<AlertDelivery>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT delivery_id, rule_name, channel_name, job_key, execution_id,
                        state, error, fired_at, delivered_at
                 FROM alert_deliveries WHERE delivery_id = ?1",
            )
            .map_err(map_err)?;
        stmt.query_row(params![delivery_id], row_to_alert_delivery)
            .optional()
            .map_err(map_err)
    }

    fn upsert_alert_rule_override(&self, ov: &AlertRuleOverride) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO alert_rule_overrides
                (rule_name, enabled, snooze_until, throttle_secs,
                 note, set_by_user_id, set_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(rule_name) DO UPDATE SET
                enabled        = excluded.enabled,
                snooze_until   = excluded.snooze_until,
                throttle_secs  = excluded.throttle_secs,
                note           = excluded.note,
                set_by_user_id = excluded.set_by_user_id,
                set_at         = excluded.set_at,
                expires_at     = excluded.expires_at",
            params![
                ov.rule_name,
                ov.enabled,
                opt_dt_to_sql(&ov.snooze_until),
                ov.throttle_secs.map(|s| s as i64),
                ov.note,
                ov.set_by_user_id,
                dt_to_sql(&ov.set_at),
                opt_dt_to_sql(&ov.expires_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get_alert_rule_override(
        &self,
        rule_name: &str,
    ) -> Result<Option<AlertRuleOverride>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT rule_name, enabled, snooze_until, throttle_secs,
                        note, set_by_user_id, set_at, expires_at
                 FROM alert_rule_overrides WHERE rule_name = ?1",
            )
            .map_err(map_err)?;
        stmt.query_row(params![rule_name], row_to_alert_rule_override)
            .optional()
            .map_err(map_err)
    }

    fn list_alert_rule_overrides(&self) -> Result<Vec<AlertRuleOverride>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT rule_name, enabled, snooze_until, throttle_secs,
                        note, set_by_user_id, set_at, expires_at
                 FROM alert_rule_overrides ORDER BY set_at DESC",
            )
            .map_err(map_err)?;
        let rows = stmt
            .query_map([], row_to_alert_rule_override)
            .map_err(map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_err)
    }

    fn delete_alert_rule_override(&self, rule_name: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "DELETE FROM alert_rule_overrides WHERE rule_name = ?1",
                params![rule_name],
            )
            .map_err(map_err)?;
        Ok(n > 0)
    }

    fn delete_expired_alert_rule_overrides(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let now_sql = dt_to_sql(&now);
        let cleared = {
            let mut stmt = conn
                .prepare(
                    "SELECT rule_name FROM alert_rule_overrides
                     WHERE expires_at IS NOT NULL AND expires_at <= ?1",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map(params![now_sql], |row| row.get::<_, String>(0))
                .map_err(map_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_err)?
        };
        if !cleared.is_empty() {
            conn.execute(
                "DELETE FROM alert_rule_overrides
                 WHERE expires_at IS NOT NULL AND expires_at <= ?1",
                params![now_sql],
            )
            .map_err(map_err)?;
        }
        Ok(cleared)
    }

    fn prune_alert_rule_overrides(
        &self,
        valid_rule_names: &[String],
    ) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let existing = {
            let mut stmt = conn
                .prepare("SELECT rule_name FROM alert_rule_overrides")
                .map_err(map_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(map_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_err)?
        };
        let valid: std::collections::HashSet<&str> =
            valid_rule_names.iter().map(String::as_str).collect();
        let orphans: Vec<String> = existing
            .into_iter()
            .filter(|name| !valid.contains(name.as_str()))
            .collect();
        for name in &orphans {
            conn.execute(
                "DELETE FROM alert_rule_overrides WHERE rule_name = ?1",
                params![name],
            )
            .map_err(map_err)?;
        }
        Ok(orphans)
    }
}

impl MaintenanceStore for SqliteStore {
    fn get_maintenance(&self) -> Result<MaintenanceState, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT manual_active, window_start, window_end, note, updated_by, updated_at
                 FROM maintenance WHERE id = 1",
            )
            .map_err(map_err)?;
        stmt.query_row([], row_to_maintenance)
            .optional()
            .map_err(map_err)
            .map(Option::unwrap_or_default)
    }

    fn set_maintenance(&self, state: &MaintenanceState) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO maintenance
                (id, manual_active, window_start, window_end, note, updated_by, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                manual_active = excluded.manual_active,
                window_start  = excluded.window_start,
                window_end    = excluded.window_end,
                note          = excluded.note,
                updated_by    = excluded.updated_by,
                updated_at    = excluded.updated_at",
            params![
                state.manual_active,
                opt_dt_to_sql(&state.window_start),
                opt_dt_to_sql(&state.window_end),
                state.note,
                state.updated_by,
                opt_dt_to_sql(&state.updated_at),
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }
}

impl Store for SqliteStore {}

// ─── Row mappers ───

fn row_to_execution(row: &rusqlite::Row<'_>) -> Result<Execution, rusqlite::Error> {
    let id_str: String = row.get(0)?;
    let metadata_str: String = row.get(12)?;
    let fire_at = sql_to_dt(&row.get::<_, String>(2)?);
    Ok(Execution {
        id: Uuid::parse_str(&id_str).unwrap(),
        job_key: row.get(1)?,
        fire_at,
        // NULL for rows written before migration 022 (or by an older binary
        // running against a migrated DB) — fall back to fire_at.
        scheduled_for: sql_to_opt_dt(row.get(15)?).unwrap_or(fire_at),
        attempt: row.get::<_, u32>(3)?,
        state: parse_execution_state(&row.get::<_, String>(4)?),
        runner_id: row.get(5)?,
        claimed_at: sql_to_opt_dt(row.get(6)?),
        started_at: sql_to_opt_dt(row.get(7)?),
        completed_at: sql_to_opt_dt(row.get(8)?),
        duration_ms: row.get(9)?,
        error: row.get(10)?,
        dead_reason: row.get(11)?,
        idempotency_key: row.get(14)?,
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
    let fire_at = sql_to_dt(&row.get::<_, String>(3)?);
    Ok(DeadLetter {
        id: Uuid::parse_str(&id_str).unwrap(),
        execution_id: Uuid::parse_str(&exec_id_str).unwrap(),
        job_key: row.get(2)?,
        fire_at,
        // NULL for rows written before migration 022 — fall back to fire_at.
        scheduled_for: sql_to_opt_dt(row.get(10)?).unwrap_or(fire_at),
        attempt: row.get::<_, u32>(4)?,
        error: row.get(5)?,
        dead_reason: row.get(6)?,
        metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
        created_at: sql_to_dt(&row.get::<_, String>(8)?),
        expires_at: sql_to_opt_dt(row.get(9)?),
    })
}

fn row_to_alert_delivery(row: &rusqlite::Row<'_>) -> Result<AlertDelivery, rusqlite::Error> {
    let state_str: String = row.get(5)?;
    let state = AlertDeliveryState::parse_db(&state_str).unwrap_or(AlertDeliveryState::Failed);
    Ok(AlertDelivery {
        delivery_id: row.get(0)?,
        rule_name: row.get(1)?,
        channel_name: row.get(2)?,
        job_key: row.get(3)?,
        execution_id: row.get(4)?,
        state,
        error: row.get(6)?,
        fired_at: sql_to_dt(&row.get::<_, String>(7)?),
        delivered_at: sql_to_opt_dt(row.get(8)?),
    })
}

fn row_to_alert_rule_override(
    row: &rusqlite::Row<'_>,
) -> Result<AlertRuleOverride, rusqlite::Error> {
    Ok(AlertRuleOverride {
        rule_name: row.get(0)?,
        enabled: row.get(1)?,
        snooze_until: sql_to_opt_dt(row.get(2)?),
        throttle_secs: row.get::<_, Option<i64>>(3)?.map(|s| s as u64),
        note: row.get(4)?,
        set_by_user_id: row.get(5)?,
        set_at: sql_to_dt(&row.get::<_, String>(6)?),
        expires_at: sql_to_opt_dt(row.get(7)?),
    })
}

fn row_to_maintenance(row: &rusqlite::Row<'_>) -> Result<MaintenanceState, rusqlite::Error> {
    Ok(MaintenanceState {
        manual_active: row.get(0)?,
        window_start: sql_to_opt_dt(row.get(1)?),
        window_end: sql_to_opt_dt(row.get(2)?),
        note: row.get(3)?,
        updated_by: row.get(4)?,
        updated_at: sql_to_opt_dt(row.get(5)?),
    })
}

fn row_to_trigger_def(row: &rusqlite::Row<'_>) -> Result<TriggerDefinition, rusqlite::Error> {
    Ok(TriggerDefinition {
        trigger_id: row.get(0)?,
        job_key: row.get(1)?,
        cron_expression: row.get(2)?,
        timezone: row.get(3)?,
        calendar: row.get(4)?,
        window: row.get(5)?,
        not_before: sql_to_opt_dt(row.get(6)?),
        not_after: sql_to_opt_dt(row.get(7)?),
        enabled: row.get::<_, bool>(8)?,
        managed_by: row.get(9)?,
        created_at: sql_to_dt(&row.get::<_, String>(10)?),
        updated_at: sql_to_dt(&row.get::<_, String>(11)?),
    })
}

// ─── Reusable insert/upsert helpers ───
//
// These take `&rusqlite::Connection` so the same SQL works against either a
// plain pooled connection (single-statement methods) or a transaction (the
// atomic scheduler-tick method, where `&Transaction` derefs to `&Connection`).

fn insert_execution_with(conn: &rusqlite::Connection, exec: &Execution) -> Result<(), StoreError> {
    let metadata = serde_json::to_string(&exec.metadata).unwrap_or_default();
    conn.execute(
        "INSERT INTO executions (id, job_key, fire_at, attempt, state, runner_id, claimed_at, started_at, completed_at, duration_ms, error, dead_reason, metadata, created_at, idempotency_key, scheduled_for)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
            exec.idempotency_key,
            dt_to_sql(&exec.scheduled_for),
        ],
    )
    .map_err(map_err)?;
    Ok(())
}

fn update_to_dead_with(
    conn: &rusqlite::Connection,
    id: Uuid,
    duration_ms: Option<i64>,
    error: Option<&str>,
    dead_reason: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE executions SET state = 'dead', completed_at = ?1, duration_ms = ?2, error = ?3, dead_reason = ?4 WHERE id = ?5",
        params![
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

fn insert_dead_letter_with(conn: &rusqlite::Connection, dl: &DeadLetter) -> Result<(), StoreError> {
    let metadata = serde_json::to_string(&dl.metadata).unwrap();
    conn.execute(
        "INSERT INTO dead_letters (id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at, expires_at, scheduled_for)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            dt_to_sql(&dl.scheduled_for),
        ],
    )
    .map_err(map_err)?;
    Ok(())
}

fn upsert_job_state_with(conn: &rusqlite::Connection, state: &JobState) -> Result<(), StoreError> {
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
