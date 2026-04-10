//! Caller context and scope model.

use serde::{Deserialize, Serialize};

/// Type of authenticated caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallerType {
    ApiKey,
    User,
}

/// The authenticated caller's identity and permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerContext {
    pub caller_type: CallerType,
    pub caller_id: String,
    pub client_id: String,
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
}

/// Known scopes in the system.
pub struct Scope;

impl Scope {
    pub const JOBS_READ: &str = "jobs:read";
    pub const JOBS_WRITE: &str = "jobs:write";
    pub const JOBS_REGISTER: &str = "jobs:register";
    pub const JOBS_TRIGGER: &str = "jobs:trigger";
    pub const SCHEDULES_READ: &str = "schedules:read";
    pub const SCHEDULES_WRITE: &str = "schedules:write";
    pub const SCHEDULES_DEADLETTER: &str = "schedules:deadletter";
    pub const EXECUTIONS_READ: &str = "executions:read";
    pub const WORK_POLL: &str = "work:poll";
    pub const WORK_RENEW: &str = "work:renew";
    pub const WORK_ACK: &str = "work:ack";
    pub const WORK_EVENTS: &str = "work:events";
    pub const RUNNERS_READ: &str = "runners:read";
    pub const RUNNERS_WRITE: &str = "runners:write";
    pub const RUNNERS_HEARTBEAT: &str = "runners:heartbeat";
    pub const CALENDARS_READ: &str = "calendars:read";
    pub const CALENDARS_WRITE: &str = "calendars:write";
    pub const ADMIN: &str = "admin";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_has_all_scopes() {
        let ctx = CallerContext {
            caller_type: CallerType::User,
            caller_id: "user-1".into(),
            client_id: "client-1".into(),
            scopes: vec!["admin".into()],
        };
        assert!(ctx.has_scope("jobs:read"));
        assert!(ctx.has_scope("anything"));
    }

    #[test]
    fn specific_scope_check() {
        let ctx = CallerContext {
            caller_type: CallerType::ApiKey,
            caller_id: "key-1".into(),
            client_id: "client-1".into(),
            scopes: vec!["jobs:read".into(), "runners:read".into()],
        };
        assert!(ctx.has_scope("jobs:read"));
        assert!(!ctx.has_scope("jobs:write"));
    }
}
