//! Semantic validation of a parsed Croniqfile AST.

use crate::ast::*;
use crate::calendar_args;
use miette::SourceSpan;
use std::collections::HashSet;

/// Validation diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A validation diagnostic.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: SourceSpan,
}

/// Which checks [`validate_with`] runs. Every field defaults to "on"; a caller
/// only ever turns a check *off* because it enforces that rule itself.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Check `calendar { }` rule arguments and job `calendar` references.
    ///
    /// The server's loader turns this off because it owns both outcomes: a
    /// calendar whose rules don't compile, and a reference that resolves to no
    /// calendar (DSL or store-registered), fail *per job* — the job loads
    /// paused with a `config_faults` entry under `policy { strict_calendars }`
    /// (issue #361). Left on, either condition would abort the whole boot
    /// instead of pausing the jobs that depend on it.
    ///
    /// Duplicate calendar *names* are checked regardless: nothing downstream
    /// reports them, and one definition silently wins.
    pub check_calendars: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            check_calendars: true,
        }
    }
}

/// Validate a Croniqfile AST, returning errors and warnings.
pub fn validate(ast: &Croniqfile) -> Vec<Diagnostic> {
    validate_with(ast, Options::default())
}

/// [`validate`] with individual checks disabled — see [`Options`].
pub fn validate_with(ast: &Croniqfile, opts: Options) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut job_keys = HashSet::new();
    let mut calendar_names = HashSet::new();

    // First pass: collect calendar names
    for item in &ast.items {
        if let Item::Calendar(cal) = item
            && !calendar_names.insert(cal.name.value.clone())
        {
            diags.push(Diagnostic {
                severity: Severity::Error,
                message: format!("duplicate calendar name '{}'", cal.name.value),
                span: cal.name.span.into(),
            });
        }
    }

    // Unknown / removed directive keys and sub-block names in the
    // operator-facing blocks (`server { }`, `pull_api { }`, …). Kept in its own
    // module because it is table-driven rather than rule-driven (issue #403).
    crate::block_directives::validate_blocks(ast, &mut diags);

    // Running `defaults { execution_mode … }` baseline. Tracked in item order
    // so per-job ephemeral detection matches compile_job: a defaults block only
    // affects jobs declared after it, and later blocks/directives win.
    let mut default_ephemeral = false;

    // Second pass: validate everything
    for item in &ast.items {
        match item {
            Item::Defaults(def) => {
                for dob in &def.directives {
                    if let DirectiveOrBlock::Directive(dir) = dob
                        && dir.key.value == "execution_mode"
                        && let Some(arg) = dir.args.first()
                    {
                        default_ephemeral = arg.value == "ephemeral";
                    }
                }
            }
            Item::Job(job) => {
                // Unique job keys
                if !job_keys.insert(job.key.raw.clone()) {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!("duplicate job key '{}'", job.key.raw),
                        span: job.key.span.into(),
                    });
                }

                // Job must have a schedule
                if job.schedule.is_none() {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!("job '{}' has no schedule", job.key.raw),
                        span: job.key.span.into(),
                    });
                }

                // Validate schedule calendar references
                if let Some(ref sched) = job.schedule
                    && opts.check_calendars
                {
                    for opt in &sched.options {
                        if opt.key.value == "calendar" {
                            for arg in &opt.args {
                                if !arg.is_placeholder && !calendar_names.contains(&arg.value) {
                                    diags.push(Diagnostic {
                                        severity: Severity::Error,
                                        message: format!(
                                            "calendar '{}' referenced in job '{}' is not defined",
                                            arg.value, job.key.raw
                                        ),
                                        span: arg.span.into(),
                                    });
                                }
                            }
                        }
                    }
                }

                // Validate time in schedule
                if let Some(ref sched) = job.schedule {
                    validate_schedule_kind(sched, &mut diags);
                }

                // Validate runner constraints
                validate_runner_constraints(job, &mut diags);

                // Validate singleton / max_concurrent (issue #278, #302)
                validate_concurrency(job, default_ephemeral, &mut diags);
            }
            // Falls through to the catch-all when the caller owns calendar
            // failures itself — see `Options::check_calendars`.
            Item::Calendar(cal) if opts.check_calendars => {
                validate_calendar(cal, &mut diags);
            }
            _ => {}
        }
    }

    diags
}

