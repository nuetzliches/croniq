# Croniq UI Auth Notes

Interim plan for securing the Angular admin surface until the backend completes the delegated operator flow and external identity provider integration.

## Access Token Handling

- The UI stores the **access token** via `AuthSessionService` (`src/app/core/auth/auth-session.service.ts`).
- Storage backend for the access token is always `sessionStorage`; no auth artifacts land in `localStorage` or IndexedDB.
- Expiration metadata is optional but recommended. Expired secrets are purged automatically.

## Refresh Token Handling

- Refresh tokens are treated as sensitive and are **not persisted**.
- Current UI behavior keeps refresh tokens **memory-only** (per tab/session). Reloading the page requires re-login.

## Password Login (`/auth/*`)

The backend exposes these routes:

- `POST /auth/login` (request: `PasswordLoginRequest`)
- `POST /auth/refresh` (request: `PasswordRefreshRequest`)
- `POST /auth/logout` (request: `PasswordLogoutRequest`)

Important: The current OpenAPI snapshot documents request shapes, but does not yet include a response schema for these routes.
The UI therefore parses the response defensively and extracts the access token from common fields like `accessToken`.

### Tenant / Environment resolution

- Tenant and environment are configured server-side.
- The UI therefore **does not set `tenantId` nor `environmentTag`** in the login request.
- Tenant selection remains part of the UI shell context, but is not required for authentication.

## Tenant Token Issuance

- The UI can request short-lived tenant-scoped tokens via `TenantTokenEndpointService`.
- `EndpointExecutor` pulls the bearer token from the credential supplier before every call. Manual overrides are still possible per request via `CroniqRequestOptions`.
- The tenant-context panel can persist the returned secret automatically whenever the backend includes it in the response payload.
- Issuance requests capture client ID, scopes, TTL, and optional labels so the backend can scope the token properly. The service also estimates an expiry timestamp locally to keep storage metadata consistent when the response omits it.

## External Login Placeholder (future)

- The tenant-context bootstrap hook still leaves space for an interactive external login handshake (PKCE/OIDC) once the backend exposes the necessary endpoints.
- For now we use `/auth/login` (username/password) for local testing and early deployments.

## Operator Impersonation vs. OAuth

- Impersonation toggles stay within `OperatorSession` storage (still `localStorage` so profiles survive reloads), while auth tokens remain session-scoped.
- Manual impersonation is the stopgap until delegated OAuth is GA. Once backend auth is authoritative, the UI will disable manual overrides and surface the backend-issued identity in the same callout.
- The plan above is now referenced from `CHECKLIST-UI.md`, keeping reviewers aligned on why we still allow manual overrides.

## Next Steps for Full Auth

1. Replace password login with real external login client logic (PKCE-based).
2. Distribute the Croniq session token via HttpInterceptors instead of the shared executor so that feature modules can call `HttpClient` directly when needed.
3. Wire logout to clear session storage, operator impersonation state, and command palette caches.
4. Document CSP changes once the login redirect domain is finalized.
