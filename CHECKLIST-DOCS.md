# CHECKLIST-DOCS

Documentation backlog extracted from `CHECKLIST.md`. Track doc-only and doc-alignment items here.

## Ready to implement (doc-only updates)

### Deep-dive docs
- [x] `docs/deep-dive/testing.md`: switch to Shouldly, remove global.json 8.x reference, fix Observability cadence, update release workflow TODO.
- [x] `docs/deep-dive/ci.md`: rename `ci-nightly.yml` references to `nightly.yml`.
- [x] `docs/deep-dive/devstack.md` + `docs/deep-dive/supplychain.md`: rename `ci-nightly.yml` references to `nightly.yml`.
- [x] `docs/deep-dive/devstack.md`: replace "future Croniq.Worker" with `Croniq.WorkerHost`.
- [x] `docs/deep-dive/auth.md`: update testing section to point at real test projects.
- [x] `docs/deep-dive/auth.md`: fix API key format to `ak_<guid>.<secret>`.
- [x] `docs/deep-dive/auth.md`: fix tenant rate limits key to `Croniq:Api:TenantRateLimits`.
- [x] `docs/deep-dive/architecture.md`: align retention defaults with `CroniqRetentionOptions` and `ExecutionLogRetentionOptions`.
- [x] `docs/deep-dive/architecture.md`: update trigger types to current cron/@once + webhooks/manual.
- [x] `docs/deep-dive/architecture.md`: fix quota description (JobKey/Scope with `MaxTriggersPerMinute` + `MaxParallelExecutionsPerJob`; API rate limits are separate).
- [x] `docs/deep-dive/designs/webhook-secret-rotation.md`: remove "future helper script" note and align retention/cleanup notes to current options/defaults.
- [x] `docs/deep-dive/release.md`: align steps with actual release workflows (no `Directory.Build.props` bump, no Snyk/Docs artifact if not used).
- [x] `docs/deep-dive/password-auth.md`: fix config key names and update `docs/guides/webhooks.md` from .NET 8 to net10.0.
- [x] `docs/deep-dive/observability.md`: fix metric name to `cronijob_executions_total`.
- [x] `docs/deep-dive/observability.md`: remove the Serilog registration claim (Default provider uses `ILoggerFactory`).
- [x] `docs/deep-dive/devstack.md`: fix Prometheus/Tempo port defaults to 9090/3200.
- [x] `docs/deep-dive/devstack.md`: fix default JobKey in step 5 (`samples:smoke`).
- [x] `docs/deep-dive/observability.md`: fix default JobKey in verification steps (`samples:smoke`).
- [x] `docs/deep-dive/devstack.md` + `docs/deep-dive/persistence.md`: clarify SQL connection sourcing (compose builds `CRONIQ_SQL_CONNECTION` from `CRONIQ_SQL_*`).
- [x] `docs/deep-dive/persistence.md`: clarify scheduler vs auth schema (`croniq` vs `auth`).
- [x] `docs/deep-dive/job-registration.md`: update backlog section (both `AddCroniqJobsFromAssembly` and validate-only startup mode exist).
- [x] `docs/deep-dive/persistence.md`: update retention backlog note (cleanup is implemented).
- [x] `docs/deep-dive/policies.md`: translate non-English sentences and normalize metric names to `cronipolicy_*`.
- [x] `docs/deep-dive/designs/job-log-persistence.md`: update "Current State" to reflect the implemented execution log store.
- [x] `docs/deep-dive/designs/polyglot-worker-protocol.md`: resolve WorkItems/WorkClaims contradiction.
- [x] `docs/deep-dive/ci.md`: update backlog note to reflect completion.
- [x] `docs/deep-dive/observability.md`: update backlog note to reflect completion.
- [x] `docs/deep-dive/supplychain.md`: update "Provision signing keys" backlog item to current state.
- [x] `docs/deep-dive/security.md`: update TenantGuard note (Scheduler RPC host already uses it).
- [x] `docs/deep-dive/index.md`: remove "roadmap" label for schedules endpoint and update gRPC client note.
- [x] `docs/deep-dive/docstreams.md`: align status with the checklist (blocked until repo public) and fix missing template references.

