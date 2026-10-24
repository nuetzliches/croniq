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
//!   `RUNNER_DRAIN_TIMEOUT_SECS` — on SIGTERM / Ctrl-C, how long running jobs
//!                               may finish before they are terminated and
//!                               acked as failed (default: 8, inside Docker's
//!                               10 s stop grace; raise both together).
//!   `CRONIQ_ALLOW_INSECURE_HTTP` — `1` accepts a cleartext `http://` server
//!                               URL on a non-loopback host, such as
//!                               `http://server:4000` on a Compose network.
//!                               Without it the runner refuses to start: the
//!                               API key would travel in the clear.
//!   `RUST_LOG`                — log filter. Every line a job prints is also
//!                               logged under the target `shell_runner::pipe`;
//!                               `RUST_LOG=info,shell_runner::pipe=off` keeps
//!                               job output out of the runner's own log (it
//!                               still reaches the execution's log panel).
//!
//! Deployment (issue #577): this binary usually has to run *inside* the image
//! that carries the tooling it invokes, not inside Croniq's own image. The
//! release publishes statically linked `*-unknown-linux-musl` archives for
//! that: they need no runtime libraries, so they drop into an Alpine-based
//! tool image as-is. The glibc archives and the binaries inside
//! `ghcr.io/nuetzliches/croniq` are dynamically linked and will not start on
//! musl — `gcompat` resolves the libraries and then fails on
//! `gnu_get_libc_version` / `__res_init`. See the "Generic shell runner"
//! section of the README for a Dockerfile.
//!
//! Cancel and timeout semantics (issue #576): the runner terminates the
//! command's process group rather than orphaning it. Cancelling an execution
//! in the UI sends the group SIGTERM and, if it is still there 5 s later,
//! SIGKILL; the job's own `timeout` is enforced here the same way. So a
//! command gets a window to clean up but does not outlive its execution. On
//! Windows only the spawned process itself is terminated — a process it
//! spawned in turn survives.

use std::time::Duration;

use croniq_execution::retry::parse_duration;
use croniq_runner_sdk::{
    CroniqRunner, ExecutionContext, HandlerError, resolve_runner_id, shutdown_signal,
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

    // Every `runner shell { … }` command inherits this process's privileges, so
    // a root runner is a root shell for whoever can edit the Croniqfile. The
    // image no longer starts as root (review 2026-10-24, R-1); say so loudly
    // when a deployment does it anyway. Not a refusal: the `user` directive
    // needs a root parent to switch to, and that is a legitimate setup.
    #[cfg(unix)]
    // SAFETY: `geteuid` cannot fail and touches no memory.
    if unsafe { libc::geteuid() } == 0 {
        tracing::warn!(
            "the shell runner is running as root — every job without a `user` directive runs \
             as root too. Run it as an unprivileged user unless jobs need `user` to switch \
             accounts."
        );
    }

    let mut builder = CroniqRunner::builder(&server_url, &runner_id)
        .capabilities(capabilities)
        .tags(tags)
        .max_inflight(max_inflight)
        .drain_timeout(drain_timeout_from_env());

    if let Ok(key) = std::env::var("CRONIQ_API_KEY") {
        builder = builder.api_key(&key);
    }
    if env_flag("CRONIQ_ALLOW_INSECURE_HTTP") {
        builder = builder.allow_insecure_http();
    }

    let runner = builder.build();

    runner.set_default_handler(handle_job).await;

    // SIGTERM / Ctrl-C drain the runner instead of killing it mid-job (review
    // 2026-10-24, R-4). Before, `docker stop` ended the process at once: no
    // ack, claims stuck until the reaper requeued them, and outside a
    // container the jobs — in their own process groups — kept running and
    // then ran a second time.
    if let Err(e) = runner.run_until(shutdown_signal()).await {
        tracing::error!(error = %e, "runner exited with error");
        std::process::exit(1);
    }
}

/// Default for `RUNNER_DRAIN_TIMEOUT_SECS`: inside Docker's 10 s stop grace,
/// with room for the final acks.
const DEFAULT_DRAIN_TIMEOUT_SECS: u64 = 8;

fn drain_timeout_from_env() -> Duration {
    let secs = std::env::var("RUNNER_DRAIN_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(DEFAULT_DRAIN_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
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

    // Enforce the execution's own `timeout` (issue #576). The server sends it
    // as a duration string and, until #576, nobody acted on it: the runner
    // ignored it, and the server-side stale-claim reaper documents itself as a
    // safety net for a *lost* runner rather than as the primary mechanism — so
    // a hung command was reaped and requeued while its first copy kept running.
    //
    // A value that does not parse is not worth failing the job over (the reaper
    // is still behind it) but is worth saying out loud, because it silently
    // removes the only in-process limit on the command.
    let timeout = match parse_duration(&ctx.timeout) {
        // Zero means "no limit", not "kill it immediately".
        Some(d) if d.is_zero() => None,
        Some(d) => Some(d),
        None => {
            tracing::warn!(
                job_key = %ctx.job_key,
                execution_id = %ctx.execution_id,
                timeout = %ctx.timeout,
                "could not parse the execution timeout — running the command unbounded; the \
                 server-side stale-claim reaper is the only remaining backstop"
            );
            None
        }
    };

    let outcome = exec::run(&exec, &writer, timeout)
        .await
        .map_err(|e| HandlerError::msg(format!("exec failed: {e}")))?;

    exec::outcome_to_handler_result(outcome, &ctx.job_key)
}