fn validate_schedule_kind(sched: &ScheduleNode, diags: &mut Vec<Diagnostic>) {
    match &sched.kind {
        ScheduleKind::Interval { count, unit } => {
            // Warn when the interval is shorter than the default long-poll
            // timeout (30 s). Jobs scheduled faster than runners poll will be
            // delayed to the next poll cycle, introducing jitter.
            let seconds = match unit {
                IntervalUnit::Seconds => *count,
                IntervalUnit::Minutes => count.saturating_mul(60),
                IntervalUnit::Hours => count.saturating_mul(3600),
            };
            if seconds < 30 {
                diags.push(Diagnostic {
                    severity: Severity::Warning,
                    message: format!(
                        "interval of {}s is shorter than the default runner poll cycle (30s); \
                         executions may be delayed by up to one poll period",
                        seconds
                    ),
                    span: sched.span.into(),
                });
            }
            if *count == 0 {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: "interval count must be greater than zero".into(),
                    span: sched.span.into(),
                });
            }
        }
        ScheduleKind::Weekdays { .. } => {}
        ScheduleKind::Monthly { .. } => {}
        _ => {}
    }
}

fn validate_calendar(cal: &CalendarBlock, diags: &mut Vec<Diagnostic>) {
    let has_timezone = cal.rules.iter().any(|r| {
        r.rule_type.value == "timezone" || r.args.first().is_some_and(|a| a.value.contains('/'))
    });

    // Timezone is expected but not strictly required (inherits from defaults)
    let _ = has_timezone;

    // Rule arguments are checked with the same `calendar_args` parsers the
    // scheduler's compile step uses, so `validate` errors exactly where the
    // loader would reject the calendar (#356). Severity rule: Error ⇔ the
    // loader rejects the rule; Warning ⇔ the loader accepts it but the
    // argument can never match / is silently ignored. Placeholder args
    // resolve at compile time and are skipped.
    for rule in &cal.rules {
        match rule.rule_type.value.as_str() {
            "weekly" => {
                for arg in &rule.args {
                    if arg.is_placeholder {
                        continue;
                    }
                    if let Err(msg) = calendar_args::parse_weekly_arg(&arg.value) {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: msg,
                            span: arg.span.into(),
                        });
                    }
                }
            }
            "window" => {
                if rule.args.is_empty() {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: "window rule requires start and end times".into(),
                        span: rule.span.into(),
                    });
                } else if rule.args.iter().all(|a| !a.is_placeholder) {
                    let values: Vec<String> = rule.args.iter().map(|a| a.value.clone()).collect();
                    if let Err(msg) = calendar_args::parse_window_args(&values) {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: msg,
                            span: rule.span.into(),
                        });
                    }
                }
            }
            "monthly" => {
                for arg in &rule.args {
                    if arg.is_placeholder {
                        continue;
                    }
                    match calendar_args::parse_monthly_arg(&arg.value) {
                        Err(msg) => diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: msg,
                            span: arg.span.into(),
                        }),
                        Ok(day) if !(1..=31).contains(&day) => diags.push(Diagnostic {
                            severity: Severity::Warning,
                            message: format!("day {day} can never match (valid days are 1–31)"),
                            span: arg.span.into(),
                        }),
                        Ok(_) => {}
                    }
                }
            }
            "annual" | "yearly" => {
                for arg in &rule.args {
                    if arg.is_placeholder {
                        continue;
                    }
                    match calendar_args::parse_annual_arg(&arg.value) {
                        Err(msg) => diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: msg,
                            span: arg.span.into(),
                        }),
                        Ok(calendar_args::AnnualArg::Ignored) => diags.push(Diagnostic {
                            severity: Severity::Warning,
                            message: format!(
                                "argument '{}' is ignored by the scheduler — expected MM-DD or YYYY-MM-DD",
                                arg.value
                            ),
                            span: arg.span.into(),
                        }),
                        Ok(calendar_args::AnnualArg::MonthDay(month, day))
                            if !(1..=12).contains(&month) || !(1..=31).contains(&day) =>
                        {
                            diags.push(Diagnostic {
                                severity: Severity::Warning,
                                message: format!("date {} can never match", arg.value),
                                span: arg.span.into(),
                            });
                        }
                        Ok(_) => {}
                    }
                }
            }
            "timezone" => {
                // timezone is handled specially
            }
            other => {
                diags.push(Diagnostic {
                    severity: Severity::Warning,
                    message: format!("unknown calendar rule type '{other}'"),
                    span: rule.rule_type.span.into(),
                });
            }
        }
    }
}

