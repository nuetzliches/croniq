# CHECKLIST

- [x] Zielbild und Scope fuer Croniq skizziert
- [x] High-level Architektur (Core, Provider, Service Layer) definiert
- [x] Priorisierte Entscheidungen zu Scheduling-Syntax, Persistenz und Policies dokumentiert
- [x] Croniq.Api Hosting-Extensions (Add/UseCroniqApi) inkl. RateLimiter und konfigurierbare Auth/Persistenz-Modi (InMemory|SqlServer) fertiggestellt
- [x] Croniq.Data.SqlServer als shared DbContext eingebunden; ConnectionString als `Croniq:SqlServer` geteilt (Auth + Persistence)
- [x] Croniq.Core: Trigger/Job-Pipeline API entwerfen inkl. JobKey-Schema und DI-Konzept
- [x] Croniq.Sdk: IJob/IJobExecutionContext Contract finalisieren und Attribute dokumentieren
- [x] JobStore-Abstraktion: IJobStore/IJobPersistenceProvider Interfaces mit Locking/Partitionierung festziehen
- [x] Quartz-kompatiblen Cron-Parser auswaehlen oder portieren
- [x] Misfire-Policies + Resolver modelliert (MaxMisfireDelay, Dead-Letter-Markierungen, Override-Kaskade)
- [x] Provider-Vertraege: Logging-, Telemetry- und Secret-Provider Schnittstellen festlegen
- [x] SqlServer-Persistenz: EF-Core-Modelle + Migrationen fuer Jobs/Trigger/DeadLetter erstellt und `Croniq.DbMigrator` fuer Deployments verdrahtet
- [x] API/RPC-Vertraege: Minimal API Endpunkte und gRPC Proto entwerfen
- [x] devstack gehört nicht in die consumer docs, sondern in die technical docs (quickstart.md anpassen)
- [x] Policy-Engine auf Polly-Basis implementieren (Retry/Timeout/Circuit-Breaker) – Polly-Ausfuehrungspipeline (Timeout→CircuitBreaker→Retry), Dead-Letter-Persistenz sowie Telemetrie (PolicyMetrics + strukturierte Logs) per `docs/deep-dive/policies.md` verdrahtet
- [x] Minimal API Skeleton mit Healthcheck, Schedule CRUD und Trigger Endpoint erstellen
- [x] gRPC SchedulerService Proto und Client SDK (Croniq.Rpc.Client) generieren
- [x] Build/Test CI Pipelines (GitHub Actions) mit Lint/Coverage Gates einrichten – Plan siehe `docs/deep-dive/ci.md`
- [x] Docker Compose Dev-Stack (API, Worker, SqlServer, OTel/Grafana) bereitstellen – Plan siehe `docs/deep-dive/devstack.md`
- [x] Developer UX/DX: `scripts/devstack-up` startet zusätzlich die UI im Dev-Serve/Watch-Modus (Log + Link zum UI-Endpunkt reicht) inkl. kurzer Doku; Dev-only, nicht als End-User-Serving verstehen
- [x] Devstack Build-Performance: Shared Docker Build (multi-target/shared layers) für ApiHost/Worker/Migrator, um Rebuild-Zeiten zu reduzieren
- [x] Observability/Grafana abgeschlossen (Loki Tenant + Croniq Log Pulse Dashboard) – Plan siehe `docs/deep-dive/observability.md`
- [x] Loki-Tenant (`croniq-devstack`) und Explore-Hinweise in `docs/deep-dive/devstack.md` dokumentiert
- [x] SBOM/Signierung und Vulnerability Scans in Release-Flow einbauen – Plan siehe `docs/deep-dive/supplychain.md`
- [x] Quota-Guards im Core verankern (Rate/Concurrency) basierend auf PolicyResolver + Tests
- [ ] Long-running Jobs: Lease-Heartbeat/Extend + Konzept fuer Ausfuehrungen > LeaseDuration; Default Timeout/Lease aufeinander abstimmen und dokumentieren
- [x] Webhook-Persistenz hardenen (EF-Migration `AddWebhookEndpoints`, DbMigrator, CRUD-Tests) - Plan siehe `docs/deep-dive/architecture.md`
- [x] Webhook-Operations ausbauen: Changefeed-basierte Cache-Invalidierung, Dual-Secret-Rotation (WebhookSecretHistory) plus CLI/SDK-Helfer dokumentieren (Dead-Letter-Tabelle + Replay-API umgesetzt)
- [x] FluentAssertions -> Shouldly Migration (MIT-only policy)
  - [x] MIT-kompatible Lizenzvorgabe in AI_ASSISTANT_INSTRUCTIONS.md dokumentiert (2025-12-11)
  - [x] Croniq.Sdk.Tests auf Shouldly portiert und FluentAssertions-Paket entfernt
  - [x] Restliche Testprojekte iterativ migrieren (Croniq.Api.Smoke, Providers.Default, JobStore.InMemory, Observability, Persistence.SqlServer, Api.Tests, Core – abgeschlossen 2025-12-12)
  - [x] CI-License-Scan (Syft + Allowlist + nightly/release gating) hinzugefügen und in docs/deep-dive/supplychain.md beschreiben (2025-12-12)
