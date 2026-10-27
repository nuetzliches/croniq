//! Croniq demo runner.
//!
//! Connects to a Croniq server, registers a set of demo jobs, and handles
//! them with a simulated workload: random sleep + configurable fail rate.
//!
//! Environment variables:
//!   CRONIQ_SERVER_URL       — server base URL  (default: http://localhost:4000)
//!   CRONIQ_API_KEY          — bearer token     (optional for open dev setups)
//!   RUNNER_ID               — explicit runner name override. If unset, the
//!                             runner reads/persists a stable ID at
//!                             `${CRONIQ_RUNNER_DATA_DIR}/runner-id` so the
//!                             same identity survives container recreates
//!                             (issue #103).
//!   CRONIQ_RUNNER_DATA_DIR  — directory for persistent runner state
//!                             (default: /var/lib/croniq-runner)
//!   RUNNER_FAIL_RATE        — fraction 0.0–1.0 that fail (default: 0.05)
//!   RUNNER_MAX_INFLIGHT     — concurrency cap  (default: 4)
//!   RUNNER_TAGS             — comma-separated free-form filter tags (optional)
//!   RUNNER_DRAIN_TIMEOUT_SECS — on SIGTERM / Ctrl-C, how long running jobs may
//!                             finish before they are aborted and acked as
//!                             failed (default: 8, inside Docker's 10 s grace)
//!   CRONIQ_ALLOW_INSECURE_HTTP — `1` accepts a cleartext http:// server URL on
//!                             a non-loopback host (the Compose demo sets it)

use std::time::Duration;

use croniq_runner_sdk::{
    CroniqRunner, ExecutionContext, HandlerError, resolve_runner_id, shutdown_signal,
};
use rand::RngExt as _; // for random_range
use tracing::{info, warn};

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
    let runner_id = resolve_runner_id("demo-runner");
    let fail_rate: f64 = std::env::var("RUNNER_FAIL_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.05);
    let max_inflight: u32 = std::env::var("RUNNER_MAX_INFLIGHT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    info!(
        server_url = %server_url,
        runner_id = %runner_id,
        fail_rate,
        max_inflight,
        "croniq demo runner starting"
    );

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

    let mut builder = CroniqRunner::builder(&server_url, &runner_id)
        .capabilities(vec!["demo".into()])
        .tags(tags)
        .max_inflight(max_inflight)
        .drain_timeout(Duration::from_secs(
            std::env::var("RUNNER_DRAIN_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(8),
        ));

    if let Ok(key) = std::env::var("CRONIQ_API_KEY") {
        builder = builder.api_key(&key);
    }
    if std::env::var("CRONIQ_ALLOW_INSECURE_HTTP")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
    {
        builder = builder.allow_insecure_http();
    }

    let runner = builder.build();

    // Register demo jobs with schedules. If the server already has these jobs
    // defined in a Croniqfile, the registration is silently skipped and the
    // DSL definition takes precedence.
    let demo_jobs = [
        ("demo:heartbeat", "every 1 minute"),
        ("demo:data-sync", "every 5 minutes"),
        ("demo:report", "every 15 minutes"),
        ("demo:cleanup", "every 1 hour"),
    ];

    for (job_key, schedule) in demo_jobs {
        let fr = fail_rate;
        runner
            .register_with_schedule(job_key, schedule, move |ctx| simulate(ctx, fr))
            .await;
    }

    // Catch-all: handles any job the server sends that isn't registered above.
    runner
        .set_default_handler(move |ctx: ExecutionContext| simulate(ctx, fail_rate))
        .await;

    // SIGTERM / Ctrl-C drain instead of dying mid-job (review 2026-10-24, R-4).
    if let Err(e) = runner.run_until(shutdown_signal()).await {
        tracing::error!(error = %e, "runner exited with error");
        std::process::exit(1);
    }
}

async fn simulate(ctx: ExecutionContext, fail_rate: f64) -> Result<(), HandlerError> {
    let sleep_ms = rand::rng().random_range(50u64..=2_000);
    info!(
        job_key = %ctx.job_key,
        execution_id = %ctx.execution_id,
        attempt = ctx.attempt,
        sleep_ms,
        "executing"
    );
    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;

    if rand::random::<f64>() < fail_rate {
        warn!(
            job_key = %ctx.job_key,
            attempt = ctx.attempt,
            "simulated failure"
        );
        return Err(HandlerError::msg(
            "simulated failure — set RUNNER_FAIL_RATE=0 to disable",
        ));
    }

    info!(job_key = %ctx.job_key, "done");
    Ok(())
}
