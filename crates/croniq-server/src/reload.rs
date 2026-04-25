//! Croniqfile reload helper.
//!
//! Builds a validated `ReloadPlan` from a path: parses the file, merges the
//! resulting DSL jobs with API-registered triggers from the store (DSL
//! precedence), and diffs the merged state against what the scheduler is
//! currently running. Does NOT mutate any state — callers decide whether to
//! apply the plan or discard it (dry-run).
//!
//! Used by:
//! - The file-watcher reload path
//! - The SIGHUP handler (unix only)
//! - `POST /v1/admin/reload-config`

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use croniq_config::compile::JobConfig;
use croniq_scheduler::trigger::Trigger;
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::loader::{
    LoadError, LoadedConfig, job_config_from_definition, load_file, trigger_from_definition,
};
use crate::scheduler::{SchedulerCommand, SchedulerLoop};
use crate::store::DynStore;

/// Summary of how a reload will (or would) affect the scheduler state.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ReloadDiff {
    /// Job keys that will be added (present after reload, not before).
    pub added: Vec<String>,
    /// Job keys that will be removed (present before reload, not after).
    pub removed: Vec<String>,
    /// Job keys that exist in both, but with changed scheduling-relevant config.
    pub changed: Vec<String>,
    /// Total job count after the reload would apply.
    pub total: usize,
}

