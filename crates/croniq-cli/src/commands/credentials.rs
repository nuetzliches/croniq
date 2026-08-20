//! `croniq api-clients` / `croniq api-keys` — manage machine credentials
//! from the command line (issue #475).
//!
//! Everything here goes over HTTP rather than straight at the database the
//! way `croniq init` does. That is deliberate: `init` only works on a fresh
//! data dir, on the same host, and never against Postgres. Day-2 credential
//! work has to reach a running server, wherever it lives.

use chrono::{DateTime, Utc};
use miette::{IntoDiagnostic, Result, miette};
use serde::{Deserialize, Serialize};

use super::remote::Remote;

// ─── Wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiClient {
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub is_active: bool,
    /// Absent on servers older than #471; those rows are API-managed.
    #[serde(default)]
    pub managed_by: Option<String>,
}

impl ApiClient {
    fn owner(&self) -> &str {
        self.managed_by.as_deref().unwrap_or("api")
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiKeySummary {
    pub key_id: String,
    pub key_prefix: String,
    pub created_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

/// `POST /v1/api-clients` answers with the created identity only — no
/// `is_active`, no `managed_by`. Decoding it as [`ApiClient`] fails on the
/// missing fields even though the client was created, which is the worst
/// shape of error: the command reports failure for work that succeeded.
#[derive(Debug, Deserialize, Serialize)]
pub struct CreatedApiClient {
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreatedApiKey {
    pub raw_key: String,
    pub key_id: String,
    pub key_prefix: String,
    pub client_id: String,
}

#[derive(Serialize)]
struct CreateClientBody<'a> {
    name: &'a str,
    scopes: &'a [String],
}

#[derive(Serialize)]
struct UpdateClientBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_active: Option<bool>,
}

#[derive(Serialize)]
struct CreateKeyBody<'a> {
    client_id: &'a str,
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value).into_diagnostic()?);
    Ok(())
}

// ─── api-clients ─────────────────────────────────────────────────────────────

pub fn clients_list(remote: &Remote, json: bool) -> Result<()> {
    let clients: Vec<ApiClient> = remote.get_json("/v1/api-clients")?;
    if json {
        return print_json(&clients);
    }
    if clients.is_empty() {
        println!("No API clients.");
        return Ok(());
    }
    println!(
        "{:<38} {:<20} {:<8} {:<8} SCOPES",
        "CLIENT ID", "NAME", "OWNER", "ACTIVE"
    );
    println!("{}", "-".repeat(96));
    for c in &clients {
        println!(
            "{:<38} {:<20} {:<8} {:<8} {}",
            c.client_id,
            c.name,
            c.owner(),
            if c.is_active { "yes" } else { "no" },
            c.scopes.join(",")
        );
    }
    // Owner is not decoration: every mutation on an env-declared client is
    // refused, so it explains in advance why `update` would fail.
    if clients.iter().any(|c| c.owner() == "env") {
        println!();
        println!(
            "Clients owned by 'env' are declared with CRONIQ_API_CLIENT_<NAME>_KEY. \
             Change the environment and reload — the API refuses to edit them."
        );
    }
    Ok(())
}

pub fn clients_create(remote: &Remote, name: &str, scopes: &[String], json: bool) -> Result<()> {
    if scopes.is_empty() {
        return Err(miette!(
            "--scopes is required: a client with no scopes can authorise nothing"
        ));
    }
    let created: CreatedApiClient =
        remote.post_json("/v1/api-clients", &CreateClientBody { name, scopes })?;
    if json {
        return print_json(&created);
    }
    println!("Created API client '{}'", created.name);
    println!("  client_id: {}", created.client_id);
    println!("  scopes:    {}", created.scopes.join(","));
    println!();
    println!(
        "Mint a key for it with:  croniq api-keys create --client {}",
        created.client_id
    );
    Ok(())
}

