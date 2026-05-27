//! Configuration diagnostics — one source of truth for "is this deployment
//! missing recommended config?".
//!
//! The same checks are surfaced in three places so an operator can't miss
//! them: as `tracing` warnings at server boot, via the offline
//! `croniq-server doctor` subcommand, and through the admin
//! `GET /v1/system/diagnostics` endpoint (which backs the Settings → System
//! UI panel). Each surface builds a [`DiagnosticsInput`] from what it has and
//! calls [`run_diagnostics`]; keeping the checks a pure function of plain
//! facts (no `ServerState`/store types) makes them trivially unit-testable.

use serde::Serialize;

/// How loudly a finding should be surfaced. `Critical` means something is
/// actively broken or unsafe; `Warning` means degraded/risky-by-default;
/// `Info` is FYI with no action required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

/// A single configuration finding.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    /// Stable machine-readable id (e.g. `"email.delivery"`), safe to match on.
    pub id: &'static str,
    pub severity: Severity,
    /// One-line human summary.
    pub title: String,
    /// What it means and why it matters.
    pub detail: String,
    /// Suggested fix. `None` for purely informational findings.
    pub remedy: Option<String>,
}

/// The facts the checks operate on. Built by each surface from what it knows.
#[derive(Debug, Clone)]
pub struct DiagnosticsInput {
    /// A public base URL is explicitly pinned — via the Croniqfile
    /// `server { app_url "…" }` directive or the `CRONIQ_APP_URL` env var.
    pub app_base_url_configured: bool,
    /// A real email transport is active (i.e. not the `NoopSender`).
    pub email_delivery_active: bool,
    /// Both `CRONIQ_SMTP_URL` and `CRONIQ_SMTP_FROM` are present in the env.
    pub smtp_env_present: bool,
    /// This `croniq-server` build was compiled with the `smtp` cargo feature.
    pub smtp_feature_compiled: bool,
    /// Enforced 2FA (`require_totp`) is on.
    pub totp_enforced: bool,
    /// Number of active users without a confirmed TOTP secret. `None` when not
    /// evaluated (the offline `doctor` has no live store to query).
    pub users_without_totp: Option<usize>,
}

impl DiagnosticsInput {
    /// Build from the facts that vary per surface, reading the shared SMTP env
    /// (`CRONIQ_SMTP_URL` + `CRONIQ_SMTP_FROM`) and the compiled `smtp` feature
    /// flag. `store` is `Some` on live surfaces (boot, endpoint) so the
    /// enforced-2FA-enrollment check can run; the offline `doctor` passes
    /// `None`. All three surfaces go through this so they gather facts
    /// identically.
    pub fn from_runtime(
        app_base_url_configured: bool,
        email_delivery_active: bool,
        totp_enforced: bool,
        store: Option<&crate::store::DynStore>,
    ) -> Self {
        Self {
            app_base_url_configured,
            email_delivery_active,
            smtp_env_present: std::env::var("CRONIQ_SMTP_URL").is_ok()
                && std::env::var("CRONIQ_SMTP_FROM").is_ok(),
            smtp_feature_compiled: crate::email::smtp_feature_compiled(),
            totp_enforced,
            users_without_totp: store.map(active_users_without_totp),
        }
    }
}

/// Count active users with no confirmed (enabled) TOTP secret — the population
/// locked out by enforced 2FA. A store error is treated as "0" so a transient
/// hiccup never fabricates a scary finding.
pub fn active_users_without_totp(store: &crate::store::DynStore) -> usize {
    let Ok(users) = store.users_list() else {
        return 0;
    };
    users
        .iter()
        .filter(|u| u.is_active)
        .filter(|u| !matches!(store.totp_get(&u.user_id), Ok(Some(t)) if t.enabled))
        .count()
}

