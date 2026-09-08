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
//!
//! As of #576 the subprocess is no longer detached from the execution that owns
//! it, so "cancel" and `timeout` mean what the UI says they mean:
//!
//! - The command is spawned with `kill_on_drop(true)` and, on unix, into its
//!   own process group. A server-issued cancel aborts the handler future, which
//!   drops [`run`]'s locals -- that now terminates the command rather than
//!   orphaning it. The process group is what makes this hold for
//!   `runner shell { ... }`, where the direct child is `sh -c` and the
//!   operator's command is a grandchild that a kill aimed at the direct child
//!   never reaches.
//! - [`run`] enforces the execution's `timeout` itself: SIGTERM to the group,
//!   [`TERM_GRACE_SECS`] seconds to clean up, then SIGKILL. The server-side
//!   stale-claim reaper describes itself as a safety net for a *lost* runner and
//!   assumed the runner did this; until #576 nobody did, so a hung command was
//!   reaped and requeued while its first copy kept running.
//! - The pipe readers are aborted with the handler instead of being left to
//!   drain a surviving child's pipes into an already-acked execution.
//!
//! Windows caveat: no process-group equivalent is wired up, so termination
//! reaches the spawned process only -- a process it spawned in turn survives.

use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use croniq_config::compile::RunnerExec;
use croniq_runner_sdk::{HandlerError, LogWriter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

/// How many lines of stdout AND stderr to retain in the rolling tail
/// buffer for failure-snippet assembly. The previous snippet was 400
/// chars of stderr; 50 lines comfortably covers that for typical line
/// lengths while bounding memory to ~50 KB / stream worst-case.
const TAIL_BUFFER_LINES: usize = 50;

/// How many trailing characters to splice into the failure-snippet that
/// becomes the Result::Err message. Matches the pre-streaming behaviour
/// so dead-letter UI snippets look identical to v0.11.0.
const FAILURE_SNIPPET_CHARS: usize = 400;

/// How long a terminated command gets between SIGTERM and SIGKILL.
///
/// A command that cleans up after itself -- releasing a lock, removing a
/// partial artefact -- should get the chance to, which is why termination is
/// not a straight kill (issue #576). Kept short because this grace is spent
/// *after* the operator's own `timeout` has already elapsed: the job is late by
/// definition at this point, and the server's stale-claim grace is ticking.
const TERM_GRACE_SECS: u64 = 5;

/// How long to keep draining the pipes after the command has exited.
///
/// Normally this is instantaneous: the child's exit closes its end of the
/// pipes and both readers hit EOF. It is not instantaneous when the job
/// backgrounded something that inherited the pipes, which keeps them open with
/// nobody left to write -- an unbounded wait there would wedge the handler (and
/// with it the ack and the inflight slot) forever. Generous, because the other
/// reason a reader is still busy is legitimate: a slow server applying
/// backpressure through the `LogWriter`'s bounded channel.
const READER_DRAIN_GRACE_SECS: u64 = 30;

#[derive(Debug)]
pub struct Outcome {
    pub status: std::process::ExitStatus,
    /// Last [`TAIL_BUFFER_LINES`] lines of stdout, in chronological
    /// order. Older lines have already been streamed to the server but
    /// are no longer retained here.
    pub stdout_tail: VecDeque<String>,
    /// Last [`TAIL_BUFFER_LINES`] lines of stderr, same semantics.
    pub stderr_tail: VecDeque<String>,
    /// True when [`run`] terminated the command because the execution's
    /// `timeout` elapsed (issue #576). `status` cannot carry this on its own:
    /// "killed by a signal" is also what an operator killing the command by
    /// hand looks like, and the dead-letter view has to tell the two apart.
    pub timed_out: bool,
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

    #[error(
        "the command was still running {grace_secs}s after SIGKILL (pid {pid:?}): the process is \
         blocked in an uninterruptible syscall -- a hung network filesystem is the usual cause -- \
         and cannot be terminated from here"
    )]
    Unkillable { pid: Option<u32>, grace_secs: u64 },

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

    // Tie the subprocess's lifetime to the execution that owns it (issue #576).
    //
    // `kill_on_drop` is the cancel path: a server cancel aborts the handler
    // future, which drops the `Child`. Tokio's default is to detach it into the
    // orphan queue instead, so the command used to outlive the execution that
    // had just been marked cancelled.
    cmd.kill_on_drop(true);

    // ... and give the job its own process group, so termination can reach the
    // whole tree. For `runner shell { ... }` the direct child is `sh -c` and the
    // operator's command is *its* child; a kill aimed at the direct child kills
    // the shell and leaves the command running. Signalling the group does not.
    // Detaching from the runner's own group is a bonus: a Ctrl-C in an
    // interactive runner no longer forwards SIGINT into every running job.
    #[cfg(unix)]
    cmd.process_group(0);

    Ok(cmd)
}

