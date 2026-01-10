# CHECKLIST-DOCS

Documentation backlog extracted from `CHECKLIST.md`. Track doc-only and doc-alignment items here.

## Ready to implement (doc-only updates)

### Deep-dive docs
- [ ] `docs/deep-dive/testing.md`: switch to Shouldly, remove global.json 8.x reference, fix Observability cadence, update release workflow TODO.
- [ ] `docs/deep-dive/ci.md`: rename `ci-nightly.yml` references to `nightly.yml`.
- [ ] `docs/deep-dive/devstack.md` + `docs/deep-dive/supplychain.md`: rename `ci-nightly.yml` references to `nightly.yml`.
- [ ] `docs/deep-dive/devstack.md`: replace "future Croniq.Worker" with `Croniq.WorkerHost`.
- [ ] `docs/deep-dive/auth.md`: update testing section to point at real test projects.
- [ ] `docs/deep-dive/auth.md`: fix API key format to `ak_<guid>.<secret>`.
- [ ] `docs/deep-dive/auth.md`: fix tenant rate limits key to `Croniq:Api:TenantRateLimits`.
- [ ] `docs/deep-dive/architecture.md`: align retention defaults with `CroniqRetentionOptions` and `ExecutionLogRetentionOptions`.
- [ ] `docs/deep-dive/architecture.md`: update trigger types to current cron/@once + webhooks/manual.
- [ ] `docs/deep-dive/architecture.md`: fix quota description (JobKey/Scope with `MaxTriggersPerMinute` + `MaxParallelExecutionsPerJob`; API rate limits are separate).
- [ ] `docs/deep-dive/designs/webhook-secret-rotation.md`: remove "future helper script" note and align retention/cleanup notes to current options/defaults.
- [ ] `docs/deep-dive/release.md`: align steps with actual release workflows (no `Directory.Build.props` bump, no Snyk/Docs artifact if not used).
- [ ] `docs/deep-dive/password-auth.md`: fix config key names and update `docs/guides/triggers.md` from .NET 8 to net10.0.
- [ ] `docs/deep-dive/observability.md`: fix metric name to `cronijob_executions_total`.
- [ ] `docs/deep-dive/observability.md`: remove the Serilog registration claim (Default provider uses `ILoggerFactory`).
- [ ] `docs/deep-dive/devstack.md`: fix Prometheus/Tempo port defaults to 9090/3200.
- [ ] `docs/deep-dive/devstack.md`: fix default JobKey in step 5 (`default:dev:samples:smoke`).
- [ ] `docs/deep-dive/observability.md`: fix default JobKey in verification steps (`default:dev:samples:smoke`).
- [ ] `docs/deep-dive/devstack.md` + `docs/deep-dive/persistence.md`: clarify SQL connection sourcing (compose builds `CRONIQ_SQL_CONNECTION` from `CRONIQ_SQL_*`).
- [ ] `docs/deep-dive/persistence.md`: clarify scheduler vs auth schema (`croniq` vs `auth`).
- [ ] `docs/deep-dive/job-registration.md`: update backlog section (both `AddCroniqJobsFromAssembly` and validate-only startup mode exist).
- [ ] `docs/deep-dive/persistence.md`: update retention backlog note (cleanup is implemented).
- [ ] `docs/deep-dive/policies.md`: translate non-English sentences and normalize metric names to `cronipolicy_*`.
- [ ] `docs/deep-dive/designs/job-log-persistence.md`: update "Current State" to reflect the implemented execution log store.
- [ ] `docs/deep-dive/designs/polyglot-worker-protocol.md`: resolve WorkItems/WorkClaims contradiction.
- [ ] `docs/deep-dive/ci.md`: update backlog note to reflect completion.
- [ ] `docs/deep-dive/observability.md`: update backlog note to reflect completion.
- [ ] `docs/deep-dive/supplychain.md`: update "Provision signing keys" backlog item to current state.
- [ ] `docs/deep-dive/security.md`: update TenantGuard note (Scheduler RPC host already uses it).
- [ ] `docs/deep-dive/index.md`: remove "roadmap" label for schedules endpoint and update gRPC client note.
- [ ] `docs/deep-dive/docstreams.md`: align status with the checklist (blocked until repo public) and fix missing template references.

