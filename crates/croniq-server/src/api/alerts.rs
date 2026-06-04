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
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_config::compile::AlertsConfig;
use croniq_store::models::{
    AlertDelivery, AlertDeliveryFilter, AlertDeliveryState, AlertRuleOverride,
};
use serde::{Deserialize, Serialize};

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;

/// Effective alerts config plus any operational overrides (issue #231).
/// `#[serde(flatten)]` keeps the original `{channels, rules}` shape so
/// existing `alerts:read` consumers are unaffected; `overrides` is
/// additive. Surfacing overrides here means the UI / `doctor` get the
/// merged picture without a separate query.
#[derive(Serialize)]
pub struct AlertsConfigView {
    #[serde(flatten)]
    pub config: AlertsConfig,
    /// Active + recently-set overrides, newest-set first. May include an
    /// expired-but-not-yet-swept row; clients should treat `expires_at`
    /// in the past as inert.
    pub overrides: Vec<AlertRuleOverride>,
}

/// `GET /v1/alerts/config` — read the effective alerts config that
/// the evaluator + watchdog are using, with operational overrides
/// surfaced inline. The `ChannelKind::Webhook` `signing_key` field is
/// `#[serde(skip_serializing)]` so the HMAC secret never leaves the
/// server via this endpoint (verified by a dedicated test).
pub async fn handle_get_config(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<AlertsConfigView>, StatusCode> {
    require_scope(&ctx, Scope::ALERTS_READ)?;
    // Overrides are best-effort: a store error (or no store) degrades to
    // an empty list rather than failing the whole config read.
    let overrides = state
        .store
        .as_ref()
        .and_then(|s| s.list_alert_rule_overrides().ok())
        .unwrap_or_default();
    Ok(Json(AlertsConfigView {
        config: state.alerts.clone(),
        overrides,
    }))
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

// ─── Operational overrides (issue #231, Phase 1) ───
//
// `alerts:write`-gated, admin-only. Each set-action overwrites the rule's
// override row wholesale (snooze | disable | throttle are distinct
// intents, not composable). `note` is mandatory — incident context is
// captured at write time, not retrofitted via git blame.

/// `POST /v1/alerts/rules/{name}/snooze` body. `until` is the instant the
/// rule resumes; it doubles as the auto-clear deadline so the override
/// evaporates without operator follow-up.
#[derive(Deserialize)]
pub struct SnoozeRequest {
    pub until: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

/// `POST /v1/alerts/rules/{name}/disable` body. `expires_at` optionally
/// auto-re-enables; omit it for an open-ended disable.
#[derive(Deserialize)]
pub struct DisableRequest {
    pub note: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// `POST /v1/alerts/rules/{name}/throttle` body. `throttle` is a duration
/// string (`"30m"`, `"1h"`) that replaces the DSL throttle window.
#[derive(Deserialize)]
pub struct ThrottleRequest {
    pub throttle: Option<String>,
    pub note: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Validate that `note` is present and non-blank, returning the trimmed
/// value or `400`. Mandatory on every set-action.
fn require_note(note: &Option<String>) -> Result<String, StatusCode> {
    match note {
        Some(n) if !n.trim().is_empty() => Ok(n.trim().to_string()),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

/// Shared preamble for the set-actions: enforce scope, ensure the store
/// is up, and confirm the named rule actually exists in the DSL config
/// (overriding a phantom rule is a `404`).
fn override_preamble<'a>(
    state: &'a Arc<ServerState>,
    ctx: &CallerContext,
    rule_name: &str,
) -> Result<&'a crate::store::DynStore, StatusCode> {
    require_scope(ctx, Scope::ALERTS_WRITE)?;
    if !state.alerts.rules.iter().any(|r| r.name == rule_name) {
        return Err(StatusCode::NOT_FOUND);
    }
    state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)
}

fn caller_id(ctx: &CallerContext) -> String {
    ctx.user_id.clone().unwrap_or_else(|| ctx.caller_id.clone())
}

/// Persist `ov`, emit an `alerts.override.set` audit event, and return it.
fn commit_override(
    store: &crate::store::DynStore,
    ctx: &CallerContext,
    ov: AlertRuleOverride,
) -> Result<Json<AlertRuleOverride>, StatusCode> {
    store
        .upsert_alert_rule_override(&ov)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let diff = serde_json::to_string(&ov).ok();
    audit::record(
        store,
        ctx,
        "alerts.override.set",
        "alert_rule",
        Some(&ov.rule_name),
        diff.as_deref(),
    );
    Ok(Json(ov))
}

/// `POST /v1/alerts/rules/{name}/snooze`
pub async fn handle_snooze_rule(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(rule_name): Path<String>,
    Json(req): Json<SnoozeRequest>,
) -> Result<Json<AlertRuleOverride>, StatusCode> {
    let store = override_preamble(&state, &ctx, &rule_name)?;
    let note = require_note(&req.note)?;
    let until = req.until.ok_or(StatusCode::BAD_REQUEST)?;
    let ov = AlertRuleOverride {
        rule_name: rule_name.clone(),
        enabled: None,
        snooze_until: Some(until),
        throttle_secs: None,
        note,
        set_by_user_id: caller_id(&ctx),
        set_at: Utc::now(),
        // A snooze auto-clears when it ends — no separate deadline needed.
        expires_at: Some(until),
    };
    commit_override(store, &ctx, ov)
}

/// `POST /v1/alerts/rules/{name}/disable`
pub async fn handle_disable_rule(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(rule_name): Path<String>,
    Json(req): Json<DisableRequest>,
) -> Result<Json<AlertRuleOverride>, StatusCode> {
    let store = override_preamble(&state, &ctx, &rule_name)?;
    let note = require_note(&req.note)?;
    let ov = AlertRuleOverride {
        rule_name: rule_name.clone(),
        enabled: Some(false),
        snooze_until: None,
        throttle_secs: None,
        note,
        set_by_user_id: caller_id(&ctx),
        set_at: Utc::now(),
        expires_at: req.expires_at,
    };
    commit_override(store, &ctx, ov)
}

/// `POST /v1/alerts/rules/{name}/throttle`
pub async fn handle_throttle_rule(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(rule_name): Path<String>,
    Json(req): Json<ThrottleRequest>,
) -> Result<Json<AlertRuleOverride>, StatusCode> {
    let store = override_preamble(&state, &ctx, &rule_name)?;
    let note = require_note(&req.note)?;
    let throttle = req.throttle.as_deref().ok_or(StatusCode::BAD_REQUEST)?;
    let secs = crate::alerts::parse_throttle_secs(throttle).ok_or(StatusCode::BAD_REQUEST)?;
    let ov = AlertRuleOverride {
        rule_name: rule_name.clone(),
        enabled: None,
        snooze_until: None,
        throttle_secs: Some(secs),
        note,
        set_by_user_id: caller_id(&ctx),
        set_at: Utc::now(),
        expires_at: req.expires_at,
    };
    commit_override(store, &ctx, ov)
}

/// `GET /v1/alerts/rules/{name}/override` — inspect the current override.
/// `404` when none is set (or the rule is unknown).
pub async fn handle_get_override(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(rule_name): Path<String>,
) -> Result<Json<AlertRuleOverride>, StatusCode> {
    require_scope(&ctx, Scope::ALERTS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .get_alert_rule_override(&rule_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `DELETE /v1/alerts/rules/{name}/override` — clear the override, back to
/// pure DSL behaviour. `404` when there was nothing to clear.
pub async fn handle_clear_override(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(rule_name): Path<String>,
) -> Result<StatusCode, StatusCode> {
    require_scope(&ctx, Scope::ALERTS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let removed = store
        .delete_alert_rule_override(&rule_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !removed {
        return Err(StatusCode::NOT_FOUND);
    }
    audit::record(
        store,
        &ctx,
        "alerts.override.cleared",
        "alert_rule",
        Some(&rule_name),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}
