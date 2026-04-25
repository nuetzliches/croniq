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

    /// Atomically persist a new execution AND update the job state.
    ///
    /// Used by the scheduler tick to close the window between two
    /// previously-independent writes. Without this, a crash after
    /// `create_execution` but before `upsert_job_state` would leave the
    /// execution row in the DB while `job_state.next_fire_at` still held
    /// the old fire time — on restart the same trigger fires again and
    /// produces a duplicate execution. Implementations must commit both
    /// rows in a single transaction (or refuse).
    fn create_execution_and_advance_job_state(
        &self,
        execution: &Execution,
        job_state: &JobState,
    ) -> Result<(), StoreError>;

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

/// Auth persistence.
pub trait AuthStore {
    // API Clients
    fn create_client(&self, client: &ApiClient) -> Result<(), StoreError>;
    fn get_client(&self, client_id: &str) -> Result<Option<ApiClient>, StoreError>;
    fn list_clients(&self) -> Result<Vec<ApiClient>, StoreError>;
    fn delete_client(&self, client_id: &str) -> Result<(), StoreError>;

    // API Keys
    fn create_api_key(&self, key: &ApiKey) -> Result<(), StoreError>;
    fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, StoreError>;
    fn revoke_api_key(&self, key_id: &str, now: DateTime<Utc>) -> Result<(), StoreError>;
    fn list_api_keys(&self, client_id: &str) -> Result<Vec<ApiKey>, StoreError>;

    // Password credentials
    fn get_credentials(&self, username: &str) -> Result<Option<PasswordCredential>, StoreError>;
    fn upsert_credentials(&self, cred: &PasswordCredential) -> Result<(), StoreError>;

    // Refresh tokens
    fn create_refresh_token(&self, token: &RefreshToken) -> Result<(), StoreError>;
    fn validate_refresh_token(&self, token_hash: &str) -> Result<Option<RefreshToken>, StoreError>;
    fn revoke_refresh_token(&self, token_hash: &str, now: DateTime<Utc>) -> Result<(), StoreError>;
}

/// Job definition persistence (CRUD for job definitions, distinct from runtime JobState).
pub trait JobDefinitionStore {
    fn create_job_definition(&self, job: &JobDefinition) -> Result<(), StoreError>;
    fn get_job_definition(&self, job_key: &str) -> Result<Option<JobDefinition>, StoreError>;
    fn list_job_definitions(&self) -> Result<Vec<JobDefinition>, StoreError>;
    fn delete_job_definition(&self, job_key: &str) -> Result<(), StoreError>;
}

/// Trigger definition persistence.
pub trait TriggerDefinitionStore {
    fn create_trigger(&self, trigger: &TriggerDefinition) -> Result<(), StoreError>;
    fn get_trigger(&self, trigger_id: &str) -> Result<Option<TriggerDefinition>, StoreError>;
    fn list_triggers(&self, job_key: Option<&str>) -> Result<Vec<TriggerDefinition>, StoreError>;
    fn delete_trigger(&self, trigger_id: &str) -> Result<(), StoreError>;
}

/// Calendar definition persistence.
pub trait CalendarDefinitionStore {
    fn create_calendar(&self, cal: &CalendarDefinition) -> Result<(), StoreError>;
    fn get_calendar(&self, calendar_id: &str) -> Result<Option<CalendarDefinition>, StoreError>;
    fn list_calendars(&self) -> Result<Vec<CalendarDefinition>, StoreError>;
    fn delete_calendar(&self, calendar_id: &str) -> Result<(), StoreError>;
}

/// Execution log persistence.
pub trait ExecutionLogStore {
    fn append_log(&self, entry: &ExecutionLogEntry) -> Result<(), StoreError>;
    fn read_logs(
        &self,
        execution_id: Uuid,
        limit: u32,
    ) -> Result<Vec<ExecutionLogEntry>, StoreError>;
}

/// Combined store trait for convenience.
pub trait Store:
    JobStore
    + ExecutionStore
    + RunnerStore
    + DeadLetterStore
    + AuthStore
    + JobDefinitionStore
    + TriggerDefinitionStore
    + CalendarDefinitionStore
    + ExecutionLogStore
{
}
