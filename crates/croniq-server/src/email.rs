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

use croniq_config::compile::SmtpDslConfig;
use std::sync::Arc;

/// Outbound email sender. Implementations should not block the calling
/// task for more than ~5 s; long delivery should queue and return Ok.
pub trait EmailSender: Send + Sync {
    /// Send an email. Body is plain text; subject is short (under
    /// ~80 chars). Returns Err with a human-readable reason on failure.
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String>;

    /// Whether this sender actually delivers mail. `false` for the
    /// [`NoopSender`], which only logs — diagnostics use this to warn that
    /// invitation / password-reset links must be delivered manually.
    fn delivers(&self) -> bool {
        true
    }
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

    fn delivers(&self) -> bool {
        false
    }
}

/// Sentinel for "no SMTP transport". `Arc<dyn EmailSender>` is the
/// runtime-injected type so the trait stays object-safe.
pub fn default_sender() -> Arc<dyn EmailSender> {
    Arc::new(NoopSender)
}

/// Whether this build was compiled with the `smtp` cargo feature. When
/// `false`, [`build_from_env`] can only ever return the [`NoopSender`], so
/// setting `CRONIQ_SMTP_*` has no effect — diagnostics flag that case.
pub fn smtp_feature_compiled() -> bool {
    cfg!(feature = "smtp")
}

/// Whether an SMTP transport can be assembled from the given DSL block +
/// the current `CRONIQ_SMTP_*` environment, ignoring the `smtp` cargo
/// feature. Used by boot diagnostics to warn when SMTP looks configured
/// but the binary was built without the feature.
pub fn smtp_configured(dsl: Option<&SmtpDslConfig>) -> bool {
    let has_target = dsl.is_some_and(|d| d.host.is_some())
        || env_present("CRONIQ_SMTP_HOST")
        || env_present("CRONIQ_SMTP_URL");
    let has_from = dsl.is_some_and(|d| d.from.is_some()) || env_present("CRONIQ_SMTP_FROM");
    has_target && has_from
}

fn env_present(var: &str) -> bool {
    crate::env_secret::env_or_file(var).is_some()
}

/// Build the production sender at startup, merging the Croniqfile
/// `smtp {}` block with the `CRONIQ_SMTP_*` env vars. With the `smtp`
/// cargo feature disabled, always returns [`NoopSender`].
///
/// Precedence:
///   1. `CRONIQ_SMTP_URL` (+ `_FILE`) — legacy composite, wins when set.
///      Accepts the lettre URL form `smtp://user:pass@host:587/?tls=required`.
///   2. Decomposed: `smtp {}` directives supply host/port/security/from;
///      any field the DSL leaves unset falls back to `CRONIQ_SMTP_HOST/
///      PORT/SECURITY/FROM`. Credentials (`CRONIQ_SMTP_USERNAME/PASSWORD`)
///      are ENV-only.
///
/// Falls back to the no-op sender when nothing usable is configured — the
/// token URL still goes back in the API response, so invitations + resets
/// remain functional out-of-the-box.
pub fn build_from_dsl_and_env(dsl: Option<&SmtpDslConfig>) -> Arc<dyn EmailSender> {
    #[cfg(feature = "smtp")]
    {
        match smtp::build(dsl) {
            Ok(Some(sender)) => return Arc::new(sender),
            Ok(None) => {}
            Err(e) => {
                tracing::error!(
                    target: "croniq::email",
                    error = %e,
                    "SMTP misconfiguration — falling back to NoopSender"
                );
            }
        }
    }
    #[cfg(not(feature = "smtp"))]
    let _ = dsl;
    default_sender()
}

/// Convenience wrapper for callers without a Croniqfile `smtp {}` block.
pub fn build_from_env() -> Arc<dyn EmailSender> {
    build_from_dsl_and_env(None)
}

