# Croniq – Technischer Entwurf

## 1. Zielsetzung & Rahmen

- Bereitstellung eines modularen .NET 10 Scheduling-Ökosystems mit mehreren Bibliotheken und Services.
- Funktionsumfang an Quartz.NET orientieren, jedoch mit leichtgewichtigem In-Memory-Processing und erweiterbarer Provider-Architektur.
- Fokus auf In-Memory-JobStore für schnelle Verarbeitung, optionale Persistenz und Integrationen über Provider.

## 2. Lösungsarchitektur (High-Level)

- **Scheduler Core**: Bibliothek, die Trigger, Jobs, Policies und Execution-Pipeline kapselt.
- **Provider Layer**: Schnittstellen + Default Provider (Xtraq-basiert) für Persistenz, Logging, Telemetrie etc.
- **Service Layer**: Minimal API zum Verwalten/Triggern von Jobs und Schedules; RPC-Kanal (z. B. gRPC oder JSON-RPC) für entfernten Zugriff.
- **Jobs Layer**: Eigenständige Projekte/Assemblies, die Job-Contracts implementieren und per DI registriert werden.
- **Infrastructure**: SQL-Skripte für Xtraq, Docker-Compose für alle Services, optionale UI zur Administration.

## 3. Repository-Struktur (Vorschlag)

```
croniq/
├─ src/
│  ├─ Croniq.Core/                # Scheduler-Engine, Trigger, Policies
│  ├─ Croniq.JobStore.InMemory/   # Referenz-In-Memory-JobStore
│  ├─ Croniq.Persistence.Abstractions/
│  ├─ Croniq.Persistence.Xtraq/   # Default Persistenz-Provider
│  ├─ Croniq.Providers.Logging/   # Logging-Provider (z. B. Serilog)
│  ├─ Croniq.Providers.Telemetry/ # OpenTelemetry-Integration
│  ├─ Croniq.Api/                 # Minimal API + RPC-Endpunkte
│  ├─ Croniq.Rpc.Client/          # RPC-Client-SDK für externe Services
│  ├─ Croniq.Sdk/                 # Contracts für Job-Authoring
│  └─ Croniq.UI/                  # Noch zu entscheidende Technologie
├─ jobs/
│  └─ Sample.Job.Project/
├─ infra/
│  ├─ sql/xtraq/*.sql
│  ├─ docker/
│  │  ├─ docker-compose.yml
│  │  └─ */Dockerfile
│  └─ k8s/ (optional)
├─ tests/
│  ├─ Croniq.Core.Tests/
│  ├─ Croniq.Api.Tests/
│  └─ Integration/
└─ docs/
	 └─ architecture.md
```

## 4. Scheduler Core

- **Trigger & Schedules**: Unterstützung für Cron-Ausdrücke (Quartz-Syntax), Intervalle (fixed/flexible), absolute Zeitpunkte; Validierung & Normalisierung zentral.
- **Execution Pipeline**: Pipeline-Middleware für Policies (Retries, Timeout, Circuit-Breaker, Dead-letter-Queue optional).
- **Job Contracts**: `IJob`-Interface (à la Quartz) mit Cancellation-Support und Kontextobjekt für Logging/Telemetry.
- **Dependency Injection**: Verwendung von `IServiceProvider` zur Auflösung von Jobs; erlaubt externe Assemblys.

## 5. JobStore & Processing

- **In-Memory JobStore** (Default): Thread-sicher, nutzt Channels/Concurrent Collections; persistiert lediglich laufende Trigger im Speicher.
- **Locking & Concurrency**: Nutzung von verteilbaren Locks (optional) via Provider; Standard: lokale Semaphore.
- **Clustering**: Optionales Feature (stretch goal) über Persistenz-Provider.

### Entscheidung 14: Clustering & Verteiltes Scheduling

