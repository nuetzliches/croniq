import { computed, ref } from 'vue'
import { defineStore } from 'pinia'

// Explicit imports in `.ts` files, even though `@nuxt/ui`'s autoImport would
// supply these. Auto-imports are declared in a file the Vite plugin generates,
// so a type check would depend on a build having run first — a footgun in CI
// and in a fresh clone. Templates and components still use them; modules that
// tests import directly say what they use.

/**
 * Session state.
 *
 * `status` exists because "not signed in" and "we do not know yet" are
 * different answers and the router has to tell them apart: on a reload there
 * is no access token (it is memory-only since #454, see ADR-0001), and the app
 * has to redeem the refresh cookie before it can say whether there is a
 * session. A guard that treats `unknown` as `anonymous` bounces every reload
 * to the login page.
 */
export type AuthStatus = 'unknown' | 'anonymous' | 'authenticated'

export const useAuthStore = defineStore('auth', () => {
  /**
   * The access token, in memory only and never persisted.
   *
   * This is the whole point of ADR-0001: the durable half of the session is an
   * `HttpOnly` cookie JavaScript cannot read, and the half JavaScript can read
   * lives one hour and dies with the tab. Writing this to `localStorage` for
   * convenience would undo #454.
   */
  const token = ref<string | null>(null)
  const status = ref<AuthStatus>('unknown')

  const isAuthenticated = computed(() => status.value === 'authenticated')

  function setToken(next: string) {
    token.value = next
    status.value = 'authenticated'
  }

  function clear() {
    token.value = null
    status.value = 'anonymous'
  }

  return { token, status, isAuthenticated, setToken, clear }
})
