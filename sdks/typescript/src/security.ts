// Transport-security checks applied to the configured base URL.
//
// Both the runner and the producer-side trigger client attach the credential
// as an `Authorization` header on every request. Over `http://` that key
// travels in cleartext — and undici honours `HTTP_PROXY` by default, so it may
// traverse an intermediary in the clear as well.
//
// The rule (identical in the .NET, Java, Python and Go SDKs): `https://` is
// always fine, `http://` is fine for a loopback host — that is the
// `http://localhost:4000` quickstart path — and `http://` against any other
// host is refused unless the caller explicitly opts in with
// `allowInsecureHttp: true`. Enforcement happens at options-resolution /
// construction time so a misconfiguration fails fast rather than on first poll.
//
// Kept regex-free on purpose, matching `url.ts`: CodeQL's "polynomial regex"
// heuristic flags even linear patterns over library-controlled input.

import type { Logger } from './logger.js';

/**
 * `true` for the loopback hosts the SDK considers safe over cleartext HTTP:
 * `localhost`, anything in `127.0.0.0/8`, and IPv6 `::1` (in the bracketed
 * `[::1]` form the WHATWG URL parser produces).
 */
export function isLoopbackHostname(hostname: string): boolean {
  const host = hostname.toLowerCase();
  if (host === 'localhost') return true;
  if (host.startsWith('[') && host.endsWith(']')) {
    return host.slice(1, -1) === '::1';
  }
  if (host === '::1') return true;
  return isIpv4Loopback(host);
}

/**
 * Throws a {@link TypeError} when `serverUrl` is a non-loopback `http://` URL
 * and `allowInsecureHttp` is not set. When the opt-in IS set for such a URL,
 * emits exactly one loud warning naming the risk.
 *
 * @param serverUrl Configured base URL (already known to be parseable).
 * @param allowInsecureHttp Caller's explicit opt-in to cleartext HTTP.
 * @param optionPath Dotted option path, used to make the error actionable.
 * @param logger Sink for the opt-in warning.
 */
export function assertSecureServerUrl(
  serverUrl: string,
  allowInsecureHttp: boolean,
  optionPath: string,
  logger: Logger,
): void {
  const parsed = new URL(serverUrl);
  const protocol = parsed.protocol.toLowerCase();

  if (protocol === 'https:') return;

  if (protocol !== 'http:') {
    throw new TypeError(
      `${optionPath} "${serverUrl}" has unsupported scheme "${parsed.protocol}": ` +
        'use https:// (or http:// for a loopback host)',
    );
  }

  if (isLoopbackHostname(parsed.hostname)) return;

  if (!allowInsecureHttp) {
    throw new TypeError(
      `${optionPath} "${serverUrl}" uses cleartext http:// with the non-loopback host ` +
        `"${parsed.hostname}": the API key would be sent in the clear on every request, ` +
        'and through any configured HTTP proxy. Use https://, or set ' +
        'allowInsecureHttp: true to accept that risk explicitly.',
    );
  }

  logger.warn(
    `SECURITY: Croniq is configured against the cleartext URL "${serverUrl}" with ` +
      'allowInsecureHttp: true. The API key is transmitted in cleartext on every request ' +
      'and is readable by anyone on the network path (including HTTP proxies). ' +
      'Use https:// in production.',
    { server_url: serverUrl },
  );
}

/** `true` for a dotted-quad IPv4 literal inside `127.0.0.0/8`. */
function isIpv4Loopback(host: string): boolean {
  const parts = host.split('.');
  if (parts.length !== 4) return false;
  for (const part of parts) {
    if (part.length === 0 || part.length > 3) return false;
    for (let i = 0; i < part.length; i++) {
      const code = part.charCodeAt(i);
      if (code < 48 /* '0' */ || code > 57 /* '9' */) return false;
    }
    if (Number(part) > 255) return false;
  }
  return Number(parts[0]) === 127;
}
