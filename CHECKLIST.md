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
- [x] Observability/Grafana abgeschlossen (Loki Tenant + Croniq Log Pulse Dashboard) – Plan siehe `docs/deep-dive/observability.md`
- [x] Loki-Tenant (`croniq-devstack`) und Explore-Hinweise in `docs/deep-dive/devstack.md` dokumentiert
- [x] SBOM/Signierung und Vulnerability Scans in Release-Flow einbauen – Plan siehe `docs/deep-dive/supplychain.md`
- [x] Quota-Guards im Core verankern (Rate/Concurrency) basierend auf PolicyResolver + Tests
- [ ] Docs Streams aufsetzen (docs root, docs/deep-dive) inkl. Quickstart & Mermaid policy – Plan siehe `docs/deep-dive/docstreams.md`
- [ ] UI-Backlog dokumentieren; Technologie nach API-Stabilisierung entscheiden – Plan siehe `docs/deep-dive/ui.md`
- [ ] Kubernetes Chart (charts/croniq) als Backlog-Platzhalter vorbereiten – Plan siehe `docs/deep-dive/kubernetes.md`

## Next Focus

1. Docstreams-Prozess etablieren (`docs/deep-dive/docstreams.md`), Quickstart synchronisieren und Consumer/Technical Docs laufend spiegeln.
2. UI-Backlog dokumentieren und Technologie-Entscheidung vorbereiten (`docs/deep-dive/ui.md`).

# Nachbesserungen

- [x] Suche im gesamten Repository nach "OpenConnectionAsync" (Provider-Artefakte ausklammern). Prüfe ob dort custom Prozedur calls mit "CommandText" vorgenommen werden? Ersetze diese durch die bereitgestellten Provider-Abstraktionen.
- [x] `docs\consumer\configuration.md` hier besteht ein Dokumentationsfehler oder gap: builder.Services.AddCroniq() gibt es nicht. Consumer Docs generell auf aktuellsten Stand bringen.
- [x] Ist es korrekt, dass `Croniq.Auth.SqlServer` einen Verweis auf `Croniq.Persistence.SqlServer` hat? Sollte die DbContext-Registrierung nicht eher in `Croniq.Data.SqlServer` stattfinden (bitte verifizieren, Empfehlungen aussprechen)? (Verifiziert: `Croniq.Auth.SqlServer` referenziert nur `Croniq.Data.SqlServer`, alle DbContext-DI-Erweiterungen leben bereits dort; Recommendation: Hosts rufen `AddCroniqSqlServerDbContext` aus `Croniq.Data.SqlServer` auf, bevor sie `AddCroniqAuthSqlServer` verkabeln.)
- [ ] Haben wir Webhooks bereits geplant? Wenn nein, bitte in `docs/deep-dive/architecture.md` und `CHECKLIST.md` aufnehmen.
- [ ] Dokumentiere mithilfe von Mermaid anstelle von drawio. Zur Visualisierung von Sequenzdiagrammen, Architekturskizzen und anderen Diagrammen. Alle neuen Diagramme in `docs/deep-dive/` sollten in Mermaid erstellt werden (Richtlinie siehe `docs/deep-dive/docstreams.md`).
