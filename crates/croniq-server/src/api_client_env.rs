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
//! CRONIQ_API_KEY_SCOPES                its scopes (required to declare)
//!
//! CRONIQ_API_CLIENT_<NAME>_KEY         key for client <name>
//! CRONIQ_API_CLIENT_<NAME>_SCOPES      its scopes (required)
//!
//! CRONIQ_API_KEY_RECONCILE=1           opt in to changing stored state
//! CRONIQ_API_KEY_ROTATION_GRACE        handover window on rotation
//! ```
//!
//! `CRONIQ_API_KEY` declares a client only when `CRONIQ_API_KEY_SCOPES` says
//! what that client is for. The same variable is what the CLI and the SDKs
//! read to *present* a credential, so on a server host it is at least as
//! likely to be a client key as a declaration — and reading a narrow key as an
//! admin declaration silently widened it (issue #502). The deprecated
//! `CRONIQ_INIT_API_KEY` has no such second meaning and keeps its implied
//! admin.
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

use std::cmp::Reverse;
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
    /// Scopes the environment actually asked for, or `None` when it named
    /// none.
    ///
    /// `None` is only reachable for the `default` client, which may be
    /// declared by its key alone. It means "the environment has no opinion",
    /// which is deliberately *not* the same as asking for admin: a new client
    /// still gets admin (that is the back-compat the bare variable has always
    /// had), but an existing one keeps whatever scopes it has. Storing the
    /// implied admin here instead made every reconcile look like a request to
    /// re-escalate a client an operator had narrowed by hand (issue #501).
    pub scopes: Option<Vec<String>>,
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
            // Not fatal: refusing to boot over a variable we do not read
            // would claim the whole `CRONIQ_API_CLIENT_*` namespace for
            // ourselves, and a deployment that happens to use it for its own
            // tooling would stop starting on upgrade (issue #503). The typo
            // that actually matters — a misspelled `_SCOPES` — still fails
            // loudly further down, as "declares a key but no scopes".
            tracing::warn!(
                var = %var,
                "ignoring {var}: not a recognised API-client declaration. Use \
                 {CLIENT_PREFIX}<NAME>_KEY (or _KEY_FILE) and {CLIENT_PREFIX}<NAME>_SCOPES"
            );
            continue;
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
            // The deprecated spelling warns and is dropped, because that is
            // what v0.33.0 did ("env value ignored") and a leftover
            // placeholder from an old template must not turn a version bump
            // into a restart loop (issue #503). The current spellings are new
            // in this release, so a malformed value there is a declaration the
            // operator just wrote wrong, and saying so at boot is the whole
            // point.
            if key_var == LEGACY_KEY_VAR {
                tracing::warn!(
                    var = %key_var,
                    "ignoring {key_var}: value does not start with 'croniq_', so it cannot be \
                     an API key. Set {KEY_VAR}=croniq_… to declare the '{DEFAULT_CLIENT_NAME}' \
                     client."
                );
                continue;
            }
            return Err(format!(
                "{key_var} must start with 'croniq_' (e.g. {key_var}=croniq_$(openssl rand -hex 32))"
            ));
        }
        let scopes = match partial.scopes {
            Some((s, _)) => Some(s),
            // The `default` client keeps working with no scopes variable at
            // all, for back-compat — but "the operator said nothing" is not
            // the same fact as "the operator asked for admin", and conflating
            // the two let an upgrade silently re-escalate a narrowed client
            // (issue #501). `None` carries the distinction; see
            // [`Declaration::scopes`].
            //
            // A *named* client with no scopes is a mistake, not a request for
            // admin — silently granting the wildcard is the exact failure
            // #471 is about.
            None if name == DEFAULT_CLIENT_NAME => None,
            None => {
                return Err(format!(
                    "API client '{name}' is declared by {key_var} but has no scopes — add \
                     {CLIENT_PREFIX}{infix}_SCOPES (e.g. work:poll,work:ack,work:renew)",
                    infix = name.to_ascii_uppercase().replace('-', "_")
                ));
            }
        };
        // `CRONIQ_API_KEY` means two different things to two different
        // processes: to the CLI and the SDKs it is the credential to *present*
        // (see croniq-cli's `--api-key` env source), to the server it declares
        // a client. An operator who exports it on the server host so they can
        // run `croniq` there — which the CLI's own help tells them to do — was
        // silently declaring an admin-scoped `default` client, and since keys
        // resolve by hash, their deliberately narrow credential then
        // authenticated as admin (issue #502).
        //
        // So the current spelling declares a client only when it says what the
        // client is for. The deprecated spelling still declares — it has
        // never been a client-side variable, and existing deployments rotate
        // with it — it simply names no scopes, which since #499 means it
        // rotates an existing client without being able to create one.
        if scopes.is_none() && key_var == KEY_VAR && !vars.contains_key(LEGACY_KEY_VAR) {
            tracing::info!(
                var = %KEY_VAR,
                "{KEY_VAR} is set but {SCOPES_VAR} is not — treating it as a client credential \
                 rather than a declaration. To declare the '{DEFAULT_CLIENT_NAME}' API client \
                 from the environment, set {SCOPES_VAR} (e.g. {SCOPES_VAR}=admin)."
            );
            continue;
        }
        out.push(Declaration {
            name,
            raw_key,
            scopes,
            key_var,
        });
    }
    reject_shared_key_values(&out)?;
    Ok(out)
}

