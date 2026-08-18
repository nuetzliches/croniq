//! HTTP response hardening (issue #429): explicit CORS + security headers.
//!
//! Two concerns live here:
//!
//! * **CORS** — an explicit allowlist replaces the previous
//!   `CorsLayer::permissive()` (which emitted `Access-Control-Allow-Origin: *`
//!   with any method and any header on every route). The only browser origin
//!   that legitimately calls this API cross-origin is a dashboard served from
//!   the operator-configured public app URL (`server { app_url … }` /
//!   `CRONIQ_APP_URL`), so exactly that origin is allowed — with the methods
//!   and headers the dashboard actually uses and **without**
//!   `Allow-Credentials`. When no app URL is configured, the SPA is served
//!   same-origin by croniq-server itself (the standard setup, including the
//!   official Docker image) and needs no CORS at all, so no CORS headers are
//!   emitted. `server.app_url` is a boot-only setting (see
//!   `reload::BootOnlySettings`), which keeps building the layer once at
//!   router construction consistent with the rest of its semantics.
//!
//! * **Security headers** — `X-Content-Type-Options`, `X-Frame-Options`,
//!   `Referrer-Policy`, and a `Content-Security-Policy` on every response.
//!   The CSP is scoped to what the Vite production bundle actually needs;
//!   see [`CONTENT_SECURITY_POLICY`] for the per-directive rationale.
//!   `Strict-Transport-Security` is deliberately **not** set: croniq-server
//!   does not terminate TLS itself, and an HSTS header baked in here would
//!   either be ignored (plain HTTP) or belong to the reverse proxy that does
//!   terminate TLS. See `docs/operations.md` → *HTTP hardening*.

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

/// Content-Security-Policy for everything this server serves — most
/// importantly the dashboard SPA. Since #454 the SPA holds only the one-hour
/// access token in JS (the refresh token is an `HttpOnly` cookie; see
/// [`crate::api::refresh_cookie`]), so an XSS can no longer walk away with
/// durable account access — but it can still act as the user for as long as
/// the page is open, which makes the CSP the main thing limiting the blast
/// radius of any future XSS.
///
/// Checked against the actual Vite production build (`ui/dist`):
///
/// * `script-src 'self' 'wasm-unsafe-eval'` — `index.html` contains no
///   inline scripts (only external module scripts + modulepreload links),
///   but the schedule builder lazily loads the `croniq_config_wasm`
///   wasm-bindgen module, and CSP gates `WebAssembly` compilation behind
///   `'wasm-unsafe-eval'` (which, unlike `'unsafe-eval'`, does not re-enable
///   JS `eval`).
/// * `style-src 'self' 'unsafe-inline'` — the bundle extracts all CSS into
///   files, but React components use `style={{…}}` attributes throughout,
///   and style *attributes* require `'unsafe-inline'`.
/// * `img-src 'self' data:` — icons ship as files under `/icons/`; `data:`
///   is a low-risk allowance for data-URI images. The TOTP QR code is
///   rendered as inline SVG markup (a DOM subtree, not a resource load) and
///   needs no directive.
/// * `connect-src 'self'` — the SPA defaults to same-origin `fetch` (see
///   `ui/src/api/base.ts`), which also covers the SSE streams and the
///   wasm-bindgen loader fetching its `.wasm` next to the JS.
/// * `frame-ancestors 'none'` — no embedding (clickjacking); the modern
///   equivalent of `X-Frame-Options: DENY`, which is also set for older
///   proxies/browsers.
/// * `object-src 'none'`, `base-uri 'self'`, `form-action 'self'` — nothing
///   uses plugins, `<base>`, or HTML form submission.
///
/// Note: a dashboard built with `VITE_API_URL` pointing at a different
/// origin is served by *that* origin's web server, whose CSP (not this one)
/// applies to it.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; \
     script-src 'self' 'wasm-unsafe-eval'; \
     style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; \
     connect-src 'self'; \
     object-src 'none'; \
     base-uri 'self'; \
     form-action 'self'; \
     frame-ancestors 'none'";

