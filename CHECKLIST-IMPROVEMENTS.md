# Croniq UX Improvements (Croniq Produkt)

Ziel: Croniq so weiterentwickeln, dass Consumer-Services (z.B. `TimeCockpit.Service`) Croniq **als einzigen Scheduler** mit **Quartz-ähnlich einfachem Setup** verwenden können.

Leitplanken:

- **Weniger Boilerplate**: so wenig `using`/Wiring wie möglich.
- **Sichere Defaults**: Out-of-the-box startbar (insb. InMemory), ohne dass man 10 Config-Sections anfassen muss.
- **Modellierbare Trigger**: Schedules/One-Offs/Metadata sollen über ein klares Modell + Config/API definierbar sein.
- **Gute Observability**: Beim Start und im Betrieb ist klar, was wann läuft und warum.

## 1) „Minimal Host“ Defaults (sehr wichtig)

Problem heute: `AddCroniqCore()` + `AddCroniqInMemoryJobStore()` + `AddCroniqWorkerHost()` + viele `Configure<...>` Calls sind für Consumer zu viel.

- [x] **Neue Convenience-Extension**: `services.AddCroniqWorker(configuration, configure?)` (oder ähnlich), die intern:
  - [x] `AddCroniqCore()` + Default Provider + `AddCroniqInMemoryJobStore()` + `AddCroniqWorkerHost()` verdrahtet.
  - [x] Minimal-Config nur aus `Croniq:Core` zieht (Tenant/Environment/InstanceId) – mit Defaults.
  - [x] Optionale Hooks für Overrides bietet (`Action<CroniqOptions>`, `Action<WorkerHostOptions>`).
  - [x] Die Convenience-Extension liegt im Facade-Projekt `Croniq` (nicht in `Croniq.Core`), damit `Croniq.Core` keine Abhängigkeiten auf konkrete Stores/Provider bekommt.
- [x] **Default Provider auto-wiring**: Prüfen, ob `AddCroniqCore()` bereits sinnvolle Provider setzt (Logger/Telemetry/Secrets). Wenn nicht: Default Provider als Teil des „Minimal Host“ aktivieren.

## 2) Konfigurationsoberfläche vereinfachen (sehr wichtig)

Problem heute: `PlatformHostingExtensions.AddCroniqPlatformServices` bindet viele Sections (Auth, OIDC, Tokens, Password, Policies, Persistence, SqlServer) – für reine Worker-Consumer ist das Overkill.

- [x] **Platform vs Worker klar trennen**:
  - [x] `AddCroniqPlatformServices(...)` bleibt für API/Plattform.
  - [x] **Neue** `AddCroniqWorkerServices(...)` (Worker-only), bindet nur die wirklich benötigten Options.
- [x] **Option-Binding Defaults**:
  - [x] Wenn Sections fehlen, sollen sinnvolle Defaults greifen statt „mysteriösem Verhalten“.
  - [x] Harte Exceptions nur dort, wo es wirklich nötig ist (z.B. SqlServer Mode ohne ConnectionString).

## 3) Trigger-Model & Seeding-Strategie im Core (sehr wichtig)

Problem heute: Consumer müssen Trigger selbst „zusammenklicken“ und beim Start per Upsert persistieren. Das lädt zu "Overrides"/"Drift" und zu Überschreiben von UI/API Änderungen ein.

- [x] **First-class Trigger-Seeding**: Core/Hosting soll ein offizielles Konzept für „Seeding“ liefern:
  - [x] `Croniq:Seeding:Mode = Off|CreateIfMissing|ForceUpdate`.
  - [x] Standard: `CreateIfMissing`.
  - [x] `ForceUpdate` nur für Trigger mit `managedBy=<app>`.
- [x] **Fluent Trigger-Seeding**: `services.AddCroniqJob(...).AddTrigger(...)` fuer code-first Seeds.
- [ ] **Trigger-Model in Config** (modellierbar):
  - [x] `Croniq:Triggers` als Liste/Map (TriggerId, JobKey, CronExpression, StartAtUtc, EndAtUtc, Enabled, Metadata).
  - [ ] Optional „typed metadata“/Konventionen (z.B. `days`) dokumentieren.
- [x] **Validation & Summaries**:
  - [x] Cron-Expression validate (fail-fast) + human readable Summary (für Logs/UI).

## 4) „One-Off / RunOnce“ als offizielles Feature (wichtig)

