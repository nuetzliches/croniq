package io.croniq.runner.config;

import java.net.InetAddress;
import java.net.URI;
import java.net.UnknownHostException;
import java.util.Locale;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Transport-security checks applied to a configured base URL.
 *
 * <p>Both the runner and the producer-side trigger client attach the credential as an
 * {@code Authorization} header on every request. Over cleartext that key travels in the
 * clear — and through any HTTP proxy the environment configures.
 *
 * <p>The rule (identical in the .NET, Python, Go and TypeScript SDKs): an {@code https}
 * URL is always fine, an {@code http} URL is fine for a loopback host — that is the
 * documented {@code http://localhost:4000} quickstart path — and an {@code http} URL
 * against any other host is refused unless the caller explicitly sets
 * {@code allowInsecureHttp(true)} on the options builder. Refusal happens in
 * {@code build()}, so a misconfiguration fails fast instead of on the first poll.
 */
public final class ServerUrls {

    private static final Logger log = LoggerFactory.getLogger(ServerUrls.class);

    private static final String UNSUPPORTED_SCHEME =
            """
            %s.serverUrl '%s' has unsupported scheme '%s': use https (or http for a loopback host).\
            """;

    private static final String CLEARTEXT_REFUSED =
            """
            %s.serverUrl '%s' uses cleartext http with the non-loopback host '%s': the API key \
            would be sent in the clear on every request, and through any configured HTTP proxy. \
            Use https, or set allowInsecureHttp(true) on the options builder to accept that risk \
            explicitly.\
            """;

    private static final String CLEARTEXT_WARNING =
            """
            SECURITY: Croniq is configured against the cleartext URL '{}' with \
            allowInsecureHttp(true). The API key is transmitted in cleartext on every request and \
            is readable by anyone on the network path (including HTTP proxies). Use https in \
            production.\
            """;

    private ServerUrls() {}

    /**
     * Returns {@code true} for the loopback hosts considered safe over cleartext HTTP:
     * {@code localhost}, anything in {@code 127.0.0.0/8}, and IPv6 {@code ::1} (bare or in
     * the bracketed form {@link URI#getHost()} returns).
     *
     * @param host host part of a URL, possibly {@code null}
     * @return whether the host is a loopback host
     */
    public static boolean isLoopbackHost(String host) {
        if (host == null) {
            return false;
        }
        String candidate = host.trim().toLowerCase(Locale.ROOT);
        if (candidate.startsWith("[") && candidate.endsWith("]")) {
            candidate = candidate.substring(1, candidate.length() - 1);
        }
        if (candidate.isEmpty()) {
            return false;
        }
        if ("localhost".equals(candidate)) {
            return true;
        }
        // Only resolve strings that are unambiguously IP literals — passing a real
        // hostname to InetAddress would trigger a blocking DNS lookup.
        if (isIpLiteral(candidate)) {
            return isLoopbackAddress(candidate);
        }
        return false;
    }

    /**
     * Validates a configured base URL, throwing when the credential would go out in the
     * clear. Emits one loud warning instead when the caller opted in explicitly.
     *
     * @param serverUrl configured base URL
     * @param allowInsecureHttp caller's explicit opt-in to cleartext HTTP
     * @param optionsName options class name, used to make the message actionable
     * @throws IllegalArgumentException when the URL is refused
     */
    static void validate(URI serverUrl, boolean allowInsecureHttp, String optionsName) {
        if (serverUrl == null) {
            throw new IllegalArgumentException(optionsName + ".serverUrl must not be null");
        }
        String scheme = serverUrl.getScheme();
        if (scheme == null) {
            throw new IllegalArgumentException(optionsName + ".serverUrl '" + serverUrl + "' must be an absolute URL");
        }
        String normalised = scheme.toLowerCase(Locale.ROOT);
        if ("https".equals(normalised)) {
            return;
        }
        if (!"http".equals(normalised)) {
            throw new IllegalArgumentException(UNSUPPORTED_SCHEME.formatted(optionsName, serverUrl, scheme));
        }
        String host = serverUrl.getHost();
        if (isLoopbackHost(host)) {
            return;
        }
        if (!allowInsecureHttp) {
            throw new IllegalArgumentException(
                    CLEARTEXT_REFUSED.formatted(optionsName, serverUrl, String.valueOf(host)));
        }
        log.warn(CLEARTEXT_WARNING, serverUrl);
    }

    /** Whether the candidate is an IPv4 dotted quad or an IPv6 literal. */
    private static boolean isIpLiteral(String candidate) {
        if (candidate.indexOf(':') >= 0) {
            return isIpv6Shaped(candidate);
        }
        return isDottedQuad(candidate);
    }

    /** Whether the candidate consists only of characters an IPv6 literal may contain. */
    private static boolean isIpv6Shaped(String candidate) {
        for (int i = 0; i < candidate.length(); i++) {
            char c = candidate.charAt(i);
            boolean allowed = c == ':' || c == '.' || (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f');
            if (!allowed) {
                return false;
            }
        }
        return true;
    }

    /** Whether the candidate is four dot-separated decimal octets. */
    private static boolean isDottedQuad(String candidate) {
        String[] parts = candidate.split("\\.", -1);
        if (parts.length != 4) {
            return false;
        }
        for (String part : parts) {
            if (part.isEmpty() || part.length() > 3) {
                return false;
            }
            for (int i = 0; i < part.length(); i++) {
                char c = part.charAt(i);
                if (c < '0' || c > '9') {
                    return false;
                }
            }
            if (Integer.parseInt(part) > 255) {
                return false;
            }
        }
        return true;
    }

    /** Resolves an IP literal (never a hostname) and reports whether it is loopback. */
    private static boolean isLoopbackAddress(String literal) {
        try {
            return InetAddress.getByName(literal).isLoopbackAddress();
        } catch (UnknownHostException e) {
            // Not a well-formed literal after all — treat as non-loopback.
            return false;
        }
    }
}
