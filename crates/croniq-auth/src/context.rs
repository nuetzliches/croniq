//! Caller context and scope model.

use croniq_store::models::Role;
use serde::{Deserialize, Serialize};

/// Type of authenticated caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallerType {
    ApiKey,
    User,
}

/// How the caller authenticated. Independent of `CallerType` so a User can
/// be authenticated via password (with or without TOTP), a personal access
/// token, or an OIDC SSO redirect; the `caller_id` is the same `user_id`
/// in all three cases but the audit log distinguishes them.
///
/// `Pat` and `Oidc` variants are reserved for follow-up PRs (A4 + A5); the
/// JSON serialisation is fixed now to avoid a breaking change later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    /// Username + password (with optional TOTP step-up).
    Password,
    /// Service API key.
    ApiKey,
    /// Personal access token (issued by a user for themselves). PR-A4.
    Pat,
    /// OIDC SSO redirect. PR-A5.
    Oidc,
}

/// The authenticated caller's identity and permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerContext {
    pub caller_type: CallerType,
    /// Identity of the caller. For users this is `user_id`; for API keys
    /// it is `key_id`. Always set.
    pub caller_id: String,
    /// API client this caller belongs to. For API keys this is the
    /// owning `api_clients.client_id`. For user logins it is set to the
    /// user_id (kept as a single value so existing refresh-token rows
    /// continue to point at something stable until PR-A2 introduces a
    /// proper user-token table).
    pub client_id: String,
    /// Set when the caller is a user (not an API key). None for API keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Role of the user. None for API keys (their permissions come from
    /// the `api_clients.scopes` column instead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// How this caller authenticated. Drives audit log granularity.
    pub auth_method: AuthMethod,
    pub scopes: Vec<String>,
}

impl CallerContext {
    /// Check if the caller has a specific scope (or the admin wildcard).
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == "admin" || s == scope)
    }

    /// Check if the caller has any of the given scopes.
    pub fn has_any_scope(&self, scopes: &[&str]) -> bool {
        scopes.iter().any(|s| self.has_scope(s))
    }

    /// True if the caller carries the admin wildcard.
    pub fn is_admin(&self) -> bool {
        self.scopes.iter().any(|s| s == "admin")
    }
}

/// Known scopes in the system.
///
/// `admin` acts as a wildcard — `has_scope()` returns true for any scope check
/// when an `admin` claim is present. Group scopes by domain so a token issuer
/// can grant the minimum needed (e.g. `jobs:read` + `executions:read` for a
/// monitoring dashboard, `work:*` for a runner).
pub struct Scope;

impl Scope {
    // Wildcard
    pub const ADMIN: &str = "admin";

    // Jobs
    pub const JOBS_READ: &str = "jobs:read";
    pub const JOBS_WRITE: &str = "jobs:write";
    pub const JOBS_REGISTER: &str = "jobs:register";
    pub const JOBS_TRIGGER: &str = "jobs:trigger";

    // Schedules (a.k.a. trigger definitions)
    pub const SCHEDULES_READ: &str = "schedules:read";
    pub const SCHEDULES_WRITE: &str = "schedules:write";

    // Executions + their logs
    pub const EXECUTIONS_READ: &str = "executions:read";
    /// Cancel an inflight or queued execution (issue #176).
    /// Granted to Operator by default; admin's wildcard implies it.
    pub const EXECUTIONS_CANCEL: &str = "executions:cancel";

    // Dead letters
    pub const DEAD_LETTERS_READ: &str = "dead-letters:read";
    pub const DEAD_LETTERS_WRITE: &str = "dead-letters:write";

    // Calendars
    pub const CALENDARS_READ: &str = "calendars:read";
    pub const CALENDARS_WRITE: &str = "calendars:write";

    // Runner-pull protocol — granted to runner API keys
    pub const WORK_POLL: &str = "work:poll";
    pub const WORK_RENEW: &str = "work:renew";
    pub const WORK_ACK: &str = "work:ack";
    pub const WORK_EVENTS: &str = "work:events";

    // Runner inventory + lifecycle
    pub const RUNNERS_READ: &str = "runners:read";
    pub const RUNNERS_WRITE: &str = "runners:write";
    pub const RUNNERS_HEARTBEAT: &str = "runners:heartbeat";

    // Identity / auth management — privileged
    pub const API_CLIENTS_ADMIN: &str = "api-clients:admin";
    pub const API_KEYS_ADMIN: &str = "api-keys:admin";
    /// User management: create/list/update/delete /v1/users + issue
    /// invitations. Implied by `admin` wildcard. Operators and viewers
    /// never get this even via role default scopes.
    pub const USERS_ADMIN: &str = "users:admin";

    // HTTP MCP transport (Streamable-HTTP at /mcp)
    pub const MCP_READ: &str = "mcp:read";
    pub const MCP_WRITE: &str = "mcp:write";

