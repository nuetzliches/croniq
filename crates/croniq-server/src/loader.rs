//! Croniqfile loader: parses, validates, and compiles the operator's configuration.
//!
//! Produces:
//! - A `RuntimeConfig` with fully resolved job definitions
//! - A map of job key → `Trigger` (ready-to-tick scheduler state machines)

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use croniq_config::ast::{Croniqfile, Item};
use croniq_config::compile::JobConfig;
use croniq_config::compile::{self, CatchUpPolicy, RuntimeConfig};
use croniq_config::import::resolve_imports_with_visited;
use croniq_config::parser::Parser;
use croniq_scheduler::{
    calendar::Calendar,
    misfire::MisfirePolicy,
    schedule::Schedule,
    trigger::{TimeWindow, Trigger, TriggerState},
};
use croniq_store::{
    models::JobStatus,
    traits::{ExecutionStore, JobStore},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {message}")]
    Parse {
        message: String,
        /// 1-based line number of the offending token (if known).
        line: Option<usize>,
        /// 1-based column of the offending token (if known).
        column: Option<usize>,
    },

    #[error("schedule error in job '{job}': {reason}")]
    Schedule { job: String, reason: String },
}

/// Convert a `ParseError` into a `LoadError::Parse` with 1-based line/column
/// extracted from the parser's SourceSpan when available.
fn parse_error_to_load(err: croniq_config::parser::ParseError, source: &str) -> LoadError {
    use croniq_config::lexer::LexError;
    use croniq_config::parser::ParseError;

    let span = match &err {
        ParseError::General { span, .. }
        | ParseError::Unexpected { span, .. }
        | ParseError::InvalidJobKey { span, .. }
        | ParseError::InvalidTime { span, .. }
        | ParseError::InvalidOrdinal { span, .. } => Some(*span),
        ParseError::Lex(lex) => match lex {
            LexError::UnterminatedString { span }
            | LexError::UnterminatedPlaceholder { span }
            | LexError::InvalidEscape { span, .. } => Some(*span),
        },
    };

    let (line, column) = match span {
        Some(s) => {
            let (l, c) = line_col(source, s.offset());
            (Some(l), Some(c))
        }
        None => (None, None),
    };

    LoadError::Parse {
        message: format!("{err}"),
        line,
        column,
    }
}

/// Compute 1-based (line, column) for a byte offset into `source`.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line: usize = 1;
    let mut col: usize = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line = line.saturating_add(1);
            col = 1;
        } else {
            col = col.saturating_add(1);
        }
    }
    (line, col)
}

