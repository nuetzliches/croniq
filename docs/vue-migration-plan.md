# Migrationsplan: UI von React auf Vue 3 (Tailwind bleibt)

Stand: 2026-06-11. Basis: Vollinventur der Codebasis (55 TSX-Dateien, ~9.900 LOC TSX,
2.232 LOC CSS, 64 Query/Mutation-Hooks, 2 handgebaute SSE-Clients), web-verifiziertes
Bibliotheks-Mapping und Bewertung von drei Migrationsstrategien.

## Ausgangslage — was die Migration leichter macht als gedacht

- **Tailwind CSS v4 ist bereits im Einsatz** (eigenes `@theme`-Token-System in
  `ui/src/index.css`, Dark Mode). "Umstellung auf Tailwind" entfällt — die Migration
  ist ein reiner Framework-Wechsel React → Vue. `tokens.css`, `shell.css`, `login.css`
  werden 1:1 übernommen.
- **Der Rust-Server ist framework-agnostisch**: `croniq-server` served nur ein
  `dist/`-Verzeichnis (`--ui-dir`, ServeDir + index.html-Fallback, `main.rs:764-769`).
  Der Cutover ist buchstäblich ein COPY-Pfad-Wechsel in der Dockerfile.
- **Die WASM-Bridge** (`croniq-config-wasm`, DSL-Parser) und ihr Build-Vertrag
  (`scripts/build-wasm.sh`, `predev`/`prebuild`-Hooks, Zielpfad `src/lib/wasm/`)
  sind framework-frei und bleiben unverändert.
- **Framework-freier Kern**: `api/types.ts` (416 LOC), `api/client.ts` (~95 %),
  `lib/croniq-dsl.ts`, `lib/env.ts`, `lib/utils.ts` portieren per Copy.

Erschwerend: Es gibt **null UI-Tests** (CI prüft nur lint + tsc + build + WASM-Budget),
und das Repo liefert ~6 UI-Commits/Woche — Feature-Drift während der Migration ist das
dominante Risiko, nicht die Technik.

## Bibliotheks-Mapping (Stand Mitte 2026, web-verifiziert)

| React (heute) | Vue (Ziel) | Anmerkung |
|---|---|---|
| `@radix-ui/react-*` (Dialog, Tooltip; Switch/DropdownMenu ungenutzt) | **Reka UI v2.8** | Radix-Nachfolger für Vue, Anatomie + `data-state`-Attribute nahezu 1:1 — Tailwind-Selektoren wie `data-[state=open]:` portieren unverändert |
| `@tanstack/react-query` v5 | **`@tanstack/vue-query` v5** | gleiche v5-API; Parameter müssen als `computed`/`MaybeRefOrGetter` übergeben werden (wichtigste Fehlerquelle, s. Risiken) |
| `react-router` v7 | **vue-router v5** | Routen-Array statt JSX-Routes, Guards statt `<ProtectedRoute>` |
| `zustand` v5 | **Pinia v3** | Setup-Stores; `pinia-plugin-persistedstate` für Sidebar |
| `react-hook-form` v7 | **plain `reactive()` + v-model**, vee-validate nur bei Bedarf | Formulare sind 3–5 Felder; RHF-Spezialfälle (TimezoneInput-Prototype-Hack, Hidden-Field-Bridge) entfallen durch v-model ersatzlos |
| `recharts` v3 | **entfernt** | ist eine tote Dependency — Charts sind handgebaute SVG-Primitives (Sparkline, Donut, HeatCell), die 1:1 portieren |
| `lucide-react` | **`lucide-vue-next`** | gleiche Icon-Namen, Suchen/Ersetzen |
| `clsx`, `tailwind-merge`, `qrcode` | unverändert | framework-frei |
| `@vitejs/plugin-react` | **`@vitejs/plugin-vue`** | läuft auf Vite 8/rolldown; `codeSplitting.groups` auf Vue-Pakete umschreiben |
| `tsc --noEmit` | **`vue-tsc --noEmit`** | vue-tsc 3.3.x vs. TypeScript ~6.0.2 als Tag-1-Spike verifizieren; Fallback: TS 5.9 nur für den Check pinnen |

