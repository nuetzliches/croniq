//! Re-wrap stored TOTP seeds after a `CRONIQ_JWT_SECRET` rotation (issue #531).
//!
//! The at-rest key for TOTP seeds is HKDF-derived from the signing secret, so
//! rotating the signing secret rotates the wrap key with it and every enrolled
//! user loses their second factor. The coupling itself is deliberate — anyone
//! who can read the signing secret can already mint admin tokens, so a second
//! key store would not raise the bar — but until this module existed the only
//! documented way through a rotation was: relax `auth { totp { required } }`,
//! rotate, have every user re-enrol, re-enable. That lowers the security
//! posture during exactly the window where it should be highest (the usual
//! reason to rotate is a suspected leak), and it scales with the number of
//! users rather than the number of operators — so in practice it does not
//! happen, and the secret stays as old as the deployment.
//!
//! Naming the outgoing value in `CRONIQ_JWT_SECRET_PREVIOUS` turns that into an
//! administrative step. At boot, before the server accepts traffic,
//! [`rewrap_all`] reads every stored seed with the old key and writes it back
//! under the new one. A rotation becomes: set both, restart, drop the old
//! value.
//!
//! Two deliberate choices:
//!
//! * **The sweep never fails the boot.** A row it cannot write is reported and
//!   skipped. Refusing to start would turn a failed *convenience migration*
//!   into an outage, and the affected rows still authenticate through the
//!   unwrap fallback on [`croniq_auth::jwt::JwtConfig::previous_secret`].
//! * **The old key never wraps anything.** Everything written after the
//!   rotation is under the current key by construction, so dropping
//!   `CRONIQ_JWT_SECRET_PREVIOUS` after a clean sweep is safe.

use croniq_auth::crypto::{WrappedUnder, unwrap_totp_secret_with_previous, wrap_totp_secret};

use crate::store::DynStore;

/// The name operators set. Also accepts the `_FILE` sibling via
/// [`crate::env_secret::env_or_file`], matching every other credential
/// variable.
pub const PREVIOUS_SECRET_VAR: &str = "CRONIQ_JWT_SECRET_PREVIOUS";

/// What one sweep did. Every stored TOTP row lands in exactly one bucket.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RewrapReport {
    /// Already readable with the current key — nothing to do. After a
    /// completed rotation this is every row.
    pub already_current: usize,
    /// Read with the previous key and written back under the current one.
    pub rewrapped: usize,
    /// Read with the previous key, but the store rejected the write. Still
    /// readable at login through the unwrap fallback; the operator has to keep
    /// `CRONIQ_JWT_SECRET_PREVIOUS` set until a later sweep succeeds.
    pub write_failed: usize,
    /// Readable with neither key. Predates *two* rotations, or the value in
    /// `CRONIQ_JWT_SECRET_PREVIOUS` is not the one these rows were stored
    /// with. `doctor` reports these as `totp.secrets_undecryptable`.
    pub undecryptable: usize,
}

impl RewrapReport {
    /// Rows the sweep left under the old key because it could not write them.
    /// While this is non-zero, `CRONIQ_JWT_SECRET_PREVIOUS` must stay set.
    pub fn still_needs_previous(&self) -> bool {
        self.write_failed > 0
    }

    /// Whether anything at all was examined.
    pub fn total(&self) -> usize {
        self.already_current + self.rewrapped + self.write_failed + self.undecryptable
    }
}

