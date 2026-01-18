# Croniq Configuration Guide

This guide explains how the Croniq API host (`Croniq.Api`) resolves configuration when you call `AddCroniqApiServices(...)` and `UseCroniqApi(...)`. It also lists the environment variables you typically need during local development or deployments. If you still need to pick an auth mode or understand how callers authenticate, start with the consumer guide in [`auth.md`](../guides/auth.md).

## 1. Configuration Sources & Priority

`builder.Services.AddCroniqApiServices(builder.Configuration)` wires API + platform services and binds option objects from these configuration sections:

- `Croniq:Api` (`CroniqApiOptions`)
- `Croniq:Core` (`CroniqOptions`)
- `Croniq:Auth` (`CroniqAuthOptions`), plus `Croniq:Auth:Tokens`, `Croniq:Auth:Password`, `Croniq:Auth:Oidc`
- `Croniq:Persistence` (`CroniqPersistenceOptions`)
- `Croniq:SqlServer` / `Croniq:Postgres` (`SqlServerOptions`, `PostgresOptions`)
- `Croniq:Startup`, `Croniq:JobRegistrySync`
- `Croniq:Policies:Misfire`, `Croniq:Policies:Execution`, `Croniq:Policies:Overrides`
- `Croniq:Webhooks:Ingress` (webhook ingress stream options)

The usual ASP.NET Core precedence applies (later providers win):

1. `appsettings.json` and other JSON files.
2. Environment-specific JSON (e.g., `appsettings.Development.json`).
3. Environment variables (`Croniq__Section__Property` notation with double underscores).
4. Secret providers such as `dotnet user-secrets`, Azure Key Vault, or custom `IConfiguration` sources.

`.env` files are not loaded automatically by Croniq. In this repo, `.env` is consumed by scripts/containers and uses `CRONIQ_*` keys that are mapped to the `Croniq__...` environment variables shown in the examples below.

For API hosts, `AddCroniqApiServices(...)` stays configuration-first. For worker-only hosts, Croniq exposes `AddCroniq(...)` (package `Croniq`) or `AddCroniqWorkerServices(...)` (package `Croniq.Hosting`) which bind the `Croniq:*` sections and apply defaults.

## 2. Registration Quick Reference

```csharp
var builder = WebApplication.CreateBuilder(args);

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();     // optional but recommended

var app = builder.Build();
app.UseCroniqApi();
app.Run();
```

Worker-only hosts use the slimmer facade:

```csharp
var builder = Host.CreateApplicationBuilder(args);
builder.Services.AddCroniq(); // or AddCroniqWorkerServices(builder.Configuration)
```

Need to override something programmatically? Use the regular options APIs before building the app:

```csharp
builder.Services.PostConfigure<CroniqAuthOptions>(options =>
{
    options.Mode = "SqlServer";
    options.SqlServer.ConnectionString = builder.Configuration.GetConnectionString("CroniqAuth")!;
});
```

## 3. Job Assembly Loading (Optional)

Production container images and shared hosts can register jobs from assemblies specified in configuration.

Supported keys:

- `Croniq:Jobs:Assemblies` (array of assembly names or file paths)
- `Croniq:Jobs:IncludeEntryAssembly` (bool)

Examples:

::: code-group

```json [appsettings.json]
{
  "Croniq": {
    "Jobs": {
      "Assemblies": ["./jobs/Acme.Jobs.dll"]
    }
  }
}
```

```dotenv [.env (compose)]
CRONIQ_JOBS_ASSEMBLIES_0=./jobs/Acme.Jobs.dll
```

```powershell [PowerShell]
$Env:Croniq__Jobs__Assemblies__0 = "C:\\croniq\\jobs\\Acme.Jobs.dll"
```

:::

You can also provide a semicolon-delimited list in a single env var:

```powershell
$Env:Croniq__Jobs__Assemblies = "/app/jobs/Acme.Jobs.dll;/app/jobs/Acme.Billing.Jobs.dll"
```

Host helper:

```csharp
builder.Services.AddCroniqJobsFromConfiguration(builder.Configuration);
```

## 4. Trigger Seeding (Worker Hosts)

Worker hosts can seed triggers on startup so schedules exist without manual API calls. This runs when you use `AddCroniq(...)` (package `Croniq`) or `AddCroniqWorkerServices(...)` (package `Croniq.Hosting`).

Seeding mode:

- `Croniq:Seeding:Mode = Off` disables seeding entirely.
- `Croniq:Seeding:Mode = CreateIfMissing` (default) creates triggers only if they are not present.
- `Croniq:Seeding:Mode = ForceUpdate` updates existing triggers **only** when they are marked `managedBy` (either the `ManagedBy` property or `metadata.managedBy`).

Configure triggers via `Croniq:Triggers` as a list:

```json
{
  "Croniq": {
    "Seeding": { "Mode": "CreateIfMissing" },
    "Triggers": [
      {
        "TriggerId": "samples-smoke-every-5s",
        "JobKey": "samples:smoke",
        "CronExpression": "0/5 * * * * ?",
        "StartAtUtc": "2025-01-01T00:00:00Z",
        "Enabled": true,
        "ManagedBy": "Croniq.Sample",
        "Metadata": { "seededBy": "Croniq.Sample" }
      }
    ]
  }
}
```

Or via a map keyed by trigger id:

```json
{
  "Croniq": {
    "Triggers": {
      "samples-smoke-every-5s": {
        "JobKey": "samples:smoke",
        "CronExpression": "0/5 * * * * ?",
        "ManagedBy": "Croniq.Sample"
      }
    }
  }
}
```

Invalid cron expressions, invalid job keys, or missing job registrations fail fast on startup and log a readable summary.

Prefer config for shared environments and use the fluent builder for inline setup:

```csharp
builder.Services
    .AddCroniq()
    .AddCroniqJob("samples", "smoke", (context, _) =>
    {
        context.Logger.LogInformation("Hello from {JobKey}", context.JobKey);
        return Task.CompletedTask;
    })
    .AddTrigger("0/5 * * * * ?", trigger =>
    {
        trigger.ManagedBy = "Croniq.Sample";
    });
```

### Job Registry Sync (Optional)

Job registry sync persists the jobs registered in `IJobRegistry` into the persistence store so UIs or operators can see them even when no schedules exist yet. It never creates triggers or deletes jobs.

Modes:

- `Croniq:JobRegistrySync:Mode = Off` disables sync (default).
- `Croniq:JobRegistrySync:Mode = CreateIfMissing` upserts only when a job is missing.
- `Croniq:JobRegistrySync:Mode = ForceUpdate` updates existing jobs **only** when their `metadata.managedBy` matches the configured `ManagedBy` value.

Example:

```json
{
  "Croniq": {
    "JobRegistrySync": {
      "Mode": "CreateIfMissing",
      "ManagedBy": "Croniq.Sample"
    }
  }
}
```

Use a stable `ManagedBy` value (not instance-id based) to avoid flapping in rolling deployments.

## 5. Key Environment Variables

See [`auth.md`](../guides/auth.md) for the end-to-end authentication story and when to prefer API keys vs bearer tokens.

| Variable                                           | Required                                                          | Description                                                                                          | Example                                                                |
| -------------------------------------------------- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `Croniq__Auth__Mode`                               | Yes                                                               | Selects `InMemory`, `SqlServer`, or `Postgres` (database-backed) authentication.                     | `SqlServer`                                                            |
| `Croniq__Auth__InMemory__ApiKey`                   | When `Auth.Mode = InMemory`                                       | API key issued to callers when using the in-memory store.                                            | `crq_dev_local_sample`                                                 |
| `Croniq__Auth__SqlServer__ConnectionString`        | When `Auth.Mode = SqlServer` and no shared connection is provided | Database connection used for issuing/validating API keys.                                            | `Server=.;Database=Croniq;Trusted_Connection=True`                     |
| `Croniq__Auth__Postgres__ConnectionString`         | When `Auth.Mode = Postgres` and no shared connection is provided  | Database connection used for issuing/validating API keys.                                            | `Host=localhost;Database=Croniq;Username=postgres;Password=Secret123!` |
| `Croniq__Persistence__Mode`                        | Yes                                                               | `InMemory` for demo workloads or `SqlServer`/`Postgres` for durable persistence.                     | `SqlServer`                                                            |
| `Croniq__Persistence__SqlServer__ConnectionString` | When `Persistence.Mode = SqlServer`                               | Connection string for the scheduler persistence schema.                                              | `Server=.;Database=Croniq;Trusted_Connection=True`                     |
| `Croniq__Persistence__Postgres__ConnectionString`  | When `Persistence.Mode = Postgres`                                | Connection string for the scheduler persistence schema.                                              | `Host=localhost;Database=Croniq;Username=postgres;Password=Secret123!` |
| `Croniq__SqlServer__ConnectionString`              | Optional                                                          | Shared fallback connection string used when SqlServer-specific auth/persistence strings are omitted. | `Server=.;Database=Croniq;...`                                         |
| `Croniq__Postgres__ConnectionString`               | Optional                                                          | Shared fallback connection string used when Postgres-specific auth/persistence strings are omitted.  | `Host=localhost;Database=Croniq;...`                                   |
| `Croniq__Postgres__SearchPath`                     | Optional                                                          | Comma-delimited Postgres search path applied to the connection.                                      | `croniq,auth,public`                                                   |
| `Croniq__Core__TenantId`                           | Optional                                                          | Tenant id used for partitioning/scoping. Default is `default`.                                       | `dev-sandbox`                                                          |
| `Croniq__Core__TenantMode`                         | Optional                                                          | Informational only (does not change tenant resolution). Default is `Single`.                         | `Multi`                                                                |
| `Croniq__Core__EnvironmentTag`                     | Optional                                                          | Distinguishes environments/instances (helps multi-dev setups).                                       | `dev-alice`                                                            |
| `Croniq__Api__RequestsPerMinute`                   | Optional                                                          | Per-key fixed-window rate limit enforced by `AddCroniqApiRateLimiter`. Default `60`.                 | `60`                                                                   |
| `Croniq__Api__ForwardedHeaders__Enabled`           | Optional                                                          | Enables `X-Forwarded-For`/`X-Forwarded-Proto` processing from trusted proxies. Default `false`.      | `true`                                                                 |
| `Croniq__Api__ForwardedHeaders__ForwardLimit`      | Optional                                                          | Max number of forwarded entries to accept. Default `1`.                                              | `2`                                                                    |
| `Croniq__Api__ForwardedHeaders__KnownNetworks__0`  | When `ForwardedHeaders.Enabled = true`                            | CIDR for trusted proxy networks (add indexes for more).                                              | `10.0.0.0/8`                                                           |
| `Croniq__Api__ForwardedHeaders__KnownProxies__0`   | When `ForwardedHeaders.Enabled = true`                            | IP address for trusted proxies (add indexes for more).                                               | `192.168.1.10`                                                         |
| `Croniq__Observability__HashIdentifiers`           | Optional                                                          | Hashes tenant/caller identifiers in logs/metrics/traces (HMAC-SHA256). Default `false`.              | `true`                                                                 |
| `Croniq__Observability__IdentifierHashKey`         | When `HashIdentifiers = true`                                     | Secret key used for HMAC hashing of tenant/caller identifiers.                                       | `<secret>`                                                             |

