//! The `croniq_refresh` cookie — refresh-token delivery for the dashboard
//! SPA (issue #454).
//!
//! ## Why a cookie at all
//!
//! The SPA used to keep both tokens in `localStorage`. Any XSS could read the
//! refresh token there and hold durable account access for its full 7-day TTL,
//! surviving reloads and outliving the access token that `token_generation`
//! (issue #431) made cheap to revoke. The refresh token is the one credential
//! the SPA never needs to *read*: it is only ever replayed to
//! `POST /v1/auth/refresh`. Moving it into an `HttpOnly` cookie takes it out
//! of JavaScript's reach entirely while the access token lives in memory only.
//!
//! ## Why this does not reintroduce CSRF
//!
//! Bearer-only auth is why Croniq has no CSRF surface: there is no ambient
//! credential for a cross-site request to ride on. The cookie keeps that
//! property because it is not ambient authority for the API — it is accepted
//! by exactly one endpoint, `/v1/auth/refresh`, which mints a token into a
//! response body that an attacker's page cannot read (CORS is origin-locked
//! since #429/#446). `SameSite=Strict` means a cross-site request never
//! carries it in the first place, and `Path=/v1/auth` keeps it off every
//! other route. Every other API call still authenticates with an
//! `Authorization` header.
//!
//! ## The invariant that makes it worth doing
//!
//! **A cookie-sourced refresh rotates the cookie and never returns
//! `refresh_token` in the body.** Without that rule the cookie buys nothing:
//! an XSS would simply `POST /v1/auth/refresh` — the browser attaches the
//! `HttpOnly` cookie unbidden — and read the fresh 7-day token out of the
//! response body. [`Delivery`] is what carries that rule through the
//! handlers; the delivery mode always mirrors where the presented token came
//! from.
//!
//! ## Why the server does not police same-origin here
//!
//! A cookie is only useful to a dashboard served from this server's own
//! origin, so refusing cookie delivery to a cross-origin caller looks
//! attractive — and an earlier draft of #454 did exactly that. It does not
//! work, in both directions:
//!
//! * **False positives.** The cookie is host-only for whatever origin the
//!   browser actually called, which is entirely independent of the `Host` the
//!   reverse proxy forwards. nginx's default `proxy_set_header Host
//!   $proxy_host` (and no `X-Forwarded-Host`) makes a perfectly working
//!   same-origin deployment look cross-origin from in here, so the check would
//!   refuse a login that the cookie would have served fine.
//! * **False negatives.** In a genuinely cross-origin deployment the
//!   configured `app_url` *is* the dashboard's origin — that is what
//!   [`crate::api::hardening`] allowlists for CORS — so an `Origin` comparison
//!   against it passes precisely in the case that should fail.
//!
//! The gate that does work sits in the UI build: `ui/vite.config.ts` refuses
//! to produce a `VITE_API_URL` (cross-origin) bundle unless the operator
//! acknowledges the weaker `localStorage` mode, and such a bundle never asks
//! for cookie delivery. Asking for a cookie from a cross-origin page is not a
//! security problem in any case — the token goes into a cookie that page can
//! never read *or* send, so its refreshes 401 and it falls back to signing in
//! again. It is a misconfiguration, and it belongs to whoever hand-built the
//! bundle.

use axum::http::{HeaderMap, HeaderValue, header};

/// Cookie name.
pub const COOKIE_NAME: &str = "croniq_refresh";

/// Path scope. Narrow enough that the cookie never rides along on a job,
/// runner or execution call, wide enough that `/v1/auth/refresh` *and*
/// `/v1/auth/logout` both see it — a `Path=/v1/auth/refresh` scope would
/// leave logout unable to clear the very cookie it is revoking.
pub const COOKIE_PATH: &str = "/v1/auth";

/// How a handler hands the refresh token back to this caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// In the JSON response body, as every release before this one did.
    /// Non-browser clients (the CLI, curl, scripted flows) and dashboards
    /// built cross-origin with `VITE_API_URL` stay on this path.
    Body,
    /// In a `Set-Cookie` header, with the body field omitted entirely.
    Cookie,
}

/// Whether to stamp `Secure` on the cookie.
///
/// Set only on positive evidence of HTTPS, because the failure modes are wildly
/// asymmetric: a missing `Secure` on an HTTPS deployment costs a hardening flag
/// on a connection that is encrypted anyway, while a spurious `Secure` on a
/// plain-HTTP deployment means the browser never sends the cookie back and
/// nobody can stay signed in.
///
/// Three signals, in order of trustworthiness. `Origin` comes from the browser
/// itself and reports the page's real scheme; `X-Forwarded-Proto` is the
/// proxy's account of it (same source `resolve_link_base` trusts); the
/// configured public URL is the operator's own statement of how the dashboard
/// is reached.
pub fn is_secure_request(headers: &HeaderMap, app_base_url: Option<&str>) -> bool {
    if let Some(origin) = first_header(headers, "origin")
        && let Some((scheme, _)) = split_origin(&origin)
    {
        if scheme == "https" {
            return true;
        }
        // An explicit `http` Origin is the browser telling us the page is not
        // on HTTPS. Believe it over anything a proxy or config claims.
        return false;
    }
    if let Some(proto) = first_header(headers, "x-forwarded-proto") {
        return proto == "https";
    }
    app_base_url
        .map(|u| u.trim_start().to_ascii_lowercase().starts_with("https://"))
        .unwrap_or(false)
}

/// Split an origin-ish URL into `(scheme, host)`, where host keeps any
/// explicit port. Returns `None` for anything that is not an absolute
/// http/https URL — including `Origin: null`.
fn split_origin(raw: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = raw.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|h| !h.is_empty())?;
    Some((scheme, host))
}

