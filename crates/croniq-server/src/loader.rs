//! Croniqfile loader: parses, validates, and compiles the operator's configuration.
//!
//! Produces:
//! - A `RuntimeConfig` with fully resolved job definitions
//! - A map of job key → `Trigger` (ready-to-tick scheduler state machines)

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use croniq_config::ast::{Croniqfile, Item};
use croniq_config::compile::{self, RuntimeConfig};
use croniq_config::import::resolve_imports_with_visited;
use croniq_config::parser::Parser;
use croniq_scheduler::{
    calendar::Calendar,
    misfire::MisfirePolicy,
    schedule::Schedule,
    trigger::{TimeWindow, Trigger, TriggerState},
};
use croniq_config::compile::JobConfig;
use croniq_store::{
    models::JobStatus,
    traits::{ExecutionStore, JobStore},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("schedule error in job '{job}': {reason}")]
    Schedule { job: String, reason: String },
}

/// The fully loaded configuration: compiled config + live triggers.
pub struct LoadedConfig {
    pub runtime: RuntimeConfig,
    /// One trigger per job that has an active schedule.
    pub triggers: HashMap<String, Trigger>,
}

/// Load a Croniqfile from a file path, resolving `import` directives recursively.
pub fn load_file(path: &Path) -> Result<LoadedConfig, LoadError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut visited = HashSet::new();
    visited.insert(canonical.clone());

    let ast = load_and_resolve(&canonical, &mut visited)?;
    let runtime = compile::compile(&ast);
    load_from_compiled(runtime, &ast)
}

/// Load and parse a single file, then recursively resolve its imports.
fn load_and_resolve(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<Croniqfile, LoadError> {
    let src = std::fs::read_to_string(path)?;
    let mut ast = Parser::parse(&src).map_err(|e| LoadError::Parse(format!("{e}")))?;

    let base_dir = path.parent().unwrap_or(Path::new("."));

    // Collect imports, replace them with the imported items
    let mut resolved_items = Vec::new();
    for item in std::mem::take(&mut ast.items) {
        if let Item::Import(ref imp) = item {
            let import_path = &imp.path.value;
            match resolve_imports_with_visited(base_dir, import_path, visited) {
                Ok(paths) => {
                    for import_file in paths {
                        match load_and_resolve(&import_file, visited) {
                            Ok(imported_ast) => {
                                resolved_items.extend(imported_ast.items);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    file = %import_file.display(),
                                    error = %e,
                                    "failed to load imported file — skipping"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        import = %import_path,
                        error = %e,
                        "failed to resolve import — skipping"
                    );
                }
            }
        } else {
            resolved_items.push(item);
        }
    }
    ast.items = resolved_items;

    Ok(ast)
}

/// Load a Croniqfile from a source string (no import resolution).
pub fn load_str(src: &str) -> Result<LoadedConfig, LoadError> {
    let ast = Parser::parse(src).map_err(|e| LoadError::Parse(format!("{e}")))?;
    let runtime = compile::compile(&ast);
    load_from_compiled(runtime, &ast)
}

/// Build a LoadedConfig from a compiled RuntimeConfig and its AST.
fn load_from_compiled(runtime: RuntimeConfig, ast: &Croniqfile) -> Result<LoadedConfig, LoadError> {
    let now = Utc::now();

    // Build calendars from compiled config
    let calendars: HashMap<String, Calendar> = runtime
        .calendars
        .iter()
        .filter_map(|cfg| {
            match Calendar::from_config(cfg) {
                Ok(cal) => Some((cfg.name.clone(), cal)),
                Err(e) => {
                    tracing::warn!(calendar = %cfg.name, error = %e, "failed to compile calendar — skipping");
                    None
                }
            }
        })
        .collect();

    let mut triggers = HashMap::new();

    // Match AST jobs to compiled jobs to extract the ScheduleKind
    let ast_jobs: Vec<&croniq_config::ast::JobBlock> = ast
        .items
        .iter()
        .filter_map(|item| {
            if let croniq_config::ast::Item::Job(j) = item {
                Some(j)
            } else {
                None
            }
        })
        .collect();

    for job_cfg in &runtime.jobs {
        // Find the matching AST job
        let ast_job = ast_jobs
            .iter()
            .find(|j| j.key.raw == job_cfg.key)
            .expect("compiled job must have matching AST job");

        // Build the runtime Schedule from the AST ScheduleKind
        let schedule = match &ast_job.schedule {
            None => Schedule::Disabled,
            Some(s) => Schedule::from_ast(&s.kind).map_err(|e| LoadError::Schedule {
                job: job_cfg.key.clone(),
                reason: e.to_string(),
            })?,
        };

        // Resolve timezone (default UTC)
        let tz: chrono_tz::Tz = job_cfg
            .timezone
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(chrono_tz::UTC);

        // Misfire policy: default FireNow (never skip a billing run)
        let misfire = MisfirePolicy::FireNow;

        // Resolve calendar reference
        let calendar = job_cfg
            .calendar
            .as_deref()
            .and_then(|name| calendars.get(name).cloned());

        // Parse time window constraint (e.g. "08:00..18:00")
        let window = job_cfg.window.as_deref().and_then(TimeWindow::parse);

        // Parse not_before / not_after bounds
        let not_before = job_cfg
            .not_before
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc)));
        let not_after = job_cfg
            .not_after
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc)));

        let trigger = Trigger::with_bounds(
            job_cfg.key.clone(),
            schedule,
            tz,
            calendar,
            window,
            misfire,
            not_before,
            not_after,
            now,
        );

        triggers.insert(job_cfg.key.clone(), trigger);
    }

    Ok(LoadedConfig { runtime, triggers })
}

