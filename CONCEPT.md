# Croniq - Technischer Entwurf

## 1. Zielsetzung & Rahmen

- Bereitstellung eines modularen .NET 10 Scheduling-Oekosystems mit mehreren Bibliotheken und Services.
- Funktionsumfang an Quartz.NET orientieren, jedoch mit leichtgewichtigem In-Memory-Processing und erweiterbarer Provider-Architektur.
- Fokus auf In-Memory-JobStore fuer schnelle Verarbeitung, optionale Persistenz und Integrationen ueber Provider.
- Nichtfunktionale Zielwerte (Defaults, best practice):
  - Trigger-Lookup und Schedule-Evaluation: <100 ms p50, <250 ms p95 bei bis zu 10k aktiven Triggern pro Node.
  - End-to-end Job-Start (Trigger bis ExecuteAsync aufgerufen): <500 ms p95 ohne Persistenz; <750 ms p95 mit SqlServer-Provider.
  - Verfuegbarkeit: 99.9% Monats-SLO fuer Scheduler/API; gRPC/API-Fehlerquote <0.1% pro Tag; Clock-Drift <50 ms zwischen Nodes.

## 2. Loesungsarchitektur (High-Level)

- **Scheduler Core**: Bibliothek, die Trigger, Jobs, Policies und Execution-Pipeline kapselt.
- **Provider Layer**: Schnittstellen + Default Provider (SqlServer-basiert) fuer Persistenz, Logging, Telemetrie etc.
- **Service Layer**: Minimal API zum Verwalten/Triggern von Jobs und Schedules; RPC-Kanal (z.B. gRPC oder JSON-RPC) fuer entfernten Zugriff.
- **Jobs Layer**: Eigenstaendige Projekte/Assemblies, die Job-Contracts implementieren und per DI registriert werden.
- **Infrastructure**: EF-Core-Migrationen fuer den SqlServer-Provider, Docker-Compose fuer alle Services, optionale UI zur Administration.
- **Hosting Extensions**: `Croniq.Api` bietet `AddCroniqApiServices`/`AddCroniqApiRateLimiter`/`UseCroniqApi` fuer Konsumenten. Auth/Persistenz sind per Konfig schaltbar (InMemory vs. SqlServer) mit einem gemeinsamen ConnectionString unter `Croniq:SqlServer`.

## 3. Repository-Struktur (Vorschlag)

```
croniq/
|-- src/
|   |-- Croniq.Core/                # Scheduler-Engine, Trigger, Policies
|   |-- Croniq.JobStore.InMemory/   # Referenz-In-Memory-JobStore
|   |-- Croniq.Persistence.Abstractions/
|   |-- Croniq.Persistence.SqlServer/   # Default Persistenz-Provider
|   |-- Croniq.Auth.Abstractions/   # Contracts fuer Tenants/Users/API-Clients/CallerContext
|   |-- Croniq.Auth.Core/           # In-Memory/Auth-Services + Middleware (dev/edge)
|   |-- Croniq.Data.SqlServer/      # Gemeinsamer DbContext + EF-Core-Entities fuer Croniq
|   |-- Croniq.Auth.SqlServer/      # SqlServer-basierter Auth-Provider (SaaS, shared DB)
|   |-- Croniq.Providers.Logging/   # Logging-Provider (z.B. Serilog)
|   |-- Croniq.Providers.Telemetry/ # OpenTelemetry-Integration
|   |-- Croniq.Api/                 # Minimal API + RPC-Endpunkte
|   |-- Croniq.Rpc.Client/          # RPC-Client-SDK fuer externe Services
|   |-- Croniq.Sdk/                 # Contracts fuer Job-Authoring
|   `-- Croniq.UI/                  # Noch zu entscheidende Technologie
|-- jobs/
|   `-- Sample.Job.Project/
|-- infra/
|   `-- docker/
|       |-- docker-compose.yml
|       `-- */Dockerfile
|-- tools/
|   `-- Croniq.DbMigrator/
|-- tests/
|   |-- Croniq.Core.Tests/
|   |-- Croniq.Api.Tests/
|   `-- Integration/
`-- docs/
    `-- architecture.md
```

### Dokumentationsstraenge

- **Consumer Docs** (`docs/consumer/*`): Quickstarts, SDK-Guides, Samples und How-Tos fuer Teams, die Croniq nutzen oder Jobs implementieren. Enthalten Schritt-fuer-Schritt-Anleitungen (z.B. "Ersten Job schreiben", "API-Key erzeugen") und fokussieren auf Developer Experience.
- **Technische Docs** (`docs/technical/*`): Architektur- und Provider-Dokumentation, Datenbank-Schemata, Deployment-Handbuecher sowie Operations-Runbooks. Diese Straenge adressieren Maintainer:innen, Platform-Teams und Contributors.
- Beide Straenge verlinken gegenseitig auf relevante Referenzen (z.B. Consumer Doc verweist auf tiefergehende Architekturpassagen), werden aber separat versioniert und in der CI validiert (Broken-Link-Checks, Samples-Builds). Release-Notes referenzieren beide Perspektiven explizit.

### Diagramme & Nachvollziehbarkeit

- Architektur- und Policy-Diagramme liegen versioniert in `docs/architecture.drawio` (diagrams.net/draw.io XML). Aktuelle Seiten: _Architecture_ (Komponenten/Provider) und _PolicyResolver_ (Default + Overrides → Resolver → Worker).
- Bearbeitung/Ansicht lokal: VS Code mit der Extension **hediet.vscode-drawio** (unofficial, de-facto Standard) oder externe diagrams.net Desktop/Web-App. Datei direkt aus dem Repo oeffnen; keine Remote-Speicherung notwendig.
- Beim Editieren bitte Seitenstruktur beibehalten, keine eingebetteten externen Ressourcen nutzen; Farben/Legende konsistent halten.

## Persistenz-Basis (SqlServer)