/// Refuse two clients declared with the same key value (issue #520).
///
/// The mirror of the conflict [`parse_declarations`]'s `set_key` catches: there
/// two variables name one client with different values, here two clients are
/// named with one value. Both are a declaration that cannot be satisfied, and
/// this half is the worse of the two because nothing downstream can repair it.
///
/// Keys resolve by hash, so one value can only ever authenticate as one
/// client. Reconciling both declarations writes two `api_keys` rows with the
/// same `key_hash` in the same pass, and #516's ordering has no tie to break
/// between them — un-revoked, open-ended, same `created_at` — so which client
/// the credential answers as comes back to the query plan. The loser is a
/// client that exists, is active and has the scopes it was declared with,
/// whose key 403s on its own endpoints with nothing in the reconcile output
/// hinting why.
///
/// Caught here rather than at reconcile time because at this point nothing has
/// been written: there is no live credential whose identity has to be revoked
/// from one side or the other, and the message can name both variables the
/// operator has to look at. A key pasted in from a client that was created
/// through the API is not covered — only one side of that collision is in the
/// environment.
fn reject_shared_key_values(declarations: &[Declaration]) -> Result<(), String> {
    let mut seen: BTreeMap<&str, &Declaration> = BTreeMap::new();
    for decl in declarations {
        if let Some(first) = seen.insert(decl.raw_key.as_str(), decl) {
            // `declarations` is built from a `BTreeMap`, so `first` is the
            // name-ordered earlier client and the message is stable.
            return Err(format!(
                "{} and {} declare the same key value for two different API clients \
                 ('{}' and '{}') — a key authenticates as exactly one client, so the other \
                 would get 403s on the scopes it was declared with. Give each client its own \
                 key.",
                first.key_var, decl.key_var, first.name, decl.name
            ));
        }
    }
    Ok(())
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

/// The variable that declares `client_name`, for a message telling an operator
/// what to edit.
///
/// Not simply the inverse of [`client_name_from_infix`]: the `default` client
/// is declared by [`KEY_VAR`], *outside* the named-client prefix. Naming it
/// `CRONIQ_API_CLIENT_DEFAULT_KEY` would send the operator to add a second
/// declaration of the same client, which the boot path rejects outright — so
/// following the advice would take the server down at its next start.
///
/// Consults the live environment first so the answer names the variable the
/// operator actually wrote, including the deprecated `CRONIQ_INIT_API_KEY`
/// alias. Falls back to the canonical spelling for a row the environment no
/// longer declares.
pub fn declaring_key_var(client_name: &str) -> String {
    if let Ok(declarations) = declarations_from_env()
        && let Some(declaration) = declarations.iter().find(|d| d.name == client_name)
    {
        return declaration.key_var.clone();
    }
    canonical_key_var(client_name)
}

/// The variable that *would* declare `client_name`, ignoring the environment.
fn canonical_key_var(client_name: &str) -> String {
    if client_name == DEFAULT_CLIENT_NAME {
        return KEY_VAR.to_string();
    }
    format!(
        "{CLIENT_PREFIX}{}_KEY",
        client_name.to_ascii_uppercase().replace('-', "_")
    )
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
             for that entire window. Check the unit ('<n>[s|m|h|d]', bare numbers are seconds)."
        );
    }
    // `Duration::seconds` panics beyond its representable range, and a bare
    // `secs as i64` would wrap a huge value negative first — turning a mistyped
    // grace into either a boot panic or the exact opposite of what was asked
    // for: `now + <negative>` is in the past, so every superseded key would be
    // revoked on the spot. Report it instead.
    i64::try_from(secs)
        .ok()
        .and_then(Duration::try_seconds)
        .ok_or_else(|| {
            format!(
                "{ROTATION_GRACE_VAR}: {secs} seconds is beyond the longest representable \
                 grace window"
            )
        })
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
    /// The declared key was dated or revoked; it was restored so it keeps
    /// working, and the keys it supersedes were retired. What a rolled-back
    /// rotation needs, whether the grace window dated the old key (issue
    /// #500) or it was revoked outright (issue #516).
    KeyRevived,
    /// Scopes brought in line with the declaration.
    ScopesUpdated,
    /// An existing api-owned client is now owned by the environment.
    Adopted,
    /// Store already matches the declaration.
    Unchanged,
    /// A change is needed but the opt-in is not set, so nothing was written.
    Blocked,
    /// The declaration asked for a client that does not exist but named no
    /// scopes, so nothing was created; see [`skip_implicit_creation`].
    Skipped,
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
            // A declaration that never said what the client is for rotates
            // the key of one that exists, but does not bring one back that
            // does not (issue #499). Passing the scopes rather than the whole
            // declaration keeps that invariant structural: there is no
            // implied-admin fall-back left for `create` to reach for.
            None => match decl.scopes.as_deref() {
                None => skip_implicit_creation(decl),
                Some(scopes) => create_declared_client(store, decl, scopes, now, inputs.dry_run)?,
            },
            Some(client) => sync_declared_client(store, decl, client, inputs, now)?,
        };
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Report a declaration that names a client the store does not have, and did
/// not say what that client is for.
///
/// Creating one from a key alone means a stale value left in a deployment
/// reinstalls a credential the operator had removed: delete the `default`
/// client after a key leak, leave `CRONIQ_INIT_API_KEY=<leaked>` behind, and
/// the next boot recreates it — active, admin-scoped, keyed with the leaked
/// value. Worse, the recreated row is `managed_by=env`, so
/// `DELETE /v1/api-clients/{id}` then answers 409 and the operator cannot undo
/// it through the API at all. On v0.33.0 the same environment was a no-op: the
/// variable "only seeds on fresh `croniq init`" (issue #499).
///
/// The ungated-creation rationale — "additive, it cannot break a working
/// credential" — holds for a client that never existed. It does not hold for
/// one that was deliberately removed, and an implied `admin` is the least
/// defensible thing to grant on a guess. A declaration that names its scopes
/// still creates: saying what a client is for is an unambiguous statement that
/// it should exist, and the one-step "render the env, boot the stack" flow
/// depends on it.
fn skip_implicit_creation(decl: &Declaration) -> ClientOutcome {
    tracing::info!(
        var = %decl.key_var,
        client = %decl.name,
        "{} is set but no '{}' API client exists, and it names no scopes. A key on its own          rotates an existing client; it does not create one. To declare the client from the          environment, add {SCOPES_VAR}; to mint a key for an existing client, use          POST /v1/api-keys.",
        decl.key_var,
        decl.name,
    );
    ClientOutcome {
        client: decl.name.clone(),
        action: Action::Skipped,
        detail: Some(format!(
            "no such client, and {} names no scopes — add {SCOPES_VAR} to declare it",
            decl.key_var
        )),
    }
}

