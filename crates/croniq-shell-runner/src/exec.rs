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
//!
//! As of #431 jobs no longer inherit the runner's whole environment. They get
//! [`INHERITED_ENV_ALLOWLIST`] — PATH, HOME, locale, TZ and the Windows
//! variables a subprocess cannot start without — and nothing else, so the
//! runner's own `CRONIQ_API_KEY` is not readable from a job. Operators who
//! need more set [`ENV_PASSTHROUGH_VAR`]. In the same change, a `user`
//! directive the runner cannot honour fails the job instead of silently
//! running it with the runner's own (possibly root) privileges.

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

    #[error(
        "`user {0}` is not a numeric uid: the shell runner cannot resolve user names, and running \
         the job as the runner's own user would grant more privilege than the job asked for. Set a \
         numeric uid (e.g. `user 1000`), or drop the directive and run the runner process itself as \
         the desired user."
    )]
    NonNumericUser(String),

    #[error(
        "`user {0}` cannot be honoured on this platform: privilege dropping is only implemented for \
         unix targets. Drop the directive and run the runner process itself as the desired user."
    )]
    UserUnsupported(String),
}

/// Environment variables inherited from the runner process into every job.
///
/// Before #431 the runner re-injected all of `std::env::vars()`, so every
/// shell/exec job saw the runner's own environment — including the runner's
/// `CRONIQ_API_KEY`. Jobs now start from this allowlist instead: the entries a
/// subprocess genuinely cannot work without, and nothing else.
///
/// Names are compared case-insensitively on Windows (where env names are
/// case-insensitive and appear as `Path` / `SystemRoot` in `env::vars()`) and
/// exactly on unix.
const INHERITED_ENV_ALLOWLIST: &[&str] = &[
    // POSIX essentials. Without PATH, `sh -c` cannot find any binary at all.
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    // Timezone — jobs that format timestamps rely on the runner's zone.
    "TZ",
    // Locale. Missing LANG silently switches many tools to the C collation.
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    // Windows. These are not optional: the CRT resolves temp paths through
    // TEMP/TMP, `cmd.exe` is found via COMSPEC, DLL loading and countless
    // tools resolve through SYSTEMROOT, and PATHEXT decides which extensions
    // count as executable. A job spawned without them fails in obscure ways.
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)",
    "USERNAME",
    "USERPROFILE",
    "USERDOMAIN",
    "HOMEDRIVE",
    "HOMEPATH",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
];

/// Operator escape hatch for the inheritance allowlist: a comma-separated list
/// of additional variable names to pass through, or the single value `*` to
/// inherit the runner's whole environment (the pre-#431 behaviour).
///
/// `CRONIQ_*` is never inherited via `*` — that prefix is where the runner's
/// own credentials live, and a blunt wildcard must not leak them. An operator
/// who genuinely needs one (say `CRONIQ_SERVER_URL`) names it explicitly,
/// which is a deliberate act rather than a side effect.
const ENV_PASSTHROUGH_VAR: &str = "CRONIQ_RUNNER_ENV_PASSTHROUGH";

/// The runner's own configuration/credential namespace. Never inherited
/// implicitly, and never covered by the `*` wildcard.
const RESERVED_ENV_PREFIX: &str = "CRONIQ_";

fn env_name_eq(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn is_reserved_env_name(name: &str) -> bool {
    name.len() >= RESERVED_ENV_PREFIX.len()
        && name[..RESERVED_ENV_PREFIX.len()].eq_ignore_ascii_case(RESERVED_ENV_PREFIX)
}

/// Parse [`ENV_PASSTHROUGH_VAR`] into (`wildcard`, `explicit names`).
fn parse_passthrough(raw: Option<&str>) -> (bool, Vec<String>) {
    let Some(raw) = raw else {
        return (false, Vec::new());
    };
    let mut wildcard = false;
    let mut names = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part == "*" {
            wildcard = true;
        } else {
            names.push(part.to_string());
        }
    }
    (wildcard, names)
}