- [ ] (blocked bis Repo public) Docs Streams aufsetzen (docs root, docs/deep-dive) inkl. Quickstart & Mermaid policy – Plan siehe `docs/deep-dive/docstreams.md`
- [ ] (deferred – wartet auf expliziten Stakeholder-Request) Kubernetes Chart (charts/croniq) als Backlog-Platzhalter vorbereiten – Plan siehe `docs/deep-dive/kubernetes.md`
- [ ] (deferred) Workflow-Ausführungen an Execution-Logging anbinden (ExecutionKind/WorkflowId nutzen; eigenes Interface/Adapter bei Workflow-Feature einziehen)
- [x] CI/CD Validation Backlog abschließen (`docs/deep-dive/ci.md`): `ci-pr.yml`, reusable Scripts, Coverage-Kommentar, automatisches Staging-Deploy sowie Toolchain-Pinning + Secrets-Runbook sind stand 2025-12-10 umgesetzt.
- [x] Policy-Dokumentation & Observability vervollständigen (`docs/deep-dive/policies.md`): Konfigurationsbeispiele dokumentieren sowie Dashboards/Alerts gemäß Observability-Plan verdrahten.
- [x] Security/Bearer-Basis liefern (`docs/deep-dive/security.md`): Bearer-Token-Validierung + Dual-Auth-Middleware, CallerContext + RateLimiter-Partitionierung, gRPC-Interceptor, Admin-APIs, Doku und Regressionstests implementieren.
- [x] Supply-Chain-Nacharbeiten (`docs/deep-dive/supplychain.md`): Signing Keys bereitstellen, Verification-Doku + Waiver-Prozess ergänzt (`docs/deep-dive/release-verification.md`, `docs/deep-dive/supplychain-waivers.md`, `docs/SECURITY.md`). (`syft`/`trivy` Toolchain + lokale Anleitung erledigt 2025-12-12.)
- [x] gRPC-Clients/Samples: Neben `Croniq.Rpc.Client` (.NET) schlanke gRPC-Client-Samples/SDK-Snippets für Python, Go, Node (nur Proto + Auth/Metadata Helpers) bereitstellen; kein WorkerHost nötig; Java nur bei Bedarf.
- [x] gRPC Observability & CI: gRPC-Routen mit OTel/Activity-Tags (Tenant/Environment/Job/Trigger) versehen; Sample-Syntax/Build-Checks via `eng/validate-grpc-samples.ps1` in CI (`ci-pr.yml`) verdrahtet.
- [ ] (deferred – vNext) gRPC Client Packages (non-.NET): Pro Sprache ein leichtes Paket inkl. generierter Stubs + kleinem Helper bereitstellen und dokumentieren (Python/PyPI, Go/Go module, Node/NPM); Samples referenzieren diese Pakete statt lokaler Stubs.
- [ ] (deferred – vNext) gRPC Docs ausbauen: Sprachspezifische Abschnitte (Python/Go/Node, optional Java) mit Paketnamen, Installation, Auth/Metadata-Helpers und Minimalbeispielen ergänzen.
- [x] Webhook-Trigger (Croniq.Webhooks Projekt) planen, host implementieren und in `docs/deep-dive/architecture.md` verankern (inkl. CRUD-API + persistente Hooks)
- [x] Job-Log-Persistenz (Plan siehe `docs/deep-dive/designs/job-log-persistence.md`)
  - [x] ExecutionId/Correlation im Scheduler-Pipeline-Scope propagieren; `ExecutionLogSink` + opt-in (`IExecutionLogStore`/`IExecutionLogReader`/Exporter) anlegen
  - [x] Store/Modi (Filesystem NDJSON, No-Op) + Optionen/Retention-Service verdrahten; Reader bereitstellen
  - [x] Tests + Failure-Handling (Drop/Buffer) absichern; CLI-Reader bereitstellen (API-Endpoint vorhanden)