- **Empfehlung**: Clustering erst aktivieren, wenn der Xtraq-Persistenz-Provider produktionsreif ist. Wir betreiben mehrere Scheduler-Instanzen im Active/Active-Modus, koordinieren Trigger über pessimistische Locks in Xtraq (`sp_Croniq_AcquireTrigger`) plus eine optionale Leader-Election (z. B. `Croniq.Cluster.Leader`, basierend auf verteilten Leases). Jeder Node besitzt eine eindeutige `InstanceId`, Heartbeats werden alle 10 s persisted; ein Health-Monitor räumt verwaiste Leases nach `FailoverGrace` (30 s default) auf. Locking geschieht immer über die Persistenz-Ebene, nicht via in-memory gossip. Cluster-spezifische Einstellungen (Batch-Größe, prefetch, fairness) werden via `ClusterOptions` je Tenant konfigurierbar.
- **Begründung**: Xtraq liefert ACID-Locks und ist bereits die Quelle für Trigger, daher lässt sich Clustering ohne weiteren Distributed Cache umsetzen; Heartbeat + Grace-Period verhindern Doppel-Executes. Active/Active erhöht Durchsatz, ohne eine dedizierte Master-Node vorauszusetzen, dennoch ermöglicht Leader-Election koordinierte Aufgaben (Cleanup, Metrics Aggregation).
- **Konsequenzen**: `Croniq.JobStore.InMemory` bleibt Single-Node; Clusterfähiger Betrieb setzt `Croniq.Persistence.Xtraq` voraus. Neue Tabellen/Prozeduren (Instances, Heartbeats, Leases) müssen in `infra/sql/xtraq` ergänzt werden. Telemetrie und Admin-API müssen Cluster-Health exponieren (`GET /cluster/nodes`). Tests benötigen Integration-Szenarien mit mehreren Scheduler-Prozessen (Testcontainers). Ops-Team muss sicherstellen, dass NTP/clock-drift zwischen Nodes minimal bleibt und Heartbeat-Intervalle mit Infrastruktur abgestimmt sind.

### Entscheidung 2: JobStore-Strategie

- **Empfehlung**: In-Memory-JobStore als Default behalten, aber alle Zugriffe strikt über `IJobStore`/`IJobPersistenceProvider` abstrahieren und früh einen Xtraq-basierten Provider prototypisieren.
- **Begründung**: In-Memory liefert niedrigste Latenz und minimalen Footprint für lokale Entwicklung; durch die Abstraktion können persistente Stores (Xtraq, später evt. Redis/SQL) ohne API-Bruch ergänzt werden, sobald Clusterbetrieb oder Recovery nötig wird.
- **Konsequenzen**: Concurrency- und Locking-Konzepte müssen in der Abstraktion modelliert sein (z. B. `AcquireTrigger`, `ReleaseTrigger`); Integrationstests müssen beide Varianten (In-Memory, Xtraq) abdecken; zusätzliche Dev-Kosten für Provider-Schnittstellen jetzt, weniger Rework später.

## 6. Provider-Modell

- Gemeinsame `IProvider`/`IPlugin`-Abstraktionen mit Registrierung über DI.
- **Persistenz**: `IJobPersistenceProvider` (CRUD für Trigger, Kalender, Job-Metadaten). Default-Implementierung nutzt Xtraq.
- **Logging**: Schnittstelle an `ILogger` anbinden, aber erweiterbarer Provider für zentrale Audit-Logs.
- **Telemetry**: OpenTelemetry-Exporter oder eigener Provider.
- Erweiterbarkeit für weitere Domänen (z. B. Secrets, Notifications).

### Entscheidung 3: Persistenz-Stack

- **Empfehlung**: Ausschließlich custom SQL-Skripte verwenden; sämtliche Datenzugriffe laufen über die Xtraq-Prozedur-Schicht (Stored Procedures) plus klar versionierte Init-/Migrationsskripte. Keine ORMs oder Ad-hoc-SQL aus dem Code.
- **Begründung**: Volle Kontrolle über Schema, Optimierungen und Sicherheitsaspekte; Stored Procedures kapseln Geschäftslogik, erlauben DB-First-Workflow und minimieren Risiken durch dynamische Statements.
- **Konsequenzen**: Dedizierte SQL-Projekte/Ordner (z. B. `infra/sql/xtraq`) müssen gepflegt werden; Code ruft nur definierte Prozeduren über Xtraq auf; Versionierung und Deployment der Skripte sind Teil der CI/CD-Pipeline;

## 7. Xtraq-Persistenz

- Nutzung des nuetzliches/xtraq-Projekts als Basis.
- SQL-Skripte unter `infra/sql/xtraq` für: Tabellen (Jobs, Triggers, Calendars, Executions), Stored Procedures (Upsert, AcquireTrigger), Seed-Daten.
- Optionaler Migrations-Layer (EF Core oder Dapper-Skripte) zur einfachen Bereitstellung.

## 8. Scheduling-Fähigkeiten

