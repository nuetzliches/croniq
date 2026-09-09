# Phase 0: Screen-Inventar für den Vue-Neubau

Stand: 2026-09-10. Ergebnis der Design-Phase aus
[ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md). Grundlage: Code-Analyse
des React-Baums, keine Annahmen.

**Stack (entschieden):** der `nuts-customer-portal`-Stack — Vue 3 + Nuxt UI 4 +
Pinia + `@tanstack/vue-query` + `ofetch` + vue-router. Damit ersetzt Nuxt UIs
Theming das projekteigene oklch-Token-System aus `ui/src/styles/`; das ist
Absicht und der Grund, warum das hier ein Neubau und kein Port ist.

**Zuschnitt (entschieden):** Screens werden neu gedacht und vereinfacht, nicht
1:1 übernommen.

---

## Korrektur an ADR-0004

ADR-0004s Scope-Guard sagt: *„Die Screen-Liste ist bei den heutigen zehn Routen
eingefroren."* Das war als Schutz gegen Scope-**Wachstum** gemeint, ist aber zu
wörtlich formuliert — Zusammenlegen ist das Gegenteil davon.

Präzisierung: eingefroren ist die Menge der **Fähigkeiten**, nicht die Menge
der Routen. Der Zuschnitt ist offen; jede heute erreichbare Aktion und jede
heute sichtbare Information muss im neuen Zuschnitt einen Ort haben. Die Liste
unter *Was nicht verloren gehen darf* ist die verbindliche Fassung davon.

---

## Befund: eine Sache wird vierfach gebaut

`useExecutions` läuft auf **vier von zehn Screens**:

| Ort | Was | Belege |
|---|---|---|
| `JobsPage` → Tab *Overview* | Executions-Tabelle, 12 Zeilen, 5 Spalten | `JobsPage.tsx:966–997` |
| `JobsPage` → Tab *Executions* | dieselbe Tabelle, alle Zeilen, + `attempt`, `error` | `JobsPage.tsx:1181–1218` |
| `ExecutionsPage` | dieselbe Liste, global, mit Filtern | `?job_key=` |
| `RunnersPage` → Detail | dieselbe Liste, `runner_id`-gefiltert, `limit: 50` | `RunnersPage.tsx:195` |

Die beiden Job-Tabs rendern denselben Query mit identischem Zellen-Markup —
`ExecutionLink`, `StatusPill`, `RunnerLink`, `formatRelative`, `durationFmt` —
und unterscheiden sich in zwei Spalten und einem `slice(0, 12)`. Der
*Executions*-Tab verlinkt am Kopf zusätzlich nach `/executions?job_key=…`, also
auf eine dritte Darstellung derselben Daten.

Dasselbe Muster, kleiner:

- `useAuditEvents` — in `JobsPage` (Tab *Audit*) und in `settings/AuditTab`.
- `useAlertDeliveries` — in `JobsPage` (Tab *Alerts*) und in `AlertsPage`.

Das ist kein Zufall, sondern ein Muster: **eine Detailseite baut eine globale
Seite nach, gefiltert.** Drei der sechs Job-Tabs existieren nur deshalb.

---

## Vorschlag: ein Ort pro Konzept

Leitregel — *eine Liste pro Konzept; Detailseiten verlinken gefiltert hinein,
statt sie nachzubauen.*

### 1. Runs: die eine Ausführungsansicht

`/executions` wird die einzige Stelle, an der Ausführungen als Liste
erscheinen. Filter: Job, Runner, Status, Zeitraum — alle in der URL, wie heute
schon für `state` und `job_key`.