- Croniq persistiert ausschliesslich via EF Core (SqlServer Provider). `src/Croniq.Data.SqlServer` enthaelt den gemeinsamen `SqlServerDbContext`, Entity-Mappings sowie `SqlServerOptions`. Schema-Name ist `croniq`; alle Entities erzwingen `TenantId`, `EnvironmentTag` und RowMetadata (Created/Updated/Concurrency).
- `Croniq.Persistence.SqlServer` implementiert den `IJobPersistenceProvider` und kapselt Migrations-/DbContextFactory-Setup fuer Scheduler, API und Worker Hosts. `Croniq.Auth.SqlServer` teilt dasselbe DbContext-Modell fuer ApiClients/ApiKeys.
- Connection Strings und EF-Optionen liegen zentral unter `Croniq:SqlServer:*` (z.B. `ConnectionString`, `EnableDetailedErrors`, `EnableSensitiveDataLogging`). Samples/CI nutzen `Croniq:SqlServer:ConnectionString` mit einem lokalen SQL Server 2022 Container (`infra/docker/docker-compose.yml`).
- Migrations & Seeding laufen ueber `tools/Croniq.DbMigrator` (CLI). Tests verwenden denselben Codepfad via Testcontainers.

### SQL-Schema Layout (Entwurf)

```
src/Croniq.Data.SqlServer/
  Entities/
    ApiClientEntity.cs      # Mandanten-spezifische API-Clients (TenantId, ClientId, DefaultScopes, EnvTag)
    ApiKeyEntity.cs         # Gehashte Secrets + Ablauf + Aktiv-Flag
    DeadLetterEntity.cs     # Fehlerhafte Ausfuehrungen inkl. Payload/Reason
    JobEntity.cs            # Definitionen inkl. JobKey, Namespace, Metadata
    TriggerEntity.cs        # Trigger inkl. NextFireAtUtc, RowVersion
  SqlServerDbContext.cs     # Konsolidierter DbContext fuer Persistence + Auth
  SqlServerOptions.cs       # Gemeinsame Konfiguration (Connection, Logging)
src/Croniq.Persistence.SqlServer/
  SqlServerJobPersistenceProvider.cs
  ServiceCollectionExtensions.cs
src/Croniq.Auth.SqlServer/
  SqlServerApiKeyStore.cs
  ServiceCollectionExtensions.cs
tools/Croniq.DbMigrator/
  Program.cs                # CLI zum Ausfuehren von EF-Core-Migrationen
```

- Multi-Tenancy wird ausschliesslich ueber das Schema `croniq` modelliert: Tabellen verwenden Identity-Keys (`Id` BIGINT) fuer interne Relationen, waehrend `TenantId` und `EnvironmentTag` als Pflichtfelder dienen und gemeinsam partitionieren; Indizes decken sowohl Partition als auch Lookup-Schluessel (z.B. `JobKey`, `TriggerKey`).
- API-Keys werden gehasht gespeichert (KeyId+Prefix im Klartext, Secret nie persisted). Rotation erfolgt ueber Application Services; keine Stored Procedures erforderlich.

## 3a. Auth & Identity Modell (SaaS)

- **Ziele**: Multi-Tenant SaaS mit isolierten Jobs/Policies pro Tenant, verwaltbare Nutzer (OIDC-first), API-Clients/Keys fuer Automatisierung, zentrale Quotas/RateLimits pro Tenant/API-Key. Eine gemeinsame SQL Server Datenbank fuer Persistenz + Auth.
- **Domain-Objekte**:
  - Tenant: Basisobjekt, enthaelt Reference/Name/Plan/CreatedBy. Jede Croniq-Entity referenziert TenantId (FK).
  - User: Entweder federated (OIDC Subject + Issuer) oder optional lokal (nur fuer dev). Rollen pro Tenant (`TenantAdmin`, `SchedulerAdmin`, `Reader`). Tabelle `auth.Users`, Join `auth.UserRoles`.
  - ApiClient/ApiKey: Maschinenidentitaet mit `KeyId` (prefix), `KeyHash`, optional `EnvTag`/`Scopes` (z.B. `schedules:write`, `jobs:trigger`). Tabelle `auth.ApiClients`, `auth.ApiKeys`.
  - CallerContext: HTTP/gRPC Middleware erzeugt einen `ICallerContext` (TenantId, EnvironmentTag, CallerType=User|ApiKey, Scopes, RateLimitKey) aus Header `X-Croniq-Key` oder Bearer Token (OIDC Access Token).
- **AuthN/AutZ**:
  - Default: API-Key Flow (Header `X-Croniq-Key`) fuer Maschinen; OIDC Access Token (Bearer) fuer User. Beide Pfade mappen auf `ICallerContext`.
  - AuthZ: Scope-basiert (Claim `scope` oder ApiKey-Scopes) und Tenant-Enforcement (TenantId wird aus ApiKey oder Token-Claim gelesen und in `PartitionScope` injiziert). Optional EnvTag-Restriktion pro ApiKey.
  - Rate Limiting: ASP.NET RateLimiter nutzt `RateLimitKey = TenantId + ':' + ApiKeyId` (oder UserId) statt global. Konfig in `Croniq:Api:RequestsPerMinute` + Tenant-Overrides.
- **Services/Abstraktionen**:
  - `Croniq.Auth.Abstractions`: `ITenantStore`, `IUserStore`, `IApiKeyStore`, `ICallerContextAccessor`, `ICallerContextFactory`, Contracts fuer Scopes/Claims/ApiClientMetadata.
  - `Croniq.Auth.Core`: In-Memory Implementation fuer lokale Dev/Tests, Middleware fuer API-Key + JWT Bearer Validation (ohne konkrete IdP-Details).
  - `Croniq.Auth.SqlServer`: EF-Core-basierte Implementierung der Stores auf Basis des gemeinsamen `SqlServerDbContext` inkl. Hash-Storage fuer Secrets.
- **DB-Konzept & Shared DbContext**:
  - `Croniq.Data.SqlServer` enthaelt den EF-Core-`SqlServerDbContext` inkl. Entities fuer Jobs, Trigger, DeadLetters sowie Auth (`ApiClients`, `ApiKeys`). Keine Domain-Logik, nur Mappings + Options.
  - `Croniq.Persistence.SqlServer` und `Croniq.Auth.SqlServer` referenzieren `Croniq.Data.SqlServer`, nutzen ein gemeinsames DbContextFactory-Setup und teilen ConnectionStrings/Migrations.
  - Auth-Datenmodell: `[croniq].[ApiClients]` verwaltet Mandanten-Clients (TenantId, EnvTag, DefaultScopes), `[croniq].[ApiKeys]` speichert gehashte Secrets + Scopes + Ablaufdaten.
- **API-Erweiterungen**:
  - Admin-Routen (geschuetzt, Scope `tenants:admin`): `POST /tenants`, `POST /tenants/{id}/api-keys`, `POST /tenants/{id}/api-keys/{keyId}/rotate`, `DELETE /tenants/{id}/api-keys/{keyId}`, `GET /tenants/{id}/users`.
  - Developer-Routen: `GET /me` (aus CallerContext), optional `GET /quota` (aktuelle Limits).
  - Abwarten bis UI-Backlog startet, dann Wiederverwendung derselben Auth-Backends.