**Komponentenstrategie: Headless (Reka UI) + eigene Tailwind-Tokens behalten.**
Keine Komplett-Library (Nuxt UI v4, PrimeVue) — die brächte ein eigenes
Theming-/Token-System mit, das dauerhaft gegen die vorhandenen Tokens gepflegt werden
müsste; die App nutzt real nur zwei Radix-Primitives. shadcn-vue punktuell als
Scaffold-Referenz (generiert Reka-UI-Komponenten mit Tailwind-v4-Support ins eigene Repo).

## Empfohlene Strategie

**Big-Bang-Port in einem Parallel-Verzeichnis `ui-vue/`**, angereichert um die
günstigen Foundation-Elemente des "Foundation-First"-Ansatzes. Drei Strategien wurden
durchgeplant und gegeneinander bewertet (Big-Bang parallel, Strangler/inkrementell,
Foundation-First mit hartem Cutover); Big-Bang gewinnt für genau dieses Setup:

- Ein Entwickler, produktives `ui/` bleibt durchgehend releasebar, kein
  Dual-Framework-Betrieb (Strangler kostet 10–15 PT reine Infrastruktur: doppelte
  Shell, doppelte hooks.ts-Pflege, pfadbasiertes Splitting im Rust-Server, Full-Reload
  an jeder Welt-Grenze — bei einer dicht verlinkten 10-Seiten-SPA der falsche Ansatz).
- Der Cutover ist ein einzelner, trivial rollback-barer Commit (Dockerfile-COPY-Pfad).
- ~10k LOC mit zentralem Daten-Layer (eine `hooks.ts`, drei kleine Stores) liegen im
  Sweet Spot für einen kompakten Komplett-Port.

**Aufwand realistisch: ~40–48 Personentage** (erfahrener Entwickler mit
KI-Unterstützung, ~7–9 Kalenderwochen). KI beschleunigt die mechanischen Ports;
Flaschenhals bleibt die manuelle Verifikation — deshalb ist Phase 0 (Testnetz) Pflicht.

## Phasenplan

### Phase 0 — Sicherheitsnetz + Foundation im bestehenden `ui/` (5–7 PT)

Alles hier sind normale, sofort mergebare PRs, die auch der React-App nutzen —
bei Abbruch der Migration kein Totalverlust.

1. **Playwright-Smoke-Suite** (~12–15 Szenarien) gegen die laufende React-UI:
   Login (Passwort-Flow), Dashboard-KPIs, `/jobs/:jobKey`-Deep-Link, Executions-Filter
   `?state&job_key`, Dead-Letter-Replay, Runners-SSE-Indikator, Console-Tail,
   Settings-`?tab`, Theme-Toggle, Sidebar-Persistenz, **Session-Kontinuität**
   (bestehende `croniq_token`/`_refresh`/`_theme`/`_sidebar`-Werte überleben).
   Diese Suite ist das Abnahmekriterium des Cutovers.
2. **Framework-freie Core-Extraktion** (~3–4 PT, schrumpft die Portierungsfläche):
   - `lib/auth-token.ts`: Token-Holder ohne Zustand-Import — entkoppelt `client.ts`
     vom Store; Pinia dockt später identisch an.
   - `lib/sse.ts` (`createSseStream`): EIN getesteter SSE-Kern (fetch-Streaming,
     Backoff, Abort) für `useRunnersSSE` und ConsolePage — wird importiert statt
     zweimal portiert.
   - `lib/api-error.ts` (`parseApiError`): einzige Fehlerparsing-Stelle für
     client.ts/toast.ts/LoginPage.
   - Theme-FOUC-Bootstrap als Inline-Script nach `index.html` — überlebt den
     Framework-Wechsel garantiert.
   - Vitest-Setup mit ~25–35 Unit-Tests für diesen Kern (Bearer/401/204,
     SSE-Chunk-Grenzen, Fehler-Envelope, theme-resolve).
