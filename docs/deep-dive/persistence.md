# Croniq SqlServer Persistence

This document describes the current SqlServer persistence layer for Croniq: schema layout, DbContext usage, migration workflow, and operational guidance. It captures the decisions referenced in `architecture.md` and fulfils the docstreams backlog item "Document persistence deep-dive".

## Scope & Goals

- A single SqlServer database uses the `croniq` schema for scheduler persistence and the `auth` schema for auth data by default.
- Every entity includes `TenantId`, `EnvironmentTag`, and concurrency metadata to guarantee tenant isolation.
- EF Core is the only abstraction; migrations are versioned in `src/Croniq.Data.SqlServer/Migrations` and applied via `tools/Croniq.DbMigrator`.
- Croniq hosts can switch between in-memory and SqlServer persistence via configuration (`Croniq:Persistence:Mode`).

## Schema Overview

Below is a simplified entity diagram covering the scheduler (`croniq`) and auth (`auth`) tables (shared DbContext).

```mermaid
classDiagram
    class JobEntity {
        long Id
        string TenantId
        string EnvironmentTag
        string Namespace
        string Name
        string Variant?
        string MetadataJson
        datetime UpdatedAt
    }
    class TriggerEntity {
        long Id
        long JobId
        string TenantId
        string EnvironmentTag
        string TriggerType
        string PayloadJson
        datetime NextFireAtUtc
        rowversion RowVersion
    }
    class DeadLetterEntity {
        long Id
        long JobId
        string TenantId
        string EnvironmentTag
        string Reason
        string PayloadJson
        datetime CreatedAt
        datetime? ResolvedAt
    }
    class ApiClientEntity {
        long Id
        string TenantId
        string ClientId
        string DefaultScopes
        string EnvironmentTag
        datetime CreatedAt
    }
    class ApiKeyEntity {
        long Id
        long ApiClientId
        string KeyId
        string KeyHash
        datetime? ExpiresAt
        bool IsActive
        string Scopes
    }
    JobEntity <|-- TriggerEntity : job
    JobEntity <|-- DeadLetterEntity : job
    ApiClientEntity <|-- ApiKeyEntity : keys
```

Additional tables (leases, worker instances, audit log) follow the same tenant/environment pattern; scheduler tables live under `croniq` while auth tables live under `auth`. See `SqlServerDbContext` for exact property lists.

## Code Layout

```text
src/Croniq.Data.SqlServer/
  SqlServerDbContext.cs
  SqlServerOptions.cs
  Entities/
    JobEntity.cs
    TriggerEntity.cs
    DeadLetterEntity.cs
    ApiClientEntity.cs
    ApiKeyEntity.cs
    ...
src/Croniq.Persistence.SqlServer/
  SqlServerJobPersistenceProvider.cs
  ServiceCollectionExtensions.cs
src/Croniq.Auth.SqlServer/
  SqlServerApiKeyStore.cs
  ServiceCollectionExtensions.cs
tools/Croniq.DbMigrator/
  Program.cs
```

- `Croniq.Data.SqlServer` exposes `AddCroniqSqlServerDbContext(IServiceCollection, SqlServerOptions)` for host registration.
- `Croniq.Persistence.SqlServer` implements `IJobPersistenceProvider` and adds health checks.
- `Croniq.Auth.SqlServer` depends on the same DbContext, providing `IApiKeyStore`.

## Configuration Matrix

| Setting                                              | Purpose                                                | Notes                                                                           |
| ---------------------------------------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------- |
| `Croniq:SqlServer:ConnectionString`                  | Default connection string shared by auth + persistence | Example: `Server=localhost,1433;Database=Croniq;User Id=sa;Password=Secret123!` |
| `Croniq:Persistence:Mode`                            | `InMemory` or `SqlServer`                              | Controls which provider `AddCroniqApiServices` wires up                         |
| `Croniq:Persistence:SqlServer:ConnectionString`      | Optional override for persistence only                 | Falls back to `Croniq:SqlServer:ConnectionString`                               |
| `Croniq:Persistence:SqlServer:CommandTimeoutSeconds` | EF command timeout                                     | Defaults to 30s                                                                 |
| `Croniq:Auth:Mode`                                   | `InMemory` or `SqlServer`                              | Auth shares the DbContext when `SqlServer`                                      |
| `Croniq:Auth:SqlServer:ConnectionString`             | Optional override for auth only                        | Use when auth DB is separate                                                    |

