//! System endpoints — operational metadata for admins.
//!
//!   GET /v1/system/diagnostics   admin — configuration health report

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;

use super::ServerState;
use crate::api::auth_middleware::require_scope;
use crate::diagnostics::{self, Diagnostic, DiagnosticsInput, RuntimeFacts};

/// `GET /v1/system/diagnostics` — admin-only configuration health report.
///
/// Read-only. Reports posture (e.g. "no SMTP configured", "app URL not
/// pinned"), never secrets. Backs the Settings → System UI panel and mirrors
/// the boot-time warnings / `croniq-server doctor` output.
pub async fn handle_diagnostics(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<Diagnostic>>, StatusCode> {
    require_scope(&ctx, Scope::ADMIN)?;
    let input = DiagnosticsInput::from_runtime(RuntimeFacts {
        app_base_url_configured: state.app_base_url.is_some(),
        email_delivery_active: state.email_sender.delivers(),
        totp_enforced: state.require_totp,
        retention_configured: state.retention_configured,
        store: state.store.as_ref(),
        jwt_secret: state.jwt_config.as_ref().map(|c| c.secret.as_str()),
    });
    Ok(Json(diagnostics::run_diagnostics(&input)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use croniq_auth::{AuthMethod, CallerType};
    use croniq_runner::AppState;
    use tokio::sync::mpsc;

    fn state() -> Arc<ServerState> {
        // Fresh state: app_base_url = None and the default NoopSender.
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::new(AppState::new(), tx)
    }

    fn ctx(scopes: &[&str]) -> CallerContext {
        CallerContext {
            caller_type: CallerType::User,
            caller_id: "u".into(),
            client_id: "u".into(),
            user_id: Some("u".into()),
            role: None,
            auth_method: AuthMethod::Password,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            token_generation: None,
        }
    }

    #[tokio::test]
    async fn non_admin_is_forbidden() {
        let res = handle_diagnostics(State(state()), Extension(ctx(&["jobs:read"]))).await;
        assert_eq!(res.unwrap_err(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_gets_findings_including_app_url() {
        let Json(findings) = handle_diagnostics(State(state()), Extension(ctx(&["admin"])))
            .await
            .expect("admin is allowed");
        // app_base_url is None on a fresh state, so this finding is present
        // regardless of ambient SMTP env vars.
        assert!(findings.iter().any(|d| d.id == "links.app_url"));
    }
}
