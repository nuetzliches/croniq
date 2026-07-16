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

    /// Find the most recent execution carrying the given trigger
    /// idempotency key for a job (issue #279). Matches when the execution
    /// is still in-flight (`queued` / `claimed`) OR was created at or after
    /// `window_start` (even if it already finished). Returns `None` when no
    /// execution matches — the caller then proceeds with a fresh trigger.
    fn find_execution_by_idempotency_key(
        &self,
        job_key: &str,
        idempotency_key: &str,
        window_start: DateTime<Utc>,
    ) -> Result<Option<Execution>, StoreError>;

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

    /// Count executions of `job_key` currently in any of `states`.
    ///
    /// Backs the per-job concurrency guard (issue #278): the claim path
    /// counts `Claimed` rows — the store's single in-flight state — to decide
    /// whether a `singleton` / `max_concurrent` job has a free slot. An empty
    /// `states` slice returns 0.
    fn count_executions_in_states(
        &self,
        job_key: &str,
        states: &[ExecutionState],
    ) -> Result<u64, StoreError>;

    /// Per-job execution aggregates for the `/metrics` endpoint, computed on
    /// demand with one grouped scan (no separate counters are persisted).
    /// Returns one entry per `job_key` that has at least one execution.
    fn job_execution_metrics(&self) -> Result<Vec<JobExecutionMetrics>, StoreError>;
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

    /// Mark an execution as dead AND insert the matching dead-letter row in
    /// a single transaction. Both writes commit together or both fail.
    ///
    /// Replaces the previous two-call pattern (`complete_execution(.., Dead)`
    /// followed by `add_dead_letter`) where errors were swallowed and the
    /// executions table could end up with `state='dead'` rows that had no
    /// corresponding `dead_letters` row, leaving the Dead Letters UI page
    /// empty (#104).
    fn complete_as_dead(
        &self,
        execution_id: Uuid,
        duration_ms: Option<i64>,
        error: Option<&str>,
        dead_letter: &DeadLetter,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError>;

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

    // Users — identity decoupled from auth method. Migration 011 backfills
    // existing password_credentials rows into users with role=admin; new
    // multi-user flows (invitations, OIDC JIT, PATs) all attach to a row
    // here. `users_create` is upsert-on-user_id; `users_update` rejects
    // attempts to remove the last admin.
    fn users_create(&self, user: &User) -> Result<(), StoreError>;
    fn users_get_by_id(&self, user_id: &str) -> Result<Option<User>, StoreError>;
    fn users_get_by_username(&self, username: &str) -> Result<Option<User>, StoreError>;
    fn users_list(&self) -> Result<Vec<User>, StoreError>;
    fn users_update(&self, user: &User) -> Result<(), StoreError>;
    fn users_delete(&self, user_id: &str) -> Result<(), StoreError>;
    fn users_set_last_login(&self, user_id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    /// Count active users with role=admin. Used by user_update / user_delete to
    /// prevent the last admin from being demoted or removed (avoids lock-out).
    fn users_count_active_admins(&self) -> Result<u64, StoreError>;

    // Invitations — admin issues, user redeems with a raw token.
    fn invitations_create(&self, invite: &Invitation) -> Result<(), StoreError>;
    fn invitations_get(&self, invitation_id: &str) -> Result<Option<Invitation>, StoreError>;
    fn invitations_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Invitation>, StoreError>;
    fn invitations_list(&self) -> Result<Vec<Invitation>, StoreError>;
    fn invitations_mark_accepted(
        &self,
        invitation_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError>;
    fn invitations_revoke(&self, invitation_id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;

    // Password resets — same hash-on-create / raw-token-on-redeem pattern.
    fn password_resets_create(&self, reset: &PasswordReset) -> Result<(), StoreError>;
    fn password_resets_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordReset>, StoreError>;
    fn password_resets_mark_used(
        &self,
        reset_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError>;

    // TOTP secrets — one per user_id (PK). Upsert lets `/totp/setup`
    // be retried (the secret stays at enabled=0 until confirmed).
    fn totp_upsert(&self, secret: &TotpSecret) -> Result<(), StoreError>;
    fn totp_get(&self, user_id: &str) -> Result<Option<TotpSecret>, StoreError>;
    fn totp_set_enabled(
        &self,
        user_id: &str,
        enabled: bool,
        confirmed_at: Option<DateTime<Utc>>,
    ) -> Result<(), StoreError>;
    fn totp_delete(&self, user_id: &str) -> Result<(), StoreError>;

    // Recovery codes — bulk insert at TOTP confirm time, single-use
    // consumption via mark_used. Replace_all is used by the
    // regenerate-codes endpoint.
    fn recovery_codes_replace_all(
        &self,
        user_id: &str,
        codes: &[RecoveryCode],
    ) -> Result<(), StoreError>;
    fn recovery_codes_find_unused(
        &self,
        user_id: &str,
        code_hash: &str,
    ) -> Result<Option<RecoveryCode>, StoreError>;
    fn recovery_codes_mark_used(&self, code_id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    fn recovery_codes_count_unused(&self, user_id: &str) -> Result<u64, StoreError>;

    // Personal Access Tokens — user-bound API credentials.
    fn pat_create(&self, pat: &PersonalAccessToken) -> Result<(), StoreError>;
    fn pat_find_by_hash(&self, token_hash: &str)
    -> Result<Option<PersonalAccessToken>, StoreError>;
    fn pat_list(&self, user_id: &str) -> Result<Vec<PersonalAccessToken>, StoreError>;
    fn pat_revoke(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;
    fn pat_touch_last_used(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError>;

    // OIDC — JIT-linked external identities + short-TTL state-param store.
    fn oidc_link(&self, identity: &OidcIdentity) -> Result<(), StoreError>;
    fn oidc_get_by_subject(
        &self,
        provider: &str,
        subject: &str,
    ) -> Result<Option<OidcIdentity>, StoreError>;
    fn oidc_touch_last_login(
        &self,
        provider: &str,
        subject: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError>;
    fn oidc_pending_create(&self, pending: &OidcPendingLogin) -> Result<(), StoreError>;
    fn oidc_pending_take(&self, state: &str) -> Result<Option<OidcPendingLogin>, StoreError>;
    /// Purge expired oidc_pending_logins rows. Called opportunistically
    /// (e.g. before every callback handler invocation) — best-effort.
    fn oidc_pending_purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError>;

    // Audit log — append-only.
    fn audit_log(&self, event: &AuditEvent) -> Result<(), StoreError>;
    fn audit_list(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError>;
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
    /// Update the editable fields of an existing API-managed trigger.
    /// Returns `Ok(true)` when a row was updated, `Ok(false)` when no
    /// row matched (`trigger_id` unknown or `managed_by != 'api'`).
    /// Implementations MUST refuse to update DSL-owned triggers — the
    /// 409 vs 404 distinction is enforced one layer up in the handler.
    fn update_trigger(&self, trigger: &TriggerDefinition) -> Result<bool, StoreError>;
}

/// Calendar definition persistence.
pub trait CalendarDefinitionStore {
    fn create_calendar(&self, cal: &CalendarDefinition) -> Result<(), StoreError>;
    fn get_calendar(&self, calendar_id: &str) -> Result<Option<CalendarDefinition>, StoreError>;
    fn list_calendars(&self) -> Result<Vec<CalendarDefinition>, StoreError>;
    fn delete_calendar(&self, calendar_id: &str) -> Result<(), StoreError>;
}

/// DSL adoption tracking. An adoption record means the loader should skip
/// the DSL definition for the named resource on next reload — the API copy
/// in the corresponding store table wins.
pub trait DslAdoptionStore {
    /// Insert an adoption record. Idempotent — replaces an existing record
    /// with the same `(resource_type, resource_key)` pair (re-adopt scenario
    /// after an unadopt + re-adopt).
    fn insert_adoption(&self, adoption: &DslAdoption) -> Result<(), StoreError>;
    /// Remove an adoption record. Returns `Ok(true)` when a row was removed,
    /// `Ok(false)` when no matching adoption existed.
    fn delete_adoption(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError>;
    /// Returns `true` if an adoption record exists for the given resource.
    fn is_adopted(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError>;
    /// List all adoption records for the given `resource_type`. Used by the
    /// loader to build the exclusion set.
    fn list_adoptions(&self, resource_type: &str) -> Result<Vec<DslAdoption>, StoreError>;
}

/// Alert delivery log (issue #140). Recorded by the evaluator for
/// each rule fire so operators (and the future Alerts UI tab) can
/// see what fired, when, and whether delivery succeeded.
pub trait AlertStore {
    /// Insert a delivery row. Idempotent on `delivery_id` (PRIMARY KEY)
    /// — re-inserting the same id with the same payload is a no-op,
    /// re-inserting with different payload is an error.
    fn record_alert_delivery(&self, delivery: &AlertDelivery) -> Result<(), StoreError>;

    /// List recent deliveries, newest first. Used by the future Alerts
    /// tab and an admin diagnostic endpoint.
    fn list_alert_deliveries(
        &self,
        filter: &AlertDeliveryFilter,
    ) -> Result<Vec<AlertDelivery>, StoreError>;

    /// Look up a single delivery by `delivery_id`. Backs the
    /// `GET /v1/alerts/deliveries/{id}` admin endpoint added in
    /// issue #140 PR-5.
    fn get_alert_delivery(&self, delivery_id: &str) -> Result<Option<AlertDelivery>, StoreError>;

    /// Look up the most recent fire timestamp for a (rule, job_key)
    /// pair across `Delivered`, `Failed`, and `Throttled` states.
    ///
    /// Used by the evaluator on boot to seed the in-memory throttle
    /// map so a server restart doesn't reset the suppression window
    /// for jobs that were recently quieted. Returns `None` when the
    /// rule has never fired for that job_key.
    fn last_alert_fire_at(
        &self,
        rule_name: &str,
        job_key: &str,
    ) -> Result<Option<DateTime<Utc>>, StoreError>;

    // ─── Operational overrides (issue #231, Phase 1) ───

    /// Insert or replace the override row for a rule (PRIMARY KEY on
    /// `rule_name`). A set-action overwrites any prior override wholesale
    /// — partial merges happen in the handler before this call.
    fn upsert_alert_rule_override(&self, ov: &AlertRuleOverride) -> Result<(), StoreError>;

    /// Look up the override for a single rule. `None` = pure DSL behaviour.
    fn get_alert_rule_override(
        &self,
        rule_name: &str,
    ) -> Result<Option<AlertRuleOverride>, StoreError>;

    /// List all override rows. Used by `GET /v1/alerts/config` to surface
    /// override state inline. Newest-set first.
    fn list_alert_rule_overrides(&self) -> Result<Vec<AlertRuleOverride>, StoreError>;

    /// Remove the override for a rule. Returns `Ok(true)` when a row was
    /// removed, `Ok(false)` when none existed. Backs `DELETE …/override`.
    fn delete_alert_rule_override(&self, rule_name: &str) -> Result<bool, StoreError>;

    /// Delete every override whose `expires_at <= now`. Returns the names
    /// of the rules whose overrides were cleared, so the watchdog can emit
    /// one `alerts.override.cleared` audit event per row.
    fn delete_expired_alert_rule_overrides(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<String>, StoreError>;

    /// Delete override rows whose `rule_name` is not in `valid_rule_names`.
    /// Implements the FK-cascade-by-name: when a DSL rule is removed, its
    /// orphaned override is pruned (called at boot after loading the
    /// alerts config). Returns the pruned rule names.
    fn prune_alert_rule_overrides(
        &self,
        valid_rule_names: &[String],
    ) -> Result<Vec<String>, StoreError>;
}

/// Global maintenance switch persistence (single-row table).
pub trait MaintenanceStore {
    /// Read the current maintenance state, or the all-off default when the
    /// switch has never been written.
    fn get_maintenance(&self) -> Result<MaintenanceState, StoreError>;

    /// Upsert the singleton maintenance row.
    fn set_maintenance(&self, state: &MaintenanceState) -> Result<(), StoreError>;
}

/// Execution log persistence.
pub trait ExecutionLogStore {
    fn append_log(&self, entry: &ExecutionLogEntry) -> Result<(), StoreError>;

    /// Append many log entries for the same execution in one transaction,
    /// auto-assigning each entry's `seq` to be strictly increasing within
    /// its execution. Used by the per-line log path (#108) so a job that
    /// produces 3000 lines of output doesn't take 3000 separate
    /// lock+INSERT round-trips.
    ///
    /// The `seq` field on the input entries is ignored — the store is the
    /// source of truth for ordering. The other fields are persisted as-is.
    /// Implementations must ensure all entries commit together or none do.
    fn append_logs_batch(&self, entries: &[ExecutionLogEntry]) -> Result<(), StoreError>;

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
    + DslAdoptionStore
    + ExecutionLogStore
    + AlertStore
    + MaintenanceStore
{
}
