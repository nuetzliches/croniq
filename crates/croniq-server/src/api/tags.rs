//! Tag aggregation endpoints — returns distinct tag values across an entity
//! kind for UI autocomplete and filter chips.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use serde::{Deserialize, Serialize};

use super::ServerState;
use crate::api::auth_middleware::require_scope;
use crate::loader::synth_job_def_from_dsl;

#[derive(Debug, Deserialize)]
pub struct TagQuery {
    /// Entity kind to aggregate over: `"jobs"` or `"runners"`.
    #[serde(default = "default_entity")]
    pub entity: String,
}

fn default_entity() -> String {
    "jobs".into()
}

#[derive(Debug, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub count: usize,
}

/// `GET /v1/tags?entity={jobs|runners}` — distinct tags across the entity kind
/// with usage counts, sorted by count desc then alphabetically.
pub async fn handle_list_tags(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Query(q): axum::extract::Query<TagQuery>,
) -> Result<Json<Vec<TagCount>>, StatusCode> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    match q.entity.as_str() {
        "jobs" => {
            require_scope(&ctx, Scope::JOBS_READ)?;
            let store = state
                .store
                .as_ref()
                .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

            let jobs = store
                .list_job_definitions()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            for j in &jobs {
                for t in &j.tags {
                    *counts.entry(t.clone()).or_insert(0) += 1;
                }
            }

            // DSL-only jobs (those with no row in job_definitions).
            if let Some(dsl) = state.dsl_jobs.as_ref() {
                let guard = dsl.read().await;
                let seen: std::collections::HashSet<&str> =
                    jobs.iter().map(|j| j.job_key.as_str()).collect();
                let now = chrono::Utc::now();
                for cfg in guard.iter() {
                    if seen.contains(cfg.key.as_str()) {
                        continue;
                    }
                    let synth = synth_job_def_from_dsl(cfg, now);
                    for t in &synth.tags {
                        *counts.entry(t.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        "runners" => {
            require_scope(&ctx, Scope::RUNNERS_READ)?;
            let reg = state.runner.registry.read().await;
            for r in reg.all() {
                for t in &r.tags {
                    *counts.entry(t.clone()).or_insert(0) += 1;
                }
            }
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    }

    let mut out: Vec<TagCount> = counts
        .into_iter()
        .map(|(tag, count)| TagCount { tag, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
    Ok(Json(out))
}
