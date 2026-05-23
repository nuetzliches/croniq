//! Audit-log read endpoint — `GET /v1/audit`.
//!
//! Append-only writes happen inline from the handlers that perform the
//! audited action (login_success, user.created, job.deleted, …). This
//! file is only the read-side.
//!
//! Scope: `users:admin` or `admin` wildcard.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{AuditEvent, AuditFilter};
use serde::{Deserialize, Serialize};

use super::ServerState;

#[derive(Deserialize)]
pub struct ListParams {
    #[serde(default)]
    pub actor_type: Option<String>,
    #[serde(default)]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub target_type: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Serialize)]
pub struct AuditEventView {
    pub event_id: String,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub diff_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<AuditEvent> for AuditEventView {
    fn from(e: AuditEvent) -> Self {
        // Strip IP / User-Agent from the public view — the admin sees
        // them on the per-event detail (PR-B1b), not in the bulk list.
        // For PR-B1 we skip the detail endpoint entirely.
        AuditEventView {
            event_id: e.event_id,
            actor_type: e.actor_type,
            actor_id: e.actor_id,
            action: e.action,
            target_type: e.target_type,
            target_id: e.target_id,
            diff_json: e.diff_json,
            created_at: e.created_at,
        }
    }
}

/// `GET /v1/audit`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<AuditEventView>>, StatusCode> {
    if !ctx.has_any_scope(&[Scope::ADMIN, Scope::USERS_ADMIN]) {
        return Err(StatusCode::FORBIDDEN);
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let filter = AuditFilter {
        actor_type: params.actor_type,
        actor_id: params.actor_id,
        action: params.action,
        target_type: params.target_type,
        target_id: params.target_id,
        since: params.since,
        until: params.until,
        limit: params.limit,
    };
    let events = store
        .audit_list(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(events.into_iter().map(AuditEventView::from).collect()))
}

// ─── Convenience helpers used by other API modules to record events ──────────

/// Best-effort audit-log write. Fire-and-forget — every caller already
/// returns a response, and the audit log being unavailable should never
/// fail the original mutation.
pub fn record(
    store: &crate::store::DynStore,
    ctx: &CallerContext,
    action: &str,
    target_type: &str,
    target_id: Option<&str>,
    diff_json: Option<&str>,
) {
    let event = AuditEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        actor_type: actor_type_str(ctx).into(),
        actor_id: ctx.user_id.clone().or_else(|| Some(ctx.caller_id.clone())),
        action: action.into(),
        target_type: target_type.into(),
        target_id: target_id.map(|s| s.into()),
        diff_json: diff_json.map(|s| s.into()),
        ip_address: None,
        user_agent: None,
        created_at: Utc::now(),
    };
    if let Err(e) = store.audit_log(&event) {
        tracing::warn!(target: "croniq::audit", error = ?e, action = %action, "audit_log write failed");
    }
}

fn actor_type_str(ctx: &CallerContext) -> &'static str {
    use croniq_auth::AuthMethod;
    match ctx.auth_method {
        AuthMethod::Password => "user",
        AuthMethod::ApiKey => "api_key",
        AuthMethod::Pat => "pat",
        AuthMethod::Oidc => "oidc",
    }
}
