/**
 * API clients the server's environment owns (issue #471).
 *
 * Every mutation on such a client is refused, and the dashboard has to explain
 * that *before* the attempt — which means naming the variable to edit instead.
 */

/** `managed_by` value for a client the environment declares. */
export const MANAGED_BY_ENV = 'env'

/**
 * The environment variable that declares `clientName`.
 *
 * `default` is the exception the obvious template gets wrong: it is declared by
 * `CRONIQ_API_KEY`, *outside* the `CRONIQ_API_CLIENT_` namespace. Telling an
 * operator to edit `CRONIQ_API_CLIENT_DEFAULT_KEY` would have them add a second
 * declaration of the same client, which the server refuses at its next boot —
 * so the wrong hint does not merely fail to help, it takes the server down
 * (issue #481).
 *
 * Mirrors `api_client_env::canonical_key_var` on the server. The server's own
 * 409 resolves this against the live environment and can therefore also name
 * the deprecated `CRONIQ_INIT_API_KEY` alias; the dashboard cannot see that
 * environment, so it names the canonical spelling.
 */
export function declaringKeyVar(clientName: string): string {
  if (clientName === 'default') return 'CRONIQ_API_KEY'
  return `CRONIQ_API_CLIENT_${clientName.toUpperCase().replace(/-/g, '_')}_KEY`
}
