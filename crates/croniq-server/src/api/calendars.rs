//! Calendars CRUD endpoints.
//!
//! DSL-defined calendars (from the Croniqfile) are synthesized at read time
//! with `managed_by="dsl"` and a synthetic ID `dsl:{name}` so the UI can
//! reference them in schedule editors. Mutations on DSL-managed calendars
//! return 409 Conflict by default — the Croniqfile is the source of truth.
//!
//! When the Croniqfile sets `policy { dsl_adopt_on_mutate true }`, the
//! explicit `POST /v1/calendars/dsl:{name}/adopt` endpoint copies the DSL
//! definition into the API store with `managed_by="api"` and adds a row to
//! `dsl_adoptions` so subsequent reloads skip the DSL key. `unadopt`
//! reverses the effect.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_config::compile::CalendarConfig;
use croniq_config::format::format_calendar_rule_parts;
use croniq_config::parser::Parser;
use croniq_store::models::{CalendarDefinition, DslAdoption};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

/// Synthetic-ID prefix used for DSL calendars in API responses.
pub const DSL_ID_PREFIX: &str = "dsl:";

/// Stable synthetic calendar ID derived from the DSL name. Mirrors
/// [`crate::loader::dsl_trigger_id`] for triggers — references round-trip
/// through `GET /v1/calendars` and `GET /v1/calendars/{id}`.
pub fn dsl_calendar_id(name: &str) -> String {
    format!("{DSL_ID_PREFIX}{name}")
}

/// Returns true if the given ID refers to a synthesized DSL calendar.
fn is_dsl_id(id: &str) -> bool {
    id.starts_with(DSL_ID_PREFIX)
}

/// Format a DSL `CalendarConfig` into the rule-text format that the
/// Croniqfile parser accepts (one directive per line). Each line is
/// emitted via the canonical formatter shared with `croniq fmt` and the
/// WASM form builder (weekly collapses to `weekday`/`Mon..Fri`, window
/// endpoints use the quoted `".."` range syntax), so API responses show
/// the same shape the CLI would write. Used by the synthesizer below,
/// and re-used in Phase 2 for adoption.
pub fn dsl_calendar_rules_text(cfg: &CalendarConfig) -> String {
    cfg.rules
        .iter()
        .map(|r| format_calendar_rule_parts(&r.kind, &r.rule_type, &r.args))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Synthesize a `CalendarDefinition` from a DSL-loaded `CalendarConfig` so
/// DSL calendars appear alongside stored API-managed ones in list responses.
/// Mirrors [`crate::loader::synth_trigger_def_from_dsl`].
pub fn synth_calendar_def_from_dsl(cfg: &CalendarConfig, now: DateTime<Utc>) -> CalendarDefinition {
    CalendarDefinition {
        calendar_id: dsl_calendar_id(&cfg.name),
        name: cfg.name.clone(),
        timezone: cfg.timezone.clone(),
        rules: dsl_calendar_rules_text(cfg),
        managed_by: "dsl".into(),
        created_at: now,
        updated_at: now,
    }
}

/// Check whether `name` is owned by the Croniqfile. Mutations on DSL-managed
/// calendars must be refused.
async fn is_dsl_managed_calendar(state: &ServerState, name: &str) -> bool {
    let Some(dsl) = state.dsl_calendars.as_ref() else {
        return false;
    };
    dsl.read().await.iter().any(|c| c.name == name)
}

#[derive(Deserialize)]
pub struct CreateCalendarRequest {
    pub name: String,
    pub timezone: Option<String>,
    /// Calendar rules in Croniqfile DSL syntax (lines of `include`/`exclude`/`timezone`).
    pub rules: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationError {
    pub error: &'static str,
    pub message: String,
}

/// Reject a `timezone` that is not an IANA zone name, with the did-you-mean
/// from `croniq_config::timezone` (issue #426's helper, applied to the one
/// timezone-bearing column that never went through it).
///
/// The calendar's zone decides which day its rules gate and when its `window`
/// rules open (issue #450), so a typo here shifts every job that consults the
/// calendar. The loader falls back to UTC with a warning for rows written
/// before this check existed; new writes fail at the request that made them.
/// An empty string means "unset", matching `handle_update`'s clear convention.
fn check_timezone_valid(timezone: Option<&str>) -> Result<(), (StatusCode, Json<ValidationError>)> {
    let Some(name) = timezone.filter(|t| !t.is_empty()) else {
        return Ok(());
    };
    croniq_config::timezone::parse(name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ValidationError {
                error: "unknown_timezone",
                message: e.to_string(),
            }),
        )
    })?;
    Ok(())
}