- Cron Parser mit Quartz-kompatibler Syntax.
- Intervalle: `FixedInterval`, `SlidingInterval`, `DailyTimeInterval`.
- Kalender-Ausnahmen (Holiday Calendars) optional.
- Zeitliche Präzision: Nutzung von `DateTimeOffset` (UTC-first) plus Zeitzonen-Konverter.
- Schedule-Quelle: Primär persisted im Persistence-Provider (z. B. Xtraq). Ohne Persistenz wird ein In-Memory-Store genutzt; Schedules können optional beim Startup registriert werden (z. B. via `AddCroniqJob` + Seed-Schedules).

### Entscheidung 1: Scheduling-Syntax

- **Empfehlung**: Quartz-Syntax vollständig übernehmen (7 Felder inkl. Sekunden + Sonderzeichen `?`, `L`, `W`, `#`).
- **Begründung**: Deckt Sekundenauflösung, komplexe Regeln (letzter Werktag usw.) ab und ist mit Quartz.NET/Community-Tooling kompatibel; Crontab (5 Felder) würde Mehrfachlogik in Policies erzwingen.
- **Konsequenzen**: Parser über Quartz.NET-Implementierung oder Portierung aufbauen, Validierung + UI-Komponenten müssen Sonderzeichen erklären; Migration zu alternativer Syntax später per Adapter möglich.

## 9. API & RPC

- **Minimal API** (`Croniq.Api`):
  - Endpunkte: `POST /jobs/trigger`, `POST /schedules`, `GET /schedules/{id}`, `DELETE /schedules/{id}`, `GET /health`.
  - AuthN/AuthZ via API Keys oder OAuth2 (extension point).
- **RPC**:
  - gRPC-Service `SchedulerService` mit Methoden `TriggerJob`, `GetSchedules`, `RegisterSchedule`.
  - Alternativ JSON-RPC für leichtere Clients; Client-SDK in `Croniq.Rpc.Client`.

### Entscheidung 4: API-Transport & RPC

- **Empfehlung**: Minimal API (REST + JSON) als offizielle Verwaltungs- und Integrationsschnittstelle beibehalten und gRPC als primären RPC-Kanal etablieren; JSON-RPC nur als optionale Community-Erweiterung.
- **Begründung**: REST/JSON ist für Admin-UIs, DevOps und Skripting am zugänglichsten; gRPC bietet stark typisierte, performante Kommunikation für interne Services/Worker. Zwei offiziell unterstützte Kanäle halten den Aufwand überschaubar.
- **Konsequenzen**: API-Verträge werden via OpenAPI/Swagger versioniert; gRPC erfordert Proto-Spezifikationen und CI-Generierung von Client-SDKs; AuthZ muss für beide Kanäle konsistent sein; JSON-RPC bleibt „best effort“ und wird nicht als Kernprodukt garantiert.

## 10. Jobs in separaten Projekten

- `Croniq.Sdk` liefert NuGet-Package mit Interfaces, DTOs und Annotations.
- Jobs implementieren `IJob` und werden per Assembly-Scanning oder expliziter Registrierung (`services.AddCroniqJob<TJob>()`) eingebunden.
- Packaging-Empfehlung: Jede Domäne eigenes Class Library Projekt; Deployment via NuGet oder direkte Projekt-Referenzen.

### Entscheidung 5: Job-Autorenschaft

- **Empfehlung**: `Croniq.Sdk` als verbindliche Vertragsgrundlage etablieren, Jobs ausschließlich über separate Class Libraries erstellen und per DI registrieren; Assembly-Scanning als Komfort-Feature, aber kein Hidden-Magic.
- **Begründung**: Klare NuGet-Verträge verhindern enge Kopplung an interne Scheduler-Typen; getrennte Projekte erleichtern Versionierung, Testbarkeit und erlauben unterschiedliche Deployment-Strategien (NuGet, Source Reference).
- **Konsequenzen**: SDK muss strikt semantisch versioniert werden; Breaking Changes benötigen Deprecation-Plan; Dokumentation für Job-Autoren (Templates, Samples) erforderlich; Registry/DI-Konfiguration wird Teil der öffentlichen API.

## 11. Policies & Error Handling

- Policy-Engine basierend auf Polly oder eigener Implementierung.
- Konfigurierbare Retry-Strategien (exponential backoff, fixed retry count).
- Error Routing: Failed Jobs -> Dead-letter Queue (In-Memory oder Persistenz), optional Notification Provider.