- **Security**:
  - ApiKey Speicherung gehasht (HMAC/SHA-256 + per-key salt), Ausgabe nur einmal beim Issue/Rotate.
  - Key Prefix (z.B. `crq_dev_`) + zufaellige 32 Bytes; zusammengesetzt als `crq_dev_<keyId>_<secret>`, Secret nie gespeichert. Validation: Prefix -> DB lookup -> Hashvergleich.
  - Audit: `auth.AuditLog` optional fuer Issuance/Revocation/FailedAuth.
- **Migration & Kompatibilitaet**:
  - V1 minimal: kein globaler ApiKey mehr; Auth-Modus ist konfigurierbar (InMemory|SqlServer). InMemory benoetigt `Croniq:Auth:InMemory:ApiKey`, SqlServer nutzt EF Core (`SqlServerApiKeyStore`).
  - In-Memory Auth bleibt fuer Samples/Unit-Tests. SqlServer-Auth laeuft gegen denselben SQL Server wie die Persistenz (oder optional eigene Connection) und nutzt dieselben Testcontainer.
- **Konfiguration**:
  - Auth: `Croniq:Auth:Mode = SqlServer|InMemory`; SqlServer nutzt per Default `Croniq:SqlServer:ConnectionString` (optional Override `Croniq:Auth:SqlServer:*`), InMemory nutzt `Croniq:Auth:InMemory:ApiKey`.
  - Persistence: `Croniq:Persistence:Mode = SqlServer|InMemory`; SqlServer verwendet denselben Connection String (oder `Croniq:Persistence:SqlServer:*` Override). Runtime-JobStore bleibt InMemory; der SqlServer-Provider liefert Recovery/Sync/Leases.

### Rollout-Checklist (Auth + SqlServer)

- [ ] EF-Core-Migrationen fuer Auth/Persistenz-Schema in `Croniq.Data.SqlServer` pflegen (Jobs, Trigger, ApiClients, ApiKeys) + Seeds fuer Dev/Testcontainer.
- [ ] `Croniq.Persistence.SqlServer` finalisieren: Provider, Health Checks, Optionen fuer Dead-Letter/Lease.
- [ ] `Croniq.Auth.Abstractions` + `Croniq.Auth.Core` pflegen (CallerContext, InMemory), `Croniq.Auth.SqlServer` mit hashed Secret Storage + Rotation.
- [ ] `Croniq.Api` erweitern: CallerContext-Middleware (ApiKey + JWT/OIDC), Tenant/Env-Enforcement, Admin-Routen fuer Tenant/ApiKey Management; RateLimiter Key = TenantId:CallerId.
- [ ] Tests/Docs: Contract-Tests fuer SQL-Auth-Store (Testcontainers SQL), Docs ergaenzen (Key-Issuance, Rotation, OIDC Setup).

## 4. Scheduler Core

- **Trigger & Schedules**: Unterstuetzung fuer Cron-Ausdruecke (Quartz-Syntax), Intervalle (fixed/flexible), absolute Zeitpunkte; Validierung & Normalisierung zentral.
- **Execution Pipeline**: Pipeline-Middleware fuer Policies (Retries, Timeout, Circuit-Breaker, Dead-letter-Queue optional).
- **Job Contracts**: Offentliches `IJob`-Interface (aehnlich Quartz); erhaelt Cancellation-Support, ein Execution-Context-Objekt fuer Logging/Telemetry sowie optionale Attribute (z.B. `[CroniqJob("billing", "InvoiceDispatch")]`) zur Beschreibung der Job-Metadaten.
- **Job Keys & Partitionierung**: Jeder Job erzeugt deterministisch einen `JobKey` nach dem Schema `TenantId:EnvironmentTag:Namespace:JobName` (optional ergaenzt um `:Variant`). Der Scheduler registriert bzw. aktualisiert Job-Definitionen beim Startup ueber den `IJobPersistenceProvider`; die Persistenz erzwingt `UNIQUE (TenantId, EnvironmentTag, Namespace, JobName)` und nutzt dieselben Spalten als Partition Keys, sodass Dev-Instanzen ihre eigenen Datensaetze verwalten koennen, waehrend Shared-Cluster-Szenarien gezielt ueber identische Tags laufen.
- **Dependency Injection**: Verwendung von `IServiceProvider` zur Aufloesung von Jobs; erlaubt externe Assemblys.

### API Sketch: `IJob` Contract

```csharp
namespace Croniq.Sdk;

[AttributeUsage(AttributeTargets.Class, Inherited = false, AllowMultiple = false)]
public sealed class CroniqJobAttribute : Attribute
{
  public CroniqJobAttribute(string namespaceSegment, string jobName, string? variant = null)
  {
    NamespaceSegment = namespaceSegment;
    JobName = jobName;
    Variant = variant;
  }

  public string NamespaceSegment { get; }
  public string JobName { get; }
  public string? Variant { get; }
}

public interface IJob
{
  Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken);
}

public interface IJobExecutionContext
{
  string JobKey { get; }
  IReadOnlyDictionary<string, string> Metadata { get; }
  ILogger Logger { get; }
  ActivitySource ActivitySource { get; }
}
```

Der Scheduler liest das `CroniqJobAttribute`, bildet daraus den `JobKey` und registriert den Typ beim Startup. Consumer-Dokumentation zeigt Beispiele mit vereinfachtem `IJob`, waehrend die technischen Docs tiefer auf `IJobExecutionContext`, Policies und Provider-Hooks eingehen.

**Handler UX Leitplanken**

- Jobs sollen in der Regel nur einen `Handle`-Delegate verwenden; zusaetzliche Handler-Formen werden aktuell nicht dokumentiert.
- Fortschritt wird ueber `IJobExecutionContext.InitProgress(total)` (einmal pro Ausfuehrungspfad) und `ReportProgress(processed)` kommuniziert. Diese Informationen fliessen in Telemetrie, UI und optionale Resume-Strategien.
- Semantische Statusmeldungen nutzen `CustomState(string detail, JobState state = JobState.Waiting)`. Der Croniq-Core-State (`JobState`) bleibt Pflicht, `detail` beschreibt domaenenspezifische Unterteilungen (z.B. `waiting-on-dependency`, `step-1`).
- Fehler werden im Handler geloggt und erneut geworfen; Policies (Retry, Dead-Letter) reagieren auf die Exception. `CustomState(..., JobState.Error)` ist optional fuer zusaetzliche Sichtbarkeit.
- Das Quickstart-Beispiel unter `docs/consumer/quickstart.md` fungiert als Referenz fuer die drei empfohlenen Handler-Patterns (minimal, Progress, CustomState).

