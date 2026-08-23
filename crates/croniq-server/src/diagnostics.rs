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

/// What a surface knows about the resolved configuration and, where it has one,
/// the live store. Passed to [`DiagnosticsInput::from_runtime`] as a struct so
/// adding a fact doesn't turn every call site into a row of unlabelled bools.
pub struct RuntimeFacts<'a> {
    /// A public base URL is explicitly pinned — via the Croniqfile
    /// `server { app_url "…" }` directive or the `CRONIQ_APP_URL` env var.
    pub app_base_url_configured: bool,
    /// A real email transport is active (i.e. not the `NoopSender`).
    pub email_delivery_active: bool,
    /// Enforced 2FA (`require_totp`) is on.
    pub totp_enforced: bool,
    /// Some run-history cap is configured: `server { execution_retention … }`
    /// or a `keep_last` on `defaults { }` / any `job { }`.
    pub retention_configured: bool,
    /// Live store. `Some` on the boot and endpoint surfaces; the offline
    /// `doctor` has no DB to query and passes `None`, which skips the checks
    /// that need stored rows.
    pub store: Option<&'a crate::store::DynStore>,
    /// The active JWT secret — the HKDF input for the key that wraps stored
    /// TOTP secrets. Needed together with `store` to tell whether stored
    /// secrets still decrypt.
    pub jwt_secret: Option<&'a str>,
    /// `CRONIQ_JWT_SECRET_PREVIOUS`, when the operator named one. Separates
    /// "waiting to be re-wrapped" from "readable under no key at all"
    /// (issue #531).
    pub previous_jwt_secret: Option<&'a str>,
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
    /// Some run-history cap is configured (`execution_retention` / `keep_last`).
    pub retention_configured: bool,
    /// How the stored TOTP secrets sort by the key they decrypt under
    /// (issues #408, #531). `None` when not evaluated — no store, or no JWT
    /// secret to try, as on the offline `doctor` surface.
    pub totp_secrets: Option<TotpSecretTally>,
}

impl DiagnosticsInput {
    /// Build from the facts that vary per surface, reading the shared SMTP env
    /// (`CRONIQ_SMTP_URL` + `CRONIQ_SMTP_FROM`) and the compiled `smtp` feature
    /// flag. All three surfaces go through this so they gather facts
    /// identically.
    pub fn from_runtime(facts: RuntimeFacts<'_>) -> Self {
        Self {
            app_base_url_configured: facts.app_base_url_configured,
            email_delivery_active: facts.email_delivery_active,
            smtp_env_present: std::env::var("CRONIQ_SMTP_URL").is_ok()
                && std::env::var("CRONIQ_SMTP_FROM").is_ok(),
            smtp_feature_compiled: crate::email::smtp_feature_compiled(),
            totp_enforced: facts.totp_enforced,
            users_without_totp: facts.store.map(active_users_without_totp),
            retention_configured: facts.retention_configured,
            totp_secrets: facts.store.zip(facts.jwt_secret).map(|(store, secret)| {
                tally_totp_secrets(store, secret, facts.previous_jwt_secret)
            }),
        }
    }
}

/// Whether the resolved config caps run history at all — the input for the
/// `retention.unbounded_history` finding (issue #405).
///
/// `keep_last` is read off the compiled jobs because `defaults { keep_last N }`
/// is folded into every job at compile time.
pub fn retention_configured(rt: &croniq_config::compile::RuntimeConfig) -> bool {
    rt.server.execution_retention.is_some() || rt.jobs.iter().any(|j| j.keep_last.is_some())
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

/// Which key every confirmed TOTP secret is readable under.
///
/// One walk produces all three counts so they can never disagree with each
/// other — "12 stored, 11 fine, 3 broken" is worse than no report at all.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TotpSecretTally {
    /// Readable with the active JWT secret. The steady state.
    pub under_current: usize,
    /// Readable only with `CRONIQ_JWT_SECRET_PREVIOUS` (issue #531). Not
    /// broken — the login path falls back — but the variable cannot be
    /// dropped while any of these remain.
    pub under_previous: usize,
    /// Readable with neither (issue #408).
    pub undecryptable: usize,
}

