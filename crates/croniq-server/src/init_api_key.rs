//! Boot-time reconciliation for `CRONIQ_INIT_API_KEY`.
//!
//! On a fresh data dir, `croniq init` consumes `CRONIQ_INIT_API_KEY` and
//! seeds the default API client. On every subsequent start the env var
//! used to be silently ignored — operators rotating the value via their
//! orchestrator (set the same parameter on server and runner, restart)
//! got `401 Unauthorized` with no log line explaining why.
//!
//! This module fixes that: at server startup, if `CRONIQ_INIT_API_KEY` is
//! set, we log whether it matches the stored key for the `default`
//! client, and — when `CRONIQ_INIT_API_KEY_RECONCILE=1` is also set —
//! actively rotate the key (insert the new, retire the old). The default
//! is to *only log*, so an accidental env-var change never silently
//! revokes a working credential.
//!
//! ## Rotation grace window
//!
//! Retiring the old key immediately makes the rotation instant *for the
//! server* and a hard cut for everything else: a runner in another
//! container still holds the previous value in memory, and the runner SDK
//! classifies `401` as transient (`ClientError::Server`), so it retries
//! every few seconds forever instead of exiting for its orchestrator to
//! restart. Nothing recovers on its own.
//!
//! So rotation gives the old key an expiry instead of a revocation:
//! `expires_at = now + CRONIQ_API_KEY_ROTATION_GRACE` (default 15m,
//! enforced by the auth middleware like any other expiry). That covers the
//! window in which a Kubernetes secret volume refreshes and a consumer
//! rollout completes. Set the grace to `0s` for the previous behaviour —
//! an immediate revoke — which is what a compliance rule demanding
//! instant revocation wants.
//!
//! The grace window is *not* the answer to a leaked key: it deliberately
//! keeps the old value alive. To kill a credential now, revoke it
//! explicitly (`DELETE /v1/api-keys/{id}`, with `GET /v1/api-keys` to find
//! the id) after rotating, or rotate with the grace set to `0s`.
//!
//! See issues #217 and #471.

use chrono::{DateTime, Duration, Utc};
use croniq_auth::api_key::hash_api_key;
use croniq_store::models::ApiKey;
use croniq_store::traits::{AuthStore, StoreError};
use uuid::Uuid;

/// Name of the API client `croniq init --api-key` seeds. The reconcile
/// flow operates on this client only — operators who want to rotate
/// arbitrary clients should use the API.
const DEFAULT_CLIENT_NAME: &str = "default";

/// Inputs collected from process env. Extracted so tests can drive the
/// reconciler without setting real env vars.
pub struct ReconcileInputs<'a> {
    pub api_key: Option<&'a str>,
    pub reconcile_enabled: bool,
    /// How long a replaced key stays usable after a rotation. `Duration::zero()`
    /// revokes it immediately (pre-#471 behaviour).
    pub rotation_grace: Duration,
}

impl<'a> ReconcileInputs<'a> {
    pub fn from_env_borrowed(api_key: &'a Option<String>) -> Result<Self, String> {
        Ok(Self {
            api_key: api_key.as_deref(),
            reconcile_enabled: env_truthy(std::env::var("CRONIQ_INIT_API_KEY_RECONCILE").ok()),
            rotation_grace: rotation_grace_from_env(
                std::env::var(ROTATION_GRACE_VAR).ok().as_deref(),
            )?,
        })
    }
}

/// Env var naming the rotation grace window.
pub const ROTATION_GRACE_VAR: &str = "CRONIQ_API_KEY_ROTATION_GRACE";

/// Grace window applied when the operator sets none. Long enough for a
/// Kubernetes secret-volume refresh plus a consumer rollout, short enough
/// that an operator can wait it out when they would rather not revoke by
/// hand.
pub const DEFAULT_ROTATION_GRACE_SECS: u64 = 15 * 60;

/// A grace beyond this is almost always a mistyped duration (`30` meaning
/// seconds, `30h` typed as `30d`-worth by accident). We still honour it —
/// it is a legitimate, if unusual, choice — but say so at boot.
const ROTATION_GRACE_WARN_SECS: u64 = 24 * 60 * 60;

