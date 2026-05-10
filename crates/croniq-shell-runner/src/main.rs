//! Croniq generic shell runner binary.
//!
//! Connects to a Croniq server, registers itself as a runner with the
//! `shell-runner` capability, and runs jobs whose Croniqfile carries a
//! `runner shell { ... }` or `runner exec { ... }` block.
//!
//! Environment variables:
//!   `CRONIQ_SERVER_URL`       — server base URL  (default: http://localhost:4000)
//!   `CRONIQ_API_KEY`          — bearer token     (recommended)
//!   `RUNNER_ID`               — explicit runner name override. If unset, the
//!                               runner reads/persists a stable ID at
//!                               `${CRONIQ_RUNNER_DATA_DIR}/runner-id` so the
//!                               same identity survives container recreates
//!                               (issue #103). Mount a volume on this path to
//!                               make it stable, e.g.
//!                                 volumes:
//!                                   - croniq-runner-state:/var/lib/croniq-runner
//!   `CRONIQ_RUNNER_DATA_DIR`  — directory for persistent runner state
//!                               (default: /var/lib/croniq-runner)
//!   `RUNNER_MAX_INFLIGHT`     — concurrency cap  (default: 4)
//!   `RUNNER_CAPABILITIES`     — comma-separated extra capabilities to
//!                               advertise on top of the implicit
//!                               `shell-runner` capability.
//!   `RUNNER_TAGS`             — comma-separated free-form tags for filtering
//!                               in the UI. Not routing-relevant. Convention:
//!                               `key=value` strings (`env=prod`, `team=ops`).

use croniq_runner_sdk::{
    CroniqRunner, ExecutionContext, HandlerError, WorkEvent, resolve_runner_id,
};
use croniq_shell_runner::exec;
use tracing::info;

const IMPLICIT_CAPABILITY: &str = "shell-runner";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let server_url =
        std::env::var("CRONIQ_SERVER_URL").unwrap_or_else(|_| "http://localhost:4000".into());
    let runner_id = resolve_runner_id(IMPLICIT_CAPABILITY);
    let max_inflight: u32 = std::env::var("RUNNER_MAX_INFLIGHT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    let mut capabilities = vec![IMPLICIT_CAPABILITY.to_string()];
    if let Ok(extra) = std::env::var("RUNNER_CAPABILITIES") {
        for cap in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if cap != IMPLICIT_CAPABILITY {
                capabilities.push(cap.to_string());
            }
        }
    }

    let tags: Vec<String> = std::env::var("RUNNER_TAGS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    info!(
        server_url = %server_url,
        runner_id = %runner_id,
        ?capabilities,
        ?tags,
        max_inflight,
        "croniq shell runner starting"
    );

    let mut builder = CroniqRunner::builder(&server_url, &runner_id)
        .capabilities(capabilities)
        .tags(tags)
        .max_inflight(max_inflight);

    if let Ok(key) = std::env::var("CRONIQ_API_KEY") {
        builder = builder.api_key(&key);
    }

    let runner = builder.build();

    runner.set_default_handler(handle_job).await;

    if let Err(e) = runner.start().await {
        tracing::error!(error = %e, "runner exited with error");
        std::process::exit(1);
    }
}

async fn handle_job(ctx: ExecutionContext) -> Result<(), HandlerError> {
    let exec = match exec::decode(&ctx.metadata) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                job_key = %ctx.job_key,
                execution_id = %ctx.execution_id,
                error = %e,
                "skipping job — no `__runner_exec` payload"
            );
            return Err(HandlerError::msg(format!("payload error: {e}")));
        }
    };

    info!(
        job_key = %ctx.job_key,
        execution_id = %ctx.execution_id,
        attempt = ctx.attempt,
        "running"
    );

    let outcome = exec::run(&exec)
        .await
        .map_err(|e| HandlerError::msg(format!("exec failed: {e}")))?;

    // Push captured output to the server so the Execution Detail UI can show it.
    // One event per line so the UI can filter / search and downstream log
    // sinks (Loki, CloudWatch) get individually-indexed entries instead of
    // a single multi-KB blob. Failures here are non-fatal — just warn.
    let events = build_log_events(&outcome.stdout, &outcome.stderr);
    if !events.is_empty()
        && let Err(e) = ctx.push_log_events(&events).await
    {
        tracing::warn!(
            execution_id = %ctx.execution_id,
            error = %e,
            "failed to push log events — output is only in container logs"
        );
    }

    exec::outcome_to_handler_result(outcome, &ctx.job_key)
}

/// Split captured stdout/stderr into per-line `WorkEvent`s. Empty trailing
/// newlines are skipped; truly empty streams produce no events.
fn build_log_events(stdout: &str, stderr: &str) -> Vec<WorkEvent> {
    let mut events = Vec::new();
    push_lines_as_events(&mut events, stdout, "info");
    push_lines_as_events(&mut events, stderr, "warn");
    events
}

fn push_lines_as_events(events: &mut Vec<WorkEvent>, text: &str, level: &str) {
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        events.push(WorkEvent {
            level: Some(level.into()),
            message: line.to_string(),
            fields: Default::default(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_log_events_emits_one_event_per_line() {
        let events = build_log_events("first line\nsecond line\nthird\n", "");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].message, "first line");
        assert_eq!(events[0].level.as_deref(), Some("info"));
        assert_eq!(events[2].message, "third");
    }

    #[test]
    fn build_log_events_skips_empty_lines() {
        let events = build_log_events("a\n\n\nb\n", "");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].message, "a");
        assert_eq!(events[1].message, "b");
    }

    #[test]
    fn build_log_events_uses_warn_for_stderr() {
        let events = build_log_events("", "ERROR: boom\nstack trace");
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.level.as_deref() == Some("warn")));
    }

    #[test]
    fn build_log_events_orders_stdout_before_stderr() {
        let events = build_log_events("out", "err");
        assert_eq!(events[0].message, "out");
        assert_eq!(events[0].level.as_deref(), Some("info"));
        assert_eq!(events[1].message, "err");
        assert_eq!(events[1].level.as_deref(), Some("warn"));
    }

    #[test]
    fn build_log_events_returns_empty_for_silent_run() {
        assert!(build_log_events("", "").is_empty());
        assert!(build_log_events("\n\n", "").is_empty());
    }
}
