# Croniq Troubleshooting

Use this checklist to diagnose the most common issues developers hit while working with Croniq. Each section links to deeper guidance under `/deep-dive` when you need the full background.

## 1. Authentication & Authorization

| Symptom                                           | Likely Cause                                     | Fix                                                                                                                                                                                                      |
| ------------------------------------------------- | ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `401 Unauthorized` with API keys                  | Missing `X-Croniq-Key` header or wrong auth mode | Confirm `Croniq__Auth__Mode` and ensure the header is present. For InMemory mode, restart the host after changing `Croniq__Auth__InMemory__ApiKey`. See [`auth.md`](../guides/auth.md) for issuing keys. |
| `401 Unauthorized` with bearer tokens             | Token audience or issuer mismatch                | Validate the token with `jwt.ms` or `jwt.io` and ensure the host is configured to validate that issuer/audience.                                                                                         |
| `403 Forbidden` even though the token looks valid | Missing scopes or tenant claim                   | Ensure the token contains the expected scope + tenant claims. Missing claims/scopes are rejected at ingress.                                                                                             |
| Requests rate-limited immediately                 | Tenant/caller resolved to `anonymous`            | Inspect logs for `RateLimitPartition` messages. Provide either a valid key or bearer token so Croniq can derive the caller context before rate limiting.                                                 |

## 2. Dev Stack (Docker) Issues

| Symptom                                                                 | Likely Cause                                | Fix                                                                                                                                                                                                                  |
| ----------------------------------------------                          | ------------------------------------------  | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------                                           |
| `mssql-22` container never becomes healthy                              | Port already in use or volume corruption    | Stop other SQL instances on port 1433, then rerun `scripts\devstack-down.cmd --volumes` (wipes local DB). Reference `/deep-dive/devstack.md` for port overrides in `.env`.                                           |
| `croniq-db-migrator` exits with "login failed"                          | Wrong SQL credentials in `.env`             | Update `CRONIQ_SQL_HOST`, `CRONIQ_SQL_PASSWORD`, and `CRONIQ_SQL_DATABASE` (compose builds the connection string), or override `Croniq__SqlServer__ConnectionString`. Recreate `.env` from `.env.example` if unsure. |
| `croniq-db-migrator` exits with "No EF Core migrations were discovered" | Migration designer files missing or ignored | Ensure `src/**/Migrations/*.Designer.cs` and `SqlServerDbContextModelSnapshot.cs` are committed (not ignored by `.gitignore`), then rebuild and rerun the dev stack.                                                 |
| API container restarts constantly                                       | Missing auth or persistence config          | Check `docker compose logs api -f`. Ensure the env file includes `Croniq__Auth__Mode` and persistence settings.                                                                                                      |
| Observability profile fails (`loki` issues)                             | Overlapping port bindings or stale volumes  | Run `scripts\devstack-down.cmd --profile obs --volumes` to reset observability containers before starting again.                                                                                                     |

## 3. Jobs Do Not Run

| Symptom                    | Likely Cause                          | Fix                                                                                                                                                                                 |
| -------------------------- | ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Manual triggers return 404 | Wrong `jobKey` namespace/name         | Ensure the job registration matches the requested job key (format `namespace:name[:variant]`). Use `JobKey.Create("namespace", "name")` in code and align with the trigger request. |
| Job stuck in waiting state | Custom prerequisites never satisfied  | Log additional detail within the job handler and watch the Croniq Log Pulse dashboard. Validate external dependencies (queues, APIs) before requeuing.                              |
| Schedules never fire       | Worker host offline or policy blocked | Confirm the worker container/service is running. Check scheduler logs for policy rejections (quota, concurrency).                                                                   |
| Dead-lettered executions   | Exceptions bubble from handler        | Review Serilog logs or Grafana panels for the job. Add retries/policies as needed.                                                                                                  |

## 4. Observability & Telemetry

| Symptom                              | Likely Cause                         | Fix                                                                                                                                                                               |
| ------------------------------------ | ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Grafana dashboards empty             | Collector not receiving OTLP traffic | Verify `Croniq__Observability__OtlpEndpoint` and protocol. When running outside Docker, point to `http://localhost:4317`. `/deep-dive/observability.md` lists the full checklist. |
| Loki log panels missing data         | Tenant headers out of sync           | Ensure both the OTEL collector and Grafana Datasource use the same tenant ID (`croniq-devstack` by default). Update `infra/docker/observability/*` together.                      |
| Prometheus alerts firing immediately | Dev stack running without workload   | Silence alerts or disable the rules when using a tiny dev stack. When investigating actual problems, read `infra/monitoring/rules/` annotations for runbooks.                     |

## 5. Scripts & CLI Helpers

| Symptom                                             | Likely Cause          | Fix                                                                                                         |
| --------------------------------------------------- | --------------------- | ----------------------------------------------------------------------------------------------------------- |
| `scripts\devstack-up.cmd` fails with ".env missing" | `.env` not created    | Copy `.env.example` to `.env` and edit the secrets. The script checks for the file before launching Docker. |
| `scripts\devstack-trigger-job.cmd` returns `401`    | Missing API key       | Populate `CRONIQ_API_KEY` in `.env`. The script forwards it as `X-Croniq-Key`.                              |

## Still Stuck?

1. Capture `docker compose ps` and `docker compose logs <service> --tail=200` outputs.
2. Note the Croniq version/commit you are running.
3. Share the details in your team channel or file an issue with the steps to reproduce.

Additional background lives in `/deep-dive/devstack.md`, `/deep-dive/security.md`, and `/deep-dive/observability.md`.
