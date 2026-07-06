//! Subprocess execution for `runner shell { ... }` and `runner exec { ... }`.
//!
//! The actual SDK plumbing (poll/ack/lease) lives in `croniq-runner-sdk`;
//! this module is just the shell/exec dispatcher that the runner binary
//! plugs in as a job handler.
//!
//! As of #118, stdout/stderr are no longer buffered until process exit —
//! each line is streamed live through the SDK's `LogWriter` so the
//! Execution Detail Logs panel renders chatty / long-running jobs as they
//! progress. A bounded rolling tail-buffer keeps the last lines around so
//! failure snippets in the dead-letter view stay meaningful.

use std::collections::VecDeque;
use std::process::Stdio;

use croniq_config::compile::RunnerExec;
use croniq_runner_sdk::{HandlerError, LogWriter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// How many lines of stdout AND stderr to retain in the rolling tail
/// buffer for failure-snippet assembly. The previous snippet was 400
/// chars of stderr; 50 lines comfortably covers that for typical line
/// lengths while bounding memory to ~50 KB / stream worst-case.
const TAIL_BUFFER_LINES: usize = 50;

/// How many trailing characters to splice into the failure-snippet that
/// becomes the Result::Err message. Matches the pre-streaming behaviour
/// so dead-letter UI snippets look identical to v0.11.0.
const FAILURE_SNIPPET_CHARS: usize = 400;

#[derive(Debug)]
pub struct Outcome {
    pub status: std::process::ExitStatus,
    /// Last [`TAIL_BUFFER_LINES`] lines of stdout, in chronological
    /// order. Older lines have already been streamed to the server but
    /// are no longer retained here.
    pub stdout_tail: VecDeque<String>,
    /// Last [`TAIL_BUFFER_LINES`] lines of stderr, same semantics.
    pub stderr_tail: VecDeque<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(
        "metadata is missing the `__runner_exec` payload — is this job actually declared with `runner shell {{}}` or `runner exec {{}}`?"
    )]
    MissingExec,

    #[error("metadata `__runner_exec` is not a string: {0}")]
    NotAString(serde_json::Value),

    #[error("metadata `__runner_exec` is malformed JSON: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("failed to spawn subprocess: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("failed while waiting for subprocess: {0}")]
    Wait(#[source] std::io::Error),

    #[error("`runner exec` requires a non-empty `args` list")]
    EmptyArgv,
}

/// Decode the `__runner_exec` payload out of the work-assignment metadata.
pub fn decode(metadata: &serde_json::Value) -> Result<RunnerExec, RunError> {
    let raw = metadata
        .get(croniq_config::compile::RUNNER_EXEC_METADATA_KEY)
        .ok_or(RunError::MissingExec)?;
    let s = raw
        .as_str()
        .ok_or_else(|| RunError::NotAString(raw.clone()))?;
    serde_json::from_str::<RunnerExec>(s).map_err(RunError::ParseError)
}

/// Build a `tokio::process::Command` from a `RunnerExec`.
///
/// Public so callers (tests, future runtime adapters) can inspect the
/// configured command before actually spawning it.
pub fn build_command(exec: &RunnerExec) -> Result<Command, RunError> {
    let (mut cmd, workdir, user, env) = match exec {
        RunnerExec::Shell {
            command,
            workdir,
            user,
            env,
        } => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            (c, workdir, user, env)
        }
        RunnerExec::Exec {
            argv,
            workdir,
            user,
            env,
        } => {
            let argv0 = argv.first().ok_or(RunError::EmptyArgv)?;
            let mut c = Command::new(argv0);
            if argv.len() > 1 {
                c.args(&argv[1..]);
            }
            (c, workdir, user, env)
        }
    };

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    cmd.env_clear();
    for (k, v) in std::env::vars() {
        // Inherit the bare minimum from the runner's own environment so that
        // `PATH`, `HOME`, locale, etc. work; user-supplied env in the Croniqfile
        // overrides any of these by being applied afterwards.
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }

    if let Some(u) = user
        && !u.is_empty()
    {
        // Best-effort: only a numeric uid is honoured. Building a full
        // user-resolution dance would mean linking nss/libc which the runner
        // image deliberately avoids.
        #[cfg(unix)]
        {
            if let Ok(uid) = u.parse::<u32>() {
                cmd.uid(uid);
            } else {
                tracing::warn!(
                    user = %u,
                    "`user` was not a numeric uid — set a numeric value (e.g. `user 0`) or \
                     drop the directive and run the runner container as the desired user."
                );
            }
        }
        #[cfg(not(unix))]
        {
            tracing::warn!(user = %u, "`user` is only honoured on unix targets");
        }
    }

    Ok(cmd)
}

