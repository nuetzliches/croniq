//! Runtime-safe wrapper around [`PgStore`] for use inside a Tokio runtime.
//!
//! # Why this exists
//!
//! [`PgStore`] is built on the **synchronous** `postgres` crate, which holds
//! its own `tokio::runtime::Runtime` and calls `runtime.block_on(...)` on every
//! connect / query / execute. Calling that from a thread that is *already*
//! inside a Tokio runtime — which is exactly where croniq-server drives the
//! store (async `main`, the scheduler tick, the completion processor, the
//! watchdog, and every axum handler) — panics with:
//!
//! > Cannot start a runtime from within a runtime.
//!
//! The `Store` trait is synchronous and called pervasively from those async
//! contexts, so `spawn_blocking` at the connect site alone does not help.
//!
//! # The fix: an actor on a dedicated OS thread
//!
//! [`PgStoreHandle`] owns a plain `std::thread` (**not** a Tokio worker) that
//! holds the `PgStore`. That thread has no ambient Tokio runtime, so the
//! `postgres` crate's internal `block_on` runs fine — the panic is impossible
//! there. Every `Store` method on the handle packages its work into a closure,
//! ships it to the actor thread over a channel, and blocks on a per-call
//! response channel. A plain `mpsc::recv()` parks the calling thread but does
//! **not** create or enter a runtime, so it never trips the panic (unlike
//! `block_on`).
//!
//! Because the handle implements the synchronous `Store` trait unchanged, no
//! caller needs to know Postgres is involved — it is just another
//! `Arc<dyn Store + Send + Sync>`.
//!
//! # Known characteristics (matching the existing SQLite path)
//!
//! * A store call blocks the calling (Tokio worker) thread for the duration of
//!   the DB round-trip. This is the same blocking model the SQLite backend
//!   already uses (`SqliteStore` wraps one `Mutex<Connection>`); the single
//!   actor thread + single connection here mirrors `PgStore`'s existing
//!   `Mutex<Client>`, so there is no serialization regression. A connection
//!   pool / multiple actor threads is a possible future optimization.
//! * There is no automatic reconnect if the Postgres connection drops mid-run
//!   (same as the underlying `PgStore`). Subsequent calls surface
//!   [`StoreError::Database`]; the server's scheduler supervisor and the
//!   container `restart:` policy recover by restarting the process.

use crate::models::*;
use crate::pg::PgStore;
use crate::traits::*;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread::JoinHandle;
use uuid::Uuid;

/// A unit of work handed to the actor thread. Each closure captures its own
/// (owned) arguments plus a response sender, runs the real `PgStore` method,
/// and ships the result back.
type Job = Box<dyn FnOnce(&PgStore) + Send>;

/// A `Store` implementation that drives a [`PgStore`] living on a dedicated OS
/// thread, so it is safe to call from inside a Tokio runtime. See the module
/// docs for the full rationale.
pub struct PgStoreHandle {
    /// Wrapped in a `Mutex` purely so the handle is `Sync` without relying on
    /// `mpsc::Sender: Sync`. The lock is held only for the (non-blocking)
    /// enqueue, never across the DB round-trip.
    tx: Mutex<mpsc::Sender<Job>>,
    /// Kept alive for the handle's lifetime; the thread winds down on its own
    /// once every `Sender` is dropped. Never joined (that could block).
    _thread: JoinHandle<()>,
}

impl PgStoreHandle {
    /// Connect to PostgreSQL and start the actor thread. Connection + migration
    /// happen on the actor thread (where the `postgres` crate's `block_on` is
    /// safe); any failure there propagates out of this call.
    pub fn connect(connection_string: &str) -> Result<Self, StoreError> {
        let dsn = connection_string.to_owned();
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        // One-shot: the actor reports whether connect + migrate succeeded so
        // `connect` can fail synchronously instead of returning a dead handle.
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), StoreError>>(1);

