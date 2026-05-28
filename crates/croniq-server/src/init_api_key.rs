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
//! actively rotate the key (revoke the old, insert the new). The default
//! is to *only log*, so an accidental env-var change never silently
//! revokes a working credential.
//!
//! See issue #217.

use chrono::Utc;
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
}

impl<'a> ReconcileInputs<'a> {
    pub fn from_env_borrowed(api_key: &'a Option<String>) -> Self {
        Self {
            api_key: api_key.as_deref(),
            reconcile_enabled: env_truthy(std::env::var("CRONIQ_INIT_API_KEY_RECONCILE").ok()),
        }
    }
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
             CRONIQ_INIT_API_KEY_RECONCILE=1 to rotate (revokes existing active keys and \
             installs the env value), or rotate via POST /v1/api-keys. See issue #217."
        );
        return Ok(());
    }

    // Reconcile: insert the new key first, then revoke the old active
    // ones. The order matters — if revocation succeeded but creation
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

    let mut revoked = 0u32;
    for k in keys.iter().filter(|k| k.revoked_at.is_none()) {
        store.revoke_api_key(&k.key_id, now)?;
        revoked += 1;
    }
    tracing::warn!(
        revoked,
        "CRONIQ_INIT_API_KEY_RECONCILE=1 — installed env value as new key for client \
         '{DEFAULT_CLIENT_NAME}' and revoked {revoked} previous active key(s)."
    );

    Ok(())
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
            },
        )
        .unwrap();
        // Nothing changed.
        let keys = store.list_api_keys(&store.list_clients().unwrap()[0].client_id).unwrap();
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
            },
        )
        .unwrap();
        let keys = store.list_api_keys(&client_id).unwrap();
        assert_eq!(keys.len(), 1, "no key should be added without RECONCILE=1");
        assert!(keys[0].revoked_at.is_none(), "old key must not be revoked");
    }

    #[test]
    fn differing_key_with_reconcile_rotates() {
        let (store, client_id) = store_with_default_client("croniq_old");
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_new"),
                reconcile_enabled: true,
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
    fn no_default_client_is_logged_noop() {
        let dir = tempdir();
        let store = SqliteStore::open(&dir.join("croniq.db")).unwrap();
        // No client seeded.
        reconcile(
            &store,
            ReconcileInputs {
                api_key: Some("croniq_whatever"),
                reconcile_enabled: true,
            },
        )
        .unwrap();
        assert!(store.list_clients().unwrap().is_empty());
    }
}
