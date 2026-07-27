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
        let body = ReloadSuccess {
            applied: false,
            dry_run: true,
            diff: plan.diff,
            pending_restart,
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
            let body = ReloadSuccess {
                applied: true,
                dry_run: false,
                diff,
                pending_restart,
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