        let thread = std::thread::Builder::new()
            .name("croniq-pg-store".into())
            .spawn(move || {
                // block_on runs HERE, on a plain OS thread with no ambient
                // Tokio runtime — so it cannot trip the "runtime within a
                // runtime" panic that blocks the async server.
                let store = match PgStore::connect(&dsn) {
                    Ok(store) => {
                        // If the caller already gave up there is nothing to
                        // serve — just exit.
                        if ready_tx.send(Ok(())).is_err() {
                            return;
                        }
                        store
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // Serve calls until every `PgStoreHandle` (and thus every
                // `Sender`) is dropped, at which point `recv` errors and the
                // thread winds down.
                while let Ok(job) = job_rx.recv() {
                    job(&store);
                }
            })
            .map_err(|e| StoreError::Database(format!("failed to spawn pg store thread: {e}")))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx: Mutex::new(job_tx),
                _thread: thread,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(StoreError::Database(
                "pg store thread exited before signalling readiness".into(),
            )),
        }
    }

    /// Run `f` on the actor thread and block until it returns. All `Store`
    /// methods below funnel through here.
    fn call<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&PgStore) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let (resp_tx, resp_rx) = mpsc::sync_channel::<Result<T, StoreError>>(1);
        let job: Job = Box::new(move |store: &PgStore| {
            // Ignore the error: it only fails if the caller stopped waiting.
            let _ = resp_tx.send(f(store));
        });
        // Enqueue (lock released at the end of this statement, before we block
        // on the response).
        self.tx
            .lock()
            .map_err(|_| StoreError::Database("pg store actor channel poisoned".into()))?
            .send(job)
            .map_err(|_| StoreError::Database("pg store actor thread is gone".into()))?;
        resp_rx
            .recv()
            .map_err(|_| StoreError::Database("pg store actor dropped the response".into()))?
    }
}

// ─── JobStore ───

impl JobStore for PgStoreHandle {
    fn get_job_state(&self, job_key: &str) -> Result<Option<JobState>, StoreError> {
        let job_key = job_key.to_owned();
        self.call(move |s| s.get_job_state(&job_key))
    }

    fn upsert_job_state(&self, state: &JobState) -> Result<(), StoreError> {
        let state = state.clone();
        self.call(move |s| s.upsert_job_state(&state))
    }

    fn list_job_states(&self) -> Result<Vec<JobState>, StoreError> {
        self.call(|s| s.list_job_states())
    }

    fn delete_job_state(&self, job_key: &str) -> Result<(), StoreError> {
        let job_key = job_key.to_owned();
        self.call(move |s| s.delete_job_state(&job_key))
    }
}

// ─── ExecutionStore ───

impl ExecutionStore for PgStoreHandle {
    fn create_execution(&self, execution: &Execution) -> Result<(), StoreError> {
        let execution = execution.clone();
        self.call(move |s| s.create_execution(&execution))
    }

    fn create_execution_and_advance_job_state(
        &self,
        execution: &Execution,
        job_state: &JobState,
    ) -> Result<(), StoreError> {
        let execution = execution.clone();
        let job_state = job_state.clone();
        self.call(move |s| s.create_execution_and_advance_job_state(&execution, &job_state))
    }

    fn get_execution(&self, id: Uuid) -> Result<Option<Execution>, StoreError> {
        self.call(move |s| s.get_execution(id))
    }

