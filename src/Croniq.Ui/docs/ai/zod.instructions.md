# Zod – Hinweise & Konventionen (Croniq UI)

Status: **Zod v4** (siehe `package.json`).

Diese Datei ist ein kurzer Leitfaden für Zod-Nutzung im UI-Code und für generierte OpenAPI→Zod Schemas.

## 1) „Obsolete“ APIs – Kurzfazit

- `.passthrough()` ist **nicht deprecated** in Zod v4. Es ist ein bewusstes Objekt-„unknown keys“ Verhalten.
- `z.string().datetime()` (und `date`/`time`/`duration`) ist in Zod v4 **deprecated** → nutze stattdessen `z.iso.datetime()` (bzw. `z.iso.date()`/`z.iso.time()`/`z.iso.duration()`).
- Problematisch wären eher alte/entfernte Aliase wie `.nonstrict()` (falls sie irgendwo auftauchen sollten). Aktuell ist das im Repo nicht der Fall.

## 2) Unknown-Keys-Policy: `strict` vs. Default vs. `passthrough`

Zod-Objekte haben ein konfigurierbares Verhalten für unbekannte Felder:

- **Default (implicit)**: unbekannte Keys werden **gestrippt** (nicht in das Ergebnis übernommen).
- **`.strict()`**: unbekannte Keys führen zu einem **Parse-Fehler**.
- **`.passthrough()`**: unbekannte Keys werden **beibehalten**.

Empfehlung im UI:

- **Request-Payloads, die wir selbst bauen** (z.B. Form → API): eher **`.strict()`** oder Default – damit wir Fehler früh sehen.
- **Responses von Croniq.Api** (die sich erweitern können): häufig **`.passthrough()`** oder Default – damit additive Backend-Änderungen die UI nicht unnötig brechen.

Hinweis: Bei Responses kann `.passthrough()` zusätzlich helfen, Debug-/Telemetry-Rohdaten zu behalten (z.B. `raw`).

## 3) Generated OpenAPI → Zod (projects/api-schema/generated)

- Die Generator-Ausgabe unter `projects/api-schema/generated/` wird überschrieben und nicht manuell editiert.
- Aktuell werden dort **keine** `.passthrough()`/`.nonstrict()`-Aufrufe generiert; die Objekte nutzen das Zod-Default-Verhalten.
- Wenn ihr die Unknown-Keys-Policy der generierten Schemas ändern wollt, dann ist der Ort dafür:
  - Templates unter `tools/templates/`
  - oder eine bewusste „Wrapper“-Schicht in `projects/api-schema/src` (manuelle Overrides/Re-Exports)

## 4) Beispiel: defensive Response-Parsing

Pattern (vereinfacht):

- response ist manchmal String oder Objekt
- Token-Feldnamen variieren (`accessToken`/`token`/`value`)
- Backend kann neue Felder hinzufügen

Dann ist `.passthrough()` sinnvoll, solange wir die Felder, die wir benötigen, sauber validieren (z.B. via `.superRefine`).

## 5) Wenn etwas „komisch“ aussieht

Checkliste bei Verdacht auf inkonsistente/alte Zod-Ausgabe:

1. Zod-Version prüfen (`zod` in `package.json`).
2. Generator-Version prüfen (`openapi-zod-client` in `package.json`).
3. In `projects/api-schema/generated/` nach `.nonstrict(` / `.passthrough(` / `.strict(` suchen.
4. Falls eine API in Zod v4 wirklich deprecated/entfernt ist: Fix entweder in Templates oder als manuelle Override-Schemas.
