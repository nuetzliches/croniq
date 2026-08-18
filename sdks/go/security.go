package croniq

import (
	"fmt"
	"log/slog"
	"net"
	"net/url"
	"strings"
)

// Transport-security checks applied to the configured base URL.
//
// Both the runner and the producer-side trigger client attach the credential
// as an Authorization header on every request. Over http:// that key travels
// in cleartext — and Go's http.DefaultTransport honours HTTP_PROXY, so it may
// traverse an intermediary in the clear as well.
//
// The rule (identical in the .NET, Java, Python and TypeScript SDKs):
// https:// is always fine, http:// is fine for a loopback host — that is the
// http://localhost:4000 quickstart path — and http:// against any other host
// is refused unless the caller explicitly opts in with WithInsecureHTTP.

// IsLoopbackHost reports whether host is one of the loopback hosts the SDK
// considers safe over cleartext HTTP: "localhost", any address in
// 127.0.0.0/8, and IPv6 ::1 (bare or in its bracketed "[::1]" form).
func IsLoopbackHost(host string) bool {
	h := strings.ToLower(strings.TrimSpace(host))
	h = strings.TrimSuffix(strings.TrimPrefix(h, "["), "]")
	if h == "" {
		return false
	}
	if h == "localhost" {
		return true
	}
	if ip := net.ParseIP(h); ip != nil {
		return ip.IsLoopback()
	}
	return false
}

// validateBaseURL rejects a base URL that would put the credential on the
// wire in the clear. Returns nil for https://, for http:// on a loopback
// host, and for http:// elsewhere when allowInsecure is set — in which case
// it also emits one loud warning naming the risk.
func validateBaseURL(baseURL string, allowInsecure bool) error {
	trimmed := strings.TrimSpace(baseURL)
	if trimmed == "" {
		return fmt.Errorf("croniq: server URL must not be empty")
	}

	parsed, err := url.Parse(trimmed)
	if err != nil {
		return fmt.Errorf("croniq: server URL %q is not a valid URL: %w", baseURL, err)
	}

	switch strings.ToLower(parsed.Scheme) {
	case "https":
		return nil
	case "http":
		// Checked below.
	default:
		return fmt.Errorf(
			"croniq: server URL %q has unsupported scheme %q; use https:// (or http:// for a loopback host)",
			baseURL, parsed.Scheme,
		)
	}

	if IsLoopbackHost(parsed.Hostname()) {
		return nil
	}

	if !allowInsecure {
		return fmt.Errorf(
			"croniq: server URL %q uses cleartext http:// with the non-loopback host %q: "+
				"the API key would be sent in the clear on every request, and through any "+
				"configured HTTP proxy. Use https://, or call WithInsecureHTTP() to accept "+
				"that risk explicitly",
			baseURL, parsed.Hostname(),
		)
	}

	slog.Warn(
		"SECURITY: Croniq is configured against a cleartext URL with WithInsecureHTTP(). "+
			"The API key is transmitted in cleartext on every request and is readable by "+
			"anyone on the network path (including HTTP proxies). Use https:// in production.",
		"server_url", baseURL,
	)
	return nil
}
