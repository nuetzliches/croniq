import { computed, type ComputedRef } from 'vue'
import { useCurrentUser } from '~/api/queries'

/**
 * Is this session allowed to see the admin surfaces?
 *
 * The default when there is no user record is `true`, and that is deliberate
 * rather than permissive. `GET /v1/users/me` resolves only for password, OIDC
 * and PAT sessions; an API-key session has no user row and the query answers
 * with nothing. Such a session's authority comes from its *scopes*, which the
 * server enforces on every request — so hiding the admin surfaces from it
 * would hide them from a caller that may well be entitled to them, on the
 * strength of a record that was never going to exist.
 *
 * The same default also covers the moment before the query resolves: showing
 * the surfaces and letting the server refuse is better than flashing a nav
 * that loses entries a heartbeat later.
 *
 * This is presentation only. Nothing here grants anything; the server decides.
 *
 * It lives in one place because the expression is short enough to retype and
 * subtle enough to retype wrong — the shell and the settings screen have to
 * agree about who sees what.
 */
export function useIsAdmin(): ComputedRef<boolean> {
  const { data: me } = useCurrentUser()
  return computed(() => (me.value ? me.value.role === 'admin' : true))
}
