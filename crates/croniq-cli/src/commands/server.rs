//! CLI commands that talk to a running croniq-server over HTTP.

use croniq_runner::{HealthResponse, RunnerSummary, TriggerRequest, TriggerResponse};
use miette::{IntoDiagnostic, Result, miette};

// ─── status ──────────────────────────────────────────────────────────────────

/// `croniq status` — print scheduler health from the running server.
pub fn status(server_url: &str) -> Result<()> {
    let url = format!("{server_url}/health");
    let resp: HealthResponse = reqwest::blocking::get(&url)
        .map_err(|e| miette!("Could not connect to {url}: {e}"))?
        .json()
        .into_diagnostic()?;

    println!("Status:          {}", resp.status);
    println!("Queue depth:     {}", resp.queued);
    println!("Runners online:  {}", resp.runners_online);
    println!("Runners stale:   {}", resp.runners_stale);
    println!("Runners dead:    {}", resp.runners_dead);

    Ok(())
}

// ─── list-runners ─────────────────────────────────────────────────────────────

/// `croniq list-runners` — print all runners with their liveness status.
pub fn list_runners(server_url: &str) -> Result<()> {
    let url = format!("{server_url}/v1/runners");
    let runners: Vec<RunnerSummary> = reqwest::blocking::get(&url)
        .map_err(|e| miette!("Could not connect to {url}: {e}"))?
        .json()
        .into_diagnostic()?;

    if runners.is_empty() {
        println!("No runners connected.");
        return Ok(());
    }

    // Column header
    println!(
        "{:<24} {:<8} {:<10} {:<12} {}",
        "RUNNER ID", "STATUS", "INFLIGHT", "CAPACITY", "CAPABILITIES"
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
    server_url: &str,
    job_key: &str,
    require: Vec<String>,
    prefer: Vec<String>,
    timeout: &str,
) -> Result<()> {
    let url = format!("{server_url}/v1/trigger");
    let req = TriggerRequest {
        job_key: job_key.to_string(),
        require,
        prefer,
        metadata: serde_json::Value::Null,
        timeout: timeout.to_string(),
    };

    let resp: TriggerResponse = reqwest::blocking::Client::new()
        .post(&url)
        .json(&req)
        .send()
        .map_err(|e| miette!("Could not connect to {url}: {e}"))?
        .json()
        .into_diagnostic()?;

    println!("Triggered job '{job_key}'");
    println!("  execution_id: {}", resp.execution_id);
    println!("  queue depth:  {}", resp.queued);

    Ok(())
}