/// Owns the spawned child for the lifetime of one [`run`] call and terminates
/// its process group if that call is cancelled out from under it (issue #576).
///
/// The child lives in here rather than in a local because `Drop` needs to
/// *move* it into the task that finishes the SIGTERM -> grace -> SIGKILL
/// sequence: holding an unreaped `Child` keeps its pid -- and therefore the
/// process group id, which is that same number -- allocated, so the delayed
/// SIGKILL cannot land on an unrelated process that recycled the pid meanwhile.
struct Job {
    /// `Some` until `Drop` takes it. Only `Drop` ever takes it, so every other
    /// access can unwrap.
    child: Option<Child>,
    /// The group to signal, `Some` only while the child is known alive.
    /// Cleared by [`Job::reaped`] once `wait()` has returned, both because
    /// there is then nothing left to terminate and because a reaped pid may be
    /// recycled -- signalling it later would hit a stranger.
    #[cfg(unix)]
    pgid: Option<i32>,
}

impl Job {
    fn new(child: Child) -> Self {
        // `process_group(0)` makes the child a group leader, so its pid is the
        // pgid. `id()` is `None` only once the child has been reaped, which
        // cannot have happened yet here.
        #[cfg(unix)]
        let pgid = child.id().map(|pid| pid as i32);
        Self {
            child: Some(child),
            #[cfg(unix)]
            pgid,
        }
    }

    fn child(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("child is taken only in Drop, which ends this Job")
    }

    /// Record that `wait()` has reaped the child: disarms the drop guard.
    fn reaped(&mut self) {
        #[cfg(unix)]
        {
            self.pgid = None;
        }
    }

    /// Ask the command to stop, giving it the chance to clean up first.
    fn request_stop(&mut self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            signal_group(pgid, libc::SIGTERM);
            return;
        }
        // Windows, or a child already reaped: the best available reach is the
        // spawned process itself.
        let _ = self.child().start_kill();
    }

    /// Stop the command whether it likes it or not.
    fn kill_now(&mut self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            signal_group(pgid, libc::SIGKILL);
        }
        let _ = self.child().start_kill();
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        #[cfg(unix)]
        {
            let Some(pgid) = self.pgid.take() else {
                // Already reaped by `run` -- the normal-completion path, and
                // there is nothing to signal.
                return;
            };
            // A cancelled command deserves the same courtesy as a timed-out
            // one, so this is SIGTERM and not a kill. `Drop` cannot await the
            // grace period, so the escalation and the reap are handed to a
            // detached task -- which also keeps `child` alive, and with it the
            // pid, until the SIGKILL has been sent.
            signal_group(pgid, libc::SIGTERM);
            tracing::info!(
                pgid,
                grace_secs = TERM_GRACE_SECS,
                "execution cancelled -- terminating the command's process group"
            );
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    let mut child = child;
                    handle.spawn(async move {
                        tokio::time::sleep(Duration::from_secs(TERM_GRACE_SECS)).await;
                        signal_group(pgid, libc::SIGKILL);
                        let _ = child.wait().await;
                    });
                }
                Err(_) => {
                    // No runtime left to schedule the grace period on (the
                    // process is shutting down). Kill now: skipping SIGTERM's
                    // courtesy window is better than leaking the group.
                    signal_group(pgid, libc::SIGKILL);
                    // `kill_on_drop(true)` reaps the direct child as `child`
                    // goes out of scope here.
                }
            }
        }
        #[cfg(not(unix))]
        {
            // No group to signal; `kill_on_drop(true)` terminates the spawned
            // process as `child` goes out of scope here.
            drop(child);
        }
    }
}

/// Send `sig` to the process group led by `pgid`.
///
/// `ESRCH` is the expected, uninteresting outcome once the group is empty --
/// every caller races a command that may have just exited on its own -- so
/// only anything else is worth a log line.
#[cfg(unix)]
fn signal_group(pgid: i32, sig: i32) {
    // SAFETY: `killpg` is a thin syscall wrapper with no memory contract. The
    // pgid comes from a child this process spawned into its own group and is
    // cleared as soon as that child is reaped, so the call either reaches that
    // group or fails with ESRCH.
    if unsafe { libc::killpg(pgid, sig) } != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!(pgid, sig, error = %err, "failed to signal the job's process group");
        }
    }
}