pub fn clients_update(
    remote: &Remote,
    client_id: &str,
    name: Option<&str>,
    scopes: Option<&[String]>,
    is_active: Option<bool>,
    json: bool,
) -> Result<()> {
    if name.is_none() && scopes.is_none() && is_active.is_none() {
        return Err(miette!(
            "nothing to change — pass --name, --scopes, --active or --inactive"
        ));
    }
    let updated: ApiClient = remote.put_json(
        &format!("/v1/api-clients/{client_id}"),
        &UpdateClientBody {
            name,
            scopes,
            is_active,
        },
    )?;
    if json {
        return print_json(&updated);
    }
    println!("Updated API client '{}'", updated.name);
    println!("  scopes: {}", updated.scopes.join(","));
    println!("  active: {}", if updated.is_active { "yes" } else { "no" });
    Ok(())
}

pub fn clients_delete(remote: &Remote, client_id: &str) -> Result<()> {
    remote.delete(&format!("/v1/api-clients/{client_id}"))?;
    println!("Deleted API client {client_id} (its API keys go with it).");
    Ok(())
}

// ─── api-keys ────────────────────────────────────────────────────────────────

/// The state to print for one key.
///
/// Four distinct states, and `retiring` is the whole point of the rotation
/// grace window (#472): still working, but on a deadline. Once that deadline
/// passes the key is *not* still retiring — the server's auth middleware has
/// been answering `401` for it since the instant in the `EXPIRES` column, so
/// calling it `retiring` describes a handover that is already over and invites
/// the operator to keep waiting for something that already happened.
fn key_state(key: &ApiKeySummary, now: DateTime<Utc>) -> &'static str {
    if key.revoked_at.is_some() {
        return "revoked";
    }
    match key.expires_at.as_deref().and_then(parse_timestamp) {
        // Strictly-later, to match what the server actually enforces:
        // `auth_middleware` rejects on `now > expires`, so the deadline
        // instant itself still authenticates.
        Some(expiry) if expiry < now => "expired",
        Some(_) => "retiring",
        // An `expires_at` this CLI cannot parse still means a deadline exists;
        // reporting `active` would be the one wrong answer, so keep the
        // deadline visible and let the EXPIRES column speak for itself.
        None if key.expires_at.is_some() => "retiring",
        None => "active",
    }
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

pub fn keys_list(remote: &Remote, client_id: &str, json: bool) -> Result<()> {
    let keys: Vec<ApiKeySummary> =
        remote.get_json(&format!("/v1/api-keys?client_id={client_id}"))?;
    if json {
        return print_json(&keys);
    }
    if keys.is_empty() {
        println!("No API keys for client {client_id}.");
        return Ok(());
    }
    println!(
        "{:<38} {:<14} {:<10} {:<26} CREATED",
        "KEY ID", "PREFIX", "STATE", "EXPIRES"
    );
    println!("{}", "-".repeat(110));
    let now = Utc::now();
    for k in &keys {
        let state = key_state(k, now);
        println!(
            "{:<38} {:<14} {:<10} {:<26} {}",
            k.key_id,
            k.key_prefix,
            state,
            k.expires_at.as_deref().unwrap_or("-"),
            k.created_at
        );
    }
    let states: Vec<&str> = keys.iter().map(|k| key_state(k, now)).collect();
    if states.contains(&"retiring") {
        println!();
        println!(
            "A 'retiring' key was replaced by a rotation and stops working at its expiry. \
             To end one now: croniq api-keys revoke <key-id>"
        );
    }
    if states.contains(&"expired") {
        println!();
        println!(
            "An 'expired' key's rotation grace window has already elapsed — the server \
             rejects it with 401. The row stays for audit; nothing needs doing."
        );
    }
    Ok(())
}

pub fn keys_create(remote: &Remote, client_id: &str, json: bool) -> Result<()> {
    let created: CreatedApiKey = remote.post_json("/v1/api-keys", &CreateKeyBody { client_id })?;
    if json {
        return print_json(&created);
    }
    // Printed to stdout unconditionally, unlike `croniq init`, which routes
    // secrets through CredentialSink when stdout is not a terminal. The
    // difference is intent: there the key is a side effect of bootstrapping,
    // here producing it is the entire command, and a deployment script piping
    // this into a variable is the expected use.
    println!("Created API key for client {}", created.client_id);
    println!();
    println!("  {}", created.raw_key);
    println!();
    println!(
        "This is the only time the key is shown. key_id: {}",
        created.key_id
    );
    Ok(())
}

