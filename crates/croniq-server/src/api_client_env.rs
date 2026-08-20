//! API clients declared in the environment (issues #217, #471).
//!
//! A deployment that renders its environment before the container boots —
//! Compose, a Kubernetes Deployment, a secret manager writing files — could
//! previously pin exactly one credential by value: the single `admin`-scoped
//! `CRONIQ_INIT_API_KEY`. Anything narrower had to be created after boot
//! through the API, which inverts the usual ordering and is neither
//! declarative nor idempotent. So every deployment reused the admin key for
//! the runner poll and the trigger producer alike — the opposite of the
//! scoping the API already supports.
//!
//! This module lets the environment declare *named* clients with their own
//! scopes, and reconciles the store against that declaration.
//!
//! ## Grammar
//!
//! ```text
//! CRONIQ_API_KEY                       key for the `default` client
//! CRONIQ_API_KEY_SCOPES                its scopes (default: admin)
//!
//! CRONIQ_API_CLIENT_<NAME>_KEY         key for client <name>
//! CRONIQ_API_CLIENT_<NAME>_SCOPES      its scopes (required)
//!
//! CRONIQ_API_KEY_RECONCILE=1           opt in to changing stored state
//! CRONIQ_API_KEY_ROTATION_GRACE        handover window on rotation
//! ```
//!
//! Every key-bearing variable also accepts the `<VAR>_FILE` form. `<NAME>` is
//! `[A-Z0-9_]+`, lowercased with `_` → `-`, so `CRONIQ_API_CLIENT_RUNNER_POLL_KEY`
//! declares the client `runner-poll`.
//!
//! Named clients live under `CRONIQ_API_CLIENT_` rather than extending
//! `CRONIQ_API_KEY_<NAME>` deliberately. With the key value carrying no
//! attribute suffix, `CRONIQ_API_KEY_FOO_SCOPES` would be ambiguous — the
//! scopes of client `foo`, or the key of a client named `foo-scopes`? — and
//! it would collide with the control variables above. Under
//! `CRONIQ_API_CLIENT_` every declaration ends in a known attribute, so
//! parsing is a suffix strip with no reserved-word list.
//!
//! `CRONIQ_INIT_API_KEY` and `CRONIQ_INIT_API_KEY_RECONCILE` keep working as
//! aliases for the `default` client. They are deprecated, not removed.
//!
//! ## Ownership
//!
//! A client the environment created is stored with `managed_by = "env"`, and
//! from then on the environment is its source of truth: the reconciler syncs
//! name, scopes and key on every explicit reload, and the API refuses edits
//! to the row (see `auth_endpoints::refuse_env_managed`). Without that
//! marker a dashboard scope change would be reverted at the next reconcile —
//! drift that is invisible from both sides.
//!
//! Taking over a client that already exists as `managed_by = "api"` requires
//! the operator to opt in, so upgrading a deployment that happens to have a
//! colliding client name never silently moves ownership.
//!
//! ## What needs the opt-in
//!
//! `CRONIQ_API_KEY_RECONCILE=1` gates everything that *changes* stored state:
//! rotating a key, changing scopes, adopting an existing client. Creating a
//! client that does not exist yet is additive — it cannot break a working
//! credential — so it happens without the flag. That is what makes "render
//! the env, boot the stack, get two scoped clients" work in one step.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use croniq_auth::api_key::hash_api_key;
use croniq_auth::context::Scope;
use croniq_store::models::{ApiClient, ApiKey};
use croniq_store::traits::{AuthStore, StoreError};
use serde::Serialize;
use uuid::Uuid;

use crate::env_secret::env_or_file;

/// Key for the `default` client.
pub const KEY_VAR: &str = "CRONIQ_API_KEY";
/// Scopes for the `default` client.
pub const SCOPES_VAR: &str = "CRONIQ_API_KEY_SCOPES";
/// Deprecated alias of [`KEY_VAR`].
pub const LEGACY_KEY_VAR: &str = "CRONIQ_INIT_API_KEY";
/// Opt-in to changing stored state.
pub const RECONCILE_VAR: &str = "CRONIQ_API_KEY_RECONCILE";
/// Deprecated alias of [`RECONCILE_VAR`].
pub const LEGACY_RECONCILE_VAR: &str = "CRONIQ_INIT_API_KEY_RECONCILE";
/// Handover window applied to a key a rotation replaced.
pub const ROTATION_GRACE_VAR: &str = "CRONIQ_API_KEY_ROTATION_GRACE";
/// Prefix for named-client declarations.
pub const CLIENT_PREFIX: &str = "CRONIQ_API_CLIENT_";