## 5. JobStore & Processing

- **In-Memory JobStore** (Default): Thread-sicher, nutzt Channels/Concurrent Collections; persistiert lediglich laufende Trigger im Speicher.
- **Locking & Concurrency**: Nutzung von verteilbaren Locks (optional) via Provider; Standard: lokale Semaphore.
- **Clustering**: Optionales Feature (stretch goal) ueber Persistenz-Provider.

### Entscheidung 14: Clustering & Verteiltes Scheduling

- **Empfehlung**: Clustering erst aktivieren, wenn der SqlServer-Persistenz-Provider einen stabilen Lease-/Locking-Satz besitzt. Mehrere Scheduler-Instanzen laufen Active/Active, koordinieren Trigger ueber dedizierte Tabellen (`croniq.TriggerLeases`, `croniq.WorkerInstances`) und nutzen RowVersion/`FOR UPDATE`-Semantik statt Stored Procedures. Eine optionale Leader-Election (z.B. `Croniq.Cluster.Leader`) basiert auf den gleichen Tabellen.
- **Begruendung**: SQL Server bietet ACID-Transaktionen und Zeilenlocks, womit wir ohne zusaetzliche Distributed-Cache-Systeme auskommen. Heartbeats + Grace-Period verhindern Doppel-Executions; dieselben Tabellen liefern Telemetrie fuer Admin-UIs.
- **Konsequenzen**: `Croniq.JobStore.InMemory` bleibt Single-Node; clusterfaehiger Betrieb setzt `Croniq.Persistence.SqlServer` voraus. Neue Tabellen/Entity-Mappings fuer Instances, Heartbeats und Leases leben in `Croniq.Data.SqlServer` und werden ueber EF-Migrationen ausgeliefert. Telemetrie/API exponiert Cluster-Health (`GET /cluster/nodes`). Tests muessen Mehrprozess-Szenarien mit SqlServer-Testcontainers fahren; Ops stellt NTP/Clock-Sync sicher.

### Entscheidung 2: JobStore-Strategie

- **Empfehlung**: In-Memory-JobStore als Default behalten, aber alle Zugriffe strikt ueber `IJobStore`/`IJobPersistenceProvider` abstrahieren. `Croniq.Persistence.SqlServer` ist der erste produktive Provider; weitere Stores (z.B. Postgres, Cosmos) muessen sich an dieselben Contracts halten.
- **Begruendung**: In-Memory liefert niedrigste Latenz fuer Dev/Test, waehrend SqlServer Durability, Recovery und Clustering bereitstellt. Die Abstraktion ermoeglicht spaetere Provider ohne API-Bruch.
- **Konsequenzen**: Interfaces modellieren Locking (`AcquireTriggerAsync`, `ReleaseTriggerAsync`), Dead-Letter, Lease- und Recovery-Operationen. Integrationstests muessen InMemory + SqlServer (Testcontainers) abdecken, damit Provider-Gleichheit gewaehrleistet bleibt.

## 6. Provider-Modell

- Gemeinsame `IProvider`/`IPlugin`-Abstraktionen mit Registrierung ueber DI.
- **Persistenz**: `IJobPersistenceProvider` (CRUD fuer Trigger, Kalender, Job-Metadaten). Default-Implementierung ist `Croniq.Persistence.SqlServer`.
- **Konfig-Modi**: Per Host konfigurierbar zwischen InMemory und SqlServer (Auth getrennt von Persistence). Gemeinsamer ConnectionString unter `Croniq:SqlServer:ConnectionString` kann fuer beide Domänen genutzt werden; domänenspezifische Overrides moeglich (`Croniq:Auth:SqlServer`, `Croniq:Persistence:SqlServer`).
- **Logging**: Schnittstelle an `ILogger` anbinden, aber erweiterbarer Provider fuer zentrale Audit-Logs.
- **Telemetry**: OpenTelemetry-Exporter oder eigener Provider.
- Erweiterbarkeit fuer weitere Domaenen (z.B. Secrets, Notifications).

### Entscheidung 3: Persistenz-Stack

- **Empfehlung**: EF Core als einzige Abstraktion verwenden. `Croniq.Data.SqlServer` definiert Entities/DbContext, Migrationen werden versioniert im Code und ueber `tools/Croniq.DbMigrator` ausgerollt. Keine Generatoren/Stored-Procedure-Schichten.
- **Begruendung**: Gemeinsames Modell fuer Auth + Persistence reduziert Drift, erleichtert Unit-/Integrationstests (Testcontainers) und erlaubt uns, Schema-Aenderungen via C#-Migrations zu reviewen. EF Core deckt Concurrency, RowVersioning und Default-Werte bereits ab.
- **Konsequenzen**: Der fruehere SQL-Skript-Ordner entfaellt; Schema-Aenderungen entstehen durch `dotnet ef migrations add`. CI muss Migrationsdrift testen (`Croniq.DbMigrator --apply --connection <ci-connection>`). Provider muessen auf `SqlServerDbContext` basieren.

## 7. SqlServer-Persistenz

- Default-Persistenz ist `Croniq.Persistence.SqlServer`. Der Provider registriert den gemeinsamen `SqlServerDbContext`, wendet Migrations an und implementiert `IJobPersistenceProvider` (Jobs, Trigger, DeadLetters) inkl. Dead-Letter- und Lease-APIs.
- Auth- und Persistence-Stores teilen sich dieselbe Datenbank. `Croniq.Auth.SqlServer` implementiert `IApiKeyStore` auf Basis der Tabellen `croniq.ApiClients` und `croniq.ApiKeys`; Secrets werden nur gehasht abgelegt.
- Lokale Entwicklung: `infra/docker/docker-compose.yml` startet SQL Server 2022 + Admin UI. Connection String steht in `Croniq:SqlServer:ConnectionString`. Samples/Worker/API nutzen denselben Key.
- CI/CD Workflow:
  1. Schema-Aenderung durch `dotnet ef migrations add <Name> --project src/Croniq.Data.SqlServer --output-dir Migrations` erstellen.
  2. `dotnet run --project tools/Croniq.DbMigrator -- --connection "${CRONIQ_SQL_CONNECTION}" --apply` fuehrt Migrationen aus.
  3. Integrationstests starten einen SQL Server Testcontainer und rufen `Croniq.DbMigrator` vor den Tests auf.
