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

## 6. Provider-Modell

- Gemeinsame `IProvider`/`IPlugin`-Abstraktionen mit Registrierung über DI.
- **Persistenz**: `IJobPersistenceProvider` (CRUD für Trigger, Kalender, Job-Metadaten). Default-Implementierung nutzt Xtraq.
- **Logging**: Schnittstelle an `ILogger` anbinden, aber erweiterbarer Provider für zentrale Audit-Logs.
- **Telemetry**: OpenTelemetry-Exporter oder eigener Provider.
- Erweiterbarkeit für weitere Domänen (z. B. Secrets, Notifications).

## 7. Xtraq-Persistenz

- Nutzung des nuetzliches/xtraq-Projekts als Basis.
- SQL-Skripte unter `infra/sql/xtraq` für: Tabellen (Jobs, Triggers, Calendars, Executions), Stored Procedures (Upsert, AcquireTrigger), Seed-Daten.
- Optionaler Migrations-Layer (EF Core oder Dapper-Skripte) zur einfachen Bereitstellung.

## 8. Scheduling-Fähigkeiten

- Cron Parser mit Quartz-kompatibler Syntax.
- Intervalle: `FixedInterval`, `SlidingInterval`, `DailyTimeInterval`.
- Kalender-Ausnahmen (Holiday Calendars) optional.
- Zeitliche Präzision: Nutzung von `DateTimeOffset` (UTC-first) plus Zeitzonen-Konverter.

## 9. API & RPC

- **Minimal API** (`Croniq.Api`):
  - Endpunkte: `POST /jobs/trigger`, `POST /schedules`, `GET /schedules/{id}`, `DELETE /schedules/{id}`, `GET /health`.
  - AuthN/AuthZ via API Keys oder OAuth2 (extension point).
- **RPC**:
  - gRPC-Service `SchedulerService` mit Methoden `TriggerJob`, `GetSchedules`, `RegisterSchedule`.
  - Alternativ JSON-RPC für leichtere Clients; Client-SDK in `Croniq.Rpc.Client`.

## 10. Jobs in separaten Projekten

- `Croniq.Sdk` liefert NuGet-Package mit Interfaces, DTOs und Annotations.
- Jobs implementieren `IJob` und werden per Assembly-Scanning oder expliziter Registrierung (`services.AddCroniqJob<TJob>()`) eingebunden.
- Packaging-Empfehlung: Jede Domäne eigenes Class Library Projekt; Deployment via NuGet oder direkte Projekt-Referenzen.

## 11. Policies & Error Handling

- Policy-Engine basierend auf Polly oder eigener Implementierung.
- Konfigurierbare Retry-Strategien (exponential backoff, fixed retry count).
- Error Routing: Failed Jobs -> Dead-letter Queue (In-Memory oder Persistenz), optional Notification Provider.

## 12. Docker & Deployment

- Dockerfiles pro Service (`Croniq.Api`, `Croniq.UI`, optionale Worker Nodes).
- Docker-Compose zum lokalen Start: API, UI, Xtraq-Datenbank (z. B. PostgreSQL), Telemetry Stack (Jaeger/Prometheus).
- CI/CD-Pipeline (GitHub Actions) zum Bauen, Testen, Publish der Images und NuGet-Packages.

## 13. UI-Projekt

- Technologie offen (Blazor, React + ASP.NET, Vue etc.).
- Anforderungen: Schedule-Übersicht, Job-Trigger, Execution-Historie, Provider-Status, Policy-Konfiguration.
- Sollte API/RPC konsumieren und ggf. SignalR für Echtzeit-Updates verwenden.

## 14. Weiteres & Offene Punkte

- Security Hardenings (Rate Limiting, Secrets Handling, Multi-Tenant-Fähigkeit).
- Observability: Standardisierte Logs, Metriken (Execution Duration, Queue Depth), Traces.
- Testing-Strategie: Unit-Tests für Core, Contract-Tests für Provider, Integrationstests mit Docker Compose.
- Roadmap: Cluster-Fähigkeit, UI-Technologie-Entscheidung, zusätzliche Provider (Cloud Storage, Message Bus).
