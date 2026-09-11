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

**Querschnitt:** Login inkl. MFA und OIDC **sowie Passwort-Wiederherstellung
(Anforderung *und* Zielseite) und Einladungsannahme** — diese drei fehlten in
der ersten Fassung dieser Liste, und die beiden Zielseiten fehlten sogar im
ausgelieferten Dashboard: der Server verschickt `…/password-reset/confirm` und
`…/invitations/accept`, und keiner der Bäume bediente die Routen (Durchgang 12).
Theme-Umschaltung, Sidebar-Zustand,
Command-Palette, Dead-Letter-Zähler in der Topbar, und der Wartungsmodus in
**beiden** Hälften — der Banner für alle, die Steuerung (manuell an/aus,
geplantes Fenster, Notiz) admin-only in der Topbar. Die erste Fassung dieser
Liste nannte nur den Banner; die Steuerung fehlte und wäre damit still unter den
Tisch gefallen.

---

## Entschieden (2026-09-10)

Alle vier offenen Fragen beantwortet.

**Dead Letters bleiben ein eigener Screen.** Technisch wäre der Filter möglich
— `ExecutionState::Dead` existiert und `/v1/executions?state=dead` funktioniert
heute schon. Ausschlaggebend war die Rolle, nicht die Technik: Dead Letters
sind die einzige Fläche im Produkt, die eine To-do-Liste ist. Eine Arbeitsliste
in eine Browsing-Liste zu mischen macht die Arbeit unsichtbar — man müsste den
Filter erst setzen, um zu sehen, dass etwas ansteht. Das Topbar-Badge behält
sein Ziel; das Run-Detail eines toten Laufs verlinkt hierher.

**Dashboard wird Statusboard plus Fehler-Auszug.** Zahlen, Durchsatz, Heatmap,
dazu ein schmaler Auszug ausschließlich der letzten Fehlschläge mit Link in die
gefilterte Runs-Liste. Kein allgemeiner „letzte Läufe"-Block — der wäre das
vierte Rendering derselben Tabelle. Der Auszug ist bewusst eine andere
Darstellung (kompakt, nur Fehler), kein verkleinerter Klon.

**Kalender bleiben in der Hauptnavigation.** Sie hängen fachlich an Jobs, nicht
an der Serververwaltung, und der Regel-Builder ist zu groß für einen
Settings-Tab.

**Konsole bleibt ein eigener Screen**, rollenbasiert eingeblendet wie heute.
Server-Tracing und Lauf-Logs heißen beide „Logs" und sind verschiedene Dinge;
zusammenzulegen erzeugt genau die Verwechslung, die beim Debuggen stört.

## Resultierender Zuschnitt

```
Betrieb    Dashboard   Runs   Runner   Dead Letters
Konfig     Jobs        Kalender   Alerts
System     Konsole (admin)   Einstellungen
```

| Route | Inhalt |
|---|---|
| `/` | Statusboard + Fehler-Auszug |
| `/executions`, `/executions/:id` | die eine Lauf-Liste; Filter job/runner/state/zeit in der URL |
| `/runners` | Live-Liste + Detail **ohne** Executions-Block |
| `/dead-letters` | Arbeitsliste: Replay, Löschen, Massenaktion, Retention |
| `/jobs`, `/jobs/:key` | Master/Detail, **zwei** Tabs (Übersicht inkl. Schedule, DSL) |
| `/calendars` | CRUD + Regel-Builder |
| `/alerts` | Regeln, Kanäle, Overrides |
| `/console` | Live-Tail, admin-only |
| `/settings` | Profil, Benutzer, API-Clients, Audit |
| `/login` | inkl. MFA und OIDC |

**Es sind weiterhin zehn Routen.** Der Gewinn liegt nicht in der Anzahl der
Screens, sondern im entfallenen Inhalt: Job-Detail von sechs auf zwei Tabs,
Executions-Block im Runner-Detail weg, Audit und Alert-Zustellungen mit je
einem Ort statt zwei, Dashboard ohne allgemeine Lauf-Liste. Vier Renderings der
Executions-Tabelle werden eins.