- Backups/Drift: `Croniq.DbMigrator --verify` prueft, ob lokale Migrations dem DB-Status entsprechen. Kein separates Skript-Repo mehr notwendig.

## 8. Scheduling-Faehigkeiten

- Cron Parser mit Quartz-kompatibler Syntax.
- Intervalle: `FixedInterval`, `SlidingInterval`, `DailyTimeInterval`.
- Kalender-Ausnahmen (Holiday Calendars) optional.
- Zeitliche Praezision: Nutzung von `DateTimeOffset` (UTC-first) plus Zeitzonen-Konverter.
- Schedule-Quelle: Primaer persisted im Persistence-Provider (z.B. SqlServer). Ohne Persistenz wird ein In-Memory-Store genutzt; Schedules koennen optional beim Startup registriert werden (z.B. via `AddCroniqJob` + Seed-Schedules).

### Entscheidung 1: Scheduling-Syntax

- **Empfehlung**: Quartz-Syntax vollstaendig uebernehmen (7 Felder inkl. Sekunden + Sonderzeichen `?`, `L`, `W`, `#`).
- **Begruendung**: Deckt Sekundenaufloesung, komplexe Regeln (letzter Werktag usw.) ab und ist mit Quartz.NET/Community-Tooling kompatibel; Crontab (5 Felder) wuerde Mehrfachlogik in Policies erzwingen.
- **Konsequenzen**: Parser ueber Quartz.NET-Implementierung oder Portierung aufbauen, Validierung + UI-Komponenten muessen Sonderzeichen erklaeren; Migration zu alternativer Syntax spaeter per Adapter moeglich.

## 9. API & RPC

- **Minimal API** (`Croniq.Api`):
  - Endpunkte: `POST /jobs/trigger`, `POST /schedules`, `GET /schedules/{id}`, `DELETE /schedules/{id}`, `GET /health`.
  - AuthN/AuthZ via API Keys oder OAuth2 (extension point).
- **Hosting**: Konsumenten hosten die API ueber Extensions `AddCroniqApiServices` + `AddCroniqApiRateLimiter` + `UseCroniqApi`; Auth/Persistence-Mode werden per Konfig (InMemory/SqlServer) geschaltet.
- **RPC**:
  - gRPC-Service `SchedulerService` mit Methoden `TriggerJob`, `GetSchedules`, `RegisterSchedule`.
  - Alternativ JSON-RPC fuer leichtere Clients; Client-SDK in `Croniq.Rpc.Client`.
- **Auth je Route (Default)**:
  - `GET /health`: keine Auth.
  - Alle uebrigen REST-Endpunkte: API-Key im Header `X-Croniq-Key` oder OAuth2 Client Credentials.
  - gRPC: Metadata-Header `x-croniq-key` oder OAuth2 Token; Rate Limiter identisch zu REST.
- **Debug/Tracing (gRPC)**: Optionale Logging/Tracing-Interceptors (per Config/Sampling) fuer Entwicklungs- und Troubleshooting-Modus; Payload-Logging nur in dev/stage, mit PII-Redaction und Groessen-Limits. Serializer fuer Debug-Logs im Proto-JSON-Format, um Binaerpayloads lesbar zu halten.

### Entscheidung 4: API-Transport & RPC

- **Empfehlung**: Minimal API (REST + JSON) als offizielle Verwaltungs- und Integrationsschnittstelle beibehalten und gRPC als primaeren RPC-Kanal etablieren; JSON-RPC nur als optionale Community-Erweiterung.
- **Begruendung**: REST/JSON ist fuer Admin-UIs, DevOps und Skripting am zugaenglichsten; gRPC bietet stark typisierte, performante Kommunikation fuer interne Services/Worker. Zwei offiziell unterstuetzte Kanaele halten den Aufwand ueberschaubar.
- **Konsequenzen**: API-Vertraege werden via OpenAPI/Swagger versioniert; gRPC erfordert Proto-Spezifikationen und CI-Generierung von Client-SDKs; AuthZ muss fuer beide Kanaele konsistent sein; JSON-RPC bleibt "best effort" und wird nicht als Kernprodukt garantiert.

## 10. Jobs in separaten Projekten

- `Croniq.Sdk` liefert NuGet-Package mit Interfaces, DTOs und Annotations.
- Jobs implementieren `IJob` und werden per Assembly-Scanning oder expliziter Registrierung (`services.AddCroniqJob<TJob>()`) eingebunden.
- Packaging-Empfehlung: Jede Domaene eigenes Class Library Projekt; Deployment via NuGet oder direkte Projekt-Referenzen.

### Entscheidung 5: Job-Autorenschaft

- **Empfehlung**: `Croniq.Sdk` als verbindliche Vertragsgrundlage etablieren, Jobs ausschliesslich ueber separate Class Libraries erstellen und per DI registrieren; Assembly-Scanning als Komfort-Feature, aber kein Hidden-Magic.
- **Begruendung**: Klare NuGet-Vertraege verhindern enge Kopplung an interne Scheduler-Typen; getrennte Projekte erleichtern Versionierung, Testbarkeit und erlauben unterschiedliche Deployment-Strategien (NuGet, Source Reference).
- **Konsequenzen**: SDK muss strikt semantisch versioniert werden; Breaking Changes benoetigen Deprecation-Plan; Dokumentation fuer Job-Autoren (Templates, Samples) erforderlich; Registry/DI-Konfiguration wird Teil der oeffentlichen API.

## 11. Policies & Error Handling

- Policy-Engine basierend auf Polly oder eigener Implementierung.
- Konfigurierbare Retry-Strategien (exponential backoff, fixed retry count).
- Error Routing: Failed Jobs -> Dead-letter Queue (In-Memory oder Persistenz), optional Notification Provider.

Handler signalisieren Fehler stets ueber Exceptions: zuerst loggen, optional `CustomState(..., JobState.Error)` setzen, anschliessend `throw;`, damit Retry- und Dead-Letter-Policies greifen. Semantische Zwischenzustaende (Waiting/Running/Finalized) werden nur bei Bedarf ueber `CustomState` publiziert, waehrend Fortschritt ueber `InitProgress`/`ReportProgress` laeuft. Das konkrete Authoring-Pattern ist in `docs/consumer/quickstart.md` dokumentiert.

