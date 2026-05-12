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

use croniq_runner_sdk::{CroniqRunner, ExecutionContext, HandlerError, resolve_runner_id};
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

    // Acquire the streaming log writer up-front; `exec::run` feeds each
    // stdout/stderr line into it as the subprocess emits them. The
    // runner SDK awaits the writer's drain (up to 5 s) before sending
    // `ack`, so all queued events are server-side by the time the
    // execution is marked complete (#115 / #117 / #118).
    let writer = ctx.log_writer();

    let outcome = exec::run(&exec, &writer)
        .await
        .map_err(|e| HandlerError::msg(format!("exec failed: {e}")))?;

    exec::outcome_to_handler_result(outcome, &ctx.job_key)
}