/// Validate free-form calendar rules by wrapping them in a dummy calendar
/// block and running the Croniqfile parser plus semantic validation.
/// Returns a human-readable error message on failure.
///
/// Semantic validation uses the same `calendar_args` checks as the
/// scheduler's compile step, so the API can no longer store rules the
/// loader would reject (#356).
fn validate_rules(rules: &str) -> Result<(), String> {
    if rules.trim().is_empty() {
        return Ok(());
    }
    let source = format!("calendar \"__validate__\" {{\n{rules}\n}}\n");
    let ast = Parser::parse(&source).map_err(|e| e.to_string())?;
    let errors: Vec<String> = croniq_config::validate::validate(&ast)
        .into_iter()
        .filter(|d| d.severity == croniq_config::validate::Severity::Error)
        .map(|d| d.message)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Re-attach calendar gates to every running API-managed trigger that
/// references one of `names`, after a calendar was edited or deleted (issue
/// #393). Without this, an already-attached trigger keeps its stale compiled
/// calendar (or none) until the next reload/restart — invisible on
/// pure-API deployments that never reload.
///
/// Each affected trigger is rebuilt against the *current* calendar set and
/// re-sent as `AddJob` (replaces in place, keyed by job_key). A now-dangling
/// reference (renamed/deleted calendar) fails closed under `strict_calendars`
/// — the trigger is paused and a `config_error` recorded — instead of running
/// with a gate that no longer exists. Returns the number of triggers rebuilt.
async fn propagate_calendar_change(state: &ServerState, names: &[&str]) -> usize {
    let Some(ref tx) = state.scheduler_tx else {
        return 0;
    };
    let Some(store) = state.store.as_ref() else {
        return 0;
    };
    let Ok(triggers) = store.list_triggers(None) else {
        return 0;
    };
    let now = Utc::now();
    let resolved = state.resolved_calendars().await;
    let mut rebuilt = 0;
    for def in &triggers {
        if def.managed_by == "dsl" || !def.enabled {
            continue;
        }
        let refs = def.calendar.as_deref().is_some_and(|c| names.contains(&c));
        if !refs {
            continue;
        }
        if let Some(built) = crate::loader::trigger_from_definition(def, &resolved, now) {
            let job_config = crate::loader::job_config_from_definition(def, None);
            let _ = tx.send(crate::scheduler::SchedulerCommand::AddJob {
                job: Box::new(job_config),
                trigger: Box::new(built.trigger),
            });
            state.set_config_fault(&def.job_key, built.config_fault);
            rebuilt += 1;
        }
    }
    rebuilt
}

/// `GET /v1/calendars` — returns the union of API-persisted and
/// DSL-synthesized calendars. On name collision the DSL entry wins, mirroring
/// the precedence rule used for jobs/triggers in `loader.rs`.
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<CalendarDefinition>>, StatusCode> {
    require_scope(&ctx, Scope::CALENDARS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut cals = store
        .list_calendars()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(dsl) = state.dsl_calendars.as_ref() {
        let guard = dsl.read().await;
        let dsl_names: HashSet<String> = guard.iter().map(|c| c.name.clone()).collect();
        // DSL precedence: drop any API row whose name collides with a DSL one.
        cals.retain(|c| !dsl_names.contains(&c.name));
        let now = Utc::now();
        for cfg in guard.iter() {
            cals.push(synth_calendar_def_from_dsl(cfg, now));
        }
        cals.sort_by(|a, b| a.name.cmp(&b.name));
    }

    Ok(Json(cals))
}

/// `GET /v1/calendars/{id}` — supports both real UUIDs and synthetic
/// `dsl:{name}` IDs.
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<CalendarDefinition>, StatusCode> {
    require_scope(&ctx, Scope::CALENDARS_READ)?;

    if is_dsl_id(&id) {
        let name = &id[DSL_ID_PREFIX.len()..];
        if let Some(dsl) = state.dsl_calendars.as_ref()
            && let Some(cfg) = dsl.read().await.iter().find(|c| c.name == name)
        {
            return Ok(Json(synth_calendar_def_from_dsl(cfg, Utc::now())));
        }
        return Err(StatusCode::NOT_FOUND);
    }

    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .get_calendar(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `POST /v1/calendars`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateCalendarRequest>,
) -> Result<(StatusCode, Json<CalendarDefinition>), (StatusCode, Json<ValidationError>)> {
    if !ctx.has_scope(Scope::CALENDARS_WRITE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ValidationError {
                error: "forbidden",
                message: format!("missing scope: {}", Scope::CALENDARS_WRITE),
            }),
        ));
    }
    if is_dsl_managed_calendar(&state, &req.name).await {
        return Err((
            StatusCode::CONFLICT,
            Json(ValidationError {
                error: "dsl_managed",
                message: format!(
                    "calendar '{name}' is managed by the Croniqfile. Edit the file and reload, or call POST /v1/calendars/dsl:{name}/adopt to take ownership (requires `policy {{ dsl_adopt_on_mutate true }}`).",
                    name = req.name
                ),
            }),
        ));
    }
    if let Err(message) = validate_rules(&req.rules) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ValidationError {
                error: "invalid_rules",
                message,
            }),
        ));
    }
    check_timezone_valid(req.timezone.as_deref())?;
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ValidationError {
            error: "no_store",
            message: "store unavailable".into(),
        }),
    ))?;
    let now = Utc::now();
    let cal = CalendarDefinition {
        calendar_id: Uuid::new_v4().to_string(),
        name: req.name,
        timezone: req.timezone,
        rules: req.rules,
        managed_by: "api".into(),
        created_at: now,
        updated_at: now,
    };
    store.create_calendar(&cal).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ValidationError {
                error: "store_error",
                message: "failed to persist calendar".into(),
            }),
        )
    })?;
    Ok((StatusCode::CREATED, Json(cal)))
}