/// Name of the client `croniq init --api-key` seeds and [`KEY_VAR`] declares.
pub const DEFAULT_CLIENT_NAME: &str = "default";

/// `managed_by` for a client the environment owns.
pub const MANAGED_BY_ENV: &str = "env";

/// Grace window applied when the operator sets none. Long enough for a
/// Kubernetes secret-volume refresh plus a consumer rollout, short enough that
/// an operator can wait it out rather than revoking by hand.
pub const DEFAULT_ROTATION_GRACE_SECS: u64 = 15 * 60;

/// A grace beyond this is almost always a mistyped duration. Honoured — it is
/// a legitimate if unusual choice — but said out loud at boot.
const ROTATION_GRACE_WARN_SECS: u64 = 24 * 60 * 60;

// ─── Declarations ────────────────────────────────────────────────────────────

/// One API client the environment declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// Client name as stored, e.g. `default` or `runner-poll`.
    pub name: String,
    pub raw_key: String,
    pub scopes: Vec<String>,
    /// Variable the key came from, so a message can name it.
    pub key_var: String,
}

/// Partial state while folding variables into declarations.
#[derive(Default)]
struct Partial {
    key: Option<(String, String)>,
    scopes: Option<(Vec<String>, String)>,
}

/// Map an env-var infix to a client name: `RUNNER_POLL` → `runner-poll`.
///
/// Rejects anything outside `[A-Z0-9_]` so the mapping stays reversible — the
/// API-side refusal reconstructs the variable name from the stored client name
/// to tell the operator what to edit.
fn client_name_from_infix(infix: &str) -> Result<String, String> {
    if infix.is_empty() {
        return Err("client name is empty".to_string());
    }
    if !infix
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(format!(
            "client name '{infix}' must use only A-Z, 0-9 and '_' (it maps to a \
             lowercase, dash-separated client name)"
        ));
    }
    Ok(infix.to_ascii_lowercase().replace('_', "-"))
}

fn parse_scopes(raw: &str, var: &str) -> Result<Vec<String>, String> {
    let scopes: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if scopes.is_empty() {
        return Err(format!("{var} lists no scopes"));
    }
    let unknown: Vec<&String> = scopes.iter().filter(|s| !Scope::is_known(s)).collect();
    if !unknown.is_empty() {
        // A typo'd scope would create a credential that authorises nothing and
        // fails only at first use, in whichever service picked it up — far
        // from the env file that caused it.
        return Err(format!(
            "{var} lists unknown scope(s): {}. Known scopes: {}",
            unknown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            Scope::ALL.join(", ")
        ));
    }
    Ok(scopes)
}

/// Fold resolved variables into declarations.
///
/// `vars` is already `<VAR>_FILE`-resolved: the caller reads any file-backed
/// value and stores it under the base variable name, so this stays pure and
/// testable.
pub fn parse_declarations(vars: &BTreeMap<String, String>) -> Result<Vec<Declaration>, String> {
    let mut partials: BTreeMap<String, Partial> = BTreeMap::new();

    let set_key = |name: &str, value: &str, var: &str, partials: &mut BTreeMap<_, _>| {
        let entry: &mut Partial = partials.entry(name.to_string()).or_default();
        match &entry.key {
            // Two variables naming the same client must agree. Silently
            // preferring one would make which credential is live depend on
            // an ordering the operator cannot see.
            Some((existing, existing_var)) if existing != value => Err(format!(
                "{existing_var} and {var} both declare the key for API client '{name}' \
                 but with different values — remove one"
            )),
            Some(_) => Ok(()),
            None => {
                entry.key = Some((value.to_string(), var.to_string()));
                Ok(())
            }
        }
    };

    if let Some(v) = vars.get(KEY_VAR) {
        set_key(DEFAULT_CLIENT_NAME, v, KEY_VAR, &mut partials)?;
    }
    if let Some(v) = vars.get(LEGACY_KEY_VAR) {
        set_key(DEFAULT_CLIENT_NAME, v, LEGACY_KEY_VAR, &mut partials)?;
    }
    if let Some(v) = vars.get(SCOPES_VAR) {
        let scopes = parse_scopes(v, SCOPES_VAR)?;
        partials
            .entry(DEFAULT_CLIENT_NAME.to_string())
            .or_default()
            .scopes = Some((scopes, SCOPES_VAR.to_string()));
    }

    for (var, value) in vars {
        let Some(rest) = var.strip_prefix(CLIENT_PREFIX) else {
            continue;
        };
        if let Some(infix) = rest.strip_suffix("_SCOPES") {
            let name = client_name_from_infix(infix).map_err(|e| format!("{var}: {e}"))?;
            let scopes = parse_scopes(value, var)?;
            partials.entry(name).or_default().scopes = Some((scopes, var.clone()));
        } else if let Some(infix) = rest.strip_suffix("_KEY") {
            let name = client_name_from_infix(infix).map_err(|e| format!("{var}: {e}"))?;
            set_key(&name, value, var, &mut partials)?;
        } else {
            return Err(format!(
                "{var} is not a recognised API-client declaration. Use \
                 {CLIENT_PREFIX}<NAME>_KEY (or _KEY_FILE) and {CLIENT_PREFIX}<NAME>_SCOPES"
            ));
        }
    }

    let mut out = Vec::new();
    for (name, partial) in partials {
        let Some((raw_key, key_var)) = partial.key else {
            let scopes_var = partial
                .scopes
                .map(|(_, v)| v)
                .unwrap_or_else(|| "<scopes>".into());
            return Err(format!(
                "{scopes_var} declares scopes for API client '{name}' but no key — add \
                 {CLIENT_PREFIX}{infix}_KEY (or remove the scopes)",
                infix = name.to_ascii_uppercase().replace('-', "_")
            ));
        };
        if !raw_key.starts_with("croniq_") {
            return Err(format!(
                "{key_var} must start with 'croniq_' (e.g. {key_var}=croniq_$(openssl rand -hex 32))"
            ));
        }
        let scopes = match partial.scopes {
            Some((s, _)) => s,
            // Back-compat: the `default` client has always been admin-scoped
            // when seeded from the environment. A *named* client with no
            // scopes is a mistake, not a request for admin — silently
            // granting the wildcard is the exact failure #471 is about.
            None if name == DEFAULT_CLIENT_NAME => vec![Scope::ADMIN.to_string()],
            None => {
                return Err(format!(
                    "API client '{name}' is declared by {key_var} but has no scopes — add \
                     {CLIENT_PREFIX}{infix}_SCOPES (e.g. work:poll,work:ack,work:renew)",
                    infix = name.to_ascii_uppercase().replace('-', "_")
                ));
            }
        };
        out.push(Declaration {
            name,
            raw_key,
            scopes,
            key_var,
        });
    }
    Ok(out)
}

