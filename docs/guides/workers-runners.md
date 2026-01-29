# Workers & Runners (gRPC + HTTP)

Croniq.WorkerHost uses a lease-based model to claim and execute due triggers.
Runners use gRPC streaming as the primary transport with HTTP polling as a fallback; both share the same lease lifecycle so non-.NET runners can participate.
Worker host presence is tracked separately via `/workers`; this guide focuses on runner identities used by the `/work/*` surface.
For protocol details, see [`polyglot-runner-protocol.md`](../deep-dive/designs/polyglot-runner-protocol.md).

## Worker Presence (Heartbeat)

Worker hosts publish heartbeats to `/tenants/{tenantId}/workers/heartbeat` and are listed via `/tenants/{tenantId}/workers`.
Heartbeat metadata (`metadataJson`) is optional and currently carries host identity plus dispatch status:

- `kind`: `"worker"`
- `hostname`: machine name
- `dispatch.grpcConnected`: `true` when the gRPC dispatch stream is connected
- `dispatch.lastConnectedAtUtc`: ISO timestamp of the last successful gRPC connection
- `dispatch.lastFallbackAtUtc`: ISO timestamp of the last fallback polling window

The Workers UI reads this metadata to surface gRPC vs fallback state. Presence is informational; it does not affect lease correctness.

## Authentication & Scoping

All work endpoints:

- Require a least-privilege work scope per endpoint (`work:poll`, `work:renew`, `work:ack`, `work:events`).
- Are tenant-scoped via the route: `/tenants/{tenantId}/...`.
- Require an `environment` (query) or `environmentTag` (body).

Authentication supports both:

- `Authorization: Bearer ...`
- `X-Croniq-Key: <api-key>`

## Runner Identity

`runnerId` is treated as the lease owner and must match the authenticated caller identity (API client id for API keys or subject for bearer tokens). A runner represents a worker process instance and can execute many jobs over time. Use a stable value (for example `hostname + process`) and reuse the same `runnerId` for polling, renewing, and acknowledging work. If the `runnerId` does not match the authenticated caller identity, the server rejects the request with `403 runner-mismatch`.

Runner ids must be unique per live process. SDKs generate a `runnerInstanceId` and include it in the hello/poll/heartbeat metadata; if the API host sees the same `runnerId` already active with a different instance id, it rejects the new session with `409 runner-id-in-use` and the runner should fail fast.

Jobs are assigned 1:1 to a `runnerId` within a tenant/environment. Active jobs must have an assignment, and dispatch only leases work to the assigned runner. Reassignments require the job to be inactive.

Horizontal scale-out is achieved by running multiple runners with distinct `runnerId` values; lease ownership guarantees one execution per lease while job-level concurrency policies still apply. Scale-out for a single job requires a future `RunnerPool` concept.

## Endpoints

### Poll

`POST /tenants/{tenantId}/work/poll?environment=dev`

Claims due trigger leases for the caller.

Scope: `work:poll`

Request body:

- `runnerId` (string, required): stable runner identity (e.g., host + process).
- `batchSize` (int, optional): number of leases to claim. Default `1`.
- `waitForMs` (int, optional): long-poll timeout in milliseconds. Default `0` (immediate).
- `allowTestExecutions` (bool, optional): if `true`, the runner accepts test executions.
- `maxInflight` (int, optional): max in-flight hints for the server.
- `capabilities` (string[], optional): capability tags to associate with the runner.

Response:

- `leases`: array of lease tokens.
  - `executionId`: execution identifier for logs/events.
  - `executionMode`: `normal|test`.
  - `invocationSource`: `schedule|manual|api|webhook-ingress|webhook-invoke` (extensible).

### Renew

`POST /tenants/{tenantId}/work/renew?environment=dev`

Renews a lease while a work item is still being processed.

Scope: `work:renew`

Request body:

- `runnerId` (string, required)
- `lease` (token, required)

Response:

- `renewed` (bool)
- `lease` (updated token, when renewed)

### Ack

`POST /tenants/{tenantId}/work/ack?environment=dev`

Acknowledges completion and releases the lease.

Scope: `work:ack`

Request body:

- `runnerId` (string, required)
- `lease` (token, required)
- `succeeded` (bool, required)
- `nextFireTimeUtc` (optional): when set, the trigger is rescheduled.
- `deadLetterReason` (optional): set for failed work when no reschedule is requested.

Test execution rejection:

- If a runner does not allow tests, it should reject by sending `deadLetterReason: "test-not-allowed"` (non-retryable).

### Events / Logs

`POST /tenants/{tenantId}/work/{executionId}:events?environment=dev`

Pushes execution-scoped events that are persisted via the execution log sink.

Scope: `work:events`

Request body:

- `runnerId` (string, required)
- `lease` (token, required)
- `events` (array):
  - `message` (string, required)
  - `level` (string, optional): `Trace|Debug|Information|Warning|Error|Critical`
  - `eventType` (string, optional)
  - `timestampUtc` (optional)
  - `properties` (optional)

## Sample

A minimal runner loop that polls/renews/acks is available at:

- `sdk/runner-go`
- `sdk/runner-node`
- `sdk/runner-python`
- `src/Croniq.Runner.Sdk` (package) + `samples/runners/dotnet/basic`

## .NET Hosted Service (SDK)

Use the .NET runner SDK with a hosted service to keep the sample code minimal:

