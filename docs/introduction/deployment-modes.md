---
layout: doc
---

# Deployment modes

Croniq can be operated at different maturity levels. These are not different products; they are different compositions of hosts, persistence, and optional features.

## 1) Minimal (samples / local development)

Goal: get started fast with little configuration and few running components.

- Typical: run the `Croniq.Sample` project as a single process.
- It avoids requiring DB permissions/schema in scenarios that are intentionally "no database".
- Features: keep the baseline small; additional surfaces are opt-in (API/UI, gRPC, webhooks, observability, durable SqlServer/Postgres persistence).
- Trade-off: without durable persistence, jobs and schedules are not a stable, shared "source of truth" across restarts and scaling.

When it fits:

- local development, PoCs, small single-machine automations, demos.

## 2) Platform (self-hosted, separated components)

Goal: durable, operable, and UI/management-first.

Typical separation:

- API host (REST, optionally UI)
- Worker host (job execution)
- Webhook host (optional)
- gRPC clients (from applications or services)

In this repo, the platform-style samples are split across dedicated projects (for example `Croniq.Sample.ApiHost` and `Croniq.Sample.WorkerHost`).

Baseline requirements:

- durable persistence (SqlServer/Postgres)
- more configuration, in exchange for predictable operations and full management capabilities

### Minimal configuration per host (platform mode)

Use these minimal templates when you split API, worker, and webhooks into separate hosts. Replace the connection string values and swap `SqlServer` for `Postgres` if needed.

#### API host

::: code-group

```json [appsettings.json]
{
  "Croniq": {
    "Core": {
      "TenantId": "prod",
      "EnvironmentTag": "prod-cluster"
    },
    "Auth": { "Mode": "SqlServer" },
    "Persistence": { "Mode": "SqlServer" },
    "SqlServer": {
      "ConnectionString": "Server=sql;Database=Croniq;User Id=croniq;Password=***;"
    },
    "Api": { "RequestsPerMinute": 60 },
    "Webhooks": { "Mode": "SqlServer" }
  }
}
```

```dotenv [.env]
CRONIQ_CORE_TENANT_ID=prod
CRONIQ_ENVIRONMENT=prod-cluster
CRONIQ_AUTH_MODE=SqlServer
CRONIQ_PERSISTENCE_MODE=SqlServer
CRONIQ_SQL_HOST=sql
CRONIQ_SQL_DATABASE=Croniq
CRONIQ_SQL_PASSWORD=***
CRONIQ_API_REQUESTS_PER_MINUTE=60
CRONIQ_WEBHOOKS_MODE=SqlServer
```

```powershell [PowerShell]
$Env:Croniq__Core__TenantId = "prod"
$Env:Croniq__Core__EnvironmentTag = "prod-cluster"
$Env:Croniq__Auth__Mode = "SqlServer"
$Env:Croniq__Persistence__Mode = "SqlServer"
$Env:Croniq__SqlServer__ConnectionString = "Server=sql;Database=Croniq;User Id=croniq;Password=***;"
$Env:Croniq__Api__RequestsPerMinute = "60"
$Env:Croniq__Webhooks__Mode = "SqlServer"
```

:::

#### Worker host

::: code-group

```json [appsettings.json]
{
  "Croniq": {
    "Core": {
      "TenantId": "prod",
      "EnvironmentTag": "prod-cluster"
    },
    "Persistence": { "Mode": "SqlServer" },
    "SqlServer": {
      "ConnectionString": "Server=sql;Database=Croniq;User Id=croniq;Password=***;"
    },
    "Jobs": {
      "Assemblies": ["/app/jobs/Acme.Jobs.dll"]
    }
  }
}
```

```dotenv [.env]
CRONIQ_CORE_TENANT_ID=prod
CRONIQ_ENVIRONMENT=prod-cluster
CRONIQ_PERSISTENCE_MODE=SqlServer
CRONIQ_SQL_HOST=sql
CRONIQ_SQL_DATABASE=Croniq
CRONIQ_SQL_PASSWORD=***
CRONIQ_JOBS_ASSEMBLIES_0=/app/jobs/Acme.Jobs.dll
```

```powershell [PowerShell]
$Env:Croniq__Core__TenantId = "prod"
$Env:Croniq__Core__EnvironmentTag = "prod-cluster"
$Env:Croniq__Persistence__Mode = "SqlServer"
$Env:Croniq__SqlServer__ConnectionString = "Server=sql;Database=Croniq;User Id=croniq;Password=***;"
$Env:Croniq__Jobs__Assemblies__0 = "/app/jobs/Acme.Jobs.dll"
```