fn validate_runner_constraints(job: &JobBlock, diags: &mut Vec<Diagnostic>) {
    let mut requires = HashSet::new();
    let mut prefers = HashSet::new();
    let mut excludes = HashSet::new();
    let mut exec_block_seen = false;

    for dob in &job.directives {
        if let DirectiveOrBlock::Block(block) = dob
            && block.name.value == "runner"
        {
            match block.qualifier.as_ref().map(|q| q.value.as_str()) {
                None => {
                    for inner in &block.directives {
                        if let DirectiveOrBlock::Directive(d) = inner {
                            let cap = d.args.first().map(|a| a.value.as_str()).unwrap_or("");
                            match d.key.value.as_str() {
                                "require" => {
                                    requires.insert(cap.to_string());
                                }
                                "prefer" => {
                                    prefers.insert(cap.to_string());
                                }
                                "exclude" => {
                                    excludes.insert(cap.to_string());
                                }
                                "sticky" => {}
                                other => {
                                    diags.push(Diagnostic {
                                        severity: Severity::Warning,
                                        message: format!(
                                            "unknown runner directive '{other}' in job '{}'",
                                            job.key.raw
                                        ),
                                        span: d.key.span.into(),
                                    });
                                }
                            }
                        }
                    }
                }
                Some(kind @ ("shell" | "exec")) => {
                    if exec_block_seen {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: format!(
                                "job '{}' has more than one `runner shell|exec` block; only one execution payload is allowed",
                                job.key.raw
                            ),
                            span: block.name.span.into(),
                        });
                    }
                    exec_block_seen = true;
                    validate_runner_exec_block(job, kind, block, diags);
                }
                Some(other) => {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!(
                            "unknown runner type '{other}' in job '{}' \u{2014} expected `shell` or `exec`",
                            job.key.raw
                        ),
                        span: block
                            .qualifier
                            .as_ref()
                            .map(|q| q.span)
                            .unwrap_or(block.name.span)
                            .into(),
                    });
                }
            }
        }
    }

    // Check for overlaps
    for cap in requires.intersection(&excludes) {
        diags.push(Diagnostic {
            severity: Severity::Error,
            message: format!(
                "capability '{cap}' is both required and excluded in job '{}'",
                job.key.raw
            ),
            span: job.key.span.into(),
        });
    }
}

