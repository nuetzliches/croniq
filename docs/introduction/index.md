# Croniq Documentation

> Croniq gives platform teams a centralized scheduler for distributed systems: orchestrate jobs across microservices, enforce policies, and monitor executions without re-implementing cron logic in every repo.

## What Is Croniq?

Croniq is a tenant-aware scheduling platform built on .NET 10. It combines a fluent job SDK, a configurable policy engine, and REST/gRPC gateways with SqlServer-backed durability. By keeping scheduling, throttling, and observability in one place, Croniq helps you coordinate maintenance tasks, compliance workflows, and recurring automations across dozens of services.

### Why teams adopt Croniq

- **Unified orchestration:** Register jobs once, attach cron/interval/event triggers, and let Croniq fan them out across workers.
- **Central guardrails:** Apply retry, timeout, concurrency, and quota policies globally while keeping tenant isolation.
- **Operational clarity:** Stream logs/metrics/traces via the built-in OpenTelemetry stack and inspect dead letters centrally.
- **Secure access:** Mix API keys and bearer tokens with per-tenant scopes, rate limits, and auditing.
- **Easy onboarding:** Spin up the Docker dev stack, run the quickstart, and grow into the deeper architecture guides at your own pace.

Whenever you need implementation details (dev stack bootstrap, CI/CD, troubleshooting), jump into `/deep-dive/`.

## Getting Started

1. Review [`/deep-dive/architecture.md`](../deep-dive/architecture.md) for the core scheduling model.
2. Walk through the [Hello Croniq Quickstart](./quickstart.md) to register an `IJob` implementation and trigger it via the Minimal API.
3. Pick a profile via [Deployment modes](./deployment-modes.md) (minimal samples vs a separated, self-hosted platform setup).
4. Configure endpoints, API keys, and tenant scopes via the [Configuration Guide](./configuration.md).
5. Learn about job policies and trigger options via [`policies.md`](../guides/policies.md) and [`triggers.md`](../guides/triggers.md).
6. Use the SDK reference (coming soon) for detailed descriptions of `IJob`, `IJobExecutionContext`, and helper attributes.
7. Need diagnostics, observability, or CI internals? Follow the "Learn more" links that point into `/deep-dive/*` (for example, the Docker dev stack lives in `/deep-dive/devstack.md`).

## Minimal Worker Host (InMemory)

```csharp
using Croniq;
using Croniq.Sdk;

var builder = WebApplication.CreateBuilder(args);

builder.Services
    .AddCroniq(builder.Configuration)
    .AddCroniqJob("samples", "ping", (context, _) =>
    {
        context.Logger.LogInformation("Ping from {JobKey}", context.JobKey);
        return Task.CompletedTask;
    })
    .AddTrigger("@once");

var app = builder.Build();
app.Run();
```

Want fewer `using` statements? Install the optional `Croniq.Usings` package for opt-in global usings.

## Placeholder Topics

- [Quickstart: first job & schedule](./quickstart.md)
- [Configuration & environment variables](./configuration.md)
- [Policies & operational controls](../guides/policies.md)
- [Trigger types](../guides/triggers.md)
- How to provision API keys
- Job metadata & `CroniqJobAttribute`
- Working with environments/Tenant scopes
- Troubleshooting & FAQ (link to `/deep-dive/devstack.md` + `/deep-dive/observability.md` for deeper debugging guides)

The dev stack, diagnostics, and operational handbooks are **not** duplicated here—always defer to the `/deep-dive/` equivalents once you move beyond first contact.

Each topic should link back to relevant deep dives in `/deep-dive/` for advanced context.

Contributing to the docs? Start with the [Documentation Streams plan](../deep-dive/docstreams.md) to understand personas, ownership, and review expectations.