### Entscheidung 6: Policy-Engine & Fehlerbehandlung

- **Empfehlung**: Auf Polly als Grundlage setzen und eigene Policy-Pipelines drumherum bauen (Retry, Circuit-Breaker, Timeout, Fallback). Dead-Letter-Handling wird als separater Provider implementiert, der Policies Ereignisse liefert.
- **Begruendung**: Polly ist battle-tested, integriert sich nativ in .NET und erlaubt deklarative Konfiguration; eigene Implementierung wuerde mehr Zeit kosten und weniger Community-Support bieten.
- **Konsequenzen**: Policies werden ueber Konfigurationsobjekte/Options an Jobs gebunden; Telemetrie muss Policy-Events (Retry, Breaker Open, Fallback) erfassen; Dead-Letter-Provider benoetigt Persistenzkonzept (z.B. SqlServer-Tabelle) und Tracing, um manuell zu rehydratisieren.

**Policy Resolution (Resolver)**

## 12. Docker & Deployment

- Dockerfiles pro Service (`Croniq.Api`, `Croniq.UI`, optionale Worker Nodes).
- Docker-Compose zum lokalen Start: API, UI, SQL Server 2022 Container (`mssql-22`/`CroniqDev`) mit Croniq-Schema (EF Core), Telemetry Stack (Jaeger/Prometheus).
- CI/CD-Pipeline (GitHub Actions) zum Bauen, Testen, Publish der Images und NuGet-Packages.

### Entscheidung 8: Container- & Deployment-Strategie

- **Empfehlung**: Multi-Stage Dockerfiles mit .NET 10 SDK/ASP.NET Runtime verwenden, Images auf Slim/Distroless-Basis fuer Produktion bauen. Lokales Dev-Setup via `docker-compose` (API, Worker, SQL Server mit Croniq-Schema, OTel, Grafana). GitHub Actions erzeugt signierte OCI-Images + Paket-Releases.
- **Begruendung**: Multi-Stage reduziert Image-Groesse und Angriffsflaeche; Compose beschleunigt lokales Onboarding; GitHub Actions integriert gut mit GitHub Container Registry + Code Signing.
- **Konsequenzen**: Einheitliche `Dockerfile`-Templates je Service; `.devcontainer` optional; CI-Pipeline benoetigt Buildx/Cache + Cosign/SBOM; Deployment-Envs (dev/stage/prod) nutzen identische Images, Konfigurationsunterschiede kommen ueber ENV/Secrets.

## 13. UI-Projekt (Backlog)

- Aktuell nachgelagert; Umsetzung startet erst, wenn API/Provider stabil sind.
- Anforderungen (Schedule-Uebersicht, Job-Trigger, Execution-Historie etc.) bleiben bestehen, werden jedoch im Backlog gefuehrt.
- Referenz: Abschnitt 16 "Kubernetes (Backlog)" fuer allgemeine Backlog-Vorgehensweise; UI-Technologie wird spaeter entschieden.

## 14. Weiteres & Offene Punkte

- Security Hardenings (Rate Limiting, Secrets Handling, Multi-Tenant-Faehigkeit).
- Observability: Standardisierte Logs, Metriken (Execution Duration, Queue Depth), Traces.
- Testing-Strategie: Unit-Tests fuer Core, Contract-Tests fuer Provider, Integrationstests mit Docker Compose.
- Roadmap: Cluster-Faehigkeit, UI-Technologie-Entscheidung, zusaetzliche Provider (Cloud Storage, Message Bus).

### Entscheidung 12: Test- & Quality-Strategie

- **Empfehlung**: Drei Teststufen verbindlich etablieren: (1) Unit-Tests mit xUnit + FluentAssertions fuer Core/Policies/SDK; (2) Contract-Tests gegen Provider ueber `Croniq.TestKit` (Shared Fixtures, Golden Files) inklusive Testcontainers fuer SqlServer; (3) End-to-End-Integration via Docker Compose Smoke-Suite, die API, Worker und Observability Stack hochzieht. Jede PR muss alle Unit- und Contract-Tests bestehen; E2E laeuft nightly und vor Release. Zusaetzlich erzwingen wir Coverage-Gates (min. 80% Core, 70% Gesamt) per Coverlet/ReportGenerator und statische Analyse (dotnet analyzers + SonarQube optional).
- **Begruendung**: Die Kombination deckt Logikfehler, Provider-Regressions und Deployability ab; Testcontainers haelt Feedback-Zeit niedrig, Compose-E2E spuert Interop-Bugs auf. Coverage-Gates und statische Analyse verhindern Qualitaetsabfall bei wachsender Codebasis.
- **Konsequenzen**: Repo benoetigt `Croniq.TestKit`-Projekt, gemeinsame Fixtures und Docker-Compose-Testdefinition. GitHub Actions erhaelt gestufte Jobs (Unit/Contract parallel, E2E separat) mit Pflicht-Gates. Entwickler brauchen lokale Testcontainer-Setup, Doku muss beschreiben, wie Tests gebootstrapped werden. Anforderungen an Hardware/CI (Docker Support) steigen.

### Entscheidung 9: Security-Hardening & Secrets

- **Empfehlung**: Minimal API und gRPC standardmaessig mit API-Key-Auth versehen (Header `X-Croniq-Key`), ergaenzt um optionales OAuth2 Client-Credentials fuer Enterprise-Deployments. Rate Limiting per ASP.NET Core RateLimiter (Sliding Window + Burst) pro API-Key/Tenant erzwingen; gRPC erhaelt denselben Guard ueber Interceptors. Secrets (API Keys, Connection Strings) werden ausschliesslich ueber einen `ISecretProvider` bezogen, der in Produktion gegen Vault/KeyVault/Secrets Manager gebunden ist; lokale Entwicklung nutzt `.env` + user secrets.
- **Begruendung**: API Keys sind schnell einsatzbereit und passen zu automatisierten Operator-Workloads; OAuth2 stellt Integration mit bestehenden IdPs sicher, ohne alle Nutzer dazu zu zwingen. Zentrales Rate Limiting schuetzt den Scheduler vor Abuse und ist in .NET 8+ nativ verfuegbar. Ein abstrahierter Secret Provider ermoeglicht Rotation, auditiertes Lesen und reduziert das Risiko hartkodierter Credentials.
- **Konsequenzen**: `Croniq.Api` benoetigt Middleware fuer Key-Validation, OAuth2 Bearer-Validation und RateLimiter-Policies; gRPC erfordert Interceptors + Metadata-Konvention. Provider/DB-Schema muessen Tenant-/API-Key-Metadaten aufnehmen (z.B. `TenantId`, `Quota`). Deployment-Pipelines muessen sichere Secret-Stores provisionieren; lokale Doku beschreibt, wie Keys erzeugt/rotiert werden. Multi-Tenant-Isolation setzt Namespacing im JobStore (Tenant-Scopes) und Policy-Konfiguration voraus.

