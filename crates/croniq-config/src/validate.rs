//! Semantic validation of a parsed Croniqfile AST.

use crate::ast::*;
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

/// Validate a Croniqfile AST, returning errors and warnings.
pub fn validate(ast: &Croniqfile) -> Vec<Diagnostic> {
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

    // Second pass: validate everything
    for item in &ast.items {
        match item {
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
                if let Some(ref sched) = job.schedule {
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
            }
            Item::Calendar(cal) => {
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

    for rule in &cal.rules {
        match rule.rule_type.value.as_str() {
            "weekly" => {
                for arg in &rule.args {
                    if Weekday::parse(&arg.value).is_none()
                        && arg.value != "weekday"
                        && arg.value != "weekend"
                    {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            message: format!("invalid day name '{}'", arg.value),
                            span: arg.span.into(),
                        });
                    }
                }
            }
            "window" => {
                // Expect exactly 2 time args or 1 range arg
                if rule.args.is_empty() {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: "window rule requires start and end times".into(),
                        span: rule.span.into(),
                    });
                }
            }
            "annual" | "yearly" | "monthly" => {
                // Date format validation could go here
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
}
