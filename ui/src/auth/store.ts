import { create } from 'zustand'

/**
 * Session state. Nothing here is persisted (issue #454).
 *
 * The access token lives in module memory only: a reload starts with none and
 * `session.bootstrap()` mints a fresh one from the `HttpOnly` refresh cookie.
 * The refresh token is never in JavaScript's hands at all — except in a
 * `VITE_API_URL` cross-origin build, where `session.ts` keeps it in
 * `localStorage` because a `SameSite=Strict` cookie cannot reach that origin.
 */
export type AuthStatus =
  /** Boot-time refresh still in flight — we do not yet know which it is. */
  | 'unknown'
  /** Holding a usable access token. */
  | 'authenticated'
  /** No session: never logged in, logged out, or refresh failed. */
  | 'anonymous'

interface AuthState {
  token: string | null
  status: AuthStatus
  /** Record a freshly minted access token. */
  setToken: (token: string) => void
  /** Drop the session. The caller decides whether to also tell the server. */
  clear: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  // `unknown`, not `anonymous`: at module-eval time the boot refresh has not
  // run yet, and rendering the login page before it answers would bounce
  // every reload of an authenticated session (see ProtectedRoute).
  token: null,
  status: 'unknown',
  setToken: (token) => set({ token, status: 'authenticated' }),
  clear: () => set({ token: null, status: 'anonymous' }),
}))
