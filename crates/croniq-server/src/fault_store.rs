//! Store test double for fault injection.
//!
//! Delegates every call to a real inner store, but returns an injected
//! `StoreError` for whichever operations a test arms. It lives outside any one
//! test module because two very different call paths need the same double: the
//! retry-persist failure in [`crate::completion`], and the fail-closed
//! concurrency-group guard in [`crate::api`] (issue #546).
//!
//! Only the arming flags are new — the delegating `Store` impl below is the
//! one the completion tests have used since the retry-persist path grew a
//! test, moved here rather than copied.

#![cfg(test)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use croniq_store::models::*;
use croniq_store::traits::*;
use uuid::Uuid;

use crate::store::DynStore;

pub struct FaultStore {
    pub inner: DynStore,
    /// `create_execution` returns a database error while set.
    pub fail_create: AtomicBool,
    /// `claim_execution` returns a database error while set — an
    /// *infrastructure* failure, as distinct from the `Conflict` / `NotFound`
    /// a stale row produces.
    pub fail_claim: AtomicBool,
    /// `count_executions_in_group_in_states` returns a database error while
    /// set. Drives the group guard's fail-closed arm (issue #546).
    pub fail_group_count: AtomicBool,
    /// `count_executions_in_states` returns a database error while set. The
    /// counterpart of [`Self::fail_group_count`], so a test can pin the
    /// deliberate asymmetry: the per-job guard dispatches on a store error,
    /// the group guard does not.
    pub fail_job_count: AtomicBool,
}

impl FaultStore {
    /// Wrap `inner` with every fault disarmed.
    pub fn wrap(inner: DynStore) -> Arc<Self> {
        Arc::new(Self {
            inner,
            fail_create: AtomicBool::new(false),
            fail_claim: AtomicBool::new(false),
            fail_group_count: AtomicBool::new(false),
            fail_job_count: AtomicBool::new(false),
        })
    }

    pub fn arm_create(&self) {
        self.fail_create.store(true, Ordering::SeqCst);
    }

    pub fn arm_claim(&self) {
        self.fail_claim.store(true, Ordering::SeqCst);
    }

    pub fn arm_group_count(&self) {
        self.fail_group_count.store(true, Ordering::SeqCst);
    }

    pub fn arm_job_count(&self) {
        self.fail_job_count.store(true, Ordering::SeqCst);
    }
}

macro_rules! delegate {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)+) => {
        $(fn $name(&self, $($arg: $ty),*) -> Result<$ret, StoreError> {
            self.inner.$name($($arg),*)
        })+
    };
}

impl JobStore for FaultStore {
    delegate! {
        get_job_state(job_key: &str) -> Option<JobState>;
        upsert_job_state(state: &JobState) -> ();
        list_job_states() -> Vec<JobState>;
        delete_job_state(job_key: &str) -> ();
        list_register_fires() -> Vec<croniq_store::models::JobRegisterFire>;
        upsert_register_fire(record: &croniq_store::models::JobRegisterFire) -> ();
        delete_register_fire(job_key: &str) -> ();
    }
}

#[allow(clippy::too_many_arguments)] // delegated complete_execution
impl ExecutionStore for FaultStore {
    fn create_execution(&self, execution: &Execution) -> Result<(), StoreError> {
        if self.fail_create.load(Ordering::SeqCst) {
            return Err(StoreError::Database(
                "injected create_execution failure".into(),
            ));
        }
        self.inner.create_execution(execution)
    }