/// Spawn the subprocess and stream stdout / stderr line-by-line through
/// the runner SDK's [`LogWriter`].
///
/// Each line is:
///
/// 1. Mirrored to the runner's own container logs via `tracing::info!`
///    (target `shell_runner::stdout` / `shell_runner::stderr`) so a
///    sidecar log shipper (Loki, Promtail, CloudWatch agent) picks it
///    up alongside the runner's lifecycle messages.
/// 2. Appended to a rolling tail buffer capped at [`TAIL_BUFFER_LINES`]
///    so [`outcome_to_handler_result`] can build a meaningful snippet
///    for the failure path.
/// 3. Forwarded to the streaming `LogWriter`, which batches and POSTs
///    to the server without blocking the reader on HTTP.
///
/// Backpressure for genuinely slow servers propagates from the writer's
/// bounded channel back through the reader → OS pipe → child process
/// `write()`, which is the safe degraded mode (vs. v0.11.0's pattern-B
/// per-line `ctx.log().await` deadlock potential, per issue #115).
pub async fn run(exec: &RunnerExec, writer: &LogWriter) -> Result<Outcome, RunError> {
    let mut cmd = build_command(exec)?;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(RunError::Spawn)?;
    let stdout = child
        .stdout
        .take()
        .expect("stdout pipe must be present after Stdio::piped()");
    let stderr = child
        .stderr
        .take()
        .expect("stderr pipe must be present after Stdio::piped()");

    let stdout_task = tokio::spawn(stream_lines(stdout, writer.clone(), Stream::Stdout));
    let stderr_task = tokio::spawn(stream_lines(stderr, writer.clone(), Stream::Stderr));

    // Wait for the child to exit. Once it does the kernel closes its
    // end of the pipes; the reader tasks see EOF and finish naturally.
    let status = child.wait().await.map_err(RunError::Wait)?;

    let stdout_tail = match stdout_task.await {
        Ok(Ok(tail)) => tail,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "stdout reader errored — tail buffer may be incomplete");
            VecDeque::new()
        }
        Err(e) => {
            tracing::warn!(error = %e, "stdout reader task panicked");
            VecDeque::new()
        }
    };
    let stderr_tail = match stderr_task.await {
        Ok(Ok(tail)) => tail,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "stderr reader errored — tail buffer may be incomplete");
            VecDeque::new()
        }
        Err(e) => {
            tracing::warn!(error = %e, "stderr reader task panicked");
            VecDeque::new()
        }
    };

    Ok(Outcome {
        status,
        stdout_tail,
        stderr_tail,
    })
}

/// Which pipe the reader task is consuming. Decides the log level used
/// for [`LogWriter::send`] and the tracing target name.
#[derive(Copy, Clone)]
enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn level(self) -> &'static str {
        match self {
            Stream::Stdout => "info",
            Stream::Stderr => "warn",
        }
    }

    fn tracing_target(self) -> &'static str {
        match self {
            Stream::Stdout => "shell_runner::stdout",
            Stream::Stderr => "shell_runner::stderr",
        }
    }
}

/// Reader task body. Pulls lines from one pipe, streams them through
/// the writer + tracing, and returns the rolling tail buffer.
async fn stream_lines<R>(
    reader: R,
    writer: LogWriter,
    stream: Stream,
) -> std::io::Result<VecDeque<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL_BUFFER_LINES);
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        // 1. Operator-visible container log.
        tracing::info!(target: "shell_runner::pipe", stream = stream.tracing_target(), line = %line);
        // 2. Rolling tail buffer for the failure snippet.
        if tail.len() == TAIL_BUFFER_LINES {
            tail.pop_front();
        }
        tail.push_back(line.clone());
        // 3. Stream to the server-side execution log panel. Awaits only
        //    on bounded channel capacity, never on HTTP (per LogWriter
        //    contract documented in croniq-runner-sdk::log_writer).
        writer.send(stream.level(), line).await;
    }
    Ok(tail)
}

