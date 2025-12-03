# Croniq Configuration Guide

This guide explains how the Croniq client components (e.g., `Croniq.Api`) resolve configuration values during `AddCroniq(...)` and which environment variables you should provide.

## 1. Configuration Sources & Priority

`builder.Services.AddCroniq()` loads settings in the following order (later items overwrite earlier ones):

1. Default values from `appsettings.json` / strongly typed options.
2. Environment variables prefixed with `CRONIQ_`.
3. Secret providers (e.g., `dotnet user-secrets`, Azure Key Vault, etc.) wired through `ISecretProvider`.

If you need full manual control, you can still pass a configuration delegate: `builder.Services.AddCroniq(options => { ... })`. In most cases the zero-argument overload is enough when environment variables are present.

## 2. `AddCroniq` Quick Reference

```csharp
builder.Services.AddCroniq();                    // rely on CRONIQ_* env vars
builder.Services.AddCroniq(options =>            // explicit overrides
{
    options.Endpoint = new Uri("https://api.croniq.cloud");
    options.ApiKey = configuration["Croniq:ApiKey"]!;
    options.TenantId = "dev-sandbox";           // optional override
    options.EnvironmentTag = "dev-alice";       // optional override
});
```

Use the delegate overload only when you need different behavior than the defaults gathered from environment variables or secret providers.

## 3. Supported Environment Variables

| Variable            | Required | Description                                                                                     | Example                         |
|---------------------|----------|-------------------------------------------------------------------------------------------------|---------------------------------|
| `CRONIQ_ENDPOINT`   | Yes      | Base URL of the Croniq management endpoint (cloud or self-hosted).                              | `https://api.croniq.cloud`      |
| `CRONIQ_API_KEY`    | Yes      | API key used for authenticating management calls.                                               | `crq_live_xxx`                  |
| `CRONIQ_TENANT`     | Optional | Logical tenant identifier. Overrides server-side defaults if provided.                         | `dev-sandbox`                   |
| `CRONIQ_ENV`        | Optional | Environment/instance tag (e.g., developer name, deployment stage).                              | `dev-alice`                     |
| `CRONIQ_CLUSTER`    | Optional | Named cluster profile (if you operate multiple scheduler clusters).                              | `shared-dev`                    |
| `CRONIQ_LOG_LEVEL`  | Optional | Overrides default log level for Croniq components (`Information`, `Debug`, etc.).                | `Debug`                         |

> **Tip:** Keep API keys outside source control. For local development, store values in `dotnet user-secrets` or an `.env` file consumed by your host.

## 4. Sample Local Setup

```cmd
set CRONIQ_ENDPOINT=https://localhost:5001
set CRONIQ_API_KEY=crq_dev_local_sample
set CRONIQ_TENANT=dev-sandbox
set CRONIQ_ENV=dev-alice
```

For PowerShell:

```powershell
$Env:CRONIQ_ENDPOINT = "https://localhost:5001"
$Env:CRONIQ_API_KEY = "crq_dev_local_sample"
```

Or use user-secrets:

```cmd
cd HelloCroniq.Api
dotnet user-secrets set "Croniq:Endpoint" "https://localhost:5001"
dotnet user-secrets set "Croniq:ApiKey" "crq_dev_local_sample"
```

## 5. Explicit Overrides

If you need to override values programmatically (e.g., multi-tenant SaaS host), you can pass a delegate:

```csharp
builder.Services.AddCroniq(options =>
{
    options.Endpoint = new Uri(env["CRONIQ_ENDPOINT"]!);
    options.ApiKey = secretProvider.Get("croniq-key");
    options.TenantId = tenantProvider.ResolveTenant();
    options.EnvironmentTag = hostEnvironment.EnvironmentName;
});
```

Only specify the values you actually need to override; everything else continues to flow from environment variables.

## 6. Troubleshooting

- **Missing API key**: `AddCroniq` throws an exception if `CRONIQ_API_KEY` is absent. Verify that the environment variable is defined for the process user or supply a configuration delegate.
- **Wrong endpoint**: ensure the URL uses HTTPS for cloud deployments. For local testing with self-hosted Croniq, use `https://localhost:<port>` and trust the development certificate.
- **Tenant collisions**: when multiple developers share the same database, set unique `CRONIQ_ENV` values so job registrations remain isolated (see `CONCEPT.md` section 5).

## 7. Next Steps

- Return to the [Quickstart](quickstart.md) to continue the walkthrough.
- Consult `docs/technical/job-registration.md` (upcoming) for the in-depth view on how the runtime persists job metadata during startup.
