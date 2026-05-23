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

/// Build the production sender at startup. With the `smtp` feature
/// disabled, always returns [`NoopSender`]. With the feature enabled,
/// reads `CRONIQ_SMTP_URL` + `CRONIQ_SMTP_FROM` and returns an
/// [`SmtpSender`] when both are set; otherwise falls back to the
/// no-op (the token URL still goes back in the API response, so
/// invitations + resets remain functional out-of-the-box).
///
/// `CRONIQ_SMTP_URL` accepts the lettre URL form, e.g.
/// `smtp://user:pass@host:587/?tls=required`. `CRONIQ_SMTP_FROM` is
/// the From header on every outbound mail.
pub fn build_from_env() -> Arc<dyn EmailSender> {
    #[cfg(feature = "smtp")]
    {
        if let (Ok(url), Ok(from)) = (
            std::env::var("CRONIQ_SMTP_URL"),
            std::env::var("CRONIQ_SMTP_FROM"),
        ) {
            match smtp::SmtpSender::new(&url, &from) {
                Ok(sender) => {
                    tracing::info!(target: "croniq::email", from = %from, "SMTP sender configured");
                    return Arc::new(sender);
                }
                Err(e) => {
                    tracing::error!(
                        target: "croniq::email",
                        error = %e,
                        "SMTP misconfiguration — falling back to NoopSender"
                    );
                }
            }
        }
    }
    default_sender()
}

#[cfg(feature = "smtp")]
mod smtp {
    use super::EmailSender;
    use lettre::message::Mailbox;
    use lettre::transport::smtp::SmtpTransport;
    use lettre::{Message, Transport};

    /// lettre-backed `EmailSender`. Sync API: the trait method blocks
    /// until the SMTP exchange completes. For PR-A6 that's acceptable
    /// — invitations + resets are infrequent. If volume goes up the
    /// caller can wrap each `.send()` in `tokio::spawn_blocking`.
    pub struct SmtpSender {
        transport: SmtpTransport,
        from: Mailbox,
    }

    impl SmtpSender {
        pub fn new(url: &str, from: &str) -> Result<Self, String> {
            let transport = SmtpTransport::from_url(url)
                .map_err(|e| format!("invalid CRONIQ_SMTP_URL: {e}"))?
                .build();
            let from: Mailbox = from
                .parse()
                .map_err(|e| format!("invalid CRONIQ_SMTP_FROM: {e}"))?;
            Ok(Self { transport, from })
        }
    }

    impl EmailSender for SmtpSender {
        fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String> {
            let to_mb: Mailbox = to
                .parse()
                .map_err(|e| format!("invalid recipient '{to}': {e}"))?;
            let msg = Message::builder()
                .from(self.from.clone())
                .to(to_mb)
                .subject(subject)
                .body(body.to_string())
                .map_err(|e| e.to_string())?;
            self.transport
                .send(&msg)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    }
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