/// Validate the per-job concurrency guard directives (issue #278):
/// `singleton` (bare) and `max_concurrent N` are mutually exclusive, and
/// `max_concurrent` requires a positive integer argument.
///
/// Also rejects the guard on an `ephemeral` job (issue #302): ephemeral
/// executions are never persisted, so the claim-time concurrency guard — which
/// counts `Claimed` execution rows in the store — can never observe an
/// in-flight ephemeral run. `singleton` / `max_concurrent` therefore compiles
/// clean but is silently inert on an ephemeral job. `default_ephemeral` is the
/// running `defaults { execution_mode … }` baseline; the effective mode is
/// resolved exactly as `compile_job` does (default → schedule prefix →
/// `execution_mode` directive).
fn validate_concurrency(job: &JobBlock, default_ephemeral: bool, diags: &mut Vec<Diagnostic>) {
    let mut singleton_seen = false;
    let mut max_concurrent_seen = false;
    // Keyword + span of the first concurrency directive seen, reused for the
    // ephemeral-combo diagnostic so it points at the offending directive.
    let mut guard: Option<(&str, SourceSpan)> = None;

    for dob in &job.directives {
        let DirectiveOrBlock::Directive(d) = dob else {
            continue;
        };
        match d.key.value.as_str() {
            "singleton" => {
                if max_concurrent_seen {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!(
                            "`singleton` and `max_concurrent` are mutually exclusive (job '{}')",
                            job.key.raw
                        ),
                        span: d.key.span.into(),
                    });
                }
                singleton_seen = true;
                guard.get_or_insert(("singleton", d.key.span.into()));
            }
            "max_concurrent" => {
                if singleton_seen {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!(
                            "`singleton` and `max_concurrent` are mutually exclusive (job '{}')",
                            job.key.raw
                        ),
                        span: d.key.span.into(),
                    });
                }
                max_concurrent_seen = true;
                guard.get_or_insert(("max_concurrent", d.key.span.into()));

                match d.args.first() {
                    None => {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: format!(
                                "`max_concurrent` in job '{}' requires a positive integer argument",
                                job.key.raw
                            ),
                            span: d.key.span.into(),
                        });
                    }
                    // Placeholder values ({vars.X}, {env.X}) resolve at
                    // compile time — nothing to check statically here.
                    Some(arg) if arg.is_placeholder => {}
                    Some(arg) => match arg.value.parse::<u32>() {
                        Ok(0) => {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                message: format!(
                                    "`max_concurrent` in job '{}' must be greater than zero",
                                    job.key.raw
                                ),
                                span: arg.span.into(),
                            });
                        }
                        Ok(_) => {}
                        Err(_) => {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                message: format!(
                                    "`max_concurrent` in job '{}' requires a positive integer, got '{}'",
                                    job.key.raw, arg.value
                                ),
                                span: arg.span.into(),
                            });
                        }
                    },
                }
            }
            _ => {}
        }
    }

    // Issue #302: the guard is inert on ephemeral jobs — reject the combination
    // so it can't ship silently. Anchored on the first guard directive.
    if let Some((kw, span)) = guard
        && job_is_ephemeral(job, default_ephemeral)
    {
        diags.push(Diagnostic {
            severity: Severity::Error,
            message: format!(
                "`{kw}` has no effect on the `ephemeral` job '{}': ephemeral executions are not \
                 persisted, so the concurrency guard can never observe an in-flight run. Use \
                 `queued` for a real overlap guarantee, or drop `{kw}`.",
                job.key.raw
            ),
            span,
        });
    }
}

/// Whether a job's effective execution mode is `ephemeral`, mirroring
/// `compile_job`'s precedence: the `defaults` baseline (`default_ephemeral`),
/// overridden by a schedule prefix (`ephemeral` / `queued`), overridden last
/// by an `execution_mode` directive. Placeholder directive values can't be
/// resolved statically and read as non-ephemeral, matching compile's
/// `_ => Queued` fallback for anything that isn't the literal `ephemeral`.
fn job_is_ephemeral(job: &JobBlock, default_ephemeral: bool) -> bool {
    let mut ephemeral = default_ephemeral;
    if let Some(mode) = job.schedule.as_ref().and_then(|s| s.mode) {
        ephemeral = matches!(mode, ScheduleMode::Ephemeral);
    }
    for dob in &job.directives {
        if let DirectiveOrBlock::Directive(d) = dob
            && d.key.value == "execution_mode"
            && let Some(arg) = d.args.first()
        {
            ephemeral = arg.value == "ephemeral";
        }
    }
    ephemeral
}