### Entscheidung 6: Policy-Engine & Fehlerbehandlung

- **Empfehlung**: Auf Polly als Grundlage setzen und eigene Policy-Pipelines drumherum bauen (Retry, Circuit-Breaker, Timeout, Fallback). Dead-Letter-Handling wird als separater Provider implementiert, der Policies Ereignisse liefert.
- **Begründung**: Polly ist battle-tested, integriert sich nativ in .NET und erlaubt deklarative Konfiguration; eigene Implementierung würde mehr Zeit kosten und weniger Community-Support bieten.
- **Konsequenzen**: Policies werden über Konfigurationsobjekte/Options an Jobs gebunden; Telemetrie muss Policy-Events (Retry, Breaker Open, Fallback) erfassen; Dead-Letter-Provider benötigt Persistenzkonzept (z. B. Xtraq-Tabelle) und Tracing, um manuell zu rehydratisieren.

## 12. Docker & Deployment

- Dockerfiles pro Service (`Croniq.Api`, `Croniq.UI`, optionale Worker Nodes).
- Docker-Compose zum lokalen Start: API, UI, Xtraq-Datenbank (z. B. PostgreSQL), Telemetry Stack (Jaeger/Prometheus).
- CI/CD-Pipeline (GitHub Actions) zum Bauen, Testen, Publish der Images und NuGet-Packages.

### Entscheidung 8: Container- & Deployment-Strategie

- **Empfehlung**: Multi-Stage Dockerfiles mit .NET 10 SDK/ASP.NET Runtime verwenden, Images auf Slim/Distroless-Basis für Produktion bauen. Lokales Dev-Setup via `docker-compose` (API, Worker, Xtraq, OTel, Grafana). GitHub Actions erzeugt signierte OCI-Images + Paket-Releases.
- **Begründung**: Multi-Stage reduziert Image-Größe und Angriffssfläche; Compose beschleunigt lokalen Onboarding; GitHub Actions integriert gut mit GitHub Container Registry + Code Signing.
- **Konsequenzen**: Einheitliche `Dockerfile`-Templates je Service; `.devcontainer` optional; CI-Pipeline benötigt Buildx/Cache + Cosign/SBOM; Deployment-Envs (dev/stage/prod) nutzen identische Images, Konfigurationsunterschiede kommen über ENV/Secrets.

## 13. UI-Projekt (Backlog)

- Aktuell nachgelagert; Umsetzung startet erst, wenn API/Provider stabil sind.
- Anforderungen (Schedule-Übersicht, Job-Trigger, Execution-Historie etc.) bleiben bestehen, werden jedoch im Backlog geführt.
- Referenz: Abschnitt 16 „Kubernetes (Backlog)“ für allgemeine Backlog-Vorgehensweise; UI-Technologie wird später entschieden.

## 14. Weiteres & Offene Punkte

- Security Hardenings (Rate Limiting, Secrets Handling, Multi-Tenant-Fähigkeit).
- Observability: Standardisierte Logs, Metriken (Execution Duration, Queue Depth), Traces.
- Testing-Strategie: Unit-Tests für Core, Contract-Tests für Provider, Integrationstests mit Docker Compose.
- Roadmap: Cluster-Fähigkeit, UI-Technologie-Entscheidung, zusätzliche Provider (Cloud Storage, Message Bus).

### Entscheidung 12: Test- & Quality-Strategie

- **Empfehlung**: Drei Teststufen verbindlich etablieren: (1) Unit-Tests mit xUnit + FluentAssertions für Core/Policies/SDK; (2) Contract-Tests gegen Provider über `Croniq.TestKit` (Shared Fixtures, Golden Files) inklusive Testcontainers für Xtraq/Postgres; (3) End-to-End-Integration via Docker Compose Smoke-Suite, die API, Worker und Observability stack hochzieht. Jede PR muss alle Unit- und Contract-Tests bestehen; E2E läuft nightly und vor Release. Zusätzlich erzwingen wir Coverage-Gates (min. 80 % Core, 70 % Gesamt) per Coverlet/ReportGenerator und statische Analyse (dotnet analyzers + SonarQube optional).
- **Begründung**: Die Kombination deckt Logikfehler, Provider-Regressions und Deployability ab; Testcontainers hält Feedback-Zeit niedrig, Compose-E2E spürt Interop-Bugs auf. Coverage-Gates und statische Analyse verhindern Qualitätsabfall bei wachsender Codebasis.
- **Konsequenzen**: Repo benötigt `Croniq.TestKit`-Projekt, gemeinsame Fixtures und Docker-Compose-Testdefinition. GitHub Actions erhält gestufte Jobs (Unit/Contract parallel, E2E separat) mit Pflicht-Gates. Entwickler brauchen lokale Testcontainer-Setup, Doku muss beschreiben, wie Tests gebootstrapped werden. Anforderungen an Hardware/CI (Docker Support) steigen.