Problem heute: One-Off wird oft direkt über Pipeline „manuell“ ausgelöst. Das ist funktional, aber UX (Nachvollziehbarkeit) leidet.

- [ ] **Offizielles API/SDK**: `IJobTrigger` (oder ähnlich) für `TriggerOnceAsync(jobKey, metadata, delay?)`.
- [ ] **Optional persistierter One-Off Trigger**: Wenn es besser passt, One-Off als TriggerDefinition mit `StartAtUtc` + Cron „@once“/special schedule modellieren (oder eigener Schedule-Typ).

## 5) Startup UX: „Was läuft wann?“ (wichtig)

Problem heute: Quartz-User lieben „Job Summary“. Croniq sollte das out-of-the-box liefern.

- [x] **HostedService im WorkerHost**: Beim Start die Trigger des Scopes laden und in Logs ausgeben:
  - [x] Trigger count, disabled triggers, nächster Run je Trigger.
  - [x] Einheitliche Log-Templates + strukturierte Felder (`tenantId`, `environmentTag`, `jobKey`, `triggerId`, `nextFireAtUtc`).

## 6) Usability: weniger `using` / weniger "Core"-Typen direkt anfassen (mittel)

- [ ] **Facade-Paket als Entry-Point (empfohlen)**: neues Projekt `Croniq`, das:
  - [x] die „Happy Path“-API in `Croniq` bündelt (`AddCroniqWorker(...)`, optional Builder/Fluent API).
  - [x] Dependencies bündelt (Core + DefaultProviders + JobStore), sodass Consumer idealerweise nur 1 Package referenzieren.
  - [ ] Options/Model-Typen (die Consumer konfigurieren sollen) nicht im `Croniq.Core.*` Namespace „versteckt“.
- [ ] **Optional: Global Usings (opt-in)**: separates Package `Croniq.Usings` (NuGet `buildTransitive` `.props` mit `<Using Include="..." />`) statt „magisch“ in Core/Worker.
- [ ] **Docs + Copy/Paste Snippets**: Minimal WorkerHost Setup (InMemory) in 10–15 Zeilen, ohne versteckte prerequisites.

## 7) Policies als Defaults (mittel)

- [x] **Sinnvolle Default Policies** (Retry/Timeout/Misfire) aktiv, aber konservativ.
- [x] **Override Modell**: Einfacher Einstieg (ein globaler Default) + optional pro JobKey.

## 8) Trigger-API konsistent mit Core (mittel)

- [ ] **API/Grpc/Config nutzen dieselben DTOs/Validatoren** wie das Core-Trigger-Model.
- [ ] **No-surprises**: API-Änderungen dürfen nicht beim nächsten Worker-Start "weg-geupsertet" werden (nur bei ForceUpdate + managedBy).

## 9) Job Registration UX (wichtig)

Problem heute: Jobs müssen häufig einzeln registriert werden; das erhöht Boilerplate und ist fehleranfällig.

- [x] **Inline Job Registration**: `AddCroniqJob("namespace", "name", handler)` fuer delegate-based Jobs.
- [ ] **Assembly-Scanning**: `services.AddCroniqJobsFromAssembly(Assembly)` / `AddCroniqJobsFromEntryAssembly()` (scan nach `CroniqJobAttribute` + auto `AddCroniqJob<T>`).
- [ ] **Gute Fehlermeldungen**: Bei Duplicate JobKeys/fehlenden Attributen klare Errors inkl. Vorschlag.
- [ ] **Optional: Source Generator** (später): Compile-time Registrierung für „zero reflection“ + bessere DX.

## 10) Fail-fast Options/Config UX (wichtig)

- [ ] **ValidateOnStart**: Core/Worker-Defaults sollen fehlende Pflichtwerte sofort mit klarer Message abbrechen (statt „mysteriöser“ Runtime-Effekte).
- [ ] **„Validate only“ Mode**: `Croniq:Startup:Mode = Run|Validate` (lädt/bindet/validiert Config + Triggers, startet aber keine Worker-Loops).
- [ ] **Health/Readiness**: Optional `AddCroniqHealthChecks()` (z.B. „JobStore erreichbar“, „Trigger geladen“, „Worker lease ok“).

## 11) Templates & Tooling (nice-to-have)

- [ ] **dotnet templates**: `dotnet new croniq-worker` / `dotnet new croniq-platform` inkl. minimaler `appsettings.json`.
- [ ] **CLI/Dev-Tool** (optional): Trigger-Liste, „next runs“, config validate, export/import (z.B. `dotnet tool`).