fn validate_runner_exec_block(
    job: &JobBlock,
    kind: &str,
    block: &NamedBlock,
    diags: &mut Vec<Diagnostic>,
) {
    let mut has_command = false;
    let mut has_args = false;

    for inner in &block.directives {
        match inner {
            DirectiveOrBlock::Directive(d) => match d.key.value.as_str() {
                "command" => {
                    has_command = true;
                    if d.args.is_empty() {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: format!(
                                "`command` in job '{}' requires a string argument",
                                job.key.raw
                            ),
                            span: d.key.span.into(),
                        });
                    }
                }
                "args" => {
                    has_args = true;
                    if d.args.is_empty() {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: format!(
                                "`args` in job '{}' requires at least one argv entry",
                                job.key.raw
                            ),
                            span: d.key.span.into(),
                        });
                    }
                }
                "workdir" | "user" => {
                    if d.args.len() != 1 {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: format!(
                                "`{}` in job '{}' takes exactly one argument",
                                d.key.value, job.key.raw
                            ),
                            span: d.key.span.into(),
                        });
                    }
                }
                other => {
                    diags.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!(
                            "unknown directive '{other}' in `runner {kind}` block of job '{}'",
                            job.key.raw
                        ),
                        span: d.key.span.into(),
                    });
                }
            },
            DirectiveOrBlock::Block(inner) if inner.name.value == "env" => { /* validated by shape */
            }
            DirectiveOrBlock::Block(inner) => {
                diags.push(Diagnostic {
                    severity: Severity::Warning,
                    message: format!(
                        "unknown sub-block '{}' in `runner {kind}` of job '{}'",
                        inner.name.value, job.key.raw
                    ),
                    span: inner.name.span.into(),
                });
            }
            DirectiveOrBlock::Comment(_) => {}
        }
    }

    match kind {
        "shell" => {
            if !has_command {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: format!(
                        "`runner shell` in job '{}' requires `command \"…\"`",
                        job.key.raw
                    ),
                    span: block.name.span.into(),
                });
            }
            if has_args {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: format!(
                        "`args` is only valid in `runner exec`; use `command` for shell strings (job '{}')",
                        job.key.raw
                    ),
                    span: block.name.span.into(),
                });
            }
        }
        "exec" => {
            if !has_args {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: format!(
                        "`runner exec` in job '{}' requires `args <argv0> <argv1> …`",
                        job.key.raw
                    ),
                    span: block.name.span.into(),
                });
            }
            if has_command {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: format!(
                        "`command` is only valid in `runner shell`; use `args` for argv arrays (job '{}')",
                        job.key.raw
                    ),
                    span: block.name.span.into(),
                });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn validate_src(src: &str) -> Vec<Diagnostic> {
        let ast = Parser::parse(src).unwrap();
        validate(&ast)
    }

    #[test]
    fn valid_croniqfile() {
        let diags = validate_src(
            r#"
            calendar biz {
                include weekly monday tuesday wednesday thursday friday
            }
            job billing:invoice {
                every weekday at 09:00 { calendar biz }
                timeout 5m
            }
        "#,
        );
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    fn errors(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    fn warnings(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect()
    }

    #[test]
    fn calendar_weekly_alias_no_errors() {
        // #356: `weekday`/`weekend` are first-class — validate must
        // agree with the loader, which now compiles them.
        let diags = validate_src(
            r#"
            calendar biz { include weekly weekday }
            calendar off { include weekly weekend }
        "#,
        );
        assert!(errors(&diags).is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn calendar_weekly_bad_day_errors() {
        let diags = validate_src("calendar biz { include weekly funday }");
        assert!(
            errors(&diags)
                .iter()
                .any(|d| d.message == "unknown weekday: funday"),
            "got: {diags:?}"
        );
    }

    #[test]
    fn calendar_window_bad_time_errors() {
        let diags = validate_src(r#"calendar biz { include window "25:00".."26:00" }"#);
        assert!(
            errors(&diags)
                .iter()
                .any(|d| d.message == "invalid time: 25:00"),
            "got: {diags:?}"
        );
    }

    #[test]
    fn calendar_window_valid_ok() {
        let diags = validate_src(r#"calendar biz { include window "08:00".."18:00" }"#);
        assert!(errors(&diags).is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn calendar_monthly_non_numeric_errors() {
        let diags = validate_src("calendar biz { include monthly foo }");
        assert!(
            errors(&diags)
                .iter()
                .any(|d| d.message == "invalid day: foo"),
            "got: {diags:?}"
        );
    }

    #[test]
    fn calendar_monthly_out_of_range_warns() {
        // The loader accepts `monthly 45` (the rule is inert), so this
        // must stay a Warning — validate may not be stricter.
        let diags = validate_src("calendar biz { include monthly 45 }");
        assert!(errors(&diags).is_empty(), "unexpected errors: {diags:?}");
        assert!(
            warnings(&diags)
                .iter()
                .any(|d| d.message.contains("can never match")),
            "got: {diags:?}"
        );
    }

    #[test]
    fn calendar_annual_bad_format_errors() {
        let diags = validate_src(
            r#"
            calendar biz {
                exclude annual notadate
                exclude annual 2026-02-30
            }
        "#,
        );
        let errs = errors(&diags);
        assert!(
            errs.iter()
                .any(|d| d.message.contains("invalid annual date format"))
        );
        assert!(errs.iter().any(|d| d.message == "invalid date: 2026-02-30"));
    }

    #[test]
    fn calendar_annual_implausible_warns() {
        // `13-45` parses at load time and simply never matches.
        let diags = validate_src("calendar biz { exclude annual 13-45 }");
        assert!(errors(&diags).is_empty(), "unexpected errors: {diags:?}");
        assert!(
            warnings(&diags)
                .iter()
                .any(|d| d.message == "date 13-45 can never match"),
            "got: {diags:?}"
        );
    }

    #[test]
    fn calendar_annual_ignored_arg_warns() {
        // Len-5 args with too many dashes are silently skipped by the
        // loader — surface that as a Warning, not an Error.
        let diags = validate_src("calendar biz { exclude annual 1-2-3 }");
        assert!(errors(&diags).is_empty(), "unexpected errors: {diags:?}");
        assert!(
            warnings(&diags)
                .iter()
                .any(|d| d.message.contains("is ignored by the scheduler")),
            "got: {diags:?}"
        );
    }

    #[test]
    fn calendar_weekly_placeholder_skipped() {
        // Placeholders resolve at compile time — no diagnostics.
        let diags = validate_src(
            r#"
            vars { days "weekday" }
            calendar biz { include weekly {vars.days} }
        "#,
        );
        assert!(errors(&diags).is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn duplicate_job_key() {
        let diags = validate_src(
            r#"
            job etl:sync { every 5 minutes; timeout 5m }
            job etl:sync { every 10 minutes; timeout 5m }
        "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate job key"))
        );
    }

    #[test]
    fn undefined_calendar_ref() {
        let diags = validate_src(
            r#"
            job etl:sync {
                every day at 02:00 { calendar nonexistent }
                timeout 5m
            }
        "#,
        );
        assert!(diags.iter().any(|d| d.message.contains("not defined")));
    }

    #[test]
    fn conflicting_runner_constraints() {
        let diags = validate_src(
            r#"
            job etl:sync {
                every 5 minutes
                runner { require billing; exclude billing }
                timeout 5m
            }
        "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("required and excluded"))
        );
    }

    // ── Sub-30s interval warnings ─────────────────────────────────────────────

    #[test]
    fn interval_10_seconds_emits_warning() {
        let diags = validate_src(r#"job ops:ping { every 10 seconds; timeout 30s }"#);
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Warning && d.message.contains("10s")),
            "expected a sub-30s warning, got: {diags:?}"
        );
    }

    #[test]
    fn interval_29_seconds_emits_warning() {
        let diags = validate_src(r#"job ops:ping { every 29 seconds; timeout 30s }"#);
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "expected a warning for 29s interval"
        );
    }

    #[test]
    fn interval_30_seconds_no_warning() {
        let diags = validate_src(r#"job ops:ping { every 30 seconds; timeout 30s }"#);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning && d.message.contains("poll cycle"))
            .collect();
        assert!(
            warnings.is_empty(),
            "unexpected poll-cycle warning for 30s: {warnings:?}"
        );
    }

    #[test]
    fn interval_5_minutes_no_warning() {
        let diags = validate_src(r#"job etl:sync { every 5 minutes; timeout 5m }"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        let poll_warns: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning && d.message.contains("poll cycle"))
            .collect();
        assert!(
            poll_warns.is_empty(),
            "unexpected poll-cycle warning for 5m"
        );
    }

    #[test]
    fn interval_1_hour_no_warning() {
        let diags = validate_src(r#"job reports:daily { every 1 hours; timeout 30m }"#);
        assert!(
            !diags.iter().any(|d| d.message.contains("poll cycle")),
            "should not warn for hourly intervals"
        );
    }

    #[test]
    fn interval_1_second_emits_warning() {
        let diags = validate_src(r#"job debug:tick { every 1 seconds; timeout 5s }"#);
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Warning && d.message.contains("1s")),
            "expected a sub-30s warning for 1s interval"
        );
    }

    // ── runner shell / runner exec ────────────────────────────────────────────

    #[test]
    fn runner_shell_with_command_validates() {
        let diags = validate_src(
            r#"
            job ops:dump {
                every day at 03:00
                runner shell { command "echo hi" }
            }
        "#,
        );
        let errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    }

    #[test]
    fn runner_shell_without_command_errors() {
        let diags = validate_src(
            r#"
            job ops:dump {
                every day at 03:00
                runner shell { workdir /opt }
            }
        "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains("requires `command")),
            "expected `requires command` error, got: {diags:?}"
        );
    }

    #[test]
    fn runner_exec_without_args_errors() {
        let diags = validate_src(
            r#"
            job ops:rotate {
                every 1 hour
                runner exec { workdir /opt }
            }
        "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains("requires `args")),
            "expected `requires args` error, got: {diags:?}"
        );
    }

    #[test]
    fn runner_shell_rejects_args_directive() {
        let diags = validate_src(
            r#"
            job ops:dump {
                every 1 hour
                runner shell {
                    command "echo hi"
                    args /bin/echo
                }
            }
        "#,
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error
                && d.message.contains("only valid in `runner exec`")),
            "expected mutually-exclusive error, got: {diags:?}"
        );
    }

    #[test]
    fn unknown_runner_qualifier_errors() {
        let diags = validate_src(
            r#"
            job ops:weird {
                every 1 hour
                runner http { url "https://example.com" }
            }
        "#,
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error
                && d.message.contains("unknown runner type 'http'")),
            "expected unknown-qualifier error, got: {diags:?}"
        );
    }

    #[test]
    fn duplicate_runner_exec_blocks_error() {
        let diags = validate_src(
            r#"
            job ops:dump {
                every 1 hour
                runner shell { command "echo a" }
                runner exec { args /bin/echo b }
            }
        "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains("more than one")),
            "expected duplicate-exec error, got: {diags:?}"
        );
    }

    // ── singleton / max_concurrent (issue #278) ───────────────────────────────

    #[test]
    fn singleton_alone_is_valid() {
        let diags = validate_src(r#"job etl:sync { every 5 minutes; singleton }"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn max_concurrent_alone_is_valid() {
        let diags = validate_src(r#"job etl:sync { every 5 minutes; max_concurrent 3 }"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn singleton_and_max_concurrent_are_mutually_exclusive() {
        let diags = validate_src(
            r#"
            job etl:sync {
                every 5 minutes
                singleton
                max_concurrent 3
            }
        "#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains("mutually exclusive")),
            "expected mutual-exclusion error, got: {diags:?}"
        );
    }

    #[test]
    fn max_concurrent_zero_errors() {
        let diags = validate_src(r#"job etl:sync { every 5 minutes; max_concurrent 0 }"#);
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains("greater than zero")),
            "expected zero-value error, got: {diags:?}"
        );
    }

    #[test]
    fn max_concurrent_non_numeric_errors() {
        let diags = validate_src(r#"job etl:sync { every 5 minutes; max_concurrent many }"#);
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains("positive integer")),
            "expected non-numeric error, got: {diags:?}"
        );
    }

    #[test]
    fn max_concurrent_without_argument_errors() {
        let diags = validate_src(r#"job etl:sync { every 5 minutes; max_concurrent }"#);
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error
                && d.message.contains("requires a positive integer argument")),
            "expected missing-argument error, got: {diags:?}"
        );
    }

    // ── ephemeral + concurrency guard (issue #302) ────────────────────────────

    /// True when any diagnostic is the ephemeral/guard-combo rejection.
    fn has_ephemeral_guard_error(diags: &[Diagnostic]) -> bool {
        diags.iter().any(|d| {
            d.severity == Severity::Error && d.message.contains("no effect on the `ephemeral` job")
        })
    }

    #[test]
    fn ephemeral_prefix_with_singleton_errors() {
        let diags = validate_src(r#"job beat:tick { ephemeral every 1 minute; singleton }"#);
        assert!(
            has_ephemeral_guard_error(&diags),
            "singleton on an ephemeral job must be rejected, got: {diags:?}"
        );
    }

    #[test]
    fn ephemeral_prefix_with_max_concurrent_errors() {
        let diags = validate_src(r#"job beat:tick { ephemeral every 1 minute; max_concurrent 3 }"#);
        assert!(
            has_ephemeral_guard_error(&diags),
            "max_concurrent on an ephemeral job must be rejected, got: {diags:?}"
        );
    }

    #[test]
    fn ephemeral_directive_with_singleton_errors() {
        let diags = validate_src(
            r#"job beat:tick { every 1 minute; execution_mode ephemeral; singleton }"#,
        );
        assert!(
            has_ephemeral_guard_error(&diags),
            "execution_mode ephemeral + singleton must be rejected, got: {diags:?}"
        );
    }

    #[test]
    fn ephemeral_default_with_singleton_errors() {
        // Ephemeral inherited from a `defaults` block declared before the job.
        let diags = validate_src(
            r#"
            defaults { execution_mode ephemeral }
            job beat:tick { every 1 minute; singleton }
        "#,
        );
        assert!(
            has_ephemeral_guard_error(&diags),
            "singleton on a defaults-ephemeral job must be rejected, got: {diags:?}"
        );
    }

    #[test]
    fn ephemeral_without_guard_is_valid() {
        let diags = validate_src(r#"job beat:tick { ephemeral every 1 minute }"#);
        assert!(
            !has_ephemeral_guard_error(&diags),
            "a plain ephemeral job must not be rejected, got: {diags:?}"
        );
    }

    #[test]
    fn queued_with_singleton_has_no_ephemeral_error() {
        // The default (queued) job keeps the guard — no ephemeral rejection.
        let diags = validate_src(r#"job etl:sync { every 1 minute; singleton }"#);
        assert!(
            !has_ephemeral_guard_error(&diags),
            "queued + singleton must not trigger the ephemeral rejection, got: {diags:?}"
        );
    }

    #[test]
    fn queued_prefix_overrides_ephemeral_default_no_error() {
        // A `queued` schedule prefix overrides an ephemeral default, so the
        // guard is valid again — mirrors compile_job's precedence.
        let diags = validate_src(
            r#"
            defaults { execution_mode ephemeral }
            job etl:sync { queued every 1 minute; singleton }
        "#,
        );
        assert!(
            !has_ephemeral_guard_error(&diags),
            "queued prefix must un-reject the guard, got: {diags:?}"
        );
    }

    #[test]
    fn execution_mode_queued_directive_overrides_ephemeral_default_no_error() {
        let diags = validate_src(
            r#"
            defaults { execution_mode ephemeral }
            job etl:sync { every 1 minute; execution_mode queued; singleton }
        "#,
        );
        assert!(
            !has_ephemeral_guard_error(&diags),
            "execution_mode queued must un-reject the guard, got: {diags:?}"
        );
    }

    #[test]
    fn defaults_after_job_does_not_reject_earlier_job() {
        // A defaults block declared *after* a job does not apply to it, matching
        // compile_job's in-order handling — so no false-positive rejection.
        let diags = validate_src(
            r#"
            job etl:sync { every 1 minute; singleton }
            defaults { execution_mode ephemeral }
        "#,
        );
        assert!(
            !has_ephemeral_guard_error(&diags),
            "a later defaults block must not retroactively reject an earlier job, got: {diags:?}"
        );
    }
}