:::

#### Webhooks host

::: code-group

```json [appsettings.json]
{
  "Croniq": {
    "Core": {
      "TenantId": "prod",
      "EnvironmentTag": "prod-cluster"
    },
    "Webhooks": {
      "Mode": "SqlServer",
      "Ingress": { "DispatchMode": "TriggerJob" }
    },
    "SqlServer": {
      "ConnectionString": "Server=sql;Database=Croniq;User Id=croniq;Password=***;"
    },
    "Jobs": {
      "Assemblies": ["/app/jobs/Acme.Jobs.dll"]
    }
  }
}
```

```dotenv [.env]
CRONIQ_CORE_TENANT_ID=prod
CRONIQ_ENVIRONMENT=prod-cluster
CRONIQ_WEBHOOKS_MODE=SqlServer
CRONIQ_WEBHOOKS_INGRESS_DISPATCH_MODE=TriggerJob
CRONIQ_SQL_HOST=sql
CRONIQ_SQL_DATABASE=Croniq
CRONIQ_SQL_PASSWORD=***
CRONIQ_JOBS_ASSEMBLIES_0=/app/jobs/Acme.Jobs.dll
```

```powershell [PowerShell]
$Env:Croniq__Core__TenantId = "prod"
$Env:Croniq__Core__EnvironmentTag = "prod-cluster"
$Env:Croniq__Webhooks__Mode = "SqlServer"
$Env:Croniq__Webhooks__Ingress__DispatchMode = "TriggerJob"
$Env:Croniq__SqlServer__ConnectionString = "Server=sql;Database=Croniq;User Id=croniq;Password=***;"
$Env:Croniq__Jobs__Assemblies__0 = "/app/jobs/Acme.Jobs.dll"
```

:::

Why it matters:

- Once operators expect "full management in the UI", Croniq needs a server-side source of truth (persistence), not only an in-process registry.

---

## Job catalog vs schedules (why cataloging is necessary)

Conceptually, Croniq distinguishes between:

- Job catalog (job definitions): "Which jobs exist in this tenant/environment?" including metadata (JobKey, display name, description, owner, capabilities).
- Schedules/triggers: "When should a job run?" (cron/interval/webhook/event) and with which policies.

For UI/management the crucial point is:

- Jobs must exist independently of schedules.
  - Otherwise the UI cannot show/manage jobs that exist but have not been scheduled yet.
- Ownership signaled by a client/host (for a given JobKey) makes cataloging mandatory.
  - The UI can then reliably show ownership, lifecycle, and enforce scope/policy rules even when no schedules exist.

## Recommended baseline

- Platform mode: enable opt-in "Job Catalog Seeding" on host startup to upsert job definitions per tenant/environment.
  - No schedule creation, no deletions.
- Minimal mode: keep it off by default, enable it when you want UI/management completeness.

Why opt-in (instead of implicit):

- It avoids unexpected writes to persistence on startup (principle of least surprise).
- It avoids requiring DB permissions/schema in scenarios that are intentionally "no database".
- It makes ownership explicit: multiple hosts can register the same JobKey, so seeding needs clear rules for how metadata/owner is set and how conflicts are resolved.
- It keeps failure modes obvious: if seeding fails (DB down/misconfigured), you get a clear startup error only when you opted into that dependency.

## 3) Remote ingress (webhooks via a DMZ or edge tier)

Goal: accept public webhooks via a remote ingress tier (for example, a DMZ) without allowing ingress hosts to open outbound connections into the internal network.

Typical separation (DMZ example):

- Ingress/DMZ: `Croniq.Api` in `WebhookAdminOnly` mode + `Croniq.Webhooks` with `Ingress.DispatchMode=StoreOnly`, backed by a dedicated SqlServer/Postgres instance.
- Internal: `Croniq.Api` + worker hosts + UI. Internal API uses `Croniq:Webhooks:Mode=Remote` to manage webhook definitions in the DMZ and runs the relay worker to consume ingress events over gRPC (or SSE/polling via `StreamFallback` when gRPC is blocked).

Network paths:

- Inbound: public callers -> ingress webhook ingress.
- Outbound: internal network -> ingress admin API + ingress gRPC/SSE/polling stream.
- Ingress hosts do **not** connect into the internal network.

Security expectations:

- Use API keys with least-privilege scopes (`webhooks:read|write|rotate|deadletter` for admin, `webhooks:ingress` for the relay).
- Apply `Croniq:Api:AllowedIpCidrs` on the ingress host to restrict admin access to internal egress ranges.