/// Read the process environment and resolve every `<VAR>_FILE` sibling.
fn resolve_env_vars() -> BTreeMap<String, String> {
    let mut bases: BTreeSet<String> = [KEY_VAR, LEGACY_KEY_VAR, SCOPES_VAR]
        .iter()
        .map(|s| s.to_string())
        .collect();

    for (k, _) in std::env::vars() {
        if !k.starts_with(CLIENT_PREFIX) {
            continue;
        }
        // `_FILE` is the indirection, not part of the declaration: fold
        // `…_KEY_FILE` back onto `…_KEY` so both spellings resolve alike.
        bases.insert(k.strip_suffix("_FILE").unwrap_or(&k).to_string());
    }

    let mut out = BTreeMap::new();
    for base in bases {
        if let Some(v) = env_or_file(&base) {
            out.insert(base, v);
        }
    }
    out
}

/// Declarations from the process environment.
pub fn declarations_from_env() -> Result<Vec<Declaration>, String> {
    parse_declarations(&resolve_env_vars())
}

fn env_truthy(v: Option<String>) -> bool {
    matches!(
        v.as_deref().map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// Whether the operator opted in to changing stored state.
pub fn reconcile_enabled_from_env() -> bool {
    env_truthy(std::env::var(RECONCILE_VAR).ok())
        || env_truthy(std::env::var(LEGACY_RECONCILE_VAR).ok())
}

/// Parse [`ROTATION_GRACE_VAR`], falling back to
/// [`DEFAULT_ROTATION_GRACE_SECS`]. A malformed value is an error rather than
/// a silent fall-back: a typo here decides how long a replaced credential
/// keeps working, which is not something to guess at.
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
             for that entire window. Check the unit ('<n>[s|m|h]', bare numbers are seconds)."
        );
    }
    Ok(Duration::seconds(secs as i64))
}

// ─── Reconciliation ──────────────────────────────────────────────────────────

/// Inputs for one reconcile pass. Extracted so tests drive it without env.
pub struct ReconcileInputs {
    pub declarations: Vec<Declaration>,
    pub reconcile_enabled: bool,
    pub rotation_grace: Duration,
    /// Compute the outcomes without writing anything. Backs
    /// `POST /v1/admin/reload-config?dry_run=true`, so an operator can see
    /// what a reload would do to their credentials before doing it.
    pub dry_run: bool,
}