## Migration Workflow

1. **Create/Update entities** in `src/Croniq.Data.SqlServer/Entities`.
2. **Add a migration** from the repository root (run `dotnet tool restore` once to install the local `dotnet-ef` tool):

   ```cmd
   dotnet ef migrations add <Name> --project src/Croniq.Data.SqlServer --startup-project tools/Croniq.DbMigrator --output-dir Migrations
   ```

   Make sure the generated migration files are checked in, including the `*.Designer.cs` files and `SqlServerDbContextModelSnapshot.cs`. EF Core uses the designer attributes to discover migrations, and missing designer files will surface as "No EF Core migrations were discovered" in CI.

3. **Apply locally** via the migrator:

   ```cmd
   dotnet run --project tools/Croniq.DbMigrator -- --connection "<connection-string>" --apply
   ```

4. **Verify CI**: `Croniq.DbMigrator --verify` runs in nightly workflows to detect drift.
5. **Docs**: Update this file (and any consumer references) whenever connection options or defaults change.

### Applying `WebhookEndpointIpRules` (2025-12)

The `WebhookEndpointIpRules` migration ships with Croniq `main` (Dec 2025) and must be applied before enabling webhook IP allow lists in production:

1. **Set the connection string** for the migrator container/CLI:

```cmd
set CRONIQ_SQL_CONNECTION=Server=<sql-host>;Database=Croniq;User Id=cronq_admin;Password=<secret>;
```

1. **Run the migrator once per environment** (dev/test/prod). The tool is idempotent, so reruns are safe:

```cmd
dotnet run --project tools/Croniq.DbMigrator -- --apply
```

> Containerized clusters can execute the same step with `docker compose run --rm croniq-db-migrator` as long as `CRONIQ_SQL_CONNECTION` is injected.

1. **Verify the table exists** before exposing the new API endpoints:

```sql
SELECT TOP (5) HookKey, TenantId, EnvironmentTag, Cidr
FROM croniq.WebhookEndpointIpRules
ORDER BY CreatedAtUtc DESC;
```

1. **Rollback plan**: restore from backup if the migration fails. The schema change is additive (new table + indexes), so no data loss occurs when rolling forward again.

## Local Development & Dev Stack

- The Docker dev stack (`infra/docker/docker-compose.yml`) launches SQL Server 2022 with the Croniq schema. Compose derives `CRONIQ_SQL_CONNECTION` from `CRONIQ_SQL_HOST`, `CRONIQ_SQL_DATABASE`, and `CRONIQ_SQL_PASSWORD` (using `sa` on port 1433) based on `.env`.
- `scripts\devstack-up.cmd` waits for SQL health before running the migrator container (`croniq-db-migrator`).
- Developers can also run `dotnet run --project tools/Croniq.DbMigrator -- --connection %CRONIQ_SQL_CONNECTION% --apply` manually.
- Test projects rely on Testcontainers to spin up SQL Server and call the migrator automatically.

## Clustering & Leases (Future)

Clustering rides on the same SqlServer provider once GA-ready:

- Lease tables (`croniq.TriggerLeases`, `croniq.WorkerInstances`) coordinate trigger ownership.
- Heartbeats and grace periods prevent duplicate executions after failover.
- Cluster health endpoints (`/cluster/nodes`) read directly from these tables.

This document will expand with schema details once clustering ships.

## Operational Guidance

- Use the shared DbContext for both persistence and auth to avoid duplication. If production separates the databases, configure the domain-specific connection strings.
- Always enable Transparent Data Encryption and TLS on production SQL instances; Croniq simply uses the ADO.NET connection string.
- Monitor `SqlServerJobPersistenceProvider` health checks; they expose readiness/liveness for the API and worker hosts.
- Set retention policies via configuration. Retention jobs are implemented and configured via `Croniq:Retention` and `ExecutionLogRetentionOptions`.

## Backlog & Ownership

- Finish the lease/worker instance tables for clustering (tracked in `architecture.md`).
- Keep retention configuration aligned with `CroniqRetentionOptions` and `ExecutionLogRetentionOptions` whenever defaults change.
- Mirror any schema changes in both this doc and the consumer configuration guide so operators know which settings exist.
- Owners: `@CroniqMaintainers` (persistence), `@CroniqDocs` (documentation).