/// Build the CORS layer for the API router.
///
/// Returns `None` when no public app URL is configured (or when it cannot be
/// parsed into an http/https origin): same-origin requests need no CORS
/// headers, and emitting none means browsers enforce the same-origin policy
/// unaided. When configured, exactly the app URL's origin is allowed, with
/// the methods and headers the dashboard uses. `Allow-Credentials` is never
/// set: cross-origin auth is Bearer-header only, and the refresh cookie (#454)
/// is `SameSite=Strict`, so it never rides a cross-origin request in any case.
pub fn cors_layer(app_base_url: Option<&str>) -> Option<CorsLayer> {
    let origin = app_base_url.and_then(allowed_origin)?;
    Some(
        CorsLayer::new()
            // `list` (unlike `exact`) compares against the request's Origin
            // and emits `Allow-Origin` only on a match — a non-matching
            // origin gets no CORS headers at all instead of a constant echo
            // of the allowed one.
            .allow_origin(AllowOrigin::list([origin]))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
            // Cache preflight results for an hour so the dashboard does not
            // pay one OPTIONS round-trip per mutating call.
            .max_age(std::time::Duration::from_secs(3600)),
    )
}

/// Reduce a configured app URL to the exact `Origin` header value a browser
/// would send for pages served from it: lowercased scheme + host, explicit
/// port only when it is not the scheme default, no path/query/fragment.
///
/// Returns `None` for values that do not parse as an absolute http/https
/// URL — a misconfigured `app_url` then fails closed (no CORS) rather than
/// allowlisting garbage.
fn allowed_origin(app_url: &str) -> Option<HeaderValue> {
    let url = url::Url::parse(app_url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    // `Url::port()` is `None` when the port is unspecified *or* the scheme
    // default (the parser normalizes `https://x:443` to portless), which is
    // exactly how browsers serialize the Origin header.
    let host = url.host_str()?;
    let origin = match url.port() {
        Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
        None => format!("{}://{}", url.scheme(), host),
    };
    HeaderValue::from_str(&origin).ok()
}

/// Layer the security headers onto a fully assembled router.
///
/// Applied twice on purpose: inside `server_router()` (so every embedding of
/// the API router is hardened, tests included) and again in `main.rs` over
/// the final app (so the SPA `ServeDir` fallback and the `/mcp` router —
/// both mounted *after* `server_router()` returns, and therefore outside any
/// layer it applies — are covered too). `if_not_present` semantics make the
/// double application idempotent and let a specific handler deliberately
/// override any of these headers.
pub fn apply_security_headers(router: Router) -> Router {
    router
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(s: &str) -> Option<String> {
        allowed_origin(s).map(|v| v.to_str().unwrap().to_string())
    }

    #[test]
    fn plain_origin_passes_through() {
        assert_eq!(
            origin("https://app.example.com"),
            Some("https://app.example.com".into())
        );
    }

    #[test]
    fn path_query_and_trailing_slash_are_stripped() {
        assert_eq!(
            origin("https://app.example.com:8443/dash/?x=1"),
            Some("https://app.example.com:8443".into())
        );
    }

    #[test]
    fn scheme_and_host_are_lowercased_and_default_port_dropped() {
        // Browsers serialize the Origin header lowercased and without the
        // scheme-default port — the allowlisted value must match exactly.
        assert_eq!(
            origin("HTTPS://App.Example.com:443/"),
            Some("https://app.example.com".into())
        );
        assert_eq!(
            origin("http://app.example.com:80"),
            Some("http://app.example.com".into())
        );
    }

    #[test]
    fn non_http_and_garbage_fail_closed() {
        assert_eq!(origin("ftp://files.example.com"), None);
        assert_eq!(origin("not a url"), None);
        assert_eq!(origin(""), None);
    }

    #[test]
    fn no_app_url_means_no_cors_layer() {
        assert!(cors_layer(None).is_none());
        assert!(cors_layer(Some("   ")).is_none());
        assert!(cors_layer(Some("https://app.example.com")).is_some());
    }
}