### Entscheidung 9: Security-Hardening & Secrets

- **Empfehlung**: Minimal API und gRPC standardmäßig mit API-Key-Auth versehen (Header `X-Croniq-Key`), ergänzt um optionales OAuth2 Client-Credentials für Enterprise-Deployments. Rate Limiting per ASP.NET Core RateLimiter (Sliding Window + Burst) pro API-Key/Tenant erzwingen; gRPC erhält denselben Guard über Interceptors. Secrets (API Keys, Connection Strings) werden ausschließlich über einen `ISecretProvider` bezogen, der in Produktion gegen Vault/KeyVault/Secrets Manager gebunden ist; lokale Entwicklung nutzt `.env` + user secrets.
- **Begründung**: API Keys sind schnell einsatzbereit und passen zu automatisierten Operator-Workloads; OAuth2 stellt Integration mit bestehenden IdPs sicher, ohne alle Nutzer dazu zu zwingen. Zentrales Rate Limiting schützt den Scheduler vor Abuse und ist in .NET 8+ nativ verfügbar. Ein abstrahierter Secret Provider ermöglicht Rotation, auditiertes Lesen und reduziert das Risiko hartkodierter Credentials.
- **Konsequenzen**: `Croniq.Api` benötigt Middleware für Key-Validation, OAuth2 Bearer-Validation und RateLimiter-Policies; gRPC erfordert Interceptors + Metadata-Konvention. Provider/DB-Schema müssen Tenant-/API-Key-Metadaten aufnehmen (z. B. `TenantId`, `Quota`). Deployment-Pipelines müssen sichere Secret-Stores provisionieren; lokale Doku beschreibt, wie Keys erzeugt/rotiert werden. Multi-Tenant-Isolation setzt Namespacing im JobStore (Tenant-Scopes) und Policy-Konfiguration voraus.

### Entscheidung 7: Observability-Stack

- **Empfehlung**: Logging über Serilog (Structured Logging) mit Sink nach OpenTelemetry + optional Seq/ELK; Metriken und Traces durchgängig via OpenTelemetry SDK und OTLP-Exporter. Standard-Dashboarding über Grafana/Tempo/Prometheus im DevOps-Stack.
- **Begründung**: OpenTelemetry bietet einheitliche Instrumentierung für Logs/Metrics/Traces und ist Cloud-/Vendor-neutral; Serilog erleichtert strukturierte Logs im .NET-Ökosystem und spielt gut mit OTLP zusammen.
- **Konsequenzen**: Alle Services benötigen OTel-Instrumentierung (Resource Builder, ActivitySource); CI/CD muss Collectors/Sinks provisionieren; Lokale Dev-Compose-Datei enthält OTel-Collector + Grafana/Tempo; Alerts/Dashboards definieren Kennzahlen (Queue Depth, Execution Duration, Policy Events).

## 15. Reliability & Recovery

- Misfire Handling: Verhalten bei Downtime (nachholen vs. verwerfen), definierte Maximalverzoegerung pro Trigger.
- Startup-Recovery: Persistente Trigger/Jobs nach Neustart laden, verwaiste Locks/Executions bereinigen.
- Zeitquellen: Clock-Drift Monitoring (NTP), Zeitzonen pro Schedule, Sicherstellung von UTC-first in allen Services.
- Data Retention: Aufbewahrung und automatische Bereinigung fuer Execution-Historie, Dead-Letter-Queue, Audit-Logs.
- Backup/Restore: Strategien fuer Xtraq-Datenbank, Konfigurationen und Secrets.

### Entscheidung 10: Reliability & Recovery