pub fn keys_revoke(remote: &Remote, key_id: &str) -> Result<()> {
    remote.delete(&format!("/v1/api-keys/{key_id}"))?;
    println!("Revoked API key {key_id}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(managed_by: Option<&str>) -> ApiClient {
        ApiClient {
            client_id: "c1".into(),
            name: "runner".into(),
            scopes: vec!["work:poll".into()],
            is_active: true,
            managed_by: managed_by.map(str::to_string),
        }
    }

    #[test]
    fn a_server_without_the_ownership_field_reads_as_api_managed() {
        // Older servers omit `managed_by`; defaulting to "env" would tell the
        // operator their client is uneditable when it is not.
        assert_eq!(client(None).owner(), "api");
        assert_eq!(client(Some("env")).owner(), "env");
    }

    #[test]
    fn a_client_payload_round_trips_without_the_ownership_field() {
        let parsed: ApiClient = serde_json::from_str(
            r#"{"client_id":"c1","name":"x","scopes":["jobs:read"],"is_active":true}"#,
        )
        .expect("an older server's response must still parse");
        assert_eq!(parsed.owner(), "api");
    }

    #[test]
    fn the_create_response_decodes_without_is_active_or_ownership() {
        // POST /v1/api-clients returns the created identity only. Decoding it
        // as the full ApiClient made `api-clients create` report a decode
        // failure for a client the server had already created.
        let parsed: CreatedApiClient =
            serde_json::from_str(r#"{"client_id":"c1","name":"reporting","scopes":["jobs:read"]}"#)
                .expect("the create response shape must decode");
        assert_eq!(parsed.name, "reporting");
    }

    fn key(expires_at: Option<&str>, revoked_at: Option<&str>) -> ApiKeySummary {
        ApiKeySummary {
            key_id: "k1".into(),
            key_prefix: "croniq_ab".into(),
            created_at: "2026-08-20T00:00:00Z".into(),
            expires_at: expires_at.map(str::to_string),
            revoked_at: revoked_at.map(str::to_string),
        }
    }

    fn at(raw: &str) -> DateTime<Utc> {
        parse_timestamp(raw).expect("test timestamp must parse")
    }

    #[test]
    fn an_elapsed_grace_window_reads_as_expired_not_retiring() {
        // The bug: a key whose deadline passed hours ago was still labelled
        // `retiring`, which reads as "works until its expiry" for a key the
        // server has been 401-ing ever since.
        let k = key(Some("2026-08-20T09:15:00Z"), None);
        assert_eq!(key_state(&k, at("2026-08-20T10:00:00Z")), "expired");
        assert_eq!(key_state(&k, at("2026-08-20T09:10:00Z")), "retiring");
        // The deadline instant itself still works: `auth_middleware` rejects
        // on `now > expires`, so the CLI must not call it dead a moment early.
        assert_eq!(key_state(&k, at("2026-08-20T09:15:00Z")), "retiring");
        assert_eq!(key_state(&k, at("2026-08-20T09:15:01Z")), "expired");
    }

    #[test]
    fn revocation_outranks_any_expiry() {
        let k = key(Some("2026-08-20T09:15:00Z"), Some("2026-08-20T09:01:00Z"));
        assert_eq!(key_state(&k, at("2026-08-20T09:05:00Z")), "revoked");
        assert_eq!(key_state(&k, at("2026-08-20T23:00:00Z")), "revoked");
    }

    #[test]
    fn a_key_with_no_deadline_is_active() {
        assert_eq!(
            key_state(&key(None, None), at("2026-08-20T10:00:00Z")),
            "active"
        );
    }

    #[test]
    fn an_unparsable_expiry_keeps_the_deadline_visible() {
        // Never report `active` for a row that carries a deadline this CLI
        // could not read — that is the one answer that would mislead.
        assert_eq!(
            key_state(
                &key(Some("not-a-timestamp"), None),
                at("2026-08-20T10:00:00Z")
            ),
            "retiring"
        );
    }

    #[test]
    fn a_key_summary_parses_without_the_optional_timestamps() {
        // `expires_at` / `revoked_at` are omitted when unset (#472).
        let parsed: ApiKeySummary = serde_json::from_str(
            r#"{"key_id":"k1","client_id":"c1","key_prefix":"croniq_ab","created_at":"2026-08-20T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(parsed.expires_at.is_none());
        assert!(parsed.revoked_at.is_none());
    }
}