impl TotpSecretTally {
    /// Confirmed secrets examined, in total.
    pub fn total(&self) -> usize {
        self.under_current + self.under_previous + self.undecryptable
    }
}

/// Sort every confirmed TOTP secret by the key it decrypts under.
///
/// The at-rest wrap key is HKDF-derived from the JWT secret, so a secret change
/// — the classic one being an upgrade past 0.29.0 that drops a
/// `pull_api { auth … }` line and falls through to a freshly generated
/// `$DATA_DIR/jwt.secret` — silently makes every stored secret unusable. Login
/// then fails with a bare 500 for those users while the server is otherwise
/// healthy, which is very hard to diagnose from the outside (issue #408).
/// Passing the pre-rotation secret as `previous` separates "still to be
/// re-wrapped" from "genuinely lost", which are very different problems
/// (issue #531).
///
/// Only *confirmed* (`enabled`) secrets count: an unconfirmed enrolment is
/// replaced by the next attempt and locks nobody out. A store error yields an
/// empty tally so a transient hiccup never fabricates a scary finding.
pub fn tally_totp_secrets(
    store: &crate::store::DynStore,
    jwt_secret: &str,
    previous: Option<&str>,
) -> TotpSecretTally {
    use croniq_auth::crypto::{WrappedUnder, unwrap_totp_secret_with_previous};

    let mut tally = TotpSecretTally::default();
    let Ok(users) = store.users_list() else {
        return tally;
    };
    for row in users
        .iter()
        .filter_map(|u| store.totp_get(&u.user_id).ok().flatten())
        .filter(|t| t.enabled)
    {
        match unwrap_totp_secret_with_previous(jwt_secret, previous, &row.secret_enc) {
            Ok((_, WrappedUnder::Current)) => tally.under_current += 1,
            Ok((_, WrappedUnder::Previous)) => tally.under_previous += 1,
            Err(_) => tally.undecryptable += 1,
        }
    }
    tally
}