### Entscheidung 7: Observability-Stack

- **Empfehlung**: Logging ueber Serilog (Structured Logging) mit Sink nach OpenTelemetry + optional Seq/ELK; Metriken und Traces durchgaengig via OpenTelemetry SDK und OTLP-Exporter. Standard-Dashboarding ueber Grafana/Tempo/Prometheus im DevOps-Stack.
- **Begruendung**: OpenTelemetry bietet einheitliche Instrumentierung fuer Logs/Metrics/Traces und ist Cloud-/Vendor-neutral; Serilog erleichtert strukturierte Logs im .NET-Oekosystem und spielt gut mit OTLP zusammen.
- **Konsequenzen**: Alle Services benoetigen OTel-Instrumentierung (Resource Builder, ActivitySource); CI/CD muss Collectors/Sinks provisionieren; Lokale Dev-Compose-Datei enthaelt OTel-Collector + Grafana/Tempo; Alerts/Dashboards definieren Kennzahlen (Queue Depth, Execution Duration, Policy Events).

## 15. Reliability & Recovery

- Misfire Handling: Verhalten bei Downtime (nachholen vs. verwerfen), definierte Maximalverzoegerung pro Trigger.
- Startup-Recovery: Persistente Trigger/Jobs nach Neustart laden, verwaiste Locks/Executions bereinigen.
- Zeitquellen: Clock-Drift Monitoring (NTP), Zeitzonen pro Schedule, Sicherstellung von UTC-first in allen Services.
- Data Retention: Aufbewahrung und automatische Bereinigung fuer Execution-Historie, Dead-Letter-Queue, Audit-Logs.
- Backup/Restore: Strategien fuer die CroniqDev SQL Server DB (Croniq-Schema), Konfigurationen und Secrets.

### Entscheidung 10: Reliability & Recovery

- **Empfehlung**: Misfires grundsaetzlich nachholen, solange sie innerhalb eines konfigurierbaren `MaxMisfireDelay` (Default 5 Minuten) liegen; Werte lassen sich global, pro Tenant und pro Trigger via `IMisfirePolicy` ueberschreiben. Policies unterstuetzen logarithmisch/exponentiell gedrosselte Catch-up-Strategien (z.B. nur jede n-te verpasste Ausfuehrung nachholen), um Fluten zu vermeiden; jenseits der Policy-Grenzen werden Events verworfen und als Dead-Letter markiert.
- **Startup-Recovery Ablauf (Default)**: Boot -> Connection zu Persistenz -> Persistente Trigger in Batches laden -> Verwaiste Locks via `sp_Croniq_CleanupLocks` loesen -> Recovery-Worker verarbeitet Dead-Letter/Reschedules -> Scheduler gibt neue Ausfuehrungen frei -> Healthcheck meldet "ready". Tests spiegeln diesen Ablauf (inkl. Failover-Case mit zwei Instanzen).
- **Zeitquelle**: `DateTimeOffset` mit NTP-validierter Systemzeit; kritische Komponenten ueberwachen Drift via `ITimeProvider` (Warnung ab 50 ms).
- **Retention & Backup**: Retention-Defaults: Dead-Letter 30 Tage (policy-gesteuert), Execution-Historie 90 Tage, Audit-Logs 365 Tage (alle konfigurierbar). Backups der CroniqDev-DB (SQL Server, Croniq-Schema) nightly per Dump/Snapshot; Wiederherstellungs-Playbook ist Teil der Ops-Doku.
- **Begruendung**: Nachholen innerhalb kurzer Zeitfenster stellt SLA-Verlaesslichkeit sicher, ohne nach langen Downtimes Jobs zu fluten. Expliziter Recovery-Worker verhindert Race Conditions beim Rehydrieren. Einheitliche Zeitquelle vermeidet Zeitzonenbugs, Retention haelt Datenbank schlank. Dokumentierte Backup-/Restore-Pfade erfuellen Compliance-Anforderungen.
- **Konsequenzen**: `Croniq.Core` benoetigt Misfire-Policy pro Trigger und Dead-Letter-Markierungen; Persistence-Skripte brauchen Cleanup-Prozeduren und Retention-Jobs. Startup-Sequenz blockt Scheduling bis Recovery abgeschlossen ist und Telemetrie meldet Status. Ops-Team muss NTP-Monitoring und Backup-Pipeline (inkl. Test-Restore) betreiben; Konfigurationswerte (`MaxMisfireDelay`, Retention) werden als Options exponiert und versioniert.

## 16. Kubernetes (Backlog)

- Aktuell zurueckgestellt; erst nach Kernfunktionen priorisieren, Punkte unten als Spickzettel behalten.
- Deployment-Form: Helm-Chart oder Kustomize-Basis mit Values fuer dev/stage/prod; Secrets/ConfigMaps klar getrennt.
- Probes & Readiness: Liveness/Readiness/Startup-Probes fuer API, Scheduler/Worker; Health-Endpoint festlegen.
- Ressourcen & SLOs: Requests/Limits, HPA (CPU/RAM/Queue-Length), PodDisruptionBudget, Anti-Affinity fuer Datenbank.
- Storage: Persistente Volumes fuer die CroniqDev SQL Server DB (Croniq-Schema), Backup-Jobs, Migrations-Job als initContainer/Job.
- Netzwerk & Security: NetworkPolicies, Ingress/TLS, RBAC/ServiceAccount, Leader-Election falls mehrere Scheduler-Instanzen.

### Entscheidung 13: Kubernetes-Basisstrategie (Backlog)