/// Decide whether `name` from the runner's environment is inherited by jobs.
fn inherits(name: &str, wildcard: bool, extra: &[String]) -> bool {
    // Explicitly named by the operator — honoured even inside the reserved
    // prefix, because naming it is the deliberate opt-in.
    if extra.iter().any(|e| env_name_eq(e, name)) {
        return true;
    }
    if is_reserved_env_name(name) {
        return false;
    }
    wildcard || INHERITED_ENV_ALLOWLIST.iter().any(|a| env_name_eq(a, name))
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

    // Inherit only what a subprocess genuinely needs (issue #431). The runner's
    // own credentials — `CRONIQ_API_KEY` above all — stay in the runner
    // process. User-supplied env from the Croniqfile is applied afterwards and
    // still overrides any inherited value.
    cmd.env_clear();
    let passthrough = std::env::var(ENV_PASSTHROUGH_VAR).ok();
    let (wildcard, extra) = parse_passthrough(passthrough.as_deref());
    for (k, v) in std::env::vars() {
        if inherits(&k, wildcard, &extra) {
            cmd.env(k, v);
        }
    }
    for (k, v) in env {
        cmd.env(k, v);
    }

    if let Some(u) = user
        && !u.is_empty()
    {
        // A privilege drop that cannot be performed must fail the job rather
        // than run it as the runner's own user — possibly root — which is
        // strictly more privilege than the job asked for (issue #431). Only a
        // numeric uid is honoured: resolving names would mean linking nss/libc,
        // which the runner image deliberately avoids.
        #[cfg(unix)]
        {
            let uid = u
                .parse::<u32>()
                .map_err(|_| RunError::NonNumericUser(u.clone()))?;
            cmd.uid(uid);
        }
        #[cfg(not(unix))]
        {
            return Err(RunError::UserUnsupported(u.clone()));
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

    // ─── Environment inheritance + privilege drop (issue #431) ──────────────

    fn probe_exec() -> RunnerExec {
        RunnerExec::Exec {
            argv: vec!["/bin/echo".to_string()],
            workdir: None,
            user: None,
            env: HashMap::new(),
        }
    }

    /// Names the built command will hand to the child. `env_clear()` plus
    /// explicit `env()` calls means this is exactly the inherited set, so the
    /// assertion needs no subprocess and works identically on every platform.
    fn built_env_names(exec: &RunnerExec) -> Vec<String> {
        build_command(exec)
            .expect("build ok")
            .as_std()
            .get_envs()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn runner_credentials_are_never_inherited() {
        // The whole point of #431: `CRONIQ_API_KEY` and friends stay in the
        // runner process. This asserts against the real process environment,
        // so it also catches an allowlist entry that accidentally matches.
        let names = built_env_names(&probe_exec());
        assert!(
            names.iter().all(|n| !is_reserved_env_name(n)),
            "reserved-prefix variable leaked into the job: {names:?}"
        );
    }

    #[test]
    fn path_is_still_inherited() {
        // PATH is the one variable whose absence breaks every job, so the
        // allowlist has to keep working — a test that only asserts absence
        // would pass on an empty environment.
        let names = built_env_names(&probe_exec());
        assert!(
            names.iter().any(|n| env_name_eq(n, "PATH")),
            "PATH must stay inherited: {names:?}"
        );
    }

    #[test]
    fn allowlist_admits_only_known_names() {
        assert!(inherits("PATH", false, &[]));
        assert!(inherits("LC_ALL", false, &[]));
        assert!(!inherits("AWS_SECRET_ACCESS_KEY", false, &[]));
        assert!(!inherits("CRONIQ_API_KEY", false, &[]));
    }

    #[test]
    fn wildcard_passthrough_still_withholds_the_reserved_prefix() {
        // `*` restores the pre-#431 blanket inheritance for operators who need
        // it, but a blunt wildcard must not hand out the runner's own
        // credentials.
        let (wildcard, extra) = parse_passthrough(Some("*"));
        assert!(wildcard);
        assert!(extra.is_empty());
        assert!(inherits("AWS_SECRET_ACCESS_KEY", wildcard, &extra));
        assert!(!inherits("CRONIQ_API_KEY", wildcard, &extra));
    }

    #[test]
    fn explicitly_named_variables_pass_through_including_reserved_ones() {
        let (wildcard, extra) = parse_passthrough(Some("MY_TOKEN, CRONIQ_SERVER_URL ,"));
        assert!(!wildcard);
        assert_eq!(extra, vec!["MY_TOKEN", "CRONIQ_SERVER_URL"]);
        assert!(inherits("MY_TOKEN", wildcard, &extra));
        // Naming a reserved variable is a deliberate operator act, so it wins.
        assert!(inherits("CRONIQ_SERVER_URL", wildcard, &extra));
        // Not named, so still withheld.
        assert!(!inherits("CRONIQ_API_KEY", wildcard, &extra));
    }

    #[test]
    fn unset_passthrough_is_the_plain_allowlist() {
        let (wildcard, extra) = parse_passthrough(None);
        assert!(!wildcard);
        assert!(extra.is_empty());
    }

    #[test]
    fn dsl_env_overrides_an_inherited_value() {
        // User-supplied env is applied after inheritance, so it wins. Asserted
        // here because the allowlist rewrite reordered that block.
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/croniq-override".to_string());
        let exec = RunnerExec::Exec {
            argv: vec!["/bin/echo".to_string()],
            workdir: None,
            user: None,
            env,
        };
        let cmd = build_command(&exec).expect("build ok");
        let path = cmd
            .as_std()
            .get_envs()
            .find(|(k, _)| env_name_eq(&k.to_string_lossy(), "PATH"))
            .and_then(|(_, v)| v)
            .map(|v| v.to_string_lossy().into_owned());
        assert_eq!(path.as_deref(), Some("/croniq-override"));
    }

    #[cfg(unix)]
    #[test]
    fn non_numeric_user_refuses_to_spawn() {
        // Previously logged and ignored, which ran the job as the runner's own
        // user (possibly root) — strictly more privilege than asked for.
        let exec = RunnerExec::Shell {
            command: "id -u".into(),
            workdir: None,
            user: Some("nobody".into()),
            env: HashMap::new(),
        };
        let err = build_command(&exec).unwrap_err();
        assert!(
            matches!(&err, RunError::NonNumericUser(u) if u == "nobody"),
            "expected NonNumericUser, got {err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn numeric_user_is_still_accepted() {
        let exec = RunnerExec::Shell {
            command: "id -u".into(),
            workdir: None,
            user: Some("1000".into()),
            env: HashMap::new(),
        };
        assert!(build_command(&exec).is_ok());
    }

    #[cfg(not(unix))]
    #[test]
    fn user_directive_refuses_to_spawn_off_unix() {
        let exec = RunnerExec::Exec {
            argv: vec!["cmd".to_string(), "/C".into(), "echo".into()],
            workdir: None,
            user: Some("1000".into()),
            env: HashMap::new(),
        };
        let err = build_command(&exec).unwrap_err();
        assert!(
            matches!(&err, RunError::UserUnsupported(u) if u == "1000"),
            "expected UserUnsupported, got {err:?}"
        );
    }

    #[test]
    fn empty_user_directive_is_not_a_privilege_request() {
        let exec = RunnerExec::Exec {
            argv: vec!["/bin/echo".to_string()],
            workdir: None,
            user: Some(String::new()),
            env: HashMap::new(),
        };
        assert!(build_command(&exec).is_ok());
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
