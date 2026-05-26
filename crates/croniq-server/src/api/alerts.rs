//! Read-only HTTP endpoints over the failure-alert configuration
//! and delivery log (issue #140 PR-5).
//!
//! Routes registered in [`super::server_router`]:
//!
//! ```text
//!   GET  /v1/alerts/config              effective AlertsConfig (no secrets)
//!   GET  /v1/alerts/deliveries          filtered list of `alert_deliveries`
//!   GET  /v1/alerts/deliveries/{id}     single delivery detail
//! ```
//!
//! All gated by `alerts:read` (Viewer role and above get it by
//! default; see `croniq_auth::context::default_scopes_for_role`).
//!
//! Rules and channels are DSL-managed today — there is no
//! `alerts:write` scope or POST/PUT/DELETE endpoint yet. The
//! delivery log is append-only and pruned by retention (future work);
//! these endpoints are the read view the operator UI consumes.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_config::compile::AlertsConfig;
use croniq_store::models::{AlertDelivery, AlertDeliveryFilter, AlertDeliveryState};
use serde::Deserialize;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

/// `GET /v1/alerts/config` — read the effective alerts config that
/// the evaluator + watchdog are using. The `ChannelKind::Webhook`
/// `signing_key` field is `#[serde(skip_serializing)]` so the HMAC
/// secret never leaves the server via this endpoint (verified by a
/// dedicated test).
pub async fn handle_get_config(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<AlertsConfig>, StatusCode> {
    require_scope(&ctx, Scope::ALERTS_READ)?;
    Ok(Json(state.alerts.clone()))
}

/// Query parameters for `GET /v1/alerts/deliveries`.
#[derive(Debug, Default, Deserialize)]
pub struct ListDeliveriesQuery {
    pub job_key: Option<String>,
    pub rule_name: Option<String>,
    /// Only return deliveries in the given state. Accepted values:
    /// `delivered`, `failed`, `throttled`. Anything else is rejected
    /// with `400` rather than silently ignored — operators should
    /// know when their filter has a typo.
    pub state: Option<String>,
    pub since: Option<DateTime<Utc>>,
    /// Maximum rows to return. Capped server-side at 500 so a typo
    /// (`limit=999999`) can't pull the entire delivery log.
    pub limit: Option<u32>,
}

/// `GET /v1/alerts/deliveries`
pub async fn handle_list_deliveries(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(q): Query<ListDeliveriesQuery>,
) -> Result<Json<Vec<AlertDelivery>>, StatusCode> {
    require_scope(&ctx, Scope::ALERTS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Validate the `state` filter strictly — a typo would otherwise
    // silently match nothing.
    if let Some(s) = q.state.as_deref()
        && AlertDeliveryState::parse_db(s).is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut filter = AlertDeliveryFilter {
        job_key: q.job_key,
        rule_name: q.rule_name,
        since: q.since,
        limit: Some(q.limit.unwrap_or(100).min(500)),
    };
    let mut deliveries = store
        .list_alert_deliveries(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // The store-level filter doesn't take a state argument yet —
    // filtering in-handler keeps the SQL surface narrow and we
    // already cap the row count above. If this turns out to be a
    // hot path, pushing the predicate down is a one-line change.
    if let Some(want) = q.state.as_deref().and_then(AlertDeliveryState::parse_db) {
        deliveries.retain(|d| d.state == want);
    }
    // Silence the "unused" warning when no state filter is set —
    // filter itself isn't reused below.
    let _ = &mut filter;

    Ok(Json(deliveries))
}

/// `GET /v1/alerts/deliveries/{id}`
pub async fn handle_get_delivery(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(delivery_id): axum::extract::Path<String>,
) -> Result<Json<AlertDelivery>, StatusCode> {
    require_scope(&ctx, Scope::ALERTS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    store
        .get_alert_delivery(&delivery_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