/// Re-wrap every stored TOTP seed that is under `previous` so it is under
/// `current` instead.
///
/// Walks users rather than the `totp_secrets` table directly because that is
/// the access the store trait offers (`totp_get` is keyed by user), and it is
/// the same walk the `totp.secrets_undecryptable` diagnostic already does.
/// Both confirmed and pending secrets are re-wrapped: a pending enrolment is
/// worthless after the rotation otherwise, and re-wrapping it costs one row.
///
/// A store error listing users yields an empty report — the same
/// treat-a-hiccup-as-nothing posture the diagnostics checks take — rather than
/// a fabricated count.
pub fn rewrap_all(store: &DynStore, current: &str, previous: &str) -> RewrapReport {
    let mut report = RewrapReport::default();
    let Ok(users) = store.users_list() else {
        tracing::error!(
            "could not list users to re-wrap stored TOTP secrets; skipping the sweep. \
             Stored secrets still unwrap through the {PREVIOUS_SECRET_VAR} fallback."
        );
        return report;
    };

    for user in &users {
        let Ok(Some(row)) = store.totp_get(&user.user_id) else {
            continue;
        };
        match unwrap_totp_secret_with_previous(current, Some(previous), &row.secret_enc) {
            Ok((_, WrappedUnder::Current)) => report.already_current += 1,
            Ok((plaintext, WrappedUnder::Previous)) => {
                let Ok(rewrapped) = wrap_totp_secret(current, &plaintext) else {
                    // AES-GCM encryption of a 16-byte seed does not fail in
                    // practice; count it with the write failures so the row is
                    // never silently reported as done.
                    report.write_failed += 1;
                    continue;
                };
                // Write the row back whole so `enabled` and `confirmed_at`
                // survive: `totp_upsert`'s ON CONFLICT updates all three
                // columns, and a re-wrap must not quietly un-confirm a user.
                let updated = croniq_store::models::TotpSecret {
                    secret_enc: rewrapped,
                    ..row.clone()
                };
                match store.totp_upsert(&updated) {
                    Ok(()) => report.rewrapped += 1,
                    Err(e) => {
                        tracing::error!(
                            user_id = %user.user_id,
                            error = %e,
                            "could not write the re-wrapped TOTP secret; it stays under the \
                             previous key and keeps working through the fallback"
                        );
                        report.write_failed += 1;
                    }
                }
            }
            Err(_) => report.undecryptable += 1,
        }
    }
    report
}