#[derive(Deserialize)]
pub struct UpdateCalendarRequest {
    pub name: Option<String>,
    pub timezone: Option<String>,
    pub rules: Option<String>,
}

/// `PUT /v1/calendars/{id}` — patch one or more fields of an existing
/// calendar. Omitted fields are left untouched. Same validation as
/// `handle_create`: `rules`, when provided, must parse as Croniqfile DSL.
pub async fn handle_update(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<UpdateCalendarRequest>,
) -> Result<Json<CalendarDefinition>, (StatusCode, Json<ValidationError>)> {
    if !ctx.has_scope(Scope::CALENDARS_WRITE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ValidationError {
                error: "forbidden",
                message: format!("missing scope: {}", Scope::CALENDARS_WRITE),
            }),
        ));
    }
    if is_dsl_id(&id) {
        let name = id.strip_prefix("dsl:").unwrap_or(&id);
        return Err((
            StatusCode::CONFLICT,
            Json(ValidationError {
                error: "dsl_managed",
                message: format!(
                    "calendar '{name}' is managed by the Croniqfile. Edit the file and reload, or call POST /v1/calendars/dsl:{name}/adopt to take ownership (requires `policy {{ dsl_adopt_on_mutate true }}`)."
                ),
            }),
        ));
    }
    if let Some(ref rules) = req.rules
        && let Err(message) = validate_rules(rules)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ValidationError {
                error: "invalid_rules",
                message,
            }),
        ));
    }
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ValidationError {
            error: "no_store",
            message: "store unavailable".into(),
        }),
    ))?;

    let mut existing = store
        .get_calendar(&id)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ValidationError {
                    error: "store_error",
                    message: "failed to load calendar".into(),
                }),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ValidationError {
                error: "not_found",
                message: format!("calendar {id} not found"),
            }),
        ))?;

    if existing.managed_by == "dsl" {
        return Err((
            StatusCode::CONFLICT,
            Json(ValidationError {
                error: "dsl_managed",
                message: format!(
                    "calendar '{name}' is managed by the Croniqfile. Edit the file and reload, or call POST /v1/calendars/dsl:{name}/adopt to take ownership (requires `policy {{ dsl_adopt_on_mutate true }}`).",
                    name = existing.name
                ),
            }),
        ));
    }

    // Capture the pre-edit name: a rename must re-resolve triggers pointing
    // at BOTH the old name (now dangling → fail closed) and the new name
    // (may heal a previously-faulted trigger already referencing it).
    let old_name = existing.name.clone();
    if let Some(name) = req.name {
        // Renaming onto a DSL name would silently shadow it on next read; reject.
        if name != existing.name && is_dsl_managed_calendar(&state, &name).await {
            return Err((
                StatusCode::CONFLICT,
                Json(ValidationError {
                    error: "dsl_managed",
                    message: format!(
                        "calendar '{name}' is managed by the Croniqfile — pick a different name"
                    ),
                }),
            ));
        }
        existing.name = name;
    }
    if let Some(tz) = req.timezone {
        check_timezone_valid(Some(tz.as_str()))?;
        existing.timezone = if tz.is_empty() { None } else { Some(tz) };
    }
    if let Some(rules) = req.rules {
        existing.rules = rules;
    }
    existing.updated_at = Utc::now();

    store.create_calendar(&existing).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ValidationError {
                error: "store_error",
                message: "failed to persist calendar".into(),
            }),
        )
    })?;

    // Propagate the edit to running triggers (issue #393). Both names matter
    // on a rename; when unchanged they collapse to a single entry.
    let new_name = existing.name.clone();
    let mut affected = vec![old_name.as_str()];
    if new_name != old_name {
        affected.push(new_name.as_str());
    }
    let rebuilt = propagate_calendar_change(&state, &affected).await;
    if rebuilt > 0 {
        tracing::info!(
            calendar = %new_name,
            rebuilt,
            "calendar update propagated to running triggers"
        );
    }

    Ok(Json(existing))
}