- **Empfehlung**: Misfires grundsätzlich nachholen, solange sie innerhalb eines konfigurierbaren `MaxMisfireDelay` (Default 5 Minuten) liegen; Werte lassen sich global, pro Tenant und pro Trigger via `IMisfirePolicy` überschreiben. Policies unterstützen logarithmisch/exponentiell gedrosselte Catch-up-Strategien (z. B. nur jede n-te verpasste Ausführung nachholen), um Fluten zu vermeiden; jenseits der Policy-Grenzen werden Events verworfen und als Dead-Letter markiert. Beim Startup lädt der Scheduler persistente Trigger in Batches, räumt verwaiste Locks über Stored Procedures (`sp_Croniq_CleanupLocks`) und nutzt einen dedizierten Recovery-Worker, bevor neue Ausführungen zugelassen werden. Zeitquelle ist `DateTimeOffset` mit NTP-validierter Systemzeit; kritische Komponenten überwachen Drift via `ITimeProvider`. Execution-, Dead-Letter- und Audit-Daten erhalten standardisierte Retention-Policies (z. B. 30/90/365 Tage) mit Rolling Cleanup. Backups der Xtraq-DB werden nightly per Dump/Snapshot gefahren, inklusive Wiederherstellungs-Playbook.
- **Begründung**: Nachholen innerhalb kurzer Zeitfenster stellt SLA-Verlässlichkeit sicher, ohne nach langen Downtimes Jobs zu fluten. Expliziter Recovery-Worker verhindert Race Conditions beim Rehydrieren. Einheitliche Zeitquelle vermeidet Zeitzonenbugs, Retention hält Datenbank schlank. Dokumentierte Backup-/Restore-Pfade erfüllen Compliance-Anforderungen.
- **Konsequenzen**: `Croniq.Core` benötigt Misfire-Policy pro Trigger und Dead-Letter-Markierungen; Persistence-Skripte brauchen Cleanup-Prozeduren und Retention-Jobs. Startup-Sequenz blockt Scheduling bis Recovery abgeschlossen ist und Telemetrie meldet Status. Ops-Team muss NTP-Monitoring und Backup-Pipeline (inkl. Test-Restore) betreiben; Konfigurationswerte (`MaxMisfireDelay`, Retention) werden als Options exponiert und versioniert.

## 16. Kubernetes (Backlog)

- Aktuell zurueckgestellt; erst nach Kernfunktionen priorisieren, Punkte unten als Spickzettel behalten.

- Deployment-Form: Helm-Chart oder Kustomize-Basis mit Values fuer dev/stage/prod; Secrets/ConfigMaps klar getrennt.
- Probes & Readiness: Liveness/Readiness/Startup-Probes fuer API, Scheduler/Worker; Health-Endpoint festlegen.
- Ressourcen & SLOs: Requests/Limits, HPA (CPU/RAM/Queue-Length), PodDisruptionBudget, Anti-Affinity fuer Datenbank.
- Storage: Persistente Volumes fuer Xtraq-DB, Backup-Jobs, Migrations-Job als initContainer/Job.
- Netzwerk & Security: NetworkPolicies, Ingress/TLS, RBAC/ServiceAccount, Leader-Election falls mehrere Scheduler-Instanzen.

### Entscheidung 13: Kubernetes-Basisstrategie (Backlog)

- **Empfehlung**: Wenn Kubernetes priorisiert wird, liefern wir ein einziges Helm-Chart (`charts/croniq`) mit Values-Overlays fuer dev/stage/prod; die Compose-Umgebung bleibt maßgeblich fuer lokale Entwicklung. Das Chart provisioniert Deployment + HPA fuer API/Worker, StatefulSet fuer Xtraq samt PVC-Templates und `CronJob`/Job fuer Migrationen. Secrets werden über ExternalSecrets (Vault/KeyVault) eingebunden, ConfigMaps erhalten nur nicht-sensitive Defaults. Readiness/Liveness-Probes spiegeln die Minimal-API-/gRPC-Healthchecks wider, Autoscaling basiert auf CPU und Queue-Depth-Metriken. Zusätzliche Komponenten (Ingress-Controller, Service Mesh, UI) bleiben optional/backlog und werden erst aktiviert, wenn die jeweiligen Streams starten.
- **Begründung**: Ein zentrales Chart reduziert Drift zwischen Stages, lässt sich aber per Values flexibel anpassen; Compose bleibt der schnellste Dev-Pfad. StatefulSet + PVC garantiert Datenpersistenz für Xtraq, Migration-Jobs verhindern Race Conditions beim Rollout. ExternalSecrets und Health-Probes adressieren Security/Availability ohne übermäßigen Tooling-Aufwand.
- **Konsequenzen**: Repo benötigt `infra/k8s/charts/croniq` inkl. README und Standard-Values; CI/CD muss Helm-Pakete linten/testen (z. B. `helm template` + `kubeconform`). Observability-Stack (OTel/Grafana) wird als optionales Subchart referenziert. UI bleibt weiterhin im Backlog—Chart enthält nur Platzhalter-Werte, bis Technologie gewählt ist. Team braucht Basis-Kubernetes-Richtlinien (Namespaces, RBAC), die parallel dokumentiert werden.

