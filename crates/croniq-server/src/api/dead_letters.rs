//! Dead letter management endpoints.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_runner::WorkItem;
use croniq_store::models::{DeadLetter, DeadLetterFilter, Execution, ExecutionState};
use croniq_store::traits::StoreError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub job_key: Option<String>,
    pub limit: Option<u32>,
}

/// `GET /v1/dead-letters`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<DeadLetter>>, StatusCode> {
    require_scope(&ctx, Scope::DEAD_LETTERS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let filter = DeadLetterFilter {
        job_key: q.job_key,
        limit: Some(q.limit.unwrap_or(50)),
    };
    let letters = store
        .list_dead_letters(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(letters))
}

/// `GET /v1/dead-letters/{id}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<DeadLetter>, StatusCode> {
    require_scope(&ctx, Scope::DEAD_LETTERS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    store
        .get_dead_letter(uuid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `DELETE /v1/dead-letters/{id}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_scope(&ctx, Scope::DEAD_LETTERS_WRITE) {
        return s;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Ok(uuid) = Uuid::parse_str(&id) else {
        return StatusCode::BAD_REQUEST;
    };
    match store.remove_dead_letter(uuid) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize, Default)]
pub struct BulkDeleteRequest {
    /// Explicit dead-letter ids to delete. Takes precedence over `all`
    /// when non-empty.
    #[serde(default)]
    pub ids: Vec<String>,
    /// Delete every pending dead letter (optionally scoped to `job_key`).
    #[serde(default)]
    pub all: bool,
    /// Restrict an `all` delete to a single `job_key`. Ignored when `ids`
    /// is provided.
    #[serde(default)]
    pub job_key: Option<String>,
}

#[derive(Serialize)]
pub struct BulkDeleteResponse {
    pub deleted: u64,
}

/// `POST /v1/dead-letters/bulk-delete` — remove many dead letters at once:
/// either an explicit `ids` list, or (with `all: true`) the whole queue,
/// optionally scoped to a single `job_key`. Returns the number deleted.
pub async fn handle_bulk_delete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<BulkDeleteRequest>,
) -> Result<Json<BulkDeleteResponse>, StatusCode> {
    require_scope(&ctx, Scope::DEAD_LETTERS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let deleted = if !req.ids.is_empty() {
        let ids = req
            .ids
            .iter()
            .map(|s| Uuid::parse_str(s))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        store
            .remove_dead_letters(&ids)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else if req.all {
        store
            .clear_dead_letters(req.job_key.as_deref())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        // Neither an id list nor an explicit `all` flag — refuse rather
        // than silently deleting nothing (or, worse, the whole queue).
        return Err(StatusCode::BAD_REQUEST);
    };

    Ok(Json(BulkDeleteResponse { deleted }))
}

#[derive(Serialize)]
pub struct ReplayResponse {
    pub execution_id: String,
    pub attempt: u32,
    /// The original logical fire time carried into the new execution.
    pub scheduled_for: chrono::DateTime<Utc>,
}

/// Optional request body for replay. `force` overrides the stale-replay guard.
#[derive(Deserialize, Default)]
pub struct ReplayRequest {
    #[serde(default)]
    pub force: bool,
}

/// 409 body when the stale-replay guard rejects a replay.
#[derive(Serialize)]
pub struct ReplayError {
    /// Machine-readable discriminator (`"stale_replay"`).
    pub error: String,
    pub message: String,
    pub scheduled_for: chrono::DateTime<Utc>,
    pub age_seconds: i64,
    pub replay_max_age: String,
}

/// Error type for the replay handler: either a bare status, or the structured
/// 409 the staleness guard produces.
pub enum ReplayApiError {
    Status(StatusCode),
    Stale(Box<ReplayError>),
}

impl From<StatusCode> for ReplayApiError {
    fn from(s: StatusCode) -> Self {
        ReplayApiError::Status(s)
    }
}

impl axum::response::IntoResponse for ReplayApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ReplayApiError::Status(s) => s.into_response(),
            ReplayApiError::Stale(body) => (StatusCode::CONFLICT, Json(*body)).into_response(),
        }
    }
}

/// Resolve a job's compiled config for the replay path: prefer the live DSL
/// jobs, fall back to a store-persisted API job definition. Returns `None`
/// when the job no longer exists anywhere (e.g. deleted since it dead-lettered).
async fn resolve_job_config(
    state: &ServerState,
    job_key: &str,
) -> Option<croniq_config::compile::JobConfig> {
    if let Some(ref dsl_jobs) = state.dsl_jobs {
        let jobs = dsl_jobs.read().await;
        if let Some(j) = jobs.iter().find(|j| j.key == job_key) {
            return Some(j.clone());
        }
    }
    if let Some(ref store) = state.store
        && let Ok(Some(def)) = store.get_job_definition(job_key)
    {
        return Some(crate::loader::job_config_from_job_def(&def));
    }
    None
}

/// `POST /v1/dead-letters/{id}/replay` — replay a dead letter as a new execution.
///
/// Honours the opt-in stale-replay guard: when the job declares
/// `dead_letter { replay_max_age … }` and the dead letter's original
/// `scheduled_for` is older than that, the replay is rejected with 409 unless
/// the request body passes `force: true`. `scheduled_for` (not `created_at`)
/// is the anchor — it measures the drift that breaks time-coupled job logic.
pub async fn handle_replay(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Option<Json<ReplayRequest>>,
) -> Result<(StatusCode, Json<ReplayResponse>), ReplayApiError> {
    require_scope(&ctx, Scope::DEAD_LETTERS_WRITE)?;
    let force = body.map(|Json(b)| b.force).unwrap_or(false);
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let dl_uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let dl = store
        .get_dead_letter(dl_uuid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let now = Utc::now();

    // Resolve job config for the staleness guard and replay fidelity (timeout,
    // require/prefer). A job that no longer exists keeps the legacy behaviour:
    // no guard, safe defaults.
    let job = resolve_job_config(&state, &dl.job_key).await;

    // Stale-replay guard (opt-in via `dead_letter { replay_max_age … }`).
    if let Some(err) = stale_replay_check(job.as_ref(), dl.scheduled_for, now, force) {
        tracing::info!(
            dead_letter_id = %dl_uuid,
            job_key = %dl.job_key,
            age_seconds = err.age_seconds,
            replay_max_age = %err.replay_max_age,
            "replay rejected — dead letter older than replay_max_age (pass force to override)"
        );
        return Err(ReplayApiError::Stale(Box::new(err)));
    }

    let new_id = Uuid::new_v4();
    let next_attempt = dl.attempt + 1;

    // Create a new execution from the dead letter
    let execution = Execution {
        id: new_id,
        job_key: dl.job_key.clone(),
        fire_at: now,
        // Preserve the original logical fire time so a time-coupled job
        // replayed weeks later still computes against the intended instant,
        // not wall-clock now.
        scheduled_for: dl.scheduled_for,
        attempt: next_attempt,
        state: ExecutionState::Queued,
        runner_id: None,
        claimed_at: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
        error: None,
        dead_reason: None,
        // Intentionally not carried: the trigger idempotency key's dedup window
        // (default 10m, #279) is long expired by replay time, and reusing it
        // could wrongly coalesce a legitimate fresh trigger into this replay.
        idempotency_key: None,
        metadata: dl.metadata.clone(),
        created_at: now,
    };

    // Single transaction: a failure leaves neither an orphaned `queued`
    // execution (which would never be enqueued as a work item) nor a
    // still-replayable dead letter. NotFound means a concurrent replay
    // consumed the dead letter between our read and this write.
    store
        .replay_dead_letter(dl_uuid, &execution)
        .map_err(|e| match e {
            StoreError::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;

    // require/prefer: prefer the values captured in the dead letter's metadata,
    // fall back to the job's current runner config, then empty.
    let require = dl
        .metadata
        .get("__require")
        .and_then(|v| serde_json::from_str(v).ok())
        .or_else(|| job.as_ref().map(|j| j.runner.require.clone()))
        .unwrap_or_default();
    let prefer = dl
        .metadata
        .get("__prefer")
        .and_then(|v| serde_json::from_str(v).ok())
        .or_else(|| job.as_ref().map(|j| j.runner.prefer.clone()))
        .unwrap_or_default();
    // Timeout: use the job's configured timeout instead of a hard-coded 5m.
    let timeout = job
        .as_ref()
        .and_then(|j| j.timeout.clone())
        .unwrap_or_else(|| "5m".into());

    // Enqueue work item
    let item = WorkItem {
        execution_id: new_id.to_string(),
        job_key: dl.job_key.clone(),
        fire_at: now,
        scheduled_for: dl.scheduled_for,
        attempt: next_attempt,
        require,
        prefer,
        metadata: serde_json::Value::Object(
            dl.metadata
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
        ),
        timeout,
        // A replay always persists a fresh execution row above, so the
        // dispatch path has a claim target (issue #539).
        is_ephemeral: false,
    };

    {
        let mut q = state.runner.queue.write().await;
        q.enqueue(item);
    }
    state.runner.work_notify.notify_waiters();

    let diff = serde_json::json!({
        "force": force,
        "age_seconds": (now - dl.scheduled_for).num_seconds(),
        "new_execution_id": new_id.to_string(),
    });
    crate::api::audit::record(
        store,
        &ctx,
        "dead_letter.replayed",
        "dead_letter",
        Some(&id),
        Some(&diff.to_string()),
    );

    Ok((
        StatusCode::OK,
        Json(ReplayResponse {
            execution_id: new_id.to_string(),
            attempt: next_attempt,
            scheduled_for: dl.scheduled_for,
        }),
    ))
}

/// Pure staleness decision for a replay. Returns `Some(ReplayError)` when the
/// job declares `dead_letter { replay_max_age … }`, the request is not forced,
/// and the dead letter's original `scheduled_for` is older than the limit.
/// `None` means the replay may proceed (no policy, forced, within window, or
/// unparseable duration → fail open).
fn stale_replay_check(
    job: Option<&croniq_config::compile::JobConfig>,
    scheduled_for: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
    force: bool,
) -> Option<ReplayError> {
    if force {
        return None;
    }
    let max_age_str = job?.dead_letter.replay_max_age.as_ref()?;
    let max_age = croniq_execution::retry::parse_duration(max_age_str)?;
    let age = now - scheduled_for;
    if age <= chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::MAX) {
        return None;
    }
    let age_seconds = age.num_seconds();
    Some(ReplayError {
        error: "stale_replay".into(),
        message: format!(
            "Originally scheduled {} ago; job declares replay_max_age {max_age_str}. Pass force:true to replay anyway.",
            humanize_age(age_seconds)
        ),
        scheduled_for,
        age_seconds,
        replay_max_age: max_age_str.clone(),
    })
}

/// Render an age in seconds as a coarse human string for the 409 message
/// (e.g. `34d`, `5h`, `12m`, `8s`).
fn humanize_age(secs: i64) -> String {
    let secs = secs.max(0);
    if secs >= 86400 {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use croniq_config::compile::{JobConfig, compile};
    use croniq_config::parser::Parser;

    /// Build a compiled `JobConfig` from a minimal Croniqfile, optionally with
    /// a `replay_max_age`. Exercises the real DSL → config path.
    fn job_with_replay_max_age(v: Option<&str>) -> JobConfig {
        let dsl = match v {
            Some(age) => format!(
                "job billing:report {{ every 5 minutes; dead_letter {{ replay_max_age {age} }} }}"
            ),
            None => "job billing:report { every 5 minutes }".to_string(),
        };
        let ast = Parser::parse(&dsl).unwrap();
        compile(&ast).jobs.remove(0)
    }

    fn ts(s: &str) -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn no_policy_allows_replay() {
        let job = job_with_replay_max_age(None);
        assert!(
            stale_replay_check(
                Some(&job),
                ts("2026-01-01T00:00:00Z"),
                ts("2027-01-01T00:00:00Z"),
                false
            )
            .is_none()
        );
    }

    #[test]
    fn missing_job_allows_replay() {
        assert!(
            stale_replay_check(
                None,
                ts("2026-01-01T00:00:00Z"),
                ts("2027-01-01T00:00:00Z"),
                false
            )
            .is_none()
        );
    }

    #[test]
    fn within_window_allows_replay() {
        let job = job_with_replay_max_age(Some("7d"));
        // 3 days old < 7d window
        assert!(
            stale_replay_check(
                Some(&job),
                ts("2026-06-01T00:00:00Z"),
                ts("2026-06-04T00:00:00Z"),
                false
            )
            .is_none()
        );
    }

    #[test]
    fn beyond_window_rejects_replay() {
        let job = job_with_replay_max_age(Some("7d"));
        // 10 days old > 7d window
        let err = stale_replay_check(
            Some(&job),
            ts("2026-06-01T00:00:00Z"),
            ts("2026-06-11T00:00:00Z"),
            false,
        )
        .expect("should reject");
        assert_eq!(err.error, "stale_replay");
        assert_eq!(err.replay_max_age, "7d");
        assert_eq!(err.age_seconds, 10 * 86400);
        assert_eq!(err.scheduled_for, ts("2026-06-01T00:00:00Z"));
    }

    #[test]
    fn force_bypasses_guard() {
        let job = job_with_replay_max_age(Some("7d"));
        assert!(
            stale_replay_check(
                Some(&job),
                ts("2026-06-01T00:00:00Z"),
                ts("2026-06-30T00:00:00Z"),
                true
            )
            .is_none()
        );
    }

    #[test]
    fn unparseable_duration_fails_open() {
        let job = job_with_replay_max_age(Some("not-a-duration"));
        assert!(
            stale_replay_check(
                Some(&job),
                ts("2026-06-01T00:00:00Z"),
                ts("2027-06-01T00:00:00Z"),
                false
            )
            .is_none()
        );
    }

    /// Build a store-persisted `JobDefinition` (an API-registered job, no
    /// Croniqfile entry) with an optional `dead_letter_replay_max_age` and
    /// run it through the same `job_config_from_job_def` fallback the replay
    /// handler uses — the guard must work for API jobs, not just DSL ones.
    fn api_job_with_replay_max_age(v: Option<&str>) -> JobConfig {
        let now = ts("2026-01-01T00:00:00Z");
        let def = croniq_store::models::JobDefinition {
            job_key: "api:report".into(),
            description: None,
            assigned_runner_id: None,
            is_active: true,
            metadata: Default::default(),
            created_at: now,
            updated_at: now,
            timeout: None,
            max_retries: None,
            dead_letter_enabled: None,
            dead_letter_retention: None,
            dead_letter_operator_hint: None,
            dead_letter_replay_max_age: v.map(str::to_string),
            tags: vec![],
        };
        crate::loader::job_config_from_job_def(&def)
    }

    #[test]
    fn api_job_beyond_window_rejects_replay() {
        let job = api_job_with_replay_max_age(Some("7d"));
        // 10 days old > 7d window — same 409 shape as the DSL path.
        let err = stale_replay_check(
            Some(&job),
            ts("2026-06-01T00:00:00Z"),
            ts("2026-06-11T00:00:00Z"),
            false,
        )
        .expect("should reject");
        assert_eq!(err.error, "stale_replay");
        assert_eq!(err.replay_max_age, "7d");
    }

    #[test]
    fn api_job_force_bypasses_guard() {
        let job = api_job_with_replay_max_age(Some("7d"));
        assert!(
            stale_replay_check(
                Some(&job),
                ts("2026-06-01T00:00:00Z"),
                ts("2026-06-30T00:00:00Z"),
                true
            )
            .is_none()
        );
    }

    #[test]
    fn api_job_without_guard_allows_stale_replay() {
        // NULL column = guard not configured — the pre-023 behaviour.
        let job = api_job_with_replay_max_age(None);
        assert!(
            stale_replay_check(
                Some(&job),
                ts("2026-01-01T00:00:00Z"),
                ts("2027-01-01T00:00:00Z"),
                false
            )
            .is_none()
        );
    }

    #[test]
    fn humanize_age_units() {
        assert_eq!(humanize_age(34 * 86400), "34d");
        assert_eq!(humanize_age(5 * 3600), "5h");
        assert_eq!(humanize_age(12 * 60), "12m");
        assert_eq!(humanize_age(8), "8s");
        assert_eq!(humanize_age(-5), "0s");
    }
}