/// `DELETE /v1/calendars/{id}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ValidationError>)> {
    if !ctx.has_scope(Scope::CALENDARS_WRITE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ValidationError {
                error: "forbidden",
                message: format!("missing scope: {}", Scope::CALENDARS_WRITE),
            }),
        ));
    }
    if is_dsl_id(&id) {
        let name = id.strip_prefix("dsl:").unwrap_or(&id);
        return Err((
            StatusCode::CONFLICT,
            Json(ValidationError {
                error: "dsl_managed",
                message: format!(
                    "calendar '{name}' is managed by the Croniqfile. Remove it from the file and reload, or call POST /v1/calendars/dsl:{name}/adopt then DELETE the API copy (requires `policy {{ dsl_adopt_on_mutate true }}`)."
                ),
            }),
        ));
    }
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ValidationError {
            error: "no_store",
            message: "store unavailable".into(),
        }),
    ))?;
    let existing = store.get_calendar(&id).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ValidationError {
                error: "store_error",
                message: "failed to load calendar".into(),
            }),
        )
    })?;
    if let Some(ref existing) = existing
        && existing.managed_by == "dsl"
    {
        return Err((
            StatusCode::CONFLICT,
            Json(ValidationError {
                error: "dsl_managed",
                message: format!(
                    "calendar '{name}' is managed by the Croniqfile. Remove it from the file and reload, or call POST /v1/calendars/dsl:{name}/adopt then DELETE the API copy (requires `policy {{ dsl_adopt_on_mutate true }}`).",
                    name = existing.name
                ),
            }),
        ));
    }
    store.delete_calendar(&id).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ValidationError {
                error: "store_error",
                message: "failed to delete calendar".into(),
            }),
        )
    })?;

    // Fail-close any running triggers that referenced the deleted calendar
    // (issue #393): re-resolving now finds no match, so under
    // `strict_calendars` those triggers pause with a `config_error` instead
    // of firing with a stale compiled gate. A DSL calendar of the same name
    // still shadows via the resolver's DSL precedence, keeping those armed.
    if let Some(existing) = existing {
        let rebuilt = propagate_calendar_change(&state, &[existing.name.as_str()]).await;
        if rebuilt > 0 {
            tracing::info!(
                calendar = %existing.name,
                rebuilt,
                "calendar deletion propagated to running triggers"
            );
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct AdoptResponse {
    pub calendar: CalendarDefinition,
    /// Phase 2 reload semantics: this row replaces the DSL definition until
    /// `unadopt` is called.
    pub dsl_key: String,
}

/// `POST /v1/calendars/dsl:{name}/adopt` — copy the DSL calendar into the
/// API store with `managed_by="api"` and a fresh UUID, and record the
/// adoption so the loader skips the DSL key on subsequent reloads.
///
/// Requires `policy { dsl_adopt_on_mutate true }` in the Croniqfile. Returns
/// 409 when the policy is off, the ID is not a DSL synthetic, or the DSL
/// calendar with that name doesn't exist.
pub async fn handle_adopt(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<(StatusCode, Json<AdoptResponse>), (StatusCode, Json<ValidationError>)> {
    if !ctx.has_scope(Scope::CALENDARS_WRITE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ValidationError {
                error: "forbidden",
                message: format!("missing scope: {}", Scope::CALENDARS_WRITE),
            }),
        ));
    }
    if !state.policy_dsl_adopt_on_mutate.load(Ordering::Relaxed) {
        return Err((
            StatusCode::CONFLICT,
            Json(ValidationError {
                error: "adoption_disabled",
                message:
                    "DSL adoption is disabled — set `policy { dsl_adopt_on_mutate true }` in the Croniqfile to enable"
                        .into(),
            }),
        ));
    }
    if !is_dsl_id(&id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ValidationError {
                error: "not_dsl_id",
                message: format!("'{id}' is not a DSL calendar ID (must start with 'dsl:')"),
            }),
        ));
    }
    let name = id[DSL_ID_PREFIX.len()..].to_string();

    // Pull the DSL config snapshot and synthesize a definition to persist.
    let cfg = {
        let dsl = state.dsl_calendars.as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ValidationError {
                error: "no_dsl_state",
                message: "DSL calendar state not available".into(),
            }),
        ))?;
        let guard = dsl.read().await;
        match guard.iter().find(|c| c.name == name) {
            Some(c) => c.clone(),
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ValidationError {
                        error: "not_found",
                        message: format!("DSL calendar '{name}' not found"),
                    }),
                ));
            }
        }
    };

    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ValidationError {
            error: "no_store",
            message: "store unavailable".into(),
        }),
    ))?;

    let now = Utc::now();
    let mut adopted = synth_calendar_def_from_dsl(&cfg, now);
    // Persist as a real API-managed row with a fresh UUID.
    adopted.calendar_id = Uuid::new_v4().to_string();
    adopted.managed_by = "api".into();
    adopted.created_at = now;
    adopted.updated_at = now;

    store.create_calendar(&adopted).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ValidationError {
                error: "store_error",
                message: "failed to persist adopted calendar".into(),
            }),
        )
    })?;
    store
        .insert_adoption(&DslAdoption {
            resource_type: "calendar".into(),
            resource_key: name.clone(),
            adopted_at: now,
            adopted_by: Some(ctx.caller_id.clone()),
        })
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ValidationError {
                    error: "store_error",
                    message: "failed to record adoption".into(),
                }),
            )
        })?;

    // Drop the DSL entry from the in-memory snapshot so subsequent reads
    // don't double-count. The next reload will re-emit a filtered set.
    if let Some(dsl) = state.dsl_calendars.as_ref() {
        let mut guard = dsl.write().await;
        guard.retain(|c| c.name != name);
    }

    Ok((
        StatusCode::OK,
        Json(AdoptResponse {
            calendar: adopted,
            dsl_key: name,
        }),
    ))
}