### Guides and introduction
- [ ] `docs/guides/triggers.md`, `docs/ops/retention.md`, `docs/deep-dive/architecture.md`: align cron field count (6 fields + optional year).
- [ ] `docs/guides/triggers.md`: update TriggerId default (Base64-url Cron + optional TimeZoneId, hash fallback >512 chars).
- [ ] `docs/guides/triggers.md` + `docs/deep-dive/security.md`: IP rule block is `403 ip-blocked` (not `429 ip-rule-denied`).
- [ ] `docs/guides/polyglot-workers.md`: runner mismatch returns `403 runner-mismatch` (not `409 lease-conflict`).
- [ ] `docs/guides/polyglot-workers.md`: default scopes from `issue-worker-api-key.ps1` are `work:*`, `workers:heartbeat/read`, `runners:heartbeat/read`.
- [ ] `docs/guides/auth.md`: remove "after backlog item completes" wording for `/tenants/{tenantId}/api-keys`.
- [ ] `docs/introduction/index.md`: replace "SDK reference (coming soon)" placeholder with existing doc links.
- [ ] `docs/introduction/configuration.md`: remove "job-registration.md (upcoming)" wording and link directly.
- [ ] `docs/introduction/configuration.md`: align default `Croniq__Api__RequestsPerMinute` with `CroniqApiOptions` (60).
- [ ] `docs/introduction/quickstart.md`: remove stale "(to be added)" note and fix `cd HelloCroniq.Api` path.

### Ops and troubleshooting
- [ ] `docs/ops/troubleshooting.md`: update "login failed" guidance to use `CRONIQ_SQL_HOST/CRONIQ_SQL_PASSWORD/CRONIQ_SQL_DATABASE` or compose-generated connection.
- [ ] `docs/ops/container-images.md`: fix password auth env var example to `Croniq__Auth__Password__Enabled`.

### Samples and README
- [ ] `samples/grpc-client-python/README.md`: API key env var should be `CRONIQ_API_KEY` (and `CRONIQ_ENDPOINT`).
- [ ] `tests/README.md`: verify the port note for `9464` (not mapped on host).

### Docs hygiene
- [ ] `docs/SECURITY.md` + `docs/deep-dive/release-verification.md` + `docs/deep-dive/supplychain-waivers.md`: remove stray `***` at EOF.