Damit entfallen: der Overview-Tab-Ausschnitt, der Executions-Tab und der
Executions-Block im Runner-Detail. Die Job-Seite zeigt stattdessen eine
Statuszeile („letzte 5 Läufe", Sparkline) und verlinkt in die gefilterte Liste.

**Route bleibt `/executions`.** Ein Rename auf `/runs` wäre hübscher und bricht
Bookmarks und die URL-Verträge, die `ui/e2e/url-state.spec.ts` prüft — ohne
Gegenwert.

### 2. Job-Detail: von sechs Tabs auf zwei

| heute | künftig |
|---|---|
| Overview | **Übersicht** — Definition, Policy, Status, letzte Läufe als Zusammenfassung |
| Executions | entfällt → Link in die Runs-Liste |
| Schedule | in *Übersicht* integriert (Trigger sind Teil der Definition) |
| DSL | **DSL** — bleibt eigen, ist eine andere Repräsentation desselben |
| Alerts | entfällt → Link in die Alerts-Liste, `job_key`-gefiltert |
| Audit | entfällt → Link in die Audit-Liste, entity-gefiltert |

### 3. Audit: ein Ort

Heute in Settings *und* als Job-Tab. Künftig eine Ansicht mit Entity-Filter,
aus beiden Kontexten verlinkt. Ob sie unter Settings bleibt oder eigenständig
wird, hängt an der Navigationsfrage unten.

### 4. Alerts: Konfiguration und Zustellungen trennen

`AlertsPage` mischt heute beides — Regel-/Kanal-Konfiguration mit Overrides
(Snooze, Throttle, Disable) und die Zustellhistorie. Das sind zwei Dinge: eine
Einstellung und ein Protokoll. Vorschlag: Konfiguration zu den übrigen
Einstellungen, Zustellungen als filterbare Liste neben den Runs.

---

## Was nicht verloren gehen darf

Die verbindliche Fassung des Scope-Guards. Jede Zeile ist heute erreichbar und
braucht im neuen Zuschnitt einen Ort — nicht notwendig denselben.

**Jobs:** anlegen, bearbeiten, löschen, aktivieren/deaktivieren, manuell
triggern, adoptieren/freigeben (DSL-verwaltet vs. API-verwaltet), Tags,
Dead-Letter-Policy, Timeout, Retries, Forecast (nächste Feuerzeitpunkte),
Job-Statistiken, Schedule-Verwaltung inkl. Kalenderbindung, DSL-Ansicht.

**Runs:** Liste mit Filtern (Job, Status, Runner), Detail mit Logs, Abbrechen,
Attempt und Fehlertext, Verlinkung zu Job und Runner.

**Dead Letters:** Liste, Detail, Replay (inkl. Stale-Guard), Einzel- und
Massenlöschung, Retention/Ablauf, Operator-Hinweis.

**Runners:** Live-Liste über SSE, Tags, Filter, Detail mit zugehörigen Läufen,
Entfernen.

**Kalender:** CRUD, Regel-Builder, Adoption.

**Alerts:** Regel- und Kanalkonfiguration, Overrides (Snooze, Throttle,
Disable, Clear), Zustellhistorie.

**Konsole:** Live-Tail des Server-Tracings, Level-Filter, Suche, Pause,
Kopieren, NDJSON-Export. Admin-only.

**Settings:** Profil inkl. TOTP-Enrolment und PATs, Benutzer und Einladungen,
API-Clients und Token, Audit-Log.

**Querschnitt:** Login inkl. MFA und OIDC, Theme-Umschaltung, Sidebar-Zustand,
Command-Palette, Wartungsmodus-Banner, Dead-Letter-Zähler in der Topbar.

---

## Offene Fragen

Vier Stück, alle mit echten Alternativen.

1. **Dead Letters — eigener Screen oder Status in der Runs-Liste?**
   Datenmodell: eigene Tabelle, aber jede Zeile trägt `execution_id`, ist also
   an einen Lauf gebunden. *Als Status-Filter* wäre es ein Screen weniger und
   der Weg vom fehlgeschlagenen Lauf zum Dead Letter wäre ein Filterwechsel
   statt eines Seitenwechsels. *Als eigener Screen* bleibt sichtbar, dass hier
   etwas auf eine Handlung wartet — plus Retention und Massenlöschung sind
   eigene Semantik, die in einer Runs-Liste fremd wirkt.

2. **Kalender — Hauptnavigation oder Einstellungen?**
   Sie sind Konfiguration, keine Betriebsansicht, und werden selten angefasst.
   Dagegen: sie hängen fachlich an Jobs, nicht an der Serververwaltung.

3. **Konsole — bleibt sie ein eigener Screen?**
   Empfehlung ja: sie tailt den Tracing-Feed des Servers, nicht die Logs eines
   Laufs. Das ist eine andere Sache als das Log im Run-Detail, auch wenn beides
   „Logs" heißt.

4. **Dashboard — was bleibt darauf?**
   Heute Health, KPIs, Durchsatz, Failure-Heatmap, letzte Läufe, Dead-Letter-
   Zahl. Wenn die Runs-Liste die eine Ausführungsansicht wird, ist der
   „letzte Läufe"-Block dort das vierte Duplikat. Als Einstiegsseite ist ein
   Auszug aber genau richtig. Die Frage ist, ob das Dashboard Zahlen zeigt und
   verlinkt (schmal) oder Listen einbettet (wie heute).

---

## Reihenfolge

Erst 1 und 4 beantworten — sie entscheiden, wie viele Screens es überhaupt
gibt. 2 und 3 sind Navigationsdetails und können mit dem ersten Aufschlag der
Shell fallen.
