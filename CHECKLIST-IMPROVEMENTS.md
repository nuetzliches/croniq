# Croniq UX Improvements (Croniq Produkt)

Ziel: Croniq so weiterentwickeln, dass Consumer-Services (z.B. `TimeCockpit.Service`) Croniq **als einzigen Scheduler** mit **Quartz-ähnlich einfachem Setup** verwenden können.

Leitplanken:

- **Weniger Boilerplate**: so wenig `using`/Wiring wie möglich.
- **Sichere Defaults**: Out-of-the-box startbar (insb. InMemory), ohne dass man 10 Config-Sections anfassen muss.
- **Modellierbare Trigger**: Schedules/One-Offs/Metadata sollen über ein klares Modell + Config/API definierbar sein.
- **Gute Observability**: Beim Start und im Betrieb ist klar, was wann läuft und warum.

## 1) „Minimal Host“ Defaults (sehr wichtig)

Problem heute: `AddCroniqCore()` + `AddCroniqInMemoryJobStore()` + `AddCroniqWorkerHost()` + viele `Configure<...>` Calls sind für Consumer zu viel.

- [ ] **Neue Convenience-Extension**: `services.AddCroniqWorker(configuration, configure?)` (oder ähnlich), die intern:
  - [ ] `AddCroniqCore()` + Default Provider + `AddCroniqInMemoryJobStore()` + `AddCroniqWorkerHost()` verdrahtet.
  - [ ] Minimal-Config nur aus `Croniq:Core` zieht (Tenant/Environment/InstanceId) – mit Defaults.
  - [ ] Optionale Hooks für Overrides bietet (`Action<CroniqOptions>`, `Action<WorkerHostOptions>`).
- [ ] **Default Provider auto-wiring**: Prüfen, ob `AddCroniqCore()` bereits sinnvolle Provider setzt (Logger/Telemetry/Secrets). Wenn nicht: Default Provider als Teil des „Minimal Host“ aktivieren.

## 2) Konfigurationsoberfläche vereinfachen (sehr wichtig)

Problem heute: `PlatformHostingExtensions.AddCroniqPlatformServices` bindet viele Sections (Auth, OIDC, Tokens, Password, Policies, Persistence, SqlServer) – für reine Worker-Consumer ist das Overkill.

- [ ] **Platform vs Worker klar trennen**:
  - [ ] `AddCroniqPlatformServices(...)` bleibt für API/Plattform.
  - [ ] **Neue** `AddCroniqWorkerServices(...)` (Worker-only), bindet nur die wirklich benötigten Options.
- [ ] **Option-Binding Defaults**:
  - [ ] Wenn Sections fehlen, sollen sinnvolle Defaults greifen statt „mysteriösem Verhalten“.
  - [ ] Harte Exceptions nur dort, wo es wirklich nötig ist (z.B. SqlServer Mode ohne ConnectionString).

## 3) Trigger-Model & Seeding-Strategie im Core (sehr wichtig)

Problem heute: Consumer müssen Trigger selbst „zusammenklicken“ und beim Start per Upsert persistieren. Das lädt zu "Overrides"/"Drift" und zu Überschreiben von UI/API Änderungen ein.

- [ ] **First-class Trigger-Seeding**: Core/Hosting soll ein offizielles Konzept für „Seeding“ liefern:
  - [ ] `Croniq:Seeding:Mode = Off|CreateIfMissing|ForceUpdate`.
  - [ ] Standard: `CreateIfMissing`.
  - [ ] `ForceUpdate` nur für Trigger mit `managedBy=<app>`.
- [ ] **Trigger-Model in Config** (modellierbar):
  - [ ] `Croniq:Triggers` als Liste/Map (TriggerId, JobKey, CronExpression, StartAtUtc, EndAtUtc, Enabled, Metadata).
  - [ ] Optional „typed metadata“/Konventionen (z.B. `days`) dokumentieren.
- [ ] **Validation & Summaries**:
  - [ ] Cron-Expression validate (fail-fast) + human readable Summary (für Logs/UI).

## 4) „One-Off / RunOnce“ als offizielles Feature (wichtig)

Problem heute: One-Off wird oft direkt über Pipeline „manuell“ ausgelöst. Das ist funktional, aber UX (Nachvollziehbarkeit) leidet.

- [ ] **Offizielles API/SDK**: `IJobTrigger` (oder ähnlich) für `TriggerOnceAsync(jobKey, metadata, delay?)`.
- [ ] **Optional persistierter One-Off Trigger**: Wenn es besser passt, One-Off als TriggerDefinition mit `StartAtUtc` + Cron „@once“/special schedule modellieren (oder eigener Schedule-Typ).

## 5) Startup UX: „Was läuft wann?“ (wichtig)

Problem heute: Quartz-User lieben „Job Summary“. Croniq sollte das out-of-the-box liefern.

- [ ] **HostedService im WorkerHost**: Beim Start die Trigger des Scopes laden und in Logs ausgeben:
  - [ ] Trigger count, disabled triggers, nächster Run je Trigger.
  - [ ] Einheitliche Log-Templates + strukturierte Felder (`tenantId`, `environmentTag`, `jobKey`, `triggerId`, `nextFireAtUtc`).

## 6) Usability: weniger `using` / weniger "Core"-Typen direkt anfassen (mittel)

- [ ] **Re-exports / Facade Namespace**: Für Consumer ein „Croniq“ Facade-Package oder Namespace anbieten, sodass typische Hosts nur `using Croniq;` brauchen (statt `Croniq.Core.*`, `Croniq.JobStore.*`, `Croniq.Persistence.*`).
- [ ] **Docs + Copy/Paste Snippets**: Minimal WorkerHost Setup (InMemory) in 10–15 Zeilen, ohne versteckte prerequisites.

## 7) Policies als Defaults (mittel)

- [ ] **Sinnvolle Default Policies** (Retry/Timeout/Misfire) aktiv, aber konservativ.
- [ ] **Override Modell**: Einfacher Einstieg (ein globaler Default) + optional pro JobKey.

## 8) Trigger-API konsistent mit Core (mittel)

- [ ] **API/Grpc/Config nutzen dieselben DTOs/Validatoren** wie das Core-Trigger-Model.
- [ ] **No-surprises**: API-Änderungen dürfen nicht beim nächsten Worker-Start "weg-geupsertet" werden (nur bei ForceUpdate + managedBy).
