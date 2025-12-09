# Croniq Authentication Guide

This consumer-focused guide explains how to secure Croniq hosts with API keys or OAuth2/OIDC. Use it when you deploy `Croniq.Api`, any sample host, or your own app that embeds Croniq services. The deep-dive design remains in `/deep-dive/security.md`.

## Choose an Authentication Mode

| Scenario                                     | Recommended Mode          | Notes                                                                                                                   |
| -------------------------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Automation, schedulers, integration tests    | API keys                  | Lowest friction, scopes enforced per key, easiest to rotate non-interactive callers.                                    |
| Human operators via UI, self-service portals | OAuth2/OIDC bearer tokens | Reuse your identity provider (Entra ID, Auth0, etc.) and map tenants via claims.                                        |
| Hybrid workloads                             | Mixed (enable both)       | Croniq inspects `Authorization: Bearer ...` first, then `X-Croniq-Key`. Only one caller context is created per request. |

You can switch between modes (or enable both) by changing configuration—no code changes are required.

## API Keys

### When to Use API Keys

- Service-to-service calls (CI pipelines, platform schedulers).
- Croniq SDK clients that run outside your identity provider.
- Local development where you do not want to maintain an IdP tenant.

### Provisioning Flow

1. **Pick a backing store**
   - `Croniq:Auth:Mode = InMemory`: single key, best for samples/tests only.
   - `Croniq:Auth:Mode = SqlServer`: production-ready, uses `Croniq.Auth.SqlServer` to hash and store keys.
2. **Issue a key**
   - SQL-backed hosts will expose admin endpoints under `/tenants/{id}/api-keys` (after the backlog item completes). Until then, call `IApiKeyStore.IssueAsync` via a bootstrap script or console app.
   - In-memory mode reads the secret from `Croniq__Auth__InMemory__ApiKey` (or `appsettings.*`).
3. **Distribute the plaintext** once. Operators copy it into CI/CD variables or `.env.local`. Croniq never stores the plaintext value.
4. **Call the API** with the header `X-Croniq-Key: <your-secret>`.

### Configuration Checklist

| Setting                                                   | Required?             | Description                                                                     |
| --------------------------------------------------------- | --------------------- | ------------------------------------------------------------------------------- |
| `Croniq__Auth__Mode`                                      | Always                | `InMemory` or `SqlServer`.                                                      |
| `Croniq__Auth__SqlServer__ConnectionString`               | When `SqlServer` mode | Overrides the default Croniq SQL connection if needed.                          |
| `Croniq__Auth__InMemory__ApiKey`                          | When `InMemory` mode  | Single dev key used by all callers.                                             |
| `Croniq__Core__TenantId` / `Croniq__Core__EnvironmentTag` | Optional              | Embedded in every issued key so rate limiting and auditing remain tenant-aware. |

Example local `.cmd` snippet:

```cmd
set Croniq__Auth__Mode=InMemory
set Croniq__Auth__InMemory__ApiKey=crq_dev_local_sample
set Croniq__Core__TenantId=dev-sandbox
set Croniq__Core__EnvironmentTag=dev-jane
```

### Rotation & Revocation

- SQL mode: call `IApiKeyStore.RotateAsync` / `RevokeAsync` (directly or via the future admin API). Croniq writes the action to `auth.AuditLog` so you can track who changed a key.
- In-memory mode: update the environment variable and restart the host. Any cached callers must pick up the new value.
- Always remove revoked keys from CI/CD secrets. Croniq rate limiting partitions by Tenant + Caller ID, so stale keys fall back to anonymous throttles and fail fast.

## OAuth2 / OIDC

### When to Use OAuth2/OIDC

- Human users interact with Croniq dashboards or management APIs.
- You already have an IdP and prefer centralized lifecycle management.
- You need per-user scopes (e.g., schedule viewers vs tenant admins).

### High-Level Steps

1. **Create an application** in your identity provider.
   - Enable the authorization code + PKCE or client credentials flow depending on your client type.
   - Configure audiences/scopes that match Croniq permissions (e.g., `schedules.read`, `jobs.trigger`, `tenants.admin`).
2. **Configure Croniq** via `Croniq:Auth:Oidc` options:
   - `Authority`: the issuer URL (e.g., `https://login.microsoftonline.com/<tenant>/v2.0`).
   - `Audience`: the API resource identifier your IdP issues.
   - `RequiredScopes`: comma-separated list enforced at gateway level.
   - `TenantClaim`: claim name that contains the tenant id (`tenant`, `tid`, or custom claim).
3. **Enable JWT bearer authentication** in your host (Croniq.Api wires this automatically once the backlog item lands).
4. **Send requests** with `Authorization: Bearer <access-token>` headers. Croniq validates the token using the authority JWKS and builds the caller context from claims.

### Claim Mapping

| Croniq Field    | Default Claim             | Notes                                                                      |
| --------------- | ------------------------- | -------------------------------------------------------------------------- |
| Tenant Id       | `tenant` (fallback `tid`) | Override via `Croniq:Auth:Oidc:TenantClaim` when you use custom naming.    |
| Environment Tag | `env`                     | Missing value falls back to the configured default per tenant/environment. |
| Scopes          | `scope` or `scp`          | Missing required scopes cause a 403 response.                              |
| Caller Id       | `sub`                     | combine with tenant when you audit requests.                               |

### Mixed Mode

Set both configurations (API keys + OIDC). Croniq inspects headers in this order per request:

1. `Authorization: Bearer ...` → OIDC path.
2. `X-Croniq-Key` → API key path.
3. No headers → request rejected with 401.

This allows service accounts and humans to coexist without separate gateways.

## Local Development

| Task                      | Tip                                                                                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Use a single shared key   | Keep `Croniq__Auth__Mode=InMemory` for local runs and place the key in `.env`. Scripts in `scripts/` already load `.env` when they start the dev stack. |
| Test OAuth flows          | Run the API outside Docker (so it can reach your IdP callback) and point it at a dev tenant. Use `dotnet user-secrets` to store client secrets.         |
| Simulate multiple tenants | Override `Croniq__Core__TenantId` and `Croniq__Core__EnvironmentTag` per terminal session to see how rate limiting and observability labels change.     |

## FAQs

**How do I know which mode is active?**
Check the startup logs—Croniq logs the effective `Auth.Mode` and the connection string source. You can also hit `/health` with `X-Croniq-Debug: auth` once that probe lands (tracked in the security backlog).

**Can I use mTLS or a gateway instead?**
Yes. Terminate TLS or authenticate upstream (e.g., Azure APIM, Ambassador) and forward either `Authorization` or `X-Croniq-Key` headers. Additional first-class providers will plug into `ICallerContextFactory` later.

**Where do I store secrets?**
Use `ISecretProvider` (Key Vault, AWS Secrets Manager, etc.). The default sample hosts accept `.env.local` for convenience but production deployments should keep secrets outside appsettings.

## Related Docs

- `/introduction/configuration.md` – environment variable matrix and troubleshooting tips.
- `/ops/troubleshooting.md` – common auth failures and rate limit issues.
- `/deep-dive/security.md` – in-depth design, rate limiting, and backlog status.
