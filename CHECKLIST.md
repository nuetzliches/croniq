# CHECKLIST

- [x] Zielbild und Scope fuer Croniq skizziert
- [x] High-level Architektur (Core, Provider, Service Layer) definiert
- [x] Priorisierte Entscheidungen zu Scheduling-Syntax, Persistenz und Policies dokumentiert
- [x] Croniq.Api Hosting-Extensions (Add/UseCroniqApi) inkl. RateLimiter und konfigurierbare Auth/Persistenz-Modi (InMemory|Xtraq) fertiggestellt
- [x] Croniq.Data.Xtraq als shared Artefakte/DbContext eingebunden; ConnectionString als `Croniq:Xtraq` geteilt (Auth + Persistence)
- [x] Croniq.Core: Trigger/Job-Pipeline API entwerfen inkl. JobKey-Schema und DI-Konzept
- [x] Croniq.Sdk: IJob/IJobExecutionContext Contract finalisieren und Attribute dokumentieren
- [x] JobStore-Abstraktion: IJobStore/IJobPersistenceProvider Interfaces mit Locking/Partitionierung festziehen
- [x] Quartz-kompatiblen Cron-Parser auswaehlen oder portieren
- [x] Misfire-Policies + Resolver modelliert (MaxMisfireDelay, Dead-Letter-Markierungen, Override-Kaskade)
- [x] Provider-Vertraege: Logging-, Telemetry- und Secret-Provider Schnittstellen festlegen
- [x] Xtraq-Persistenz: Tabellen/UDTs/Procs fuer Jobs/Triggers/Leases/DeadLetter modelliert und idempotente Deploy-Skripte (apply.ps1) erstellt
- [x] API/RPC-Vertraege: Minimal API Endpunkte und gRPC Proto entwerfen
- [ ] Teststrategie: Unit/Contract/E2E Testplan mit Tools (xUnit, Testcontainers, Compose) detaillieren (`docs/technical/testing.md` enthaelt den Plan + Backlog)
- [ ] Security-Basis: API-Key/OAuth2 Flow und Rate Limiter Design ausarbeiten (Plan siehe `docs/technical/security.md`; Umsetzung OIDC/JWT + RateLimiter-Refactor steht aus)
- [ ] Observability: OTel/Serilog Setup und Dashboard-Kennzahlen festlegen (Plan siehe `docs/technical/observability.md`)
- [x] Repository-Struktur anlegen (src/, jobs/, infra/sql/xtraq, docs/)
- [x] Referenz-In-Memory-JobStore implementieren
- [x] Xtraq-Persistence-Provider prototypen inkl. Acquire/Release Trigger Procs (Croniq.Persistence.Xtraq + SQL-Skripte)
- [x] Auth-Provider-Umschaltung (InMemory/Xtraq) per Options integriert und SampleHost auf Xtraq verdrahtet
- [x] Policy-Engine auf Polly-Basis implementieren (Retry/Timeout/Circuit-Breaker) – Polly-Ausfuehrungspipeline (Timeout→CircuitBreaker→Retry), Dead-Letter-Persistenz sowie Telemetrie (PolicyMetrics + strukturierte Logs) per `docs/technical/policies.md` verdrahtet
- [x] Minimal API Skeleton mit Healthcheck, Schedule CRUD und Trigger Endpoint erstellen
- [x] gRPC SchedulerService Proto und Client SDK (Croniq.Rpc.Client) generieren
- [ ] Build/Test CI Pipelines (GitHub Actions) mit Lint/Coverage Gates einrichten – Plan siehe `docs/technical/ci.md`
- [ ] Docker Compose Dev-Stack (API, Worker, Xtraq, OTel/Grafana) bereitstellen – Plan siehe `docs/technical/devstack.md`
- [ ] SBOM/Signierung und Vulnerability Scans in Release-Flow einbauen – Plan siehe `docs/technical/supplychain.md`
- [x] Quota-Guards im Core verankern (Rate/Concurrency) basierend auf PolicyResolver + Tests
- [ ] Docs Streams aufsetzen (docs/consumer, docs/technical) inkl. Quickstart – Plan siehe `docs/technical/docstreams.md`
- [ ] UI-Backlog dokumentieren; Technologie nach API-Stabilisierung entscheiden – Plan siehe `docs/technical/ui.md`
- [ ] Kubernetes Chart (charts/croniq) als Backlog-Platzhalter vorbereiten – Plan siehe `docs/technical/kubernetes.md`

## Next Focus

1. Teststrategie-Dokument (`docs/technical/testing.md`) detaillieren, damit CI/E2E-Planung auf einer klaren Grundlage steht.
2. GitHub-Actions-Pipeline laut `docs/technical/ci.md` aufsetzen (Build + Tests + Coverage), sobald der Testplan final ist.

# Nachbesserungen

- [x] Suche im gesamten Repository nach "OpenConnectionAsync" (skip Xtraq-Artefakte). Prüfe ob dort custom Prozedur calls mit "CommandText" vorgenommen werden? Ersetze diese durch die Xtraq-Artefakte.
- [x] `docs\consumer\configuration.md` hier besteht ein Dokumentationsfehler oder gap: builder.Services.AddCroniq() gibt es nicht. Consumer Docs generell auf aktuellsten Stand bringen.
- [x] Ist es korrekt, dass `Croniq.Auth.Xtraq` einen Verweis auf `Croniq.Persistence.Xtraq` hat? Sollte die XtraqDbContext Registrierung nicht eher in `Croniq.Data.Xtraq` stattfinden (bitte verifizieren, Empfehlungen aussprechen)? (Verifiziert: `Croniq.Auth.Xtraq` referenziert nur `Croniq.Data.Xtraq`, alle XtraqDbContext-DI-Erweiterungen leben bereits dort; Recommendation: Hosts rufen `AddXtraqDbContext` aus `Croniq.Data.Xtraq` auf, bevor sie `AddCroniqAuthXtraq` verkabeln.)
