//! Outbound email delivery — invitations + password-reset notifications.
//!
//! The trait + `NoopSender` land here so PR-A2 ships a working invite /
//! reset flow without an SMTP dependency. When no sender is configured,
//! the API returns the raw invite/reset URL in the response so the
//! admin can deliver it out-of-band (Slack, copy/paste, etc.) — the
//! "token URL fallback" the user picked over SMTP-mandatory.
//!
//! PR-A6 adds an `lettre`-backed `SmtpSender` behind the optional
//! `smtp` cargo feature.

use std::sync::Arc;

/// Outbound email sender. Implementations should not block the calling
/// task for more than ~5 s; long delivery should queue and return Ok.
pub trait EmailSender: Send + Sync {
    /// Send an email. Body is plain text; subject is short (under
    /// ~80 chars). Returns Err with a human-readable reason on failure.
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String>;
}

/// No-op sender used when no SMTP transport is configured. Logs a
/// structured `info!` line for audit, but never emits the body (which
/// contains a single-use token URL — that goes back in the API response
/// for the admin to deliver manually).
pub struct NoopSender;

impl EmailSender for NoopSender {
    fn send(&self, to: &str, subject: &str, _body: &str) -> Result<(), String> {
        tracing::info!(
            target: "croniq::email",
            to = %to,
            subject = %subject,
            "email skipped (no SMTP configured) — caller delivers token URL via API response"
        );
        Ok(())
    }
}

/// Sentinel for "no SMTP transport". `Arc<dyn EmailSender>` is the
/// runtime-injected type so the trait stays object-safe.
pub fn default_sender() -> Arc<dyn EmailSender> {
    Arc::new(NoopSender)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_sender_returns_ok_and_does_not_leak_body() {
        let s = NoopSender;
        // Body deliberately contains a token-looking value to make sure
        // the test fails loudly if we ever start logging it.
        let body = "Click https://croniq.test/invitations/accept?token=croniq_inv_secret123";
        s.send("a@e.org", "You're invited", body).unwrap();
    }
}