impl ReloadDiff {
    pub fn is_noop(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// A validated reload ready to apply.
pub struct ReloadPlan {
    /// DSL-only jobs (for `dsl_jobs_shared` update on apply).
    pub dsl_jobs: Vec<JobConfig>,
    /// Merged jobs: DSL + API-registered (DSL wins on conflict).
    pub merged_jobs: Vec<JobConfig>,
    /// Merged triggers: DSL + API-registered (DSL wins on conflict).
    pub merged_triggers: HashMap<String, Trigger>,
    /// Summary of the difference vs. the running state.
    pub diff: ReloadDiff,
}

/// Reload failure modes — shaped so the admin handler can surface them as
/// structured JSON.
#[derive(Debug)]
pub enum ReloadError {
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    Validation {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    Store(String),
}

impl std::fmt::Display for ReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadFile { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::Validation {
                message,
                line: Some(l),
                column: Some(c),
            } => write!(f, "validation failed at line {l}, column {c}: {message}"),
            Self::Validation { message, .. } => write!(f, "validation failed: {message}"),
            Self::Store(e) => write!(f, "store error: {e}"),
        }
    }
}

impl std::error::Error for ReloadError {}

/// Build a reload plan from a Croniqfile path.
///
/// Validates everything fully and returns an applicable plan, but does not
/// mutate scheduler or store state. Failure at any step aborts — existing
/// state is unchanged.
pub async fn build_plan(
    path: &Path,
    store: &DynStore,
    current_triggers: &RwLock<HashMap<String, Trigger>>,
    current_dsl_jobs: &RwLock<Vec<JobConfig>>,
) -> Result<ReloadPlan, ReloadError> {
    let loaded: LoadedConfig = load_file(path).map_err(|e| match e {
        LoadError::Io(err) => ReloadError::ReadFile {
            path: path.to_path_buf(),
            source: err,
        },
        LoadError::Parse {
            message,
            line,
            column,
        } => ReloadError::Validation {
            message,
            line,
            column,
        },
        LoadError::Schedule { job, reason } => ReloadError::Validation {
            message: format!("schedule error in job '{job}': {reason}"),
            line: None,
            column: None,
        },
    })?;

    let now = Utc::now();
    let dsl_keys: HashSet<String> = loaded.runtime.jobs.iter().map(|j| j.key.clone()).collect();

    let mut merged_triggers = loaded.triggers.clone();
    let mut merged_jobs = loaded.runtime.jobs.clone();

    let api_triggers = store
        .list_triggers(None)
        .map_err(|e| ReloadError::Store(format!("{e}")))?;
    for def in &api_triggers {
        if def.managed_by == "dsl" || !def.enabled {
            continue;
        }
        if dsl_keys.contains(&def.job_key) {
            // DSL precedence — drop API trigger for this key.
            continue;
        }
        if let Some(trigger) = trigger_from_definition(def, now) {
            let job_config = job_config_from_definition(def, None);
            merged_jobs.push(job_config);
            merged_triggers.insert(def.job_key.clone(), trigger);
        }
    }

    // Diff against the current scheduler state.
    let diff = {
        let current = current_triggers.read().await;
        let current_dsl = current_dsl_jobs.read().await;

        let current_keys: HashSet<String> = current.keys().cloned().collect();
        let merged_keys: HashSet<String> = merged_triggers.keys().cloned().collect();

        let mut added: Vec<String> = merged_keys.difference(&current_keys).cloned().collect();
        added.sort();

        let mut removed: Vec<String> = current_keys.difference(&merged_keys).cloned().collect();
        removed.sort();

        let current_dsl_by_key: HashMap<&str, &JobConfig> =
            current_dsl.iter().map(|j| (j.key.as_str(), j)).collect();
        let mut changed: Vec<String> = Vec::new();
        for new_job in &merged_jobs {
            if let Some(old_job) = current_dsl_by_key.get(new_job.key.as_str())
                && job_changed(old_job, new_job)
            {
                changed.push(new_job.key.clone());
            }
        }
        changed.sort();

        ReloadDiff {
            added,
            removed,
            changed,
            total: merged_triggers.len(),
        }
    };

    Ok(ReloadPlan {
        dsl_jobs: loaded.runtime.jobs,
        merged_jobs,
        merged_triggers,
        diff,
    })
}

/// Structural check for scheduling-relevant changes. Intentionally brittle —
/// covers the fields that actually alter runtime behaviour.
fn job_changed(a: &JobConfig, b: &JobConfig) -> bool {
    a.schedule_summary != b.schedule_summary
        || a.timezone != b.timezone
        || a.timeout != b.timeout
        || a.calendar != b.calendar
        || a.window != b.window
        || a.not_before != b.not_before
        || a.not_after != b.not_after
        || a.retry.max_attempts != b.retry.max_attempts
        || a.execution_mode != b.execution_mode
        || a.catch_up != b.catch_up
        || a.dead_letter.enabled != b.dead_letter.enabled
}

/// Apply a validated reload plan from outside the scheduler task.
///
/// Sends a `SchedulerCommand::Reload` and waits for the scheduler to ack.
/// After the ack, updates the shared DSL job list and trigger snapshot so
/// subsequent API reads and reload diffs see the new state.
pub async fn apply_plan(
    plan: ReloadPlan,
    scheduler_tx: &mpsc::UnboundedSender<SchedulerCommand>,
    dsl_jobs_shared: &RwLock<Vec<JobConfig>>,
    trigger_snapshot: &RwLock<HashMap<String, Trigger>>,
) -> Result<(), ApplyError> {
    let post_triggers = plan.merged_triggers.clone();
    let post_dsl = plan.dsl_jobs;

    let (ack_tx, ack_rx) = oneshot::channel();
    scheduler_tx
        .send(SchedulerCommand::Reload {
            triggers: plan.merged_triggers,
            jobs: plan.merged_jobs,
            ack: ack_tx,
        })
        .map_err(|_| ApplyError::SchedulerDown)?;

    ack_rx.await.map_err(|_| ApplyError::SchedulerDown)?;

    *dsl_jobs_shared.write().await = post_dsl;
    *trigger_snapshot.write().await = post_triggers;
    Ok(())
}

/// Apply a reload plan directly from inside the scheduler task.
///
/// Use this from the scheduler select-loop when you already own `&mut
/// SchedulerLoop` — sending a command + awaiting the ack from within the
/// scheduler task itself would deadlock.
pub async fn apply_plan_direct(
    plan: ReloadPlan,
    scheduler: &mut SchedulerLoop,
    dsl_jobs_shared: &RwLock<Vec<JobConfig>>,
    trigger_snapshot: &RwLock<HashMap<String, Trigger>>,
) {
    let post_triggers = plan.merged_triggers.clone();
    scheduler.reload(plan.merged_triggers, plan.merged_jobs);
    *dsl_jobs_shared.write().await = plan.dsl_jobs;
    *trigger_snapshot.write().await = post_triggers;
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("scheduler task is not running")]
    SchedulerDown,
}

/// Atomic counters for `croniq_config_reload_total`.
#[derive(Debug, Default)]
pub struct ReloadCounters {
    pub success: AtomicU64,
    pub validation_error: AtomicU64,
    pub apply_error: AtomicU64,
}