- **Empfehlung**: Wenn Kubernetes priorisiert wird, liefern wir ein einziges Helm-Chart (`charts/croniq`) mit Values-Overlays fuer dev/stage/prod; die Compose-Umgebung bleibt massgeblich fuer lokale Entwicklung. Das Chart provisioniert Deployment + HPA fuer API/Worker, StatefulSet fuer SQL Server (Croniq-Schema) samt PVC-Templates und `CronJob`/Job fuer Migrationen. Secrets werden ueber ExternalSecrets (Vault/KeyVault) eingebunden, ConfigMaps erhalten nur nicht-sensitive Defaults. Readiness/Liveness-Probes spiegeln die Minimal-API-/gRPC-Healthchecks wider, Autoscaling basiert auf CPU und Queue-Depth-Metriken. Zusaetzliche Komponenten (Ingress-Controller, Service Mesh, UI) bleiben optional/backlog und werden erst aktiviert, wenn die jeweiligen Streams starten.
- **Begruendung**: Ein zentrales Chart reduziert Drift zwischen Stages, laesst sich aber per Values flexibel anpassen; Compose bleibt der schnellste Dev-Pfad. StatefulSet + PVC garantiert Datenpersistenz fuer SQL Server (Croniq-Schema), Migration-Jobs verhindern Race Conditions beim Rollout. ExternalSecrets und Health-Probes adressieren Security/Availability ohne uebermaessigen Tooling-Aufwand.
- **Konsequenzen**: Repo benoetigt `infra/k8s/charts/croniq` inkl. README und Standard-Values; CI/CD muss Helm-Pakete linten/testen (z.B. `helm template` + `kubeconform`). Observability-Stack (OTel/Grafana) wird als optionales Subchart referenziert. UI bleibt weiterhin im Backlog - Chart enthaelt nur Platzhalter-Werte, bis Technologie gewaehlt ist. Team braucht Basis-Kubernetes-Richtlinien (Namespaces, RBAC), die parallel dokumentiert werden.

## 17. Release & Compliance

- Versionierung: SemVer fuer Libraries/SDKs, API-Versionierung (z.B. `/v1`) und Deprecation-Policy.
- Migrationen: Forward-/Backward-kompatible DB-Migrationen, Rollback-Plan und Smoke-Tests nach Deployment.
- Supply Chain Security: SBOM-Erzeugung, Signierung von Images/Packages, Vulnerability-Scanning in CI/CD.
- Lizenzen: Third-Party-Notice, Lizenzpruefungen der Abhaengigkeiten, Richtlinie fuer neue Dependencies.

### Entscheidung 11: Release-, Compliance- & Supply-Chain-Strategie

- **Empfehlung**: Alle Libraries/SDKs strikt nach SemVer veroeffentlichen; Services exponieren `/v1`-Routen, Breaking Changes nur ueber neue Major-Versionen + 2 Release-Zyklen Deprecation. Datenbankmigrationen laufen via EF Core Migrations (`Croniq.Data.SqlServer`) mit Forward-only-Strategie (Rollback via Restore). Releases enthalten automatische Smoke-Tests nach Deployment. GitHub Actions erzeugt fuer jedes Artefakt (NuGet, OCI) eine attestierte SBOM (Syft) und signiert Images/Packages via Cosign/SignPath. Dependency-Updates laufen durch woechentliche Renovate-PRs mit Lizenz-Check (OSS Review Toolkit) und verpflichtendem Vulnerability-Scan (Trivy/Snyk) vor Merge.
- **Begruendung**: Strikte Versionierung und Deprecation-Regeln geben Konsumenten Planbarkeit; DB-Hardening mit Rollbacks reduziert Downtime-Risiken. SBOM + Signaturen beantworten Supply-Chain-Compliance-Anforderungen und vereinfachen Incident Response. Automatisierte Scans halten Dependencies aktuell und rechtssicher.
- **Konsequenzen**: Release-Templates in GitHub Actions muessen Version-Bumping, Changelog-Generierung und SBOM/Signatur-Schritte enthalten; QA betreibt automatische Smoke-Tests (Compose/K8s). EF-Migrationen werden als Teil des Release-Prozesses geprueft (`Croniq.DbMigrator --verify`). Compliance-Dokumentation (Third-Party-Notice, Policy fuer neue Dependencies) wird Teil des Release-Prozesses; fehlende Scans blockieren Merge/Release.

## 18. Multi-Tenancy & Quotas

- Tenant-Namespace fuer Schedules/Jobs/Policies im JobStore; API-Key/Tenant-Zuordnung erzwingt Isolation in Persistenz und Telemetrie.
- Quotas pro Tenant (z.B. max Trigger/Minute, parallele Executions, Payload-Groesse) abgestimmt auf RateLimiter-Policy.
- Namespacing in Provider: Persistence-, Dead-Letter- und Telemetry-Daten immer mit TenantId speichern; Admin-APIs brauchen Scope-Grenzen.
- Optional: Dedicated Connection Strings pro Tenant (Premium) vs. Shared DB; Migrations/Backups muessen beide Topologien abdecken.

## 19. Execution Semantics & Idempotenz

- Delivery-Garantie: Standard at-least-once; Deduplikation via `ExecutionId` + optionale `IdempotencyKey` vom Caller. At-most-once nur fuer spezifische Provider/Policies.
- Parallelitaet: Concurrency-Grenzen pro Job/Schedule (SingleFlight pro JobKey default), optional konfigurierbar; Queue-Prioritaeten per Policy.
- Timeouts & Cancellation: Jobs erhalten CancellationToken; Timeout-Policies brechen Lauf ab, markieren Ergebnis und verschieben in Dead-Letter gemaess Policy.
- Side-Effects: Empfehlung Outbox/Inbox fuer externe Events; Job-Kontrakte sollen idempotent sein oder Deduplikation ueber Provider anbieten.

## 20. Decision Tracking & Offene Fragen

- **Gefestigt**: Scheduling-Syntax (Quartz-kompatibel), In-Memory-JobStore als Default + SqlServer-Provider, Policy-Engine auf Polly-Basis, API-Key/OAuth2 + Rate Limiting, Observability via OTel/Serilog, Release-Flow mit SBOM/Signierung, Quota-Default (60 Trigger/Minute, 5 parallele Executions/JobKey), Dead-Letter-Retention 30 Tage (policy-gesteuert).
- **Offen / zu konkretisieren**:
  - UI-Technologie (wird nach API-Stabilisierung entschieden).
  - Cluster-GA-Kriterien und Zeitpunkt (wann SqlServer-Provider als produktionsreif gilt).
  - Dead-Letter manuelle Rehydrate-Flows (Standard 30 Tage, API/CLI-Flow definieren).
  - JSON-RPC Support-Level (Community-only oder voller Support).

Nächste sinnvolle Schritte: OIDC/JWT-Pfad ergänzen (CallerContext aus Bearer), Security-/Test-Strategie ausarbeiten und die Admin-Routen (Tenant/API-Key Management) auf die neue Auth-Schicht aufsetzen.
