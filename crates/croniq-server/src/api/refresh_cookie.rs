//! The `croniq_refresh` cookie — refresh-token delivery for the dashboard
//! SPA (issue #454). `CRONIQ_REFRESH_COOKIE` renames it (issue #880, see
//! [`COOKIE_NAME_ENV`]).
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
//! The gate that does work sits in the UI build, and since the Vue cutover it
//! is stronger than a check: `ui/vite.config.ts` has no `VITE_API_URL` at
//! all, so the dashboard cannot be built cross-origin and therefore never asks
//! for cookie delivery from a page that could not use it. (The React tree that
//! preceded it did have the flag, behind an `assertTokenStorageAcknowledged`
//! build guard.) Asking for a cookie from a cross-origin page is not a
//! security problem in any case — the token goes into a cookie that page can
//! never read *or* send, so its refreshes 401 and it falls back to signing in
//! again. It is a misconfiguration, and it belongs to whoever hand-built the
//! bundle.

use axum::http::{HeaderMap, HeaderValue, header};

/// Cookie name unless [`COOKIE_NAME_ENV`] names another one.
pub const DEFAULT_COOKIE_NAME: &str = "croniq_refresh";

/// Environment variable that renames the cookie (issue #880).
///
/// Browsers keep cookies per host, not per port, so every croniq server on
/// `localhost` — a devstack lane beside the main tree, a second checkout,
/// staging beside production on one box — shares the one `croniq_refresh`
/// slot. A login on one replaces the other's cookie, whose next refresh then
/// presents a token its own store has never seen and signs the user out.
/// A distinct name per server keeps the two sessions apart.
pub const COOKIE_NAME_ENV: &str = "CRONIQ_REFRESH_COOKIE";

/// The cookie name to use, from the raw value of [`COOKIE_NAME_ENV`]. Unset or
/// blank means [`DEFAULT_COOKIE_NAME`]; surrounding whitespace is trimmed.
///
/// Anything else must be a usable cookie name, or the server refuses to start:
/// falling back to the default would quietly bring back the collision the
/// operator set the variable to avoid.
pub fn resolve_name(raw: Option<&str>) -> Result<String, String> {
    let Some(name) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_COOKIE_NAME.to_string());
    };
    // RFC 6265 makes a cookie name an RFC 9110 `token`: visible ASCII minus
    // the separators. That also keeps `=` and `;` out, which `read` relies on.
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || TOKEN_PUNCTUATION.contains(*c)))
    {
        return Err(format!(
            "{COOKIE_NAME_ENV}={name:?}: {c:?} is not allowed in a cookie name \
             (letters, digits and {TOKEN_PUNCTUATION} are)"
        ));
    }
    // Browsers accept these prefixes only on a `Secure` cookie, and `__Host-`
    // only with `Path=/`. Ours is `Path=/v1/auth` and `Secure` only over HTTPS,
    // so the browser would drop it and nobody would stay signed in.
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("__host-") || lower.starts_with("__secure-") {
        return Err(format!(
            "{COOKIE_NAME_ENV}={name:?}: browsers refuse a __Host- or __Secure- cookie \
             unless it is Secure (and, for __Host-, on Path=/), which croniq's \
             refresh cookie cannot promise"
        ));
    }
    Ok(name.to_string())
}

/// The cookie name this process should use: [`COOKIE_NAME_ENV`] read from the
/// environment and run through [`resolve_name`].
///
/// Reads the raw `OsString` on purpose. `std::env::var(..).ok()` turns a value
/// that is not valid Unicode into `None`, which [`resolve_name`] would answer
/// with the shared default — the silent fallback it exists to rule out.
pub fn name_from_env() -> Result<String, String> {
    resolve_os_name(std::env::var_os(COOKIE_NAME_ENV).as_deref())
}

fn resolve_os_name(raw: Option<&std::ffi::OsStr>) -> Result<String, String> {
    match raw.map(|v| v.to_str().ok_or(v)) {
        None => resolve_name(None),
        Some(Ok(s)) => resolve_name(Some(s)),
        Some(Err(v)) => Err(format!(
            "{COOKIE_NAME_ENV}={v:?} is not valid Unicode, so it cannot be a cookie name"
        )),
    }
}