## Entschieden (2026-09-11): Alerts

Die letzte offene Frage — bleibt die Zustellhistorie beim Regelwerk oder wird
sie eine filterbare Liste neben den Runs? — ist beim Bau des Screens
beantwortet worden, und zwar mit **keiner der beiden Fassungen, wie gestellt**.

Der Einwand aus der Bestandsaufnahme war, dass die React-Seite zwei Dinge
*vermischt*: Konfiguration und Protokoll, flach auf einer Seite. Die Antwort
auf Vermischung ist aber nicht Trennung auf verschiedene Screens, sondern
Struktur. Die Fragen laufen quer über die Grenze: man schnoozt eine Regel
*wegen* dem, was das Protokoll zeigt, und man liest das Protokoll, um
herauszufinden, welche Regel gepiept hat. Zwei Screens setzen einen
Navigationsschritt zwischen Frage und Antwort.

Also **ein Screen, drei Ansichten**, jede eine saubere Liste einer Sache, alle
drei adressierbar:

| Route | Inhalt |
|---|---|
| `/alerts` | die Regeln, mit ihren Overrides |
| `/alerts/rules/:name` | eine Regel: Konfiguration, Override, ihre Zustellungen |
| `/alerts/channels` | wohin zugestellt wird, und welche Regel welchen Kanal nutzt |
| `/alerts/deliveries` | was tatsächlich rausging, filterbar (Regel, Job, Status) |

**Zustellungen bleiben aus `/executions` heraus**, aus demselben Grund, aus dem
Dead Letters einen eigenen Screen behalten haben: ein Job-Lauf und eine
Alert-Zustellung sind verschiedene Dinge, die zufällig dieselbe Form haben. Sie
zusammenzulegen hieße, dass „200 Läufe" zweierlei bedeutet.

Zwei Dinge, die der Kanal-Blick sichtbar macht und die vorher stumm waren: ein
Kanal, den keine Regel nutzt (`unused`), und eine Regel, die einen Kanal nennt,
den es nicht gibt — der Compiler behält die Referenz wörtlich und warnt erst
zur Feuerzeit, die Regel sieht also konfiguriert aus und stellt nirgends zu.

## Reihenfolge für den Aufbau

Der Zuschnitt steht; als Nächstes das Gerüst. Reihenfolge nach Abhängigkeit,
nicht nach Größe:

1. **Scaffold + Daten-Layer** — Vite/Vue-Projekt, Nuxt UI 4, Pinia-Stores mit
   identischen localStorage-Keys, vue-query über `ofetch`, Dev-Proxy und die
   WASM-Hooks unverändert übernommen.
2. **Shell + Auth** — Navigation nach obigem Zuschnitt, Router-Guard *plus*
   Watch auf `isAuthenticated` (Vue-Guards feuern nur bei Navigation), Login
   inkl. MFA.
3. **Runs-Liste** — der Screen, den drei andere ersetzt. Zuerst, weil Job- und
   Runner-Detail auf ihn verlinken statt ihn nachzubauen.
4. **Dashboard, Runner, Dead Letters** — bauen auf denselben Listen-Bausteinen.
5. **Jobs** — der dickste Screen, profitiert am meisten von fertigen
   Bausteinen. ✓ (Durchgang 6 in `ui-visual-design.md`)
6. **Kalender** ✓ (Durchgang 7), **Alerts** ✓ (Durchgang 9),
   **Settings** ✓ (Durchgang 10), **Konsole** ✓ (Durchgang 11).

**Die Reihenfolge ist abgearbeitet.** Jede Route des Zuschnitts oben ist
gebaut; der `NotBuiltYet`-Platzhalter ist entfernt. Offen bleibt das
Abnahmekriterium selbst — siehe unten.

Abnahmekriterium pro Schritt bleibt die Playwright-Suite
([ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md), Scope-Guard 2). Sie
prüft Routen, Session, URL-Verträge und beide SSE-Flächen — nichts davon
Optik.