/// Convert an `Outcome` into a runner-SDK `Result`. A non-zero exit becomes a
/// `HandlerError` whose message is short enough to fit in the execution log
/// row but informative enough to debug from.
///
/// Each individual line was already streamed via the `LogWriter`, so this
/// function does **not** re-emit stdout/stderr. Operators see live progress
/// in the Logs panel; the runner's own `tracing::info!` per-line in
/// [`stream_lines`] handles container-log visibility.
pub fn outcome_to_handler_result(outcome: Outcome, _job_key: &str) -> Result<(), HandlerError> {
    if outcome.status.success() {
        Ok(())
    } else {
        let code = outcome
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".into());
        let snippet = build_failure_snippet(&outcome.stderr_tail);
        if snippet.is_empty() {
            Err(HandlerError::msg(format!("exit {code}")))
        } else {
            Err(HandlerError::msg(format!("exit {code}: {snippet}")))
        }
    }
}

/// Reconstruct a trailing snippet from the rolling stderr tail buffer.
/// Joins the buffer with `\n`, then takes the last
/// [`FAILURE_SNIPPET_CHARS`] characters — char-aware to avoid splitting
/// multi-byte UTF-8 sequences (the old impl used the same `chars().rev()`
/// trick).
fn build_failure_snippet(tail: &VecDeque<String>) -> String {
    let joined: String = tail
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = joined.trim_end();
    trimmed
        .chars()
        .rev()
        .take(FAILURE_SNIPPET_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // The `RunnerExec::Shell` arm hardcodes `sh -c` (the runner ships in a
    // Linux container), and stock Windows has no `sh` on PATH — so every
    // test that spawns through the Shell arm is unix-only. The Exec arm is
    // genuinely cross-platform and stays tested everywhere, as do the
    // pure-logic tests (decode, argv validation, snippet assembly).

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_command_runs_and_returns_stdout_tail() {
        let exec = RunnerExec::Shell {
            command: "echo croniq-shell-runner".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null()).await.expect("spawn ok");
        assert!(
            outcome.status.success(),
            "exit status: {:?}",
            outcome.status
        );
        assert!(
            outcome
                .stdout_tail
                .iter()
                .any(|l| l.contains("croniq-shell-runner")),
            "stdout_tail: {:?}",
            outcome.stdout_tail
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_command_failure_propagates() {
        let exec = RunnerExec::Shell {
            command: "exit 7".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null()).await.expect("spawn ok");
        assert!(!outcome.status.success());
        assert_eq!(outcome.status.code(), Some(7));
    }

    #[tokio::test]
    async fn exec_runs_argv_directly() {
        // Pick an echo that reliably exists on the host: /bin/echo is part
        // of POSIX coreutils (and always present in CI); Windows has no
        // /bin/echo, but `cmd /C echo` ships with every install.
        #[cfg(unix)]
        let argv = vec!["/bin/echo".to_string(), "hello".into(), "exec".into()];
        #[cfg(windows)]
        let argv = vec![
            "cmd".to_string(),
            "/C".into(),
            "echo".into(),
            "hello".into(),
            "exec".into(),
        ];
        let exec = RunnerExec::Exec {
            argv,
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null()).await.expect("spawn ok");
        assert!(outcome.status.success());
        assert!(
            outcome.stdout_tail.iter().any(|l| l.contains("hello exec")),
            "stdout_tail: {:?}",
            outcome.stdout_tail
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn user_supplied_env_reaches_subprocess() {
        let mut env = HashMap::new();
        env.insert("CRONIQ_TEST_KEY".to_string(), "from-dsl".to_string());
        let exec = RunnerExec::Shell {
            command: "printf %s \"$CRONIQ_TEST_KEY\"".into(),
            workdir: None,
            user: None,
            env,
        };
        let outcome = run(&exec, &LogWriter::null()).await.expect("spawn ok");
        assert!(outcome.status.success());
        // `printf %s` produces a single line with no trailing newline.
        assert_eq!(
            outcome.stdout_tail.back().map(String::as_str),
            Some("from-dsl")
        );
    }

    #[test]
    fn empty_argv_is_rejected() {
        let exec = RunnerExec::Exec {
            argv: vec![],
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let err = build_command(&exec).unwrap_err();
        assert!(matches!(err, RunError::EmptyArgv));
    }

    #[test]
    fn decode_extracts_payload_from_metadata() {
        let payload = serde_json::json!({
            "kind": "shell",
            "command": "echo hi",
        });
        let metadata = serde_json::json!({
            croniq_config::compile::RUNNER_EXEC_METADATA_KEY: payload.to_string(),
            "month": "2026-05",
        });
        let exec = decode(&metadata).expect("decode ok");
        match exec {
            RunnerExec::Shell { command, .. } => assert_eq!(command, "echo hi"),
            other => panic!("expected Shell, got {other:?}"),
        }
    }

    #[test]
    fn decode_reports_missing_payload() {
        let metadata = serde_json::json!({});
        assert!(matches!(decode(&metadata), Err(RunError::MissingExec)));
    }

    // ─── Tail buffer + failure snippet (issue #118) ─────────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn tail_buffer_caps_at_configured_limit() {
        // Emit far more than TAIL_BUFFER_LINES so we can verify the cap
        // discards the oldest lines but keeps the newest ones.
        let count = TAIL_BUFFER_LINES + 10;
        let exec = RunnerExec::Shell {
            command: format!("for i in $(seq 1 {count}); do echo line $i; done"),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null()).await.expect("spawn ok");
        assert!(outcome.status.success());
        assert_eq!(
            outcome.stdout_tail.len(),
            TAIL_BUFFER_LINES,
            "tail buffer must cap at {TAIL_BUFFER_LINES}, got {}",
            outcome.stdout_tail.len()
        );
        // Newest lines retained — last one should be `line {count}`.
        assert_eq!(
            outcome.stdout_tail.back().map(String::as_str),
            Some(format!("line {count}").as_str())
        );
        // Oldest line is the (count - TAIL_BUFFER_LINES + 1)-th.
        let expected_oldest = count - TAIL_BUFFER_LINES + 1;
        assert_eq!(
            outcome.stdout_tail.front().map(String::as_str),
            Some(format!("line {expected_oldest}").as_str())
        );
    }

    #[test]
    fn build_failure_snippet_joins_lines_with_newlines() {
        let mut tail = VecDeque::new();
        tail.push_back("first".to_string());
        tail.push_back("second".to_string());
        tail.push_back("third".to_string());
        let snippet = build_failure_snippet(&tail);
        assert_eq!(snippet, "first\nsecond\nthird");
    }

    #[test]
    fn build_failure_snippet_truncates_to_last_400_chars() {
        // Build a tail that exceeds the 400-char snippet budget.
        let mut tail = VecDeque::new();
        for i in 0..30 {
            tail.push_back(format!("line {i} with some filler text to make it longer"));
        }
        let snippet = build_failure_snippet(&tail);
        assert!(snippet.len() <= FAILURE_SNIPPET_CHARS);
        // Snippet covers the tail end, not the head.
        assert!(snippet.ends_with(&tail[tail.len() - 1]));
    }

    #[test]
    fn build_failure_snippet_handles_empty_tail() {
        assert_eq!(build_failure_snippet(&VecDeque::new()), "");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn outcome_to_handler_result_failure_includes_stderr_snippet() {
        let exec = RunnerExec::Shell {
            command: "echo 'boom — something went wrong' 1>&2; exit 2".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null()).await.expect("spawn ok");
        let err = outcome_to_handler_result(outcome, "test:job").unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("exit 2"), "msg: {msg}");
        assert!(msg.contains("boom"), "stderr snippet missing: {msg}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn outcome_to_handler_result_success_returns_ok() {
        let exec = RunnerExec::Shell {
            command: "true".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null()).await.expect("spawn ok");
        assert!(outcome_to_handler_result(outcome, "test:job").is_ok());
    }
}