## Needs discussion / code change (decide: doc-only vs behavior change)
- [ ] Docstreams process (blocked until repo is public): create docs root + deep-dive streams, quickstart alignment, Mermaid policy.
- [ ] (deferred - vNext) gRPC docs expansion for Python/Go/Node (optional Java) with packages, install, auth helpers, minimal examples.
- [ ] `docs/deep-dive/architecture.md`: clock drift monitoring via ITimeProvider is documented but not implemented.
- [ ] `docs/deep-dive/designs/dmz-ingress-remote-webhooks.md`: options mention `LeaseSeconds/MaxBatchSize/PollingIntervalMilliseconds`, but `WebhookIngressOptions` exposes only `DispatchMode`.
- [ ] `docs/deep-dive/password-auth.md`: admin endpoints `/tenants/{tenantId}/users` and `/tenants/{tenantId}/users/{userId}/reset-password` are not implemented.
- [ ] `docs/deep-dive/persistence.md`: `Croniq:Persistence:SqlServer:CommandTimeoutSeconds` is documented but not implemented.
- [ ] Webhook remote: restrict `AllowInvalidServerCertificate` to dev-only; align samples/docs accordingly.
- [ ] Tenant defaults: align `CRONIQ_TENANT_ID` "1" (smoke tests + gRPC samples) vs "default" in core/devstack.
- [ ] Smoke API key env: align `CRONIQ_API_KEY` vs `CRONIQ_SMOKE_API_KEY` across tests/devstack.
- [ ] Environment tag env: align `CRONIQ_ENVIRONMENT_TAG` vs `CRONIQ_ENVIRONMENT` across samples/docs.
- [ ] `Croniq.DbMigrator` CLI docs: decide whether to implement `--apply/--verify/--connection` or update docs to match current behavior.
- [ ] Samples/READMEs: `CRONIQ_API_KEY=dev-key` vs devstack `.env.example` `CRONIQ_SMOKE_API_KEY=smoke-key` (pick one default or document the difference).
- [ ] JobKey format: parser accepts `namespace:name[:variant]`, but docs/samples use `tenant:env:namespace:name`.
- [ ] `docs/guides/triggers.md` + `docs/deep-dive/security.md`: replay/idempotency headers documented but not implemented.
- [ ] `docs/deep-dive/security.md`: per-hook metadata enrichment toggle documented but not implemented.
- [ ] `docs/deep-dive/security.md`: `cluster:read` scope documented but not implemented.
- [ ] `docs/deep-dive/security.md`: webhook secrets are stored as plaintext but docs claim "hashed only".
- [ ] `docs/deep-dive/security.md`: correlation/actor documented for all webhook management requests, but only IP rule CRUD sets them.
- [ ] `docs/deep-dive/security.md`: payload size/content-type guardrails are documented but not implemented.
- [ ] `docs/guides/triggers.md`: ingress example uses `X-Croniq-Key`, but ingress ignores API keys.
- [ ] `docs/guides/triggers.md` + `docs/introduction/quickstart.md` + `docs/deep-dive/architecture.md`: ingress route mismatch (`/webhooks/{hookKey}` vs `/tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}`).
- [ ] `docs/deep-dive/architecture.md`: processing stages mention optional caller auth for ingress, but `Croniq.Webhooks` does not validate auth.
- [ ] `docs/deep-dive/ci.md`: health probe is documented as `/webhooks/health`, but host exposes `/health`.
- [ ] `docs/deep-dive/security.md`: "TenantId/CallerId only after hashing" does not match current log/metric tagging.
- [ ] `docs/guides/auth.md`: `/health` with `X-Croniq-Debug: auth` is documented but not implemented.
- [ ] `docs/guides/auth.md` + `docs/deep-dive/auth.md` + `docs/deep-dive/persistence.md` + `docs/deep-dive/architecture.md`: audit log table/retention claims are not backed by code.
- [ ] `docs/index.md` + `docs/introduction/index.md`: auditing claims are documented but audit logging is not implemented.
- [ ] `docs/deep-dive/architecture.md`: `IJobExecutionContext` progress APIs are documented but not implemented.
- [ ] `docs/deep-dive/architecture.md` (and possibly `docs/deep-dive/policies.md`): fallback policy is documented but not implemented in `ExecutionPolicyPipelineProvider`.
- [ ] `docs/deep-dive/architecture.md`: Serilog + OTel sink is documented, but default provider is `ILoggerFactory`.

## Pending doc scans
- [ ] Scan `docs/README.md` + `docs/_templates/README.md` for current template/link guidance.
- [ ] Scan `docs/ops/index.md` and `docs/ops/retention.md` for configuration/port consistency.
- [ ] Scan `docs/guides/handlers.md`, `docs/guides/policies.md`, and `docs/guides/grpc.md` for API drift.
- [ ] Scan `docs/deep-dive/ui.md`, `docs/deep-dive/kubernetes.md`, and `docs/deep-dive/sdk-worker-integration.md` for current-state accuracy.
- [ ] Scan `samples/worker-sdk-*` and `samples/grpc-client-{go,node,python}` READMEs for env var and JobKey consistency.
