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
    trigger::{PendingFire, TimeWindow, Trigger, TriggerState},
};
use croniq_store::{
    models::{JobState, JobStatus},
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

    /// A `timezone` value that is not an IANA zone name (issue #426). Before
    /// this the loader fell back to `UTC`, so a one-character typo moved every
    /// wall-clock fire of the job by the zone's offset — permanently, with a
    /// green `croniq validate` and nothing in the log. Fails the boot instead:
    /// the alternative is running the wrong schedule and not knowing.
    #[error("invalid timezone in job '{job}': {reason}")]
    Timezone { job: String, reason: String },

    /// Semantic validation failed (issue #402). Carries every error-severity
    /// diagnostic, not just the first, so one boot attempt reports the whole
    /// list. Locations are omitted deliberately: after `import` resolution the
    /// AST spans belong to whichever file contributed the item, and pinning a
    /// line number to the wrong file is worse than none — `croniq validate`
    /// reports exact positions per file.
    #[error(
        "invalid configuration:\n{}\nRun `croniq validate <Croniqfile>` for exact locations.",
        .messages.iter().map(|m| format!("  - {m}")).collect::<Vec<_>>().join("\n")
    )]
    Validate { messages: Vec<String> },
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
            | LexError::InvalidEscape { span, .. }
            | LexError::UnexpectedChar { span, .. } => Some(*span),
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

/// Semantic validation options used on every server load path.
///
/// Only the calendar checks are off: a calendar that fails to compile and a
/// reference that resolves to nothing are both the loader's own business (see
/// [`ResolvedCalendars`] and issue #361) and must pause the affected jobs
/// rather than abort the boot.
const LOADER_VALIDATE: croniq_config::validate::Options = croniq_config::validate::Options {
    check_calendars: false,
};

/// Run semantic validation over the resolved AST and fail closed on any
/// error-severity diagnostic (issue #402).
///
/// Before this, the server path ran parse → compile → load and skipped the
/// validator entirely, so everything it uniquely catches — duplicate job keys,
/// schedule-less jobs, unknown runner types, `ephemeral` + `singleton`,
/// unknown directives (#403) — was silently accepted at boot and by
/// `croniq-server doctor`. Duplicate keys and schedule-less jobs quietly
/// reduced what the scheduler ended up running.
///
/// Warnings are logged but never block: they describe configurations that work
/// and are merely risky (e.g. an interval shorter than the runner poll cycle).
fn validate_ast(ast: &Croniqfile) -> Result<(), LoadError> {
    use croniq_config::validate::Severity;

    let mut messages = Vec::new();
    for d in croniq_config::validate::validate_with(ast, LOADER_VALIDATE) {
        match d.severity {
            Severity::Error => messages.push(d.message),
            Severity::Warning => {
                tracing::warn!(diagnostic = %d.message, "Croniqfile warning")
            }
        }
    }
    if messages.is_empty() {
        Ok(())
    } else {
        Err(LoadError::Validate { messages })
    }
}

/// Build a LoadedConfig from a compiled RuntimeConfig and its AST.
fn load_from_compiled(runtime: RuntimeConfig, ast: &Croniqfile) -> Result<LoadedConfig, LoadError> {
    validate_ast(ast)?;

    let now = Utc::now();

    let strict_calendars = runtime.policy.strict_calendars;

    // Build calendars from compiled config. Calendars that fail to compile are
    // dropped from the set but remembered as errors so a job that references
    // one can be failed closed (paused) instead of silently un-gated (issue
    // #361). The DSL path has no store calendars — those only matter for
    // API-registered triggers (see `trigger_from_definition`, issue #393).
    let resolved_calendars = resolve_calendars(&runtime.calendars, &[], strict_calendars);

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

        // Resolve the timezone. An unparseable zone aborts the load (issue
        // #426) — `validate_ast` above already rejects it for a Croniqfile, so
        // reaching this arm means the zone came from a path the validator does
        // not see; either way it must not degrade to UTC silently. No zone
        // declared anywhere is a different thing entirely: UTC is then the
        // documented default, and `validate` warns about it (issue #427).
        let tz: chrono_tz::Tz = match job_cfg.timezone.as_deref() {
            Some(name) => {
                croniq_config::timezone::parse(name).map_err(|e| LoadError::Timezone {
                    job: job_cfg.key.clone(),
                    reason: e.to_string(),
                })?
            }
            None => chrono_tz::UTC,
        };

        // Misfire policy: default FireNow (never skip a billing run)
        let misfire = MisfirePolicy::FireNow;

        // Resolve calendar reference. A reference that does not resolve (the
        // calendar failed to compile, or is not defined) is a fault: under the
        // default `strict_calendars` policy the job is failed closed (paused)
        // rather than fired without its gate (issue #361).
        let (calendar, calendar_fault) =
            resolved_calendars.resolve(&job_cfg.key, job_cfg.calendar.as_deref());

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
///   the next tick fires at the correct time instead of re-computing from now,
///   as long as the instant can still belong to the schedule now loaded —
///   `Trigger::carry_over_pending_fire` owns that judgement and names the two
///   exceptions: a gate-disallowed instant from a pre-#391 build, and one
///   later than this schedule's own next fire, which outlived the schedule
///   that produced it (#535, typically a shortened interval).
/// - `Paused`/`Disabled`/unknown → no change (trigger stays as loaded).
///
/// States healed here (re-armed exhausted triggers, recomputed gate-blocked
/// fires, instants that outlived their schedule) are persisted back to the
/// store immediately, so the UI and the missed-fire watchdog see the corrected
/// `next_fire_at` right after boot instead of only after the next fire (#391).
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

    // Rows whose job no longer exists. Deliberately not deleted: a job
    // commented out for a week should keep its state, and this loader cannot
    // tell "removed" from "temporarily absent". But they are worth naming —
    // they are invisible otherwise, and until issue #470 they also kept the
    // metrics exporter emitting `croniq_job_overdue` for jobs the server does
    // not know about. To clear one deliberately: DELETE /v1/jobs/{job_key}.
    let orphans: Vec<&str> = states
        .iter()
        .filter(|s| !triggers.contains_key(&s.job_key))
        .map(|s| s.job_key.as_str())
        .collect();
    if !orphans.is_empty() {
        tracing::info!(
            count = orphans.len(),
            job_keys = %orphans.join(", "),
            "job_states rows exist for jobs this configuration does not define. They are \
             kept (a job may be temporarily absent) and no longer produce metrics; clear \
             one with DELETE /v1/jobs/{{job_key}}."
        );
    }

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
                    persist_healed_state(store, trigger, &job_state, now);
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
                // or double-fire due to restart timing differences — unless it
                // cannot belong to the schedule now loaded (see
                // `carry_over_pending_fire`).
                if let Some(stored) = job_state.next_fire_at {
                    trigger.fire_count = job_state.fire_count;
                    match trigger.carry_over_pending_fire(stored, now) {
                        PendingFire::Adopted => {
                            tracing::debug!(
                                job_key = %job_state.job_key,
                                next_fire_at = ?job_state.next_fire_at,
                                "trigger restore: next_fire_at restored"
                            );
                        }
                        PendingFire::HealedGateClosed => {
                            tracing::warn!(
                                job_key = %job_state.job_key,
                                stored = %stored,
                                next_fire_at = ?trigger.next_fire_at,
                                "trigger restore: healed calendar/window-disallowed next_fire_at (pre-#391 row)"
                            );
                            persist_healed_state(store, trigger, &job_state, now);
                        }
                        PendingFire::HealedOutlivedSchedule => {
                            tracing::info!(
                                job_key = %job_state.job_key,
                                stored = %stored,
                                next_fire_at = ?trigger.next_fire_at,
                                schedule = %trigger.schedule.summary(),
                                "trigger restore: stored next_fire_at outlived its schedule (shortened?) — recomputed (#535)"
                            );
                            persist_healed_state(store, trigger, &job_state, now);
                        }
                    }
                }
            }
            JobStatus::Paused | JobStatus::Disabled => {
                // DSL intent already reflected in the trigger — nothing to do
            }
        }
    }
}

