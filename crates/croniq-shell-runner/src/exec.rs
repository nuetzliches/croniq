//! Subprocess execution for `runner shell { ... }` and `runner exec { ... }`.
//!
//! The actual SDK plumbing (poll/ack/lease) lives in `croniq-runner-sdk`;
//! this module is just the shell/exec dispatcher that the runner binary
//! plugs in as a job handler.

use std::process::Stdio;

use croniq_config::compile::RunnerExec;
use croniq_runner_sdk::HandlerError;
use tokio::process::Command;

#[derive(Debug)]
pub struct Outcome {
    pub status: std::process::ExitStatus,
    pub stdout: String,
    pub stderr: String,
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

/// Spawn the subprocess, capture stdout / stderr, return an `Outcome`.
///
/// Note: stdout/stderr are captured in full and only returned at the end. For
/// long-running jobs this means the runner buffers all output in memory.
/// Streaming via `croniq_runner_sdk::client::push_events` is a follow-up.
pub async fn run(exec: &RunnerExec) -> Result<Outcome, RunError> {
    let mut cmd = build_command(exec)?;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd
        .spawn()
        .map_err(RunError::Spawn)?
        .wait_with_output()
        .await
        .map_err(RunError::Wait)?;

    Ok(Outcome {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Convert an `Outcome` into a runner-SDK `Result`. A non-zero exit becomes a
/// `HandlerError` whose message is short enough to fit in the execution log
/// row but informative enough to debug from.
pub fn outcome_to_handler_result(outcome: Outcome, job_key: &str) -> Result<(), HandlerError> {
    if outcome.status.success() {
        if !outcome.stdout.is_empty() {
            tracing::info!(
                job_key = %job_key,
                stdout = %outcome.stdout.trim_end(),
                "job stdout"
            );
        }
        if !outcome.stderr.is_empty() {
            tracing::info!(
                job_key = %job_key,
                stderr = %outcome.stderr.trim_end(),
                "job stderr"
            );
        }
        Ok(())
    } else {
        let code = outcome
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".into());
        let tail = outcome.stderr.trim_end();
        let snippet: String = tail
            .chars()
            .rev()
            .take(400)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        Err(HandlerError::msg(format!("exit {code}: {snippet}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn shell_command_runs_and_returns_stdout() {
        let exec = RunnerExec::Shell {
            command: "echo croniq-shell-runner".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec).await.expect("spawn ok");
        assert!(
            outcome.status.success(),
            "exit status: {:?}",
            outcome.status
        );
        assert!(
            outcome.stdout.contains("croniq-shell-runner"),
            "stdout: {:?}",
            outcome.stdout
        );
    }

    #[tokio::test]
    async fn shell_command_failure_propagates() {
        let exec = RunnerExec::Shell {
            command: "exit 7".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec).await.expect("spawn ok");
        assert!(!outcome.status.success());
        assert_eq!(outcome.status.code(), Some(7));
    }

    #[tokio::test]
    async fn exec_runs_argv_directly() {
        // /bin/echo is part of POSIX coreutils and reliably available in CI.
        let exec = RunnerExec::Exec {
            argv: vec!["/bin/echo".into(), "hello".into(), "exec".into()],
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec).await.expect("spawn ok");
        assert!(outcome.status.success());
        assert!(
            outcome.stdout.contains("hello exec"),
            "stdout: {:?}",
            outcome.stdout
        );
    }

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
        let outcome = run(&exec).await.expect("spawn ok");
        assert!(outcome.status.success());
        assert_eq!(outcome.stdout, "from-dsl");
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
}