3. **Entrümpeln**: tote Deps raus (`recharts`, `@radix-ui/react-switch`,
   `@radix-ui/react-dropdown-menu` + recharts-Chunk-Regel); `DeliveriesList` aus
   `AlertsPage.tsx` nach `components/` extrahieren (löst Cross-Page-Import aus JobsPage).
4. **CSS-Kollisionen im Live-Tree fixen** (hier gibt es die laufende App als Orakel):
   `components.css` definiert `.grid`, `.gap-4/6/8/10/14`, `.grow`, `.cols-*` als
   unlayered CSS und überschreibt damit Tailwind-Utilities (z. B. bekommt
   `grid grid-cols-7 gap-1` im ScheduleBuilder real `gap:14px`). Umbenennen oder in
   `@layer` legen; totes CSS (~150–200 LOC) streichen.
5. **PORTING-NOTES.md**: Checkliste aller stillen Verträge — dynamische Klassennamen
   (`lvl-${lvl}`, `kpi-delta ${direction}`, `audit-${kind}`), data-Attribute
   (`.app[data-sidebar]`, `.sidebar[data-collapsed]`, `aria-expanded`),
   localStorage-Keys, URL-Verträge, Query-Key-Eigenheiten (Singular/Plural
   `dead-letter`/`dead-letters`), implizite Erstauswahl der JobsPage,
   `?tab`-Whitelist, 409/400-Fehlerparsing. Wird verbindliche Abnahme-Checkliste
   pro Seite.
6. **Toolchain-Spikes (Go/No-Go vor Phase 1)**: `@vitejs/plugin-vue` auf
   rolldown-Vite 8; `vue-tsc` 3.3.x gegen TS ~6.0.2. Fallbacks: TS 5.9 für den
   Check pinnen, notfalls manuelles Chunking aufgeben.

### Phase 1 — `ui-vue/` Scaffold + Daten-Layer (5–6 PT)

- Vite-8-Projekt mit `@vitejs/plugin-vue`, Alias `@→src`, identischem Dev-Proxy
  (`/v1`, `/health`, `/metrics`), WASM-Hooks unverändert verdrahtet.
- 1:1-Kopien: `types.ts`, `utils.ts`, `env.ts`, `croniq-dsl.ts`, `tokens.css`,
  `shell.css`, `login.css`, bereinigte `components.css`; `client.ts` mit
  `auth-token.ts` aus Phase 0.
- Dark-Mode auf EIN System konsolidieren: `@theme`-Hex-Tokens auf die
  oklch-Variablen mappen, nur noch `[data-theme]` statt zusätzlicher `.dark`-Klasse.
- 3 Pinia-Stores (auth/sidebar/toast) mit **identischen localStorage-Keys** —
  bestehende Sessions überleben den Cutover.
- **`hooks.ts` (772 LOC) in EINEM fokussierten PR** auf vue-query: alle 26 Queries +
  38 Mutationen, Query-Keys byte-identisch, **jeder Parameter als
  `computed`/`MaybeRefOrGetter`** (harte Konvention, Review-Checkliste pro Hook);
  globaler `MutationCache.onError`-Replikat (`meta.action`, 'Unauthorized'-Silencing).

### Phase 2 — Design-System: Primitives, ui-Komponenten, Dialoge, Builder (7–9 PT)

- 13 triviale Primitives als SFCs (ReactNode-Props → Slots, `useId` für SVG-mask-ids);
  Duplikate konsolidieren (EmptyState 2→1, CopyBtn 2→1).
- ui-Basis: Button (forwardRef entfällt), Badge, Card, RelativeTime (VueUse `useNow`),
  DataTable mit **Scoped Slots** statt cell-Render-Props (`<script setup generic="T">`).
- Reka-UI-Basis-Dialog + **`useConfirm` als Composable mit global gemountetem
  `ConfirmHost`** (Teleport) — ersetzt das React-Muster "Hook liefert JSX" zentral
  für alle 8+ Aufruferseiten.
