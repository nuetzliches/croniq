//! `croniq init` — initialize database with an admin user.
//!
//! A default API client + key is only seeded when `--api-key` is passed.
//! Operators who need keys should create scoped clients via the UI
//! (Settings → API Keys) or `POST /v1/api-clients`.

use std::path::Path;

use chrono::Utc;
use croniq_auth::api_key::hash_api_key;
use croniq_auth::crypto::wrap_totp_secret;
use croniq_auth::jwt_secret;
use croniq_auth::password::{hash_password, validate_password};
use croniq_auth::totp::{enroll_user, hash_recovery_code};
use croniq_store::models::{
    ApiClient, ApiKey, PasswordCredential, RecoveryCode, Role, TotpSecret, User,
};
use croniq_store::sqlite::SqliteStore;
use croniq_store::traits::AuthStore;
use miette::{IntoDiagnostic, Result, miette};
use uuid::Uuid;

use super::secret_output::CredentialSink;

/// Recovery code baked into the demo seed so a marketing walkthrough
/// can reach the MFA step at `admin/demo-admin` and complete it with a
/// fixed code. Mirrored from issue #137 — never use outside the demo
/// image.
const DEMO_MFA_RECOVERY_CODE: &str = "123456";

pub fn init(
    data_dir: &Path,
    username: &str,
    password: Option<&str>,
    api_key_override: Option<&str>,
    scopes: Option<Vec<String>>,
    demo_mfa: bool,
    sink: &mut CredentialSink,
) -> Result<()> {
    // Validate `--api-key` and `--scopes` up front, before any disk/DB writes,
    // so a malformed key cannot leave behind a half-initialized DB (admin user
    // created, no API key persisted) that masks the failure on the next start.
    let seed_request: Option<(&str, Vec<String>)> = match api_key_override {
        Some(raw_key) => {
            if !raw_key.starts_with("croniq_") {
                return Err(miette!(
                    "--api-key must start with 'croniq_' (got prefix '{}…'); \
                     e.g. CRONIQ_INIT_API_KEY=croniq_$(openssl rand -hex 32)",
                    raw_key.chars().take(6).collect::<String>()
                ));
            }
            let resolved = scopes.unwrap_or_else(|| vec!["admin".to_string()]);
            if resolved.is_empty() {
                return Err(miette!("--scopes must list at least one scope"));
            }
            Some((raw_key, resolved))
        }
        None => None,
    };

    let password = match password {
        Some(p) => p.to_string(),
        None => {
            eprintln!("Enter admin password: ");
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).into_diagnostic()?;
            buf.trim().to_string()
        }
    };
    // The very first admin password is held to exactly the same policy as
    // every later one (`POST /v1/users`, change-password, password reset,
    // invitation accept) — one shared constant in croniq-auth, no
    // "non-empty is good enough" first-run exception (issue #428).
    validate_password(&password).map_err(|e| miette!("{e}"))?;

    std::fs::create_dir_all(data_dir).into_diagnostic()?;
    let db_path = data_dir.join("croniq.db");

    println!("Opening database at {}", db_path.display());
    let store = SqliteStore::open(&db_path).map_err(|e| miette!("Failed to open database: {e}"))?;

    let now = Utc::now();
    let user_id = Uuid::new_v4().to_string();

    let pw_hash = hash_password(&password).map_err(|e| miette!("Failed to hash password: {e}"))?;

    // Create the identity row first so the credential's user_id has
    // something to point at conceptually. Migration 011 backfills
    // existing single-admin password_credentials rows into users; for
    // fresh inits we create both side by side.
    store
        .users_create(&User {
            user_id: user_id.clone(),
            username: username.to_string(),
            email: None,
            display_name: None,
            role: Role::Admin,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .map_err(|e| miette!("Failed to create admin user identity: {e}"))?;

    store
        .upsert_credentials(&PasswordCredential {
            user_id: user_id.clone(),
            username: username.to_string(),
            password_hash: pw_hash,
            failed_attempts: 0,
            locked_until: None,
            created_at: now,
        })
        .map_err(|e| miette!("Failed to create admin password: {e}"))?;

    println!("Admin user '{}' created (role: admin).", username);

    // Seed a default API client + key only when an explicit key is provided.
    // This keeps reproducible setups (docker-compose demo, CI) working while
    // skipping auto-seeded admin-scope credentials for production installs.
    // Prefix and scope validation already happened up top — see fail-fast block.
    let seeded_key = if let Some((raw_key, resolved_scopes)) = seed_request {
        let client_id = Uuid::new_v4().to_string();
        store
            .create_client(&ApiClient {
                client_id: client_id.clone(),
                name: "default".to_string(),
                scopes: resolved_scopes.clone(),
                is_active: true,
                created_at: now,
                // Operator-owned, not env-owned, even though the Docker
                // entrypoint sources `--api-key` from CRONIQ_INIT_API_KEY:
                // `croniq init` cannot tell an env-declared key from one an
                // operator typed. The environment takes ownership only when
                // the operator opts in with CRONIQ_API_KEY_RECONCILE=1, so
                // the default Docker flow keeps behaving exactly as before
                // (issue #471).
                managed_by: "api".to_string(),
            })
            .map_err(|e| miette!("Failed to create API client: {e}"))?;

        let key_hash = hash_api_key(raw_key);
        let key_prefix = raw_key.chars().take(12).collect();
        store
            .create_api_key(&ApiKey {
                key_id: Uuid::new_v4().to_string(),
                client_id,
                key_hash,
                key_prefix,
                expires_at: None,
                revoked_at: None,
                created_at: now,
            })
            .map_err(|e| miette!("Failed to create API key: {e}"))?;

        println!(
            "API client 'default' seeded with provided key (scopes: {}).",
            resolved_scopes.join(", ")
        );
        Some(raw_key.to_string())
    } else {
        None
    };

    // Demo-only: pre-enable TOTP so a marketing walkthrough of the demo
    // admin login hits the MFA step instead of jumping straight to the
    // dashboard. Real
    // TOTP codes are time-based, so we bake "123456" into one of the
    // recovery codes — operators in the demo image type that at the
    // recovery prompt. The TOTP secret is generated normally so anyone who
    // scans the (unprinted) QR with an authenticator app still gets valid
    // 6-digit codes side-by-side with the fixed recovery code.
    if demo_mfa {
        seed_demo_mfa(&store, &user_id, username, data_dir, now)?;
    }

    println!();
    println!("=== Initialization complete ===");
    println!();
    println!("Admin login:");
    println!("  Username: {}", username);
    println!("  Password: (as provided)");
    println!();

    match seeded_key {
        Some(raw_key) => {
            let usage = format!("Authorization: ApiKey {raw_key}");
            sink.add_with_usage("API Key", raw_key, usage);
        }
        None => {
            println!("No API key was seeded. Create scoped clients and keys via:");
            println!("  UI  → Settings → API Keys");
            println!("  API → POST /v1/api-clients, then POST /v1/api-keys");
            println!("  CLI → rerun `croniq init --api-key croniq_...` for reproducible seeds");
            println!();
        }
    }

    println!("Or login via:");
    println!(
        "  POST /v1/auth/login {{\"username\": \"{}\", \"password\": \"...\"}}",
        username
    );

    Ok(())
}

/// Pre-enable TOTP for the seeded admin and bake `123456` into every
/// recovery slot so the demo image's MFA walkthrough works repeatedly
/// (recovery codes are single-use; one shared value across all 10 slots
/// gives ~10 demo logins per container reseed).
///
/// The TOTP secret itself is generated normally — authenticator codes
/// derived from it also continue to work, so an operator can scan the
/// QR (if surfaced separately) in addition to typing the fixed code.
///
/// Issue: #137. Production seeds must never call this.
fn seed_demo_mfa(
    store: &SqliteStore,
    user_id: &str,
    username: &str,
    data_dir: &Path,
    now: chrono::DateTime<Utc>,
) -> Result<()> {
    // Encrypting the TOTP seed requires the same JWT secret the server
    // will load on its next boot. `ensure` creates `<data_dir>/jwt.secret`
    // if missing; the server's identical fallback path then reads the
    // value we just persisted instead of generating its own. (A server
    // started with `CRONIQ_JWT_SECRET` set wins over the file — that
    // override must agree with our environment at init time, otherwise
    // the demo TOTP unwrap fails at login.)
    let jwt_secret =
        jwt_secret::ensure(data_dir).map_err(|e| miette!("Failed to obtain JWT secret: {e}"))?;

    let enrolment =
        enroll_user(username).map_err(|e| miette!("Failed to generate TOTP enrolment: {e}"))?;

    let secret_enc = wrap_totp_secret(&jwt_secret, enrolment.secret_b32.as_bytes())
        .map_err(|e| miette!("Failed to wrap TOTP secret: {e}"))?;

    store
        .totp_upsert(&TotpSecret {
            user_id: user_id.to_string(),
            secret_enc,
            // Skip the `/totp/confirm` step — the demo flow always wants
            // the login to step up to MFA, with no setup ceremony.
            enabled: true,
            confirmed_at: Some(now),
            created_at: now,
        })
        .map_err(|e| miette!("Failed to persist TOTP secret: {e}"))?;

    let fixed_hash = hash_recovery_code(DEMO_MFA_RECOVERY_CODE);
    let codes: Vec<RecoveryCode> = (0..enrolment.recovery_codes.len())
        .map(|_| RecoveryCode {
            code_id: Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            code_hash: fixed_hash.clone(),
            used_at: None,
            created_at: now,
        })
        .collect();

    store
        .recovery_codes_replace_all(user_id, &codes)
        .map_err(|e| miette!("Failed to persist recovery codes: {e}"))?;

    println!();
    println!(
        "Demo MFA seeded: login at '{}/admin' triggers the MFA step.",
        username
    );
    println!(
        "  Recovery code '{}' is accepted up to {} times (single-use slots).",
        DEMO_MFA_RECOVERY_CODE,
        codes.len()
    );
    println!("  Production seeds must never set CRONIQ_DEMO_MFA=1 / --demo-mfa.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("croniq-init-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Locate the seeded admin's user_id by going through the password
    /// credential row (the only auth row keyed by username; the User
    /// table is keyed by UUID and has no direct username index in the
    /// AuthStore trait).
    fn admin_user_id(store: &SqliteStore, username: &str) -> String {
        store
            .get_credentials(username)
            .unwrap()
            .expect("admin credential row")
            .user_id
    }

    #[test]
    fn init_rejects_a_password_below_the_shared_minimum() {
        // `croniq init` used to accept any non-empty password, so the very
        // first admin password could be weaker than any later one (#428).
        let dir = tempdir();
        let err = init(
            &dir,
            "admin",
            Some("short"),
            None,
            None,
            false,
            &mut CredentialSink::new(true),
        )
        .expect_err("a 5-character password must be refused");
        assert!(
            err.to_string().contains("at least 8"),
            "the error names the minimum: {err}"
        );
        assert!(
            !dir.join("croniq.db").exists(),
            "validation runs before any DB is created"
        );
    }

    #[test]
    fn init_rejects_a_password_past_bcrypts_truncation_limit() {
        let dir = tempdir();
        let err = init(
            &dir,
            "admin",
            Some(&"x".repeat(73)),
            None,
            None,
            false,
            &mut CredentialSink::new(true),
        )
        .expect_err("73 bytes exceeds bcrypt's 72-byte limit");
        assert!(
            err.to_string().contains("72"),
            "the error names the limit: {err}"
        );
    }

    #[test]
    fn default_init_does_not_seed_mfa() {
        let dir = tempdir();
        init(
            &dir,
            "admin",
            Some("demo-admin"),
            None,
            None,
            false,
            &mut CredentialSink::new(true),
        )
        .unwrap();
        let store = SqliteStore::open(&dir.join("croniq.db")).unwrap();
        let user_id = admin_user_id(&store, "admin");

        assert!(
            store.totp_get(&user_id).unwrap().is_none(),
            "no TOTP row should exist when --demo-mfa is off"
        );
    }

    #[test]
    fn demo_mfa_seeds_enabled_totp_and_fixed_recovery_code() {
        let dir = tempdir();
        init(
            &dir,
            "admin",
            Some("demo-admin"),
            None,
            None,
            true,
            &mut CredentialSink::new(true),
        )
        .unwrap();
        let store = SqliteStore::open(&dir.join("croniq.db")).unwrap();
        let user_id = admin_user_id(&store, "admin");

        let totp = store
            .totp_get(&user_id)
            .unwrap()
            .expect("demo-MFA must persist a TOTP row");
        assert!(totp.enabled, "demo TOTP must skip the /totp/confirm step");
        assert!(
            totp.confirmed_at.is_some(),
            "confirmed_at should be stamped"
        );
        assert!(
            !totp.secret_enc.is_empty(),
            "secret_enc must be a real wrap, not blank"
        );

        let hash = hash_recovery_code(DEMO_MFA_RECOVERY_CODE);
        let matched = store
            .recovery_codes_find_unused(&user_id, &hash)
            .unwrap()
            .expect("recovery code '123456' must be unused and findable");
        assert_eq!(matched.user_id, user_id);
        // All slots share the same hash → unused-count should match
        // RECOVERY_CODE_COUNT (consumed only after first login).
        let unused = store.recovery_codes_count_unused(&user_id).unwrap();
        assert!(
            unused >= 1,
            "at least one recovery slot must be available for the demo"
        );
    }
}