/// The two pipe-reader tasks, tied to the lifetime of the [`run`] call.
///
/// Dropping a `JoinHandle` detaches its task, so before #576 a cancelled
/// handler left both readers draining the surviving child's pipes and pushing
/// log events into an execution the dispatch loop had already acked and
/// drained. The handles stay owned here -- including while [`Readers::join`]
/// awaits them -- so a cancel at any point aborts them.
struct Readers {
    stdout: JoinHandle<std::io::Result<VecDeque<String>>>,
    stderr: JoinHandle<std::io::Result<VecDeque<String>>>,
}

impl Readers {
    async fn join(&mut self) -> (VecDeque<String>, VecDeque<String>) {
        let out = tail_or_empty((&mut self.stdout).await, Stream::Stdout);
        let err = tail_or_empty((&mut self.stderr).await, Stream::Stderr);
        (out, err)
    }
}

impl Drop for Readers {
    fn drop(&mut self) {
        // A no-op for a task that already finished, which is the normal case.
        self.stdout.abort();
        self.stderr.abort();
    }
}

/// Unwrap a reader task's result down to its tail buffer.
///
/// A reader that errored or panicked costs the failure snippet, not the
/// execution: the exit status is what decides success, and it is already in
/// hand by the time this runs.
fn tail_or_empty(
    joined: Result<std::io::Result<VecDeque<String>>, tokio::task::JoinError>,
    stream: Stream,
) -> VecDeque<String> {
    match joined {
        Ok(Ok(tail)) => tail,
        Ok(Err(e)) => {
            tracing::warn!(
                stream = stream.tracing_target(),
                error = %e,
                "pipe reader errored -- tail buffer may be incomplete"
            );
            VecDeque::new()
        }
        Err(e) => {
            tracing::warn!(
                stream = stream.tracing_target(),
                error = %e,
                "pipe reader task panicked"
            );
            VecDeque::new()
        }
    }
}

