# Croniq Configuration Guide

This guide explains how the Croniq API host (`Croniq.Api`) resolves configuration when you call `AddCroniqApiServices(...)` and `UseCroniqApi(...)`. It also lists the environment variables you typically need during local development or deployments. If you still need to pick an auth mode or understand how callers authenticate, start with the consumer guide in [`auth.md`](/guides/auth.md).

## 1. Configuration Sources & Priority

`builder.Services.AddCroniqApiServices(builder.Configuration)` binds the following option objects from the provided `IConfiguration` instance: `CroniqApiOptions`, `CroniqOptions`, `CroniqAuthOptions`, `CroniqPersistenceOptions`, and `SqlServerOptions`. The usual ASP.NET Core precedence applies (later providers win):

1. `appsettings.json` and other JSON files.
2. Environment-specific JSON (e.g., `appsettings.Development.json`).
3. Environment variables (`Croniq__Section__Property` notation with double underscores).
4. Secret providers such as `dotnet user-secrets`, Azure Key Vault, or custom `IConfiguration` sources.

There is no bespoke `AddCroniq(...)` overload; customization is done through configuration files, environment variables, or the normal `Configure<TOptions>`/`PostConfigure<TOptions>` patterns.

## 2. Registration Quick Reference

```csharp
var builder = WebApplication.CreateBuilder(args);

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();     // optional but recommended

var app = builder.Build();
app.UseCroniqApi();
app.Run();
```

Need to override something programmatically? Use the regular options APIs before building the app:

```csharp
builder.Services.PostConfigure<CroniqAuthOptions>(options =>
{
    options.Mode = "SqlServer";
    options.SqlServer.ConnectionString = builder.Configuration.GetConnectionString("CroniqAuth")!;
});
```

## 3. Key Environment Variables

See [`auth.md`](/guides/auth.md) for the end-to-end authentication story and when to prefer API keys vs OAuth2/OIDC.

| Variable                                           | Required                                                          | Description                                                                                | Example                                            |
| -------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | -------------------------------------------------- |
| `Croniq__Auth__Mode`                               | Yes                                                               | Selects `InMemory` (single API key) or `SqlServer` (database-backed) authentication.       | `SqlServer`                                        |
| `Croniq__Auth__InMemory__ApiKey`                   | When `Auth.Mode = InMemory`                                       | API key issued to callers when using the in-memory store.                                  | `crq_dev_local_sample`                             |
| `Croniq__Auth__SqlServer__ConnectionString`        | When `Auth.Mode = SqlServer` and no shared connection is provided | Database connection used for issuing/validating API keys.                                  | `Server=.;Database=Croniq;Trusted_Connection=True` |
| `Croniq__Persistence__Mode`                        | Yes                                                               | `InMemory` for demo workloads or `SqlServer` for durable persistence.                      | `SqlServer`                                        |
| `Croniq__Persistence__SqlServer__ConnectionString` | When `Persistence.Mode = SqlServer`                               | Connection string for the scheduler persistence schema.                                    | `Server=.;Database=Croniq;Trusted_Connection=True` |
| `Croniq__SqlServer__ConnectionString`              | Optional                                                          | Shared fallback connection string used when specific auth/persistence strings are omitted. | `Server=.;Database=Croniq;...`                     |
| `Croniq__Core__TenantId`                           | Optional                                                          | Overrides the default tenant id baked into job keys.                                       | `dev-sandbox`                                      |
| `Croniq__Core__EnvironmentTag`                     | Optional                                                          | Distinguishes environments/instances (helps multi-dev setups).                             | `dev-alice`                                        |
| `Croniq__Api__RequestsPerMinute`                   | Optional                                                          | Per-key fixed-window rate limit enforced by `AddCroniqApiRateLimiter`.                     | `120`                                              |

> **Tip:** Keep secrets (API keys, connection strings) outside source control. Prefer user-secrets for local development and a managed vault for hosted environments.

## 4. Authentication Modes

Croniq keeps authentication pluggable so you can start with a single API key and grow into OAuth2/OIDC without touching application code. Pick the mode that matches your caller profile, then set the corresponding configuration keys.

### API Keys (machines / automation)

- `Croniq__Auth__Mode=InMemory` issues a single shared key defined by `Croniq__Auth__InMemory__ApiKey`. Best for local dev and tests.
- `Croniq__Auth__Mode=SqlServer` stores hashed keys via `Croniq.Auth.SqlServer`. Provide either `Croniq__Auth__SqlServer__ConnectionString` or the shared `Croniq__SqlServer__ConnectionString`.
- Issue, rotate, and revoke keys through the admin HTTP endpoints (`/tenants/{tenantId}/api-keys/**`) or by calling `IApiKeyStore` from a bootstrap script. Croniq only shows the plaintext secret on creation/rotation, so capture it immediately.
- Callers send `X-Croniq-Key: <plaintext-secret>`; the middleware turns it into an `ICallerContext` enriched with TenantId, EnvironmentTag, and scopes.