/// The fully loaded configuration: compiled config + live triggers.
pub struct LoadedConfig {
    pub runtime: RuntimeConfig,
    /// One trigger per job that has an active schedule.
    pub triggers: HashMap<String, Trigger>,
    /// Jobs whose `calendar` reference did not resolve at load time (the
    /// calendar failed to compile, or no calendar with that name is defined).
    /// Keyed by job key, value is a human-readable reason. Under the default
    /// `strict_calendars` policy these jobs are loaded **paused** (fail closed,
    /// issue #361); the map is surfaced through `ServerState.config_faults`.
    /// Empty when `policy { strict_calendars false }` restores legacy behavior.
    pub calendar_faults: HashMap<String, String>,
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
fn load_and_resolve(path: &Path, visited: &mut HashSet<PathBuf>) -> Result<Croniqfile, LoadError> {
    let src = std::fs::read_to_string(path)?;
    let mut ast = Parser::parse(&src).map_err(|e| parse_error_to_load(e, &src))?;

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
    let ast = Parser::parse(src).map_err(|e| parse_error_to_load(e, src))?;
    let runtime = compile::compile(&ast);
    load_from_compiled(runtime, &ast)
}

/// Build a LoadedConfig from a compiled RuntimeConfig and its AST.
fn load_from_compiled(runtime: RuntimeConfig, ast: &Croniqfile) -> Result<LoadedConfig, LoadError> {
    let now = Utc::now();

    let strict_calendars = runtime.policy.strict_calendars;

    // Build calendars from compiled config. Calendars that fail to compile are
    // dropped from `calendars` but remembered in `calendar_errors` so a job
    // that references one can be failed closed (paused) instead of silently
    // un-gated (issue #361).
    let mut calendars: HashMap<String, Calendar> = HashMap::new();
    let mut calendar_errors: HashMap<String, String> = HashMap::new();
    for cfg in &runtime.calendars {
        match Calendar::from_config(cfg) {
            Ok(cal) => {
                calendars.insert(cfg.name.clone(), cal);
            }
            Err(e) => {
                // A referenced-but-broken calendar is escalated to ERROR at the
                // point of reference below; log the compile failure itself at
                // WARN here (an unreferenced broken calendar stays a warning).
                tracing::warn!(calendar = %cfg.name, error = %e, "failed to compile calendar — skipping");
                calendar_errors.insert(cfg.name.clone(), e.to_string());
            }
        }
    }

    let mut triggers = HashMap::new();
    let mut calendar_faults: HashMap<String, String> = HashMap::new();

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

        // Resolve calendar reference. A reference that does not resolve (the
        // calendar failed to compile, or is not defined) is a fault: under the
        // default `strict_calendars` policy the job is failed closed (paused)
        // rather than fired without its gate (issue #361).
        let mut calendar_fault: Option<String> = None;
        let calendar = match job_cfg.calendar.as_deref() {
            None => None,
            Some(name) => match calendars.get(name) {
                Some(cal) => Some(cal.clone()),
                None => {
                    let reason = match calendar_errors.get(name) {
                        Some(err) => {
                            format!("calendar '{name}' failed to compile: {err}")
                        }
                        None => format!("calendar '{name}' is not defined"),
                    };
                    if strict_calendars {
                        tracing::error!(
                            job = %job_cfg.key,
                            calendar = %name,
                            reason = %reason,
                            "job paused: referenced calendar did not resolve (strict_calendars)"
                        );
                        calendar_fault = Some(reason);
                    } else {
                        tracing::warn!(
                            job = %job_cfg.key,
                            calendar = %name,
                            reason = %reason,
                            "job loaded without its calendar gate (strict_calendars disabled)"
                        );
                    }
                    None
                }
            },
        };

        // Parse time window constraint (e.g. "08:00..18:00")
        let window = job_cfg.window.as_deref().and_then(TimeWindow::parse);

        // Parse not_before / not_after bounds
        let not_before = job_cfg.not_before.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });
        let not_after = job_cfg.not_after.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        let mut trigger = Trigger::with_bounds(
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

        // Fail closed: a job with an unresolved calendar reference is paused so
        // it cannot fire un-gated. `Trigger::evaluate` gates on `state == Armed`,
        // so pausing suppresses firing regardless of `next_fire_at`. Triggers are
        // rebuilt on every load, so fixing the calendar self-heals on reload.
        if let Some(reason) = calendar_fault {
            trigger.pause();
            calendar_faults.insert(job_cfg.key.clone(), reason);
        }

        triggers.insert(job_cfg.key.clone(), trigger);
    }

    Ok(LoadedConfig {
        runtime,
        triggers,
        calendar_faults,
    })
}