    fn claim_execution(
        &self,
        id: Uuid,
        runner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Execution, StoreError> {
        let runner_id = runner_id.to_owned();
        self.call(move |s| s.claim_execution(id, &runner_id, now))
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
        let runner_id = runner_id.map(str::to_owned);
        let error = error.map(str::to_owned);
        let dead_reason = dead_reason.map(str::to_owned);
        self.call(move |s| {
            s.complete_execution(
                id,
                runner_id.as_deref(),
                state,
                duration_ms,
                error.as_deref(),
                dead_reason.as_deref(),
                now,
            )
        })
    }

    fn find_queued_executions(
        &self,
        capabilities: &[String],
        limit: u32,
    ) -> Result<Vec<Execution>, StoreError> {
        let capabilities = capabilities.to_vec();
        self.call(move |s| s.find_queued_executions(&capabilities, limit))
    }

    fn list_executions(&self, filter: &ExecutionFilter) -> Result<Vec<Execution>, StoreError> {
        let filter = filter.clone();
        self.call(move |s| s.list_executions(&filter))
    }

    fn list_claimed_older_than(
        &self,
        cutoff: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Execution>, StoreError> {
        self.call(move |s| s.list_claimed_older_than(cutoff, limit))
    }

    fn find_execution_by_idempotency_key(
        &self,
        job_key: &str,
        idempotency_key: &str,
        window_start: DateTime<Utc>,
    ) -> Result<Option<Execution>, StoreError> {
        let job_key = job_key.to_owned();
        let idempotency_key = idempotency_key.to_owned();
        self.call(move |s| {
            s.find_execution_by_idempotency_key(&job_key, &idempotency_key, window_start)
        })
    }

    fn requeue_abandoned(
        &self,
        runner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, StoreError> {
        let runner_id = runner_id.to_owned();
        self.call(move |s| s.requeue_abandoned(&runner_id, now))
    }

    fn requeue_if_claimed(&self, id: Uuid, now: DateTime<Utc>) -> Result<bool, StoreError> {
        self.call(move |s| s.requeue_if_claimed(id, now))
    }

    fn cancel_execution(&self, id: Uuid, now: DateTime<Utc>) -> Result<(), StoreError> {
        self.call(move |s| s.cancel_execution(id, now))
    }

    fn count_by_state(&self) -> Result<HashMap<ExecutionState, u64>, StoreError> {
        self.call(|s| s.count_by_state())
    }

    fn count_executions_in_states(
        &self,
        job_key: &str,
        states: &[ExecutionState],
    ) -> Result<u64, StoreError> {
        let job_key = job_key.to_owned();
        let states = states.to_vec();
        self.call(move |s| s.count_executions_in_states(&job_key, &states))
    }

    fn job_execution_metrics(&self) -> Result<Vec<JobExecutionMetrics>, StoreError> {
        self.call(|s| s.job_execution_metrics())
    }

    fn prune_executions_older_than(
        &self,
        cutoff: DateTime<Utc>,
        limit: u32,
    ) -> Result<u64, StoreError> {
        self.call(move |s| s.prune_executions_older_than(cutoff, limit))
    }

    fn prune_executions_keep_last(
        &self,
        job_key: &str,
        keep_last: u32,
        limit: u32,
    ) -> Result<u64, StoreError> {
        let job_key = job_key.to_owned();
        self.call(move |s| s.prune_executions_keep_last(&job_key, keep_last, limit))
    }
}

// ─── RunnerStore ───

impl RunnerStore for PgStoreHandle {
    fn upsert_runner(&self, runner: &Runner) -> Result<(), StoreError> {
        let runner = runner.clone();
        self.call(move |s| s.upsert_runner(&runner))
    }

    fn get_runner(&self, runner_id: &str) -> Result<Option<Runner>, StoreError> {
        let runner_id = runner_id.to_owned();
        self.call(move |s| s.get_runner(&runner_id))
    }

    fn list_runners(&self) -> Result<Vec<Runner>, StoreError> {
        self.call(|s| s.list_runners())
    }

    fn remove_runner(&self, runner_id: &str) -> Result<(), StoreError> {
        let runner_id = runner_id.to_owned();
        self.call(move |s| s.remove_runner(&runner_id))
    }

    fn update_poll(
        &self,
        runner_id: &str,
        inflight: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let runner_id = runner_id.to_owned();
        let inflight = inflight.to_vec();
        self.call(move |s| s.update_poll(&runner_id, &inflight, now))
    }

    fn runner_identity_bind(
        &self,
        runner_id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<String, StoreError> {
        let runner_id = runner_id.to_owned();
        let owner_id = owner_id.to_owned();
        self.call(move |s| s.runner_identity_bind(&runner_id, &owner_id, now))
    }

    fn runner_identity_owner(&self, runner_id: &str) -> Result<Option<String>, StoreError> {
        let runner_id = runner_id.to_owned();
        self.call(move |s| s.runner_identity_owner(&runner_id))
    }

    fn runner_identity_release(&self, runner_id: &str) -> Result<(), StoreError> {
        let runner_id = runner_id.to_owned();
        self.call(move |s| s.runner_identity_release(&runner_id))
    }
}

// ─── DeadLetterStore ───

impl DeadLetterStore for PgStoreHandle {
    fn add_dead_letter(&self, dl: &DeadLetter) -> Result<(), StoreError> {
        let dl = dl.clone();
        self.call(move |s| s.add_dead_letter(&dl))
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
        let runner_id = runner_id.map(str::to_owned);
        let error = error.map(str::to_owned);
        let dead_letter = dead_letter.clone();
        self.call(move |s| {
            s.complete_as_dead(
                execution_id,
                runner_id.as_deref(),
                duration_ms,
                error.as_deref(),
                &dead_letter,
                now,
            )
        })
    }

    fn replay_dead_letter(
        &self,
        dead_letter_id: Uuid,
        execution: &Execution,
    ) -> Result<(), StoreError> {
        let execution = execution.clone();
        self.call(move |s| s.replay_dead_letter(dead_letter_id, &execution))
    }

    fn get_dead_letter(&self, id: Uuid) -> Result<Option<DeadLetter>, StoreError> {
        self.call(move |s| s.get_dead_letter(id))
    }

    fn list_dead_letters(&self, filter: &DeadLetterFilter) -> Result<Vec<DeadLetter>, StoreError> {
        let filter = filter.clone();
        self.call(move |s| s.list_dead_letters(&filter))
    }

    fn remove_dead_letter(&self, id: Uuid) -> Result<(), StoreError> {
        self.call(move |s| s.remove_dead_letter(id))
    }

    fn remove_dead_letters(&self, ids: &[Uuid]) -> Result<u64, StoreError> {
        let ids = ids.to_vec();
        self.call(move |s| s.remove_dead_letters(&ids))
    }

    fn clear_dead_letters(&self, job_key: Option<&str>) -> Result<u64, StoreError> {
        let job_key = job_key.map(str::to_owned);
        self.call(move |s| s.clear_dead_letters(job_key.as_deref()))
    }

    fn purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        self.call(move |s| s.purge_expired(now))
    }
}

// ─── AuthStore ───

impl AuthStore for PgStoreHandle {
    fn create_client(&self, client: &ApiClient) -> Result<(), StoreError> {
        let client = client.clone();
        self.call(move |s| s.create_client(&client))
    }

    fn get_client(&self, client_id: &str) -> Result<Option<ApiClient>, StoreError> {
        let client_id = client_id.to_owned();
        self.call(move |s| s.get_client(&client_id))
    }

    fn list_clients(&self) -> Result<Vec<ApiClient>, StoreError> {
        self.call(|s| s.list_clients())
    }

    fn delete_client(&self, client_id: &str) -> Result<(), StoreError> {
        let client_id = client_id.to_owned();
        self.call(move |s| s.delete_client(&client_id))
    }

    fn create_api_key(&self, key: &ApiKey) -> Result<(), StoreError> {
        let key = key.clone();
        self.call(move |s| s.create_api_key(&key))
    }

    fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, StoreError> {
        let key_hash = key_hash.to_owned();
        self.call(move |s| s.find_api_key_by_hash(&key_hash))
    }

    fn revoke_api_key(&self, key_id: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        let key_id = key_id.to_owned();
        self.call(move |s| s.revoke_api_key(&key_id, now))
    }

    fn list_api_keys(&self, client_id: &str) -> Result<Vec<ApiKey>, StoreError> {
        let client_id = client_id.to_owned();
        self.call(move |s| s.list_api_keys(&client_id))
    }

    fn set_api_key_expiry(
        &self,
        key_id: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(), StoreError> {
        let key_id = key_id.to_owned();
        self.call(move |s| s.set_api_key_expiry(&key_id, expires_at))
    }

    fn get_credentials(&self, username: &str) -> Result<Option<PasswordCredential>, StoreError> {
        let username = username.to_owned();
        self.call(move |s| s.get_credentials(&username))
    }

    fn upsert_credentials(&self, cred: &PasswordCredential) -> Result<(), StoreError> {
        let cred = cred.clone();
        self.call(move |s| s.upsert_credentials(&cred))
    }

    fn create_refresh_token(&self, token: &RefreshToken) -> Result<(), StoreError> {
        let token = token.clone();
        self.call(move |s| s.create_refresh_token(&token))
    }

    fn validate_refresh_token(&self, token_hash: &str) -> Result<Option<RefreshToken>, StoreError> {
        let token_hash = token_hash.to_owned();
        self.call(move |s| s.validate_refresh_token(&token_hash))
    }

    fn revoke_refresh_token(&self, token_hash: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        let token_hash = token_hash.to_owned();
        self.call(move |s| s.revoke_refresh_token(&token_hash, now))
    }

    fn users_create(&self, user: &User) -> Result<(), StoreError> {
        let user = user.clone();
        self.call(move |s| s.users_create(&user))
    }

    fn users_get_by_id(&self, user_id: &str) -> Result<Option<User>, StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.users_get_by_id(&user_id))
    }

    fn users_get_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        let username = username.to_owned();
        self.call(move |s| s.users_get_by_username(&username))
    }

    fn users_list(&self) -> Result<Vec<User>, StoreError> {
        self.call(|s| s.users_list())
    }

    fn users_update(&self, user: &User) -> Result<(), StoreError> {
        let user = user.clone();
        self.call(move |s| s.users_update(&user))
    }

    fn users_delete(&self, user_id: &str) -> Result<(), StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.users_delete(&user_id))
    }

    fn users_set_last_login(&self, user_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.users_set_last_login(&user_id, at))
    }

    fn users_count_active_admins(&self) -> Result<u64, StoreError> {
        self.call(|s| s.users_count_active_admins())
    }

    fn users_token_generation(&self, user_id: &str) -> Result<Option<i64>, StoreError> {
        let user_id = user_id.to_string();
        self.call(move |s| s.users_token_generation(&user_id))
    }

    fn users_bump_token_generation(&self, user_id: &str) -> Result<(), StoreError> {
        let user_id = user_id.to_string();
        self.call(move |s| s.users_bump_token_generation(&user_id))
    }

    fn invitations_create(&self, invite: &Invitation) -> Result<(), StoreError> {
        let invite = invite.clone();
        self.call(move |s| s.invitations_create(&invite))
    }

    fn invitations_get(&self, invitation_id: &str) -> Result<Option<Invitation>, StoreError> {
        let invitation_id = invitation_id.to_owned();
        self.call(move |s| s.invitations_get(&invitation_id))
    }

    fn invitations_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Invitation>, StoreError> {
        let token_hash = token_hash.to_owned();
        self.call(move |s| s.invitations_get_by_token_hash(&token_hash))
    }

    fn invitations_list(&self) -> Result<Vec<Invitation>, StoreError> {
        self.call(|s| s.invitations_list())
    }

    fn invitations_mark_accepted(
        &self,
        invitation_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let invitation_id = invitation_id.to_owned();
        self.call(move |s| s.invitations_mark_accepted(&invitation_id, at))
    }

    fn invitations_revoke(&self, invitation_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let invitation_id = invitation_id.to_owned();
        self.call(move |s| s.invitations_revoke(&invitation_id, at))
    }

    fn password_resets_create(&self, reset: &PasswordReset) -> Result<(), StoreError> {
        let reset = reset.clone();
        self.call(move |s| s.password_resets_create(&reset))
    }

    fn password_resets_get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PasswordReset>, StoreError> {
        let token_hash = token_hash.to_owned();
        self.call(move |s| s.password_resets_get_by_token_hash(&token_hash))
    }

    fn password_resets_mark_used(
        &self,
        reset_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let reset_id = reset_id.to_owned();
        self.call(move |s| s.password_resets_mark_used(&reset_id, at))
    }

    fn totp_upsert(&self, secret: &TotpSecret) -> Result<(), StoreError> {
        let secret = secret.clone();
        self.call(move |s| s.totp_upsert(&secret))
    }

    fn totp_get(&self, user_id: &str) -> Result<Option<TotpSecret>, StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.totp_get(&user_id))
    }

    fn totp_set_enabled(
        &self,
        user_id: &str,
        enabled: bool,
        confirmed_at: Option<DateTime<Utc>>,
    ) -> Result<(), StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.totp_set_enabled(&user_id, enabled, confirmed_at))
    }

    fn totp_delete(&self, user_id: &str) -> Result<(), StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.totp_delete(&user_id))
    }

    fn recovery_codes_replace_all(
        &self,
        user_id: &str,
        codes: &[RecoveryCode],
    ) -> Result<(), StoreError> {
        let user_id = user_id.to_owned();
        let codes = codes.to_vec();
        self.call(move |s| s.recovery_codes_replace_all(&user_id, &codes))
    }

    fn recovery_codes_find_unused(
        &self,
        user_id: &str,
        code_hash: &str,
    ) -> Result<Option<RecoveryCode>, StoreError> {
        let user_id = user_id.to_owned();
        let code_hash = code_hash.to_owned();
        self.call(move |s| s.recovery_codes_find_unused(&user_id, &code_hash))
    }

    fn recovery_codes_mark_used(&self, code_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let code_id = code_id.to_owned();
        self.call(move |s| s.recovery_codes_mark_used(&code_id, at))
    }

    fn recovery_codes_count_unused(&self, user_id: &str) -> Result<u64, StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.recovery_codes_count_unused(&user_id))
    }

    fn pat_create(&self, pat: &PersonalAccessToken) -> Result<(), StoreError> {
        let pat = pat.clone();
        self.call(move |s| s.pat_create(&pat))
    }

    fn pat_find_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PersonalAccessToken>, StoreError> {
        let token_hash = token_hash.to_owned();
        self.call(move |s| s.pat_find_by_hash(&token_hash))
    }

    fn pat_list(&self, user_id: &str) -> Result<Vec<PersonalAccessToken>, StoreError> {
        let user_id = user_id.to_owned();
        self.call(move |s| s.pat_list(&user_id))
    }

    fn pat_revoke(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let token_id = token_id.to_owned();
        self.call(move |s| s.pat_revoke(&token_id, at))
    }

    fn pat_touch_last_used(&self, token_id: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let token_id = token_id.to_owned();
        self.call(move |s| s.pat_touch_last_used(&token_id, at))
    }

    fn oidc_link(&self, identity: &OidcIdentity) -> Result<(), StoreError> {
        let identity = identity.clone();
        self.call(move |s| s.oidc_link(&identity))
    }

    fn oidc_get_by_subject(
        &self,
        provider: &str,
        subject: &str,
    ) -> Result<Option<OidcIdentity>, StoreError> {
        let provider = provider.to_owned();
        let subject = subject.to_owned();
        self.call(move |s| s.oidc_get_by_subject(&provider, &subject))
    }

    fn oidc_touch_last_login(
        &self,
        provider: &str,
        subject: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let provider = provider.to_owned();
        let subject = subject.to_owned();
        self.call(move |s| s.oidc_touch_last_login(&provider, &subject, at))
    }

    fn oidc_pending_create(&self, pending: &OidcPendingLogin) -> Result<(), StoreError> {
        let pending = pending.clone();
        self.call(move |s| s.oidc_pending_create(&pending))
    }

    fn oidc_pending_take(&self, state: &str) -> Result<Option<OidcPendingLogin>, StoreError> {
        let state = state.to_owned();
        self.call(move |s| s.oidc_pending_take(&state))
    }

    fn oidc_pending_purge_expired(&self, now: DateTime<Utc>) -> Result<u64, StoreError> {
        self.call(move |s| s.oidc_pending_purge_expired(now))
    }

    fn audit_log(&self, event: &AuditEvent) -> Result<(), StoreError> {
        let event = event.clone();
        self.call(move |s| s.audit_log(&event))
    }

    fn audit_list(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>, StoreError> {
        let filter = filter.clone();
        self.call(move |s| s.audit_list(&filter))
    }
}