> **Tip:** Keep secrets (API keys, connection strings) outside source control. Prefer user-secrets for local development and a managed vault for production environments.

### Forwarded headers (reverse proxy)

When running `Croniq.Api` behind a reverse proxy, enable forwarded headers and declare the proxy IPs or networks to trust. If you enable this without any known proxy entries, Croniq only accepts forwarded headers from loopback and logs a warning.

Examples:

::: code-group

```json [appsettings.json]
{
  "Croniq": {
    "Api": {
      "ForwardedHeaders": {
        "Enabled": true,
        "ForwardLimit": 2,
        "KnownNetworks": ["10.0.0.0/8"],
        "KnownProxies": ["192.168.1.10"]
      }
    }
  }
}
```

```dotenv [.env (compose)]
CRONIQ_API_FORWARDED_HEADERS_ENABLED=true
CRONIQ_API_FORWARDED_HEADERS_FORWARD_LIMIT=2
CRONIQ_API_FORWARDED_HEADERS_KNOWN_NETWORKS_0=10.0.0.0/8
CRONIQ_API_FORWARDED_HEADERS_KNOWN_PROXIES_0=192.168.1.10
```

```powershell [PowerShell]
$Env:Croniq__Api__ForwardedHeaders__Enabled = "true"
$Env:Croniq__Api__ForwardedHeaders__ForwardLimit = "2"
$Env:Croniq__Api__ForwardedHeaders__KnownNetworks__0 = "10.0.0.0/8"
$Env:Croniq__Api__ForwardedHeaders__KnownProxies__0 = "192.168.1.10"
```

:::

## 6. Authentication Modes

Croniq keeps authentication pluggable so you can start with a single API key and grow into bearer tokens without touching application code. Pick the mode that matches your caller profile, then set the corresponding configuration keys.

### API Keys (machines / automation)

- `Croniq__Auth__Mode=InMemory` issues a single shared key defined by `Croniq__Auth__InMemory__ApiKey`. Best for local dev and tests.
- `Croniq__Auth__Mode=SqlServer` stores hashed keys via `Croniq.Auth.SqlServer`. Provide either `Croniq__Auth__SqlServer__ConnectionString` or the shared `Croniq__SqlServer__ConnectionString`.
- `Croniq__Auth__Mode=Postgres` stores hashed keys via `Croniq.Auth.Postgres`. Provide either `Croniq__Auth__Postgres__ConnectionString` or the shared `Croniq__Postgres__ConnectionString`.
- Issue, rotate, and revoke keys through the admin HTTP endpoints (`/tenants/{tenantId}/api-keys/**`) or by calling `IApiKeyStore` from a bootstrap script. Croniq only shows the plaintext secret on creation/rotation, so capture it immediately.
- Callers send `X-Croniq-Key: <plaintext-secret>`; the middleware turns it into an `ICallerContext` enriched with TenantId, EnvironmentTag, and scopes.