/// Evaluate every check against `input` and return the findings worth
/// surfacing (clean checks produce nothing).
pub fn run_diagnostics(input: &DiagnosticsInput) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    // ── Email delivery ──────────────────────────────────────────────────
    if !input.email_delivery_active {
        if input.smtp_env_present && !input.smtp_feature_compiled {
            // The operator clearly *intends* email to work (they set the SMTP
            // vars) but this binary can't honour it — that's a misconfig, not
            // just an unset default.
            out.push(Diagnostic {
                id: "email.smtp_feature_missing",
                severity: Severity::Critical,
                title: "SMTP is configured but this build cannot send email".into(),
                detail: "CRONIQ_SMTP_URL / CRONIQ_SMTP_FROM are set, but croniq-server was \
                         compiled without the `smtp` cargo feature. A NoopSender is used, so \
                         invitation and password-reset emails are silently dropped."
                    .into(),
                remedy: Some(
                    "Deploy a build compiled with the `smtp` feature (or unset the SMTP \
                     variables to silence this and deliver links manually)."
                        .into(),
                ),
            });
        } else {
            out.push(Diagnostic {
                id: "email.delivery",
                severity: Severity::Warning,
                title: "No email delivery configured".into(),
                detail: "Invitation and password-reset emails are not sent. The accept / reset \
                         link is only returned in the API (and shown in the UI) and must be \
                         delivered to the recipient manually."
                    .into(),
                remedy: Some(
                    "Configure SMTP via CRONIQ_SMTP_URL + CRONIQ_SMTP_FROM (requires a build \
                     with the `smtp` feature) to email links automatically."
                        .into(),
                ),
            });
        }
    }

    // ── Public base URL for user-facing links ───────────────────────────
    if !input.app_base_url_configured {
        out.push(Diagnostic {
            id: "links.app_url",
            severity: Severity::Warning,
            title: "Public base URL not configured".into(),
            detail: "Invitation, password-reset, and OIDC login links are derived per-request \
                     from the forwarded / Host headers. That works behind a trusted reverse \
                     proxy, but on a directly-exposed server the public password-reset link \
                     cannot trust the Host header and falls back to http://localhost:4000."
                .into(),
            remedy: Some(
                "Pin the URL with server { app_url \"https://your.host\" } in the Croniqfile \
                 or the CRONIQ_APP_URL env var."
                    .into(),
            ),
        });
    }

    // ── Enforced 2FA without enrollment ─────────────────────────────────
    if let (true, Some(n)) = (input.totp_enforced, input.users_without_totp)
        && n > 0
    {
        out.push(Diagnostic {
            id: "totp.enforced_without_enrollment",
            severity: Severity::Warning,
            title: format!("{n} active user(s) cannot sign in under enforced 2FA"),
            detail: "Enforced two-factor (require_totp) is on, but these accounts have no \
                     confirmed TOTP secret, so they are refused at login until they enrol."
                .into(),
            remedy: Some(
                "Have them enrol TOTP, or temporarily set auth { totp { required false } } \
                 (CRONIQ_REQUIRE_TOTP=false) so they can sign in and enrol."
                    .into(),
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> DiagnosticsInput {
        DiagnosticsInput {
            app_base_url_configured: true,
            email_delivery_active: true,
            smtp_env_present: true,
            smtp_feature_compiled: true,
            totp_enforced: false,
            users_without_totp: Some(0),
        }
    }

    fn ids(ds: &[Diagnostic]) -> Vec<&str> {
        ds.iter().map(|d| d.id).collect()
    }

    #[test]
    fn fully_configured_is_silent() {
        assert!(run_diagnostics(&healthy()).is_empty());
    }

    #[test]
    fn no_email_transport_warns() {
        let input = DiagnosticsInput {
            email_delivery_active: false,
            smtp_env_present: false,
            smtp_feature_compiled: true,
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["email.delivery"]);
        assert_eq!(ds[0].severity, Severity::Warning);
        assert!(ds[0].remedy.is_some());
    }

    #[test]
    fn smtp_env_without_feature_is_critical() {
        let input = DiagnosticsInput {
            email_delivery_active: false,
            smtp_env_present: true,
            smtp_feature_compiled: false,
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["email.smtp_feature_missing"]);
        assert_eq!(ds[0].severity, Severity::Critical);
    }

    #[test]
    fn unconfigured_app_url_warns() {
        let input = DiagnosticsInput {
            app_base_url_configured: false,
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["links.app_url"]);
        assert_eq!(ds[0].severity, Severity::Warning);
    }

    #[test]
    fn multiple_findings_accumulate() {
        let input = DiagnosticsInput {
            app_base_url_configured: false,
            email_delivery_active: false,
            smtp_env_present: false,
            smtp_feature_compiled: false,
            totp_enforced: true,
            users_without_totp: Some(3),
        };
        let ds = run_diagnostics(&input);
        assert_eq!(
            ids(&ds),
            vec![
                "email.delivery",
                "links.app_url",
                "totp.enforced_without_enrollment"
            ]
        );
    }

    #[test]
    fn enforced_2fa_without_enrollment_warns() {
        let input = DiagnosticsInput {
            totp_enforced: true,
            users_without_totp: Some(2),
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["totp.enforced_without_enrollment"]);
        assert_eq!(ds[0].severity, Severity::Warning);
        assert!(ds[0].title.contains('2'));
    }

    #[test]
    fn enforced_2fa_all_enrolled_is_silent() {
        let input = DiagnosticsInput {
            totp_enforced: true,
            users_without_totp: Some(0),
            ..healthy()
        };
        assert!(run_diagnostics(&input).is_empty());
    }

    #[test]
    fn totp_not_evaluated_is_silent() {
        // Offline `doctor`: users_without_totp = None → no finding even when
        // enforcement is on (it's a runtime concern the preflight can't judge).
        let input = DiagnosticsInput {
            totp_enforced: true,
            users_without_totp: None,
            ..healthy()
        };
        assert!(run_diagnostics(&input).is_empty());
    }
}
