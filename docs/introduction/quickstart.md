# Croniq Quickstart: Hello Croniq Job

This guide walks you through creating your first Croniq job, registering it with the Scheduler, and triggering it via the Minimal API. It assumes you already reviewed the architectural context in [`/deep-dive/architecture.md`](../deep-dive/architecture.md).

> **Prerequisites**
>
> - .NET SDK `net10.0`
> - Access to the Croniq repository or NuGet feeds (once packages are published)
>
> All commands are shown for Windows PowerShell / CMD; adapt paths for your OS if needed.
>
> Still deciding how to secure the API? Read [`auth.md`](../guides/auth.md) for the consumer-facing guide to API keys vs OAuth2 before exposing your endpoints.

## 1. Create a Single Project

```cmd
mkdir HelloCroniq
cd HelloCroniq
dotnet new web -n HelloCroniq -f net10.0
```

This creates `HelloCroniq/HelloCroniq.csproj`, which will host both the Minimal API and your job implementations.

## 2. Reference Croniq Packages

Until official packages exist, you can reference local projects or NuGet prereleases.

```cmd
cd HelloCroniq
 dotnet add package Croniq --version LATEST_VERSION
 dotnet add package Croniq.Api --version LATEST_VERSION
```

> Always install the latest stable version (see `AGENTS.md`). For local development before packages exist, add project references to `src/Croniq` and `src/Croniq.Api` instead of NuGet packages.

## 3. Register a Job Class

Implement `IJob` and register it via `AddCroniqJobsFromEntryAssembly()` inside `HelloCroniq/Program.cs`:

```csharp
using Croniq;
using Croniq.Sdk;
using Croniq.Api;
using Microsoft.Extensions.Logging;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddCroniq(builder.Configuration); // worker defaults (InMemory job store + worker host)
builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();

builder.Services.AddCroniqJobsFromEntryAssembly();

var app = builder.Build();

app.UseCroniqApi();
app.Run();

[CroniqJob("samples", "HelloWorld")]
public sealed class HelloWorldJob : IJob
{
  public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken)
  {
    context.Logger.LogInformation("Hello from Croniq! JobKey={JobKey}", context.JobKey);
    return Task.CompletedTask;
  }
}
```

- `AddCroniq(...)` wires the worker runtime with safe defaults; drop the `configuration` argument if you want pure defaults.
- `AddCroniqApiServices(...)` exposes the management API. If you do not want to co-host, move API registration into a separate web host.
- Job keys are composed as `namespace:name[:variant]`. Tenant/environment scope comes from `Croniq:Core:*` and auth config, not the job key.

Prefer an inline handler and a seeded schedule instead of a job class + API call? Use the fluent registration:

```csharp
builder.Services
    .AddCroniq(builder.Configuration)
    .AddCroniqJob("samples", "HelloWorld", (context, _) =>
    {
        context.Logger.LogInformation("Hello from Croniq! JobKey={JobKey}", context.JobKey);
        return Task.CompletedTask;
    })
    .AddTrigger("0 * * * * ?", trigger =>
    {
        trigger.ManagedBy = "HelloCroniq";
    });
```

With `AddTrigger(...)` the schedule is seeded at startup, so you can skip the API call below if you prefer config/code-first schedules.

```cmd
# Terminal 1
cd HelloCroniq
dotnet run
```

```cmd
curl -X POST https://localhost:5001/tenants/default/schedules?environment=dev \
     -H "Content-Type: application/json" \
     -H "X-Croniq-Key: YOUR_DEV_KEY" \
     -d "{
           \"jobKey\": \"samples:HelloWorld\",
           \"cronExpression\": \"0 * * * * ?\",
           \"metadata\": { \"initiator\": \"quickstart\" }
         }"
```

Refer to `/deep-dive/persistence.md` for the exact schedule payload and validation rules.

## 4. Trigger Jobs via Webhooks (Optional)

Inbound webhooks let external systems trigger the job you just registered without touching the management API. For local development you can co-host the webhook ingress alongside the API by adding the `Croniq.Webhooks` package:

```cmd
 dotnet add package Croniq.Webhooks --version LATEST_VERSION
```

Register the services in `Program.cs` right after the existing Croniq calls:

```csharp
builder.Services.AddCroniqWebhookServices(builder.Configuration);
builder.Services.AddCroniqWebhookRateLimiter();

var app = builder.Build();

app.UseCroniqApi();
app.UseCroniqWebhooks(mapHealthEndpoints: false); // skip duplicate /health when co-hosted
```