// ─── JobDefinitionStore ───

impl JobDefinitionStore for PgStoreHandle {
    fn create_job_definition(&self, job: &JobDefinition) -> Result<(), StoreError> {
        let job = job.clone();
        self.call(move |s| s.create_job_definition(&job))
    }

    fn get_job_definition(&self, job_key: &str) -> Result<Option<JobDefinition>, StoreError> {
        let job_key = job_key.to_owned();
        self.call(move |s| s.get_job_definition(&job_key))
    }

    fn list_job_definitions(&self) -> Result<Vec<JobDefinition>, StoreError> {
        self.call(|s| s.list_job_definitions())
    }

    fn delete_job_definition(&self, job_key: &str) -> Result<(), StoreError> {
        let job_key = job_key.to_owned();
        self.call(move |s| s.delete_job_definition(&job_key))
    }
}

// ─── TriggerDefinitionStore ───

impl TriggerDefinitionStore for PgStoreHandle {
    fn create_trigger(&self, trigger: &TriggerDefinition) -> Result<(), StoreError> {
        let trigger = trigger.clone();
        self.call(move |s| s.create_trigger(&trigger))
    }

    fn get_trigger(&self, trigger_id: &str) -> Result<Option<TriggerDefinition>, StoreError> {
        let trigger_id = trigger_id.to_owned();
        self.call(move |s| s.get_trigger(&trigger_id))
    }

