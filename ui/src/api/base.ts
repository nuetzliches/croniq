// Default to same-origin (relative URLs) so the UI works unchanged in any
// deployment where croniq-server serves both the UI and the API — which is
// the standard setup, including the official Docker image. For local dev
// (npm run dev on :5173) the Vite dev-server proxies /v1, /health, /metrics
// to http://localhost:4000 — see vite.config.ts. Override VITE_API_URL at
// build time only when UI and API live on different origins.
export const API_BASE = import.meta.env.VITE_API_URL ?? ''

/**
 * Whether this build can use the `HttpOnly` refresh cookie (issue #454).
 *
 * The cookie is `SameSite=Strict`, so it only ever reaches an API on our own
 * origin. A `VITE_API_URL` build talks to a different origin, never gets the
 * cookie back, and therefore has to keep the refresh token in `localStorage`
 * the way every release before #454 did — with the XSS exposure that implies.
 *
 * Producing such a build requires setting `VITE_ALLOW_LOCALSTORAGE_REFRESH=1`
 * alongside `VITE_API_URL`; `vite.config.ts` refuses to build otherwise, so
 * nobody ends up in the weaker mode without having said so. The server-side
 * half of the same gate lives in `api::auth_endpoints::resolve_delivery`.
 */
export const REFRESH_COOKIE_MODE = API_BASE === ''
