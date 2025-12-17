# Croniq gRPC Guide

## Proto & Client

- Proto: `src/Croniq.Rpc.Client/Protos/scheduler.proto`
- .NET Client: `Croniq.Rpc.Client` bietet `AddCroniqSchedulerClient` (HTTP/2, optional `X-Croniq-Key`) und Safe-Wrapper (`*SafeAsync`) mit `CroniqRpcException`.

### Minimal .NET Usage

```csharp
var services = new ServiceCollection();
services.AddCroniqSchedulerClient(opts =>
{
    opts.Endpoint = "http://localhost:5080";
    opts.ApiKey = "<your-key>";
});
var client = services.BuildServiceProvider().GetRequiredService<Scheduler.SchedulerClient>();

try
{
    var upsert = await client.UpsertScheduleSafeAsync(new UpsertScheduleRequest
    {
        JobKey = "tenant:dev:ops:demo",
        CronExpression = "0/5 * * * * ?"
    });

    var trigger = await client.TriggerJobSafeAsync(new TriggerJobRequest { JobKey = upsert.JobKey });
}
catch (CroniqRpcException ex)
{
    Console.WriteLine($"gRPC error {ex.StatusCode}: {ex.Detail}");
}
```

## Auth & Headers

- API-Key: send `X-Croniq-Key` header (AddCroniqSchedulerClient setzt ihn, wenn `ApiKey` hinterlegt ist).
- Bearer: send `Authorization: Bearer <token>`. Tenant/Environment werden aus Claims gezogen; Cross-Tenant wird mit `PermissionDenied` geblockt.

## Proto Semantik

- `UpsertScheduleRequest`: `job_key` (Tenant:Env:Namespace:Job[:Variant]), `cron_expression`, optional `trigger_id`, `description`, `metadata`, optional `enabled`.
- `DeleteScheduleRequest`: `trigger_id`, `tenant_id`, `environment_tag` erforderlich.
- `TriggerJobRequest`: `job_key`, optional `metadata`.

## Non-.NET Clients

- Verwende die Proto mit `protoc`/`buf` für Python/Go; sende `X-Croniq-Key` oder Bearer-Metadaten.
- Node/TS kann via `@grpc/grpc-js` + `@grpc/proto-loader` ohne Vorab-Generation arbeiten (siehe `samples/grpc-client-node`).
- Samples: `samples/grpc-client-python`, `samples/grpc-client-go`, `samples/grpc-client-node`.
- Empfohlenes Set: Python (Scripting), Go (CLI/Automation), Node/TS (Serverless/Edge). Java nur bei Bedarf der Konsumenten; gleiche Proto kann via `protoc --java_out` generiert werden.
- Geplante Pakete: Leichte Client-Bundles pro Sprache (Python/PyPI, Go/Go module, Node/NPM) mit generierten Stubs + Minimal-Helper für Endpoint/Auth/Metadata, damit Samples nur noch das Paket konsumieren müssen.

## Logging & Telemetry

- gRPC-Routen nutzen dieselben Guards wie HTTP und instrumentieren Aktivitäten mit Tenant/Environment/Job-Tags (`Croniq.Api.Grpc` ActivitySource). Trigger/Upsert/Delete setzen Status/Error, sodass OTel/Tracing-Dashboards konsistent bleiben.
- Rate-Limiter greift per gRPC-Interceptor (`TenantRateLimitInterceptor`) mit den gleichen Partition-IDs wie die REST-Route.

## Validierung / CI-Hooks

- Schneller Syntax-/Build-Check: `eng/validate-grpc-samples.ps1` prüft Node (Syntax) und, falls generierte Stubs vorliegen, optional Python/Go Builds. Die eigentlichen Calls benötigen einen laufenden Croniq.Api Host (siehe Samples).