    fn list_triggers(&self, job_key: Option<&str>) -> Result<Vec<TriggerDefinition>, StoreError> {
        let job_key = job_key.map(str::to_owned);
        self.call(move |s| s.list_triggers(job_key.as_deref()))
    }

    fn delete_trigger(&self, trigger_id: &str) -> Result<(), StoreError> {
        let trigger_id = trigger_id.to_owned();
        self.call(move |s| s.delete_trigger(&trigger_id))
    }

    fn update_trigger(&self, trigger: &TriggerDefinition) -> Result<bool, StoreError> {
        let trigger = trigger.clone();
        self.call(move |s| s.update_trigger(&trigger))
    }
}

// ─── CalendarDefinitionStore ───

impl CalendarDefinitionStore for PgStoreHandle {
    fn create_calendar(&self, cal: &CalendarDefinition) -> Result<(), StoreError> {
        let cal = cal.clone();
        self.call(move |s| s.create_calendar(&cal))
    }

    fn get_calendar(&self, calendar_id: &str) -> Result<Option<CalendarDefinition>, StoreError> {
        let calendar_id = calendar_id.to_owned();
        self.call(move |s| s.get_calendar(&calendar_id))
    }

    fn list_calendars(&self) -> Result<Vec<CalendarDefinition>, StoreError> {
        self.call(|s| s.list_calendars())
    }

