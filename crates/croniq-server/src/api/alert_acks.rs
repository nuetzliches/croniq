//! Per-job alert acknowledgements: "we know this job is failing, and we are
//! waiting it out".
//!
//! ```text
//!   GET    /v1/alerts/acks                every acknowledgement     alerts:read
//!   GET    /v1/jobs/{job_key}/alert-ack   one job's                 alerts:read
//!   POST   /v1/jobs/{job_key}/alert-ack   acknowledge a failing job alerts:ack
//!   DELETE /v1/jobs/{job_key}/alert-ack   end it by hand            alerts:ack
//! ```
//!
//! What a rule snooze cannot do: it holds back one job's alerts, not every
//! job a rule matches, and it needs no one to remember to lift it. The job's
//! first successful run ends it and sends a recovery (see
//! [`crate::completion`]); its deadline, which is always set and capped,
//! ends it with an alert that the job is still failing (see
//! [`crate::watchdog`]).

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Duration, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{ExecutionFilter, ExecutionState, JobAlertAck};
use serde::{Deserialize, Serialize};

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;
use crate::api::notes::{author_name, job_exists};
use crate::store::DynStore;

/// How long an acknowledgement lasts when the caller names no end: a weekend.
pub const DEFAULT_ACK_HOURS: i64 = 72;
/// The longest an acknowledgement may last. An outage that outlives it gets
/// acknowledged again, with a fresh note — that is the reminder.
pub const MAX_ACK_DAYS: i64 = 7;
/// The longest note, matching an operator note.
const MAX_NOTE_CHARS: usize = 2000;

/// `POST /v1/jobs/{job_key}/alert-ack` body.
#[derive(Debug, Deserialize)]
pub struct AckRequest {
    /// Why. Required.
    pub note: Option<String>,
    /// When the acknowledgement runs out if the job has not recovered by
    /// then. Defaults to [`DEFAULT_ACK_HOURS`] from now; at most
    /// [`MAX_ACK_DAYS`].
    pub until: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct AckErrorBody {
    error: &'static str,
    message: String,
}

/// A refusal the dashboard can show as it is.
pub enum AckError {
    Status(StatusCode),
    Refused {
        status: StatusCode,
        error: &'static str,
        message: String,
    },
}

impl From<StatusCode> for AckError {
    fn from(status: StatusCode) -> Self {
        Self::Status(status)
    }
}

impl IntoResponse for AckError {
    fn into_response(self) -> Response {
        match self {
            Self::Status(status) => status.into_response(),
            Self::Refused {
                status,
                error,
                message,
            } => (status, Json(AckErrorBody { error, message })).into_response(),
        }
    }
}

fn invalid(message: impl Into<String>) -> AckError {
    AckError::Refused {
        status: StatusCode::BAD_REQUEST,
        error: "invalid_ack",
        message: message.into(),
    }
}

fn internal<E>(_: E) -> AckError {
    AckError::Status(StatusCode::INTERNAL_SERVER_ERROR)
}

fn store_of(state: &ServerState) -> Result<&DynStore, AckError> {
    Ok(state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?)
}

/// `GET /v1/alerts/acks` — every acknowledgement, newest first. One past its
/// deadline that the watchdog has not removed yet is included; it holds
/// nothing back.
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<JobAlertAck>>, AckError> {
    require_scope(&ctx, Scope::ALERTS_READ)?;
    let store = store_of(&state)?;
    Ok(Json(store.list_job_alert_acks().map_err(internal)?))
}

/// `GET /v1/jobs/{job_key}/alert-ack` — `404` when the job has none.
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(job_key): Path<String>,
) -> Result<Json<JobAlertAck>, AckError> {
    require_scope(&ctx, Scope::ALERTS_READ)?;
    let store = store_of(&state)?;
    store
        .get_job_alert_ack(&job_key)
        .map_err(internal)?
        .map(Json)
        .ok_or(AckError::Status(StatusCode::NOT_FOUND))
}

/// `POST /v1/jobs/{job_key}/alert-ack` — acknowledge a failing job.
///
/// Refused with `409` unless the job's latest finished run failed: there is
/// nothing to acknowledge on a job that works, and a recovery for it would
/// announce the end of a failure nobody saw. Acknowledging a job that already
/// has an acknowledgement replaces it — new note, new deadline, count from
/// zero.
pub async fn handle_ack(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(job_key): Path<String>,
    Json(req): Json<AckRequest>,
) -> Result<Json<JobAlertAck>, AckError> {
    require_scope(&ctx, Scope::ALERTS_ACK)?;
    let store = store_of(&state)?;

    let note = req.note.as_deref().map(str::trim).unwrap_or_default();
    if note.is_empty() {
        return Err(invalid("say why the job is acknowledged"));
    }
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(invalid(format!(
            "a note is at most {MAX_NOTE_CHARS} characters"
        )));
    }

    let now = Utc::now();
    let expires_at = req
        .until
        .unwrap_or(now + Duration::hours(DEFAULT_ACK_HOURS));
    if expires_at <= now {
        return Err(invalid("until must be in the future"));
    }
    if expires_at > now + Duration::days(MAX_ACK_DAYS) {
        return Err(invalid(format!(
            "an acknowledgement lasts at most {MAX_ACK_DAYS} days; acknowledge again if the \
             outage outlasts it"
        )));
    }

    if !job_exists(&state, store, &job_key)
        .await
        .map_err(internal)?
    {
        return Err(StatusCode::NOT_FOUND.into());
    }
    let latest = store
        .list_executions(&ExecutionFilter {
            job_key: Some(job_key.clone()),
            states: vec![
                ExecutionState::Completed,
                ExecutionState::Failed,
                ExecutionState::Dead,
            ],
            limit: Some(1),
            ..Default::default()
        })
        .map_err(internal)?;
    let failing = latest
        .first()
        .is_some_and(|e| matches!(e.state, ExecutionState::Failed | ExecutionState::Dead));
    if !failing {
        return Err(AckError::Refused {
            status: StatusCode::CONFLICT,
            error: "job_not_failing",
            message: format!(
                "the latest finished run of '{job_key}' did not fail, so there is nothing to \
                 acknowledge"
            ),
        });
    }

    let ack = JobAlertAck {
        job_key: job_key.clone(),
        note: note.to_string(),
        acked_by: ctx.user_id.clone().unwrap_or_else(|| ctx.caller_id.clone()),
        acked_by_name: author_name(store, &ctx),
        acked_at: now,
        expires_at,
        suppressed_count: 0,
        last_execution_id: None,
        last_error: None,
        last_suppressed_at: None,
    };
    store.upsert_job_alert_ack(&ack).map_err(internal)?;
    let diff = serde_json::json!({ "note": ack.note, "expires_at": ack.expires_at }).to_string();
    audit::record(
        store,
        &ctx,
        "alerts.ack.set",
        "job",
        Some(&job_key),
        Some(&diff),
    );
    Ok(Json(ack))
}

/// `DELETE /v1/jobs/{job_key}/alert-ack` — end the acknowledgement now. No
/// recovery is sent: the job has not recovered, someone just wants its
/// alerts back. `404` when there was none.
pub async fn handle_clear(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(job_key): Path<String>,
) -> Result<StatusCode, AckError> {
    require_scope(&ctx, Scope::ALERTS_ACK)?;
    let store = store_of(&state)?;
    if store
        .take_job_alert_ack(&job_key)
        .map_err(internal)?
        .is_none()
    {
        return Err(StatusCode::NOT_FOUND.into());
    }
    audit::record(
        store,
        &ctx,
        "alerts.ack.cleared",
        "job",
        Some(&job_key),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}