impl ReloadCounters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn inc_success(&self) {
        self.success.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_validation_error(&self) {
        self.validation_error.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_apply_error(&self) {
        self.apply_error.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_str;
    use crate::store::sqlite_store;
    use croniq_store::sqlite::SqliteStore;

    fn empty_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    async fn state_from(src: &str) -> (RwLock<HashMap<String, Trigger>>, RwLock<Vec<JobConfig>>) {
        let loaded = load_str(src).unwrap();
        (
            RwLock::new(loaded.triggers),
            RwLock::new(loaded.runtime.jobs),
        )
    }

    #[tokio::test]
    async fn diff_detects_added_and_removed() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;

        // Write new config to a temp file.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job b:two { every 1 hours }").unwrap();

        let store = empty_store();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert_eq!(plan.diff.added, vec!["b:two"]);
        assert_eq!(plan.diff.removed, vec!["a:one"]);
        assert!(plan.diff.changed.is_empty());
        assert_eq!(plan.diff.total, 1);
    }

    #[tokio::test]
    async fn diff_detects_schedule_change() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job a:one { every 30 minutes }").unwrap();

        let store = empty_store();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert!(plan.diff.added.is_empty());
        assert!(plan.diff.removed.is_empty());
        assert_eq!(plan.diff.changed, vec!["a:one"]);
    }

    #[tokio::test]
    async fn diff_noop_when_identical() {
        let src = "job a:one { every 1 hours }";
        let (cur_tr, cur_dsl) = state_from(src).await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), src).unwrap();

        let store = empty_store();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert!(plan.diff.is_noop());
    }

    #[tokio::test]
    async fn invalid_file_returns_validation_error_with_line_col() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        // Intentional garbage on line 2.
        std::fs::write(tmp.path(), "\n@@@not valid DSL@@@\n").unwrap();

        let store = empty_store();
        let err = match build_plan(tmp.path(), &store, &cur_tr, &cur_dsl).await {
            Ok(_) => panic!("expected validation error"),
            Err(e) => e,
        };

        match err {
            ReloadError::Validation { line, column, .. } => {
                assert!(line.is_some(), "line should be populated from span");
                assert!(column.is_some(), "column should be populated from span");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_file_returns_read_error() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;
        let store = empty_store();
        let err = match build_plan(
            Path::new("this-file-does-not-exist-42.croniq"),
            &store,
            &cur_tr,
            &cur_dsl,
        )
        .await
        {
            Ok(_) => panic!("expected read error"),
            Err(e) => e,
        };
        assert!(matches!(err, ReloadError::ReadFile { .. }));
    }

    #[tokio::test]
    async fn api_triggers_merged_when_not_in_dsl() {
        let (cur_tr, cur_dsl) = state_from("job dsl:only { every 1 hours }").await;

        // Seed an API-registered trigger in the store.
        let store = empty_store();
        store
            .create_trigger(&croniq_store::models::TriggerDefinition {
                trigger_id: "api-1".into(),
                job_key: "api:kept".into(),
                cron_expression: Some("5m".into()),
                timezone: None,
                calendar: None,
                window: None,
                not_before: None,
                not_after: None,
                enabled: true,
                managed_by: "api".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        // New DSL drops the old DSL job.
        std::fs::write(tmp.path(), "job dsl:new { every 2 hours }").unwrap();

        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        let keys: HashSet<&str> = plan.merged_triggers.keys().map(|s| s.as_str()).collect();
        assert!(keys.contains("dsl:new"), "DSL job survives");
        assert!(
            keys.contains("api:kept"),
            "API-registered job survives reload"
        );
        assert!(!keys.contains("dsl:only"), "old DSL job is removed");
    }

    #[tokio::test]
    async fn dsl_wins_on_conflict_with_api_trigger() {
        let (cur_tr, cur_dsl) = state_from("job shared:key { every 1 hours }").await;

        let store = empty_store();
        store
            .create_trigger(&croniq_store::models::TriggerDefinition {
                trigger_id: "api-1".into(),
                job_key: "shared:key".into(),
                cron_expression: Some("5m".into()),
                timezone: None,
                calendar: None,
                window: None,
                not_before: None,
                not_after: None,
                enabled: true,
                managed_by: "api".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job shared:key { every 30 minutes }").unwrap();

        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        // Only one entry for the key, and its summary is the DSL's.
        let job = plan
            .merged_jobs
            .iter()
            .find(|j| j.key == "shared:key")
            .unwrap();
        assert_eq!(job.schedule_summary, "every 30 minutes");
        assert_eq!(
            plan.merged_jobs
                .iter()
                .filter(|j| j.key == "shared:key")
                .count(),
            1
        );
    }
}
