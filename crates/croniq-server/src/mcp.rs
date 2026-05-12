//! HTTP MCP (Model Context Protocol) endpoint.
//!
//! Mounts an rmcp Streamable-HTTP service at `/mcp` behind the standard
//! JWT/API-key auth with two-tier scope gating:
//!
//! * `mcp:read` — required for any `/mcp` request (initialize, tools/list,
//!   observe tool calls). 401 without auth, 403 with auth but missing scope.
//! * `mcp:write` — additionally required for mutation tools listed in
//!   [`croniq_mcp::MUTATION_TOOL_NAMES`] (`enqueue_job`, `cancel_execution`,
//!   `job_trigger`, `update_job`, `dlq_retry`).
//!
//! `admin` is a wildcard — admin tokens pass both gates. Mutation gating is
//! enforced by a body-inspection middleware that parses the JSON-RPC request
//! and matches the tool name before forwarding to rmcp.

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
use croniq_mcp::{MUTATION_TOOL_NAMES, streamable_http_service};
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

/// Inspect the JSON-RPC body. If the request is `tools/call` for a tool listed
/// in [`croniq_mcp::MUTATION_TOOL_NAMES`], require `mcp:write` scope; otherwise
/// pass through unchanged.
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

    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && value.get("method").and_then(|v| v.as_str()) == Some("tools/call")
    {
        let tool_name = value
            .pointer("/params/name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if MUTATION_TOOL_NAMES.contains(&tool_name) {
            let ctx = parts
                .extensions
                .get::<CallerContext>()
                .ok_or(StatusCode::UNAUTHORIZED)?;
            if !ctx.has_scope(Scope::MCP_WRITE) {
                tracing::info!(
                    tool = tool_name,
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
