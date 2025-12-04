## SQL Server dev setup (sqlpackage/dacpac)

### .env anlegen

`infra/atlas/.env` basierend auf `.env.example` erstellen:
```
SA_PASSWORD=Croniq@12345!
MSSQL_PORT=14333
MSSQL_DB=CroniqDev
MSSQL_USER=sa
```

### Docker-DB starten

```
docker compose -f infra/atlas/docker-compose.yml --env-file infra/atlas/.env up -d
```

### Skripte anwenden und ggf. dacpac erzeugen

PowerShell:
```
powershell -ExecutionPolicy Bypass -File infra\atlas\apply.ps1 -EnvFile infra\atlas\.env [-UseDockerSqlcmd] [-SkipExtract] [-DacpacOutput schema.dacpac] [-SqlPackagePath sqlpackage]
```
- `-UseDockerSqlcmd` nutzt das sqlcmd im Container (TLS/Path-Probleme umgehen).
- Standard: wendet alle SQL-Skripte an und erstellt ein dacpac via `sqlpackage /Action:Extract`.
- `-SkipExtract` ueberspringt das dacpac-Extract.

### Voraussetzungen

- Docker + mssql/server:2022-latest Image.
- `sqlcmd` (Host oder im Container) und `sqlpackage` (für dacpac-Extract) installiert/erreichbar.





Nächte Schritte: sqlpackage auf dem Host sicherstellen; falls TLS/Certs zicken, -UseDockerSqlcmd nutzen.