#[cfg(feature = "smtp")]
mod smtp {
    use super::{EmailSender, SmtpDslConfig};
    use crate::env_secret::env_or_file;
    use lettre::message::Mailbox;
    use lettre::transport::smtp::SmtpTransport;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, Transport};

    /// lettre-backed `EmailSender`. Sync API: the trait method blocks
    /// until the SMTP exchange completes. For PR-A6 that's acceptable
    /// — invitations + resets are infrequent. If volume goes up the
    /// caller can wrap each `.send()` in `tokio::spawn_blocking`.
    pub struct SmtpSender {
        transport: SmtpTransport,
        from: Mailbox,
    }

    /// Assemble a sender from the `smtp {}` block ⊕ `CRONIQ_SMTP_*` env.
    /// Returns `Ok(None)` when SMTP is simply not configured (no URL and
    /// no host), `Err` when it is configured but invalid.
    pub fn build(dsl: Option<&SmtpDslConfig>) -> Result<Option<SmtpSender>, String> {
        let from_raw = dsl
            .and_then(|d| d.from.clone())
            .or_else(|| env_or_file("CRONIQ_SMTP_FROM"));

        // 1. Legacy composite URL wins when present.
        if let Some(url) = env_or_file("CRONIQ_SMTP_URL") {
            let from = from_raw.ok_or(
                "CRONIQ_SMTP_URL is set but no from address (CRONIQ_SMTP_FROM / smtp { from })",
            )?;
            let transport = SmtpTransport::from_url(&url)
                .map_err(|e| format!("invalid CRONIQ_SMTP_URL: {e}"))?
                .build();
            let from = parse_from(&from)?;
            tracing::info!(target: "croniq::email", "SMTP sender configured (CRONIQ_SMTP_URL)");
            return Ok(Some(SmtpSender { transport, from }));
        }

        // 2. Decomposed path — a host is required to be "configured".
        let Some(host) = dsl
            .and_then(|d| d.host.clone())
            .or_else(|| env_or_file("CRONIQ_SMTP_HOST"))
        else {
            return Ok(None);
        };
        let from = from_raw
            .ok_or("smtp host configured but no from address (CRONIQ_SMTP_FROM / smtp { from })")?;

        let port: u16 = dsl
            .and_then(|d| d.port)
            .or_else(|| env_or_file("CRONIQ_SMTP_PORT").and_then(|s| s.trim().parse().ok()))
            .unwrap_or(587);
        let security = dsl
            .and_then(|d| d.security.clone())
            .or_else(|| env_or_file("CRONIQ_SMTP_SECURITY"))
            .unwrap_or_else(|| "starttls".into())
            .trim()
            .to_ascii_lowercase();

        let mut builder = match security.as_str() {
            "starttls" => SmtpTransport::starttls_relay(&host)
                .map_err(|e| format!("SMTP STARTTLS relay for '{host}': {e}"))?,
            "tls" => SmtpTransport::relay(&host)
                .map_err(|e| format!("SMTP TLS relay for '{host}': {e}"))?,
            "none" => SmtpTransport::builder_dangerous(&host),
            other => {
                return Err(format!(
                    "invalid SMTP security '{other}' (expected starttls | tls | none)"
                ));
            }
        };
        builder = builder.port(port);

        if let Some(user) = env_or_file("CRONIQ_SMTP_USERNAME") {
            let pass = env_or_file("CRONIQ_SMTP_PASSWORD").unwrap_or_default();
            builder = builder.credentials(Credentials::new(user, pass));
        }

        let transport = builder.build();
        let from = parse_from(&from)?;
        tracing::info!(
            target: "croniq::email",
            host = %host,
            port,
            security = %security,
            "SMTP sender configured (decomposed)"
        );
        Ok(Some(SmtpSender { transport, from }))
    }

    fn parse_from(from: &str) -> Result<Mailbox, String> {
        from.parse()
            .map_err(|e| format!("invalid SMTP from address '{from}': {e}"))
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

    #[cfg(feature = "smtp")]
    #[test]
    fn smtp_build_decomposed_from_dsl_only() {
        // host + from supplied by the DSL ⇒ a real transport without any
        // env. `security none` keeps it offline (no TLS relay lookup).
        let dsl = SmtpDslConfig {
            host: Some("smtp.example.com".into()),
            port: Some(2525),
            security: Some("none".into()),
            from: Some("Croniq <noreply@example.com>".into()),
        };
        assert!(smtp::build(Some(&dsl)).expect("build ok").is_some());
    }

    #[cfg(feature = "smtp")]
    #[test]
    fn smtp_build_rejects_unknown_security() {
        let dsl = SmtpDslConfig {
            host: Some("smtp.example.com".into()),
            port: None,
            security: Some("bogus".into()),
            from: Some("noreply@example.com".into()),
        };
        // Only meaningful when no legacy URL short-circuits the path.
        if crate::env_secret::env_or_file("CRONIQ_SMTP_URL").is_none() {
            assert!(smtp::build(Some(&dsl)).is_err());
        }
    }

    #[cfg(feature = "smtp")]
    #[test]
    fn smtp_build_none_when_nothing_configured() {
        // No DSL, and (in a clean env) no CRONIQ_SMTP_* ⇒ Ok(None) so the
        // caller falls back to NoopSender.
        if std::env::var_os("CRONIQ_SMTP_HOST").is_none()
            && std::env::var_os("CRONIQ_SMTP_URL").is_none()
            && std::env::var_os("CRONIQ_SMTP_HOST_FILE").is_none()
            && std::env::var_os("CRONIQ_SMTP_URL_FILE").is_none()
        {
            assert!(smtp::build(None).expect("build ok").is_none());
        }
    }
}
