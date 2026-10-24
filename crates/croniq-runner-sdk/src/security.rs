//! Transport-security check on the configured server URL (issue #440; review
//! 2026-10-24, K-3).
//!
//! The runner and the trigger client attach the credential as an
//! `Authorization` header on every request. Over `http://` that key travels in
//! cleartext, and through any configured HTTP proxy as well. The rule is the
//! one the Go, .NET, Java, Python and TypeScript SDKs apply: `https://` is
//! always fine, `http://` is fine for a loopback host (the
//! `http://localhost:4000` quickstart), and `http://` to any other host is
//! refused unless the caller opts in explicitly.

use reqwest::Url;

/// Whether `host` is a loopback host the SDK accepts over cleartext HTTP:
/// `localhost`, any address in 127.0.0.0/8, and `::1` (bare or bracketed).
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    let h = h.trim_start_matches('[').trim_end_matches(']');
    if h == "localhost" {
        return true;
    }
    h.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Refuse a base URL that would put the credential on the wire in the clear.
///
/// `Ok` for `https://`, for `http://` on a loopback host, and for `http://`
/// elsewhere when `allow_insecure` is set — in which case one loud warning
/// names the risk. The error is a complete operator-facing message.
pub(crate) fn validate_base_url(base_url: &str, allow_insecure: bool) -> Result<(), String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("croniq: server URL must not be empty".into());
    }
    let parsed = Url::parse(trimmed)
        .map_err(|e| format!("croniq: server URL {base_url:?} is not a valid URL: {e}"))?;
    match parsed.scheme() {
        "https" => return Ok(()),
        "http" => {}
        other => {
            return Err(format!(
                "croniq: server URL {base_url:?} has unsupported scheme {other:?}; use https:// \
                 (or http:// for a loopback host)"
            ));
        }
    }
    let host = parsed.host_str().unwrap_or_default();
    if is_loopback_host(host) {
        return Ok(());
    }
    if !allow_insecure {
        return Err(format!(
            "croniq: server URL {base_url:?} uses cleartext http:// with the non-loopback host \
             {host:?}: the API key would be sent in the clear on every request, and through any \
             configured HTTP proxy. Use https://, or opt in with allow_insecure_http() \
             (CRONIQ_ALLOW_INSECURE_HTTP=1 for the croniq runner binaries) to accept that risk \
             explicitly"
        ));
    }
    tracing::warn!(
        server_url = %base_url,
        "SECURITY: Croniq is configured against a cleartext http:// URL with \
         allow_insecure_http. The API key is transmitted in cleartext on every request and is \
         readable by anyone on the network path (including HTTP proxies). Use https:// in \
         production."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts() {
        for h in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback_host(h), "{h}");
        }
        for h in ["", "server", "10.0.0.1", "localhost.example.com", "::2"] {
            assert!(!is_loopback_host(h), "{h}");
        }
    }

    #[test]
    fn https_and_loopback_http_are_accepted() {
        for url in [
            "https://croniq.example.com",
            "https://10.0.0.5:4000/",
            "http://localhost:4000",
            "http://127.0.0.1:4000",
            "http://[::1]:4000",
        ] {
            assert!(validate_base_url(url, false).is_ok(), "{url}");
        }
    }

    #[test]
    fn cleartext_to_a_remote_host_is_refused_without_the_opt_in() {
        let err = validate_base_url("http://server:4000", false).unwrap_err();
        assert!(err.contains("cleartext"), "{err}");
        assert!(err.contains("allow_insecure_http"), "{err}");
        assert!(validate_base_url("http://server:4000", true).is_ok());
    }

    #[test]
    fn junk_and_other_schemes_are_refused_even_with_the_opt_in() {
        for url in ["", "   ", "server:4000", "ftp://server", "not a url"] {
            assert!(validate_base_url(url, true).is_err(), "{url:?}");
        }
    }
}
