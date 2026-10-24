//! Supervision for the server's long-running background tasks.
//!
//! The scheduler task got a supervisor in #248: a panicking tick had left a
//! dead scheduler behind a healthy-looking HTTP server. The completion
//! processor and the watchdog stayed `let _ = tokio::spawn(..)`, so a panic in
//! either ended the task without a word — results were acknowledged with
//! `200` and then never processed, claims were never reaped (review
//! 2026-10-24, D-1). All three now share the rule written down here: a task
//! that is meant to run for the life of the process and stops anyway takes
//! the process with it, so the orchestrator's restart policy brings back a
//! complete server instead of one running on with a missing organ.

use tokio::task::JoinHandle;

/// Exit code for a background task that stopped (`EX_SOFTWARE`).
pub const EXIT_TASK_DIED: i32 = 70;

/// What a supervised task is expected to do when it finishes normally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnReturn {
    /// The task loops forever; returning at all is a failure.
    Fatal,
    /// The task ends when its input closes, which only happens on shutdown
    /// (the completion processor drains a channel whose senders live in the
    /// server state). Returning is not a failure; only a panic or a
    /// cancellation is.
    Expected,
}

/// Wait for `handle` and describe why it counts as a failure, or `None` when
/// it finished in a way `on_return` allows.
pub async fn watch(name: &str, handle: JoinHandle<()>, on_return: OnReturn) -> Option<String> {
    match handle.await {
        Ok(()) if on_return == OnReturn::Expected => {
            tracing::info!(task = name, "background task finished");
            None
        }
        Ok(()) => Some(format!("{name} task exited unexpectedly (loop returned)")),
        Err(e) if e.is_panic() => Some(format!("{name} task panicked: {e}")),
        Err(e) => Some(format!("{name} task was cancelled: {e}")),
    }
}

/// Watch `handle` on a task of its own and end the process with
/// [`EXIT_TASK_DIED`] when [`watch`] reports a failure.
pub fn supervise(name: &'static str, handle: JoinHandle<()>, on_return: OnReturn) {
    tokio::spawn(async move {
        if let Some(reason) = watch(name, handle, on_return).await {
            tracing::error!(
                task = name,
                reason = %reason,
                "background task stopped — aborting process so it restarts"
            );
            std::process::exit(EXIT_TASK_DIED);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panic_is_a_failure_whatever_the_return_rule() {
        for rule in [OnReturn::Fatal, OnReturn::Expected] {
            let handle = tokio::spawn(async { panic!("boom") });
            let reason = watch("completion", handle, rule).await;
            assert!(
                reason.as_deref().is_some_and(|r| r.contains("panicked")),
                "{reason:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_return_is_a_failure_only_for_a_forever_loop() {
        let reason = watch("watchdog", tokio::spawn(async {}), OnReturn::Fatal).await;
        assert!(reason.is_some_and(|r| r.contains("exited unexpectedly")));
        assert_eq!(
            watch("completion", tokio::spawn(async {}), OnReturn::Expected).await,
            None
        );
    }

    #[tokio::test]
    async fn a_cancellation_is_a_failure() {
        let handle = tokio::spawn(std::future::pending::<()>());
        handle.abort();
        let reason = watch("scheduler", handle, OnReturn::Expected).await;
        assert!(reason.is_some_and(|r| r.contains("cancelled")));
    }
}