impl ReconcileInputs {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            declarations: declarations_from_env()?,
            reconcile_enabled: reconcile_enabled_from_env(),
            rotation_grace: rotation_grace_from_env(
                std::env::var(ROTATION_GRACE_VAR).ok().as_deref(),
            )?,
            dry_run: false,
        })
    }

    /// Same inputs, but computing only.
    pub fn dry_run_from_env() -> Result<Self, String> {
        Ok(Self {
            dry_run: true,
            ..Self::from_env()?
        })
    }
}

/// What the reconciler did to one client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Client did not exist and was created with its declared key.
    Created,
    /// Declared key installed; the previous one is retired.
    Rotated,
    /// Scopes brought in line with the declaration.
    ScopesUpdated,
    /// An existing api-owned client is now owned by the environment.
    Adopted,
    /// Store already matches the declaration.
    Unchanged,
    /// A change is needed but the opt-in is not set, so nothing was written.
    Blocked,
}

/// Per-client result, surfaced on the reload response so a headless caller
/// sees the outcome instead of having to read logs.
#[derive(Debug, Clone, Serialize)]
pub struct ClientOutcome {
    pub client: String,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Run the reconciliation.
///
/// Logs via `tracing` and returns one outcome per declaration. Store errors
/// propagate so the caller fails fast rather than limping along with an
/// inconsistent auth table.
pub fn reconcile<S: AuthStore + ?Sized>(
    store: &S,
    inputs: &ReconcileInputs,
) -> Result<Vec<ClientOutcome>, StoreError> {
    let mut outcomes = Vec::new();
    if inputs.declarations.is_empty() {
        return Ok(outcomes);
    }
    let existing = store.list_clients()?;
    let now = Utc::now();

    for decl in &inputs.declarations {
        let outcome = match existing.iter().find(|c| c.name == decl.name) {
            None => create_declared_client(store, decl, now, inputs.dry_run)?,
            Some(client) => sync_declared_client(store, decl, client, inputs, now)?,
        };
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

fn create_declared_client<S: AuthStore + ?Sized>(
    store: &S,
    decl: &Declaration,
    now: DateTime<Utc>,
    dry_run: bool,
) -> Result<ClientOutcome, StoreError> {
    let outcome = ClientOutcome {
        client: decl.name.clone(),
        action: Action::Created,
        detail: Some(format!("scopes {}", decl.scopes.join(","))),
    };
    if dry_run {
        return Ok(outcome);
    }

    let client_id = Uuid::new_v4().to_string();
    store.create_client(&ApiClient {
        client_id: client_id.clone(),
        name: decl.name.clone(),
        scopes: decl.scopes.clone(),
        is_active: true,
        created_at: now,
        managed_by: MANAGED_BY_ENV.to_string(),
    })?;
    store.create_api_key(&new_key_row(&client_id, &decl.raw_key, now))?;
    tracing::info!(
        client = %decl.name,
        scopes = %decl.scopes.join(","),
        var = %decl.key_var,
        "created API client from environment declaration"
    );
    Ok(outcome)
}

fn sync_declared_client<S: AuthStore + ?Sized>(
    store: &S,
    decl: &Declaration,
    client: &ApiClient,
    inputs: &ReconcileInputs,
    now: DateTime<Utc>,
) -> Result<ClientOutcome, StoreError> {
    let keys = store.list_api_keys(&client.client_id)?;
    let declared_hash = hash_api_key(&decl.raw_key);

    // Decide everything before writing anything: it keeps the dry-run honest
    // (one early return, not a write guard per statement) and keeps the
    // opt-in check in one place.
    let needs_ownership = client.managed_by != MANAGED_BY_ENV;
    let needs_scopes = client.scopes != decl.scopes;
    let needs_key = !keys
        .iter()
        .any(|k| k.key_hash == declared_hash && k.revoked_at.is_none());

    let outcome = |action, detail| ClientOutcome {
        client: decl.name.clone(),
        action,
        detail,
    };

    if !needs_ownership && !needs_scopes && !needs_key {
        tracing::debug!(client = %decl.name, "API client matches its environment declaration");
        return Ok(outcome(Action::Unchanged, None));
    }

    if !inputs.reconcile_enabled {
        // Everything below rewrites a credential or moves ownership. Doing
        // that off an env value the operator may have changed by accident is
        // how a working deployment goes dark, so it stays behind the flag —
        // and we say precisely what is being withheld.
        let mut pending = Vec::new();
        if needs_ownership {
            pending.push("take ownership from the API");
        }
        if needs_scopes {
            pending.push("update scopes");
        }
        if needs_key {
            pending.push("rotate the key");
        }
        let detail = format!(
            "would {} — set {RECONCILE_VAR}=1 to apply",
            pending.join(", ")
        );
        // Ownership on its own is not a fault: the credential works, the row
        // is simply still editable through the API. Warning about it every
        // boot would train operators to ignore the line that *does* mean
        // something — a key or scope change the environment asked for and
        // did not get.
        if needs_key || needs_scopes {
            tracing::warn!(
                client = %decl.name,
                var = %decl.key_var,
                "API client differs from its environment declaration: {detail}"
            );
        } else {
            tracing::info!(
                client = %decl.name,
                var = %decl.key_var,
                "API client matches its environment declaration but is still owned by the \
                 API: {detail}"
            );
        }
        return Ok(outcome(Action::Blocked, Some(detail)));
    }

    // A rotation is the most consequential of the three, then a change of
    // owner, then scopes — report the loudest one.
    let action = if needs_key {
        Action::Rotated
    } else if needs_ownership {
        Action::Adopted
    } else {
        Action::ScopesUpdated
    };
    if inputs.dry_run {
        return Ok(outcome(action, Some("not applied (dry run)".into())));
    }

    if needs_ownership || needs_scopes {
        // `create_client` upserts, so ownership and scopes land in one write.
        store.create_client(&ApiClient {
            client_id: client.client_id.clone(),
            name: client.name.clone(),
            scopes: decl.scopes.clone(),
            is_active: client.is_active,
            created_at: client.created_at,
            managed_by: MANAGED_BY_ENV.to_string(),
        })?;
        if needs_ownership {
            tracing::warn!(
                client = %decl.name,
                var = %decl.key_var,
                "{RECONCILE_VAR}=1 — API client is now owned by the environment; \
                 edits through the API will be refused"
            );
        }
        if needs_scopes {
            tracing::warn!(
                client = %decl.name,
                from = %client.scopes.join(","),
                to = %decl.scopes.join(","),
                "{RECONCILE_VAR}=1 — API client scopes updated from the environment"
            );
        }
    }

    if needs_key {
        // New key first, then retire the old ones. If retirement succeeded
        // but creation failed, every consumer of the old key would start
        // 401-ing with no working replacement.
        store.create_api_key(&new_key_row(&client.client_id, &decl.raw_key, now))?;
        let retired = retire_superseded_keys(store, &keys, now, inputs.rotation_grace)?;
        match retired.grace_until {
            Some(until) => tracing::warn!(
                client = %decl.name,
                retired = retired.count,
                grace_until = %until,
                "{RECONCILE_VAR}=1 — installed the declared key; {} previous key(s) keep \
                 working until {until} ({ROTATION_GRACE_VAR}); revoke sooner with \
                 DELETE /v1/api-keys/{{id}}",
                retired.count
            ),
            None => tracing::warn!(
                client = %decl.name,
                retired = retired.count,
                "{RECONCILE_VAR}=1 — installed the declared key and revoked {} previous \
                 key(s) immediately ({ROTATION_GRACE_VAR}=0)",
                retired.count
            ),
        }
    }

    Ok(outcome(action, None))
}

fn new_key_row(client_id: &str, raw_key: &str, now: DateTime<Utc>) -> ApiKey {
    ApiKey {
        key_id: Uuid::new_v4().to_string(),
        client_id: client_id.to_string(),
        key_hash: hash_api_key(raw_key),
        key_prefix: raw_key.chars().take(12).collect(),
        expires_at: None,
        revoked_at: None,
        created_at: now,
    }
}

/// What [`retire_superseded_keys`] did, for logging.
struct Retirement {
    count: u32,
    /// When the keys stop working, or `None` when they were revoked outright.
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
    use croniq_store::models::PasswordCredential;
    use croniq_store::sqlite::SqliteStore;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn store() -> SqliteStore {
        SqliteStore::in_memory().unwrap()
    }

    fn inputs(decls: Vec<Declaration>, enabled: bool, grace: Duration) -> ReconcileInputs {
        ReconcileInputs {
            declarations: decls,
            reconcile_enabled: enabled,
            rotation_grace: grace,
            dry_run: false,
        }
    }

    fn decl(name: &str, key: &str, scopes: &[&str]) -> Declaration {
        Declaration {
            name: name.into(),
            raw_key: key.into(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            key_var: format!(
                "CRONIQ_API_CLIENT_{}_KEY",
                name.to_uppercase().replace('-', "_")
            ),
        }
    }

    fn seed_client(store: &SqliteStore, name: &str, scopes: &[&str], managed_by: &str) -> String {
        let id = Uuid::new_v4().to_string();
        store
            .create_client(&ApiClient {
                client_id: id.clone(),
                name: name.into(),
                scopes: scopes.iter().map(|s| s.to_string()).collect(),
                is_active: true,
                created_at: Utc::now(),
                managed_by: managed_by.into(),
            })
            .unwrap();
        id
    }

    // ─── Declaration parsing ─────────────────────────────────────────────────

    #[test]
    fn bare_key_declares_the_default_client_as_admin() {
        let d = parse_declarations(&vars(&[(KEY_VAR, "croniq_abc")])).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, DEFAULT_CLIENT_NAME);
        assert_eq!(d[0].scopes, vec!["admin"]);
    }

    #[test]
    fn legacy_init_var_is_an_alias_for_the_default_client() {
        let d = parse_declarations(&vars(&[(LEGACY_KEY_VAR, "croniq_abc")])).unwrap();
        assert_eq!(d.len(), 1, "must not declare a second client");
        assert_eq!(d[0].name, DEFAULT_CLIENT_NAME);
        assert_eq!(d[0].key_var, LEGACY_KEY_VAR);
    }

    #[test]
    fn the_two_default_spellings_must_agree() {
        // Same value: one declaration, no complaint.
        parse_declarations(&vars(&[
            (KEY_VAR, "croniq_abc"),
            (LEGACY_KEY_VAR, "croniq_abc"),
        ]))
        .unwrap();

        // Different values: which one is live would depend on invisible
        // ordering, so refuse rather than pick.
        let err = parse_declarations(&vars(&[
            (KEY_VAR, "croniq_abc"),
            (LEGACY_KEY_VAR, "croniq_xyz"),
        ]))
        .unwrap_err();
        assert!(
            err.contains(KEY_VAR) && err.contains(LEGACY_KEY_VAR),
            "{err}"
        );
    }

    #[test]
    fn named_clients_map_underscores_to_dashes() {
        let d = parse_declarations(&vars(&[
            ("CRONIQ_API_CLIENT_RUNNER_POLL_KEY", "croniq_r"),
            ("CRONIQ_API_CLIENT_RUNNER_POLL_SCOPES", "work:poll,work:ack"),
        ]))
        .unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, "runner-poll");
        assert_eq!(d[0].scopes, vec!["work:poll", "work:ack"]);
    }

    #[test]
    fn two_scoped_clients_from_env_alone() {
        // The acceptance case from #471: one runner credential, one producer.
        let d = parse_declarations(&vars(&[
            ("CRONIQ_API_CLIENT_RUNNER_KEY", "croniq_r"),
            (
                "CRONIQ_API_CLIENT_RUNNER_SCOPES",
                "work:poll,work:ack,work:renew",
            ),
            ("CRONIQ_API_CLIENT_PRODUCER_KEY", "croniq_p"),
            ("CRONIQ_API_CLIENT_PRODUCER_SCOPES", "jobs:trigger"),
        ]))
        .unwrap();
        let names: Vec<&str> = d.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["producer", "runner"]);
        assert_eq!(d[0].scopes, vec!["jobs:trigger"]);
        assert_eq!(d[1].scopes.len(), 3);
    }

    #[test]
    fn a_named_client_must_declare_scopes() {
        // Falling back to admin here would recreate exactly the problem
        // #471 exists to fix.
        let err = parse_declarations(&vars(&[("CRONIQ_API_CLIENT_PRODUCER_KEY", "croniq_p")]))
            .unwrap_err();
        assert!(err.contains("CRONIQ_API_CLIENT_PRODUCER_SCOPES"), "{err}");
    }

    #[test]
    fn scopes_without_a_key_declare_nothing_and_are_refused() {
        let err = parse_declarations(&vars(&[(
            "CRONIQ_API_CLIENT_PRODUCER_SCOPES",
            "jobs:trigger",
        )]))
        .unwrap_err();
        assert!(err.contains("CRONIQ_API_CLIENT_PRODUCER_KEY"), "{err}");
    }

    #[test]
    fn an_unknown_scope_is_refused_with_the_catalogue() {
        let err = parse_declarations(&vars(&[
            ("CRONIQ_API_CLIENT_P_KEY", "croniq_p"),
            ("CRONIQ_API_CLIENT_P_SCOPES", "jobs:trigger,job:reed"),
        ]))
        .unwrap_err();
        assert!(err.contains("job:reed"), "{err}");
        assert!(
            err.contains("jobs:read"),
            "catalogue should be listed: {err}"
        );
    }

    #[test]
    fn a_key_without_the_croniq_prefix_is_refused() {
        let err = parse_declarations(&vars(&[(KEY_VAR, "hunter2")])).unwrap_err();
        assert!(err.contains("croniq_"), "{err}");
    }

    #[test]
    fn an_unrecognised_attribute_names_the_valid_ones() {
        let err = parse_declarations(&vars(&[("CRONIQ_API_CLIENT_FOO_SECRET", "x")])).unwrap_err();
        assert!(err.contains("_KEY") && err.contains("_SCOPES"), "{err}");
    }

    #[test]
    fn a_client_named_after_an_attribute_suffix_stays_addressable() {
        // The reason named clients live under CRONIQ_API_CLIENT_: every
        // declaration ends in a known attribute, so `foo-scopes` is not
        // ambiguous with the scopes of `foo`.
        let d = parse_declarations(&vars(&[
            ("CRONIQ_API_CLIENT_FOO_SCOPES_KEY", "croniq_f"),
            ("CRONIQ_API_CLIENT_FOO_SCOPES_SCOPES", "jobs:read"),
        ]))
        .unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, "foo-scopes");
    }