### Guides and introduction
- [x] `docs/guides/triggers.md`, `docs/ops/retention.md`, `docs/deep-dive/architecture.md`: align cron field count (6 fields + optional year).
- [x] `docs/guides/triggers.md`: update TriggerId default (Base64-url Cron + optional TimeZoneId, hash fallback >512 chars).
- [x] `docs/guides/webhooks.md` + `docs/deep-dive/security.md`: IP rule block is `403 ip-blocked` (not `429 ip-rule-denied`).
- [x] `docs/guides/workers-runners.md`: runner mismatch returns `403 runner-mismatch` (not `409 lease-conflict`).
- [x] `docs/guides/workers-runners.md`: default scopes from `issue-worker-api-key.ps1` are `work:*`, `workers:heartbeat/read`, `runners:heartbeat/read`.
- [x] `docs/guides/auth.md`: remove "after backlog item completes" wording for `/tenants/{tenantId}/api-keys`.
- [x] `docs/introduction/index.md`: replace "SDK reference (coming soon)" placeholder with existing doc links.
- [x] `docs/introduction/configuration.md`: remove "job-registration.md (upcoming)" wording and link directly.
- [x] `docs/introduction/configuration.md`: align default `Croniq__Api__RequestsPerMinute` with `CroniqApiOptions` (60).
- [x] `docs/introduction/quickstart.md`: remove stale "(to be added)" note and fix `cd HelloCroniq.Api` path.

### Ops and troubleshooting
- [x] `docs/ops/troubleshooting.md`: update "login failed" guidance to use `CRONIQ_SQL_HOST/CRONIQ_SQL_PASSWORD/CRONIQ_SQL_DATABASE` or compose-generated connection.
- [x] `docs/ops/container-images.md`: fix password auth env var example to `Croniq__Auth__Password__Enabled`.

### Samples and README
- [x] `samples/grpc-client-python/README.md`: API key env var should be `CRONIQ_API_KEY` (and `CRONIQ_ENDPOINT`).
- [x] `tests/README.md`: verify the port note for `9464` (not mapped on host).

### Docs hygiene
- [x] `docs/SECURITY.md` + `docs/deep-dive/release-verification.md` + `docs/deep-dive/supplychain-waivers.md`: remove stray `***` at EOF.

