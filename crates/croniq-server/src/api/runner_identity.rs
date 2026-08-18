//! Binds a work-protocol `runner_id` to the credential that authenticated the
//! request.
//!
//! The pull-based work protocol takes the acting `runner_id` from the request
//! body (`PollRequest.runner_id`, `CompleteRequest.runner_id`, …). Scope checks
//! alone therefore only establish that *some* holder of `work:*` is calling,
//! not that the caller is the runner it claims to be. Since `runner_id`s are
//! operator-chosen names — hostnames, `demo-runner` — a second runner holding
//! its own credential could name someone else's id and interfere with that
//! runner's work: take its identity over (requeueing its in-flight
//! executions), complete its executions, renew its lease, or append log lines
//! to its executions.
//!
//! ## Semantics: first-writer-wins
//!
//! The first work request naming a given `runner_id` binds it to the caller's
//! identity (`runner_identities` table, migration 024). Later requests naming
//! that `runner_id` must come from the same identity or are refused with
//! `403`. Nothing is pre-provisioned, so no deployment has to be reconfigured
//! before upgrading — including deployments that share one runner key across
//! many runners, where every runner resolves to the same owner and every
//! binding therefore matches. `DELETE /v1/runners/{id}` releases a binding,
//! which is how an operator hands a `runner_id` to a different credential.
//!
//! The identity is the caller's `client_id`: the owning API client for API
//! keys (`user_id` is `None` there), and the user id for JWT/PAT callers. Key
//! rotation within one client keeps the same `client_id`, so it does not
//! disturb existing bindings.
//!
//! Enforcement is skipped when it could not distinguish callers or could not
//! persist a decision — when auth is unconfigured (every caller is the same
//! synthetic anonymous identity) or no store is configured — and can be turned
//! off explicitly with `pull_api { runner_identity_binding "off" }`.

use axum::http::StatusCode;
use chrono::Utc;
use croniq_auth::CallerContext;
use uuid::Uuid;

use super::{ServerState, audit};

/// The identity a work request acts under. For API-key callers this is the
/// owning client, not the individual key, so rotating a key does not orphan
/// the runner's binding.
fn caller_identity(ctx: &CallerContext) -> &str {
    &ctx.client_id
}

/// Whether identity binding applies to this request at all.
///
/// Without auth every request carries the same synthetic `anonymous` context
/// (see `auth_middleware::require_auth`), so binding would record a value that
/// distinguishes nothing — and would then mismatch every real caller once the
/// operator configures auth. Without a store there is nowhere to record the
/// binding.
fn enforced(state: &ServerState) -> bool {
    state.runner_identity_binding && state.jwt_config.is_some() && state.store.is_some()
}

/// Claim `runner_id` for the calling credential, or confirm it already owns
/// it.
///
/// Returns `403` when another credential bound the id first, and `503` when
/// the binding could not be read or written — a security fence must not fail
/// open, and a runner that retries is the correct response to a store blip.
pub fn authorize_runner(
    state: &ServerState,
    ctx: &CallerContext,
    runner_id: &str,
) -> Result<(), StatusCode> {
    if !enforced(state) {
        return Ok(());
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let caller = caller_identity(ctx);

    let owner = store
        .runner_identity_bind(runner_id, caller, Utc::now())
        .map_err(|e| {
            tracing::error!(
                runner_id = %runner_id,
                error = %e,
                "could not resolve runner identity binding — refusing the work request"
            );
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    if owner == caller {
        return Ok(());
    }

    // Deliberately does not log the owner: the point of the message is that
    // this caller is not it, and the operator can read the binding from the
    // store. `runner_id` is a public identifier (see AGENTS.md).
    tracing::warn!(
        runner_id = %runner_id,
        caller = %caller,
        "work request refused — this runner_id is bound to a different credential; \
         deregister the runner to hand the id over, or give this runner its own id"
    );
    audit::record_event(
        store,
        "system",
        None,
        "runner.identity_rejected",
        "runner",
        Some(runner_id),
    );
    Err(StatusCode::FORBIDDEN)
}

/// Resolve the `runner_id` that holds `execution_id` and authorize the caller
/// against it. This is the ownership fence for endpoints addressed by
/// execution rather than by runner (`…:events`).
///
/// An execution nobody has claimed has no owning runner to compare against, so
/// there is no basis on which to accept writes for it — refused with `403`,
/// same as a foreign claim. A missing execution is `404`.
pub fn authorize_execution(
    state: &ServerState,
    ctx: &CallerContext,
    execution_id: Uuid,
) -> Result<(), StatusCode> {
    if !enforced(state) {
        return Ok(());
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let execution = store
        .get_execution(execution_id)
        .map_err(|e| {
            tracing::error!(
                execution_id = %execution_id,
                error = %e,
                "could not load execution to check runner ownership"
            );
            StatusCode::SERVICE_UNAVAILABLE
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let Some(ref runner_id) = execution.runner_id else {
        tracing::warn!(
            execution_id = %execution_id,
            caller = %caller_identity(ctx),
            "work request refused — execution is not claimed by any runner"
        );
        return Err(StatusCode::FORBIDDEN);
    };

    authorize_runner(state, ctx, runner_id)
}

/// Drop the binding for `runner_id`, freeing the id for another credential.
/// Called when an operator deregisters a runner. Best-effort: a failure here
/// leaves the binding in place, which is the safe direction.
pub fn release_runner(state: &ServerState, runner_id: &str) {
    let Some(store) = state.store.as_ref() else {
        return;
    };
    if let Err(e) = store.runner_identity_release(runner_id) {
        tracing::warn!(
            runner_id = %runner_id,
            error = %e,
            "could not release runner identity binding"
        );
    }
}
