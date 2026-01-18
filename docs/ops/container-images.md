# Croniq Container Images

This guide describes the production container images and the configuration contract they expect. The images ship without `appsettings.Development.json`; all production settings should flow through environment variables or mounted configuration files.

## Images

- `croniq-api`: management API + gRPC (scheduler + worker) surfaces.
- `croniq-worker`: scheduler/worker host that executes jobs.
- `croniq-webhooks`: webhook ingress host (ingress-only surface).
- `croniq-db-migrator`: applies EF Core migrations for Croniq schemas.

## Build and push (manual)

For local or ad-hoc registry uploads, build from the production Dockerfile targets:

```bash
docker build -f infra/docker/Dockerfile.production --target api -t registry.example.com/croniq-api:0.1.0 .
docker build -f infra/docker/Dockerfile.production --target worker -t registry.example.com/croniq-worker:0.1.0 .
docker build -f infra/docker/Dockerfile.production --target webhooks -t registry.example.com/croniq-webhooks:0.1.0 .
docker build -f infra/docker/Dockerfile.production --target migrator -t registry.example.com/croniq-db-migrator:0.1.0 .

docker push registry.example.com/croniq-api:0.1.0
docker push registry.example.com/croniq-worker:0.1.0
docker push registry.example.com/croniq-webhooks:0.1.0
docker push registry.example.com/croniq-db-migrator:0.1.0
```

## Local testing with Docker Compose

Use the sample compose file to spin up SQL Server plus the production images:

```bash
docker compose -f infra/docker/docker-compose.production.yml up --build
```

Defaults use in-memory auth for quick smoke tests. Use `X-Croniq-Key: ${CRONIQ_API_KEY}` when calling the API.
To exercise SQL auth instead, set `Croniq__Auth__Mode=SqlServer` and `Croniq__Auth__Password__Enabled=true` (keep `CRONIQ_SEED_ADMIN=true` so the migrator seeds the admin user).
To use Postgres instead, run a Postgres instance and set `CRONIQ_DB_PROVIDER=Postgres` plus `CRONIQ_POSTGRES_CONNECTION` (or `Croniq__Postgres__ConnectionString`).

## Quick run (single host)

Use this for smoke tests only. Production deployments should use SqlServer/Postgres-backed auth and persistence.

::: code-group

```bash [docker run]
docker run --rm -p 5080:8080 \
  -e Croniq__Auth__Mode=InMemory \
  -e Croniq__Auth__InMemory__ApiKey=local-dev-key \
  -e Croniq__Persistence__Mode=InMemory \
  -e Croniq__Core__TenantId=default \
  -e Croniq__Core__EnvironmentTag=dev \
  croniq-api:0.1.0
```

```dotenv [.env (docker run)]
Croniq__Auth__Mode=InMemory
Croniq__Auth__InMemory__ApiKey=local-dev-key
Croniq__Persistence__Mode=InMemory
Croniq__Core__TenantId=default
Croniq__Core__EnvironmentTag=dev
```

:::

To load the `.env` file with Docker, run `docker run --rm --env-file .env -p 5080:8080 croniq-api:0.1.0`.
For Docker Compose, use the root `.env` with `CRONIQ_*` keys (see `.env.example`); the compose file maps those into `Croniq__...` settings inside the container.

## Common configuration

Most hosts rely on the same Croniq core + persistence/auth settings:

- `Croniq__Core__TenantId` (recommended)
- `Croniq__Core__EnvironmentTag` (recommended)
- `Croniq__Core__InstanceId` (recommended for multi-node deployments)
- `Croniq__Auth__Mode` (`InMemory`, `SqlServer`, or `Postgres`)
- `Croniq__Persistence__Mode` (`InMemory`, `SqlServer`, or `Postgres`)
- `Croniq__SqlServer__ConnectionString` (shared fallback for SqlServer auth + persistence)
- `Croniq__Postgres__ConnectionString` (shared fallback for Postgres auth + persistence)

## Job assembly loading

API, worker, and webhook ingress hosts can register jobs from external assemblies via configuration:

- `Croniq__Jobs__Assemblies__0=/app/jobs/Acme.Jobs.dll`
- `Croniq__Jobs__Assemblies__1=/app/jobs/Acme.Billing.Jobs.dll`
- `Croniq__Jobs__IncludeEntryAssembly=true` (optional; scan the host assembly)

Assemblies are loaded at startup only. If you mount or replace job DLLs, restart the host to pick them up.

You can also provide a delimited list:

- `Croniq__Jobs__Assemblies=/app/jobs/Acme.Jobs.dll;/app/jobs/Acme.Billing.Jobs.dll`

## Image-specific notes

### croniq-api

Required for production:

- `Croniq__Auth__Mode=SqlServer` (or `Postgres`)
- `Croniq__Persistence__Mode=SqlServer` (or `Postgres`)
- `Croniq__SqlServer__ConnectionString=...` (or `Croniq__Postgres__ConnectionString=...`)

Optional:

- `Croniq__Api__RequestsPerMinute` (rate limit)
- `Croniq__Api__ExposeSchemas=true` (Swagger + gRPC reflection)
- `Croniq__Webhooks__Mode=SqlServer|Postgres|Remote` (webhook admin persistence)
- `Croniq__Webhooks__Remote__BaseUrl` + `Croniq__Webhooks__Remote__ApiKey` when `Mode=Remote`

Persisted webhook secrets rely on ASP.NET Core Data Protection. Share the key ring across API/webhook hosts; see [`docs/deep-dive/security.md`](../deep-dive/security.md).

The API host does not expose webhook ingress endpoints. Use `croniq-webhooks` for inbound webhooks.

### croniq-worker

Required for production:

- `Croniq__Persistence__Mode=SqlServer` (or `Postgres`)
- `Croniq__SqlServer__ConnectionString=...` (or `Croniq__Postgres__ConnectionString=...`)
- `Croniq__Jobs__Assemblies__0=...` (or include entry assembly)

The worker host must load job assemblies or it cannot execute scheduled triggers.

### croniq-webhooks

Required for production:

- `Croniq__Webhooks__Mode=SqlServer|Postgres` (ingress hosts cannot run in `Remote` mode)
- `Croniq__SqlServer__ConnectionString=...` when `Mode=SqlServer` (or `Croniq__Postgres__ConnectionString=...` when `Mode=Postgres`)
- `Croniq__Jobs__Assemblies__0=...` when `Ingress.DispatchMode=TriggerJob`

If you run in DMZ ingress-only mode (`Ingress.DispatchMode=StoreOnly`), the host stores ingress events and does not execute jobs. Use `Croniq__Webhooks__Mode=Remote` on the internal API/relay side for the split.

### croniq-db-migrator

Provide the connection string via `CRONIQ_DB_PROVIDER` plus `CRONIQ_SQL_CONNECTION` or `CRONIQ_POSTGRES_CONNECTION` and run the migrator (idempotent; safe to rerun):

```bash
CRONIQ_DB_PROVIDER=SqlServer \
CRONIQ_SQL_CONNECTION="Server=sql;Database=Croniq;User Id=sa;Password=..." \
  dotnet Croniq.DbMigrator.dll
```

```bash
CRONIQ_DB_PROVIDER=Postgres \
CRONIQ_POSTGRES_CONNECTION="Host=postgres;Database=Croniq;Username=postgres;Password=..." \
  dotnet Croniq.DbMigrator.dll
```