    // Failure alerts (issue #140). Read-only access to the current
    // alerts config + delivery log. Rules and channels themselves are
    // DSL-managed; `alerts:write` (issue #231) does NOT edit them — it
    // gates the operational-override surface (snooze / disable /
    // re-throttle a rule), a temporary runtime-state layer next to Adopt.
    pub const ALERTS_READ: &str = "alerts:read";
    /// Operational overrides for DSL-managed alert rules (issue #231).
    /// Admin-only for v1 — not granted to Operator/Viewer role defaults.
    /// Implied by the `admin` wildcard.
    pub const ALERTS_WRITE: &str = "alerts:write";
}

/// Default scope set for a user role. The login handler embeds these in
/// the issued JWT so a role change on the user row propagates only on the
/// next login — same behaviour as scope changes on API clients today.
pub fn default_scopes_for_role(role: Role) -> Vec<String> {
    match role {
        Role::Admin => vec![Scope::ADMIN.to_string()],
        Role::Operator => vec![
            Scope::JOBS_READ.to_string(),
            Scope::JOBS_WRITE.to_string(),
            Scope::JOBS_TRIGGER.to_string(),
            Scope::SCHEDULES_READ.to_string(),
            Scope::SCHEDULES_WRITE.to_string(),
            Scope::CALENDARS_READ.to_string(),
            Scope::CALENDARS_WRITE.to_string(),
            Scope::EXECUTIONS_READ.to_string(),
            Scope::EXECUTIONS_CANCEL.to_string(),
            Scope::DEAD_LETTERS_READ.to_string(),
            Scope::DEAD_LETTERS_WRITE.to_string(),
            Scope::RUNNERS_READ.to_string(),
            Scope::ALERTS_READ.to_string(),
        ],
        Role::Viewer => vec![
            Scope::JOBS_READ.to_string(),
            Scope::SCHEDULES_READ.to_string(),
            Scope::CALENDARS_READ.to_string(),
            Scope::EXECUTIONS_READ.to_string(),
            Scope::DEAD_LETTERS_READ.to_string(),
            Scope::RUNNERS_READ.to_string(),
            Scope::ALERTS_READ.to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_ctx(scopes: Vec<String>, role: Option<Role>) -> CallerContext {
        CallerContext {
            caller_type: CallerType::User,
            caller_id: "user-1".into(),
            client_id: "user-1".into(),
            user_id: Some("user-1".into()),
            role,
            auth_method: AuthMethod::Password,
            scopes,
        }
    }

    #[test]
    fn admin_has_all_scopes() {
        let ctx = user_ctx(vec!["admin".into()], Some(Role::Admin));
        assert!(ctx.has_scope("jobs:read"));
        assert!(ctx.has_scope("anything"));
        assert!(ctx.is_admin());
    }

    #[test]
    fn specific_scope_check_for_api_key() {
        let ctx = CallerContext {
            caller_type: CallerType::ApiKey,
            caller_id: "key-1".into(),
            client_id: "client-1".into(),
            user_id: None,
            role: None,
            auth_method: AuthMethod::ApiKey,
            scopes: vec!["jobs:read".into(), "runners:read".into()],
        };
        assert!(ctx.has_scope("jobs:read"));
        assert!(!ctx.has_scope("jobs:write"));
        assert!(!ctx.is_admin());
    }

    #[test]
    fn admin_role_scopes_are_just_the_wildcard() {
        let scopes = default_scopes_for_role(Role::Admin);
        assert_eq!(scopes, vec!["admin"]);
    }

    #[test]
    fn operator_role_can_write_jobs_but_not_admin() {
        let scopes = default_scopes_for_role(Role::Operator);
        let ctx = user_ctx(scopes, Some(Role::Operator));
        assert!(ctx.has_scope(Scope::JOBS_WRITE));
        assert!(ctx.has_scope(Scope::JOBS_TRIGGER));
        assert!(ctx.has_scope(Scope::SCHEDULES_WRITE));
        assert!(ctx.has_scope(Scope::DEAD_LETTERS_WRITE));
        // No admin powers.
        assert!(!ctx.has_scope(Scope::API_CLIENTS_ADMIN));
        assert!(!ctx.has_scope(Scope::API_KEYS_ADMIN));
        assert!(!ctx.has_scope(Scope::RUNNERS_WRITE));
        assert!(!ctx.is_admin());
    }

    #[test]
    fn viewer_role_is_read_only() {
        let scopes = default_scopes_for_role(Role::Viewer);
        let ctx = user_ctx(scopes, Some(Role::Viewer));
        assert!(ctx.has_scope(Scope::JOBS_READ));
        assert!(ctx.has_scope(Scope::EXECUTIONS_READ));
        assert!(ctx.has_scope(Scope::RUNNERS_READ));
        assert!(!ctx.has_scope(Scope::JOBS_WRITE));
        assert!(!ctx.has_scope(Scope::JOBS_TRIGGER));
        assert!(!ctx.has_scope(Scope::DEAD_LETTERS_WRITE));
    }
}