/// Persist a trigger state healed during restore, so the UI and the
/// missed-fire watchdog see the corrected `next_fire_at` immediately after
/// boot instead of only after the next fire (#391). Best-effort: a failed
/// write only logs — the next fire persists the same state anyway.
fn persist_healed_state(
    store: &dyn JobStore,
    trigger: &Trigger,
    stored: &JobState,
    now: DateTime<Utc>,
) {
    let healed_status = if trigger.state == TriggerState::Armed {
        JobStatus::Active
    } else {
        JobStatus::Exhausted
    };
    if healed_status == stored.status && trigger.next_fire_at == stored.next_fire_at {
        return; // nothing changed — don't touch updated_at
    }
    let healed = JobState {
        job_key: stored.job_key.clone(),
        next_fire_at: trigger.next_fire_at,
        // Preserve history — restore never rebuilds it.
        last_fired_at: stored.last_fired_at,
        fire_count: trigger.fire_count,
        status: healed_status,
        updated_at: now,
    };
    if let Err(e) = store.upsert_job_state(&healed) {
        tracing::warn!(
            job_key = %stored.job_key,
            error = %e,
            "trigger restore: could not persist healed job state"
        );
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

/// Parse a store-persisted `CalendarDefinition` (whose `rules` field holds
/// line-separated Croniqfile calendar-body DSL) into a compiled
/// `CalendarConfig`. Mirrors `api::calendars::validate_rules`: the rules are
/// wrapped in a synthetic calendar block and run through the real parser and
/// semantic validation, so stored rows compile exactly like Croniqfile
/// calendars. Rows predating the #356 validation gate may legitimately fail
/// here — callers treat that as a compile error, not a panic.
pub fn calendar_config_from_definition(
    def: &croniq_store::models::CalendarDefinition,
) -> Result<croniq_config::compile::CalendarConfig, String> {
    let source = format!("calendar \"__store__\" {{\n{}\n}}\n", def.rules);
    let ast = Parser::parse(&source).map_err(|e| e.to_string())?;
    let errors: Vec<String> = croniq_config::validate::validate(&ast)
        .into_iter()
        .filter(|d| d.severity == croniq_config::validate::Severity::Error)
        .map(|d| d.message)
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let runtime = compile::compile(&ast);
    let mut cfg = runtime
        .calendars
        .into_iter()
        .next()
        .ok_or_else(|| "rules did not produce a calendar".to_string())?;
    cfg.name = def.name.clone();
    // A `timezone` directive inside the rules text wins; otherwise fall back
    // to the definition's own timezone column. There is no `defaults { }` in
    // the synthetic block above, so a store calendar that declares nothing
    // lands on UTC — a row created through the API is not part of any
    // Croniqfile and must not silently change meaning when that file's
    // `defaults { timezone … }` does (issue #450).
    if cfg.timezone.is_none() {
        cfg.timezone = def.timezone.clone();
    }
    // The zone became load-bearing in #450, and the column predates any
    // validation on it (`POST`/`PUT /v1/calendars` now rejects a bad value,
    // but rows written before that were stored unchecked). Report and clear it
    // rather than failing the calendar: under `strict_calendars` an error here
    // would pause every job consulting it, which is a worse upgrade than
    // falling back to UTC out loud.
    if let Some(name) = cfg.timezone.as_deref().filter(|t| !t.is_empty())
        && let Err(e) = croniq_config::timezone::parse(name)
    {
        tracing::warn!(
            calendar = %def.name,
            timezone = %name,
            error = %e,
            "stored calendar timezone is not an IANA zone name — its rules are evaluated in UTC"
        );
        cfg.timezone = None;
    }
    Ok(cfg)
}

/// The effective calendar set used to attach gates to runtime triggers:
/// DSL-defined calendars plus store-persisted (API-managed) ones, with DSL
/// winning on name collision — the same precedence `GET /v1/calendars` uses.
pub struct ResolvedCalendars {
    /// Successfully compiled calendars, by name.
    pub calendars: HashMap<String, Calendar>,
    /// Calendars that failed to parse/compile, by name, with the reason.
    pub errors: HashMap<String, String>,
    /// `policy { strict_calendars }`: unresolvable references fail closed.
    pub strict: bool,
}

impl ResolvedCalendars {
    /// An empty, lenient set — for contexts with no calendar sources at all
    /// (storeless servers, tests).
    pub fn empty_lenient() -> Self {
        Self {
            calendars: HashMap::new(),
            errors: HashMap::new(),
            strict: false,
        }
    }

    /// Resolve a job's calendar reference to `(gate, fault)`.
    ///
    /// - `None` or empty name → no gate, no fault (empty string is the API's
    ///   "clear the gate" convention).
    /// - Known name → a clone of the compiled calendar, no fault.
    /// - Unknown/broken name → no gate; under `strict` a `Some(reason)` fault
    ///   the caller must fail closed on (pause the trigger, issue #361),
    ///   otherwise legacy fail-open with a warning.
    pub fn resolve(&self, job_key: &str, name: Option<&str>) -> (Option<Calendar>, Option<String>) {
        let name = match name {
            None | Some("") => return (None, None),
            Some(n) => n,
        };
        if let Some(cal) = self.calendars.get(name) {
            return (Some(cal.clone()), None);
        }
        let reason = match self.errors.get(name) {
            Some(err) => format!("calendar '{name}' failed to compile: {err}"),
            None => format!("calendar '{name}' is not defined"),
        };
        if self.strict {
            tracing::error!(
                job = %job_key,
                calendar = %name,
                reason = %reason,
                "job paused: referenced calendar did not resolve (strict_calendars)"
            );
            (None, Some(reason))
        } else {
            tracing::warn!(
                job = %job_key,
                calendar = %name,
                reason = %reason,
                "job loaded without its calendar gate (strict_calendars disabled)"
            );
            (None, None)
        }
    }
}

/// Compile the union of DSL and store calendars into a [`ResolvedCalendars`].
///
/// Store calendars are parsed from their persisted rules text; DSL calendars
/// are compiled on top so they win name collisions (Croniqfile precedence,
/// mirroring `GET /v1/calendars`). Compile failures land in `errors` so a
/// referencing job can be failed closed with a specific reason.
pub fn resolve_calendars(
    dsl: &[croniq_config::compile::CalendarConfig],
    stored: &[croniq_store::models::CalendarDefinition],
    strict: bool,
) -> ResolvedCalendars {
    let mut calendars: HashMap<String, Calendar> = HashMap::new();
    let mut errors: HashMap<String, String> = HashMap::new();
    for def in stored {
        // Skip DSL-synthesized rows if a caller passes the raw `/v1/calendars`
        // union — the DSL configs below are authoritative for those names.
        if def.managed_by == "dsl" {
            continue;
        }
        let compiled = calendar_config_from_definition(def)
            .and_then(|cfg| Calendar::from_config(&cfg).map_err(|e| e.to_string()));
        match compiled {
            Ok(cal) => {
                calendars.insert(def.name.clone(), cal);
            }
            Err(e) => {
                tracing::warn!(calendar = %def.name, error = %e, "failed to compile stored calendar — skipping");
                errors.insert(def.name.clone(), e);
            }
        }
    }
    for cfg in dsl {
        match Calendar::from_config(cfg) {
            Ok(cal) => {
                calendars.insert(cfg.name.clone(), cal);
                errors.remove(&cfg.name);
            }
            Err(e) => {
                // A referenced-but-broken calendar is escalated to ERROR at
                // the point of reference (`resolve`); the compile failure
                // itself stays a warning (an unreferenced broken calendar is
                // harmless). DSL wins even when broken: shadowed store
                // entries must not silently take over.
                tracing::warn!(calendar = %cfg.name, error = %e, "failed to compile calendar — skipping");
                calendars.remove(&cfg.name);
                errors.insert(cfg.name.clone(), e.to_string());
            }
        }
    }
    ResolvedCalendars {
        calendars,
        errors,
        strict,
    }
}

/// A runtime trigger built from a persisted `TriggerDefinition`, plus the
/// config fault that paused it (if any).
pub struct BuiltTrigger {
    pub trigger: Trigger,
    /// `Some(reason)` when the definition could not be honoured as written and
    /// the trigger is returned already **paused** (fail closed, mirroring the
    /// DSL path / issue #361). Callers surface the reason through
    /// `ServerState.config_faults`. Two conditions raise it:
    ///
    /// - the `calendar` reference did not resolve under `strict_calendars`;
    /// - the `timezone` is not an IANA zone name (issue #426). A stored row
    ///   cannot abort a boot the way a Croniqfile can, so it pauses instead —
    ///   firing a wall-clock schedule in the wrong zone is worse than not
    ///   firing it visibly.
    pub config_fault: Option<String>,
}

/// Build a runtime Trigger from a persisted TriggerDefinition.
///
/// Used for API/runner-registered jobs that don't come from the Croniqfile.
/// The definition's `calendar` name is resolved against `calendars` and the
/// compiled gate attached (issue #393). Returns None if the schedule can't
/// be parsed.
pub fn trigger_from_definition(
    def: &croniq_store::models::TriggerDefinition,
    calendars: &ResolvedCalendars,
    now: DateTime<Utc>,
) -> Option<BuiltTrigger> {
    let cron_expr = def.cron_expression.as_deref()?;
    let schedule = schedule_from_expr(cron_expr)?;

    // An unparseable zone pauses the trigger instead of silently becoming UTC
    // (issue #426). UTC is still what the paused trigger carries — it has to
    // carry something — but it can no longer fire, so the wrong-zone fires that
    // used to happen here don't.
    let (tz, timezone_fault) = match def.timezone.as_deref() {
        Some(name) => match croniq_config::timezone::parse(name) {
            Ok(tz) => (tz, None),
            Err(e) => (chrono_tz::UTC, Some(e.to_string())),
        },
        None => (chrono_tz::UTC, None),
    };

    let window = def.window.as_deref().and_then(TimeWindow::parse);

    let not_before = def.not_before;
    let not_after = def.not_after;

    let (calendar, calendar_fault) = calendars.resolve(&def.job_key, def.calendar.as_deref());

    let mut trigger = Trigger::with_bounds(
        def.job_key.clone(),
        schedule,
        tz,
        calendar,
        window,
        MisfirePolicy::FireNow,
        not_before,
        not_after,
        now,
    );

    // Fail closed: an unresolved calendar reference must not fire un-gated
    // (same contract as the DSL path in `load_from_compiled`), and neither must
    // a schedule whose declared zone is unknown. Both reasons are reported when
    // both apply — fixing one and re-registering must not reveal the other as a
    // surprise.
    let config_fault = match (calendar_fault, timezone_fault) {
        (Some(cal), Some(tz)) => Some(format!("{cal}; {tz}")),
        (cal, tz) => cal.or(tz),
    };
    if config_fault.is_some() {
        trigger.pause();
    }

    Some(BuiltTrigger {
        trigger,
        config_fault,
    })
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

    // Reconstruct the compiled schedule from the persisted expression so
    // next-fire previews work for API/adopted jobs. Previously hardcoded to
    // `Disabled`, which made every adopted job read as "never fires".
    let schedule = def
        .cron_expression
        .as_deref()
        .and_then(compiled_schedule_from_expr)
        .unwrap_or(CompiledSchedule::Disabled);

    JobConfig {
        key: def.job_key.clone(),
        namespace: ns.to_string(),
        name: name.to_string(),
        variant: None,
        description: job_def.and_then(|j| j.description.clone()),
        schedule,
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
        // `run_on_register` is a Croniqfile directive; an API/runner-registered
        // job has no Croniqfile definition to be adopted from (issue #555).
        run_on_register: false,
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
        // `run_on_register` is a Croniqfile directive; an API/runner-registered
        // job has no Croniqfile definition to be adopted from (issue #555).
        run_on_register: false,
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
        // Canonical, re-parseable DSL line — NOT `schedule_summary`, whose
        // comma-joined weekday/monthly forms don't round-trip through the
        // scheduler on reload (the summary is display-only).
        cron_expression: Some(cfg.schedule.to_dsl()),
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

/// Build a runtime [`Schedule`] from a persisted `cron_expression`.
///
/// API-created triggers store interval shorthand (`"5m"`, `"300"`,
/// `"*/5 * * * *"`); DSL-synthesized and adopted triggers store a canonical DSL
/// schedule line (`"every day at 02:00"`, `"every monday friday at 09:00"`,
/// `"once at \"…\""`) — see [`CompiledSchedule::to_dsl`]. The shorthand is tried
/// first so the fast path and existing API values are untouched, then the full
/// DSL grammar, so *every* schedule shape rebuilds — not just `Interval`
/// (issue found while fixing #393). Malformed expressions yield `None`.
fn schedule_from_expr(expr: &str) -> Option<Schedule> {
    if let Some(seconds) = parse_interval_seconds(expr) {
        return Some(Schedule::Interval { seconds });
    }
    let kind = croniq_config::parser::parse_schedule_expr(expr).ok()?;
    Schedule::from_ast(&kind).ok()
}

/// Reconstruct the [`CompiledSchedule`] a persisted `cron_expression` describes,
/// mirroring [`schedule_from_expr`] but for the config-side schedule type used
/// in previews / `JobConfig`. `None` for malformed expressions.
fn compiled_schedule_from_expr(expr: &str) -> Option<croniq_config::schedule::CompiledSchedule> {
    use croniq_config::schedule::CompiledSchedule;
    if let Some(seconds) = parse_interval_seconds(expr) {
        return Some(CompiledSchedule::Interval { seconds });
    }
    let kind = croniq_config::parser::parse_schedule_expr(expr).ok()?;
    Some(CompiledSchedule::from_ast(&kind))
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

    /// The error-severity messages a load produced, or an empty vec when it
    /// succeeded. Used by the #402 cases below.
    fn load_errors(src: &str) -> Vec<String> {
        match load_str(src) {
            Ok(_) => Vec::new(),
            Err(LoadError::Validate { messages }) => messages,
            Err(other) => panic!("expected a validation failure, got: {other}"),
        }
    }

    // ── Issue #402: the server load path must run the validator ─────────────
    //
    // Each case below used to load with exit 0 — the validator was only
    // reachable through `croniq validate`, so `doctor` reported success for
    // semantically broken configs.

    #[test]
    fn duplicate_job_key_fails_closed() {
        // The exact repro from #402: one of the two jobs used to be dropped
        // silently ("jobs=2 triggers=1").
        let src = r#"
            job demo:noop { every 1 hour
                            runner shell { command "true" } }
            job demo:noop { every 2 hours
                            runner shell { command "true" } }
        "#;
        assert_eq!(
            load_errors(src),
            vec!["duplicate job key 'demo:noop'".to_string()]
        );
    }

    #[test]
    fn job_without_schedule_fails_closed() {
        let src = r#"job demo:noop { runner shell { command "true" } }"#;
        assert_eq!(
            load_errors(src),
            vec!["job 'demo:noop' has no schedule".to_string()]
        );
    }

    #[test]
    fn unknown_runner_type_fails_closed() {
        let src = r#"
            job demo:noop {
                every 1 hour
                runner http { url "https://example.test" }
            }
        "#;
        let errors = load_errors(src);
        assert!(
            errors
                .iter()
                .any(|m| m.contains("unknown runner type 'http'")),
            "got: {errors:?}"
        );
    }

    #[test]
    fn ephemeral_with_singleton_fails_closed() {
        // #302 was fixed in `croniq validate` but not on the path that
        // schedules the job.
        let src = r#"
            job demo:noop {
                ephemeral every 1 hour
                singleton
                runner shell { command "true" }
            }
        "#;
        let errors = load_errors(src);
        assert!(
            errors
                .iter()
                .any(|m| m.contains("has no effect on the `ephemeral` job")),
            "got: {errors:?}"
        );
    }

    #[test]
    fn unknown_directive_fails_closed() {
        // #403 repro: a typo'd retention knob left history growing forever and
        // the only signal was a missing line in the boot log.
        let src = r#"
            server { listen :4000
                     execution_retentionn 90d }
            job demo:noop { every 1 hour
                            runner shell { command "true" } }
        "#;
        let errors = load_errors(src);
        assert_eq!(errors.len(), 1, "got: {errors:?}");
        assert!(
            errors[0].contains("unknown directive 'execution_retentionn'"),
            "got: {}",
            errors[0]
        );
    }

    #[test]
    fn removed_pull_api_auth_fails_closed() {
        // #408: silently ignoring this line rotates the key that wraps stored
        // TOTP secrets, so it must not load.
        let src = r#"
            pull_api { auth some-secret }
            job demo:noop { every 1 hour
                            runner shell { command "true" } }
        "#;
        let errors = load_errors(src);
        assert_eq!(errors.len(), 1, "got: {errors:?}");
        assert!(
            errors[0].contains("CRONIQ_JWT_SECRET"),
            "got: {}",
            errors[0]
        );
    }

    #[test]
    fn validation_error_lists_every_finding() {
        // One boot attempt reports the whole list, not just the first error.
        let src = r#"
            server { listenn :4000 }
            job demo:noop { runner shell { command "true" } }
        "#;
        let errors = load_errors(src);
        assert_eq!(errors.len(), 2, "got: {errors:?}");
        let rendered = format!(
            "{}",
            LoadError::Validate {
                messages: errors.clone()
            }
        );
        assert!(rendered.contains("croniq validate"), "got: {rendered}");
        for m in &errors {
            assert!(rendered.contains(m), "missing {m} in: {rendered}");
        }
    }

    #[test]
    fn valid_config_still_loads() {
        // The counterpart to the cases above: a clean file must not acquire
        // new errors from the validator running on this path.
        let src = r#"
            server { listen :4000; execution_retention 30d }
            defaults { timezone UTC; timeout 5m }
            calendar biz { include weekly weekday }
            job demo:noop {
                every 1 hour { calendar biz }
                runner shell { command "true" }
            }
        "#;
        let cfg = load_str(src).expect("clean config must load");
        assert_eq!(cfg.runtime.jobs.len(), 1);
        assert!(cfg.calendar_faults.is_empty());
    }

    #[test]
    fn broken_calendar_still_pauses_instead_of_failing_the_load() {
        // Guard the #361 boundary: calendar rule failures and unresolvable
        // references stay per-job faults, so the loader disables those checks
        // rather than turning them into boot failures.
        for src in [
            r#"calendar biz { include weekly funday }
               job ops:tick { every 1 minutes { calendar biz } }"#,
            r#"job ops:tick { every 1 minutes { calendar nonexistent } }"#,
        ] {
            let cfg = load_str(src).expect("calendar faults must not fail the load");
            assert_eq!(cfg.triggers["ops:tick"].state, TriggerState::Paused);
            assert!(cfg.calendar_faults.contains_key("ops:tick"));
        }
    }

    #[test]
    fn calendar_with_weekday_alias_loads() {
        // #356 repro: the `weekday` alias used to fail compilation, the
        // calendar was silently skipped, and the job lost its gate.
        let src = r#"
            calendar biz { include weekly weekday }
            job demo:tick {
                every 1 minutes { calendar biz }
            }
        "#;
        let cfg = load_str(src).unwrap();
        assert!(
            cfg.triggers["demo:tick"].calendar.is_some(),
            "calendar gate must not be dropped"
        );
    }

    #[test]
    fn fmt_round_trip_calendar_survives_load() {
        // #356: `croniq fmt` collapses Mon–Fri to `weekday` — a fmt
        // round-trip of a working Croniqfile must stay loadable.
        let src = r#"
            calendar biz { include weekly "Mon".."Fri" }
            job demo:tick {
                every day at 09:00 { calendar biz }
            }
        "#;
        let ast = croniq_config::parser::Parser::parse(src).unwrap();
        let formatted = croniq_config::format::format(&ast);
        assert!(
            formatted.contains("include weekly weekday"),
            "fmt should emit the alias, got:\n{formatted}"
        );
        let cfg = load_str(&formatted).unwrap();
        assert!(cfg.triggers["demo:tick"].calendar.is_some());
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
    fn restore_active_recomputes_pending_fire_of_a_shortened_schedule() {
        // Issue #535: the job ran `every 1 hour`, was edited to
        // `every 1 minute`, and the server restarted. The persisted fire time
        // belongs to the hourly phase — adopting it would keep the job silent
        // for up to an hour while every API surface reports `every 1 minute`.
        let src = r#"job etl:sync { every 1 minutes }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        let now = Utc::now();
        let stale_hourly = now + chrono::Duration::minutes(41);
        seed_job_state(
            &store,
            "etl:sync",
            croniq_store::models::JobStatus::Active,
            Some(stale_hourly),
            7,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, now);

        let trigger = &cfg.triggers["etl:sync"];
        assert_eq!(
            trigger.next_fire_at,
            Some(now + chrono::Duration::minutes(1)),
            "the pending hourly fire must not outlive the schedule that produced it"
        );
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.fire_count, 7);

        // Healed state is persisted, so /v1/jobs and the missed-fire watchdog
        // stop reporting the stale instant right away (#391 pattern).
        let row = store
            .list_job_states()
            .unwrap()
            .into_iter()
            .find(|s| s.job_key == "etl:sync")
            .unwrap();
        assert_eq!(row.next_fire_at, trigger.next_fire_at);
        assert_eq!(row.fire_count, 7);
    }

    #[test]
    fn restore_active_keeps_a_pending_fire_the_schedule_still_allows() {
        // Counterpart to the test above: an untouched job must keep its
        // persisted instant across a restart, or a restart loop could
        // postpone it forever.
        let src = r#"job etl:sync { every 1 hours }"#;
        let mut cfg = load_str(src).unwrap();
        let store = make_store();

        let now = Utc::now();
        let pending = now + chrono::Duration::minutes(3);
        seed_job_state(
            &store,
            "etl:sync",
            croniq_store::models::JobStatus::Active,
            Some(pending),
            2,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, now);

        assert_eq!(cfg.triggers["etl:sync"].next_fire_at, Some(pending));
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

    // ─── restore healing for calendar-gated jobs (#391) ─────────────────────

    /// `every 1 minute { calendar business-hours }` — the issue #391 shape.
    const GATED_JOB_SRC: &str = r#"
        calendar biz {
            include weekly weekday
            include window "08:00".."18:00"
        }
        job ops:tick { every 1 minutes { calendar biz } }
    "#;

    fn fixed_utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn stored_state(store: &SqliteStore, key: &str) -> croniq_store::models::JobState {
        store
            .list_job_states()
            .unwrap()
            .into_iter()
            .find(|s| s.job_key == key)
            .unwrap()
    }

    #[test]
    fn restore_active_gate_disallowed_next_fire_is_healed_and_persisted() {
        // A pre-#391 build could persist Active rows whose next_fire_at
        // points inside a closed calendar gate (Saturday, here). Restore must
        // recompute instead of resurrecting the stale "overdue" — and write
        // the healed row back so the UI clears right after boot.
        let mut cfg = load_str(GATED_JOB_SRC).unwrap();
        let store = make_store();

        let now = fixed_utc(2026, 3, 28, 12, 0); // Saturday noon
        let stale = fixed_utc(2026, 3, 28, 11, 59); // Saturday → gate closed
        seed_job_state(
            &store,
            "ops:tick",
            croniq_store::models::JobStatus::Active,
            Some(stale),
            42,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, now);

        let monday_open = fixed_utc(2026, 3, 30, 8, 0);
        let trigger = &cfg.triggers["ops:tick"];
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(monday_open));
        assert_eq!(trigger.fire_count, 42);

        let row = stored_state(&store, "ops:tick");
        assert_eq!(row.status, croniq_store::models::JobStatus::Active);
        assert_eq!(row.next_fire_at, Some(monday_open));
        assert_eq!(row.fire_count, 42);
    }

    #[test]
    fn restore_active_gate_allowed_past_instant_is_kept() {
        // A gate-ALLOWED instant in the past is a legitimately missed fire:
        // MisfirePolicy::FireNow catches it up once. Restore must keep it
        // verbatim and not touch the store.
        let mut cfg = load_str(GATED_JOB_SRC).unwrap();
        let store = make_store();

        let now = fixed_utc(2026, 3, 28, 12, 0); // Saturday noon
        let missed = fixed_utc(2026, 3, 27, 17, 59); // Friday, in-window
        seed_job_state(
            &store,
            "ops:tick",
            croniq_store::models::JobStatus::Active,
            Some(missed),
            7,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, now);

        let trigger = &cfg.triggers["ops:tick"];
        assert_eq!(trigger.next_fire_at, Some(missed));
        assert_eq!(trigger.fire_count, 7);

        let row = stored_state(&store, "ops:tick");
        assert_eq!(row.next_fire_at, Some(missed));
    }

    #[test]
    fn restore_exhausted_calendar_gated_recurring_rearms_and_persists() {
        // The restart case reported in #391: a calendar-gated recurring job
        // wrongly persisted as Exhausted must come back armed at the next
        // gate-open instant — and the healed row must be persisted so the
        // stale state clears without waiting for the next fire.
        let mut cfg = load_str(GATED_JOB_SRC).unwrap();
        let store = make_store();

        let now = fixed_utc(2026, 3, 28, 12, 0); // Saturday noon
        seed_job_state(
            &store,
            "ops:tick",
            croniq_store::models::JobStatus::Exhausted,
            None,
            5,
        );

        restore_trigger_states(&mut cfg.triggers, &*store, now);

        let monday_open = fixed_utc(2026, 3, 30, 8, 0);
        let trigger = &cfg.triggers["ops:tick"];
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(monday_open));

        let row = stored_state(&store, "ops:tick");
        assert_eq!(row.status, croniq_store::models::JobStatus::Active);
        assert_eq!(row.next_fire_at, Some(monday_open));
        assert_eq!(row.fire_count, 5);
    }

    // ─── API calendar resolution + attachment (#393) ────────────────────────

    use croniq_store::models::{CalendarDefinition, TriggerDefinition};

    fn cal_def(name: &str, rules: &str) -> CalendarDefinition {
        CalendarDefinition {
            calendar_id: format!("id-{name}"),
            name: name.into(),
            timezone: None,
            rules: rules.into(),
            managed_by: "api".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn trigger_def(job_key: &str, cron: &str, calendar: Option<&str>) -> TriggerDefinition {
        TriggerDefinition {
            trigger_id: format!("tid-{job_key}"),
            job_key: job_key.into(),
            cron_expression: Some(cron.into()),
            timezone: None,
            calendar: calendar.map(|c| c.into()),
            window: None,
            not_before: None,
            not_after: None,
            enabled: true,
            managed_by: "api".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn store_calendar_compiles_and_gates_trigger() {
        // A store-persisted calendar must gate an API trigger's fire time —
        // the core #393 fix. Weekdays-only, so a Saturday `now` jumps to Monday.
        let stored = vec![cal_def("weekdays", "include weekly weekday")];
        let resolved = resolve_calendars(&[], &stored, true);
        assert!(resolved.calendars.contains_key("weekdays"));

        let def = trigger_def("api:tick", "1m", Some("weekdays"));
        let now = fixed_utc(2026, 3, 28, 12, 0); // Saturday
        let built = trigger_from_definition(&def, &resolved, now).unwrap();

        assert!(built.config_fault.is_none());
        assert!(built.trigger.calendar.is_some());
        assert_eq!(built.trigger.state, TriggerState::Armed);
        // Gate skips the weekend to Monday 00:00.
        assert_eq!(
            built.trigger.next_fire_at,
            Some(fixed_utc(2026, 3, 30, 0, 0))
        );
    }

    #[test]
    fn dsl_calendar_wins_name_collision_over_store() {
        // Store and DSL both define "shared"; the DSL rule (weekly monday)
        // must win over the store rule (weekly weekday).
        let dsl = load_str("calendar shared { include weekly monday }")
            .unwrap()
            .runtime
            .calendars;
        let stored = vec![cal_def("shared", "include weekly weekday")];
        let resolved = resolve_calendars(&dsl, &stored, true);

        let def = trigger_def("api:tick", "1m", Some("shared"));
        let now = fixed_utc(2026, 3, 31, 12, 0); // Tuesday
        let built = trigger_from_definition(&def, &resolved, now).unwrap();
        // DSL "monday-only" → next fire is the following Monday, not tomorrow.
        assert_eq!(
            built.trigger.next_fire_at,
            Some(fixed_utc(2026, 4, 6, 0, 0))
        );
    }

    #[test]
    fn broken_store_calendar_faults_closed_under_strict() {
        let stored = vec![cal_def("bad", "include weekly notaday")];
        let resolved = resolve_calendars(&[], &stored, true);
        assert!(resolved.errors.contains_key("bad"));

        let def = trigger_def("api:tick", "1m", Some("bad"));
        let built = trigger_from_definition(&def, &resolved, Utc::now()).unwrap();
        assert_eq!(built.trigger.state, TriggerState::Paused);
        let reason = built.config_fault.expect("fault");
        assert!(reason.contains("failed to compile"), "reason: {reason}");
    }

    // ── Issue #426: an invalid IANA zone must not become UTC ─────────────────

    #[test]
    fn dsl_invalid_timezone_fails_the_load() {
        // The #426 repro: `Europe/Berln` used to load clean and run in UTC.
        let src = r#"
            defaults { timezone Europe/Berln }
            job billing:invoice { every day at 02:00 }
        "#;
        match load_str(src) {
            Ok(_) => panic!("an unknown timezone must not load"),
            // The validator sees it first on the DSL path, which is the
            // better error (it lists every problem at once) — either variant
            // is a hard failure, which is what this asserts.
            Err(LoadError::Validate { messages }) => {
                assert!(
                    messages.iter().any(|m| m.contains("unknown timezone")),
                    "got: {messages:?}"
                );
                assert!(
                    messages.iter().any(|m| m.contains("Europe/Berlin")),
                    "the suggestion must name the intended zone, got: {messages:?}"
                );
            }
            Err(LoadError::Timezone { job, reason }) => {
                assert_eq!(job, "billing:invoice");
                assert!(reason.contains("unknown timezone"), "got: {reason}");
            }
            Err(other) => panic!("expected a timezone failure, got: {other}"),
        }
    }

    #[test]
    fn dsl_job_level_timezone_reaches_the_trigger() {
        // Issue #426, the other half: the job-body spelling used to compile to
        // `null` and leave the trigger in UTC.
        let src = r#"
            job billing:invoice {
                every day at 02:00
                timezone Europe/Vienna
            }
        "#;
        let cfg = load_str(src).expect("job-level timezone must load");
        assert_eq!(
            cfg.triggers["billing:invoice"].timezone,
            chrono_tz::Tz::Europe__Vienna
        );
    }

    #[test]
    fn dsl_schedule_option_timezone_beats_the_job_level_one() {
        let src = r#"
            defaults { timezone UTC }
            job billing:invoice {
                every day at 02:00 { timezone America/New_York }
                timezone Europe/Vienna
            }
        "#;
        let cfg = load_str(src).expect("must load");
        assert_eq!(
            cfg.triggers["billing:invoice"].timezone,
            chrono_tz::Tz::America__New_York
        );
    }

    #[test]
    fn dsl_without_any_timezone_still_loads_as_utc() {
        // UTC-by-default stays the documented behaviour (issue #427) — only the
        // *invalid* zone is a failure, and the missing one is a validate
        // warning, which must not block the boot.
        let src = r#"job billing:invoice { every day at 02:00 }"#;
        let cfg = load_str(src).expect("no timezone anywhere must still load");
        assert_eq!(cfg.triggers["billing:invoice"].timezone, chrono_tz::UTC);
    }

    #[test]
    fn stored_trigger_with_invalid_timezone_faults_closed() {
        // A persisted row cannot abort a boot, so it pauses instead: firing a
        // wall-clock schedule in the wrong zone is worse than not firing.
        let resolved = resolve_calendars(&[], &[], true);
        let mut def = trigger_def("api:tick", "every day at 02:00", None);
        def.timezone = Some("Europe/Berln".into());

        let built = trigger_from_definition(&def, &resolved, Utc::now()).unwrap();
        assert_eq!(built.trigger.state, TriggerState::Paused);
        let reason = built.config_fault.expect("fault");
        assert!(reason.contains("unknown timezone"), "reason: {reason}");
    }

    #[test]
    fn stored_trigger_reports_both_calendar_and_timezone_faults() {
        let resolved = resolve_calendars(&[], &[], true);
        let mut def = trigger_def("api:tick", "every day at 02:00", Some("ghost"));
        def.timezone = Some("Europe/Berln".into());

        let built = trigger_from_definition(&def, &resolved, Utc::now()).unwrap();
        let reason = built.config_fault.expect("fault");
        assert!(reason.contains("not defined"), "reason: {reason}");
        assert!(reason.contains("unknown timezone"), "reason: {reason}");
    }

    #[test]
    fn stored_trigger_with_valid_timezone_is_not_faulted() {
        let resolved = resolve_calendars(&[], &[], true);
        let mut def = trigger_def("api:tick", "every day at 02:00", None);
        def.timezone = Some("Europe/Vienna".into());

        let built = trigger_from_definition(&def, &resolved, Utc::now()).unwrap();
        assert!(built.config_fault.is_none());
        assert_eq!(built.trigger.timezone, chrono_tz::Tz::Europe__Vienna);
        assert_eq!(built.trigger.state, TriggerState::Armed);
    }

    #[test]
    fn unknown_calendar_faults_closed_under_strict() {
        let resolved = resolve_calendars(&[], &[], true);
        let def = trigger_def("api:tick", "1m", Some("ghost"));
        let built = trigger_from_definition(&def, &resolved, Utc::now()).unwrap();
        assert_eq!(built.trigger.state, TriggerState::Paused);
        assert!(built.config_fault.unwrap().contains("not defined"));
    }

    #[test]
    fn unknown_calendar_lenient_runs_ungated_without_fault() {
        let resolved = resolve_calendars(&[], &[], false);
        let def = trigger_def("api:tick", "1m", Some("ghost"));
        let built = trigger_from_definition(&def, &resolved, Utc::now()).unwrap();
        assert_ne!(built.trigger.state, TriggerState::Paused);
        assert!(built.trigger.calendar.is_none());
        assert!(built.config_fault.is_none());
    }

    #[test]
    fn empty_calendar_name_is_no_gate() {
        let resolved = resolve_calendars(&[], &[], true);
        let def = trigger_def("api:tick", "1m", Some(""));
        let built = trigger_from_definition(&def, &resolved, Utc::now()).unwrap();
        assert!(built.trigger.calendar.is_none());
        assert!(built.config_fault.is_none());
    }

    #[test]
    fn synth_dsl_trigger_rebuilds_for_all_schedule_shapes() {
        // A DSL job's synthesized trigger must round-trip through the store:
        // `synth_trigger_def_from_dsl` persists a canonical schedule expression
        // that `trigger_from_definition` rebuilds into the *same* Schedule
        // shape — not just intervals. Before the fix, `cron_expression` held
        // the human summary (e.g. "every 5 minutes"), which only `Interval`
        // jobs could re-parse, so an adopted daily/weekly/monthly/once job
        // vanished from the scheduler on reload/restart. This is the unit-level
        // stand-in for "build_plan/reload rebuilds the adopted trigger" across
        // every schedule shape.
        let resolved = ResolvedCalendars::empty_lenient();
        let now = fixed_utc(2026, 3, 28, 12, 0); // Saturday

        #[allow(clippy::type_complexity)]
        let cases: &[(&str, fn(&Schedule) -> bool)] = &[
            ("job iv:tick { every 5 minutes }", |s| {
                matches!(s, Schedule::Interval { seconds: 300 })
            }),
            ("job dl:report { every day at 02:00 }", |s| {
                matches!(s, Schedule::Daily { .. })
            }),
            ("job wk:run { every monday friday at 09:00 }", |s| {
                matches!(s, Schedule::Weekdays { .. })
            }),
            ("job mo:bill { every 1st 15th of month at 10:00 }", |s| {
                matches!(s, Schedule::Monthly { .. })
            }),
            (
                r#"job on:migrate { once at "2999-01-01T00:00:00Z" }"#,
                |s| matches!(s, Schedule::Once { .. }),
            ),
        ];

        for (src, is_shape) in cases {
            let cfg = load_str(src).unwrap().runtime.jobs.pop().unwrap();
            let def = synth_trigger_def_from_dsl(&cfg, now);
            let built = trigger_from_definition(&def, &resolved, now)
                .unwrap_or_else(|| panic!("{src}: trigger_from_definition returned None"));
            assert!(
                is_shape(&built.trigger.schedule),
                "{src}: rebuilt wrong shape: {:?}",
                built.trigger.schedule
            );
            assert!(
                built.trigger.next_fire_at.is_some(),
                "{src}: rebuilt trigger has no next fire time"
            );
        }
    }

    #[test]
    fn calendar_config_timezone_precedence() {
        // A `timezone` directive inside the rules text wins over the column.
        let mut with_directive = cal_def("tz", "timezone Europe/Vienna\ninclude weekly weekday");
        with_directive.timezone = Some("UTC".into());
        let cfg = calendar_config_from_definition(&with_directive).unwrap();
        assert_eq!(cfg.timezone.as_deref(), Some("Europe/Vienna"));

        // With no directive, the column is the fallback.
        let mut column_only = cal_def("tz2", "include weekly weekday");
        column_only.timezone = Some("Europe/Berlin".into());
        let cfg = calendar_config_from_definition(&column_only).unwrap();
        assert_eq!(cfg.timezone.as_deref(), Some("Europe/Berlin"));
    }

    /// A stored zone that is not an IANA name is cleared, not fatal (#450).
    /// `POST`/`PUT /v1/calendars` rejects such a value now, but rows written
    /// before that check existed have to keep loading: under
    /// `strict_calendars` a compile error here would pause every job that
    /// consults the calendar, which is a worse upgrade than UTC plus a WARN.
    #[test]
    fn invalid_stored_calendar_timezone_falls_back_instead_of_failing() {
        let mut bad = cal_def("legacy", "include weekly weekday");
        bad.timezone = Some("Europe/Wien".into());
        let cfg = calendar_config_from_definition(&bad).unwrap();
        assert_eq!(cfg.timezone, None);

        let resolved = resolve_calendars(&[], &[bad], true);
        assert!(resolved.errors.is_empty(), "got: {:?}", resolved.errors);
        assert_eq!(
            resolved.calendars.get("legacy").map(|c| c.tz),
            Some(chrono_tz::UTC)
        );
    }

    /// A store calendar is not part of any Croniqfile, so it must not pick up
    /// that file's `defaults { timezone … }` — the synthetic block it is
    /// compiled in has no defaults, and the resolved zone stays UTC (#450).
    #[test]
    fn store_calendar_does_not_inherit_croniqfile_defaults() {
        let dsl = load_str(
            "defaults { timezone Europe/Vienna }\ncalendar dslcal { include weekly monday }",
        )
        .unwrap()
        .runtime
        .calendars;
        let stored = vec![cal_def("apical", "include weekly monday")];
        let resolved = resolve_calendars(&dsl, &stored, true);

        // The DSL calendar inherits it; the API one does not.
        assert_eq!(
            resolved.calendars.get("dslcal").map(|c| c.tz),
            Some(chrono_tz::Europe::Vienna)
        );
        assert_eq!(
            resolved.calendars.get("apical").map(|c| c.tz),
            Some(chrono_tz::UTC)
        );
    }
}