/// Restore persisted trigger state after a restart (or hot-reload).
///
/// Must be called **after** both `load_str`/`load_file` and the SQLite store
/// are available. Mutates the trigger map in-place.
///
/// Rules applied per job:
/// - `Exhausted` in DB  → trigger set to `TriggerState::Exhausted` and
///   `next_fire_at` cleared. This prevents `once`-jobs (and any scheduler-
///   exhausted trigger) from re-firing on restart.
/// - `Active` in DB     → `next_fire_at` restored from the stored value so
///   the next tick fires at the correct time instead of re-computing from now.
/// - `Paused`/`Disabled`/unknown → no change (trigger stays as loaded).
pub fn restore_trigger_states(
    triggers: &mut HashMap<String, Trigger>,
    store: &dyn JobStore,
    _now: DateTime<Utc>,
) {
    let states = match store.list_job_states() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "could not load job states for trigger restore");
            return;
        }
    };

    for job_state in states {
        let Some(trigger) = triggers.get_mut(&job_state.job_key) else {
            continue; // job removed from config since last run — ignore
        };

        match job_state.status {
            JobStatus::Exhausted => {
                // once-job already fired — never re-arm
                trigger.state = TriggerState::Exhausted;
                trigger.next_fire_at = None;
                tracing::debug!(
                    job_key = %job_state.job_key,
                    "trigger restore: exhausted (once-job already ran)"
                );
            }
            JobStatus::Active => {
                // Restore the stored next_fire_at so the trigger doesn't skip
                // or double-fire due to restart timing differences.
                if job_state.next_fire_at.is_some() {
                    trigger.next_fire_at = job_state.next_fire_at;
                    trigger.fire_count = job_state.fire_count;
                    tracing::debug!(
                        job_key = %job_state.job_key,
                        next_fire_at = ?job_state.next_fire_at,
                        "trigger restore: next_fire_at restored"
                    );
                }
            }
            JobStatus::Paused | JobStatus::Disabled => {
                // DSL intent already reflected in the trigger — nothing to do
            }
        }
    }
}

/// Restore queued executions from the database into the in-memory work queue.
///
/// On server restart, executions in `queued` state need to be re-enqueued so
/// runners can pick them up. Uses `job_to_work_item` to reconstruct the work
/// item from the execution record + job config.
pub async fn restore_queued_executions(
    store: &dyn ExecutionStore,
    jobs: &[JobConfig],
    runner_state: &croniq_runner::AppState,
) -> usize {
    use croniq_bridge::job_to_work_item;

    let executions = match store.find_queued_executions(&[], 1000) {
        Ok(execs) => execs,
        Err(e) => {
            tracing::warn!(error = %e, "could not load queued executions for restore");
            return 0;
        }
    };

    let job_map: HashMap<&str, &JobConfig> = jobs.iter().map(|j| (j.key.as_str(), j)).collect();
    let mut restored = 0;

    for exec in &executions {
        if let Some(job) = job_map.get(exec.job_key.as_str()) {
            let item = job_to_work_item(job, exec.id.to_string(), exec.fire_at, exec.attempt);
            runner_state.queue.write().await.enqueue(item);
            restored += 1;
        } else {
            tracing::warn!(
                job_key = %exec.job_key,
                execution_id = %exec.id,
                "queued execution for unknown job — skipping restore"
            );
        }
    }

    if restored > 0 {
        runner_state.work_notify.notify_waiters();
    }

    restored
}