- **TimezoneInput (281 LOC, höchstes Einzelrisiko)**: der
  HTMLInputElement-Prototype-Setter-Hack für react-hook-form entfällt zugunsten
  v-model; `createPortal` → Teleport mit Scroll/Resize-Nachführung; Combobox-Verhalten
  im Dialog-Kontext manuell testen.
- ScheduleBuilder/CalendarRuleBuilder: `croniq-dsl.ts` bleibt 1:1, aber jedes
  `useEffect`-cancelled-Flag wird diszipliniert `watch` + `onCleanup` —
  sonst WASM-Races bei schnellen Eingaben; Callbacks werden emits/v-model.
- Dialoge (EditJob/NewJob/Schedule): plain `reactive()` + v-model
  (Entscheidung im ersten Dialog treffen und durchziehen).

### Phase 3 — Shell, Routing, Auth, LoginPage (4–6 PT)

- vue-router-Routentabelle 1:1 aus `App.tsx` (history mode, Lazy-Imports pro Route).
- **Auth doppelt absichern**: `beforeEach`-Guard (`meta.requiresAuth`) PLUS
  `watch` auf `isAuthenticated` → `router.replace('/login')` — Vue-Guards feuern nur
  bei Navigation; ohne den Watch bleibt die Seite nach 401-Logout stehen
  (der reaktivste Unterschied zum React-`<Navigate>`-Muster).
- Shell-SFCs mit **exakter DOM-/Attribut-Parität** (`.app[data-sidebar=collapsed]`,
  `.sidebar[data-collapsed]`, `.user-pill[aria-expanded]`) — `shell.css` selektiert
  darauf und bricht sonst still. Chunk-Loading-Spinner über Router-Loading-State
  (Vue-Suspense ist dafür nicht das Werkzeug).
- LoginPage (1.091 LOC): Auth-Flow inkl. TOTP-Enrolment sorgfältig 1:1; die
  Demo-Konsolen-State-Machine und Verb-Rotation als eigenständige Timer-Composables
  neu schreiben statt 1:1 übersetzen. TOTP-Enrolment als geteilte Komponente
  (LoginPage + ProfileTab).

### Phase 4 — Seiten-Port aufsteigend nach Komplexität (9–13 PT)

Beide UIs parallel laufen lassen (React :5173, Vue :5174, gleiche API), jede Seite
einzeln side-by-side abnehmen, PORTING-NOTES als Gate, Playwright-Specs pro Seite
grün ziehen:

1. **DashboardPage** (368 LOC, rein deklarativ) — validiert Daten-Layer + Primitives
2. **ExecutionsPage** — etabliert das URL-Sync-Muster (`route.query` + `router.replace`)
3. **DeadLettersPage** — Master-Detail, 404-Schlucken, Replay-Banner
4. **RunnersPage** — erste SSE-Integration unter Störbedingungen (Server-Kill → Backoff)
5. **CalendarsPage** — CalendarRuleBuilder; RHF-Hidden-Field-Bridge entfällt
6. **SettingsPage + Tabs** — `?tab`-Whitelist, Rollen-Gate wird `v-if`
7. **AlertsPage**
8. **JobsPage** (1.245 LOC, 16 Hooks, der dickste Brocken) — implizite Erstauswahl
   ohne `:jobKey` erhalten, KpiRow-Countdown als `useNow`-Composable
9. **ConsolePage** — Event-Puffer als `shallowRef` + manuelles `triggerRef`,
   pending/paused als nicht-reaktive Variablen; Last-Test mit 2.000 Events
   (naive `ref([])`-Portierung macht jeden SSE-Event zum Deep-Reactivity-Trigger)

### Phase 5 — CI/Docker-Cutover, Abnahme, Rückbau (3–4 PT)