`AddCroniqWebhookServices` automatically wires the persistence provider based on `Croniq:Webhooks:Mode` (InMemory for samples, SqlServer/Postgres when you reuse `Croniq:SqlServer` or `Croniq:Postgres`). Set `Croniq:Webhooks:ConfigurePersistence=false` when you need to register a custom `IWebhookPersistenceProvider` before calling `AddCroniqWebhookServices`.

Add a matching configuration block. Point the `JobKey` at the handler you registered above so the webhook reuses the same job key used by manual triggers:

```json
{
  "Croniq": {
    "Webhooks": {
      "RequestsPerMinute": 30,
      "Endpoints": [
        {
          "HookKey": "hello-world",
          "JobKey": "samples:HelloWorld",
          "Secret": "dev-webhook-secret",
          "RequireSignature": true,
          "Metadata": {
            "source": "quickstart"
          }
        }
      ]
    }
  }
}
```

Send a test request with an HMAC signature. PowerShell example (works in Windows Terminal or VS Code):

```powershell
$Payload = '{"invoiceId":"INV-42","amount":199.0}'
$Secret = 'dev-webhook-secret'
$KeyBytes = [System.Text.Encoding]::UTF8.GetBytes($Secret)
$BodyBytes = [System.Text.Encoding]::UTF8.GetBytes($Payload)
$Hmac = [System.Security.Cryptography.HMACSHA256]::new($KeyBytes)
$Signature = 'sha256=' + ([BitConverter]::ToString($Hmac.ComputeHash($BodyBytes)).Replace('-', '').ToLower())

Invoke-WebRequest -Uri "https://localhost:5001/tenants/default/environments/dev/webhooks/hello-world" `
  -Method Post `
  -ContentType "application/json" `
  -Headers @{ "X-Croniq-Signature" = $Signature } `
  -Body $Payload
```

`Croniq.Webhooks` recomputes the signature, enforces the rate limit, and invokes the job pipeline. You should see the same log output as the manual trigger, plus webhook-specific metadata (e.g., `payload:invoiceId=INV-42`).

If you want to trigger a webhook manually through the admin API (for example, from the UI), use `POST /tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}/invoke`.

When running DMZ ingress with remote persistence, keep `Croniq:Webhooks:Remote:EnableRelay=true` only on the worker host (the one that has job registrations). The API host still needs the remote config for management endpoints, but should not run the relay.

> **Learn more:** The deep dive section [`Webhook Trigger Surface`](../deep-dive/architecture.md#webhook-trigger-surface) explains how the dedicated `Croniq.Webhooks` host scales independently, how persistence works, and what security constraints apply in production.

### Rotate webhook secrets from the CLI

Persisted hooks should rotate their secrets regularly. Instead of crafting raw HTTP calls, run the helper under `scripts/webhook-rotate-secret.ps1` (PowerShell or VS Code terminal):

```powershell
scripts/webhook-rotate-secret.ps1 `
  -TenantId tenant-samples `
  -Environment dev `
  -HookKey hello-world `
  -GracePeriodSeconds 3600 `
  -Notes "rotated during quickstart"
```

Pass `-ActivateInSeconds SECONDS` (up to seven days) when you need to stage the new secret before callers switch over. The script prints the activation window plus the plaintext secret-capture it immediately because Croniq never stores or returns it again.

## 4.1 Publish API & gRPC Schemas

Croniq hosts both Minimal API endpoints (`/tenants/{tenantId}/schedules`, `/jobs/trigger`, `/tenants/{tenantId}/webhooks` for management, `/tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}` for ingress) and the Scheduler gRPC surface defined in [src/Croniq.Rpc.Client/Protos/scheduler.proto](../../src/Croniq.Rpc.Client/Protos/scheduler.proto). Expose their schemas so downstream teams can generate clients without reverse engineering requests.

Add the recommended tooling to your host:

```cmd
dotnet add package Swashbuckle.AspNetCore
dotnet add package Grpc.AspNetCore.Server.Reflection
```

Then light up OpenAPI + Swagger UI for the Minimal API layer and gRPC reflection for SDK consumers:

```csharp
builder.Services.AddEndpointsApiExplorer();
builder.Services.AddSwaggerGen();
builder.Services.AddGrpcReflection();

var app = builder.Build();

if (app.Environment.IsDevelopment()
    || builder.Configuration.GetValue("Croniq:Api:ExposeSchemas", false))
{
  app.UseSwagger();
  app.UseSwaggerUI(options =>
  {
    options.SwaggerEndpoint("/swagger/v1/swagger.json", "Croniq Scheduler API v1");
    options.DisplayRequestDuration();
  });

  app.MapGrpcReflectionService();
}