/// Restore persisted trigger state after a restart (or hot-reload).
///
/// Must be called **after** both `load_str`/`load_file` and the SQLite store
/// are available. Mutates the trigger map in-place.
///
/// Rules applied per job:
/// - `Exhausted` in DB  → terminal **only for non-recurring schedules**
///   (`once` jobs and `disabled`): trigger set to `TriggerState::Exhausted`
///   and `next_fire_at` cleared so a once-job doesn't re-fire on restart.
///   A *recurring* schedule persisted as `Exhausted` is treated as a
///   recoverable fault (e.g. the DST spring-forward gap in #249, where
///   `next_fire_after` used to return `None`): it is re-armed by recomputing
///   `next_fire_at` from `now`, unless its `not_after` bound has passed.
/// - `Active` in DB     → `next_fire_at` restored from the stored value so
///   the next tick fires at the correct time instead of re-computing from now.
/// - `Paused`/`Disabled`/unknown → no change (trigger stays as loaded).
pub fn restore_trigger_states(
    triggers: &mut HashMap<String, Trigger>,
    store: &dyn JobStore,
    now: DateTime<Utc>,
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
                // A `once` job that already fired, or a `disabled` schedule,
                // is legitimately terminal — never re-arm it. A recurring
                // schedule, on the other hand, has no business being
                // permanently exhausted: if it got there it was a fault
                // (historically the DST spring-forward gap, #249), so recover
                // by recomputing the next fire from now.
                let recurring =
                    !matches!(trigger.schedule, Schedule::Once { .. } | Schedule::Disabled);
                let past_not_after = trigger.not_after.map(|na| now > na).unwrap_or(false);

                if recurring && !past_not_after {
                    trigger.fire_count = job_state.fire_count;
                    trigger.resume(now); // recompute next_fire_at, re-arm
                    tracing::warn!(
                        job_key = %job_state.job_key,
                        next_fire_at = ?trigger.next_fire_at,
                        "trigger restore: re-armed recurring trigger persisted as exhausted (recovery, see #249)"
                    );
                } else {
                    trigger.state = TriggerState::Exhausted;
                    trigger.next_fire_at = None;
                    tracing::debug!(
                        job_key = %job_state.job_key,
                        "trigger restore: exhausted (once-job already ran or past not_after)"
                    );
                }
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
/// runners can pick them up. Respects each job's `catch_up` and `execution_mode`
/// policies:
///
/// - `execution_mode: ephemeral` → cancel all queued executions (no catch-up)
/// - `catch_up: none`            → cancel all queued executions for this job
/// - `catch_up: latest`          → keep only the most recent queued execution,
///   cancel the rest
/// - `catch_up: all`             → restore all queued executions (default)
pub async fn restore_queued_executions(
    store: &dyn ExecutionStore,
    jobs: &[JobConfig],
    runner_state: &croniq_runner::AppState,
) -> usize {
    use croniq_bridge::job_to_work_item;
    use croniq_config::compile::ExecutionMode;

    let executions = match store.find_queued_executions(&[], 1000) {
        Ok(execs) => execs,
        Err(e) => {
            tracing::warn!(error = %e, "could not load queued executions for restore");
            return 0;
        }
    };

    let job_map: HashMap<&str, &JobConfig> = jobs.iter().map(|j| (j.key.as_str(), j)).collect();
    let mut restored = 0;

    // Group executions by job_key to apply catch_up policy per job.
    let mut by_job: HashMap<String, Vec<&croniq_store::models::Execution>> = HashMap::new();
    for exec in &executions {
        by_job.entry(exec.job_key.clone()).or_default().push(exec);
    }

    for (job_key, mut execs) in by_job {
        let job = match job_map.get(job_key.as_str()) {
            Some(j) => j,
            None => {
                tracing::warn!(
                    job_key = %job_key,
                    count = execs.len(),
                    "queued executions for unknown job — skipping restore"
                );
                continue;
            }
        };

        // Ephemeral jobs never restore queued executions.
        if job.execution_mode == ExecutionMode::Ephemeral {
            let now = chrono::Utc::now();
            for exec in &execs {
                let _ = store.cancel_execution(exec.id, now);
            }
            tracing::debug!(
                job_key = %job_key,
                cancelled = execs.len(),
                "ephemeral job — cancelled queued executions on restore"
            );
            continue;
        }

        match job.catch_up {
            CatchUpPolicy::None => {
                // Cancel all queued executions — just move to next fire.
                let now = chrono::Utc::now();
                for exec in &execs {
                    let _ = store.cancel_execution(exec.id, now);
                }
                tracing::info!(
                    job_key = %job_key,
                    cancelled = execs.len(),
                    "catch_up=none — cancelled queued executions"
                );
            }
            CatchUpPolicy::Latest => {
                // Sort by fire_at desc, keep only the most recent one.
                execs.sort_by_key(|b| std::cmp::Reverse(b.fire_at));
                let now = chrono::Utc::now();
                for (i, exec) in execs.iter().enumerate() {
                    if i == 0 {
                        // Restore the latest
                        let item = job_to_work_item(
                            job,
                            exec.id.to_string(),
                            exec.fire_at,
                            exec.scheduled_for,
                            exec.attempt,
                        );
                        runner_state.queue.write().await.enqueue(item);
                        restored += 1;
                    } else {
                        // Cancel the rest
                        let _ = store.cancel_execution(exec.id, now);
                    }
                }
                if execs.len() > 1 {
                    tracing::info!(
                        job_key = %job_key,
                        restored = 1,
                        cancelled = execs.len() - 1,
                        "catch_up=latest — coalesced queued executions"
                    );
                }
            }
            CatchUpPolicy::All => {
                // Restore all (current behaviour).
                for exec in &execs {
                    let item = job_to_work_item(
                        job,
                        exec.id.to_string(),
                        exec.fire_at,
                        exec.scheduled_for,
                        exec.attempt,
                    );
                    runner_state.queue.write().await.enqueue(item);
                    restored += 1;
                }
            }
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

    let tz: chrono_tz::Tz = def
        .timezone
        .as_deref()
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

    let (ns, name) = def
        .job_key
        .split_once(':')
        .unwrap_or(("default", &def.job_key));

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
        retry: job_def
            .and_then(|j| j.max_retries)
            .map(|n| RetryConfig {
                max_attempts: n,
                ..RetryConfig::default()
            })
            .unwrap_or_default(),
        timeout: job_def
            .and_then(|j| j.timeout.clone())
            .or_else(|| Some("5m".into())),
        dead_letter: DeadLetterConfig {
            enabled: job_def.and_then(|j| j.dead_letter_enabled).unwrap_or(true),
            retention: job_def
                .and_then(|j| j.dead_letter_retention.clone())
                .or_else(|| DeadLetterConfig::default().retention),
            operator_hint: job_def.and_then(|j| j.dead_letter_operator_hint.clone()),
            replay_max_age: job_def.and_then(|j| j.dead_letter_replay_max_age.clone()),
        },
        metadata: job_def.map(|j| j.metadata.clone()).unwrap_or_default(),
        execution_mode: ExecutionMode::default(),
        catch_up: CatchUpPolicy::default(),
        queue_ttl: None,
        max_queue_depth: None,
        // API-registered jobs have no Croniqfile `keep_last` (v1 supports
        // per-job caps only for DSL jobs); the global `execution_retention`
        // age sweep still covers them.
        keep_last: None,
        // API-registered jobs carry a concurrency limit (if any) through
        // their metadata (`__max_concurrent`) rather than this DSL field.
        max_concurrent: None,
        tags: job_def.map(|j| j.tags.clone()).unwrap_or_default(),
    }
}

/// Build a minimal `JobConfig` from a store-persisted `JobDefinition` alone —
/// used as a fallback when an API-registered job has no Croniqfile entry.
/// Policy fields (`timeout`, `max_retries`, `dead_letter_enabled` and the
/// `dead_letter_*` policy columns, incl. the stale-replay guard
/// `dead_letter_replay_max_age`) are read from the `JobDefinition`;
/// everything else uses safe defaults.
pub fn job_config_from_job_def(
    job_def: &croniq_store::models::JobDefinition,
) -> croniq_config::compile::JobConfig {
    use croniq_config::compile::*;
    use croniq_config::schedule::CompiledSchedule;

    let (ns, name) = job_def
        .job_key
        .split_once(':')
        .unwrap_or(("default", &job_def.job_key));

    JobConfig {
        key: job_def.job_key.clone(),
        namespace: ns.to_string(),
        name: name.to_string(),
        variant: None,
        description: job_def.description.clone(),
        schedule: CompiledSchedule::Disabled,
        schedule_summary: "api".into(),
        timezone: None,
        calendar: None,
        window: None,
        not_before: None,
        not_after: None,
        runner: RunnerConfig::default(),
        retry: job_def
            .max_retries
            .map(|n| RetryConfig {
                max_attempts: n,
                ..RetryConfig::default()
            })
            .unwrap_or_default(),
        timeout: job_def.timeout.clone().or_else(|| Some("5m".into())),
        dead_letter: DeadLetterConfig {
            enabled: job_def.dead_letter_enabled.unwrap_or(true),
            retention: job_def
                .dead_letter_retention
                .clone()
                .or_else(|| DeadLetterConfig::default().retention),
            operator_hint: job_def.dead_letter_operator_hint.clone(),
            replay_max_age: job_def.dead_letter_replay_max_age.clone(),
        },
        metadata: job_def.metadata.clone(),
        execution_mode: ExecutionMode::default(),
        catch_up: CatchUpPolicy::default(),
        queue_ttl: None,
        max_queue_depth: None,
        // API-registered jobs have no Croniqfile `keep_last` (v1 supports
        // per-job caps only for DSL jobs); the global `execution_retention`
        // age sweep still covers them.
        keep_last: None,
        // API-registered jobs carry a concurrency limit (if any) through
        // their metadata (`__max_concurrent`) rather than this DSL field.
        max_concurrent: None,
        tags: job_def.tags.clone(),
    }
}

/// Synthesize a `JobDefinition` from a DSL-loaded `JobConfig` so DSL jobs
/// appear alongside stored API/runner-registered jobs in list responses.
pub fn synth_job_def_from_dsl(
    cfg: &JobConfig,
    now: DateTime<Utc>,
) -> croniq_store::models::JobDefinition {
    croniq_store::models::JobDefinition {
        job_key: cfg.key.clone(),
        description: cfg.description.clone(),
        assigned_runner_id: None,
        is_active: !matches!(
            cfg.schedule,
            croniq_config::schedule::CompiledSchedule::Disabled
        ),
        metadata: cfg.metadata.clone(),
        created_at: now,
        updated_at: now,
        timeout: cfg.timeout.clone(),
        max_retries: Some(cfg.retry.max_attempts),
        dead_letter_enabled: Some(cfg.dead_letter.enabled),
        tags: cfg.tags.clone(),
        dead_letter_retention: cfg.dead_letter.retention.clone(),
        dead_letter_operator_hint: cfg.dead_letter.operator_hint.clone(),
        dead_letter_replay_max_age: cfg.dead_letter.replay_max_age.clone(),
    }
}

/// Stable synthetic trigger ID for DSL-defined schedules.
/// These are not persisted; the ID is derived from the job key so references
/// round-trip through `GET /v1/schedules` and `GET /v1/schedules/{id}`.
pub fn dsl_trigger_id(job_key: &str) -> String {
    format!("dsl:{job_key}")
}

/// Synthesize a `TriggerDefinition` from a DSL-loaded `JobConfig`.
pub fn synth_trigger_def_from_dsl(
    cfg: &JobConfig,
    now: DateTime<Utc>,
) -> croniq_store::models::TriggerDefinition {
    croniq_store::models::TriggerDefinition {
        trigger_id: dsl_trigger_id(&cfg.key),
        job_key: cfg.key.clone(),
        cron_expression: Some(cfg.schedule_summary.clone()),
        timezone: cfg.timezone.clone(),
        calendar: cfg.calendar.clone(),
        window: cfg.window.clone(),
        not_before: cfg.not_before.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        not_after: cfg.not_after.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        enabled: !matches!(
            cfg.schedule,
            croniq_config::schedule::CompiledSchedule::Disabled
        ),
        managed_by: "dsl".into(),
        created_at: now,
        updated_at: now,
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
        && let Some(n) = rest
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok())
    {
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
    fn calendar_compile_failure_pauses_job_strict_default() {
        // `funday` is not a valid weekday; it parses and compiles at the config
        // layer but fails `Calendar::from_config` at runtime (issue #361).
        let src = r#"
            calendar biz { include weekly funday }
            job ops:tick {
                every 1 minutes { calendar biz }
            }
        "#;
        let cfg = load_str(src).unwrap();
        let trigger = &cfg.triggers["ops:tick"];
        // Fail closed: paused, not armed, so it cannot fire un-gated.
        assert_eq!(trigger.state, TriggerState::Paused);
        assert!(trigger.calendar.is_none());
        assert!(trigger.evaluate(Utc::now()).is_none());
        let reason = cfg.calendar_faults.get("ops:tick").expect("fault recorded");
        assert!(reason.contains("failed to compile"), "reason: {reason}");
    }

    #[test]
    fn undefined_calendar_ref_pauses_job_strict_default() {
        let src = r#"
            job ops:tick {
                every 1 minutes { calendar nonexistent }
            }
        "#;
        let cfg = load_str(src).unwrap();
        let trigger = &cfg.triggers["ops:tick"];
        assert_eq!(trigger.state, TriggerState::Paused);
        let reason = cfg.calendar_faults.get("ops:tick").expect("fault recorded");
        assert!(reason.contains("not defined"), "reason: {reason}");
    }

    #[test]
    fn strict_calendars_false_keeps_legacy_ungated_behavior() {
        let src = r#"
            policy { strict_calendars false }
            calendar biz { include weekly funday }
            job ops:tick {
                every 1 minutes { calendar biz }
            }
        "#;
        let cfg = load_str(src).unwrap();
        let trigger = &cfg.triggers["ops:tick"];
        // Legacy behavior: armed, calendar dropped (un-gated), no fault.
        assert_ne!(trigger.state, TriggerState::Paused);
        assert!(trigger.calendar.is_none());
        assert!(cfg.calendar_faults.is_empty());
    }

    #[test]
    fn fixing_calendar_rearms_job_on_reload() {
        let broken = r#"
            calendar biz { include weekly funday }
            job ops:tick { every 1 minutes { calendar biz } }
        "#;
        assert_eq!(
            load_str(broken).unwrap().triggers["ops:tick"].state,
            TriggerState::Paused
        );

        let fixed = r#"
            calendar biz { include weekly monday }
            job ops:tick { every 1 minutes { calendar biz } }
        "#;
        let cfg = load_str(fixed).unwrap();
        let trigger = &cfg.triggers["ops:tick"];
        assert!(trigger.calendar.is_some());
        assert!(cfg.calendar_faults.is_empty());
        assert_ne!(trigger.state, TriggerState::Paused);
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
        assert_eq!(
            cfg.runtime.jobs[0].timezone.as_deref(),
            Some("Europe/Vienna")
        );
        // And the trigger uses it
        let trigger = &cfg.triggers["billing:invoice"];
        assert_eq!(trigger.timezone, chrono_tz::Europe::Vienna);
    }

    #[test]
    fn invalid_croniqfile_returns_parse_error() {
        let src = r#"this is not valid DSL @@###"#;
        assert!(matches!(load_str(src), Err(LoadError::Parse { .. })));
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
        seed_job_state(
            &store,
            "migration:v2",
            croniq_store::models::JobStatus::Exhausted,
            None,
            1,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());

        let trigger = &cfg.triggers["migration:v2"];
        assert_eq!(trigger.state, TriggerState::Exhausted);
        assert!(trigger.next_fire_at.is_none());
    }

    #[test]
    fn restore_exhausted_recurring_trigger_is_rearmed() {
        // Regression for #249(b): a recurring (daily) trigger wrongly
        // persisted as Exhausted must recover on restart, not stay
        // permanently dead.
        let src = r#"job billing:backup { every day at 02:00 }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        seed_job_state(
            &store,
            "billing:backup",
            croniq_store::models::JobStatus::Exhausted,
            None,
            9,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());

        let trigger = &cfg.triggers["billing:backup"];
        assert_eq!(trigger.state, TriggerState::Armed);
        assert!(trigger.next_fire_at.is_some());
        // Historical fire_count is preserved.
        assert_eq!(trigger.fire_count, 9);
    }

    #[test]
    fn restore_exhausted_once_trigger_stays_terminal() {
        // A genuine `once` job that already fired must NOT be re-armed.
        let src = r#"job migration:v2 { once at 2026-04-01T03:00:00Z }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        seed_job_state(
            &store,
            "migration:v2",
            croniq_store::models::JobStatus::Exhausted,
            None,
            1,
        );

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
        assert_eq!(
            trigger.next_fire_at.unwrap().timestamp(),
            stored_next.timestamp()
        );
        assert_eq!(trigger.fire_count, 7);
    }

    #[test]
    fn restore_unknown_job_is_ignored() {
        let src = r#"job etl:sync { every 1 hours }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        // Store has a stale job_state for a job that no longer exists in config
        seed_job_state(
            &store,
            "removed:job",
            croniq_store::models::JobStatus::Exhausted,
            None,
            1,
        );

        // Should not panic; the etl:sync trigger is unaffected
        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());
        assert!(cfg.triggers.contains_key("etl:sync"));
    }

    #[test]
    fn restore_paused_does_not_change_trigger() {
        let src = r#"job reports:monthly { disabled }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        seed_job_state(
            &store,
            "reports:monthly",
            croniq_store::models::JobStatus::Paused,
            None,
            0,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, Utc::now());

        // Disabled DSL → Paused trigger; Paused status in DB → no override
        let trigger = &cfg.triggers["reports:monthly"];
        assert_eq!(trigger.state, TriggerState::Paused);
    }
}
