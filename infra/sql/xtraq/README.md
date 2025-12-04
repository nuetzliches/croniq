## Xtraq SQL Layout

Spezifikation fuer UDTs und Tabellen (CamelCase) nach Schema-Trennung.

### Struktur

- `core/types.sql`: Basis-UDTs (key, keyBig, uid, utcDateTime, reference, tag, label, name, labelNullable, principal, principalNullable, jsonNullable, flag).
- `croniq/types.sql`: Domain-UDTs (cronExpression, timeZoneId, stateCode, jobVariant, deadLetterReason).
- `auth/tables.sql`: `[auth].[Tenants]` mit `TenantId` (IDENTITY 1001,1), Reference (unique), Name, Created/Updated + Principals, IsDeleted.
- `croniq/tables.sql`: `[croniq].[Jobs]`, `[Triggers]`, `[TriggerLeases]`, `[TriggerDeadLetter]` mit Tenant-FKs, Environment, Namespace, Name, Variant, Cron, TimeZoneId, Payload/Metadata (jsonNullable), Created/Updated + Principals, IsDeleted. PKs: `[TableName]Id` (Jobs/DeadLetter: keyBig IDENTITY 1001,1; Trigger/Lease: uid).

### Konventionen

- Keine NULL/NOT NULL in Spaltendefinitionen; Nullbarkeit folgt aus dem UDT (Standard NOT NULL, explizit NULL bei *_Nullable/JSON/Variant).
- Zeitstempel immer UTC (`core.utcDateTime`), Default per `SYSUTCDATETIME()`.
- Soft Delete über `core.flag` mit Default 0 (`IsDeleted`).
- CreatedBy/UpdatedBy tracking via `core.principal`/`principalNullable`.
- Foreign Keys enthalten Schema im Namen (z.B. `FK_croniq_Jobs_auth_Tenants`).
- IDENTITY-Startwert: 1001, Schritt 1 für numerische IDs.

### Offenes

- Stored Procedures für Acquire/Release/Upsert (Triggers/Leases) und Cleanup/Retention sind noch zu spezifizieren.
