## Xtraq SQL Layout

Spezifikation fuer UDTs und Tabellen (CamelCase) nach Schema-Trennung.

### Struktur

- `core/types.sql`: Basis-UDTs (key, keyBig, uid, utcDateTime, reference, tag, label, name, count, intervalMs, number, labelNullable, actor, actorNullable, jsonNullable, flag) plus UDTT `ActorRef` und Guard/Throw-Procs (`core.GuardActor`, `core.ThrowActorRequired`).
- `croniq/types.sql`: Domain-UDTs (cronExpression, timeZoneId, stateCode, jobVariant, deadLetterReason) plus UDTTs `InstanceRef`, `JobRef`, `TriggerRef` und Guard/Getter/Throw-Funktionen (z.B. `croniq.GuardInstanceRef`, `croniq.GuardJobRef`, `croniq.GuardTriggerRef`, `croniq.Throw*`).
- `auth/tables.sql`: `[auth].[Tenants]` mit `TenantId` (IDENTITY 1001,1), Reference (gefilteter Unique-Index `WHERE IsDeleted=0`), Name, Created/Updated + Principals, IsDeleted.
- `croniq/tables.sql`: `[croniq].[Instances]`, `[Jobs]`, `[Triggers]`, `[TriggerLeases]`, `[TriggerDeadLetter]` mit Tenant-FKs, Environment, Namespace, Name, Variant, Cron, TimeZoneId, Payload/Metadata (jsonNullable), Created/Updated + Principals, IsDeleted. PKs: `[TableName]Id` (Jobs/DeadLetter/Triggers/TriggerLeases: keyBig IDENTITY 1001,1; Instances: reference). Unique Keys auf Jobs/Triggers als gefilterte Indizes (`WHERE IsDeleted = 0`); Index-Hinweise fuer Stale-Cleanup (`IX_croniq_TriggerLeases_StaleCheck`, `IX_croniq_Instances_LastSeen`).
- `croniq/procs.instances.sql`: Procs fuer Cluster-Membership (`InstanceRegister`, `InstanceHeartbeat`, beide mit UDTT `ActorRef` + `InstanceRef`) und Failover-Cleanup (`TriggerLeaseCleanup` mit `ActorRef`), nutzen Guard-/Throw-Prozeduren fuer Actor/InstanceRef; Reuse von Soft-Deletes steuerbar ueber `@AllowDeletedReuse`.
- `croniq/procs.jobs.sql`: Procs fuer Jobs/Triggers (`JobUpsert`, `JobDelete`, `TriggerUpsert`, `TriggerDelete`) mit `ActorRef` + `JobRef`/`TriggerRef`, Guard-/Throw-Nutzung und OUTPUT-Parametern; Reuse von Soft-Deletes via `@AllowDeletedReuse`.
- Helper/Guard: `core.GetActor` + `core.GuardActor` fuer `ActorRef`; `croniq.GetInstanceId|Environment|NodeName|Capabilities|Version` + `croniq.GuardInstanceRef` fuer `InstanceRef`; weitere Guard/Throw-Procs fuer Job/Trigger.

### Konventionen

- Stored Procedures benennen ohne Prefix nach Muster `[schema].[EntityAction]` (keine `sp_`-Praefixe).
- Keine NULL/NOT NULL in Spaltendefinitionen; Nullbarkeit folgt aus dem UDT (Standard NOT NULL, explizit NULL bei *_Nullable/JSON/Variant).
- Zeitstempel immer UTC (`core.utcDateTime`), Default per `SYSUTCDATETIME()`.
- Soft Delete ueber `core.flag` mit Default 0 (`IsDeleted`).
- CreatedBy/UpdatedBy tracking via `core.actor`/`actorNullable`.
- Foreign Keys enthalten Schema im Namen (z.B. `FK_croniq_Jobs_auth_Tenants`).
- IDENTITY-Startwert: 1001, Schritt 1 fuer numerische IDs.
- Tabellen im Singular benennen; Aliase immer mit `AS` und lower-case.
- Nullable Parameter/Spalten erhalten kein explizites `= NULL`, Nullbarkeit ergibt sich aus dem UDT.
- Lokale Variablen mit vorhandenen UDTs deklarieren (keine nativen Systemtypen direkt), Datumswerte via `core.utcDateTime`.
- `EXISTS`-Pruefungen als `SELECT TOP (1) 1` formulieren.
- `InstanceRegister`/`InstanceHeartbeat` erwarten den UDTT `croniq.InstanceRef` fuer InstanceId, Environment, NodeName, Capabilities, Version (Version non-nullable); erster Parameter jeder Prozedur ist `@Actor [core.ActorRef]` ohne Default; Werte aus den TVPs ueber die Getter-/Guard-Funktionen beziehen.
- DML-Procs geben Werte via OUTPUT-Parameter zurueck (keine SELECT-Resultsets); Errors nur ueber benannte Throw-Prozeduren.
- Reuse-Policy: Procs unterstuetzen `@AllowDeletedReuse` (default 1) fuer Upserts von Instanzen/Jobs/Triggers; gefilterte Unique-Indizes (`WHERE IsDeleted = 0`) erlauben Reuse bei Soft-Delete.
- PKs: Trigger und TriggerLeases als `keyBig IDENTITY`, Instances als reference (guid), Jobs/DeadLetter als `keyBig IDENTITY`.

### Offenes

- Stored Procedures fuer Acquire/Release/Upsert (Triggers/Leases) und Cleanup/Retention sind noch zu spezifizieren.
