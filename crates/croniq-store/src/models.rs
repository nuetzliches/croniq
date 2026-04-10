//! Domain models for persistence.
//!
//! These are the 4 core entities:
//! - JobState: runtime state of a job (next fire, fire count, status)
//! - Execution: the central runtime entity (queued → claimed → completed|failed|dead)
//! - Runner: a connected execution agent
//! - DeadLetter: failed executions for inspection

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Job State ───

/// Runtime state of a job. Managed by the scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobState {
    pub job_key: String,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub fire_count: u64,
    pub status: JobStatus,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Active,
    Paused,
    Disabled,
    /// Trigger has fired its last scheduled time (e.g. `once at …`).
    /// On restart, the trigger is not re-armed — prevents double-firing.
    Exhausted,
}

// ─── Execution ───

/// The central runtime entity. An execution represents a single invocation
/// of a job, from queue to completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: Uuid,
    pub job_key: String,
    pub fire_at: DateTime<Utc>,
    pub attempt: u32,
    pub state: ExecutionState,

    /// Runner that claimed this execution.
    pub runner_id: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,

    pub error: Option<String>,
    pub dead_reason: Option<String>,

    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}

/// Execution lifecycle states.
///
/// ```text
///          ┌─────────────────────────────────┐
///          │                                 │
/// queued → claimed → completed               │
///                  → failed ──→ queued (retry)│
///                             → dead ────────┘
///                  → abandoned → queued (reassign)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionState {
    Queued,
    Claimed,
    Completed,
    Failed,
    Dead,
    Cancelled,
}

// ─── Runner ───

/// A connected execution agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runner {
    pub runner_id: String,
    pub capabilities: Vec<String>,
    pub max_inflight: u32,
    pub last_poll_at: DateTime<Utc>,
    pub inflight: Vec<Uuid>,
    pub status: RunnerStatus,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerStatus {
    Online,
    Stale,
    Dead,
}

// ─── Dead Letter ───

/// A dead-lettered execution for inspection and retry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetter {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub job_key: String,
    pub fire_at: DateTime<Utc>,
    pub attempt: u32,
    pub error: String,
    pub dead_reason: String,
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

// ─── Query types ───

/// Filter for listing executions.
#[derive(Debug, Clone, Default)]
pub struct ExecutionFilter {
    pub job_key: Option<String>,
    pub state: Option<ExecutionState>,
    pub runner_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

/// Filter for listing dead letters.
#[derive(Debug, Clone, Default)]
pub struct DeadLetterFilter {
    pub job_key: Option<String>,
    pub limit: Option<u32>,
}

// ─── Auth ───

/// An API client that can obtain tokens and API keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiClient {
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// A hashed API key bound to an API client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key_id: String,
    pub client_id: String,
    /// SHA-256 hash of the raw key (hex-encoded).
    pub key_hash: String,
    /// Key prefix for display (first 8 chars of raw key).
    pub key_prefix: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A refresh token for session management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshToken {
    /// SHA-256 hash of the raw token.
    pub token_hash: String,
    pub client_id: String,
    pub user_id: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Password credentials for user authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordCredential {
    pub user_id: String,
    pub username: String,
    /// bcrypt hash.
    pub password_hash: String,
    pub failed_attempts: u32,
    pub locked_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