```csharp
using Croniq.Runner;

builder.Services.AddCroniqRunnerHostedService(options =>
{
    options.Config = RunnerConfig.FromEnvironment() with
    {
        HeartbeatInterval = TimeSpan.FromSeconds(15)
    };

    options.OnExecute("demo-job", async (context, payload, logger, cancellationToken) =>
    {
        logger.Info("execution started", new Dictionary<string, object?>
        {
            ["executionId"] = context.ExecutionId,
            ["jobKey"] = context.JobKey
        });

        await Task.Delay(250, cancellationToken);
    });
});
```

## SDK/Runner Integration (Recommended)

Keep the SDK configuration explicit and stable, and document it for operators:

- Required config (as used in the samples):
  - `CRONIQ_API_BASEURL` (HTTP base URL)
  - `CRONIQ_TENANT_ID`
  - `CRONIQ_ENVIRONMENT`
  - `CRONIQ_API_KEY` or bearer token
  - `CRONIQ_RUNNER_ID` (must match API client id)
- Optional SDK knobs (exposed via config or flags):
  - poll batch size + long-poll wait
  - max inflight (for gRPC stream)
  - lease-renew lead time or renewal interval
  - request timeout + retry backoff/jitter
  - runner instance id (`CRONIQ_RUNNER_INSTANCE_ID`, auto-generated if omitted)
- Keep client code minimal; the SDK owns transport selection, lease renewals, ack/outbox behavior, and dispatches to per-job handlers.

Failover/offline strategy:

- If polling fails due to transient network errors, back off with jitter and retry; do not spin.
- Keep renewing active leases while work is running; if renew fails with a conflict or missing lease, cancel the job and stop acking (the server may have reassigned).
- Treat ack and event publishing as idempotent. Retry on transient failures; stop on `403 runner-mismatch`, `409 lease-conflict`, or `404` (lease no longer valid).
- If auth fails (`401/403` or runner mismatch), treat it as a fatal configuration error.

Local persistence fallback (outgoing queue):

- Persist outgoing acks/events locally so a runner restart or brief outage does not lose results.
- Replay the queue in order; drop entries that conflict with server state (lease expired/conflict) and move on.
- Do not execute new work offline; only process work that was already leased before the outage.

More detail: see `docs/deep-dive/sdk-runner-integration.md`.

## Graceful Shutdown (Drain)

Use a drain flow when stopping a runner:

- Stop claiming new work (close the gRPC stream and pause polling).
- Keep renewing/acking in-flight leases until completion.
- If the shutdown timeout elapses, cancel local execution and stop renewing; do not ack success after lease loss.

## Job Registration & Approval

Jobs can be registered via the API/UI and optionally by runners (self-registration). The API host applies a per-tenant policy:

- `RequireApproval` (default): runner-registered jobs are `pending` until approved in the UI/API.
- `AutoActivate`: runner-registered jobs become active immediately.
- `Deny`: runner self-registration is rejected.

Only `active` jobs are dispatched to runners; pending jobs are never assigned.

When a runner self-registers a job, the assignment is captured alongside the job. Approving the job confirms the assignment and allows dispatch.

Runner SDKs call the self-registration endpoint by default before starting work:

- `POST /tenants/{tenantId}/jobs:register?environment=dev`
- Scope: `jobs:register`
- Body: `environmentTag`, `runnerId`, `runnerInstanceId` (optional), `jobKey`, `description` (optional), `metadata` (optional)

Disable auto-registration in SDKs by setting `CRONIQ_RUNNER_REGISTER_JOBS=false`.

## Issue a Runner API Key (SQL auth)

For SQL-backed auth, you can use the helper script to create an API client and key with the runner scopes:

```powershell
./scripts/issue-worker-api-key.ps1 -TenantId default -ClientId runner-dev -Environment dev -EmitEnv
```

Use the emitted `CRONIQ_API_KEY` and set `CRONIQ_RUNNER_ID` to the same client id.
The helper defaults to the work scopes plus `jobs:register`, `workers:heartbeat`, `workers:read`, `runners:heartbeat`, and `runners:read`; use `-Scopes` to trim if you only need runner access.

## Runner Presence (Optional)

If you need runner availability for dashboards or ops tooling, use the runner heartbeat endpoints (worker hosts use `/workers` instead):

- `POST /tenants/{tenantId}/runners/heartbeat?environment=dev`
- `GET /tenants/{tenantId}/runners?environment=dev`
- `GET /tenants/{tenantId}/runners?environment=dev&includeOffline=true`

Scopes:

- `runners:heartbeat` for posting heartbeats
- `runners:read` for listing runners

Heartbeat payloads accept `runnerId`, optional `seenAtUtc`, and optional `metadataJson` for tags or capabilities.
Presence is derived from the configured TTL; offline runners are retained for `RunnerStoreOptions.OfflineRetentionTtl` so UIs can show recently offline runners when `includeOffline=true` is supplied. Presence is informational and does not affect lease correctness.

Recommended metadata fields include:

- `runnerInstanceId`
- `transportState` (`grpc`/`polling`)
- `allowTestExecutions` (boolean)
- `maxInflight` (number)
- `draining` (boolean)
- `capabilities` (string array)

## Protocol Roadmap

The longer-term gRPC streaming protocol, work-item schema, and event/log ingestion plan are tracked in `docs/deep-dive/designs/polyglot-runner-protocol.md`.

> **Learn more:** See the deep dives on [persistence & leases](../deep-dive/persistence.md) and the [polyglot runner protocol](../deep-dive/designs/polyglot-runner-protocol.md).
