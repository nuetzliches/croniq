# Croniq UI Auth Notes

Interim plan for securing the Angular admin surface until the backend completes the full OIDC + delegated operator flow.

## Session Token Handling

- Tokens are captured via the tenant context panel and persisted with `AuthSessionService` (`src/app/core/auth/auth-session.service.ts`).
- Storage backend is always `sessionStorage`; no auth artifacts land in `localStorage` or IndexedDB.
- Expiration metadata is optional but recommended. Expired secrets are purged automatically.
- When an operator clears the token, the service also clears the matching storage slot, so refresh tokens never linger between sessions.

## Croniq API Key Bootstrap

- Operators can provide a temporary `X-Croniq-Key` in the tenant context UI. The value flows through `AuthSessionService` so it shares the same lifecycle rules as the opaque session token.
- `EndpointExecutor` now pulls both the API key and the bearer token from the credential supplier before every call. Manual overrides are still possible per request via `CroniqRequestOptions` for future automated flows.
- Keys are masked in the UI (last four characters only) to reduce shoulder-surfing risk.

## OIDC / PKCE Placeholder

- The `startOidcBootstrap()` hook leaves space for the PKCE handshake once the backend exposes the authorize endpoint.
- Today it resolves immediately, but the component-level call path (button + busy state) is in place, so dropping in the actual redirect/popup logic will not require UI rewrites.
- When the backend is ready, the hook should exchange the PKCE code for a Croniq session token, then update `AuthSessionService` so the UI refreshes automatically.

## Operator Impersonation vs. OAuth

- Impersonation toggles stay within `OperatorSession` storage (still `localStorage` so profiles survive reloads), while auth tokens remain session-scoped.
- Manual impersonation is the stopgap until delegated OAuth is GA. Once backend auth is authoritative, the UI will disable manual overrides and surface the backend-issued identity in the same callout.
- The plan above is now referenced from `CHECKLIST-UI.md`, keeping reviewers aligned on why we still allow manual overrides.

## Next Steps for Full Auth

1. Replace the PKCE placeholder with real OIDC client logic (likely via `@azure/msal-browser` or a lightweight PKCE helper).
2. Distribute the Croniq session token via HttpInterceptors instead of the shared executor so that feature modules can call `HttpClient` directly when needed.
3. Wire logout to clear session storage, operator impersonation state, and command palette caches.
4. Document CSP changes once the login redirect domain is finalized.
