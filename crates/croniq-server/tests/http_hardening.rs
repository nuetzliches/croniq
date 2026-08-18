//! Integration tests for issue #429 HTTP response hardening.
//!
//! Covers:
//!   * security headers (nosniff, frame options, referrer policy, CSP) on
//!     API responses and on the SPA fallback
//!   * explicit CORS: the configured `app_url` origin is echoed, any other
//!     origin is not, and no CORS headers exist without a configured app URL
//!   * preflight requests succeed for the methods/headers the dashboard uses
//!     without authentication
//!   * `Access-Control-Allow-Credentials` is never set

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use croniq_runner::AppState;
use croniq_server::api::{ServerState, hardening, server_router};
use tokio::sync::mpsc;
use tower::util::ServiceExt;

/// Build the app the way `main.rs` assembles it: `server_router` over a
/// state with the given `app_base_url`, an optional SPA `ServeDir` fallback,
/// and the outer security-header application covering that fallback.
fn app_with(app_url: Option<&str>, ui_dir: Option<&std::path::Path>) -> axum::Router {
    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut state = ServerState::with_timeout(runner, tx, Duration::from_millis(50));
    Arc::get_mut(&mut state).unwrap().app_base_url = app_url.map(str::to_string);
    let mut app = server_router(state);
    if let Some(dir) = ui_dir {
        use tower_http::services::{ServeDir, ServeFile};
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(&index)));
        // Mirror main.rs: the outer application is what covers the fallback.
        app = hardening::apply_security_headers(app);
    }
    app
}

async fn send(app: axum::Router, req: Request<Body>) -> axum::response::Response {
    app.oneshot(req).await.unwrap()
}

fn assert_security_headers(resp: &axum::response::Response) {
    let h = resp.headers();
    assert_eq!(
        h.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff",
        "X-Content-Type-Options missing or wrong"
    );
    assert_eq!(
        h.get(header::X_FRAME_OPTIONS).unwrap(),
        "DENY",
        "X-Frame-Options missing or wrong"
    );
    assert_eq!(
        h.get(header::REFERRER_POLICY).unwrap(),
        "no-referrer",
        "Referrer-Policy missing or wrong"
    );
    let csp = h
        .get(header::CONTENT_SECURITY_POLICY)
        .expect("Content-Security-Policy missing")
        .to_str()
        .unwrap();
    assert_eq!(csp, hardening::CONTENT_SECURITY_POLICY);
    // The load-bearing directives, asserted individually so a future edit
    // of the constant cannot silently drop one.
    for needle in [
        "default-src 'self'",
        "script-src 'self' 'wasm-unsafe-eval'",
        "style-src 'self' 'unsafe-inline'",
        "frame-ancestors 'none'",
        "object-src 'none'",
        "base-uri 'self'",
    ] {
        assert!(csp.contains(needle), "CSP lost directive: {needle}");
    }
    // Exactly one value per header — the layer is applied both inside
    // server_router() and over the final app, and must not duplicate.
    assert_eq!(
        h.get_all(header::CONTENT_SECURITY_POLICY).iter().count(),
        1,
        "CSP header duplicated"
    );
}

// ─── Security headers ────────────────────────────────────────────────────────

#[tokio::test]
async fn security_headers_present_on_api_response() {
    let app = app_with(None, None);
    let resp = send(
        app,
        Request::builder()
            .uri("/version")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_security_headers(&resp);
}

#[tokio::test]
async fn security_headers_present_on_spa_fallback() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>Croniq</title>",
    )
    .unwrap();

    let app = app_with(None, Some(dir.path()));

    // A client-side route: no file exists, ServeFile serves index.html.
    let resp = send(
        app.clone(),
        Request::builder()
            .uri("/executions")
            .header(header::ACCEPT, "text/html")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_security_headers(&resp);

    // The index file itself, served by ServeDir.
    let resp = send(
        app,
        Request::builder()
            .uri("/index.html")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_security_headers(&resp);
}

// ─── CORS ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cors_allows_the_configured_app_url_origin() {
    // Path + trailing slash in the configured URL must not leak into the
    // allowlisted origin.
    let app = app_with(Some("https://app.example.com/dash/"), None);
    let resp = send(
        app,
        Request::builder()
            .uri("/version")
            .header(header::ORIGIN, "https://app.example.com")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("configured origin must be allowed"),
        "https://app.example.com"
    );
    // Bearer-header auth only — credentials must never be allowed, so a
    // future cookie could not be combined with cross-origin reads.
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .is_none(),
        "Allow-Credentials must not be set"
    );
}

#[tokio::test]
async fn cors_does_not_echo_a_disallowed_origin() {
    let app = app_with(Some("https://app.example.com"), None);
    let resp = send(
        app,
        Request::builder()
            .uri("/version")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    // The request itself still succeeds (CORS is a browser read gate, not
    // auth) — but no Allow-Origin means the browser blocks the read.
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "a disallowed origin must not be echoed"
    );
}

#[tokio::test]
async fn cors_is_absent_without_a_configured_app_url() {
    // Same-origin deployment (the default): no app_url, no CORS headers for
    // anyone — browsers enforce the same-origin policy unaided.
    let app = app_with(None, None);
    let resp = send(
        app,
        Request::builder()
            .uri("/version")
            .header(header::ORIGIN, "https://app.example.com")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "no configured app_url means no Allow-Origin at all"
    );
}

#[tokio::test]
async fn cors_preflight_covers_dashboard_methods_and_headers() {
    // Preflights are answered by the CORS layer itself, before the auth
    // middleware — an unauthenticated OPTIONS must succeed or the dashboard
    // could never make a cross-origin authenticated call.
    let app = app_with(Some("https://app.example.com"), None);
    let resp = send(
        app,
        Request::builder()
            .method("OPTIONS")
            .uri("/v1/jobs")
            .header(header::ORIGIN, "https://app.example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PUT")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization,content-type",
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "https://app.example.com"
    );
    let methods = resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .unwrap()
        .to_str()
        .unwrap()
        .to_ascii_uppercase();
    for m in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
        assert!(methods.contains(m), "method {m} missing from {methods}");
    }
    let headers = resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
        .unwrap()
        .to_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(headers.contains("authorization"), "got: {headers}");
    assert!(headers.contains("content-type"), "got: {headers}");
}

#[tokio::test]
async fn cors_preflight_from_a_disallowed_origin_is_not_approved() {
    let app = app_with(Some("https://app.example.com"), None);
    let resp = send(
        app,
        Request::builder()
            .method("OPTIONS")
            .uri("/v1/jobs")
            .header(header::ORIGIN, "https://evil.example")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PUT")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "preflight must not approve a disallowed origin"
    );
}
