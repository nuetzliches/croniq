import { useQuery } from '@tanstack/vue-query'
import { computed } from 'vue'
import { apiGet } from './client'
import type {
  AuthConfigResponse,
  DeadLetter,
  HealthResponse,
  User,
  VersionResponse,
} from './types'
import { useAuthStore } from '~/stores/auth'

/**
 * Queries the shell needs. Screen-specific ones live with their screens; these
 * are the three things the frame itself asks for.
 */

/**
 * The signed-in user.
 *
 * Gated on `isAuthenticated` rather than fired unconditionally: before the
 * bootstrap refresh answers there is no access token, and an unconditional
 * query would spend a guaranteed 401 on every page load. `enabled` takes a ref
 * so it re-evaluates when the store settles — passing `auth.isAuthenticated`
 * by value would freeze it at `false` forever, which is the single most common
 * way to misuse vue-query.
 */
export function useCurrentUser() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['users', 'me'],
    queryFn: () => apiGet<User>('/v1/users/me'),
    enabled: computed(() => auth.isAuthenticated),
    staleTime: 60_000,
  })
}

/**
 * Which login methods the server offers. Public, so it works signed out —
 * which is the point, the login page needs it before any auth happens.
 */
export function useAuthConfig() {
  return useQuery({
    queryKey: ['auth', 'config'],
    queryFn: () => apiGet<AuthConfigResponse>('/v1/auth/config'),
    staleTime: Infinity,
    retry: false,
  })
}

/**
 * Count for the Dead Letters badge.
 *
 * A count, not the list: the badge needs a number and the screen fetches its
 * own rows. Polled because a dead letter appearing is exactly the kind of thing
 * an operator should not have to reload to notice.
 */
export function useDeadLetterCount() {
  const auth = useAuthStore()
  const query = useQuery({
    queryKey: ['dead-letters', 'count'],
    queryFn: () => apiGet<DeadLetter[]>('/v1/dead-letters', { limit: 100 }),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 30_000,
  })
  return computed(() => query.data.value?.length ?? 0)
}

/**
 * Server health. Public, so the login page can show it before anyone signs in
 * — which is where the shipping dashboard uses it, and a genuinely good idea:
 * you learn the server is alive before you have credentials to check with.
 *
 * Polled, because the shell's live dot is only worth having if it can go out.
 */
export function useHealth() {
  return useQuery({
    queryKey: ['health'],
    queryFn: () => apiGet<HealthResponse>('/health'),
    refetchInterval: 5_000,
    retry: false,
  })
}

/**
 * Build version. Public, and pinned forever — it cannot change without the
 * process restarting, at which point the page reloads anyway.
 *
 * Failures are expected against an older server that has no `/version`, so the
 * caller treats `undefined` as "hide the chip" rather than as an error.
 */
export function useVersion() {
  return useQuery({
    queryKey: ['version'],
    queryFn: () => apiGet<VersionResponse>('/version'),
    staleTime: Infinity,
    retry: false,
  })
}