- CI-Job **`UI (build + typecheck)` NICHT umbenennen** (Required Status Check,
  Incident #98) — Steps intern tauschen (`tsc` → `vue-tsc`); Playwright als neuer,
  zunächst nicht-required Job.
- **Cutover = ein PR**: Dockerfile Stage 2 + Runtime-COPY auf `ui-vue/`;
  WASM-COPY-Pfad mitziehen. Rollback = Revert bzw. voriges Docker-Image-Tag.
- Abnahme: Playwright-Suite grün gegen das Docker-Image; Parity-Review gegen
  PORTING-NOTES; Session-Kontinuität (eingeloggt bleiben über den Cutover);
  FOUC-Test Light/Dark/Auto.
- Nach 1–2 Wochen Beobachtung (besonders Console/Runners-SSE unter Last):
  Rückbau-PR — `ui/` löschen, `ui-vue/` → `ui/`, React-Deps entfernen.

## Drift-Regeln während der Parallelphase

Bei ~6 UI-Commits/Woche ist Feature-Drift das Hauptrisiko (Budget: 3–6 PT einplanen,
falls kein Freeze möglich):

- UI-Feature-Freeze für die heiße Phase (4–6 Wochen) anstreben und terminieren.
- Sobald eine Datei portiert ist: **Fixes zuerst im Vue-Tree, Cherry-Pick nach React**
  — nie umgekehrt.
- Frozen-Liste pro portierter Datei führen (PR-Checkliste: "betrifft Shell/hooks?
  → beide Trees anfassen").

## Top-Risiken

| Risiko | Gegenmaßnahme |
|---|---|
| vue-query-Reaktivitätsfalle: parametrisierte Hooks frieren bei Filter-/Routenwechsel ein, wenn Keys nicht als `computed` übergeben werden | hooks.ts in einem PR mit harter Konvention; Playwright-Flows für Filterwechsel |
| Feature-Drift gegen aktives `ui/` | Drift-Regeln oben; Foundation vorziehen, Freeze nur für Phasen 1–5 |
| `shell.css`/`components.css` selektieren auf data-Attribute und dynamische Klassen — bricht still | PORTING-NOTES-Checkliste als Abnahme-Gate pro Seite |
| ConsolePage-Performance (2.000-Event-Puffer) | `shallowRef` + nicht-reaktive Puffer, Last-Test vor Abnahme |
| Toolchain-Unbekannte (plugin-vue/rolldown, vue-tsc/TS6) | Tag-1-Spikes mit definierten Fallbacks, bevor Port-Aufwand fließt |
| Required Status Check blockiert Merges bei Job-Umbenennung | Jobnamen einfrieren, nur Steps tauschen |
| Reaktiver 401-Redirect geht verloren (Guards feuern nur bei Navigation) | `isAuthenticated`-watch + dedizierter E2E-Test |

## Vor dem Start zu entscheiden

1. **Warum Vue?** ~40–48 PT ohne sichtbaren Endnutzer-Nutzen — die strategische
   Begründung (Team-Skills, Wartbarkeit, Ökosystem) sollte schriftlich stehen,
   bevor Phase 0 startet.
2. **Feature-Freeze**: Ist ein 4–6-Wochen-Fenster durchsetzbar, oder wird mit
   Drift-Budget (3–6 PT) gearbeitet?
3. **Parität vs. Konsolidierung**: Empfehlung — nur CSS-Kollisionen und Dark-Mode
   vorab im React-Tree konsolidieren; alle weiteren Redesigns (DataTable-API,
   useConfirm) sind Teil des Ports; rein optionale Vereinheitlichungen
   (Query-Key-Namen) erst NACH dem Cutover.
4. **Abnahmekriterium**: Playwright-Smoke + manuelle Side-by-Side-Abnahme pro Seite
   (Empfehlung) — kein vollflächiger Screenshot-Diff-Apparat (flaky durch
   Animationen/Portale/Fonts; für ein 1-Entwickler-Projekt überdimensioniert).
5. **Rollback-Schwelle**: Wie lange bleibt der React-Tree im Repo (1–2 Releases),
   und welche Fehlerklasse löst Rollback statt Forward-Fix aus?
6. **Playwright als Required Check** nach der Migration — das dauerhafte Schließen
   der Testlücke ist der wertvollste Nebeneffekt des Projekts und sollte nicht
   versanden.
