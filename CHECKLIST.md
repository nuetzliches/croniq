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
- [x] Webhook-Persistenz hardenen (EF-Migration `AddWebhookEndpoints`, DbMigrator, CRUD-Tests) – Plan siehe `docs/deep-dive/architecture.md`
- [x] Webhook-Operations ausbauen: Changefeed-basierte Cache-Invalidierung, Dual-Secret-Rotation (WebhookSecretHistory) plus CLI/SDK-Helfer dokumentieren (Dead-Letter-Tabelle + Replay-API umgesetzt)
- [ ] (blocked bis Repo public) Docs Streams aufsetzen (docs root, docs/deep-dive) inkl. Quickstart & Mermaid policy – Plan siehe `docs/deep-dive/docstreams.md`
- [ ] (deferred) UI-Backlog dokumentieren; Technologie nach API-Stabilisierung entscheiden – Plan siehe `docs/deep-dive/ui.md`
- [ ] (deferred) Kubernetes Chart (charts/croniq) als Backlog-Platzhalter vorbereiten – Plan siehe `docs/deep-dive/kubernetes.md`
- [x] Webhook-Trigger (Croniq.Webhooks Projekt) planen, host implementieren und in `docs/deep-dive/architecture.md` verankern (inkl. CRUD-API + persistente Hooks)

## Next Focus

1. (abgeschlossen 2025-12-09) Webhook-CRUD/API abgesichert: Authz-Scopes vereinheitlicht, Integrationstests für CRUD/Rotate/Dead-Letter-Endpunkte laufen über den neuen TestHost.
2. (Deferred) Docstreams-Prozess etablieren (`docs/deep-dive/docstreams.md`), Quickstart synchronisieren und Consumer/Technical Docs laufend spiegeln – sobald das Repo public ist.
3. Convenience-Webhooks (Provider-Autokonfiguration) + Sample-otelBuilder aufräumen, sobald offene Nachbesserungen adressiert sind.

## Webhook-Trigger-Konzept (Backlog)

- **Use Cases**: Eingehende HTTP-Events (z.B. external Systeme, SaaS Hooks, Custom Apps) sollen Croniq-Jobs auslösen – etwa wenn Payment eingetroffen ist oder Deployments Jobs anstoßen. Webhooks agieren damit als Trigger-Quelle neben Cron, Intervallen und RPC.
- **Croniq.Webhooks Service**: Eigenes ASP.NET-Hostprojekt stellt pro Tenant konfigurierbare Routen `/webhooks/{hookKey}` bereit. Samples (`Croniq.Sample.ApiHost`) binden es optional ein, Services können es separat deployen und skalieren. Jeder Hook verweist auf ein Job/Trigger-Mapping und kann optional Payload-Transformationen definieren.
- **Konfiguration**: `Croniq:Webhooks:Mode = InMemory|SqlServer`. SqlServer speichert Hooks samt Secrets, RateLimits, Payload-Schema. InMemory bietet minimale Konfiguration für Samples. Admin-API bietet CRUD (`POST /tenants/{id}/webhooks`).
- **Processing Pipeline**: Eingehender Request → Auth/Signature-Check → RateLimiter → Payload Normalizer → Job Dispatch (`TriggerJobAsync`). Fehlschläge landen in einer `WebhookIngressDeadLetter`-Queue; Retry-Policy getrennt von regulären Job-Policies.
- **Security & Governance**: Jeder Hook besitzt Secret + optional IP-Allowlist. Signatur-Header (z.B. `X-Croniq-Signature`) unterstützt HMAC-SHA256. Hooks können dedizierte rate limits (`RequestsPerMinute`, Burst) erhalten, Logging/Observability taggt Events mit `hookKey`.
- **Dokumentation**: `docs/deep-dive/architecture.md` ergänzt um Trigger-Flussdiagramm, `docs/guides/triggers.md` erhält Beispiel (cURL + Payload). Quickstart listet Webhook als zusätzliche Trigger-Option sobald GA.

# Nachbesserungen

- [x] Suche im gesamten Repository nach "OpenConnectionAsync" (Provider-Artefakte ausklammern). Prüfe ob dort custom Prozedur calls mit "CommandText" vorgenommen werden? Ersetze diese durch die bereitgestellten Provider-Abstraktionen.
- [x] `docs\consumer\configuration.md` hier besteht ein Dokumentationsfehler oder gap: builder.Services.AddCroniq() gibt es nicht. Consumer Docs generell auf aktuellsten Stand bringen.
- [x] Ist es korrekt, dass `Croniq.Auth.SqlServer` einen Verweis auf `Croniq.Persistence.SqlServer` hat? Sollte die DbContext-Registrierung nicht eher in `Croniq.Data.SqlServer` stattfinden (bitte verifizieren, Empfehlungen aussprechen)? (Verifiziert: `Croniq.Auth.SqlServer` referenziert nur `Croniq.Data.SqlServer`, alle DbContext-DI-Erweiterungen leben bereits dort; Recommendation: Hosts rufen `AddCroniqSqlServerDbContext` aus `Croniq.Data.SqlServer` auf, bevor sie `AddCroniqAuthSqlServer` verkabeln.)
- [ ] Der `otelBuilder` in `samples\Croniq.Sample.ApiHost` ist optional oder obsolet (da bereits in src\Croniq.Api definiert)?
- [ ] Convenience-Hook bauen, der aus der Konfiguration das passende Provider-Modul zieht; momentan muss man `AddCroniqWebhooksSqlServer` im Startup explizit aufrufen.
- [ ] Sollte man `Croniq.Webhooks` in `Croniq.Hosting.Webhooks` umbenennen - oder gehört das nicht zusammen?
- [ ] `CONTRIBUTING.md` aktualisieren (veraltete Inhalte z.B. `Consumer docs` -> `Croniq docs`)
- [x] Signaturen für Webhooks per Opt-Out deaktivierbar machen (env, config, fluent)?
- [ ] `Quartz.NET` Erwähnungen aus Projekt entfernen
- [ ] Suche nach `- [ ]` und prüfe, was wir noch zu erledigen haben bzw. ob es veraltete Tasks sind.

## Zwischenstand 2025-12-09

- Webhook-Verwaltung: Admin-API vollständig durchgetestet (CRUD, Secret-Rotation, Dead-Letter Replay) via neuem `WebhookApiTestHost` + In-Memory-Doubles; sichert vorherige Hardening-Arbeit.
- Offene Punkte:
	- Docs/Comms: Docstreams-Aufbau (blocked bis Repo public), CONTRIBUTING-Refresh, offene Quickstart/Consumer-Divergenzen.
	- Platform Fit & Naming: Convenience-Hook für Webhooks, evtl. Umbenennung `Croniq.Webhooks` → `Croniq.Hosting.Webhooks`, Klarstellung otelBuilder in Samples.
	- Delivery Backlog: UI-Dokumentation, Kubernetes-Chart-Platzhalter, Entfernen alter `Quartz.NET`-Referenzen, globales Audit der verbleibenden `- [ ]` Items.