    fn delete_calendar(&self, calendar_id: &str) -> Result<(), StoreError> {
        let calendar_id = calendar_id.to_owned();
        self.call(move |s| s.delete_calendar(&calendar_id))
    }
}

// ─── DslAdoptionStore ───

impl DslAdoptionStore for PgStoreHandle {
    fn insert_adoption(&self, adoption: &DslAdoption) -> Result<(), StoreError> {
        let adoption = adoption.clone();
        self.call(move |s| s.insert_adoption(&adoption))
    }

    fn delete_adoption(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError> {
        let resource_type = resource_type.to_owned();
        let resource_key = resource_key.to_owned();
        self.call(move |s| s.delete_adoption(&resource_type, &resource_key))
    }

    fn is_adopted(&self, resource_type: &str, resource_key: &str) -> Result<bool, StoreError> {
        let resource_type = resource_type.to_owned();
        let resource_key = resource_key.to_owned();
        self.call(move |s| s.is_adopted(&resource_type, &resource_key))
    }

    fn list_adoptions(&self, resource_type: &str) -> Result<Vec<DslAdoption>, StoreError> {
        let resource_type = resource_type.to_owned();
        self.call(move |s| s.list_adoptions(&resource_type))
    }
}

// ─── AlertStore ───

impl AlertStore for PgStoreHandle {
    fn record_alert_delivery(&self, delivery: &AlertDelivery) -> Result<(), StoreError> {
        let delivery = delivery.clone();
        self.call(move |s| s.record_alert_delivery(&delivery))
    }