/// Build a runtime Trigger from a persisted TriggerDefinition.
///
/// Used for API/runner-registered jobs that don't come from the Croniqfile.
/// Returns None if the schedule can't be parsed.
pub fn trigger_from_definition(
    def: &croniq_store::models::TriggerDefinition,
    now: DateTime<Utc>,
) -> Option<Trigger> {
    let cron_expr = def.cron_expression.as_deref()?;

    // Parse schedule expression: interval shorthand ("5m", "300", "*/5 * * * *")
    let secs = parse_interval_seconds(cron_expr)?;
    let schedule = croniq_scheduler::schedule::Schedule::Interval { seconds: secs };

    let tz: chrono_tz::Tz = def.timezone.as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);

    let window = def.window.as_deref().and_then(TimeWindow::parse);

    let not_before = def.not_before;
    let not_after = def.not_after;

    Some(Trigger::with_bounds(
        def.job_key.clone(),
        schedule,
        tz,
        None, // calendar resolved separately
        window,
        MisfirePolicy::FireNow,
        not_before,
        not_after,
        now,
    ))
}

/// Build a minimal JobConfig from a persisted trigger definition.
pub fn job_config_from_definition(
    def: &croniq_store::models::TriggerDefinition,
    job_def: Option<&croniq_store::models::JobDefinition>,
) -> croniq_config::compile::JobConfig {
    use croniq_config::compile::*;
    use croniq_config::schedule::CompiledSchedule;

    let (ns, name) = def.job_key.split_once(':').unwrap_or(("default", &def.job_key));

    JobConfig {
        key: def.job_key.clone(),
        namespace: ns.to_string(),
        name: name.to_string(),
        variant: None,
        description: job_def.and_then(|j| j.description.clone()),
        schedule: CompiledSchedule::Disabled,
        schedule_summary: def.cron_expression.clone().unwrap_or_default(),
        timezone: def.timezone.clone(),
        calendar: def.calendar.clone(),
        window: def.window.clone(),
        not_before: def.not_before.map(|d| d.to_rfc3339()),
        not_after: def.not_after.map(|d| d.to_rfc3339()),
        runner: RunnerConfig::default(),
        retry: RetryConfig::default(),
        timeout: Some("5m".into()),
        dead_letter: DeadLetterConfig::default(),
        metadata: job_def.map(|j| j.metadata.clone()).unwrap_or_default(),
    }
}

