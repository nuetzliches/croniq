//! CLI commands that talk to a running croniq-server over HTTP.
//!
//! All requests go through [`Remote`], which attaches the credential and
//! turns a non-2xx into a message. Before #475 these issued naked requests
//! and called `.json()` on whatever came back, so a `401` from an
//! auth-enabled server surfaced as a serde decode error.

use croniq_runner::{HealthResponse, RunnerSummary, TriggerRequest, TriggerResponse};
use miette::Result;

use super::remote::Remote;

// ─── status ──────────────────────────────────────────────────────────────────

/// `croniq status` — print scheduler health from the running server.
pub fn status(remote: &Remote) -> Result<()> {
    let resp: HealthResponse = remote.get_json("/health")?;

    println!("Status:          {}", resp.status);
    println!("Queue depth:     {}", resp.queued);
    println!("Runners online:  {}", resp.runners_online);
    println!("Runners stale:   {}", resp.runners_stale);
    println!("Runners dead:    {}", resp.runners_dead);

    Ok(())
}

// ─── list-runners ─────────────────────────────────────────────────────────────

/// `croniq list-runners` — print all runners with their liveness status.
pub fn list_runners(remote: &Remote) -> Result<()> {
    let runners: Vec<RunnerSummary> = remote.get_json("/v1/runners")?;

    if runners.is_empty() {
        println!("No runners connected.");
        return Ok(());
    }

    // Column header
    println!(
        "{:<24} {:<8} {:<10} {:<12} CAPABILITIES",
        "RUNNER ID", "STATUS", "INFLIGHT", "CAPACITY"
    );
    println!("{}", "-".repeat(72));

    for r in &runners {
        use croniq_runner::RunnerStatus;
        let status_str = match r.status {
            RunnerStatus::Online => "online",
            RunnerStatus::Stale => "stale",
            RunnerStatus::Dead => "dead",
        };
        let caps = if r.capabilities.is_empty() {
            "(any)".to_string()
        } else {
            r.capabilities.join(", ")
        };
        println!(
            "{:<24} {:<8} {:<10} {:<12} {}",
            r.runner_id, status_str, r.inflight, r.max_inflight, caps
        );
    }

    Ok(())
}

// ─── trigger ─────────────────────────────────────────────────────────────────

/// `croniq trigger` — immediately fire a job by enqueuing it on the server.
pub fn trigger(
    remote: &Remote,
    job_key: &str,
    require: Vec<String>,
    prefer: Vec<String>,
    timeout: Option<String>,
) -> Result<()> {
    let req = TriggerRequest {
        job_key: job_key.to_string(),
        require,
        prefer,
        metadata: serde_json::Value::Null,
        // Left unset unless `--timeout` was passed, so the server applies the
        // job's own configured timeout (issue #551). Sending a default here
        // would read as an explicit override and cap every manual fire at it.
        timeout,
        idempotency_key: None,
    };

    let resp: TriggerResponse = remote.post_json("/v1/trigger", &req)?;

    println!("Triggered job '{job_key}'");
    println!("  execution_id: {}", resp.execution_id);
    println!("  queue depth:  {}", resp.queued);

    Ok(())
}