Quick sample (PowerShell):

```powershell
$Env:Croniq__Auth__Mode = "SqlServer"
$Env:Croniq__Auth__SqlServer__ConnectionString = "Server=.;Database=CroniqAuth;Trusted_Connection=True"
$Env:Croniq__Core__TenantId = "prod"
$Env:Croniq__Core__EnvironmentTag = "prod-cluster"
```

### OAuth2 / OIDC (interactive callers)

- Enable by setting `Croniq__Auth__Oidc__Enabled=true` in addition to `Croniq__Auth__Mode=SqlServer` (API keys can stay on for hybrid setups).
- Required fields: `Authority`, `Audience`, `TenantClaim`, optional `EnvironmentClaim`, and `RequiredScopes`.
- Croniq validates bearer tokens using the issuer's JWKS metadata and enforces the configured scopes **before** routing the request.

Sample JSON snippet:

```json
"Croniq": {
    "Auth": {
        "Mode": "SqlServer",
        "Oidc": {
            "Enabled": true,
            "Authority": "https://login.microsoftonline.com/<tenant>",
            "Audience": "api://cronq",
            "TenantClaim": "tid",
            "EnvironmentClaim": "env",
            "RequiredScopes": [ "cronq.api" ]
        }
    }
}
```

### Mixed Mode & Scope Mapping

- You can keep both modes enabled: Croniq checks `Authorization: Bearer ...` first, then falls back to `X-Croniq-Key`. Only one caller context is created per request.
- Map scopes to REST permissions (e.g., `schedules:write`, `jobs:trigger`, `api-keys:manage`). When callers lack a scope, Croniq returns `403 insufficient-scope`.
- For a deeper walkthrough (including sample IdP setups), jump to [`guides/auth.md`](/guides/auth.md) or the security deep dive.

## 5. Sample Local Setup

```cmd
set Croniq__Auth__Mode=InMemory
set Croniq__Auth__InMemory__ApiKey=crq_dev_local_sample
set Croniq__Core__TenantId=dev-sandbox
set Croniq__Core__EnvironmentTag=dev-alice
set Croniq__Api__RequestsPerMinute=60
```

To point both persistence and auth at the same SQL Server via the shared settings:

```powershell
$Env:Croniq__Auth__Mode = "SqlServer"
$Env:Croniq__Persistence__Mode = "SqlServer"
$Env:Croniq__SqlServer__ConnectionString = "Server=localhost;Database=Croniq;User Id=sa;Password=Secret123!"
```

## 6. Programmatic Overrides

When you need per-tenant or per-cluster customization, hook into the options pipeline instead of inventing new configuration entry points:

```csharp
builder.Services.AddCroniqApiServices(builder.Configuration);

builder.Services.PostConfigure<CroniqOptions>(options =>
{
    options.TenantId = tenantResolver.Resolve();
    options.EnvironmentTag = hostEnvironment.EnvironmentName;
});

builder.Services.PostConfigure<CroniqPersistenceOptions>(options =>
{
    if (options.Mode.Equals("SqlServer", StringComparison.OrdinalIgnoreCase))
    {
        options.SqlServer.ConnectionString = secretProvider.GetConnectionString("CroniqPersistence");
    }
});
```

Only override the values you truly need—everything else continues to flow from configuration files or environment variables.

## 7. Troubleshooting

- **Missing connection string:** When either `Auth.Mode` or `Persistence.Mode` is `SqlServer`, the extension throws if it cannot find a connection string on the domain-specific section or the shared `Croniq__SqlServer__ConnectionString` key.
- **Missing API key:** When `Auth.Mode = InMemory`, you must provide `Croniq__Auth__InMemory__ApiKey`. Otherwise startup throws `InvalidOperationException`.
- **Unexpected tenant scope:** Verify `Croniq__Core__TenantId`/`EnvironmentTag` when multiple developers work on the same database to avoid job collisions.
- **Rate limiter rejecting calls:** Increase `Croniq__Api__RequestsPerMinute` or tailor the limiter via `AddCroniqApiRateLimiter` options.

Need a bigger checklist? Jump to [`troubleshooting.md`](/ops/troubleshooting.md) for Docker/dev-stack, observability, and CLI-specific fixes.

## 8. Next Steps

- Return to the [Quickstart](/introduction/quickstart.md) to continue the walkthrough.
- Consult `docs/deep-dive/job-registration.md` (upcoming) for the in-depth view on how the runtime persists job metadata during startup.
- Switch to [`auth.md`](/guides/auth.md) when you need detailed guidance on caller flows and secret rotation.
- Keep [`troubleshooting.md`](/ops/troubleshooting.md) handy when startup or dev stack issues block you.
