//! Failure notification hooks.
//!
//! Executes a configurable shell command when executions fail permanently
//! (dead-lettered or dropped). The command receives job details via environment
//! variables.
//!
//! Configuration via environment variable:
//!   CRONIQ_ON_FAILURE_CMD="curl -X POST https://hooks.slack.com/... -d '{\"text\": \"Job $CRONIQ_JOB_KEY failed\"}'"
//!
//! Available env vars passed to the command:
//!   CRONIQ_JOB_KEY, CRONIQ_EXECUTION_ID, CRONIQ_ERROR, CRONIQ_ATTEMPT, CRONIQ_REASON

use std::process::Command;

/// Run the failure notification hook if configured.
pub fn notify_failure(job_key: &str, execution_id: &str, error: &str, attempt: u32, reason: &str) {
    let Some(cmd) = std::env::var("CRONIQ_ON_FAILURE_CMD")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        return;
    };

    let result = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .env("CRONIQ_JOB_KEY", job_key)
        .env("CRONIQ_EXECUTION_ID", execution_id)
        .env("CRONIQ_ERROR", error)
        .env("CRONIQ_ATTEMPT", attempt.to_string())
        .env("CRONIQ_REASON", reason)
        .output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    job_key,
                    cmd = cmd.as_str(),
                    stderr = %stderr,
                    "failure notification hook exited with error"
                );
            } else {
                tracing::debug!(job_key, "failure notification hook executed");
            }
        }
        Err(e) => {
            tracing::warn!(
                job_key,
                cmd = cmd.as_str(),
                error = %e,
                "failed to execute notification hook"
            );
        }
    }
}
