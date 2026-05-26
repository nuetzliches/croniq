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

#[cfg(test)]
mod tests {
    use super::*;
    use croniq_auth::context::{AuthMethod, CallerType};
    use croniq_runner::AppState;
    use croniq_store::models::JobDefinition;
    use croniq_store::traits::JobDefinitionStore;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    fn ctx_with_scopes(scopes: &[&str]) -> CallerContext {
        CallerContext {
            caller_type: CallerType::User,
            caller_id: "test".into(),
            client_id: "test".into(),
            user_id: Some("test".into()),
            role: None,
            auth_method: AuthMethod::Password,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn job_with_tags(job_key: &str, tags: &[&str]) -> JobDefinition {
        let now = chrono::Utc::now();
        JobDefinition {
            job_key: job_key.into(),
            description: None,
            assigned_runner_id: None,
            is_active: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
            timeout: None,
            max_retries: None,
            dead_letter_enabled: None,
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn state_with_jobs(jobs: Vec<JobDefinition>) -> Arc<ServerState> {
        let sqlite = croniq_store::sqlite::SqliteStore::in_memory().unwrap();
        for j in &jobs {
            sqlite.create_job_definition(j).unwrap();
        }
        let store = crate::store::sqlite_store(sqlite);
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::with_auth(AppState::new(), tx, None, Some(store))
    }

    fn query(entity: &str) -> axum::extract::Query<TagQuery> {
        axum::extract::Query(TagQuery {
            entity: entity.into(),
        })
    }

    #[tokio::test]
    async fn aggregates_job_tags_by_count_then_name() {
        let state = state_with_jobs(vec![
            job_with_tags("a:one", &["env=prod", "team=ops"]),
            job_with_tags("b:two", &["env=prod"]),
            job_with_tags("c:three", &["env=staging", "team=ops"]),
        ]);

        let res = handle_list_tags(
            State(state),
            Extension(ctx_with_scopes(&["admin"])),
            query("jobs"),
        )
        .await
        .unwrap();

        // env=prod:2, team=ops:2 (tie → alphabetical), env=staging:1.
        let got: Vec<(&str, usize)> = res.0.iter().map(|t| (t.tag.as_str(), t.count)).collect();
        assert_eq!(
            got,
            vec![("env=prod", 2), ("team=ops", 2), ("env=staging", 1)]
        );
    }

    #[tokio::test]
    async fn unknown_entity_is_bad_request() {
        let state = state_with_jobs(vec![]);
        let err = handle_list_tags(
            State(state),
            Extension(ctx_with_scopes(&["admin"])),
            query("widgets"),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn jobs_entity_requires_jobs_read_scope() {
        let state = state_with_jobs(vec![job_with_tags("a:one", &["env=prod"])]);
        // Has runners:read but not jobs:read (and not admin).
        let err = handle_list_tags(
            State(state),
            Extension(ctx_with_scopes(&["runners:read"])),
            query("jobs"),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::FORBIDDEN);
    }
}