/// Wait for the command to exit, enforcing `timeout` if one is set (#576).
///
/// On expiry the command's process group gets SIGTERM, [`TERM_GRACE_SECS`]
/// seconds to exit on its own, then SIGKILL. Either way the child is reaped
/// before returning, so the caller's [`Job`] drop guard disarms and the pid
/// is never signalled again.
///
/// A zero or absent `timeout` means unbounded -- the pre-#576 behaviour, and
/// still the right reading of "no limit was configured".
async fn wait_or_terminate(
    job: &mut Job,
    timeout: Option<Duration>,
) -> Result<(std::process::ExitStatus, bool), RunError> {
    let Some(limit) = timeout.filter(|d| !d.is_zero()) else {
        let status = job.child().wait().await.map_err(RunError::Wait)?;
        job.reaped();
        return Ok((status, false));
    };

    if let Ok(status) = tokio::time::timeout(limit, job.child().wait()).await {
        let status = status.map_err(RunError::Wait)?;
        job.reaped();
        return Ok((status, false));
    }

    tracing::warn!(
        timeout_secs = limit.as_secs_f64(),
        grace_secs = TERM_GRACE_SECS,
        "execution timeout elapsed -- terminating the command"
    );
    let grace = Duration::from_secs(TERM_GRACE_SECS);
    job.request_stop();
    let status = match tokio::time::timeout(grace, job.child().wait()).await {
        Ok(status) => status.map_err(RunError::Wait)?,
        Err(_) => {
            tracing::warn!(
                grace_secs = TERM_GRACE_SECS,
                "command ignored SIGTERM -- killing it"
            );
            let pid = job.child().id();
            job.kill_now();
            match tokio::time::timeout(grace, job.child().wait()).await {
                Ok(status) => status.map_err(RunError::Wait)?,
                // Deliberately leaves the drop guard armed: the caller will
                // report a failure and free the slot, and `Job::drop` keeps
                // trying in the background rather than wedging the handler on
                // a process the kernel will not let go of.
                Err(_) => {
                    return Err(RunError::Unkillable {
                        pid,
                        grace_secs: TERM_GRACE_SECS,
                    });
                }
            }
        }
    };
    job.reaped();
    Ok((status, true))
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
pub async fn run(
    exec: &RunnerExec,
    writer: &LogWriter,
    timeout: Option<Duration>,
) -> Result<Outcome, RunError> {
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

    // Both owners abort/terminate on drop, so a cancelled handler takes the
    // command and its readers with it (issue #576). `job` is declared last so
    // it drops first: the child is signalled while the readers are still
    // draining, which is what gets the command's dying words into the log.
    let mut readers = Readers {
        stdout: tokio::spawn(stream_lines(stdout, writer.clone(), Stream::Stdout)),
        stderr: tokio::spawn(stream_lines(stderr, writer.clone(), Stream::Stderr)),
    };
    let mut job = Job::new(child);

    // Wait for the child to exit — or terminate it once the execution's
    // timeout elapses. Either way the kernel then closes its end of the pipes,
    // so the reader tasks see EOF and finish naturally.
    let (status, timed_out) = wait_or_terminate(&mut job, timeout).await?;

    let drain_grace = Duration::from_secs(READER_DRAIN_GRACE_SECS);
    let (stdout_tail, stderr_tail) = match tokio::time::timeout(drain_grace, readers.join()).await {
        Ok(tails) => tails,
        Err(_) => {
            tracing::warn!(
                grace_secs = READER_DRAIN_GRACE_SECS,
                "pipes were still open {READER_DRAIN_GRACE_SECS}s after the command exited — \
                 something it spawned inherited them, or the server is applying backpressure. \
                 Abandoning the tail buffers so the execution can be acked."
            );
            (VecDeque::new(), VecDeque::new())
        }
    };

    Ok(Outcome {
        status,
        stdout_tail,
        stderr_tail,
        timed_out,
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
    if outcome.timed_out {
        // Say "timed out" rather than reporting the signal it died from: the
        // status is SIGTERM/SIGKILL either way, which tells an operator reading
        // the dead-letter view nothing about why (issue #576).
        let snippet = build_failure_snippet(&outcome.stderr_tail);
        return Err(HandlerError::msg(if snippet.is_empty() {
            "timed out — the command was terminated".to_string()
        } else {
            format!("timed out — the command was terminated: {snippet}")
        }));
    }
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
        let outcome = run(&exec, &LogWriter::null(), None)
            .await
            .expect("spawn ok");
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
        let outcome = run(&exec, &LogWriter::null(), None)
            .await
            .expect("spawn ok");
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
        let outcome = run(&exec, &LogWriter::null(), None)
            .await
            .expect("spawn ok");
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
        let outcome = run(&exec, &LogWriter::null(), None)
            .await
            .expect("spawn ok");
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
        let outcome = run(&exec, &LogWriter::null(), None)
            .await
            .expect("spawn ok");
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
        let outcome = run(&exec, &LogWriter::null(), None)
            .await
            .expect("spawn ok");
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
        let outcome = run(&exec, &LogWriter::null(), None)
            .await
            .expect("spawn ok");
        assert!(outcome_to_handler_result(outcome, "test:job").is_ok());
    }

    // ─── Subprocess lifetime: cancel + timeout (issue #576) ─────────────────

    /// Is `pid` still alive? `kill(pid, 0)` performs the existence and
    /// permission checks and delivers nothing, which is exactly the probe
    /// these tests need.
    #[cfg(unix)]
    fn alive(pid: u32) -> bool {
        // SAFETY: signal 0 is the documented existence probe; no memory is
        // touched and no signal is delivered.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    /// Wait up to `secs` for `pid` to disappear. Termination is asynchronous —
    /// the kernel delivers the signal, the process unwinds — so a bare assert
    /// right after the trigger would be a race.
    #[cfg(unix)]
    async fn wait_gone(pid: u32, secs: u64) -> bool {
        for _ in 0..(secs * 20) {
            if !alive(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        !alive(pid)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn build_command_puts_the_job_in_its_own_process_group() {
        // The group is what lets termination reach the command inside
        // `sh -c`, so assert the spawned child really leads a group of its own
        // rather than inheriting the test process's.
        let exec = RunnerExec::Shell {
            command: "sleep 30".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let mut cmd = build_command(&exec).expect("build ok");
        let mut child = cmd.spawn().expect("spawn ok");
        let pid = child.id().expect("child has a pid") as i32;
        // SAFETY: plain syscall wrapper, no memory contract.
        let pgid = unsafe { libc::getpgid(pid) };
        assert_eq!(pgid, pid, "child must be its own process group leader");
        // SAFETY: as above.
        let own_pgid = unsafe { libc::getpgid(0) };
        assert_ne!(
            pgid, own_pgid,
            "child must not share the runner's process group"
        );
        let _ = child.kill().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_terminates_the_command_inside_the_shell() {
        // `sh -c 'sleep 30'` on most shells execs into sleep, so print the pid
        // of a *background* sleep to get a genuine grandchild — the case
        // `kill_on_drop` alone cannot reach.
        let exec = RunnerExec::Shell {
            command: "sleep 30 & echo $!; wait".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null(), Some(Duration::from_millis(200)))
            .await
            .expect("spawn ok");
        assert!(outcome.timed_out, "outcome must report the timeout");
        assert!(!outcome.status.success());
        let pid: u32 = outcome
            .stdout_tail
            .front()
            .expect("the command printed the background pid")
            .trim()
            .parse()
            .expect("pid parses");
        assert!(
            wait_gone(pid, 10).await,
            "grandchild {pid} survived the timeout"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_result_reports_a_timeout_not_a_signal() {
        let exec = RunnerExec::Shell {
            command: "sleep 30".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null(), Some(Duration::from_millis(200)))
            .await
            .expect("spawn ok");
        let msg = outcome_to_handler_result(outcome, "test:job")
            .expect_err("a timed-out command must fail the execution")
            .to_string();
        assert!(msg.starts_with("timed out"), "msg: {msg}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_a_command_that_ignores_sigterm() {
        // Trapping SIGTERM is the case the straight-kill shortcut gets wrong:
        // the polite phase is ignored, and only the SIGKILL escalation ends it.
        // The shell ignores SIGTERM and restarts its sleep, so the polite
        // phase cannot end it — only the SIGKILL escalation can.
        let exec = RunnerExec::Shell {
            command: "trap '' TERM; while :; do sleep 0.2; done".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let start = std::time::Instant::now();
        let outcome = run(&exec, &LogWriter::null(), Some(Duration::from_millis(200)))
            .await
            .expect("spawn ok");
        assert!(outcome.timed_out);
        assert!(!outcome.status.success());
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_secs(TERM_GRACE_SECS),
            "SIGTERM alone should not have ended this command: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(TERM_GRACE_SECS + 20),
            "the escalation to SIGKILL never landed: {elapsed:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_the_handler_terminates_the_whole_process_group() {
        // The reported bug: aborting the handler future left the command
        // running while the execution was acked as cancelled.
        let exec = RunnerExec::Shell {
            command: "sleep 30 & echo $! > \"$CRONIQ_TEST_PIDFILE\"; wait".into(),
            workdir: None,
            user: None,
            env: HashMap::from([(
                "CRONIQ_TEST_PIDFILE".to_string(),
                std::env::temp_dir()
                    .join(format!("croniq-576-{}.pid", std::process::id()))
                    .to_string_lossy()
                    .into_owned(),
            )]),
        };
        let pidfile = std::env::temp_dir().join(format!("croniq-576-{}.pid", std::process::id()));
        let _ = std::fs::remove_file(&pidfile);

        let handle = tokio::spawn(async move {
            let _ = run(&exec, &LogWriter::null(), None).await;
        });

        // Wait for the background sleep to record its pid.
        let mut pid = None;
        for _ in 0..200 {
            if let Ok(raw) = std::fs::read_to_string(&pidfile)
                && let Ok(parsed) = raw.trim().parse::<u32>()
            {
                pid = Some(parsed);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let pid = pid.expect("the command recorded its background pid");
        assert!(alive(pid), "precondition: the grandchild is running");

        handle.abort();
        assert!(
            wait_gone(pid, 20).await,
            "grandchild {pid} survived the cancel"
        );
        let _ = std::fs::remove_file(&pidfile);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_zero_timeout_means_unbounded() {
        // The server sends `timeout` as a string; a caller that means "no
        // limit" must not get an instant kill.
        let exec = RunnerExec::Shell {
            command: "echo done".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null(), Some(Duration::ZERO))
            .await
            .expect("spawn ok");
        assert!(outcome.status.success());
        assert!(!outcome.timed_out);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_finishing_inside_its_timeout_is_untouched() {
        let exec = RunnerExec::Shell {
            command: "echo quick".into(),
            workdir: None,
            user: None,
            env: HashMap::new(),
        };
        let outcome = run(&exec, &LogWriter::null(), Some(Duration::from_secs(30)))
            .await
            .expect("spawn ok");
        assert!(outcome.status.success());
        assert!(!outcome.timed_out);
        assert!(outcome.stdout_tail.iter().any(|l| l.contains("quick")));
    }
}
