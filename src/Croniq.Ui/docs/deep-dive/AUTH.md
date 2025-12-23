# Croniq UI Auth Notes

Interim plan for securing the Angular admin surface until the backend completes the delegated operator flow and external identity provider integration.

## Access Token Handling

- The UI stores the **access token** via `AuthSessionService` (`src/app/core/auth/auth-session.service.ts`).
- Storage backend for the access token is always `sessionStorage`; no auth artifacts land in `localStorage` or IndexedDB.
- Expiration metadata is optional but recommended. Expired secrets are purged automatically.

## Refresh Token Handling

- Refresh tokens are treated as sensitive and are **not persisted**.
- Current UI behavior keeps refresh tokens **memory-only** (per tab/session). Reloading the page requires re-login.

### Transport (decision)

Croniq uses **Variant A** for refresh token transport:

- `/auth/login` returns `refreshToken` in the JSON response body.
- `/auth/refresh` and `/auth/logout` expect `refreshToken` in the JSON request body.

The UI must not rely on refresh-token cookies.

## Password Login (`/auth/*`)

The backend exposes these routes:

- `POST /auth/login` (request: `PasswordLoginRequest`)
- `POST /auth/refresh` (request: `PasswordRefreshRequest`)
- `POST /auth/logout` (request: `PasswordLogoutRequest`)

Important: The current OpenAPI snapshot documents request shapes, but may not include a response schema for these routes.
The UI therefore parses the response defensively.

### Concrete response shapes (backend)

The backend implementation lives in `src/Croniq.Api/ApiHostingExtensions.PasswordAuth.cs` and currently returns:

- `POST /auth/login` → `200 OK`
  - `tenantId: string | null`
  - `accessToken: string`
  - `tokenType: "Bearer"`
  - `expiresIn: number` (seconds)
  - `refreshToken: string | null`
  - `passwordChangeRequired: boolean | null`
- `POST /auth/refresh` → `200 OK`
  - `accessToken: string`
  - `tokenType: "Bearer"`
  - `expiresIn: number` (seconds)
  - `refreshToken: string | null`
  - `passwordChangeRequired: boolean | null`
- `POST /auth/logout` → `204 NoContent`

The UI computes a best-effort `expiresAt` from `expiresIn` when the backend does not return an absolute timestamp.

### Tenant / Environment resolution

- The UI provides `tenantId` for password auth requests.
- The UI also does not set `environmentTag` in login/refresh; environment selection is part of the UI shell context.
- Tenant selection remains part of the UI shell context.

Current UI scope (single-tenant):

- The **Tenants** feature module is intentionally **excluded from the UI navigation** (no menu entries / command palette shortcuts).
- The UI still needs a tenant identifier for tenant-scoped API routes (e.g. `/tenants/:tenantId/*`).
- Per the root documentation, this value should be treated as a **tenant id** and may be provided by the backend (e.g. `tenantId` in the login response) or by server-side defaults.

## Tenant Context in the UI

- The tenant-context panel stores the active tenant identity and environment plus feature-flag overrides.
- It no longer supports manual token entry or token issuance; authentication is handled via the `/login` page.

## Next Steps for Full Auth

1. Replace password login with real external login client logic (PKCE-based).
2. Distribute the Croniq session token via HttpInterceptors instead of the shared executor so that feature modules can call `HttpClient` directly when needed.
3. Wire logout to clear session storage and any relevant client caches.
4. Document CSP changes once the login redirect domain is finalized.
