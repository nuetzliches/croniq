# Croniq Authentication Guide

This consumer-focused guide explains how to secure Croniq hosts with API keys and (optionally) bearer tokens. Use it when you deploy `Croniq.Api`, any sample host, or your own app that embeds Croniq services. The deep-dive design remains in `/deep-dive/security.md`.

## Choose an Authentication Mode

| Scenario                                  | Recommended Mode    | Notes                                                                                                                   |
| ----------------------------------------- | ------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Automation, schedulers, integration tests | API keys            | Lowest friction, scopes enforced per key, easiest to rotate non-interactive callers.                                    |
| Human operators via UI                    | Password login      | Self-hosted option; Croniq issues access + refresh tokens.                                                              |
| Hybrid workloads                          | Mixed (enable both) | Croniq inspects `Authorization: Bearer ...` first, then `X-Croniq-Key`. Only one caller context is created per request. |

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

## Password Login

Croniq can expose a username/password login for self-hosted deployments.

- Endpoints: `/auth/login`, `/auth/refresh`, `/auth/logout`
- Configure default tenant resolution via `Croniq:Auth:Password:DefaultTenant`.
- Stand jetzt: the API returns `refreshToken` in the JSON response body and expects it in the request body for refresh/logout.

See [docs/deep-dive/password-auth.md](/deep-dive/password-auth.md) for details.

## Local Development

| Task                      | Tip                                                                                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Use a single shared key   | Keep `Croniq__Auth__Mode=InMemory` for local runs and place the key in `.env`. Scripts in `scripts/` already load `.env` when they start the dev stack. |
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