/// Run the sweep and log what it did, in the shape an operator can act on.
///
/// Separated from [`rewrap_all`] so the counting is testable without a
/// tracing subscriber, and so `main` stays a list of steps.
pub fn run_and_log(store: &DynStore, current: &str, previous: &str) -> RewrapReport {
    if current == previous {
        tracing::warn!(
            "{PREVIOUS_SECRET_VAR} holds the same value as CRONIQ_JWT_SECRET, so there is \
             nothing to re-wrap. If you meant to rotate, set CRONIQ_JWT_SECRET to the new \
             value and leave {PREVIOUS_SECRET_VAR} on the old one."
        );
        return RewrapReport::default();
    }

    let report = rewrap_all(store, current, previous);

    if report.total() == 0 {
        tracing::info!(
            "{PREVIOUS_SECRET_VAR} is set but no TOTP secrets are stored; it can be removed."
        );
        return report;
    }

    tracing::info!(
        rewrapped = report.rewrapped,
        already_current = report.already_current,
        write_failed = report.write_failed,
        undecryptable = report.undecryptable,
        "re-wrapped stored TOTP secrets under the current JWT secret"
    );

    if report.undecryptable > 0 {
        tracing::warn!(
            count = report.undecryptable,
            "these stored TOTP secrets decrypt under neither the current nor the previous JWT \
             secret — {PREVIOUS_SECRET_VAR} is not the value they were stored with. They stay \
             unusable; affected users sign in with a recovery code and re-enrol. \
             `croniq-server doctor` reports this as totp.secrets_undecryptable."
        );
    }

    if report.still_needs_previous() {
        tracing::warn!(
            count = report.write_failed,
            "keep {PREVIOUS_SECRET_VAR} set: some secrets could not be written back and still \
             need it to unwrap. Fix the store error and restart to retry the sweep."
        );
    } else {
        tracing::info!("{PREVIOUS_SECRET_VAR} can now be removed.");
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use croniq_store::models::{Role, TotpSecret, User};

    const OLD: &str = "old-jwt-secret";
    const NEW: &str = "new-jwt-secret";
    const SEED: &[u8] = b"JBSWY3DPEHPK3PXP";

    fn user(user_id: &str) -> User {
        let now = Utc::now();
        User {
            user_id: user_id.to_string(),
            username: user_id.to_string(),
            email: None,
            display_name: None,
            role: Role::Admin,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        }
    }

    fn empty_store() -> DynStore {
        crate::store::sqlite_store(croniq_store::sqlite::SqliteStore::in_memory().unwrap())
    }

    fn store_with(rows: &[(&str, &str, bool)]) -> DynStore {
        let store = empty_store();
        for (user_id, secret_enc, enabled) in rows {
            store.users_create(&user(user_id)).unwrap();
            store
                .totp_upsert(&TotpSecret {
                    user_id: (*user_id).to_string(),
                    secret_enc: (*secret_enc).to_string(),
                    enabled: *enabled,
                    confirmed_at: enabled.then(Utc::now),
                    created_at: Utc::now(),
                })
                .unwrap();
        }
        store
    }

    fn wrapped(secret: &str) -> String {
        wrap_totp_secret(secret, SEED).unwrap()
    }

    #[test]
    fn a_row_under_the_previous_key_is_rewrapped_under_the_current_one() {
        let store = store_with(&[("alice", &wrapped(OLD), true)]);
        let report = rewrap_all(&store, NEW, OLD);
        assert_eq!(report.rewrapped, 1);
        assert_eq!(report.already_current, 0);
        assert_eq!(report.undecryptable, 0);

        // The point of the exercise: the row now reads with the current key
        // alone, so CRONIQ_JWT_SECRET_PREVIOUS can be dropped.
        let row = store.totp_get("alice").unwrap().unwrap();
        assert_eq!(
            croniq_auth::crypto::unwrap_totp_secret(NEW, &row.secret_enc).unwrap(),
            SEED
        );
    }

    #[test]
    fn rewrapping_preserves_enabled_and_confirmed_at() {
        // A re-wrap that un-confirmed a user would lock them out just as
        // thoroughly as the rotation it is meant to fix.
        let store = store_with(&[("alice", &wrapped(OLD), true)]);
        let before = store.totp_get("alice").unwrap().unwrap();
        rewrap_all(&store, NEW, OLD);
        let after = store.totp_get("alice").unwrap().unwrap();
        assert!(after.enabled);
        assert_eq!(after.confirmed_at, before.confirmed_at);
        assert_ne!(after.secret_enc, before.secret_enc);
    }

    #[test]
    fn a_pending_enrolment_is_rewrapped_too() {
        let store = store_with(&[("alice", &wrapped(OLD), false)]);
        let report = rewrap_all(&store, NEW, OLD);
        assert_eq!(report.rewrapped, 1);
        let after = store.totp_get("alice").unwrap().unwrap();
        assert!(!after.enabled, "an unconfirmed row must stay unconfirmed");
    }

    #[test]
    fn a_row_already_under_the_current_key_is_left_alone() {
        let already = wrapped(NEW);
        let store = store_with(&[("alice", &already, true)]);
        let report = rewrap_all(&store, NEW, OLD);
        assert_eq!(report.already_current, 1);
        assert_eq!(report.rewrapped, 0);
        assert_eq!(
            store.totp_get("alice").unwrap().unwrap().secret_enc,
            already,
            "an untouched row must not be re-encrypted — that is churn, not work"
        );
    }

    #[test]
    fn a_row_under_neither_key_is_counted_not_destroyed() {
        let stranger = wrapped("a-third-secret-entirely");
        let store = store_with(&[("alice", &stranger, true)]);
        let report = rewrap_all(&store, NEW, OLD);
        assert_eq!(report.undecryptable, 1);
        assert_eq!(report.rewrapped, 0);
        assert_eq!(
            store.totp_get("alice").unwrap().unwrap().secret_enc,
            stranger,
            "an unreadable row must be left byte-for-byte intact — the operator may \
             still find the secret it belongs to"
        );
    }

    #[test]
    fn a_mixed_deployment_is_reported_bucket_by_bucket() {
        let store = store_with(&[
            ("alice", &wrapped(OLD), true),
            ("bob", &wrapped(OLD), true),
            ("carol", &wrapped(NEW), true),
            ("dave", &wrapped("something-else"), true),
        ]);
        let report = rewrap_all(&store, NEW, OLD);
        assert_eq!(
            report,
            RewrapReport {
                already_current: 1,
                rewrapped: 2,
                write_failed: 0,
                undecryptable: 1,
            }
        );
        assert_eq!(report.total(), 4);
        assert!(!report.still_needs_previous());
    }

    #[test]
    fn a_second_sweep_is_a_no_op() {
        // Idempotence matters: an operator who leaves the variable set for a
        // few restarts must not see the rows churn on every boot.
        let store = store_with(&[("alice", &wrapped(OLD), true)]);
        rewrap_all(&store, NEW, OLD);
        let after_first = store.totp_get("alice").unwrap().unwrap().secret_enc;
        let second = rewrap_all(&store, NEW, OLD);
        assert_eq!(second.already_current, 1);
        assert_eq!(second.rewrapped, 0);
        assert_eq!(
            store.totp_get("alice").unwrap().unwrap().secret_enc,
            after_first
        );
    }

    #[test]
    fn a_user_without_a_totp_secret_is_not_counted() {
        let store = store_with(&[("alice", &wrapped(OLD), true)]);
        store.users_create(&user("bob")).unwrap();
        assert_eq!(rewrap_all(&store, NEW, OLD).total(), 1);
    }

    #[test]
    fn an_identical_previous_value_is_refused_rather_than_swept() {
        let store = store_with(&[("alice", &wrapped(NEW), true)]);
        assert_eq!(run_and_log(&store, NEW, NEW), RewrapReport::default());
    }
}