Quick sample (PowerShell):

```powershell
$Env:Croniq__Auth__Mode = "SqlServer"
$Env:Croniq__Auth__SqlServer__ConnectionString = "Server=.;Database=CroniqAuth;Trusted_Connection=True"

$Env:Croniq__Auth__Mode = "Postgres"
$Env:Croniq__Auth__Postgres__ConnectionString = "Host=localhost;Database=CroniqAuth;Username=postgres;Password=Secret123!"
$Env:Croniq__Core__TenantId = "prod"
$Env:Croniq__Core__EnvironmentTag = "prod-cluster"
```

### Password Login (humans)

Croniq can optionally expose username/password endpoints for self-hosted deployments.

- Enable via `Croniq:Auth:Password:Enabled`.
- Callers must provide `tenantId`.

See [docs/deep-dive/password-auth.md](../deep-dive/password-auth.md) for the full flow.

### Mixed Mode & Scope Mapping

- You can keep both modes enabled: Croniq checks `Authorization: Bearer ...` first, then falls back to `X-Croniq-Key`. Only one caller context is created per request.
- Map scopes to REST permissions (e.g., `schedules:write`, `jobs:trigger`, `api-keys:manage`). When callers lack a scope, Croniq returns `403 insufficient-scope`.
- For a deeper walkthrough (including sample IdP setups), jump to [`guides/auth.md`](../guides/auth.md) or the security deep dive.

## 7. Sample Local Setup

::: code-group

```cmd [Windows cmd]
set Croniq__Auth__Mode=InMemory
set Croniq__Auth__InMemory__ApiKey=crq_dev_local_sample
set Croniq__Core__TenantId=dev-sandbox
set Croniq__Core__EnvironmentTag=dev-alice
set Croniq__Api__RequestsPerMinute=60
```

```dotenv [.env (compose)]
CRONIQ_AUTH_MODE=InMemory
CRONIQ_API_KEY=crq_dev_local_sample
CRONIQ_CORE_TENANT_ID=dev-sandbox
CRONIQ_ENVIRONMENT=dev-alice
CRONIQ_API_REQUESTS_PER_MINUTE=60
```

:::

To point both persistence and auth at the same database via the shared settings:

```powershell
# SqlServer
$Env:Croniq__Auth__Mode = "SqlServer"
$Env:Croniq__Persistence__Mode = "SqlServer"
$Env:Croniq__SqlServer__ConnectionString = "Server=localhost;Database=Croniq;User Id=sa;Password=Secret123!"

# Postgres
$Env:Croniq__Auth__Mode = "Postgres"
$Env:Croniq__Persistence__Mode = "Postgres"
$Env:Croniq__Postgres__ConnectionString = "Host=localhost;Database=Croniq;Username=postgres;Password=Secret123!"
```

## 8. Programmatic Overrides

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
    else if (options.Mode.Equals("Postgres", StringComparison.OrdinalIgnoreCase))
    {
        options.Postgres.ConnectionString = secretProvider.GetConnectionString("CroniqPersistence");
    }
});
```

Only override the values you truly need-everything else continues to flow from configuration files or environment variables.

## 9. Troubleshooting

- **Missing connection string:** When either `Auth.Mode` or `Persistence.Mode` is `SqlServer` or `Postgres`, the extension throws if it cannot find a connection string on the domain-specific section or the shared `Croniq__SqlServer__ConnectionString`/`Croniq__Postgres__ConnectionString` key.
- **Missing API key:** When `Auth.Mode = InMemory`, you must provide `Croniq__Auth__InMemory__ApiKey`. Otherwise startup throws `InvalidOperationException`.
- **Missing identifier hash key:** When `Croniq__Observability__HashIdentifiers=true`, you must provide `Croniq__Observability__IdentifierHashKey`.
- **Unexpected tenant scope:** Verify `Croniq__Core__TenantId`/`EnvironmentTag` when multiple developers work on the same database to avoid job collisions.
- **Rate limiter rejecting calls:** Increase `Croniq__Api__RequestsPerMinute` or tailor the limiter via `AddCroniqApiRateLimiter` options.

Need a bigger checklist? Jump to [`troubleshooting.md`](../ops/troubleshooting.md) for Docker/dev-stack, observability, and CLI-specific fixes.

## 10. Next Steps

- Return to the [Quickstart](./quickstart.md) to continue the walkthrough.
- Consult `docs/deep-dive/job-registration.md` for the in-depth view on how the runtime persists job metadata during startup.
- Switch to [`auth.md`](../guides/auth.md) when you need detailed guidance on caller flows and secret rotation.
- Keep [`troubleshooting.md`](../ops/troubleshooting.md) handy when startup or dev stack issues block you.
