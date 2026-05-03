//! Croniq generic shell runner binary.
//!
//! Connects to a Croniq server, registers itself as a runner with the
//! `shell-runner` capability, and runs jobs whose Croniqfile carries a
//! `runner shell { ... }` or `runner exec { ... }` block.
//!
//! Environment variables:
//!   `CRONIQ_SERVER_URL`    — server base URL  (default: http://localhost:4000)
//!   `CRONIQ_API_KEY`       — bearer token     (recommended)
//!   `RUNNER_ID`            — runner name      (default: shell-runner-<hostname>)
//!   `RUNNER_MAX_INFLIGHT`  — concurrency cap  (default: 4)
//!   `RUNNER_CAPABILITIES`  — comma-separated extra capabilities to advertise on
//!                            top of the implicit `shell-runner` capability.

use croniq_runner_sdk::{CroniqRunner, ExecutionContext, HandlerError};
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
    let runner_id = std::env::var("RUNNER_ID").unwrap_or_else(|_| {
        let suffix = std::env::var("HOSTNAME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "0".into());
        format!("shell-runner-{suffix}")
    });
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

    info!(
        server_url = %server_url,
        runner_id = %runner_id,
        ?capabilities,
        max_inflight,
        "croniq shell runner starting"
    );

    let mut builder = CroniqRunner::builder(&server_url, &runner_id)
        .capabilities(capabilities)
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

    exec::outcome_to_handler_result(outcome, &ctx.job_key)
}
