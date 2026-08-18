//! HTTP MCP (Model Context Protocol) endpoint.
//!
//! Mounts an rmcp Streamable-HTTP service at `/mcp` behind the standard
//! JWT/API-key auth with two-tier scope gating:
//!
//! * `mcp:read` — required for any `/mcp` request (initialize, tools/list,
//!   observe tool calls). 401 without auth, 403 with auth but missing scope.
//! * `mcp:write` — additionally required for every mutation tool listed in
//!   [`croniq_mcp::MUTATION_TOOL_NAMES`] (17 of them, covering job, schedule
//!   and calendar CRUD, queue mutations, runner removal and dead-letter
//!   operations), and for any `tools/call` naming a tool this build cannot
//!   classify.
//!
//! `admin` is a wildcard — admin tokens pass both gates. Mutation gating is
//! enforced by a body-inspection middleware that parses the JSON-RPC request
//! and matches the tool name before forwarding to rmcp.
//!
//! The gate denies what it cannot classify (issue #431). A body that is not
//! JSON, a JSON-RPC batch (top-level array — no single `method` to inspect,
//! and removed from the MCP revision rmcp 1.5 implements), or any other shape
//! that is neither a request nor a response is rejected outright rather than
//! forwarded unchecked.

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use std::collections::HashMap;

use croniq_config::compile::JobConfig;
use croniq_mcp::streamable_http_service;
use croniq_runner::AppState;
use croniq_scheduler::trigger::Trigger;

use crate::api::{ServerState, auth_middleware};
use crate::store::DynStore;

/// Hard cap on incoming JSON-RPC body size for the inspection middleware.
/// MCP requests are small; 64 KiB is generous and keeps a runaway body from
/// blocking the worker. Requests above the limit get a 413.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Build a router that nests the MCP Streamable-HTTP service at `/mcp` with
/// JWT/API-key auth and two-tier scope gating (`mcp:read` for access,
/// `mcp:write` for mutations).
///
/// `extra_allowed_hosts` is forwarded to rmcp's `Host`-header allowlist on
/// top of the loopback defaults (see [`croniq_mcp::streamable_http_service`]
/// and issue #114). `None` keeps loopback-only behaviour.
pub fn mcp_router(
    state: Arc<ServerState>,
    runner: Arc<AppState>,
    store: Option<DynStore>,
    jobs: Vec<JobConfig>,
    triggers: Option<Arc<tokio::sync::RwLock<HashMap<String, Trigger>>>>,
    extra_allowed_hosts: Option<Vec<String>>,
) -> Router {
    let svc = streamable_http_service(runner, store, jobs, triggers, extra_allowed_hosts);

    // route_layer applies in reverse order — `require_auth` runs first
    // (injects CallerContext), then `require_mcp_read`, then
    // `check_mutation_scope`.
    Router::new()
        .nest_service("/mcp", svc)
        .route_layer(middleware::from_fn(check_mutation_scope))
        .route_layer(middleware::from_fn(require_mcp_read))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware::require_auth,
        ))
}

/// Reject any `/mcp` request whose caller lacks `mcp:read` (or `admin`).
async fn require_mcp_read(req: Request, next: Next) -> Result<Response, StatusCode> {
    let ctx = req
        .extensions()
        .get::<CallerContext>()
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !ctx.has_scope(Scope::MCP_READ) {
        tracing::info!(
            caller = %ctx.caller_id,
            "MCP request rejected: mcp:read scope required"
        );
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(req).await)
}

/// What the mutation gate made of a request body.
#[derive(Debug, PartialEq)]
enum BodyClass {
    /// A `tools/call` that needs `mcp:write`. Carries the tool name for the
    /// rejection log line.
    NeedsWrite(String),
    /// A JSON-RPC request or notification that cannot mutate anything —
    /// `initialize`, `tools/list`, `ping`, a read-only `tools/call` — or a
    /// JSON-RPC *response* to a server-initiated request, which carries no
    /// `method` but also invokes nothing.
    Harmless,
    /// Unclassifiable: not JSON, a batch (top-level array), or an object that
    /// is neither a request nor a response. Denied.
    Unclassifiable(&'static str),
}

/// Classify a JSON-RPC body for the mutation gate.
///
/// Deny-by-default in two places (issue #431): a body whose shape carries no
/// single inspectable `method` is [`BodyClass::Unclassifiable`], and a
/// `tools/call` naming a tool that is in neither of croniq-mcp's classified
/// lists is treated as a mutation. The previous version passed both through
/// unchecked because it inspected only `if let Ok(obj) … method == "tools/call"`
/// and fell out of the `if` otherwise.
fn classify_body(bytes: &[u8]) -> BodyClass {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return BodyClass::Unclassifiable("body is not valid JSON");
    };
    let Some(obj) = value.as_object() else {
        // A top-level array is a JSON-RPC batch. There is no single `method`
        // to gate on, and the MCP revision rmcp 1.5 implements removed
        // batching, so this is refused rather than forwarded unchecked.
        return BodyClass::Unclassifiable("body is not a single JSON-RPC object");
    };