/// `POST /v1/calendars/{id}/unadopt` — drop the API copy plus the
/// `dsl_adoptions` record so the next reload reinstates the DSL definition.
/// Returns 404 if the row doesn't exist or wasn't adopted from DSL.
pub async fn handle_unadopt(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ValidationError>)> {
    if !ctx.has_scope(Scope::CALENDARS_WRITE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ValidationError {
                error: "forbidden",
                message: format!("missing scope: {}", Scope::CALENDARS_WRITE),
            }),
        ));
    }
    if is_dsl_id(&id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ValidationError {
                error: "not_api_id",
                message:
                    "unadopt takes the API UUID returned from /adopt, not the DSL synthetic id"
                        .into(),
            }),
        ));
    }
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ValidationError {
            error: "no_store",
            message: "store unavailable".into(),
        }),
    ))?;

    let existing = store
        .get_calendar(&id)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ValidationError {
                    error: "store_error",
                    message: "failed to load calendar".into(),
                }),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ValidationError {
                error: "not_found",
                message: format!("calendar {id} not found"),
            }),
        ))?;

    let was_adopted = store.is_adopted("calendar", &existing.name).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ValidationError {
                error: "store_error",
                message: "failed to look up adoption".into(),
            }),
        )
    })?;
    if !was_adopted {
        return Err((
            StatusCode::CONFLICT,
            Json(ValidationError {
                error: "not_adopted",
                message: format!(
                    "calendar '{}' was not adopted from DSL — use DELETE to remove API-only calendars",
                    existing.name
                ),
            }),
        ));
    }

    let _ = ctx; // adopted_by tracking is best-effort; unadopt is symmetric.

    store.delete_calendar(&id).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ValidationError {
                error: "store_error",
                message: "failed to delete API calendar".into(),
            }),
        )
    })?;
    store
        .delete_adoption("calendar", &existing.name)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ValidationError {
                    error: "store_error",
                    message: "failed to clear adoption".into(),
                }),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::{dsl_calendar_rules_text, validate_rules};
    use croniq_config::compile::{CalendarConfig, CalendarRuleConfig};

    #[test]
    fn validate_rules_accepts_compilable_rules() {
        assert!(validate_rules("").is_ok());
        assert!(validate_rules("include weekly weekday").is_ok());
        assert!(validate_rules("include weekly Mon..Fri\nexclude annual 12-25").is_ok());
        assert!(validate_rules("include window \"08:00\"..\"18:00\"").is_ok());
    }

    #[test]
    fn validate_rules_rejects_uncompilable_rules() {
        // #356: rules the loader would reject must not be storable.
        let err = validate_rules("include weekly funday").unwrap_err();
        assert!(err.contains("unknown weekday: funday"), "got: {err}");
        let err = validate_rules("include window \"25:00\"..\"26:00\"").unwrap_err();
        assert!(err.contains("invalid time: 25:00"), "got: {err}");
    }

    #[test]
    fn dsl_rules_text_is_canonical_and_reparses() {
        let rule = |kind: &str, rule_type: &str, args: &[&str]| CalendarRuleConfig {
            kind: kind.into(),
            rule_type: rule_type.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
        };
        let cfg = CalendarConfig {
            name: "biz".into(),
            timezone: Some("Europe/Vienna".into()),
            rules: vec![
                rule(
                    "include",
                    "weekly",
                    &["monday", "tuesday", "wednesday", "thursday", "friday"],
                ),
                rule("include", "window", &["08:00", "18:00"]),
                rule("exclude", "annual", &["12-25", "12-26"]),
            ],
        };
        let text = dsl_calendar_rules_text(&cfg);
        assert_eq!(
            text,
            "include weekly weekday\ninclude window \"08:00\"..\"18:00\"\nexclude annual 12-25 12-26"
        );
        // Round-trip: the synthesized text must pass the same
        // validation gate as user-supplied API rules.
        assert!(validate_rules(&text).is_ok());
    }

    // ─── propagate_calendar_change (#393) ───────────────────────────────────

    use super::propagate_calendar_change;
    use crate::api::ServerState;
    use crate::scheduler::SchedulerCommand;
    use crate::store::{DynStore, sqlite_store};
    use croniq_runner::AppState;
    use croniq_store::models::{CalendarDefinition, TriggerDefinition};
    use croniq_store::sqlite::SqliteStore;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    fn store_calendar(store: &DynStore, name: &str, rules: &str) {
        let now = chrono::Utc::now();
        store
            .create_calendar(&CalendarDefinition {
                calendar_id: Uuid::new_v4().to_string(),
                name: name.into(),
                timezone: None,
                rules: rules.into(),
                managed_by: "api".into(),
                created_at: now,
                updated_at: now,
            })
            .unwrap();
    }

    fn api_trigger(store: &DynStore, job_key: &str, calendar: &str) {
        let now = chrono::Utc::now();
        store
            .create_trigger(&TriggerDefinition {
                trigger_id: Uuid::new_v4().to_string(),
                job_key: job_key.into(),
                cron_expression: Some("1m".into()),
                timezone: None,
                calendar: Some(calendar.into()),
                window: None,
                not_before: None,
                not_after: None,
                enabled: true,
                managed_by: "api".into(),
                created_at: now,
                updated_at: now,
            })
            .unwrap();
    }

    fn state_with_sched(
        store: DynStore,
    ) -> (Arc<ServerState>, mpsc::UnboundedReceiver<SchedulerCommand>) {
        let runner = AppState::new();
        let (comp_tx, _comp_rx) = mpsc::unbounded_channel();
        let (sched_tx, sched_rx) = mpsc::unbounded_channel();
        let mut state = ServerState::with_auth(runner, comp_tx, None, Some(store));
        Arc::get_mut(&mut state).unwrap().scheduler_tx = Some(sched_tx);
        (state, sched_rx)
    }

    #[tokio::test]
    async fn propagate_reattaches_gate_to_referencing_trigger() {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        store_calendar(&store, "biz", "include weekly weekday");
        api_trigger(&store, "api:gated", "biz");
        let (state, mut rx) = state_with_sched(store);

        let rebuilt = propagate_calendar_change(&state, &["biz"]).await;
        assert_eq!(rebuilt, 1);
        match rx.try_recv().expect("AddJob pushed") {
            SchedulerCommand::AddJob { trigger, .. } => assert!(trigger.calendar.is_some()),
            other => panic!("expected AddJob, got {other:?}"),
        }
        assert!(state.config_faults.read().unwrap().is_empty());
    }

    #[tokio::test]
    async fn propagate_after_delete_faults_dangling_trigger() {
        // Simulate a deletion: the trigger still references "biz" but the
        // calendar is gone from the store. Under strict the trigger pauses.
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        api_trigger(&store, "api:gated", "biz");
        let (state, mut rx) = state_with_sched(store);

        let rebuilt = propagate_calendar_change(&state, &["biz"]).await;
        assert_eq!(rebuilt, 1);
        match rx.try_recv().expect("AddJob pushed") {
            SchedulerCommand::AddJob { trigger, .. } => {
                assert_eq!(
                    trigger.state,
                    croniq_scheduler::trigger::TriggerState::Paused
                );
            }
            other => panic!("expected AddJob, got {other:?}"),
        }
        assert!(
            state
                .config_faults
                .read()
                .unwrap()
                .contains_key("api:gated")
        );
    }
}