app.UseCroniqApi();
```

- Default the exposure to local/dev environments and gate it in production via `Croniq:Api:ExposeSchemas` so the management plane stays private.
- When you need a signed artifact for consumers, install the Swashbuckle CLI (`dotnet tool install --global Swashbuckle.AspNetCore.Cli`) and run `dotnet swagger tofile --output docs/public/api/croniq-api-v1.json YourApp.dll v1` as part of CI.
- Check the `.proto` file into your docs (or push it to a Buf registry) whenever the gRPC contract changes so language-specific clients can regenerate strongly typed stubs without hitting your environment.

Schema exposure configuration:

::: code-group

```json [appsettings.Development.json]
{
  "Croniq": {
    "Api": {
      "ExposeSchemas": true
    }
  }
}
```

```dotenv [.env (compose)]
CRONIQ_API_EXPOSE_SCHEMAS=true
```

```powershell [PowerShell]
$Env:Croniq__Api__ExposeSchemas = "true"
```

:::

## 5. Run Everything

```cmd
# Terminal 1
cd HelloCroniq
dotnet run
```

Trigger the job manually:

```cmd
curl -X POST https://localhost:5001/jobs/trigger \
     -H "Content-Type: application/json" \
     -H "X-Croniq-Key: YOUR_DEV_KEY" \
     -d "{
           \"jobKey\": \"samples:HelloWorld\",
           \"metadata\": { \"initiator\": \"manual\" }
         }"
```

Watch the API logs; you should see the `HelloWorldJob` message. Logs, metrics, and traces are emitted via Serilog + OpenTelemetry as soon as you point Croniq at an OTLP collector (details below).

## 6.1 Add Observability (Optional but Recommended)

1. Ensure you have an OTLP collector running (self-hosted Grafana/Tempo/Prometheus, a managed collector, or your existing observability stack).

2. Configure your quickstart host to export telemetry. The `AddCroniqObservability` helper reads `Croniq:Observability` settings, so add them to `appsettings.Development.json` or export environment variables before running `dotnet run`:

   ::: code-group

   ```json [appsettings.Development.json]
   {
     "Croniq": {
       "Observability": {
         "OtlpEndpoint": "http://localhost:4317",
         "OtlpProtocol": "grpc"
       },
       "Core": {
         "TenantId": "default",
         "EnvironmentTag": "dev"
       }
     }
   }
   ```

   ```dotenv [.env (compose)]
   CRONIQ_OBS_OTLP_ENDPOINT=http://localhost:4317
   CRONIQ_OBS_OTLP_PROTOCOL=grpc
   CRONIQ_CORE_TENANT_ID=default
   CRONIQ_ENVIRONMENT=dev
   ```

   ```powershell [PowerShell]
   $Env:Croniq__Observability__OtlpEndpoint = "http://localhost:4317"
   $Env:Croniq__Observability__OtlpProtocol = "grpc"
   $Env:Croniq__Core__EnvironmentTag = "dev"
   $Env:Croniq__Core__TenantId = "default"
   ```

   :::

   Restart the application so the new settings take effect. The defaults use OTLP gRPC; override them only when your collector endpoint differs.

3. Trigger your job again. Within a few seconds you can:
   - Open Grafana > Dashboards > _Croniq Scheduler Health_ or _Croniq API Gateway_ to view the panels provisioned from `infra/docker/observability/grafana/dashboards/` (they refresh every 30s).
   - Inspect traces under Grafana > Explore > Tempo, filtering by `service.name="Croniq.Api"`.
   - Check Prometheus > Alerts to see the built-in alerts from `infra/monitoring/rules/scheduler-alerts.yaml`. Alerts fire when dead letters, misfires, queue depth, or latency breach their thresholds (`CroniqDeadLettersHigh`, `CroniqMisfireBurst`, `CroniqQueueDepthHigh`, `CroniqLatencyP95High`, `CroniqJobFailures`).

Deploying to your own observability stack? Copy the dashboard JSON + rule file into your Grafana/Prometheus setup and keep the datasource UIDs (`prometheus`, `tempo`) consistent. See [`docs/deep-dive/observability.md`](../deep-dive/observability.md#dashboards--alerts) for detailed instructions.

## 6. Clean Up & Next Steps

- Stop the API host when you're done testing.
- Extend the job to read secrets via `ISecretProvider`, add retry policies, or push telemetry to your observability stack.
- Continue with:
  - [`configuration.md`](./configuration.md) for tenant/environment options
  - [`auth.md`](../guides/auth.md) to switch between API key and password login
  - [`policies.md`](../guides/policies.md) and [`triggers.md`](../guides/triggers.md) to tune job behavior
  - `/deep-dive/job-registration.md` for internal startup flow and persistence sync details
- Hit [`troubleshooting.md`](../ops/troubleshooting.md) if any of the steps above fail.

Happy scheduling! Translate findings back into the documentation as you refine the workflow.