## 17. Release & Compliance

- Versionierung: SemVer fuer Libraries/SDKs, API-Versionierung (z. B. `/v1`) und Deprecation-Policy.
- Migrationen: Forward-/Backward-kompatible DB-Migrationen, Rollback-Plan und Smoke-Tests nach Deployment.
- Supply Chain Security: SBOM-Erzeugung, Signierung von Images/Packages, Vulnerability-Scanning in CI/CD.
- Lizenzen: Third-Party-Notice, Lizenzpruefungen der Abhaengigkeiten, Richtlinie fuer neue Dependencies.

### Entscheidung 11: Release-, Compliance- & Supply-Chain-Strategie

- **Empfehlung**: Alle Libraries/SDKs strikt nach SemVer veröffentlichen; Services exponieren `/v1`-Routen, Breaking Changes nur über neue Major-Versionen + 2 Release-Zyklen Deprecation. Datenbankmigrationen laufen via versionierten Skripten (Xtraq) mit Forward+Rollback-Skripten; Releases enthalten automatische Smoke-Tests nach Deployment. GitHub Actions erzeugt für jedes Artefakt (NuGet, OCI) eine attestierte SBOM (Syft) und signiert Images/Packages via Cosign/SignPath. Dependency-Updates laufen durch wöchentliche Renovate-PRs mit Lizenz-Check (OSS Review Toolkit) und verpflichtendem Vulnerability-Scan (Trivy/Snyk) vor Merge.
- **Begründung**: Strikte Versionierung und Deprecation-Regeln geben Konsumenten Planbarkeit; DB-Hardening mit Rollbacks reduziert Downtime-Risiken. SBOM + Signaturen beantworten Supply-Chain-Compliance-Anforderungen und vereinfachen Incident Response. Automatisierte Scans halten Dependencies aktuell und rechtssicher.
- **Konsequenzen**: Release-Templates in GitHub Actions müssen Version-Bumping, Changelog-Generierung und SBOM/Signatur-Schritte enthalten; QA betreibt automatische Smoke-Tests (Compose/K8s). Xtraq-Skripte benötigen Rollback-Pendants und werden im Repo versioniert. Compliance-Dokumentation (Third-Party-Notice, Policy für neue Dependencies) wird Teil des Release-Prozesses; fehlende Scans blockieren Merge/Release.

## 18. Multi-Tenancy & Quotas

- Tenant-Namespace fuer Schedules/Jobs/Policies im JobStore; API-Key/Tenant-Zuordnung erzwingt Isolation in Persistenz und Telemetrie.
- Quotas pro Tenant (z. B. max Trigger/Minute, parallele Executions, Payload-Groesse) abgestimmt auf RateLimiter-Policy.
- Namespacing in Provider: Persistence-, Dead-Letter- und Telemetry-Daten immer mit TenantId speichern; Admin-APIs brauchen Scope-Grenzen.
- Optional: Dedicated Connection Strings pro Tenant (Premium) vs. Shared DB; Migrations/Backups muessen beide Topologien abdecken.

## 19. Execution Semantics & Idempotenz

- Delivery-Garantie: Standard at-least-once; Deduplikation via `ExecutionId` + optionale `IdempotencyKey` vom Caller. At-most-once nur fuer spezifische Provider/Policies.
- Parallelitaet: Concurrency-Grenzen pro Job/Schedule (SingleFlight pro JobKey default), optional konfigurierbar; Queue-Prioritaeten per Policy.
- Timeouts & Cancellation: Jobs erhalten CancellationToken; Timeout-Policies brechen Lauf ab, markieren Ergebnis und verschieben in Dead-Letter gemaess Policy.
- Side-Effects: Empfehlung Outbox/Inbox fuer externe Events; Job-Kontrakte sollen idempotent sein oder Deduplikation ueber Provider anbieten.