    #[test]
    fn control_variables_are_not_client_declarations() {
        // These live outside the CRONIQ_API_CLIENT_ namespace precisely so
        // they cannot be mistaken for a client called `reconcile`.
        let d = parse_declarations(&vars(&[
            (KEY_VAR, "croniq_abc"),
            (RECONCILE_VAR, "1"),
            (ROTATION_GRACE_VAR, "5m"),
        ]))
        .unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, DEFAULT_CLIENT_NAME);
    }

    // ─── Reconciliation ──────────────────────────────────────────────────────

    #[test]
    fn a_declared_client_is_created_without_the_opt_in() {
        // Creating is additive — it cannot break a working credential — and
        // gating it would make "render env, boot stack" a two-step dance.
        let s = store();
        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_p", &["jobs:trigger"])],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Created);
        let clients = s.list_clients().unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].name, "producer");
        assert_eq!(clients[0].scopes, vec!["jobs:trigger"]);
        assert_eq!(clients[0].managed_by, MANAGED_BY_ENV);
        let keys = s.list_api_keys(&clients[0].client_id).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_hash, hash_api_key("croniq_p"));
    }

    #[test]
    fn two_declarations_produce_two_scoped_clients() {
        let s = store();
        reconcile(
            &s,
            &inputs(
                vec![
                    decl("runner", "croniq_r", &["work:poll", "work:ack"]),
                    decl("producer", "croniq_p", &["jobs:trigger"]),
                ],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        let clients = s.list_clients().unwrap();
        assert_eq!(clients.len(), 2);
        let producer = clients.iter().find(|c| c.name == "producer").unwrap();
        let runner = clients.iter().find(|c| c.name == "runner").unwrap();
        assert_eq!(producer.scopes, vec!["jobs:trigger"]);
        assert_eq!(runner.scopes, vec!["work:poll", "work:ack"]);
    }

    #[test]
    fn an_unchanged_declaration_is_a_noop() {
        let s = store();
        let d = vec![decl("producer", "croniq_p", &["jobs:trigger"])];
        reconcile(&s, &inputs(d.clone(), false, Duration::minutes(15))).unwrap();
        let out = reconcile(&s, &inputs(d, true, Duration::minutes(15))).unwrap();

        assert_eq!(out[0].action, Action::Unchanged);
        let clients = s.list_clients().unwrap();
        assert_eq!(clients.len(), 1, "no duplicate client");
        assert_eq!(s.list_api_keys(&clients[0].client_id).unwrap().len(), 1);
    }

    #[test]
    fn changing_an_existing_client_needs_the_opt_in() {
        let s = store();
        let d = vec![decl("producer", "croniq_p", &["jobs:trigger"])];
        reconcile(&s, &inputs(d, false, Duration::minutes(15))).unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_rotated", &["jobs:trigger"])],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Blocked);
        assert!(out[0].detail.as_ref().unwrap().contains("rotate the key"));
        let clients = s.list_clients().unwrap();
        let keys = s.list_api_keys(&clients[0].client_id).unwrap();
        assert_eq!(keys.len(), 1, "nothing may be written without the opt-in");
        assert_eq!(keys[0].key_hash, hash_api_key("croniq_p"));
    }

    #[test]
    fn scopes_are_synced_under_the_opt_in() {
        let s = store();
        reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_p", &["jobs:trigger"])],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_p", &["jobs:trigger", "jobs:read"])],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::ScopesUpdated);
        let clients = s.list_clients().unwrap();
        assert_eq!(clients[0].scopes, vec!["jobs:trigger", "jobs:read"]);
        assert_eq!(
            s.list_api_keys(&clients[0].client_id).unwrap().len(),
            1,
            "a scope change must not rotate the key"
        );
    }

    #[test]
    fn rotation_retires_the_old_key_with_the_grace_window() {
        let s = store();
        reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_p", &["jobs:trigger"])],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_new", &["jobs:trigger"])],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Rotated);
        let clients = s.list_clients().unwrap();
        let keys = s.list_api_keys(&clients[0].client_id).unwrap();
        let old = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_p"))
            .unwrap();
        assert!(old.revoked_at.is_none(), "grace rotation must not revoke");
        assert!(old.expires_at.is_some());
        let new = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_new"))
            .unwrap();
        assert!(new.expires_at.is_none());
    }

    #[test]
    fn zero_grace_revokes_the_superseded_key_immediately() {
        let s = store();
        reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_p", &["jobs:trigger"])],
                false,
                Duration::zero(),
            ),
        )
        .unwrap();
        reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_new", &["jobs:trigger"])],
                true,
                Duration::zero(),
            ),
        )
        .unwrap();

        let clients = s.list_clients().unwrap();
        let keys = s.list_api_keys(&clients[0].client_id).unwrap();
        let old = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_p"))
            .unwrap();
        assert!(old.revoked_at.is_some());
    }

    #[test]
    fn an_api_owned_client_is_never_adopted_silently() {
        // The upgrade case: a deployment already has a `default` client from
        // `croniq init` and now also sets CRONIQ_API_KEY. Ownership must not
        // move — the dashboard would start refusing edits with no warning.
        let s = store();
        let id = seed_client(&s, "default", &["admin"], "api");
        s.create_api_key(&new_key_row(&id, "croniq_seeded", Utc::now()))
            .unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("default", "croniq_seeded", &["admin"])],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Blocked);
        assert!(
            out[0]
                .detail
                .as_ref()
                .unwrap()
                .contains("take ownership from the API")
        );
        assert_eq!(s.list_clients().unwrap()[0].managed_by, "api");
    }

    #[test]
    fn the_opt_in_adopts_an_api_owned_client_without_touching_a_matching_key() {
        let s = store();
        let id = seed_client(&s, "default", &["admin"], "api");
        s.create_api_key(&new_key_row(&id, "croniq_seeded", Utc::now()))
            .unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("default", "croniq_seeded", &["admin"])],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Adopted);
        assert_eq!(s.list_clients().unwrap()[0].managed_by, MANAGED_BY_ENV);
        let keys = s.list_api_keys(&id).unwrap();
        assert_eq!(keys.len(), 1, "adoption alone must not rotate");
        assert!(keys[0].expires_at.is_none());
    }

    #[test]
    fn no_declarations_is_a_silent_noop() {
        let s = store();
        // A store with unrelated state must be left completely alone.
        seed_client(&s, "manual", &["jobs:read"], "api");
        s.upsert_credentials(&PasswordCredential {
            user_id: "u1".into(),
            username: "admin".into(),
            password_hash: "x".into(),
            failed_attempts: 0,
            locked_until: None,
            created_at: Utc::now(),
        })
        .unwrap();

        let out = reconcile(&s, &inputs(vec![], true, Duration::minutes(15))).unwrap();
        assert!(out.is_empty());
        assert_eq!(s.list_clients().unwrap().len(), 1);
    }

    #[test]
    fn a_dry_run_reports_the_actions_without_writing() {
        let s = store();
        // Pre-existing client that a real run would rotate and adopt.
        let id = seed_client(&s, "producer", &["jobs:read"], "api");
        s.create_api_key(&new_key_row(&id, "croniq_old", Utc::now()))
            .unwrap();

        let out = reconcile(
            &s,
            &ReconcileInputs {
                declarations: vec![
                    decl("producer", "croniq_new", &["jobs:trigger"]),
                    decl("fresh", "croniq_f", &["jobs:read"]),
                ],
                reconcile_enabled: true,
                rotation_grace: Duration::minutes(15),
                dry_run: true,
            },
        )
        .unwrap();

        assert_eq!(out.len(), 2);
        let fresh = out.iter().find(|o| o.client == "fresh").unwrap();
        let producer = out.iter().find(|o| o.client == "producer").unwrap();
        assert_eq!(fresh.action, Action::Created);
        assert_eq!(producer.action, Action::Rotated);

        // Nothing was written.
        let clients = s.list_clients().unwrap();
        assert_eq!(clients.len(), 1, "the dry run must not create a client");
        assert_eq!(clients[0].scopes, vec!["jobs:read"]);
        assert_eq!(clients[0].managed_by, "api");
        let keys = s.list_api_keys(&id).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_hash, hash_api_key("croniq_old"));
    }

    // ─── Grace parsing ───────────────────────────────────────────────────────

    #[test]
    fn grace_defaults_to_fifteen_minutes_and_rejects_garbage() {
        assert_eq!(
            rotation_grace_from_env(None).unwrap(),
            Duration::seconds(DEFAULT_ROTATION_GRACE_SECS as i64)
        );
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
        let err = rotation_grace_from_env(Some("15min")).unwrap_err();
        assert!(err.contains(ROTATION_GRACE_VAR), "{err}");
    }
}