/// First value of a possibly-repeated, possibly comma-joined header, lowercased.
/// Several proxies in a chain append rather than replace; the first value is the
/// outermost (client-facing) hop. Mirrors `api::resolve_link_base`.
fn first_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|raw| {
            raw.split(',')
                .next()
                .unwrap_or(raw)
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|s| !s.is_empty())
}

/// Read the refresh token out of the request's `Cookie` header(s).
///
/// Hand-rolled rather than pulled from a cookie crate because the value space
/// is entirely ours: refresh tokens are hyphenated UUIDv4s
/// (`croniq_auth::jwt::issue_token_pair`), so there is no quoting, escaping or
/// percent-encoding to get subtly wrong.
pub fn read(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|raw| raw.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// `Set-Cookie` value that installs the refresh token.
pub fn set(value: &str, ttl_secs: i64, secure: bool) -> Option<HeaderValue> {
    render(&format!("{COOKIE_NAME}={value}"), ttl_secs.max(0), secure)
}

/// `Set-Cookie` value that removes it. `Max-Age=0` plus an empty value, so a
/// browser that ignores one of the two still drops the cookie.
pub fn clear(secure: bool) -> Option<HeaderValue> {
    render(&format!("{COOKIE_NAME}="), 0, secure)
}

fn render(name_value: &str, max_age: i64, secure: bool) -> Option<HeaderValue> {
    let mut s =
        format!("{name_value}; Path={COOKIE_PATH}; HttpOnly; SameSite=Strict; Max-Age={max_age}");
    if secure {
        s.push_str("; Secure");
    }
    HeaderValue::from_str(&s).ok()
}

/// A `HeaderMap` carrying one `Set-Cookie`, ready to be returned from a
/// handler as `(headers, Json(body))`.
pub fn header_map(value: Option<HeaderValue>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(v) = value {
        headers.insert(header::SET_COOKIE, v);
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn set_cookie_carries_the_hardening_attributes() {
        let v = set("11111111-2222-3333-4444-555555555555", 604800, true).unwrap();
        assert_eq!(
            v.to_str().unwrap(),
            "croniq_refresh=11111111-2222-3333-4444-555555555555; Path=/v1/auth; HttpOnly; \
             SameSite=Strict; Max-Age=604800; Secure"
        );
    }

    #[test]
    fn clear_cookie_expires_immediately_and_carries_no_value() {
        let s = clear(false).unwrap();
        let s = s.to_str().unwrap();
        assert!(s.starts_with("croniq_refresh=;"), "{s}");
        assert!(s.contains("Max-Age=0"), "{s}");
        assert!(!s.contains("Secure"), "{s}");
    }

    #[test]
    fn negative_ttl_never_becomes_a_negative_max_age() {
        let s = set("t", -5, false).unwrap();
        assert!(s.to_str().unwrap().contains("Max-Age=0"));
    }

    #[test]
    fn reads_the_cookie_from_a_crowded_header() {
        let h = headers(&[(
            "cookie",
            "croniq_theme=dark; croniq_refresh=abc-123; other=x",
        )]);
        assert_eq!(read(&h).as_deref(), Some("abc-123"));
    }

    #[test]
    fn reads_the_cookie_from_a_second_cookie_header() {
        let h = headers(&[
            ("cookie", "croniq_theme=dark"),
            ("cookie", "croniq_refresh=v"),
        ]);
        assert_eq!(read(&h).as_deref(), Some("v"));
    }

    #[test]
    fn absent_and_empty_cookies_read_as_none() {
        assert_eq!(read(&headers(&[])), None);
        assert_eq!(read(&headers(&[("cookie", "croniq_refresh=")])), None);
        assert_eq!(read(&headers(&[("cookie", "croniq_refreshx=v")])), None);
        // A prefix match must not win: `xcroniq_refresh` is a different name.
        assert_eq!(read(&headers(&[("cookie", "xcroniq_refresh=v")])), None);
    }

    #[test]
    fn https_origin_means_secure() {
        assert!(is_secure_request(
            &headers(&[("origin", "https://cron.example.com")]),
            None
        ));
    }

    #[test]
    fn http_origin_beats_a_proxy_or_config_claiming_https() {
        // The browser is reporting the page's real scheme. A `Secure` cookie
        // would never come back to an http page, so believe the browser and
        // lock nobody out.
        assert!(!is_secure_request(
            &headers(&[
                ("origin", "http://localhost:5173"),
                ("x-forwarded-proto", "https")
            ]),
            Some("https://cron.example.com")
        ));
    }

    #[test]
    fn forwarded_proto_decides_when_there_is_no_origin() {
        // Top-level navigation (the OIDC callback) carries no Origin.
        assert!(is_secure_request(
            &headers(&[("x-forwarded-proto", "https")]),
            None
        ));
        assert!(!is_secure_request(
            &headers(&[("x-forwarded-proto", "http")]),
            Some("https://cron.example.com")
        ));
    }

    #[test]
    fn configured_app_url_is_the_last_resort() {
        assert!(is_secure_request(
            &headers(&[]),
            Some("  https://cron.example.com/ ")
        ));
        assert!(!is_secure_request(&headers(&[]), Some("http://box:4000")));
        assert!(!is_secure_request(&headers(&[]), None));
    }

    #[test]
    fn opaque_and_garbage_origins_fall_through_to_the_other_signals() {
        // `Origin: null` and unparseable values carry no scheme information,
        // so they must not be read as "http" and veto the proxy's account.
        assert!(is_secure_request(
            &headers(&[("origin", "null"), ("x-forwarded-proto", "https")]),
            None
        ));
        assert!(is_secure_request(
            &headers(&[("origin", "not a url")]),
            Some("https://cron.example.com")
        ));
    }
}