## Needs discussion / code change (decide: doc-only vs behavior change)
- [ ] (blocked until repo is public) Add docs publishing workflow (GitHub Pages) once the repo is public to avoid private-repo costs.
- [ ] (deferred - vNext) gRPC docs expansion for Python/Go/Node (optional Java) with packages, install, auth helpers, minimal examples.
- [x] `docs/deep-dive/architecture.md`: clock drift monitoring via ITimeProvider is documented but not implemented.
- [x] `docs/deep-dive/designs/dmz-ingress-remote-webhooks.md`: options mention `LeaseSeconds/MaxBatchSize/PollingIntervalMilliseconds`, but `WebhookIngressOptions` exposes only `DispatchMode`.
- [x] `docs/deep-dive/password-auth.md`: admin endpoints `/tenants/{tenantId}/users` and `/tenants/{tenantId}/users/{userId}/reset-password` are not implemented.
- [x] `docs/deep-dive/persistence.md`: `Croniq:Persistence:SqlServer:CommandTimeoutSeconds` is documented but not implemented.
- [x] Webhook remote: restrict `AllowInvalidServerCertificate` to dev-only; align samples/docs accordingly.
- [x] Tenant defaults: align `CRONIQ_TENANT_ID` to `default` for single-tenant samples/tests.
- [x] Smoke API key env: standardize on `CRONIQ_API_KEY` across tests/devstack.
- [x] Environment tag env: standardize on `CRONIQ_ENVIRONMENT` across samples/docs.
- [x] `Croniq.DbMigrator` CLI docs: decide whether to implement `--apply/--verify/--connection` or update docs to match current behavior.
- [x] Samples/READMEs: default `CRONIQ_API_KEY` now matches devstack (`smoke-key`).
- [x] JobKey format: docs/samples now use `namespace:name[:variant]` (tenant/environment come from scope).
- [x] `docs/guides/webhooks.md` + `docs/deep-dive/security.md`: replay/idempotency headers documented but not implemented.
- [x] `docs/deep-dive/security.md`: per-hook metadata enrichment toggle documented but not implemented.
- [x] `docs/deep-dive/security.md`: `cluster:read` scope documented but not implemented.
- [x] `docs/deep-dive/security.md`: webhook secrets are stored as plaintext but docs claim "hashed only".
- [x] `docs/deep-dive/security.md`: correlation/actor documented for all webhook management requests, but only IP rule CRUD sets them.
- [x] `docs/deep-dive/security.md`: payload size/content-type guardrails are documented but not implemented.
- [x] `docs/guides/webhooks.md`: ingress example no longer uses `X-Croniq-Key` (signature-only).
- [x] `docs/guides/webhooks.md` + `docs/introduction/quickstart.md` + `docs/deep-dive/architecture.md`: ingress route unified to `/tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}`.
- [x] `docs/deep-dive/architecture.md`: ingress processing stages no longer mention optional caller auth.
- [x] `docs/deep-dive/ci.md`: health probes align to `/health` (liveness) and `/health/persistence` (readiness).
- [x] Implement optional hashing for TenantId/CallerId in logs/metrics/traces; docs updated for the new config.
- [x] `docs/guides/auth.md`: `/health` with `X-Croniq-Debug: auth` is documented but not implemented.
- [x] `docs/guides/auth.md` + `docs/deep-dive/auth.md` + `docs/deep-dive/persistence.md` + `docs/deep-dive/architecture.md`: audit log table/retention claims are not backed by code.
- [x] `docs/index.md` + `docs/introduction/index.md`: auditing claims are documented but audit logging is not implemented.
- [x] `docs/deep-dive/architecture.md`: `IJobExecutionContext` progress APIs are documented but not implemented.
- [x] `docs/deep-dive/architecture.md` (and possibly `docs/deep-dive/policies.md`): fallback policy is documented but not implemented in `ExecutionPolicyPipelineProvider`.
- [x] `docs/deep-dive/architecture.md`: Serilog + OTel sink is documented, but default provider is `ILoggerFactory`.

### Hashing concept (implemented)
- Scope: hash TenantId/CallerId only at observability boundaries (logs, metrics, traces); keep raw values for auth and routing.
- Algorithm: HMAC-SHA256 with an environment-specific secret; emit a stable, lowercase hex digest to preserve joins.
- Toggle: `Croniq:Observability:HashIdentifiers` (default false) plus required `Croniq:Observability:IdentifierHashKey`.
- Centralize: `IdentifierHashing` helper keeps `ApiMetrics`, `PolicyMetrics`, `SchedulerMetrics`, and log scopes consistent.
- Docs: `docs/deep-dive/security.md`, `docs/deep-dive/observability.md`, and `docs/introduction/configuration.md` updated with config guidance.

## Pending doc scans
- [x] Scan `docs/README.md` + `docs/_templates/README.md` for current template/link guidance (no updates needed).
- [x] Scan `docs/ops/index.md` and `docs/ops/retention.md` for configuration/port consistency (no updates needed).
- [x] Scan `docs/guides/handlers.md`, `docs/guides/policies.md`, and `docs/guides/grpc.md` for API drift (updated `docs/guides/grpc.md`).
- [x] Scan `docs/deep-dive/ui.md`, `docs/deep-dive/kubernetes.md`, and `docs/deep-dive/sdk-worker-integration.md` for current-state accuracy (updated `docs/deep-dive/kubernetes.md` wording).
- [x] Scan `samples/worker-sdk-*` and `samples/grpc-client-{go,node,python}` READMEs for env var and JobKey consistency (updated `samples/grpc-client-go/README.md`).