fn parse_interval_seconds(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Ok(n) = s.parse::<u64>() {
        return Some(n);
    }
    if let Some(n) = s.strip_suffix('s') {
        return n.parse().ok();
    }
    if let Some(n) = s.strip_suffix('m') {
        return n.parse::<u64>().ok().map(|v| v * 60);
    }
    if let Some(n) = s.strip_suffix('h') {
        return n.parse::<u64>().ok().map(|v| v * 3600);
    }
    // Try parsing as cron-like interval: */N * * * * → every N minutes
    if let Some(rest) = s.strip_prefix("*/")
        && let Some(n) = rest.split_whitespace().next().and_then(|n| n.parse::<u64>().ok()) {
            return Some(n * 60);
        }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use croniq_scheduler::trigger::TriggerState;
    use croniq_store::sqlite::SqliteStore;
    use pretty_assertions::assert_eq;

    #[test]
    fn load_minimal_croniqfile() {
        let src = r#"
            job etl:sync {
                every 15 minutes
                timeout 10m
            }
        "#;
        let cfg = load_str(src).unwrap();
        assert_eq!(cfg.runtime.jobs.len(), 1);
        assert_eq!(cfg.runtime.jobs[0].key, "etl:sync");
        assert!(cfg.triggers.contains_key("etl:sync"));
    }

    #[test]
    fn disabled_job_creates_paused_trigger() {
        let src = r#"
            job reports:monthly {
                disabled
            }
        "#;
        let cfg = load_str(src).unwrap();
        let trigger = &cfg.triggers["reports:monthly"];
        assert_eq!(trigger.state, TriggerState::Paused);
    }

    #[test]
    fn multiple_jobs_all_have_triggers() {
        let src = r#"
            job billing:invoice { every day at 02:00 }
            job etl:sync { every 1 hours }
            job reports:weekly { every monday at 06:00 }
        "#;
        let cfg = load_str(src).unwrap();
        assert_eq!(cfg.runtime.jobs.len(), 3);
        assert_eq!(cfg.triggers.len(), 3);
    }

    #[test]
    fn timezone_defaults_to_utc() {
        let src = r#"
            job billing:invoice {
                every day at 02:00
            }
        "#;
        let cfg = load_str(src).unwrap();
        // No timezone specified → trigger uses UTC
        let trigger = &cfg.triggers["billing:invoice"];
        assert_eq!(trigger.timezone, chrono_tz::UTC);
    }

    #[test]
    fn timezone_from_defaults_block() {
        let src = r#"
            defaults { timezone Europe/Vienna }
            job billing:invoice { every day at 02:00 }
        "#;
        let cfg = load_str(src).unwrap();
        // Defaults timezone is compiled into the job config
        assert_eq!(cfg.runtime.jobs[0].timezone.as_deref(), Some("Europe/Vienna"));
        // And the trigger uses it
        let trigger = &cfg.triggers["billing:invoice"];
        assert_eq!(trigger.timezone, chrono_tz::Europe::Vienna);
    }

    #[test]
    fn invalid_croniqfile_returns_parse_error() {
        let src = r#"this is not valid DSL @@###"#;
        assert!(matches!(load_str(src), Err(LoadError::Parse(_))));
    }

    #[test]
    fn job_metadata_compiled() {
        let src = r#"
            job billing:invoice {
                every day at 02:00
                metadata { env prod; region eu }
            }
        "#;
        let cfg = load_str(src).unwrap();
        let job = &cfg.runtime.jobs[0];
        assert_eq!(job.metadata.get("env").map(|s| s.as_str()), Some("prod"));
        assert_eq!(job.metadata.get("region").map(|s| s.as_str()), Some("eu"));
    }

    // ─── restore_trigger_states ───────────────────────────────────────────────

    fn make_store() -> std::sync::Arc<SqliteStore> {
        std::sync::Arc::new(SqliteStore::in_memory().unwrap())
    }

    fn seed_job_state(
        store: &SqliteStore,
        key: &str,
        status: croniq_store::models::JobStatus,
        next_fire_at: Option<chrono::DateTime<Utc>>,
        fire_count: u64,
    ) {
        store
            .upsert_job_state(&croniq_store::models::JobState {
                job_key: key.into(),
                next_fire_at,
                last_fired_at: None,
                fire_count,
                status,
                updated_at: Utc::now(),
            })
            .unwrap();
    }

    #[test]
    fn restore_exhausted_disarms_once_trigger() {
        // Load a once-job (disabled schedule is fine for this test — we
        // care about the restore step, not the DSL parsing of 'once')
        let src = r#"job migration:v2 { disabled }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        // Simulate: this once-job already fired in a previous run
        seed_job_state(&store, "migration:v2", croniq_store::models::JobStatus::Exhausted, None, 1);

        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());

        let trigger = &cfg.triggers["migration:v2"];
        assert_eq!(trigger.state, TriggerState::Exhausted);
        assert!(trigger.next_fire_at.is_none());
    }

    #[test]
    fn restore_active_restores_next_fire_at() {
        let src = r#"job etl:sync { every 1 hours }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        let stored_next = Utc::now() + chrono::Duration::minutes(42);
        seed_job_state(
            &store,
            "etl:sync",
            croniq_store::models::JobStatus::Active,
            Some(stored_next),
            7,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());

        let trigger = &cfg.triggers["etl:sync"];
        // next_fire_at must be restored from DB (not re-computed from now)
        assert_eq!(trigger.next_fire_at.unwrap().timestamp(), stored_next.timestamp());
        assert_eq!(trigger.fire_count, 7);
    }

    #[test]
    fn restore_unknown_job_is_ignored() {
        let src = r#"job etl:sync { every 1 hours }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        // Store has a stale job_state for a job that no longer exists in config
        seed_job_state(&store, "removed:job", croniq_store::models::JobStatus::Exhausted, None, 1);

        // Should not panic; the etl:sync trigger is unaffected
        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());
        assert!(cfg.triggers.contains_key("etl:sync"));
    }

    #[test]
    fn restore_paused_does_not_change_trigger() {
        let src = r#"job reports:monthly { disabled }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        seed_job_state(&store, "reports:monthly", croniq_store::models::JobStatus::Paused, None, 0);

        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());

        // Disabled DSL → Paused trigger; Paused status in DB → no override
        let trigger = &cfg.triggers["reports:monthly"];
        assert_eq!(trigger.state, TriggerState::Paused);
    }
}