    fn claim_execution(
        &self,
        id: Uuid,
        runner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Execution, StoreError> {
        if self.fail_claim.load(Ordering::SeqCst) {
            return Err(StoreError::Database(
                "injected claim_execution failure".into(),
            ));
        }
        self.inner.claim_execution(id, runner_id, now)
    }

    fn count_executions_in_states(
        &self,
        job_key: &str,
        states: &[ExecutionState],
    ) -> Result<u64, StoreError> {
        if self.fail_job_count.load(Ordering::SeqCst) {
            return Err(StoreError::Database(
                "injected count_executions_in_states failure".into(),
            ));
        }
        self.inner.count_executions_in_states(job_key, states)
    }

    fn count_executions_in_group_in_states(
        &self,
        group: &str,
        states: &[ExecutionState],
    ) -> Result<u64, StoreError> {
        if self.fail_group_count.load(Ordering::SeqCst) {
            return Err(StoreError::Database(
                "injected count_executions_in_group_in_states failure".into(),
            ));
        }
        self.inner
            .count_executions_in_group_in_states(group, states)
    }
    delegate! {
        create_execution_and_advance_job_state(execution: &Execution, job_state: &JobState) -> ();
        get_execution(id: Uuid) -> Option<Execution>;
        complete_execution(id: Uuid, runner_id: Option<&str>, state: ExecutionState, duration_ms: Option<i64>, error: Option<&str>, dead_reason: Option<&str>, now: DateTime<Utc>) -> bool;
        find_queued_executions(capabilities: &[String], limit: u32) -> Vec<Execution>;
        list_executions(filter: &ExecutionFilter) -> Vec<Execution>;
        list_claimed_older_than(cutoff: DateTime<Utc>, limit: u32) -> Vec<Execution>;
        find_execution_by_idempotency_key(job_key: &str, idempotency_key: &str, window_start: DateTime<Utc>) -> Option<Execution>;
        requeue_abandoned(runner_id: &str, now: DateTime<Utc>) -> Vec<Uuid>;
        requeue_if_claimed(id: Uuid, now: DateTime<Utc>) -> bool;
        cancel_execution(id: Uuid, now: DateTime<Utc>) -> ();
        count_by_state() -> HashMap<ExecutionState, u64>;
        job_execution_metrics() -> Vec<JobExecutionMetrics>;
        prune_executions_older_than(cutoff: DateTime<Utc>, limit: u32) -> u64;
        prune_executions_keep_last(job_key: &str, keep_last: u32, limit: u32) -> u64;
    }
}

impl RunnerStore for FaultStore {
    delegate! {
        upsert_runner(runner: &Runner) -> ();
        get_runner(runner_id: &str) -> Option<Runner>;
        list_runners() -> Vec<Runner>;
        remove_runner(runner_id: &str) -> ();
        update_poll(runner_id: &str, inflight: &[Uuid], now: DateTime<Utc>) -> ();
        runner_identity_bind(runner_id: &str, owner_id: &str, now: DateTime<Utc>) -> String;
        runner_identity_owner(runner_id: &str) -> Option<String>;
        runner_identity_release(runner_id: &str) -> ();
    }
}

impl DeadLetterStore for FaultStore {
    delegate! {
        add_dead_letter(dl: &DeadLetter) -> ();
        complete_as_dead(execution_id: Uuid, runner_id: Option<&str>, duration_ms: Option<i64>, error: Option<&str>, dead_letter: &DeadLetter, now: DateTime<Utc>) -> bool;
        replay_dead_letter(dead_letter_id: Uuid, execution: &Execution) -> ();
        get_dead_letter(id: Uuid) -> Option<DeadLetter>;
        list_dead_letters(filter: &DeadLetterFilter) -> Vec<DeadLetter>;
        remove_dead_letter(id: Uuid) -> ();
        remove_dead_letters(ids: &[Uuid]) -> u64;
        clear_dead_letters(job_key: Option<&str>) -> u64;
        purge_expired(now: DateTime<Utc>) -> u64;
    }
}

impl AuthStore for FaultStore {
    delegate! {
        create_client(client: &ApiClient) -> ();
        get_client(client_id: &str) -> Option<ApiClient>;
        list_clients() -> Vec<ApiClient>;
        delete_client(client_id: &str) -> ();
        create_api_key(key: &ApiKey) -> ();
        find_api_key_by_hash(key_hash: &str) -> Option<ApiKey>;
        revoke_api_key(key_id: &str, now: DateTime<Utc>) -> ();
        restore_api_key(key_id: &str) -> ();
        list_api_keys(client_id: &str) -> Vec<ApiKey>;
        set_api_key_expiry(key_id: &str, expires_at: Option<DateTime<Utc>>) -> ();
        get_credentials(username: &str) -> Option<PasswordCredential>;
        upsert_credentials(cred: &PasswordCredential) -> ();
        create_refresh_token(token: &RefreshToken) -> ();
        validate_refresh_token(token_hash: &str) -> Option<RefreshToken>;
        revoke_refresh_token(token_hash: &str, now: DateTime<Utc>) -> ();
        users_create(user: &User) -> ();
        users_get_by_id(user_id: &str) -> Option<User>;
        users_get_by_username(username: &str) -> Option<User>;
        users_list() -> Vec<User>;
        users_update(user: &User) -> ();
        users_delete(user_id: &str) -> ();
        users_set_last_login(user_id: &str, at: DateTime<Utc>) -> ();
        users_count_active_admins() -> u64;
        users_token_generation(user_id: &str) -> Option<i64>;
        users_bump_token_generation(user_id: &str) -> ();
        invitations_create(invite: &Invitation) -> ();
        invitations_get(invitation_id: &str) -> Option<Invitation>;
        invitations_get_by_token_hash(token_hash: &str) -> Option<Invitation>;
        invitations_list() -> Vec<Invitation>;
        invitations_mark_accepted(invitation_id: &str, at: DateTime<Utc>) -> ();
        invitations_revoke(invitation_id: &str, at: DateTime<Utc>) -> ();
        password_resets_create(reset: &PasswordReset) -> ();
        password_resets_get_by_token_hash(token_hash: &str) -> Option<PasswordReset>;
        password_resets_mark_used(reset_id: &str, at: DateTime<Utc>) -> ();
        totp_upsert(secret: &TotpSecret) -> ();
        totp_get(user_id: &str) -> Option<TotpSecret>;
        totp_set_enabled(user_id: &str, enabled: bool, confirmed_at: Option<DateTime<Utc>>) -> ();
        totp_delete(user_id: &str) -> ();
        recovery_codes_replace_all(user_id: &str, codes: &[RecoveryCode]) -> ();
        recovery_codes_find_unused(user_id: &str, code_hash: &str) -> Option<RecoveryCode>;
        recovery_codes_mark_used(code_id: &str, at: DateTime<Utc>) -> ();
        recovery_codes_count_unused(user_id: &str) -> u64;
        pat_create(pat: &PersonalAccessToken) -> ();
        pat_find_by_hash(token_hash: &str) -> Option<PersonalAccessToken>;
        pat_list(user_id: &str) -> Vec<PersonalAccessToken>;
        pat_revoke(token_id: &str, at: DateTime<Utc>) -> ();
        pat_touch_last_used(token_id: &str, at: DateTime<Utc>) -> ();
        oidc_link(identity: &OidcIdentity) -> ();
        oidc_get_by_subject(provider: &str, subject: &str) -> Option<OidcIdentity>;
        oidc_touch_last_login(provider: &str, subject: &str, at: DateTime<Utc>) -> ();
        oidc_pending_create(pending: &OidcPendingLogin) -> ();
        oidc_pending_take(state: &str) -> Option<OidcPendingLogin>;
        oidc_pending_purge_expired(now: DateTime<Utc>) -> u64;
        audit_log(event: &AuditEvent) -> ();
        audit_list(filter: &AuditFilter) -> Vec<AuditEvent>;
    }
}

impl JobDefinitionStore for FaultStore {
    delegate! {
        create_job_definition(job: &JobDefinition) -> ();
        get_job_definition(job_key: &str) -> Option<JobDefinition>;
        list_job_definitions() -> Vec<JobDefinition>;
        delete_job_definition(job_key: &str) -> ();
    }
}

impl TriggerDefinitionStore for FaultStore {
    delegate! {
        create_trigger(trigger: &TriggerDefinition) -> ();
        get_trigger(trigger_id: &str) -> Option<TriggerDefinition>;
        list_triggers(job_key: Option<&str>) -> Vec<TriggerDefinition>;
        delete_trigger(trigger_id: &str) -> ();
        update_trigger(trigger: &TriggerDefinition) -> bool;
    }
}

impl CalendarDefinitionStore for FaultStore {
    delegate! {
        create_calendar(cal: &CalendarDefinition) -> ();
        get_calendar(calendar_id: &str) -> Option<CalendarDefinition>;
        list_calendars() -> Vec<CalendarDefinition>;
        delete_calendar(calendar_id: &str) -> ();
    }
}

impl DslAdoptionStore for FaultStore {
    delegate! {
        insert_adoption(adoption: &DslAdoption) -> ();
        delete_adoption(resource_type: &str, resource_key: &str) -> bool;
        is_adopted(resource_type: &str, resource_key: &str) -> bool;
        list_adoptions(resource_type: &str) -> Vec<DslAdoption>;
    }
}

impl ExecutionLogStore for FaultStore {
    delegate! {
        append_log(entry: &ExecutionLogEntry) -> ();
        append_logs_batch(entries: &[ExecutionLogEntry]) -> ();
        read_logs(execution_id: Uuid, limit: u32) -> Vec<ExecutionLogEntry>;
    }
}

impl AlertStore for FaultStore {
    delegate! {
        record_alert_delivery(delivery: &AlertDelivery) -> ();
        list_alert_deliveries(filter: &AlertDeliveryFilter) -> Vec<AlertDelivery>;
        get_alert_delivery(delivery_id: &str) -> Option<AlertDelivery>;
        last_alert_fire_at(rule_name: &str, job_key: &str) -> Option<DateTime<Utc>>;
        upsert_alert_rule_override(ov: &AlertRuleOverride) -> ();
        get_alert_rule_override(rule_name: &str) -> Option<AlertRuleOverride>;
        list_alert_rule_overrides() -> Vec<AlertRuleOverride>;
        delete_alert_rule_override(rule_name: &str) -> bool;
        delete_expired_alert_rule_overrides(now: DateTime<Utc>) -> Vec<String>;
        prune_alert_rule_overrides(valid_rule_names: &[String]) -> Vec<String>;
    }
}

impl MaintenanceStore for FaultStore {
    delegate! {
        get_maintenance() -> MaintenanceState;
        set_maintenance(state: &MaintenanceState) -> ();
    }
}

impl croniq_store::traits::Store for FaultStore {}