    fn list_alert_deliveries(
        &self,
        filter: &AlertDeliveryFilter,
    ) -> Result<Vec<AlertDelivery>, StoreError> {
        let filter = filter.clone();
        self.call(move |s| s.list_alert_deliveries(&filter))
    }

    fn get_alert_delivery(&self, delivery_id: &str) -> Result<Option<AlertDelivery>, StoreError> {
        let delivery_id = delivery_id.to_owned();
        self.call(move |s| s.get_alert_delivery(&delivery_id))
    }

    fn last_alert_fire_at(
        &self,
        rule_name: &str,
        job_key: &str,
    ) -> Result<Option<DateTime<Utc>>, StoreError> {
        let rule_name = rule_name.to_owned();
        let job_key = job_key.to_owned();
        self.call(move |s| s.last_alert_fire_at(&rule_name, &job_key))
    }

    fn upsert_alert_rule_override(&self, ov: &AlertRuleOverride) -> Result<(), StoreError> {
        let ov = ov.clone();
        self.call(move |s| s.upsert_alert_rule_override(&ov))
    }

    fn get_alert_rule_override(
        &self,
        rule_name: &str,
    ) -> Result<Option<AlertRuleOverride>, StoreError> {
        let rule_name = rule_name.to_owned();
        self.call(move |s| s.get_alert_rule_override(&rule_name))
    }

