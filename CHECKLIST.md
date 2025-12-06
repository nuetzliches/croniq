# CHECKLIST

## Status jetzt
- [x] Zielbild und Scope fuer Croniq skizziert
- [x] High-level Architektur (Core, Provider, Service Layer) definiert
- [x] Priorisierte Entscheidungen zu Scheduling-Syntax, Persistenz und Policies dokumentiert
- [x] Croniq.Api Hosting-Extensions (Add/UseCroniqApi) inkl. RateLimiter und konfigurierbare Auth/Persistenz-Modi (InMemory|Xtraq) fertiggestellt
- [x] Croniq.Data.Xtraq als shared Artefakte/DbContext eingebunden; ConnectionString als `Croniq:Xtraq` geteilt (Auth + Persistence)

## In Arbeit
- [x] Croniq.Core: Trigger/Job-Pipeline API entwerfen inkl. JobKey-Schema und DI-Konzept
- [x] Croniq.Sdk: IJob/IJobExecutionContext Contract finalisieren und Attribute dokumentieren
- [x] JobStore-Abstraktion: IJobStore/IJobPersistenceProvider Interfaces mit Locking/Partitionierung festziehen
- [x] Quartz-kompatiblen Cron-Parser auswaehlen oder portieren
- [x] Misfire-Policies + Resolver modelliert (MaxMisfireDelay, Dead-Letter-Markierungen, Override-Kaskade)
- [x] Provider-Vertraege: Logging-, Telemetry- und Secret-Provider Schnittstellen festlegen
- [x] Xtraq-Persistenz: Tabellen/UDTs/Procs fuer Jobs/Triggers/Leases/DeadLetter modelliert und idempotente Deploy-Skripte (apply.ps1) erstellt
- [x] API/RPC-Vertraege: Minimal API Endpunkte und gRPC Proto entwerfen
- [ ] Teststrategie: Unit/Contract/E2E Testplan mit Tools (xUnit, Testcontainers, Compose) detaillieren
- [ ] Security-Basis: API-Key/OAuth2 Flow und Rate Limiter Design ausarbeiten (API-Key-Pfad + CallerContext Middleware vorhanden, OIDC/JWT noch offen)
- [ ] Observability: OTel/Serilog Setup und Dashboard-Kennzahlen festlegen

## Naechste Schritte
- [x] Repository-Struktur anlegen (src/, jobs/, infra/sql/xtraq, docs/)
- [x] Referenz-In-Memory-JobStore implementieren
- [x] Xtraq-Persistence-Provider prototypen inkl. Acquire/Release Trigger Procs (Croniq.Persistence.Xtraq + SQL-Skripte)
- [x] Auth-Provider-Umschaltung (InMemory/Xtraq) per Options integriert und SampleHost auf Xtraq verdrahtet
- [ ] Policy-Engine auf Polly-Basis implementieren (Retry/Timeout/Circuit-Breaker)
- [x] Minimal API Skeleton mit Healthcheck, Schedule CRUD und Trigger Endpoint erstellen
- [x] gRPC SchedulerService Proto und Client SDK (Croniq.Rpc.Client) generieren
- [ ] Build/Test CI Pipelines (GitHub Actions) mit Lint/Coverage Gates einrichten
- [ ] Docker Compose Dev-Stack (API, Worker, Xtraq, OTel/Grafana) bereitstellen
- [ ] SBOM/Signierung und Vulnerability Scans in Release-Flow einbauen
- [x] Quota-Guards im Core verankern (Rate/Concurrency) basierend auf PolicyResolver + Tests
- [ ] Docs Streams aufsetzen (docs/consumer, docs/technical) inkl. Quickstart
- [ ] UI-Backlog dokumentieren; Technologie nach API-Stabilisierung entscheiden
- [ ] Kubernetes Chart (charts/croniq) als Backlog-Platzhalter vorbereiten