/// The boot-time notice logged when enforced 2FA is on.
///
/// Lives here so it cannot drift from the `totp.enforced_without_enrollment`
/// finding below — the two used to contradict each other in the same startup
/// log, one saying users are guided through inline enrolment and the other that
/// they are refused at login (issue #409). `enforced_2fa_messages_agree` locks
/// the agreement in.
pub const ENFORCED_TOTP_BOOT_NOTICE: &str = "enforced 2FA is ON — every password login must present a TOTP or recovery code. Users \
     without a confirmed secret are guided through inline enrolment on next sign-in (not locked \
     out). If you've lost both the authenticator and all recovery codes, set \
     auth { totp { required false } } (or CRONIQ_REQUIRE_TOTP=false), re-enrol, then re-enable.";

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
    // Informational on purpose: these accounts are *not* locked out. After a
    // correct password, login answers `enrollment_required` with a short-lived
    // enrol token and the UI completes setup inline, so this is the expected
    // state right after enforcement is switched on (issue #409).
    if let (true, Some(n)) = (input.totp_enforced, input.users_without_totp)
        && n > 0
    {
        out.push(Diagnostic {
            id: "totp.enforced_without_enrollment",
            severity: Severity::Info,
            title: format!("{n} active user(s) will be walked through TOTP enrolment at sign-in"),
            detail: "Enforced two-factor (require_totp) is on and these accounts have no \
                     confirmed TOTP secret. Their next sign-in verifies the password as usual, \
                     then POST /v1/auth/login answers `enrollment_required` with a short-lived \
                     enrol token and the sign-in page completes TOTP setup inline via \
                     /v1/auth/login/enroll/totp/{begin,confirm}."
                .into(),
            remedy: None,
        });
    }

    // ── Stored TOTP secrets, by the key they decrypt under ──────────────
    if let Some(tally) = input.totp_secrets {
        if tally.undecryptable > 0 {
            let n = tally.undecryptable;
            out.push(Diagnostic {
                id: "totp.secrets_undecryptable",
                severity: Severity::Critical,
                title: format!("{n} stored TOTP secret(s) cannot be decrypted"),
                detail: "These accounts have a confirmed TOTP secret that fails to unwrap with \
                         the active JWT secret, so the at-rest wrap key is not the one they were \
                         stored with — the JWT secret changed. Affected users get a 500 from \
                         POST /v1/auth/login when they submit a valid code; recovery codes still \
                         work (they are hashed, not wrapped)."
                    .into(),
                remedy: Some(
                    "Name the outgoing value in CRONIQ_JWT_SECRET_PREVIOUS (or its _FILE \
                     sibling) and restart: the server re-wraps every stored secret under the \
                     current key at boot, so no user has to re-enrol. Restoring it as \
                     CRONIQ_JWT_SECRET works too. If the old value is gone for good, have each \
                     user sign in with a recovery code and re-enrol (Settings → Security); with \
                     no recovery code left, an admin must reset their second factor."
                        .into(),
                ),
            });
        }

        // Not an error — these still authenticate through the unwrap fallback
        // — but the rotation is not finished and the variable cannot be
        // dropped yet, which is exactly what an operator is trying to find out.
        if tally.under_previous > 0 {
            let n = tally.under_previous;
            out.push(Diagnostic {
                id: "totp.secrets_under_previous_key",
                severity: Severity::Warning,
                title: format!("{n} stored TOTP secret(s) are still under the previous JWT secret"),
                detail: "The boot-time re-wrap could not write these rows back under the current \
                         key, so they only unwrap because CRONIQ_JWT_SECRET_PREVIOUS is still \
                         set. Sign-in works; removing the variable would break it."
                    .into(),
                remedy: Some(
                    "Check the server log for the store error the re-wrap reported and restart \
                     to retry the sweep. Keep CRONIQ_JWT_SECRET_PREVIOUS set until this finding \
                     is gone."
                        .into(),
                ),
            });
        }

        // The positive counterpart (issue #531): after a rotation an operator
        // needs to see that the re-wrap finished, not merely the absence of a
        // complaint. Silence is what made the old failure mode so hard to spot.
        if tally.total() > 0 && tally.under_previous == 0 && tally.undecryptable == 0 {
            let n = tally.total();
            out.push(Diagnostic {
                id: "totp.secrets_decryptable",
                severity: Severity::Info,
                title: format!("{n} stored TOTP secret(s), all decryptable under the current key"),
                detail: "Every confirmed second factor unwraps with the active JWT secret, so a \
                         completed rotation left nothing behind and CRONIQ_JWT_SECRET_PREVIOUS \
                         (if set) can be removed."
                    .into(),
                remedy: None,
            });
        }
    }

    // ── Unbounded run history ───────────────────────────────────────────
    // Never above Info: keeping history forever is a legitimate choice and the
    // opt-in default exists so an upgrade cannot silently delete run history
    // (#344). The value is in surfacing the decision, so `doctor`'s exit code
    // must not turn non-zero for it (issue #405).
    if !input.retention_configured {
        out.push(Diagnostic {
            id: "retention.unbounded_history",
            severity: Severity::Info,
            title: "Run history is never pruned".into(),
            detail: "Neither server { execution_retention } nor any keep_last is configured, so \
                     terminal executions and their logs accumulate for as long as this server \
                     lives. Retention is opt-in by design — an upgrade must never delete run \
                     history — but nothing currently caps the growth."
                .into(),
            remedy: Some(
                "Cap it with server { execution_retention 30d } and/or keep_last N in \
                 defaults { } or a job { } (see the retention section of docs/operations.md). \
                 Pruning stops the growth but does not shrink the database file — reclaiming \
                 that space needs an explicit VACUUM."
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
            retention_configured: true,
            totp_secrets: Some(TotpSecretTally::default()),
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
            retention_configured: false,
            totp_secrets: Some(TotpSecretTally::default()),
        };
        let ds = run_diagnostics(&input);
        assert_eq!(
            ids(&ds),
            vec![
                "email.delivery",
                "links.app_url",
                "totp.enforced_without_enrollment",
                "retention.unbounded_history",
            ]
        );
    }

    #[test]
    fn enforced_2fa_without_enrollment_is_informational() {
        let input = DiagnosticsInput {
            totp_enforced: true,
            users_without_totp: Some(2),
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["totp.enforced_without_enrollment"]);
        assert_eq!(ds[0].severity, Severity::Info);
        assert!(ds[0].title.contains('2'));
    }

    /// Issue #409: the finding claimed affected users "are refused at login"
    /// and recommended turning enforced 2FA off — a security downgrade for a
    /// lockout that does not happen. Login hands back an enrolment token.
    #[test]
    fn enforced_2fa_finding_does_not_claim_a_lockout() {
        let input = DiagnosticsInput {
            totp_enforced: true,
            users_without_totp: Some(1),
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        let d = &ds[0];
        let text = format!("{} {} {:?}", d.title, d.detail, d.remedy).to_lowercase();
        for claim in ["refused", "cannot sign in", "locked out"] {
            assert!(!text.contains(claim), "still claims '{claim}': {text}");
        }
        // …and must not recommend disabling enforcement as the remedy.
        assert!(
            !text.contains("required false"),
            "recommends 2FA off: {text}"
        );
        assert!(text.contains("enrol"), "does not mention enrolment: {text}");
    }

    /// The boot notice and the finding are logged in the same startup, so they
    /// must not contradict each other (issue #409).
    #[test]
    fn enforced_2fa_messages_agree() {
        let notice = ENFORCED_TOTP_BOOT_NOTICE.to_lowercase();
        // The notice's own claim: guided through inline enrolment, not locked out.
        assert!(notice.contains("inline enrolment"), "{notice}");
        assert!(notice.contains("not locked out"), "{notice}");

        let input = DiagnosticsInput {
            totp_enforced: true,
            users_without_totp: Some(1),
            ..healthy()
        };
        let finding = &run_diagnostics(&input)[0];
        let text = format!("{} {}", finding.title, finding.detail).to_lowercase();
        assert!(
            text.contains("enrol") && !text.contains("refused"),
            "finding disagrees with the boot notice: {text}"
        );
        // `required false` belongs to the genuine lockout case only (#408), and
        // the notice scopes it that way ("lost both the authenticator and all
        // recovery codes"); the finding must not repeat it as a remedy.
        assert!(notice.contains("recovery codes"), "{notice}");
        assert!(!text.contains("required false"), "{text}");
    }

    #[test]
    fn undecryptable_totp_secrets_are_critical() {
        let input = DiagnosticsInput {
            totp_secrets: Some(TotpSecretTally {
                undecryptable: 3,
                ..Default::default()
            }),
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["totp.secrets_undecryptable"]);
        assert_eq!(ds[0].severity, Severity::Critical);
        assert!(ds[0].title.contains('3'));
        // Must name the way back in — the 500 alone says nothing.
        let remedy = ds[0].remedy.as_deref().unwrap_or_default();
        assert!(remedy.contains("CRONIQ_JWT_SECRET"), "got: {remedy}");
        // Since #531 the cheap way back is the re-wrap, not a re-enrolment
        // campaign, so the remedy has to name it.
        assert!(
            remedy.contains("CRONIQ_JWT_SECRET_PREVIOUS"),
            "got: {remedy}"
        );
        assert!(ds[0].detail.contains("recovery codes"), "{}", ds[0].detail);
    }

    #[test]
    fn undecryptable_totp_not_evaluated_is_silent() {
        // Offline `doctor`: no store / no secret → nothing to report.
        let input = DiagnosticsInput {
            totp_secrets: None,
            ..healthy()
        };
        assert!(run_diagnostics(&input).is_empty());
    }

    #[test]
    fn secrets_still_under_the_previous_key_warn_without_alarming() {
        // They authenticate — the login path falls back — so this must not be
        // Critical; but the rotation is unfinished and the variable is pinned.
        let input = DiagnosticsInput {
            totp_secrets: Some(TotpSecretTally {
                under_current: 5,
                under_previous: 2,
                undecryptable: 0,
            }),
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["totp.secrets_under_previous_key"]);
        assert_eq!(ds[0].severity, Severity::Warning);
        assert!(ds[0].title.contains('2'), "{}", ds[0].title);
        let remedy = ds[0].remedy.as_deref().unwrap_or_default();
        assert!(
            remedy.contains("CRONIQ_JWT_SECRET_PREVIOUS"),
            "got: {remedy}"
        );
    }

    #[test]
    fn a_completed_rotation_is_reported_as_a_positive() {
        // The doctor half of issue #531: an operator needs to see that the
        // re-wrap finished, not merely the absence of a complaint.
        let input = DiagnosticsInput {
            totp_secrets: Some(TotpSecretTally {
                under_current: 7,
                under_previous: 0,
                undecryptable: 0,
            }),
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["totp.secrets_decryptable"]);
        assert_eq!(ds[0].severity, Severity::Info);
        assert!(ds[0].title.contains('7'), "{}", ds[0].title);
        // Info only — `doctor` must not fail a deploy over good news.
        assert!(ds[0].remedy.is_none());
    }

    #[test]
    fn the_positive_line_stays_away_while_anything_is_wrong() {
        for tally in [
            TotpSecretTally {
                under_current: 1,
                under_previous: 1,
                undecryptable: 0,
            },
            TotpSecretTally {
                under_current: 1,
                under_previous: 0,
                undecryptable: 1,
            },
        ] {
            let ds = run_diagnostics(&DiagnosticsInput {
                totp_secrets: Some(tally),
                ..healthy()
            });
            assert!(
                !ids(&ds).contains(&"totp.secrets_decryptable"),
                "got: {:?}",
                ids(&ds)
            );
        }
    }

    #[test]
    fn unbounded_retention_is_informational() {
        let input = DiagnosticsInput {
            retention_configured: false,
            ..healthy()
        };
        let ds = run_diagnostics(&input);
        assert_eq!(ids(&ds), vec!["retention.unbounded_history"]);
        // Never critical: keeping history forever is a legitimate choice, so
        // `doctor` must still exit 0 (issue #405).
        assert_eq!(ds[0].severity, Severity::Info);
        assert!(ds[0].remedy.is_some());
    }

    #[test]
    fn configured_retention_is_silent() {
        let input = DiagnosticsInput {
            retention_configured: true,
            ..healthy()
        };
        assert!(run_diagnostics(&input).is_empty());
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

    // ── Store-backed fact gathering ─────────────────────────────────────

    /// A store with one active user whose confirmed TOTP secret was wrapped
    /// with `wrapped_with`.
    fn store_with_enrolled_user(wrapped_with: &str) -> crate::store::DynStore {
        use croniq_store::models::{Role, TotpSecret, User};
        let store =
            crate::store::sqlite_store(croniq_store::sqlite::SqliteStore::in_memory().unwrap());
        let now = chrono::Utc::now();
        store
            .users_create(&User {
                user_id: "u1".into(),
                username: "admin".into(),
                email: None,
                display_name: None,
                role: Role::Admin,
                is_active: true,
                created_at: now,
                updated_at: now,
                last_login_at: None,
            })
            .unwrap();
        store
            .totp_upsert(&TotpSecret {
                user_id: "u1".into(),
                secret_enc: croniq_auth::crypto::wrap_totp_secret(
                    wrapped_with,
                    b"JBSWY3DPEHPK3PXP",
                )
                .unwrap(),
                enabled: true,
                confirmed_at: Some(now),
                created_at: now,
            })
            .unwrap();
        store
    }

    #[test]
    fn undecryptable_count_reacts_to_the_active_secret() {
        let store = store_with_enrolled_user("old-secret");
        // The secret it was wrapped with still opens it…
        assert_eq!(
            tally_totp_secrets(&store, "old-secret", None).undecryptable,
            0
        );
        // …a rotated one does not (the #408 upgrade path).
        assert_eq!(
            tally_totp_secrets(&store, "new-secret", None).undecryptable,
            1
        );
    }

    #[test]
    fn naming_the_previous_secret_moves_a_row_out_of_undecryptable() {
        // Same row, same rotated secret — but with the old value named it is
        // pending a re-wrap, not lost (issue #531).
        let store = store_with_enrolled_user("old-secret");
        let tally = tally_totp_secrets(&store, "new-secret", Some("old-secret"));
        assert_eq!(
            tally,
            TotpSecretTally {
                under_current: 0,
                under_previous: 1,
                undecryptable: 0,
            }
        );
        assert_eq!(tally.total(), 1);
    }

    #[test]
    fn a_wrong_previous_secret_leaves_the_row_undecryptable() {
        let store = store_with_enrolled_user("old-secret");
        let tally = tally_totp_secrets(&store, "new-secret", Some("not-the-old-one"));
        assert_eq!(tally.undecryptable, 1);
        assert_eq!(tally.under_previous, 0);
    }

    #[test]
    fn undecryptable_count_ignores_unconfirmed_secrets() {
        let store = store_with_enrolled_user("old-secret");
        store.totp_set_enabled("u1", false, None).unwrap();
        // A pending enrolment locks nobody out — it is replaced on retry.
        let tally = tally_totp_secrets(&store, "new-secret", None);
        assert_eq!(tally.undecryptable, 0);
        assert_eq!(tally.total(), 0);
    }

    #[test]
    fn from_runtime_reports_a_rotated_secret_as_critical() {
        let store = store_with_enrolled_user("old-secret");
        let input = DiagnosticsInput::from_runtime(RuntimeFacts {
            app_base_url_configured: true,
            email_delivery_active: true,
            totp_enforced: false,
            retention_configured: true,
            store: Some(&store),
            jwt_secret: Some("new-secret"),
            previous_jwt_secret: None,
        });
        assert_eq!(input.totp_secrets.unwrap().undecryptable, 1);
        let ds = run_diagnostics(&input);
        assert!(
            ds.iter().any(|d| d.id == "totp.secrets_undecryptable"),
            "got: {:?}",
            ids(&ds)
        );
    }

    #[test]
    fn from_runtime_skips_store_checks_without_a_secret() {
        // `doctor` offline: no store and no secret → nothing evaluated, so the
        // report can't invent findings it has no evidence for.
        let input = DiagnosticsInput::from_runtime(RuntimeFacts {
            app_base_url_configured: true,
            email_delivery_active: true,
            totp_enforced: true,
            retention_configured: true,
            store: None,
            jwt_secret: None,
            previous_jwt_secret: None,
        });
        assert_eq!(input.totp_secrets, None);
        assert_eq!(input.users_without_totp, None);
        assert!(run_diagnostics(&input).is_empty());
    }

    // ── retention_configured ────────────────────────────────────────────

    fn compiled(src: &str) -> croniq_config::compile::RuntimeConfig {
        let ast = croniq_config::parser::Parser::parse(src).unwrap();
        croniq_config::compile::compile(&ast)
    }

    #[test]
    fn retention_detects_each_knob() {
        let job = r#"job a:b { every 1 hour }"#;
        // Neither knob → unbounded.
        assert!(!retention_configured(&compiled(job)));
        // Age sweep.
        assert!(retention_configured(&compiled(&format!(
            "server {{ execution_retention 30d }}\n{job}"
        ))));
        // Per-job cap, set directly…
        assert!(retention_configured(&compiled(
            r#"job a:b { every 1 hour
                        keep_last 50 }"#
        )));
        // …and inherited from defaults, which compile folds into every job.
        assert!(retention_configured(&compiled(&format!(
            "defaults {{ keep_last 50 }}\n{job}"
        ))));
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