    fn list_alert_rule_overrides(&self) -> Result<Vec<AlertRuleOverride>, StoreError> {
        self.call(|s| s.list_alert_rule_overrides())
    }

    fn delete_alert_rule_override(&self, rule_name: &str) -> Result<bool, StoreError> {
        let rule_name = rule_name.to_owned();
        self.call(move |s| s.delete_alert_rule_override(&rule_name))
    }

    fn delete_expired_alert_rule_overrides(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<String>, StoreError> {
        self.call(move |s| s.delete_expired_alert_rule_overrides(now))
    }

    fn prune_alert_rule_overrides(
        &self,
        valid_rule_names: &[String],
    ) -> Result<Vec<String>, StoreError> {
        let valid_rule_names = valid_rule_names.to_vec();
        self.call(move |s| s.prune_alert_rule_overrides(&valid_rule_names))
    }
}

// ─── ExecutionLogStore ───

impl ExecutionLogStore for PgStoreHandle {
    fn append_log(&self, entry: &ExecutionLogEntry) -> Result<(), StoreError> {
        let entry = entry.clone();
        self.call(move |s| s.append_log(&entry))
    }

    fn append_logs_batch(&self, entries: &[ExecutionLogEntry]) -> Result<(), StoreError> {
        let entries = entries.to_vec();
        self.call(move |s| s.append_logs_batch(&entries))
    }

    fn read_logs(
        &self,
        execution_id: Uuid,
        limit: u32,
    ) -> Result<Vec<ExecutionLogEntry>, StoreError> {
        self.call(move |s| s.read_logs(execution_id, limit))
    }
}

// The blanket marker: PgStoreHandle satisfies every sub-trait above.
impl MaintenanceStore for PgStoreHandle {
    fn get_maintenance(&self) -> Result<MaintenanceState, StoreError> {
        self.call(|s| s.get_maintenance())
    }

    fn set_maintenance(&self, state: &MaintenanceState) -> Result<(), StoreError> {
        let state = state.clone();
        self.call(move |s| s.set_maintenance(&state))
    }
}

impl Store for PgStoreHandle {}