/// The punctuation an RFC 9110 `token` allows besides letters and digits.
const TOKEN_PUNCTUATION: &str = "!#$%&'*+-.^_`|~";

/// Path scope. Narrow enough that the cookie never rides along on a job,
/// runner or execution call, wide enough that `/v1/auth/refresh` *and*
/// `/v1/auth/logout` both see it — a `Path=/v1/auth/refresh` scope would
/// leave logout unable to clear the very cookie it is revoking.
pub const COOKIE_PATH: &str = "/v1/auth";

/// How a handler hands the refresh token back to this caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// In the JSON response body, as every release before this one did.
    /// Non-browser clients (the CLI, curl, scripted flows, the SDKs) and any
    /// cross-origin browser app stay on this path.
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
pub fn read(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|raw| raw.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(n, _)| *n == name)
        .map(|(_, value)| value.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// `Set-Cookie` value that installs the refresh token under `name`.
pub fn set(name: &str, value: &str, ttl_secs: i64, secure: bool) -> Option<HeaderValue> {
    render(&format!("{name}={value}"), ttl_secs.max(0), secure)
}

/// `Set-Cookie` value that removes it. `Max-Age=0` plus an empty value, so a
/// browser that ignores one of the two still drops the cookie.
pub fn clear(name: &str, secure: bool) -> Option<HeaderValue> {
    render(&format!("{name}="), 0, secure)
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
        let v = set(
            DEFAULT_COOKIE_NAME,
            "11111111-2222-3333-4444-555555555555",
            604800,
            true,
        )
        .unwrap();
        assert_eq!(
            v.to_str().unwrap(),
            "croniq_refresh=11111111-2222-3333-4444-555555555555; Path=/v1/auth; HttpOnly; \
             SameSite=Strict; Max-Age=604800; Secure"
        );
    }

    #[test]
    fn clear_cookie_expires_immediately_and_carries_no_value() {
        let s = clear(DEFAULT_COOKIE_NAME, false).unwrap();
        let s = s.to_str().unwrap();
        assert!(s.starts_with("croniq_refresh=;"), "{s}");
        assert!(s.contains("Max-Age=0"), "{s}");
        assert!(!s.contains("Secure"), "{s}");
    }

    #[test]
    fn negative_ttl_never_becomes_a_negative_max_age() {
        let s = set(DEFAULT_COOKIE_NAME, "t", -5, false).unwrap();
        assert!(s.to_str().unwrap().contains("Max-Age=0"));
    }

    #[test]
    fn reads_the_cookie_from_a_crowded_header() {
        let h = headers(&[(
            "cookie",
            "croniq_theme=dark; croniq_refresh=abc-123; other=x",
        )]);
        assert_eq!(read(&h, DEFAULT_COOKIE_NAME).as_deref(), Some("abc-123"));
    }

    #[test]
    fn reads_the_cookie_from_a_second_cookie_header() {
        let h = headers(&[
            ("cookie", "croniq_theme=dark"),
            ("cookie", "croniq_refresh=v"),
        ]);
        assert_eq!(read(&h, DEFAULT_COOKIE_NAME).as_deref(), Some("v"));
    }

    #[test]
    fn absent_and_empty_cookies_read_as_none() {
        assert_eq!(read(&headers(&[]), DEFAULT_COOKIE_NAME), None);
        assert_eq!(
            read(
                &headers(&[("cookie", "croniq_refresh=")]),
                DEFAULT_COOKIE_NAME
            ),
            None
        );
        assert_eq!(
            read(
                &headers(&[("cookie", "croniq_refreshx=v")]),
                DEFAULT_COOKIE_NAME
            ),
            None
        );
        // A prefix match must not win: `xcroniq_refresh` is a different name.
        assert_eq!(
            read(
                &headers(&[("cookie", "xcroniq_refresh=v")]),
                DEFAULT_COOKIE_NAME
            ),
            None
        );
    }

    #[test]
    fn a_renamed_cookie_is_read_beside_the_default_one() {
        // The case #880 is about: a browser on `localhost` holds the main
        // tree's cookie and a lane's, and sends both to every port.
        let h = headers(&[("cookie", "croniq_refresh=main; croniq_refresh_lane-a=lane")]);
        assert_eq!(read(&h, "croniq_refresh_lane-a").as_deref(), Some("lane"));
        assert_eq!(read(&h, DEFAULT_COOKIE_NAME).as_deref(), Some("main"));
        let s = set("croniq_refresh_lane-a", "t", 60, false).unwrap();
        assert!(s.to_str().unwrap().starts_with("croniq_refresh_lane-a=t;"));
        let s = clear("croniq_refresh_lane-a", false).unwrap();
        assert!(s.to_str().unwrap().starts_with("croniq_refresh_lane-a=;"));
    }

    #[test]
    fn unset_or_blank_name_is_the_default() {
        assert_eq!(resolve_name(None).unwrap(), DEFAULT_COOKIE_NAME);
        assert_eq!(resolve_name(Some("")).unwrap(), DEFAULT_COOKIE_NAME);
        assert_eq!(resolve_name(Some("  ")).unwrap(), DEFAULT_COOKIE_NAME);
    }

    #[test]
    fn a_devstack_lane_name_is_accepted() {
        // `croniq_refresh{lane_suffix}`: a lane name is lowercase letters,
        // digits and `-`.
        assert_eq!(
            resolve_name(Some(" croniq_refresh_feat-880 ")).unwrap(),
            "croniq_refresh_feat-880"
        );
        assert_eq!(
            resolve_name(Some("a!#$%&'*+-.^_`|~z")).unwrap(),
            "a!#$%&'*+-.^_`|~z"
        );
    }

    #[test]
    fn a_name_with_a_separator_is_refused() {
        for bad in [
            "a=b", "a;b", "a b", "a,b", "a\"b", "a/b", "a(b)", "a@b", "a:b", "café",
        ] {
            let err = resolve_name(Some(bad)).unwrap_err();
            assert!(err.contains(COOKIE_NAME_ENV), "{bad}: {err}");
        }
    }

    #[test]
    fn host_and_secure_prefixes_are_refused() {
        for bad in [
            "__Host-croniq",
            "__Secure-croniq",
            "__host-croniq",
            "__SECURE-x",
        ] {
            let err = resolve_name(Some(bad)).unwrap_err();
            assert!(err.contains("__Host-"), "{bad}: {err}");
        }
        // Only the exact prefixes: two underscores alone are fine.
        assert_eq!(resolve_name(Some("__croniq")).unwrap(), "__croniq");
    }

    #[test]
    fn a_name_that_is_not_unicode_is_refused() {
        #[cfg(unix)]
        let raw = {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(b"croniq_\xff").to_os_string()
        };
        #[cfg(windows)]
        let raw = {
            use std::os::windows::ffi::OsStringExt;
            // An unpaired surrogate: valid UTF-16 code unit, not Unicode.
            std::ffi::OsString::from_wide(&[0x63, 0xD800])
        };
        let err = resolve_os_name(Some(&raw)).unwrap_err();
        assert!(err.contains(COOKIE_NAME_ENV), "{err}");
        assert!(err.contains("not valid Unicode"), "{err}");
    }

    #[test]
    fn name_from_env_reads_the_variable() {
        // SAFETY: `set_var` / `remove_var` are unsafe in Rust 2024 because they
        // mutate process-global state. Nothing else in this test binary reads
        // CRONIQ_REFRESH_COOKIE (main.rs is the only caller of
        // `name_from_env`), and this is the only test that writes it.
        unsafe { std::env::remove_var(COOKIE_NAME_ENV) };
        assert_eq!(name_from_env().unwrap(), DEFAULT_COOKIE_NAME);
        unsafe { std::env::set_var(COOKIE_NAME_ENV, " croniq_refresh_lane-a ") };
        assert_eq!(name_from_env().unwrap(), "croniq_refresh_lane-a");
        unsafe { std::env::set_var(COOKIE_NAME_ENV, "a;b") };
        assert!(name_from_env().is_err());
        unsafe { std::env::remove_var(COOKIE_NAME_ENV) };
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
