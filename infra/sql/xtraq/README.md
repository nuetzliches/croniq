## Xtraq SQL Layout & Usage

Quickstart (lokal):
- SQL Server 2022 starten (Compose/Docker): `docker compose -f infra/docker/docker-compose.yml up -d` (Passwort/Port ueber `.env`).
- Schema anwenden: `cd infra/sql/xtraq; ./apply.ps1 -Server "localhost,11433"` (Passwort aus `.env` oder Parameter). Nur die Skripte unter `infra/sql/xtraq` werden ausgefuehrt; keine Ad-hoc-Queries.
- Xtraq CLI aus Provider-Projekt: `cd src/Croniq.Persistence.Xtraq; xtraq snapshot; xtraq build` (Connection String auf CroniqDev setzen).

Wichtige Punkte:
- Vertrag fuer Keys: Jobs und Trigger haben string-basierte Keys (`JobKey`, `TriggerKey`); FKs bleiben numerisch. Procs/TVPs transportieren die Keys, Provider nutzt ausschliesslich Stored Procedures.
- Neue Lookup-Procs: `croniq.JobFindByKey` liefert Job-Daten als JSON; `croniq.JobIdGetOrCreate` dient als Upsert-freier Lookup/Create.
- Ordnung der Skripte (siehe apply.ps1): `predeploy.sql` -> `core/types.sql` -> `core/procs.health.sql` -> `core/procs.errors.sql` -> `core/procs.guards.sql` -> `core-internal/types.sql` -> `core-internal/procs.actors.sql` -> `croniq/types.sql` -> `croniq/functions.sql` -> `croniq-internal/types.sql` -> `croniq-internal/procs.errors.sql` -> `croniq-internal/procs.guards.sql` -> `auth/types.sql` -> `auth/tables.sql` -> `auth/procs.keys.sql` -> `croniq/tables.sql` -> Procs (`instances`, `jobs`, `leases`, `deadletter`).
- Health: `[core].[HealthPing]` liegt in `core/procs.health.sql` und wird automatisch von `apply.ps1` mit ausgefuehrt. Xtraq erzeugt dafuer `Core/HealthPing.cs`; nach SQL-Aenderungen immer `xtraq snapshot`/`build` fahren.

Konventionen (Auszug):
- Alle Zugriffe laufen ueber die Prozeduren; keine direkte SQL im Provider.
- Prozedur-Parameter haben keine Default-Werte; Pflicht/Optionalitaet wird durch die UDTs/TVPs bestimmt.
- Tabellenspalten definieren kein explizites `NULL`/`NOT NULL`, da dies vom jeweiligen UDT vorgegeben ist.
- Keine Systemtypen in Tabellen/Parametern: bei Bedarf neue UDTs anlegen und verwenden (keine NVARCHAR/INT direkt in DDL/DML-Contracts).
- DML-Prozeduren liefern Ergebnisse ueber OUTPUT-Parameter statt SELECT-Resultsets; bei verschachtelten Procs nur Positionsargumente verwenden (z.B. `EXEC auth.ApiKeyRevoke @TenantId, @KeyRef, @Actor, NULL, @Affected OUTPUT`).
- UDTs/TVPs definieren Pflichtfelder; Guard-Procs werfen 5000x Fehlercodes bei fehlenden/ungueltigen Eingaben.
- Zeit/Defaults: UTC mit `SYSUTCDATETIME()`, Soft-Delete ueber `IsDeleted` (core.flag), Identity-Start 1001.
- Session-Settings: Jede Prozedur setzt zu Beginn `SET QUOTED_IDENTIFIER ON; SET ANSI_NULLS ON;`. Weitere SET-Optionen (inkl. NOCOUNT) werden zentral im SessionSettings-Helper der Anwendung gesetzt (nicht mehr in den Procs duplizieren).

Migration/Tests:
- Fuer sauberen Stand DB droppen oder leere DB anlegen, dann `apply.ps1` laufen lassen.
- Nach SQL-Aenderungen stets `xtraq snapshot`/`build` neu ausfuehren, damit die C#-Artefakte aktualisiert werden.
- `apply.ps1` ist der einzige Weg, die Schema-/Proc-Aenderungen einzuspielen. Die generierten Artefakte haengen daran: erst SQL aendern, dann `apply.ps1`, dann `xtraq snapshot`/`build`.
