## Xtraq SQL Layout & Usage

Quickstart (lokal):
- SQL Server 2022 starten (Compose/Docker): `docker compose -f infra/docker/docker-compose.yml up -d` (Passwort/Port ueber `.env`).
- Schema anwenden: `cd infra/sql/xtraq; ./apply.ps1 -Server "localhost,11433"` (Passwort aus `.env` oder Parameter). Nur die Skripte unter `infra/sql/xtraq` werden ausgefuehrt; keine Ad-hoc-Queries.
- Xtraq CLI aus Provider-Projekt: `cd src/Croniq.Persistence.Xtraq; xtraq snapshot; xtraq build` (Connection String auf CroniqDev setzen).

Wichtige Punkte:
- Vertrag fuer Keys: Jobs und Trigger haben string-basierte Keys (`JobKey`, `TriggerKey`); FKs bleiben numerisch. Procs/TVPs transportieren die Keys, Provider nutzt ausschliesslich Stored Procedures.
- Neue Lookup-Procs: `croniq.JobFindByKey` liefert Job-Daten als JSON; `croniq.JobIdGetOrCreate` dient als Upsert-freier Lookup/Create.
- Ordnung der Skripte: `core/types.sql` -> `croniq/types.sql` -> `auth/tables.sql` -> `croniq/tables.sql` -> Procs (`instances`, `jobs`, `jobs.findbykey`, `jobs.idgetorcreate`, `leases`, `deadletter`).

Konventionen (Auszug):
- Alle Zugriffe laufen ueber die Prozeduren; keine direkte SQL im Provider.
- UDTs/TVPs definieren Pflichtfelder; Guard-Procs werfen 5000x Fehlercodes bei fehlenden/ungueltigen Eingaben.
- Zeit/Defaults: UTC mit `SYSUTCDATETIME()`, Soft-Delete ueber `IsDeleted` (core.flag), Identity-Start 1001.

Migration/Tests:
- Fuer sauberen Stand DB droppen oder leere DB anlegen, dann `apply.ps1` laufen lassen.
- Nach SQL-Aenderungen stets `xtraq snapshot`/`build` neu ausfuehren, damit die C#-Artefakte aktualisiert werden.
