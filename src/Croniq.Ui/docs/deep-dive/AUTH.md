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

Current UI scope:

- The **Tenants** feature module is intentionally **excluded from the UI navigation** (no menu entries / command palette shortcuts).
- The UI still needs a tenant identifier for tenant-scoped API routes (e.g. `/tenants/:tenantId/*`).
- The tenant id must be treated as an explicit identifier: the UI must not rely on server-side defaults or mode-specific fallbacks.
- The UI persists the tenant id from the login response in `sessionStorage` (see `AuthSessionService`). Refresh/logout use that stored value.

## Tenant Context in the UI

- The tenant-context panel stores the active tenant identity and environment plus feature-flag overrides.
- It no longer supports manual token entry or token issuance; authentication is handled via the `/login` page.
- Logout clears auth state, tenant context, and tenant-scoped UI preferences (best-effort).

## Next Steps for Full Auth

- [ ] Replace password login with real external login client logic (PKCE-based).
- [x] Distribute the Croniq session token via HttpInterceptors instead of the shared executor so that feature modules can call `HttpClient` directly when needed.
- [x] Wire logout to clear session storage and any relevant client caches.
- [ ] Document CSP changes once the login redirect domain is finalized.

## Planning: Optional OIDC + PKCE (UI-first)

Goal: Keep the password flow as the default (no external provider required) while adding an optional OIDC path (e.g., Authelia) that can be enabled by configuration.

### Phase 0: Configuration Contract

- Runtime config (UI): add optional OIDC config block in `public/assets/croniq-config.json` (issuer/authority, clientId, redirectUri, scopes, enable flag).
- Server config (API): keep `Croniq:Auth:Oidc:*` as the bearer validation source; do not require OIDC for API key/password paths.
- Document environment variables in `.env.example` and in the auth guides (done).

### Phase 1: UI Routing + State Model

- Add `AuthProviderMode = Password | Oidc` derived from runtime config.
- Introduce routes:
  - `/auth/login` (current password login form remains)
  - `/auth/oidc/start` (builds PKCE challenge + redirects to IdP)
  - `/auth/oidc/callback` (handles code exchange + token storage)
- Create an `OidcSessionService` to manage PKCE verifier, state nonce, and token exchange.

### Phase 2: Token Exchange + Storage

- Exchange authorization code for tokens via backend token endpoint or via IdP (depending on deployment choice).
- Store access token in `AuthSessionService` (same as password flow).
- Do not store refresh token unless the backend issues one and policy allows it.
- Reuse existing `authRefreshInterceptor` for access token attachment.

### Phase 3: Logout Semantics

- Logout clears auth state and caches (implemented).
- If OIDC is enabled, also trigger IdP logout (optional, based on provider capabilities).

### Phase 4: CSP + Security Hardening

- Update CSP to allow the IdP domain for redirect and token exchange.
- Confirm PKCE verifier storage (memory or sessionStorage) aligns with the threat model.

### Open Questions

- Should code exchange happen via the UI directly or via a backend proxy endpoint?
- Do we require refresh tokens for OIDC sessions, or rely on short-lived access tokens + re-login?
- Which claim maps should be the defaults for Authelia (tenant/env/scope)?