/// Parse [`ROTATION_GRACE_VAR`], falling back to
/// [`DEFAULT_ROTATION_GRACE_SECS`]. A malformed value is an error rather
/// than a silent fall-back: a typo here decides how long a replaced
/// credential keeps working, which is not something to guess at.
pub fn rotation_grace_from_env(raw: Option<&str>) -> Result<Duration, String> {
    let secs = match raw.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => crate::duration::parse_duration_secs(v)
            .map_err(|e| format!("{ROTATION_GRACE_VAR}: {e}"))?,
        None => DEFAULT_ROTATION_GRACE_SECS,
    };
    if secs > ROTATION_GRACE_WARN_SECS {
        tracing::warn!(
            grace_secs = secs,
            "{ROTATION_GRACE_VAR} is longer than 24h — a rotated-out API key stays usable \
             for that entire window. Check the unit ('<n>[s|m|h]', bare numbers are \
             seconds)."
        );
    }
    Ok(Duration::seconds(secs as i64))
}

fn env_truthy(v: Option<String>) -> bool {
    matches!(
        v.as_deref().map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// Run the reconciliation. Logs via `tracing` and returns `Ok(())` even
/// when the env var is unset (a silent no-op is correct in that case).
/// Store errors are propagated so the server fails fast rather than
/// limping along with an inconsistent auth table.
pub fn reconcile<S: AuthStore + ?Sized>(
    store: &S,
    inputs: ReconcileInputs<'_>,
) -> Result<(), StoreError> {
    let Some(raw_key) = inputs.api_key else {
        return Ok(());
    };

    let clients = store.list_clients()?;
    let Some(default_client) = clients.iter().find(|c| c.name == DEFAULT_CLIENT_NAME) else {
        tracing::info!(
            "CRONIQ_INIT_API_KEY is set but no '{DEFAULT_CLIENT_NAME}' API client exists in \
             this data dir. The variable only seeds on fresh `croniq init`; on existing data \
             dirs, create or rotate keys via the API (POST /v1/api-keys) or wipe the data dir \
             to re-seed."
        );
        return Ok(());
    };

    let provided_hash = hash_api_key(raw_key);
    let keys = store.list_api_keys(&default_client.client_id)?;
    let already_present = keys
        .iter()
        .any(|k| k.key_hash == provided_hash && k.revoked_at.is_none());

    if already_present {
        tracing::info!(
            "CRONIQ_INIT_API_KEY matches an active key for client '{DEFAULT_CLIENT_NAME}' — \
             no changes."
        );
        return Ok(());
    }

    if !inputs.reconcile_enabled {
        tracing::warn!(
            "CRONIQ_INIT_API_KEY differs from every active key for client \
             '{DEFAULT_CLIENT_NAME}' — env value ignored. Set \
             CRONIQ_INIT_API_KEY_RECONCILE=1 to rotate (retires existing active keys and \
             installs the env value), or rotate via POST /v1/api-keys. See issue #217."
        );
        return Ok(());
    }

    // Reconcile: insert the new key first, then retire the old active
    // ones. The order matters — if retirement succeeded but creation
    // failed, every runner pointed at the old key would immediately
    // start 401-ing with no working replacement.
    let now = Utc::now();
    let new_key = ApiKey {
        key_id: Uuid::new_v4().to_string(),
        client_id: default_client.client_id.clone(),
        key_hash: provided_hash,
        key_prefix: raw_key.chars().take(12).collect(),
        expires_at: None,
        revoked_at: None,
        created_at: now,
    };
    store.create_api_key(&new_key)?;

    let outcome = retire_superseded_keys(store, &keys, now, inputs.rotation_grace)?;
    match outcome.grace_until {
        Some(until) => tracing::warn!(
            retired = outcome.count,
            grace_until = %until,
            "CRONIQ_INIT_API_KEY_RECONCILE=1 — installed env value as new key for client \
             '{DEFAULT_CLIENT_NAME}'. {} previous active key(s) keep working until {until} \
             (grace from {ROTATION_GRACE_VAR}); revoke sooner with DELETE /v1/api-keys/{{id}}.",
            outcome.count
        ),
        None => tracing::warn!(
            retired = outcome.count,
            "CRONIQ_INIT_API_KEY_RECONCILE=1 — installed env value as new key for client \
             '{DEFAULT_CLIENT_NAME}' and revoked {} previous active key(s) immediately \
             ({ROTATION_GRACE_VAR}=0).",
            outcome.count
        ),
    }

    Ok(())
}

/// What [`retire_superseded_keys`] did, for logging.
struct Retirement {
    /// How many previously-active keys were touched.
    count: u32,
    /// When they stop working, or `None` when they were revoked outright.
    grace_until: Option<DateTime<Utc>>,
}

/// Retire every currently-active key in `keys`: revoke it outright when
/// `grace` is zero, otherwise stamp `expires_at = now + grace` and leave it
/// usable until then.
///
/// Keys that already expire at or before the deadline are skipped. This is a
/// retirement, not a renewal, and `set_api_key_expiry` is a plain setter that
/// would happily push an earlier deadline further out — rotating twice inside
/// one grace window must not keep resurrecting the oldest key.
fn retire_superseded_keys<S: AuthStore + ?Sized>(
    store: &S,
    keys: &[ApiKey],
    now: DateTime<Utc>,
    grace: Duration,
) -> Result<Retirement, StoreError> {
    let active = keys.iter().filter(|k| k.revoked_at.is_none());

    if grace <= Duration::zero() {
        let mut count = 0u32;
        for k in active {
            store.revoke_api_key(&k.key_id, now)?;
            count += 1;
        }
        return Ok(Retirement {
            count,
            grace_until: None,
        });
    }

    let deadline = now + grace;
    let mut count = 0u32;
    for k in active {
        if k.expires_at.is_some_and(|e| e <= deadline) {
            continue;
        }
        store.set_api_key_expiry(&k.key_id, deadline)?;
        count += 1;
    }
    Ok(Retirement {
        count,
        grace_until: Some(deadline),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use croniq_store::models::{ApiClient, ApiKey};
    use croniq_store::sqlite::SqliteStore;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("croniq-reconcile-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn store_with_default_client(raw_key: &str) -> (SqliteStore, String) {
        let dir = tempdir();
        let store = SqliteStore::open(&dir.join("croniq.db")).unwrap();
        let client_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        store
            .create_client(&ApiClient {
                client_id: client_id.clone(),
                name: DEFAULT_CLIENT_NAME.to_string(),
                scopes: vec!["admin".to_string()],
                is_active: true,
                created_at: now,
            })
            .unwrap();
        store
            .create_api_key(&ApiKey {
                key_id: Uuid::new_v4().to_string(),
                client_id: client_id.clone(),
                key_hash: hash_api_key(raw_key),
                key_prefix: raw_key.chars().take(12).collect(),
                expires_at: None,
                revoked_at: None,
                created_at: now,
            })
            .unwrap();
        (store, client_id)
    }

    #[test]
    fn unset_env_is_noop() {
        let (store, _) = store_with_default_client("croniq_old");
        reconcile(
            &store,
            ReconcileInputs {
                api_key: None,
                reconcile_enabled: false,
                rotation_grace: Duration::zero(),
            },
        )
        .unwrap();
        // Nothing changed.
        let keys = store
            .list_api_keys(&store.list_clients().unwrap()[0].client_id)
            .unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].revoked_at.is_none());
    }

    #[test]
    fn matching_key_is_noop() {
        let (store, client_id) = store_with_default_client("croniq_old");
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_old"),
                reconcile_enabled: true, // even with reconcile on, matching = no-op
                rotation_grace: Duration::zero(),
            },
        )
        .unwrap();
        let keys = store.list_api_keys(&client_id).unwrap();
        assert_eq!(keys.len(), 1, "no new key should be created on match");
    }

    #[test]
    fn differing_key_without_reconcile_does_not_change_store() {
        let (store, client_id) = store_with_default_client("croniq_old");
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_new"),
                reconcile_enabled: false,
                rotation_grace: Duration::zero(),
            },
        )
        .unwrap();
        let keys = store.list_api_keys(&client_id).unwrap();
        assert_eq!(keys.len(), 1, "no key should be added without RECONCILE=1");
        assert!(keys[0].revoked_at.is_none(), "old key must not be revoked");
    }

    #[test]
    fn differing_key_with_zero_grace_revokes_immediately() {
        let (store, client_id) = store_with_default_client("croniq_old");
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_new"),
                reconcile_enabled: true,
                rotation_grace: Duration::zero(),
            },
        )
        .unwrap();
        let keys = store.list_api_keys(&client_id).unwrap();
        assert_eq!(keys.len(), 2, "new key should be appended");
        let active: Vec<_> = keys.iter().filter(|k| k.revoked_at.is_none()).collect();
        assert_eq!(active.len(), 1, "exactly one active key after rotation");
        assert_eq!(active[0].key_hash, hash_api_key("croniq_new"));
    }

    #[test]
    fn differing_key_with_grace_expires_old_key_instead_of_revoking_it() {
        let (store, client_id) = store_with_default_client("croniq_old");
        let before = Utc::now();
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_new"),
                reconcile_enabled: true,
                rotation_grace: Duration::minutes(15),
            },
        )
        .unwrap();

        let keys = store.list_api_keys(&client_id).unwrap();
        assert_eq!(keys.len(), 2);
        let old = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_old"))
            .expect("old key row still present");
        let new = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_new"))
            .expect("new key installed");

        // The old key is retired, not revoked: a consumer still holding it
        // keeps authenticating until the deadline passes.
        assert!(
            old.revoked_at.is_none(),
            "grace rotation must not revoke the old key"
        );
        let expiry = old.expires_at.expect("old key must carry an expiry");
        assert!(expiry >= before + Duration::minutes(15));
        assert!(expiry <= Utc::now() + Duration::minutes(15));

        // The incoming key is unconditional.
        assert!(new.expires_at.is_none());
        assert!(new.revoked_at.is_none());
    }

    #[test]
    fn second_rotation_inside_the_window_does_not_extend_the_oldest_key() {
        // Two rotations in quick succession: the first key's deadline was set
        // by the earlier (earlier-expiring) rotation and must stand, or a
        // credential could be kept alive indefinitely by rotating repeatedly.
        let (store, client_id) = store_with_default_client("croniq_old");
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_mid"),
                reconcile_enabled: true,
                rotation_grace: Duration::minutes(5),
            },
        )
        .unwrap();
        let first_deadline = store
            .list_api_keys(&client_id)
            .unwrap()
            .into_iter()
            .find(|k| k.key_hash == hash_api_key("croniq_old"))
            .and_then(|k| k.expires_at)
            .expect("first rotation set a deadline");

        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_new"),
                reconcile_enabled: true,
                rotation_grace: Duration::hours(2),
            },
        )
        .unwrap();

        let keys = store.list_api_keys(&client_id).unwrap();
        let oldest = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_old"))
            .unwrap();
        assert_eq!(
            oldest.expires_at,
            Some(first_deadline),
            "the oldest key's earlier deadline must not be pushed out"
        );
        let mid = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_mid"))
            .unwrap();
        assert!(
            mid.expires_at.unwrap() > first_deadline,
            "the key retired by the second rotation gets the longer window"
        );
    }

    #[test]
    fn grace_defaults_to_fifteen_minutes_and_rejects_garbage() {
        assert_eq!(
            rotation_grace_from_env(None).unwrap(),
            Duration::seconds(DEFAULT_ROTATION_GRACE_SECS as i64)
        );
        // An empty value is "unset" as far as Compose is concerned.
        assert_eq!(
            rotation_grace_from_env(Some("  ")).unwrap(),
            Duration::seconds(DEFAULT_ROTATION_GRACE_SECS as i64)
        );
        assert_eq!(
            rotation_grace_from_env(Some("30m")).unwrap(),
            Duration::minutes(30)
        );
        assert_eq!(
            rotation_grace_from_env(Some("0s")).unwrap(),
            Duration::zero()
        );
        // Malformed input must fail the boot rather than silently pick a
        // window the operator did not choose.
        let err = rotation_grace_from_env(Some("15min")).unwrap_err();
        assert!(err.contains(ROTATION_GRACE_VAR), "{err}");
    }

    #[test]
    fn no_default_client_is_logged_noop() {
        let dir = tempdir();
        let store = SqliteStore::open(&dir.join("croniq.db")).unwrap();
        // No client seeded.
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_whatever"),
                reconcile_enabled: true,
                rotation_grace: Duration::zero(),
            },
        )
        .unwrap();
        assert!(store.list_clients().unwrap().is_empty());
    }
}