    let Some(method) = obj.get("method").and_then(|v| v.as_str()) else {
        // No `method`. A JSON-RPC response to a server-initiated request looks
        // like this and cannot invoke a tool, so it passes; anything else does
        // not.
        return if obj.contains_key("id")
            && (obj.contains_key("result") || obj.contains_key("error"))
        {
            BodyClass::Harmless
        } else {
            BodyClass::Unclassifiable("object carries neither `method` nor a JSON-RPC result")
        };
    };

    if method != "tools/call" {
        return BodyClass::Harmless;
    }

    let tool = value
        .pointer("/params/name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // `None` means croniq-mcp does not know this tool. Unknown means
    // unclassified, and unclassified means write-gated.
    match croniq_mcp::tool_requires_write(tool) {
        Some(false) => BodyClass::Harmless,
        _ => BodyClass::NeedsWrite(tool.to_string()),
    }
}

/// Inspect the JSON-RPC body and enforce `mcp:write` on anything that mutates
/// — or that the gate cannot prove is harmless. See [`classify_body`].
async fn check_mutation_scope(req: Request, next: Next) -> Result<Response, StatusCode> {
    // Only POSTs carry a JSON-RPC body. GET /mcp is the SSE upgrade for
    // server→client streaming and DELETE terminates a session — neither
    // can invoke a tool, so skip inspection.
    if req.method() != Method::POST {
        return Ok(next.run(req).await);
    }

    let (parts, body) = req.into_parts();
    let bytes = to_bytes(body, MAX_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;

    match classify_body(&bytes) {
        BodyClass::Harmless => {}
        BodyClass::Unclassifiable(why) => {
            tracing::info!(
                reason = why,
                "MCP request rejected: gate cannot classify the body"
            );
            return Err(StatusCode::BAD_REQUEST);
        }
        BodyClass::NeedsWrite(tool) => {
            let ctx = parts
                .extensions
                .get::<CallerContext>()
                .ok_or(StatusCode::UNAUTHORIZED)?;
            if !ctx.has_scope(Scope::MCP_WRITE) {
                tracing::info!(
                    tool = %tool,
                    caller = %ctx.caller_id,
                    "MCP mutation rejected: mcp:write scope required"
                );
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }

    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(s: &str) -> BodyClass {
        classify_body(s.as_bytes())
    }

    #[test]
    fn mutation_tool_call_needs_write() {
        assert_eq!(
            classify(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"delete_job"}}"#
            ),
            BodyClass::NeedsWrite("delete_job".into())
        );
    }

    #[test]
    fn read_tool_call_is_harmless() {
        assert_eq!(
            classify(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_jobs"}}"#
            ),
            BodyClass::Harmless
        );
    }

    #[test]
    fn unknown_tool_call_needs_write() {
        // Deny-by-default at the tool level: a name this build does not know
        // must not ride in on a read-only credential.
        assert_eq!(
            classify(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"wipe_all"}}"#
            ),
            BodyClass::NeedsWrite("wipe_all".into())
        );
        // …including a `tools/call` with no name at all.
        assert!(matches!(
            classify(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#),
            BodyClass::NeedsWrite(_)
        ));
    }

    #[test]
    fn non_tool_methods_pass() {
        assert_eq!(
            classify(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#),
            BodyClass::Harmless
        );
        assert_eq!(
            classify(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#),
            BodyClass::Harmless
        );
    }

    #[test]
    fn batch_is_denied() {
        // The regression from #431: a top-level array carries no `method`, so
        // the old `if let … && method == "tools/call"` fell through and the
        // whole batch reached rmcp without ever meeting the mcp:write check.
        assert!(matches!(
            classify(
                r#"[{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"delete_job"}}]"#
            ),
            BodyClass::Unclassifiable(_)
        ));
    }

    #[test]
    fn unparseable_body_is_denied() {
        assert!(matches!(
            classify("not json at all"),
            BodyClass::Unclassifiable(_)
        ));
        assert!(matches!(classify(""), BodyClass::Unclassifiable(_)));
    }

    #[test]
    fn methodless_object_is_denied_unless_it_is_a_response() {
        // A JSON-RPC response to a server-initiated request invokes nothing,
        // so denying it would break the protocol.
        assert_eq!(
            classify(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
            BodyClass::Harmless
        );
        assert_eq!(
            classify(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}}"#),
            BodyClass::Harmless
        );
        // Anything else with no method is not something the gate understands.
        assert!(matches!(
            classify(r#"{"jsonrpc":"2.0","params":{"name":"delete_job"}}"#),
            BodyClass::Unclassifiable(_)
        ));
        assert!(matches!(
            classify(r#""just-a-string""#),
            BodyClass::Unclassifiable(_)
        ));
    }
}
