# Croniq gRPC Guide

## Proto and client

- Proto: `src/Croniq.Rpc.Client/Protos/scheduler.proto`
- .NET client: `Croniq.Rpc.Client` provides `AddCroniqSchedulerClient` (HTTP/2, optional `X-Croniq-Key`) and safe wrappers (`*SafeAsync`) that throw `CroniqRpcException`.

### Minimal .NET usage

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
        JobKey = "ops:demo",
        CronExpression = "0/5 * * * * ?"
    });

    var trigger = await client.TriggerJobSafeAsync(new TriggerJobRequest { JobKey = upsert.JobKey });
}
catch (CroniqRpcException ex)
{
    Console.WriteLine($"gRPC error {ex.StatusCode}: {ex.Detail}");
}
```

## Auth and headers

- API key: send the `X-Croniq-Key` header (`AddCroniqSchedulerClient` sets it when `ApiKey` is configured).
- Bearer: send `Authorization: Bearer <token>`. Tenant/environment are resolved from claims; cross-tenant access is rejected with `PermissionDenied`.

## Proto semantics

- `UpsertScheduleRequest`: `job_key` (`namespace:name[:variant]`), `cron_expression`, optional `trigger_id`, `description`, `metadata`, optional `enabled`, optional `start_at_utc`, `end_at_utc`, `time_zone_id`.
- `DeleteScheduleRequest`: `trigger_id`, `tenant_id`, `environment_tag` are required.
- `TriggerJobRequest`: `job_key`, optional `metadata`.

## Non-.NET clients

- Use the proto with `protoc` or `buf` for Python/Go; send `X-Croniq-Key` or bearer metadata.
- Node/TS can use `@grpc/grpc-js` plus `@grpc/proto-loader` without pre-generation (see `samples/grpc-client-node`).
- Samples: `samples/grpc-client-python`, `samples/grpc-client-go`, `samples/grpc-client-node`.
- Recommended set: Python (scripting), Go (CLI/automation), Node/TS (serverless/edge). Add Java only when a consumer needs it; the same proto can be generated with `protoc --java_out`.
- Planned packages: light client bundles per language (PyPI, Go module, NPM) with generated stubs and minimal endpoint/auth/metadata helpers so samples can consume the package directly.

## Logging and telemetry

- gRPC routes use the same guards as HTTP and emit activities with tenant/environment/job tags via the `Croniq.Api.Grpc` ActivitySource. Trigger/Upsert/Delete set status and error, keeping OTel tracing consistent.
- Rate limiting is enforced by the gRPC interceptor (`TenantRateLimitInterceptor`) with the same partition IDs as the REST routes.

## Validation and CI hooks

- Quick syntax/build check: `eng/validate-grpc-samples.ps1` checks Node (syntax) and, when generated stubs exist, optionally builds Python/Go. Actual calls require a running `Croniq.Api` host (see the samples).

> **Learn more:** Review [architecture.md](../deep-dive/architecture.md) for the API/RPC surface and [security.md](../deep-dive/security.md) for gRPC guardrails and rate limiting.