- [ ] Coverage-Ziel: Core/Overall ≥ 80 % erreichen (Gates nachziehen, wenn stabil)
- [x] Croniq-internes Token-Issuing: Admin-Endpunkte für Tenant-Onboarding, API-Client-Registrierung und Bearer-Token-Ausstellung (`POST /tenants`, `GET /tenants/{id}`, `POST /tenants/{id}/api-clients`, `POST /tenants/{id}/tokens`, `GET /me`) samt Dokumentation in `docs/deep-dive/auth.md` implementieren (Croniq.Api `ApiHostingExtensions` + Tests `TenantAdminEndpointsTests` + docs aktualisiert am 2025-12-15).
- [x] API-Scope-Konsistenz für `/schedules`: Endpoint ist jetzt ausschließlich tenant-scoped (`POST /tenants/{tenantId}/schedules`, 2025-12-15) und alle Clients/Docs referenzieren den neuen Pfad.
- [x] Croniq.Api Verwaltungsendpunkte vervollständigen (Jobs/Schedules/Executions/API-Clients):
- [x] Schedule-CRUD fertiggestellt: `GET /tenants/{tenantId}/schedules`, `GET /tenants/{tenantId}/schedules/{triggerId}` (inkl. optionaler `jobKey`-Filter) sowie `DELETE /tenants/{tenantId}/schedules/{triggerId}` sind umgesetzt [src/Croniq.Api/ApiHostingExtensions.cs#L213-L315](src/Croniq.Api/ApiHostingExtensions.cs#L213-L315) und liefern `ScheduleResponse`. Mapping-Helfer liegt in [src/Croniq.Api/ApiHostingExtensions.cs#L1018-L1035](src/Croniq.Api/ApiHostingExtensions.cs#L1018-L1035); Tests decken Listing/Get/Delete über [tests/Croniq.Api.Tests/ScheduleEndpointsTests.cs](tests/Croniq.Api.Tests/ScheduleEndpointsTests.cs) ab.
- [x] Job-Verwaltung: Tenant-scoped `GET/POST/DELETE /tenants/{tenantId}/jobs?environment=` inklusive `JobResponse`/`UpsertJobRequest` DTOs und Scope-Enforcement umgesetzt ([src/Croniq.Api/ApiHostingExtensions.cs#L152-L311](src/Croniq.Api/ApiHostingExtensions.cs#L152-L311), [src/Croniq.Api/Models/ScheduleRequests.cs#L20-L41](src/Croniq.Api/Models/ScheduleRequests.cs#L20-L41)). `IJobPersistenceProvider` kennt jetzt Job-Liste/Get/Delete und beide Provider + Tests wurden erweitert ([src/Croniq.Persistence.Abstractions/IJobPersistenceProvider.cs#L12-L20](src/Croniq.Persistence.Abstractions/IJobPersistenceProvider.cs#L12-L20), [src/Croniq.JobStore.InMemory/InMemoryJobStore.cs#L31-L141](src/Croniq.JobStore.InMemory/InMemoryJobStore.cs#L31-L141), [src/Croniq.Persistence.SqlServer/SqlServerJobPersistenceProvider.cs#L72-L159](src/Croniq.Persistence.SqlServer/SqlServerJobPersistenceProvider.cs#L72-L159), [tests/Croniq.JobStore.InMemory.Tests/InMemoryJobStoreTests.cs#L93-L137](tests/Croniq.JobStore.InMemory.Tests/InMemoryJobStoreTests.cs#L93-L137), [tests/Croniq.Persistence.SqlServer.Tests/SqlServerJobPersistenceProviderTests.cs#L66-L134](tests/Croniq.Persistence.SqlServer.Tests/SqlServerJobPersistenceProviderTests.cs#L66-L134)). API-Verhalten wird über [tests/Croniq.Api.Tests/JobEndpointsTests.cs](tests/Croniq.Api.Tests/JobEndpointsTests.cs) abgedeckt.
- [x] Execution-Übersicht fertiggestellt: `GET /tenants/{tenantId}/executions` + `GET /tenants/{tenantId}/executions/{executionId}` listen bzw. liefern Execution-Snapshots mit Filteroptionen, abgesichert über `executions:read` Scope [src/Croniq.Api/ApiHostingExtensions.cs#L200-L330](src/Croniq.Api/ApiHostingExtensions.cs#L200-L330). Die Daten kommen aus dem neuen `IExecutionHistoryReader` + File-basiertem Reader, der NDJSON-Starts/Completions auswertet [src/Croniq.Core/Execution/FileExecutionHistoryReader.cs](src/Croniq.Core/Execution/FileExecutionHistoryReader.cs); Tests decken Reader und API ab [tests/Croniq.Core.Tests/Execution/FileExecutionHistoryReaderTests.cs](tests/Croniq.Core.Tests/Execution/FileExecutionHistoryReaderTests.cs), [tests/Croniq.Api.Tests/ExecutionEndpointsTests.cs](tests/Croniq.Api.Tests/ExecutionEndpointsTests.cs).
- [x] API-Clients & Tokens: CRUD (`GET/POST/DELETE /tenants/{tenantId}/api-clients`) plus Token-Issuing (`POST /tenants/{tenantId}/tokens`, `/api-clients/{clientId}/tokens`) und `/me` ausgeliefert inkl. Tests/Docs [src/Croniq.Api/ApiHostingExtensions.cs#L163-L1294](src/Croniq.Api/ApiHostingExtensions.cs#L163-L1294), [tests/Croniq.Api.Tests/ApiKeyAdminIntegrationTests.cs#L19-L210](tests/Croniq.Api.Tests/ApiKeyAdminIntegrationTests.cs#L19-L210), [docs/deep-dive/auth.md#L32-L160](docs/deep-dive/auth.md#L32-L160).

## Deferred: Remote Persistence (Hosted)

- [ ] Architekturskizze `Croniq.Persistence.Remote` (Client) + `Croniq.Persistence.Remote.Service` (Service-Seite): Transport, Auth (ApiKey/Bearer), Throttling, Tenant-Isolation.
- [ ] Evaluieren, ob vorhandene `Croniq.Api`-Endpoints erweitert werden oder ein separates Service-Repo nötig ist; Migrationsplan dokumentieren.
- [ ] Sicherheits-/Governance-Aspekte festhalten (Tenant-Isolation, SLAs, Secrets, Observability).
- [ ] Betriebs- und Provisionierungs-Runbook (Deploy-Topologie, Monitoring, Kostenkontrolle).
- [ ] SDK/Worker-Integration definieren (Konfig, Failover/Offline-Strategie, Fallback auf lokale Persistence).

## Next Focus

1. (Done 2025-12-09) Webhook-CRUD/API abgesichert: Authz-Scopes vereinheitlicht, Integrationstests für CRUD/Rotate/Dead-Letter-Endpunkte laufen über den neuen TestHost.
2. (Blocked bis Repo public) Docstreams-Prozess etablieren (`docs/deep-dive/docstreams.md`), Quickstart synchronisieren und Consumer/Technical Docs laufend spiegeln.
3. CI + Supplychain Hardening fortsetzen (`docs/deep-dive/ci.md`, `docs/deep-dive/supplychain.md`): Coverage-Gates automatisieren, SBOM/Signierung in Release-Pipeline finalisieren.
4. Security/Auth + Plattform-Scaffolding adressieren (`docs/deep-dive/security.md`, `docs/deep-dive/kubernetes.md`, `docs/deep-dive/ui.md`): Auth-Flows und Secrets-Governance präzisieren, Kubernetes-Chart + UI-Backlog grobplanen.

## Outstanding Backlog (Audit 2025-12-10)

- **Docstreams & Docs Hygiene**: `docs/deep-dive/docstreams.md` + Quickstart spiegeln weiterhin Consumer/Technical-Divergenzen; Stream-Owner-Workflow erst nach Repo-Öffnung aktivierbar (siehe offenes Checklist-Item "Docs Streams").
- **CI & Supplychain**: `docs/deep-dive/ci.md` und `docs/deep-dive/supplychain.md` listen ungelöste Tasks (Toolchain-Pinning, `eng/`-Assets, Secrets-Runbook, Waiver-Prozess); das automatische Staging-Deploy via `deploy-staging.yml`/`release.yml` ist erledigt.
- **Security & Auth**: `docs/deep-dive/security.md` markiert offene Arbeiten an Auth-Flows, Secrets-Governance sowie Hardenings; entsprechendes Checklist-Item "Security/Bearer-Basis" hinzugefügt.
- **Kubernetes & UI Scaffolding**: Platzhalter in `docs/deep-dive/kubernetes.md` und `docs/deep-dive/ui.md` beschreiben fehlende Chart-Baseline, UI-Tech-Entscheid und Content-Aufbereitung (Checklist-Items "UI-Backlog" und "Kubernetes Chart").

## Webhook-Trigger-Konzept (Backlog)

- **Use Cases**: Eingehende HTTP-Events (z.B. externe Systeme, Hooks, Custom Apps) sollen Croniq-Jobs auslösen – etwa wenn Payment eingetroffen ist oder Deployments Jobs anstoßen. Webhooks agieren damit als Trigger-Quelle neben Cron, Intervallen und RPC.
- **Croniq.Webhooks Service**: Eigenes ASP.NET-Hostprojekt stellt pro Tenant konfigurierbare Routen `/webhooks/{hookKey}` bereit. Samples (`Croniq.Sample.ApiHost`) binden es optional ein, Services können es separat deployen und skalieren. Jeder Hook verweist auf ein Job/Trigger-Mapping und kann optional Payload-Transformationen definieren.
- **Konfiguration**: `Croniq:Webhooks:Mode = InMemory|SqlServer`. SqlServer speichert Hooks samt Secrets, RateLimits, Payload-Schema. InMemory bietet minimale Konfiguration für Samples. Admin-API bietet CRUD (`POST /tenants/{id}/webhooks`).
- **Processing Pipeline**: Eingehender Request → Auth/Signature-Check → RateLimiter → Payload Normalizer → Job Dispatch (`TriggerJobAsync`). Fehlschläge landen in einer `WebhookIngressDeadLetter`-Queue; Retry-Policy getrennt von regulären Job-Policies.
- **Security & Governance**: Jeder Hook besitzt Secret + optional IP-Allowlist. Signatur-Header (z.B. `X-Croniq-Signature`) unterstützt HMAC-SHA256. Hooks können dedizierte rate limits (`RequestsPerMinute`, Burst) erhalten, Logging/Observability taggt Events mit `hookKey`.
- **Dokumentation**: `docs/deep-dive/architecture.md` ergänzt um Trigger-Flussdiagramm, `docs/guides/triggers.md` erhält Beispiel (cURL + Payload). Quickstart listet Webhook als zusätzliche Trigger-Option sobald GA.

# Nachbesserungen

- [x] Suche im gesamten Repository nach "OpenConnectionAsync" (Provider-Artefakte ausklammern). Prüfe ob dort custom Prozedur calls mit "CommandText" vorgenommen werden? Ersetze diese durch die bereitgestellten Provider-Abstraktionen.
- [x] `docs\consumer\configuration.md` hier besteht ein Dokumentationsfehler oder gap: builder.Services.AddCroniq() gibt es nicht. Consumer Docs generell auf aktuellsten Stand bringen.
- [x] Ist es korrekt, dass `Croniq.Auth.SqlServer` einen Verweis auf `Croniq.Persistence.SqlServer` hat? Sollte die DbContext-Registrierung nicht eher in `Croniq.Data.SqlServer` stattfinden (bitte verifizieren, Empfehlungen aussprechen)? (Verifiziert: `Croniq.Auth.SqlServer` referenziert nur `Croniq.Data.SqlServer`, alle DbContext-DI-Erweiterungen leben bereits dort; Recommendation: Hosts rufen `AddCroniqSqlServerDbContext` aus `Croniq.Data.SqlServer` auf, bevor sie `AddCroniqAuthSqlServer` verkabeln.)
- [x] Convenience-Hook bauen, der aus der Konfiguration das passende Provider-Modul zieht; momentan muss man `AddCroniqWebhooksSqlServer` im Startup explizit aufrufen.
- [x] `CONTRIBUTING.md` aktualisieren (veraltete Inhalte z.B. `Consumer docs` -> `Croniq docs`) – Stand 2025-12-10 mit Docstreams-Hinweisen synchronisiert
- [x] Signaturen für Webhooks per Opt-Out deaktivierbar machen (env, config, fluent)?
- [x] Suche nach `- [ ]` und prüfe, was wir noch zu erledigen haben bzw. ob es veraltete Tasks sind (2025-12-10). Ergebnis siehe Abschnitt "Outstanding Backlog".

## Zwischenstand 2025-12-09

- Webhook-Verwaltung: Admin-API vollständig durchgetestet (CRUD, Secret-Rotation, Dead-Letter Replay) via neuem `WebhookApiTestHost` + In-Memory-Doubles; sichert vorherige Hardening-Arbeit.
- Offene Punkte:
  - Docs/Comms: Docstreams-Aufbau (blocked bis Repo public), CONTRIBUTING-Refresh, offene Quickstart/Consumer-Divergenzen.
  - Delivery Backlog: UI-Dokumentation, Kubernetes-Chart-Platzhalter, globales Audit der verbleibenden `- [ ]` Items.

# Logging Plan (2025-12-13)

- [x] Audit Croniq-eigene Logs je Projekt (Api, Webhooks, Worker, Persistence, Core): Lifecycle/Polling auf `Debug`/`Trace` herunterstufen, nur fachliche Zustandswechsel und extern sichtbare Aktionen auf `Information` lassen; `Warning`/`Error` nur bei Degradierung/Fehler.
- [x] Fehlende strukturierte Felder ergänzen (Tenant, Environment, InstanceId, HookKey/JobKey/ExecutionId) und `SourceContext` konsistent halten; Telemetrie-Tags mit Logs alignen.
- [x] Host-/Framework-Noise dämpfen: `MinimumLevelOverrides` standardisieren (Hosting.Diagnostics, EF Command, Lifetime, Kestrel) und als Defaults in Samples/Docs festhalten.
- [x] Logging-Guidelines in `docs/deep-dive/observability.md` verankern (Level-Definitionen, wann loggen, Noisy-Patterns vermeiden, Payload-/PII-Hinweise, strukturierte Templates).
- [x] Smoke-/Devstack-Check: Samples (InMemory/SqlServer) mit Overrides starten und verifizieren, dass keine Request- oder EF-Spam-Logs mehr auf `Information` landen.

# Security / Tenant-Isolation (2025-12-14)

- [x] Wie stellen wir eigentlich sicher, dass User nur auf zugewiesene Tenants zugreifen können?
  - [x] Audit aller API/gRPC-Endpunkte auf Tenant-Enforcement (Route/Query/metadata vs. CallerContext.TenantId) und 403 bei Mismatch (REST abgeschlossen; gRPC folgt, sobald der Scheduler-RPC-Host landet).
  - [x] Zentralen Tenant-Guard (Middleware/Filter) ergänzen, der den Abgleich erzwingt (inkl. Execution-Log-Metadaten).
  - [x] Integrationstests “cross-tenant access denied” (REST hinzugefügt; gRPC-Suite pending RPC-Surface).
  - [x] Bearer-Token-Validierung härten: required scopes + tenant-claim Pflicht, andernfalls 401/403.
  - [x] Docs notieren: Keys bleiben single-tenant; Cross-Tenant nur via Admin/Ops-Identitäten mit strengen Guards.

# Prüfen, beantworten, ggf. umsetzen

- [x] Können wir die Konfiguration des `otelBuilder` in `samples\Croniq.Sample.WorkerHost\Program.cs` so vereinfachen wie in `samples\Croniq.Sample.ApiHost\Program.cs`?
- [x] `src\Croniq.Core\Execution\IJobExecutionPipeline.cs` Naming-Check abgeschlossen: Für das aktuelle Scope bleibt das Interface job-spezifisch; Workflows würden ein eigenes Interface (`IWorkflowExecutionPipeline`) oder einen generischen `IExecutionPipeline` erhalten, sobald das Feature gestartet wird. Kein sofortiger Umbau nötig.
- [x] Route `/me` in `/profile` umbenennen? (Entscheidung: nein. `/me` bleibt bestehen, da bereits von Clients/Docs genutzt; Umbenennung wäre unnötiger Breaking-Change.)
- [x] (akut) `WithDocs` wird in Swagger nicht angezeigt: sicherstellen, dass OpenAPI Summary/Description im UI sichtbar sind (z.B. via `WithOpenApi(...)`-Integration bzw. korrigierte Fallback-Strategie).
- [x] Username/Passwort Login für BearerTokens: Konzept + Implementierung (Tenant-Isolation, Scopes, RateLimits, Lockout, Refresh-Token-Rotation)
  - [x] Entscheidung: Standard-Login über HTTPS (Ja, bereits entschieden) (Server verifiziert Password) vs. PAKE (OPAQUE/SRP) wenn "Passwort nie übertragen" zwingend ist
  - [x] Implementiert: `/auth/login`, `/auth/refresh`, `/auth/logout` inkl. Refresh-Token-Rotation, Lockout, DefaultTenant-Auflösung und `tenantReference` (Tests: `PasswordAuthEndpointsTests`)
  - [x] Konzept-Doku: `docs/deep-dive/password-auth.md` (Option A baseline, Option B PAKE outline)
  - [x] Persistenz/Seed: `PasswordChangeRequired` im User-Record + Seed `admin/admin` mit `PasswordChangeRequired=true`
  - [x] "Change password" Endpoint + Flow: `POST /auth/change-password` (oder ähnlich) inkl. Enforcement + UI-Flow
- [ ] (nice-to-have) Solutionweit usings aufräumen?
- [ ] CI Static Analysis / SAST: entscheiden und ggf. integrieren
  - [ ] CodeQL-Code-Scanning Workflow hinzufügen (optional; abhängig von GHAS/Repo-Settings)
  - [ ] SonarQube/SonarCloud evaluieren (Signal/Noise, Kosten, Gate-Policy)
  - [ ] Roslyn-Analyzer-Set/Ruleset prüfen (z.B. .editorconfig/Directory.Build.props) und nur High-Signal-Regeln aktivieren
