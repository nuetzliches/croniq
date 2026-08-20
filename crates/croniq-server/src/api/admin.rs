//! Admin endpoints: operations that reshape live server state.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use serde::{Deserialize, Serialize};

use super::ServerState;
use crate::api::auth_middleware::require_scope;
use crate::api_client_env::{self, ClientOutcome, ReconcileInputs};
use crate::reload::{self, PendingRestart, ReloadDiff, ReloadError};

#[derive(Debug, Deserialize, Default)]
pub struct ReloadQuery {
    /// Validate + compute diff without applying. Defaults to false.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct ReloadSuccess {
    pub applied: bool,
    pub dry_run: bool,
    pub diff: ReloadDiff,
    /// Boot-only settings the file changed but this reload cannot apply, so a
    /// caller can report "applied, with N settings pending restart" instead of a
    /// plain success (issue #406). Omitted when empty, and when the server has
    /// no boot snapshot to compare against.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pending_restart: Vec<PendingRestart>,
    /// What the reload did to the API clients the environment declares
    /// (issue #471), one entry per declaration. Reported rather than
    /// log-only because this endpoint is how a deployment with no dashboard
    /// rotates a credential — it has to be able to see the result.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<ClientOutcome>,
    /// Why the credential reconcile could not run, when it could not.
    /// A bad declaration fails the credential half without taking the
    /// Croniqfile reload with it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials_error: Option<String>,
}

/// Reconcile the environment-declared API clients as part of this reload.
///
/// Never fails the request: the Croniqfile half of a reload is independent,
/// and an operator fixing a typo in a scope list should not also lose the
/// ability to reload their schedule.
fn reconcile_credentials(
    state: &ServerState,
    dry_run: bool,
) -> (Vec<ClientOutcome>, Option<String>) {
    let Some(store) = state.store.as_ref() else {
        return (Vec::new(), None);
    };
    let inputs = match if dry_run {
        ReconcileInputs::dry_run_from_env()
    } else {
        ReconcileInputs::from_env()
    } {
        Ok(i) => i,
        Err(e) => {
            tracing::error!(error = %e, "environment-declared API clients are invalid");
            return (Vec::new(), Some(e));
        }
    };
    match api_client_env::reconcile(&**store, &inputs) {
        Ok(outcomes) => (outcomes, None),
        Err(e) => {
            tracing::error!(error = %e, "API client reconcile failed");
            (Vec::new(), Some(e.to_string()))
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReloadFailure {
    pub error: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

/// `POST /v1/admin/reload-config[?dry_run=true]`
///
/// Re-read the Croniqfile, merge API-registered triggers (DSL precedence),
/// and either just return a diff (`dry_run=true`, 200) or apply and return
/// the applied diff (200). Validation failures return 422 with line/column
/// when available.
///
/// Also re-reads the API clients the environment declares (issue #471). This
/// is the endpoint that makes a `<VAR>_FILE`-backed key rotatable without
/// restarting: the direct environment of a running process cannot change, but
/// the file it points at can. Like SIGHUP and unlike the file watcher, this is
/// an explicit operator request, which is why it carries the credential half.
pub async fn handle_reload_config(
    State(state): State<Arc<ServerState>>,
    axum::Extension(ctx): axum::Extension<CallerContext>,
    Query(query): Query<ReloadQuery>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    require_scope(&ctx, Scope::ADMIN)?;

    let Some(config_path) = state.config_path.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(store) = state.store.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(scheduler_tx) = state.scheduler_tx.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(triggers) = state.triggers.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(dsl_jobs) = state.dsl_jobs.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(dsl_calendars) = state.dsl_calendars.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    // Stage 1: build + validate the plan. No state changed yet.
    let plan = match reload::build_plan(&config_path, &store, &triggers, &dsl_jobs).await {
        Ok(p) => p,
        Err(e) => {
            return Ok(reload_error_response(e, &state.reload_counters));
        }
    };

    // Boot-only settings the file changed since startup. Reported on both the
    // dry-run and the applied response, and logged on apply, because a
    // successful reload otherwise reads as a full apply (issue #406).
    let pending_restart = state
        .boot_only_settings
        .as_ref()
        .map(|running| running.changes_vs(&plan.boot_only))
        .unwrap_or_default();

    // Dry-run stops here: return the diff without applying.
    if query.dry_run {
        let (credentials, credentials_error) = reconcile_credentials(&state, true);
        let body = ReloadSuccess {
            applied: false,
            dry_run: true,
            diff: plan.diff,
            pending_restart,
            credentials,
            credentials_error,
        };
        return Ok((
            StatusCode::OK,
            Json(serde_json::to_value(body).unwrap_or_default()),
        ));
    }

    reload::log_pending_restart(&pending_restart);

    // Stage 2: apply via the scheduler command channel + await ack.
    let diff = plan.diff.clone();
    match reload::apply_plan(
        plan,
        &scheduler_tx,
        &dsl_jobs,
        &dsl_calendars,
        &state.policy_dsl_adopt_on_mutate,
        &state.policy_strict_calendars,
        &triggers,
        &state.config_faults,
    )
    .await
    {
        Ok(()) => {
            state.reload_counters.inc_success();
            // After the Croniqfile half succeeded: a credential rotation is
            // the more disruptive of the two, so it does not run until the
            // config it accompanies is known good.
            let (credentials, credentials_error) = reconcile_credentials(&state, false);
            let body = ReloadSuccess {
                applied: true,
                dry_run: false,
                diff,
                pending_restart,
                credentials,
                credentials_error,
            };
            Ok((
                StatusCode::OK,
                Json(serde_json::to_value(body).unwrap_or_default()),
            ))
        }
        Err(e) => {
            state.reload_counters.inc_apply_error();
            tracing::error!(error = %e, "reload apply failed after successful validation");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn reload_error_response(
    err: ReloadError,
    counters: &crate::reload::ReloadCounters,
) -> (StatusCode, Json<serde_json::Value>) {
    let (status, body) = match err {
        ReloadError::ReadFile { path, source } => {
            counters.inc_validation_error();
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                ReloadFailure {
                    error: "read_error",
                    message: format!("failed to read {}: {source}", path.display()),
                    line: None,
                    column: None,
                },
            )
        }
        ReloadError::Validation {
            message,
            line,
            column,
        } => {
            counters.inc_validation_error();
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                ReloadFailure {
                    error: "validation_error",
                    message,
                    line,
                    column,
                },
            )
        }
        ReloadError::Store(msg) => {
            counters.inc_apply_error();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ReloadFailure {
                    error: "store_error",
                    message: msg,
                    line: None,
                    column: None,
                },
            )
        }
    };
    (status, Json(serde_json::to_value(body).unwrap_or_default()))
}
