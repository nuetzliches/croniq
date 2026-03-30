//! Store trait definitions.
//!
//! All persistence is behind these traits. Implementations: SQLite (primary), in-memory (tests).

use crate::models::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Errors from store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("database error: {0}")]
    Database(String),
}

/// Job state persistence.
pub trait JobStore {
    /// Get the runtime state of a job.
    fn get_job_state(&self, job_key: &str) -> Result<Option<JobState>, StoreError>;

    /// Upsert job state (create or update).
    fn upsert_job_state(&self, state: &JobState) -> Result<(), StoreError>;

    /// List all job states.
    fn list_job_states(&self) -> Result<Vec<JobState>, StoreError>;

    /// Delete job state.
    fn delete_job_state(&self, job_key: &str) -> Result<(), StoreError>;
}

/// Execution persistence.
pub trait ExecutionStore {
    /// Create a new queued execution.
    fn create_execution(&self, execution: &Execution) -> Result<(), StoreError>;

    /// Get an execution by ID.
    fn get_execution(&self, id: Uuid) -> Result<Option<Execution>, StoreError>;

    /// Claim a queued execution for a runner. Returns the execution if successfully claimed.
    fn claim_execution(
        &self,
        id: Uuid,
        runner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Execution, StoreError>;

    /// Complete an execution (success, failure, or dead).
    fn complete_execution(
        &self,
        id: Uuid,
        state: ExecutionState,
        duration_ms: Option<i64>,
        error: Option<&str>,
        dead_reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError>;

    /// Find the next queued execution matching runner capabilities.
    /// Returns executions ordered by fire_at (oldest first).
    fn find_queued_executions(
        &self,
        capabilities: &[String],
        limit: u32,
    ) -> Result<Vec<Execution>, StoreError>;

    /// List executions with optional filters.
    fn list_executions(&self, filter: &ExecutionFilter) -> Result<Vec<Execution>, StoreError>;

    /// Mark abandoned executions (runner dead) back to queued.
    fn requeue_abandoned(
        &self,
        runner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, StoreError>;

    /// Cancel an execution.
    fn cancel_execution(&self, id: Uuid, now: DateTime<Utc>) -> Result<(), StoreError>;

    /// Count executions by state.
    fn count_by_state(&self) -> Result<std::collections::HashMap<ExecutionState, u64>, StoreError>;
}

/// Runner persistence.
pub trait RunnerStore {
    /// Register or update a runner (upsert on runner_id).
    fn upsert_runner(&self, runner: &Runner) -> Result<(), StoreError>;

    /// Get a runner by ID.
    fn get_runner(&self, runner_id: &str) -> Result<Option<Runner>, StoreError>;

    /// List all runners.
    fn list_runners(&self) -> Result<Vec<Runner>, StoreError>;

    /// Remove a runner.
    fn remove_runner(&self, runner_id: &str) -> Result<(), StoreError>;

    /// Update runner's last poll time and inflight list.
    fn update_poll(
        &self,
        runner_id: &str,
        inflight: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<(), StoreError>;
}

/// Dead letter queue persistence.
pub trait DeadLetterStore {
    /// Add an execution to the dead letter queue.
    fn add_dead_letter(&self, dl: &DeadLetter) -> Result<(), StoreError>;

    /// Get a dead letter by ID.
    fn get_dead_letter(&self, id: Uuid) -> Result<Option<DeadLetter>, StoreError>;

    /// List dead letters.
    fn list_dead_letters(&self, filter: &DeadLetterFilter) -> Result<Vec<DeadLetter>, StoreError>;

    /// Remove a dead letter (after retry or purge).
    fn remove_dead_letter(&self, id: Uuid) -> Result<(), StoreError>;

    /// Purge expired dead letters.
    fn purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError>;
}

/// Combined store trait for convenience.
pub trait Store: JobStore + ExecutionStore + RunnerStore + DeadLetterStore {}