fn create_declared_client<S: AuthStore + ?Sized>(
    store: &S,
    decl: &Declaration,
    scopes: &[String],
    now: DateTime<Utc>,
    dry_run: bool,
) -> Result<ClientOutcome, StoreError> {
    let outcome = ClientOutcome {
        client: decl.name.clone(),
        action: Action::Created,
        detail: Some(format!("scopes {}", scopes.join(","))),
    };
    if dry_run {
        return Ok(outcome);
    }

    let client_id = Uuid::new_v4().to_string();
    store.create_client(&ApiClient {
        client_id: client_id.clone(),
        name: decl.name.clone(),
        scopes: scopes.to_vec(),
        is_active: true,
        created_at: now,
        managed_by: MANAGED_BY_ENV.to_string(),
    })?;
    store.create_api_key(&new_key_row(&client_id, &decl.raw_key, now))?;
    tracing::info!(
        client = %decl.name,
        scopes = %scopes.join(","),
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
    // The client's own row carrying the declared secret, if it has one — in
    // any state, revoked included. A revoked row does not *satisfy* the
    // declaration, but it is the row the declaration is about: minting a
    // second one for the same secret leaves `api_keys` with a live and a dead
    // row for one credential, and `key_hash` is not unique, so the auth path
    // then rejects that credential or not depending on which row came back
    // (issue #516).
    //
    // Ranked the way `find_api_key_by_hash` ranks: un-revoked before revoked,
    // open-ended before dated, latest deadline, newest row. A database
    // written before #516 may hold duplicates, and the reconciler has to be
    // looking at the same row the auth path would.
    let declared_key = keys
        .iter()
        .filter(|k| k.key_hash == declared_hash)
        .min_by_key(|k| {
            (
                k.revoked_at.is_some(),
                k.expires_at.is_some(),
                Reverse(k.expires_at),
                Reverse(k.created_at),
            )
        });
    let needs_ownership = client.managed_by != MANAGED_BY_ENV;
    // Only a *declared* scope set can drive a change. With the implied admin
    // stored in the declaration this read `client.scopes != ["admin"]`, so the
    // first boot after an upgrade re-escalated any `default` client an
    // operator had narrowed in the dashboard — in the granting direction,
    // from an environment that had said nothing about scopes (issue #501).
    let needs_scopes = decl
        .scopes
        .as_ref()
        .is_some_and(|declared| &client.scopes != declared);
    let needs_key = declared_key.is_none();
    // A row that matches the declaration but is dated or revoked is the
    // declared key mid-handover: a rotation stamped or ended it, and the
    // environment now declares it again. Checking only `revoked_at` made the
    // dated case look satisfied, so the reconcile reported `Unchanged` while
    // the key went on dying at its deadline — and nothing else ever clears
    // one, so every consumer 401'd for good with no reconcile able to say why
    // (issue #500). The revoked case is the same rollback under
    // `CRONIQ_API_KEY_ROTATION_GRACE=0`, or after an operator ended the key
    // with `DELETE /v1/api-keys/{id}`; it used to mint a duplicate row
    // instead (issue #516).
    let needs_revive = declared_key.filter(|k| k.revoked_at.is_some() || k.expires_at.is_some());

    let outcome = |action, detail| ClientOutcome {
        client: decl.name.clone(),
        action,
        detail,
    };

    if !needs_ownership && !needs_scopes && !needs_key && needs_revive.is_none() {
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
        if let Some(key) = needs_revive {
            pending.push(if key.revoked_at.is_some() {
                "restore the declared key, which is revoked"
            } else {
                "cancel the declared key's retirement"
            });
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
        if needs_key || needs_scopes || needs_revive.is_some() {
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
    } else if needs_revive.is_some() {
        Action::KeyRevived
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
        // An adoption with no declared scopes keeps the stored ones: taking
        // ownership says who decides from now on, not what the answer is.
        let scopes = match &decl.scopes {
            Some(declared) => declared.clone(),
            None => client.scopes.clone(),
        };
        store.create_client(&ApiClient {
            client_id: client.client_id.clone(),
            name: client.name.clone(),
            scopes,
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
        if let Some(declared) = decl.scopes.as_ref().filter(|_| needs_scopes) {
            tracing::warn!(
                client = %decl.name,
                from = %client.scopes.join(","),
                to = %declared.join(","),
                "{RECONCILE_VAR}=1 — API client scopes updated from the environment"
            );
        }
    }

    if let Some(key) = needs_revive {
        let was = if key.revoked_at.is_some() {
            "revoked"
        } else {
            "retiring"
        };
        // Restoring the row rather than minting a second one with the same
        // secret: `api_keys.key_hash` is not unique, so a duplicate leaves the
        // auth path choosing between a live and a dead row for one credential
        // (issue #516). Both columns are cleared in one write — a rotation may
        // have dated the row before an operator ended it early, and half a
        // restore is still a key that stops working.
        store.restore_api_key(&key.key_id)?;
        // Then finish the handover the way the rotation path does: whatever
        // key was installed when this one was superseded is now the outgoing
        // one. Re-minting used to do this via `retire_superseded_keys`, so
        // leaving it out would have made a rollback the one way to end up
        // with two live keys. Restoring before retiring means there is no
        // instant with nothing working.
        let superseded: Vec<ApiKey> = keys
            .iter()
            .filter(|k| k.key_id != key.key_id)
            .cloned()
            .collect();
        let retired = retire_superseded_keys(store, &superseded, now, inputs.rotation_grace)?;
        tracing::warn!(
            client = %decl.name,
            var = %decl.key_var,
            key_id = %key.key_id,
            retired = retired.count,
            "{RECONCILE_VAR}=1 — the declared key was {was}; restored it and retired {} \
             superseded key(s)",
            retired.count
        );
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
        store.set_api_key_expiry(&k.key_id, Some(deadline))?;
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
            scopes: Some(scopes.iter().map(|s| s.to_string()).collect()),
            key_var: format!(
                "CRONIQ_API_CLIENT_{}_KEY",
                name.to_uppercase().replace('-', "_")
            ),
        }
    }

    fn seed_key(store: &SqliteStore, client_id: &str, raw: &str) {
        store
            .create_api_key(&new_key_row(client_id, raw, Utc::now()))
            .unwrap();
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
    fn the_current_key_variable_declares_nothing_until_its_scopes_are_named() {
        // Issue #502: `CRONIQ_API_KEY` is also what the CLI and SDKs read to
        // *present* a credential. Exporting a narrow key on the server host —
        // which the CLI's help tells operators to do — used to declare an
        // admin-scoped `default` client, and because keys resolve by hash that
        // same narrow key then authenticated as admin.
        let d = parse_declarations(&vars(&[(KEY_VAR, "croniq_abc")])).unwrap();
        assert!(d.is_empty(), "a key with no scopes is a client credential");

        let d = parse_declarations(&vars(&[(KEY_VAR, "croniq_abc"), (SCOPES_VAR, "jobs:read")]))
            .unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, DEFAULT_CLIENT_NAME);
        assert_eq!(d[0].scopes.as_deref(), Some(&["jobs:read".to_string()][..]));
    }

    #[test]
    fn the_deprecated_key_variable_still_declares_without_naming_scopes() {
        // It has never been a client-side variable — the CLI does not read it
        // — so it needs no scopes to be unambiguous, and existing deployments
        // rotate the `default` client with it alone. Naming no scopes is what
        // limits it to rotating a client that exists (see #499).
        let d = parse_declarations(&vars(&[(LEGACY_KEY_VAR, "croniq_abc")])).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, DEFAULT_CLIENT_NAME);
        assert_eq!(d[0].scopes, None);
    }

    #[test]
    fn both_spellings_together_still_declare_the_default_client() {
        // A deployment mid-migration sets both. The legacy variable's meaning
        // wins, so the client keeps being declared rather than quietly
        // disappearing because the new spelling has no scopes.
        let d = parse_declarations(&vars(&[
            (KEY_VAR, "croniq_abc"),
            (LEGACY_KEY_VAR, "croniq_abc"),
        ]))
        .unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, DEFAULT_CLIENT_NAME);
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
    fn one_key_value_cannot_declare_two_clients() {
        // Issue #520, the mirror of the test above: there two variables named
        // one client with different values, here two clients are named with
        // one value. Both declarations would reconcile — each client created,
        // each with its own `api_keys` row carrying the same hash — and the
        // credential then authenticates as whichever row the query plan
        // returns, with only that client's scopes. The other client exists,
        // is active, has the scopes it asked for, and 403s.
        let err = parse_declarations(&vars(&[
            ("CRONIQ_API_CLIENT_PRODUCER_KEY", "croniq_shared"),
            ("CRONIQ_API_CLIENT_PRODUCER_SCOPES", "jobs:trigger"),
            ("CRONIQ_API_CLIENT_RUNNER_KEY", "croniq_shared"),
            ("CRONIQ_API_CLIENT_RUNNER_SCOPES", "work:poll"),
        ]))
        .unwrap_err();
        for expected in [
            "CRONIQ_API_CLIENT_PRODUCER_KEY",
            "CRONIQ_API_CLIENT_RUNNER_KEY",
            "producer",
            "runner",
        ] {
            assert!(err.contains(expected), "{expected} missing from: {err}");
        }
    }

    #[test]
    fn the_default_client_cannot_share_its_key_with_a_named_one() {
        // The `default` client is declared outside the `CRONIQ_API_CLIENT_`
        // prefix, so the check has to reach across both spellings — naming
        // the variable the operator actually wrote, not a reconstructed one.
        let err = parse_declarations(&vars(&[
            (KEY_VAR, "croniq_shared"),
            (SCOPES_VAR, "admin"),
            ("CRONIQ_API_CLIENT_RUNNER_KEY", "croniq_shared"),
            ("CRONIQ_API_CLIENT_RUNNER_SCOPES", "work:poll"),
        ]))
        .unwrap_err();
        assert!(err.contains(KEY_VAR), "{err}");
        assert!(err.contains("CRONIQ_API_CLIENT_RUNNER_KEY"), "{err}");

        let err = parse_declarations(&vars(&[
            (LEGACY_KEY_VAR, "croniq_shared"),
            ("CRONIQ_API_CLIENT_RUNNER_KEY", "croniq_shared"),
            ("CRONIQ_API_CLIENT_RUNNER_SCOPES", "work:poll"),
        ]))
        .unwrap_err();
        assert!(err.contains(LEGACY_KEY_VAR), "{err}");
    }

    #[test]
    fn presenting_a_declared_key_on_the_server_host_is_not_a_collision() {
        // `CRONIQ_API_KEY` with no scopes is the credential the CLI presents,
        // not a declaration (#502) — so exporting the runner's own key on the
        // server host to run `croniq` there stays legal. Rejecting it would
        // make the collision check undo the distinction #502 drew.
        let d = parse_declarations(&vars(&[
            (KEY_VAR, "croniq_runner"),
            ("CRONIQ_API_CLIENT_RUNNER_KEY", "croniq_runner"),
            ("CRONIQ_API_CLIENT_RUNNER_SCOPES", "work:poll"),
        ]))
        .unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, "runner");
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
        assert_eq!(
            d[0].scopes.as_deref(),
            Some(&["work:poll".to_string(), "work:ack".to_string()][..])
        );
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
        assert_eq!(
            d[0].scopes.as_deref(),
            Some(&["jobs:trigger".to_string()][..])
        );
        assert_eq!(d[1].scopes.as_ref().map(Vec::len), Some(3));
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
    fn an_unrecognised_attribute_is_ignored_rather_than_fatal() {
        // Issue #503: this used to abort the boot. Croniq does not read
        // `CRONIQ_API_CLIENT_FOO_SECRET`, so refusing to start over it claims
        // a whole env namespace it has no use for — and turns a version bump
        // into a restart loop for any deployment already using one.
        let declared = parse_declarations(&vars(&[("CRONIQ_API_CLIENT_FOO_SECRET", "x")]))
            .expect("an unread variable must not stop the server from booting");
        assert!(declared.is_empty());
    }

    #[test]
    fn a_misspelled_scopes_suffix_still_fails_loudly() {
        // The reason ignoring unknown suffixes is safe: the typo that would
        // actually change behaviour — scopes that never arrive — is caught by
        // the missing-scopes check instead of the suffix check.
        let err = parse_declarations(&vars(&[
            ("CRONIQ_API_CLIENT_P_KEY", "croniq_p"),
            ("CRONIQ_API_CLIENT_P_SCOPE", "jobs:read"),
        ]))
        .unwrap_err();
        assert!(err.contains("no scopes"), "{err}");
    }

    #[test]
    fn a_malformed_legacy_key_is_ignored_the_way_v0_33_did() {
        // A leftover `CRONIQ_INIT_API_KEY=changeme` from an old template was a
        // warning on v0.33.0 and a fatal boot error after the upgrade, taking
        // down a scheduler that had been running fine (issue #503).
        let declared = parse_declarations(&vars(&[(LEGACY_KEY_VAR, "changeme")]))
            .expect("a leftover placeholder in the deprecated variable must not be fatal");
        assert!(declared.is_empty());
    }

    #[test]
    fn a_malformed_current_key_is_still_fatal() {
        // The current spellings are new in this release: a bad value there is
        // a declaration just written wrong, not a leftover.
        let err = parse_declarations(&vars(&[(KEY_VAR, "hunter2")])).unwrap_err();
        assert!(err.contains(KEY_VAR), "{err}");
        let err = parse_declarations(&vars(&[("CRONIQ_API_CLIENT_P_KEY", "hunter2")])).unwrap_err();
        assert!(err.contains("croniq_"), "{err}");
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
            (SCOPES_VAR, "admin"),
            (RECONCILE_VAR, "1"),
            (ROTATION_GRACE_VAR, "5m"),
        ]))
        .unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, DEFAULT_CLIENT_NAME);
    }

    // ─── Reconciliation ──────────────────────────────────────────────────────

    /// A declaration whose scopes variable is unset, as the bare
    /// `CRONIQ_API_KEY` / `CRONIQ_INIT_API_KEY` spelling produces.
    fn decl_without_scopes(name: &str, key: &str) -> Declaration {
        Declaration {
            scopes: None,
            // Since #502 the deprecated spelling is the only one that
            // declares without naming scopes.
            key_var: LEGACY_KEY_VAR.into(),
            ..decl(name, key, &[])
        }
    }

    #[test]
    fn an_undeclared_scope_set_never_narrows_or_widens_an_existing_client() {
        // Issue #501: the implied admin was stored in the declaration, so a
        // `default` client an operator had narrowed to jobs:trigger in the
        // dashboard looked like a scope drift — and the first boot with the
        // legacy reconcile pair set silently put it back to full admin.
        let s = store();
        let id = seed_client(&s, DEFAULT_CLIENT_NAME, &["jobs:trigger"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_same");

        let out = reconcile(
            &s,
            &inputs(
                vec![decl_without_scopes(DEFAULT_CLIENT_NAME, "croniq_same")],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Unchanged);
        let clients = s.list_clients().unwrap();
        assert_eq!(clients[0].scopes, vec!["jobs:trigger"]);
    }

    #[test]
    fn adoption_without_declared_scopes_keeps_the_stored_ones() {
        // Taking ownership settles who decides from now on; it is not itself
        // an answer about what the scopes should be. Rewriting them to the
        // implied admin here would be the same escalation by another route.
        let s = store();
        let id = seed_client(&s, DEFAULT_CLIENT_NAME, &["jobs:read"], "api");
        seed_key(&s, &id, "croniq_same");

        let out = reconcile(
            &s,
            &inputs(
                vec![decl_without_scopes(DEFAULT_CLIENT_NAME, "croniq_same")],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Adopted);
        let clients = s.list_clients().unwrap();
        assert_eq!(clients[0].managed_by, MANAGED_BY_ENV);
        assert_eq!(clients[0].scopes, vec!["jobs:read"]);
    }

    #[test]
    fn an_explicitly_declared_scope_set_still_syncs() {
        // The feature itself must keep working: when the environment does name
        // scopes, they win.
        let s = store();
        let id = seed_client(&s, "producer", &["jobs:read"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_p");

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_p", &["jobs:trigger"])],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::ScopesUpdated);
        let clients = s.list_clients().unwrap();
        assert_eq!(clients[0].scopes, vec!["jobs:trigger"]);
    }

    fn legacy_decl(key: &str) -> Declaration {
        Declaration {
            name: DEFAULT_CLIENT_NAME.into(),
            raw_key: key.into(),
            scopes: None,
            key_var: LEGACY_KEY_VAR.into(),
        }
    }

    #[test]
    fn the_deprecated_variable_does_not_recreate_a_deleted_client() {
        // Issue #499: an operator deletes the `default` client after a key
        // leak and leaves CRONIQ_INIT_API_KEY=<leaked> in the deployment. On
        // v0.33.0 that was a no-op. Recreating it puts the leaked credential
        // back, active and admin-scoped — and as managed_by=env, so the API
        // delete then answers 409 and the operator cannot undo it at all.
        let s = store();
        let out = reconcile(
            &s,
            &inputs(
                vec![legacy_decl("croniq_leaked")],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Skipped);
        assert!(s.list_clients().unwrap().is_empty());
        assert!(
            s.find_api_key_by_hash(&hash_api_key("croniq_leaked"))
                .unwrap()
                .is_none(),
            "the leaked key must not be installed anywhere"
        );
    }

    #[test]
    fn the_deprecated_variable_still_rotates_a_client_that_exists() {
        // The behaviour it was introduced for (#217) is untouched: what it
        // does not do is bring a client back, not rotate one.
        let s = store();
        let id = seed_client(&s, DEFAULT_CLIENT_NAME, &["admin"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_old");

        let out = reconcile(
            &s,
            &inputs(vec![legacy_decl("croniq_new")], true, Duration::minutes(15)),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Rotated);
        assert!(
            s.find_api_key_by_hash(&hash_api_key("croniq_new"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn the_current_spellings_still_create_a_missing_client() {
        // Declaring a client that should exist is what they are for, and the
        // one-step "render env, boot stack" flow depends on it.
        let s = store();
        let out = reconcile(
            &s,
            &inputs(
                vec![
                    Declaration {
                        name: DEFAULT_CLIENT_NAME.into(),
                        raw_key: "croniq_d".into(),
                        scopes: Some(vec!["admin".into()]),
                        key_var: KEY_VAR.into(),
                    },
                    decl("producer", "croniq_p", &["jobs:trigger"]),
                ],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert!(out.iter().all(|o| o.action == Action::Created), "{out:?}");
        assert_eq!(s.list_clients().unwrap().len(), 2);
    }

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
    fn rolling_a_rotation_back_revives_the_re_declared_key() {
        // Issue #500: rotate A -> B with the opt-in, which stamps A with an
        // expiry, then roll back so the environment declares A again. The
        // match on `revoked_at` alone made that look satisfied, so reconcile
        // reported Unchanged while A went on dying at its deadline — and
        // nothing clears one, so every consumer 401'd for good.
        let s = store();
        let decl_a = decl("producer", "croniq_a", &["jobs:trigger"]);
        let decl_b = decl("producer", "croniq_b", &["jobs:trigger"]);
        reconcile(
            &s,
            &inputs(vec![decl_a.clone()], false, Duration::minutes(15)),
        )
        .unwrap();
        reconcile(&s, &inputs(vec![decl_b], true, Duration::minutes(15))).unwrap();

        let id = s.list_clients().unwrap()[0].client_id.clone();
        let retired = s
            .list_api_keys(&id)
            .unwrap()
            .into_iter()
            .find(|k| k.key_hash == hash_api_key("croniq_a"))
            .expect("A is still stored, just dated");
        assert!(retired.expires_at.is_some(), "precondition: A is retiring");

        // The rollback.
        let out = reconcile(&s, &inputs(vec![decl_a], true, Duration::minutes(15))).unwrap();

        assert_eq!(out[0].action, Action::KeyRevived);
        let keys = s.list_api_keys(&id).unwrap();
        let a = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_a"))
            .unwrap();
        assert_eq!(a.expires_at, None, "the deadline must be gone");
        assert!(a.revoked_at.is_none());
        assert_eq!(
            keys.iter()
                .filter(|k| k.key_hash == hash_api_key("croniq_a"))
                .count(),
            1,
            "reviving must not mint a second row with the same secret — key_hash is not \
             unique, so auth would be choosing between them"
        );
        let b = keys
            .iter()
            .find(|k| k.key_hash == hash_api_key("croniq_b"))
            .expect("B is still stored");
        assert!(
            b.expires_at.is_some(),
            "the rollback supersedes B, so B retires with the grace window — the same \
             thing re-minting used to do"
        );
    }

    #[test]
    fn rolling_back_to_a_revoked_key_restores_it_instead_of_minting_a_duplicate() {
        // Issue #516: under CRONIQ_API_KEY_ROTATION_GRACE=0 a rotation revokes
        // the superseded key outright, so a rollback declares a key whose row
        // is revoked. `needs_key` read that row as absent and minted a second
        // one for the same secret — and `key_hash` is not unique, so auth then
        // answered from whichever row the planner handed back, live or dead.
        let s = store();
        let decl_a = decl("producer", "croniq_a", &["jobs:trigger"]);
        let decl_b = decl("producer", "croniq_b", &["jobs:trigger"]);
        reconcile(&s, &inputs(vec![decl_a.clone()], false, Duration::zero())).unwrap();
        reconcile(&s, &inputs(vec![decl_b], true, Duration::zero())).unwrap();

        let id = s.list_clients().unwrap()[0].client_id.clone();
        assert!(
            s.list_api_keys(&id)
                .unwrap()
                .iter()
                .find(|k| k.key_hash == hash_api_key("croniq_a"))
                .expect("A is still stored")
                .revoked_at
                .is_some(),
            "precondition: a zero grace revokes A outright"
        );

        let out = reconcile(&s, &inputs(vec![decl_a], true, Duration::zero())).unwrap();

        assert_eq!(out[0].action, Action::KeyRevived);
        let keys = s.list_api_keys(&id).unwrap();
        let a: Vec<_> = keys
            .iter()
            .filter(|k| k.key_hash == hash_api_key("croniq_a"))
            .collect();
        assert_eq!(a.len(), 1, "the revoked row is restored, not duplicated");
        assert_eq!(a[0].revoked_at, None);
        assert_eq!(a[0].expires_at, None);
        assert!(
            keys.iter()
                .find(|k| k.key_hash == hash_api_key("croniq_b"))
                .expect("B is still stored")
                .revoked_at
                .is_some(),
            "and B, now superseded, is revoked — a zero grace on the way back too"
        );
    }

    #[test]
    fn restoring_a_declared_key_clears_a_deadline_and_a_revocation_together() {
        // A rotation dates the outgoing key, then an operator ends it early
        // with DELETE /v1/api-keys/{id}. Re-declaring it has to clear both
        // columns: a key that comes back still carrying the old deadline is a
        // credential that works until it silently does not.
        let s = store();
        let id = seed_client(&s, "producer", &["jobs:trigger"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_a");
        let key_id = s.list_api_keys(&id).unwrap()[0].key_id.clone();
        s.set_api_key_expiry(&key_id, Some(Utc::now() + Duration::minutes(5)))
            .unwrap();
        s.revoke_api_key(&key_id, Utc::now()).unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_a", &["jobs:trigger"])],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::KeyRevived);
        let keys = s.list_api_keys(&id).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].revoked_at, None);
        assert_eq!(keys[0].expires_at, None);
    }

    #[test]
    fn a_revoked_declared_key_is_reported_rather_than_quietly_reinstalled() {
        // The opt-in gates the restore like every other write, and the outcome
        // has to name it: "revoking alone does not un-declare a credential" is
        // only actionable if the reload says the environment is asking for the
        // revoked value back.
        let s = store();
        let id = seed_client(&s, "producer", &["jobs:trigger"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_a");
        let key_id = s.list_api_keys(&id).unwrap()[0].key_id.clone();
        s.revoke_api_key(&key_id, Utc::now()).unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_a", &["jobs:trigger"])],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Blocked);
        let detail = out[0].detail.as_deref().unwrap_or_default();
        assert!(detail.contains("revoked"), "{detail}");
        let keys = s.list_api_keys(&id).unwrap();
        assert_eq!(keys.len(), 1, "and no second row for the same secret");
        assert!(
            keys[0].revoked_at.is_some(),
            "nothing may be written without the opt-in"
        );
    }

    #[test]
    fn a_dead_duplicate_does_not_drag_a_working_key_into_the_restore_path() {
        // A database written before #516 can hold a revoked *and* a live row
        // for one secret, because that is what re-declaring used to produce.
        // The reconciler ranks them the way find_api_key_by_hash does, so it
        // looks at the row auth would use: otherwise it would "restore" the
        // dead one and report a change on every boot while the live row was
        // already doing the job.
        let s = store();
        let id = seed_client(&s, "producer", &["jobs:trigger"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_a");
        let dead = s.list_api_keys(&id).unwrap()[0].key_id.clone();
        s.revoke_api_key(&dead, Utc::now()).unwrap();
        seed_key(&s, &id, "croniq_a");

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_a", &["jobs:trigger"])],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Unchanged);
        let keys = s.list_api_keys(&id).unwrap();
        assert_eq!(keys.len(), 2, "and nothing is rewritten");
        assert_eq!(
            keys.iter().filter(|k| k.revoked_at.is_none()).count(),
            1,
            "the live row stays live and the dead one stays dead"
        );
    }

    #[test]
    fn a_retiring_declared_key_is_reported_rather_than_silently_dying() {
        // Without the opt-in nothing may be written, but the outcome must say
        // what is pending. Reporting `Unchanged` here is the part of #500 that
        // made the bug undiagnosable.
        let s = store();
        let id = seed_client(&s, "producer", &["jobs:trigger"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_a");
        let key_id = s.list_api_keys(&id).unwrap()[0].key_id.clone();
        s.set_api_key_expiry(&key_id, Some(Utc::now() + Duration::minutes(5)))
            .unwrap();

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_a", &["jobs:trigger"])],
                false,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Blocked);
        let detail = out[0].detail.as_deref().unwrap_or_default();
        assert!(detail.contains("retirement"), "{detail}");
        // Still un-written: the opt-in gates every change.
        assert!(
            s.list_api_keys(&id).unwrap()[0].expires_at.is_some(),
            "nothing may be written without the opt-in"
        );
    }

    #[test]
    fn an_open_ended_declared_key_stays_unchanged() {
        // The common case must not be dragged into the revive path: a key with
        // no deadline is already what the declaration asks for.
        let s = store();
        let id = seed_client(&s, "producer", &["jobs:trigger"], MANAGED_BY_ENV);
        seed_key(&s, &id, "croniq_a");

        let out = reconcile(
            &s,
            &inputs(
                vec![decl("producer", "croniq_a", &["jobs:trigger"])],
                true,
                Duration::minutes(15),
            ),
        )
        .unwrap();

        assert_eq!(out[0].action, Action::Unchanged);
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

    // ─── Variable-name reconstruction ────────────────────────────────────────

    #[test]
    fn the_default_client_maps_back_to_the_variable_that_declares_it() {
        // The bug behind #481: the reconstruction assumed every env-managed
        // client lives under CRONIQ_API_CLIENT_, so it named
        // CRONIQ_API_CLIENT_DEFAULT_KEY — a *second* declaration of `default`,
        // which parse_declarations rejects as a conflict. Following the 409's
        // advice therefore broke the next boot.
        assert_eq!(canonical_key_var(DEFAULT_CLIENT_NAME), KEY_VAR);
        assert!(!canonical_key_var(DEFAULT_CLIENT_NAME).starts_with(CLIENT_PREFIX));
    }

    #[test]
    fn a_named_client_maps_back_to_its_prefixed_variable() {
        assert_eq!(
            canonical_key_var("runner-poll"),
            "CRONIQ_API_CLIENT_RUNNER_POLL_KEY"
        );
        assert_eq!(
            canonical_key_var("reporting"),
            "CRONIQ_API_CLIENT_REPORTING_KEY"
        );
    }

    #[test]
    fn the_reconstruction_round_trips_through_the_parser() {
        // The refusal message is only actionable if editing the variable it
        // names re-declares the same client. Feed each reconstructed name back
        // through the parser and check the client comes out unchanged.
        for name in ["default", "runner-poll", "reporting", "a1"] {
            let var = canonical_key_var(name);
            let mut vars = BTreeMap::new();
            vars.insert(var.clone(), "croniq_secret".to_string());
            // Every declaration needs its scopes named, `default` included
            // since #502.
            let scopes_var = if var == KEY_VAR {
                SCOPES_VAR.to_string()
            } else {
                var.replace("_KEY", "_SCOPES")
            };
            vars.insert(scopes_var, "jobs:read".to_string());
            let declared = parse_declarations(&vars)
                .unwrap_or_else(|e| panic!("{var} must be a valid declaration: {e}"));
            assert_eq!(
                declared.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
                vec![name],
                "{var} declared the wrong client"
            );
        }
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

    #[test]
    fn grace_reports_an_out_of_range_value_instead_of_panicking() {
        // Reachable since the grace accepts `d`: this parses to a u64 second
        // count that chrono cannot represent. `Duration::seconds` would panic
        // here, taking the boot down with a backtrace instead of a message.
        let err = rotation_grace_from_env(Some("200000000000d")).unwrap_err();
        assert!(err.contains(ROTATION_GRACE_VAR), "{err}");
        assert!(err.contains("representable"), "{err}");
    }

    #[test]
    fn grace_never_resolves_to_a_negative_window() {
        // A value above i64::MAX seconds used to wrap negative, and a negative
        // grace means `now + grace` is in the past — an instant revoke of every
        // superseded key, which is the opposite of what the knob is for.
        for raw in ["18446744073709551615", "9223372036854775808"] {
            match rotation_grace_from_env(Some(raw)) {
                Ok(d) => panic!("{raw} should be rejected, got {d}"),
                Err(e) => assert!(e.contains(ROTATION_GRACE_VAR), "{e}"),
            }
        }
    }
}
