# UI-Spike Brief — Croniq UI-Vollumbau (Spike-B)

**Branch:** `feat/auth-foundation` — Backend-Foundation komplett, jetzt UI ziehen.

## Voraussetzung (einmalig)

```bash
# Node 20.19+ oder 22.12+ — Vite 8 verlangt das
node -v                                      # muss ≥ 20.19 sein

cd /home/sebastian/dev/croniq/ui
npm ci                                       # ~30s
docker compose up -d                         # Backend auf :4000
npm run dev                                  # Vite HMR auf :5173 (proxied → :4000)
```

Health-Check vor Beginn: `curl http://localhost:4000/health` → `{"status":"ok",...}`.
Login-Test: `admin / admin` über das alte UI auf :4000.

## Stand

**Backend (alles fertig, deployed im Container):**
- 6 Auth-PRs A1-A6 + DSL-Add A5b: Multi-User mit Rollen, TOTP/2FA, PATs, OIDC, optional SMTP
- 3 Stats-PRs B1/B1b/B1c: Audit-Log + Recording, Job-Stats, Throughput, Failure-Heatmap, OpenAPI
- 16 Migrationen, ~30 neue REST-Endpoints, 539 Tests grün
- Vollständige TypeScript-Types liegen in `ui/src/api/types.ts`

**UI (heute):**
- LoginPage hat MFA-Step + OIDC-Button (PR-B2, c14d37c) — aber im Default-Flow unsichtbar
- Sonst unverändert: alte Sidebar/Topbar, alte Pages

**Das Design-Bundle aus Claude Design** liegt in `/tmp/croniq-design/croniq-ui/project/`:
- `app.jsx`, `shell.jsx`, `data.js`, `styles.css`
- `page-dashboard.jsx`, `page-jobs.jsx`, `page-executions.jsx`, `page-runners.jsx`,
  `page-insights.jsx`, `page-planner.jsx`, `page-secrets.jsx`, `page-misc.jsx`,
  `page-newjob.jsx`, `page-notifications.jsx`, `page-onboarding.jsx`
- `chats/chat1.md` — Designer-Chat-Transkript mit Begründungen

## Empfohlene Reihenfolge

Die Reihenfolge folgt dem Mantra "Tokens → Primitives → Shell → Pages". Pro Schritt einen
Commit, damit der diff überschaubar bleibt.

### B-PR-1 · Design-Tokens (~250 Zeilen CSS)
- `ui/src/styles/tokens.css`: oklch-Palette + Cards-Chrome-Variables aus
  `/tmp/croniq-design/croniq-ui/project/styles.css` portieren
- `ui/src/index.css` zieht `@import "./styles/tokens.css"` direkt nach `@import "tailwindcss"`
- `<html data-theme="dark"|"light">` als Hook
- Akzeptanzkriterium: alle bestehenden Pages laden und sehen nicht kaputt aus

### B-PR-2 · Primitives (~600 Zeilen TSX)
Pure presentation-Komponenten ohne State, ohne API-Bindung:

```
ui/src/components/primitives/
  StatusPill.tsx         status string → coloured chip
  Sparkline.tsx          number[] → inline svg
  Donut.tsx              {value, max} → svg ring
  RunBars.tsx            ('ok'|'warn'|'err')[] → row of bars
  EmptyState.tsx         {icon, title, desc, actions} → centred placeholder
  Toggle.tsx             ARIA-konformer Switch
  KPICard.tsx            {title, value, sub?, chart?, delta?} → dashboard tile
  CopyBtn.tsx            kopiert {value} → Clipboard
  Avatar.tsx             initials-circle
```

Alle ohne externe Deps (außer evtl. `react`). Vorbild: Inline-`React.createElement` im
Bundle, einfach zu portierende JSX.

### B-PR-3 · App-Shell (~800 Zeilen)
Cards-Chrome-Layout:

```
ui/src/layout/
  Sidebar.tsx           ersetzt /Layout.tsx Sidebar-Part — als Card
  Topbar.tsx            Brand + Crumbs + ⌘K-Trigger + Bell + UserMenu
  CommandPalette.tsx    ⌘K-Modal, fuzzy-Search über Jobs/Runners/Actions
  LogTail.tsx           ⌘J-Dock unten, resizable, Pause/Follow/Filter
  NotificationsPanel.tsx Bell-Popover (Unread/All-Tabs)
  UserMenu.tsx          Avatar-Dropdown unten links: Profile, Theme, Logout
```

Die existierenden Page-Komponenten bleiben — nur das Layout drumherum ist neu.
Akzeptanzkriterium: alle alten Pages erreichbar, Mobile <768 verhält sich
korrekt (Sidebar auf 60px collapsed).

### B-PR-4 · Login-Page Polish
Bereits funktional via PR-B2. Aufhübschen: Operator-Console-Vibe linkes Panel?
Lass die animierte Live-Console weg — Zero-User-Value. Stattdessen:
- segmented Tabs "Password / API Token / SSO" (statt zwei separate Forms)
- Brand-Mark + Tagline + 3 KPI-Cards-Teaser ist OK
- Recovery-via-SSH **nicht** — backend kann das gar nicht. Stattdessen
  "Forgot password?" Link → `/v1/auth/password-reset/request`

### B-PR-5 · Dashboard (~600 Zeilen)
Echte Daten aus den PR-B1-Endpoints:

```
KPIs:           GET /health + /v1/jobs (count) + /v1/executions/throughput?window=24h
Sparklines:     /v1/executions/throughput (rolling 24h ok-counts)
Failure-Heatmap: /v1/insights/failures?days=28
Activity-Feed:  /v1/audit?limit=10
Top-Failing:    aggregate /v1/jobs/{key}/stats für die top-5 jobs sorted by fail rate
Upcoming:       /v1/dashboard/forecast (existiert schon, eingesetzt mit anderem Style)
Runner-Fleet:   /v1/runners + Donut für inflight/slots
```

Neue Hooks in `ui/src/api/hooks.ts`:
- `useAuditEvents(filter)` — `GET /v1/audit`
- `useJobStats(jobKey, days)` — `GET /v1/jobs/{key}/stats`
- `useThroughput(window)` — `GET /v1/executions/throughput`
- `useFailureHeatmap(days)` — `GET /v1/insights/failures`

### B-PR-6 · Jobs-Master-Detail-Split
Replace `JobsPage` + `JobDetailPage` mit Single-Page-Split:

```
ui/src/pages/JobsPage.tsx
  .split
    .master (Job-Liste mit Search + Tag-Filter)
    .detail (Tabs: Overview / Runs / Schedule / DSL / Alerts / Audit)
```

Hover-Peek auf Job-Rows zeigt Mini-Card mit success-rate + last fire.
Master-Liste hat `sticky` Header, eigene Scroll-Container. Detail-Tabs
sind separate Sub-Components, alle in `ui/src/pages/jobs/`.

### B-PR-7 · Settings-Tabs ausbauen
Neue Tabs in `ui/src/pages/SettingsPage.tsx`:

```
Tabs:    API Clients | API Keys | Webhooks | Audit | Org | Profile
         + Users (neu — admin sieht Liste, Invite, Edit-Dialog)
         + Profile bekommt TOTP-Setup + PAT-Management
```

Backend-Endpoints sind alle da (`/v1/users`, `/v1/invitations`, `/v1/users/me/tokens`,
`/v1/users/me/totp/*`).

### B-PR-8 (optional) · Executions-Throughput-Bars + Log-Viewer-Tabs
Ersetzt die `ExecutionsPage.tsx` mit Stacked-Bars + Log-Filter-Bar +
stdout/stderr/env/raw-Tabs. Niedrigere Priorität — die alte Page funktioniert.

### B-PR-9 (optional) · Insights-Page (Reliability + Latency only)
**Kein Cost-Tab**, **kein Impact-Map** (Decision vom Spike-Briefing).
Heatmap aus `/v1/insights/failures`, p50/p95/p99 aus `/v1/jobs/{key}/stats`
für die jeweils slowest jobs.

### B-PR-10 (optional) · Alerts-Rules + Channels
**Kein On-Call-Tab**, **kein Incidents-Tab**.
Braucht Backend: `/v1/alerts/rules` + `/v1/alerts/channels` — diese
Endpoints existieren noch NICHT. Vorher Backend-Erweiterung.

## Spezifika & Stolpersteine

- **Theme-Storage**: localStorage Key `croniq_theme` mit `light|dark`. Toggle
  setzt `document.documentElement.dataset.theme`.
- **Cards-Chrome ist Default** (war Decision vom Designer-Chat). Kein Lined-Style mehr.
- **MfaRequiredResponse** muss in jeder login-flow-Component-Variante via
  `isMfaRequired(res)` discriminator narrowed werden — siehe LoginPage.tsx.
- **OIDC-Button** nur zeigen wenn `GET /v1/auth/oidc/config` → `enabled: true`.
- **Audit-Page** zeigt nur `users:admin` / `admin` — die `useUser()`-hook
  (existiert noch nicht) muss `role` aus dem JWT-Decode lesen.
- **JWT-Decode im UI**: existiert evtl. nicht; Pattern in
  `ui/src/auth/store.ts` ergänzen — base64-decode des middle parts, das
  `user_id` / `role` / `auth_method` herausziehen.
- **PAT-Auth zum Testen vom Browser**: PATs sind `Authorization: Bearer croniq_pat_...`
  und gehen für CLI-Tests, nicht aber für die UI selbst (die UI nutzt JWT).
- **/v1/users/me/totp/setup** retourniert `recovery_codes` (10×) als
  Vec<String>; Profile-Tab MUSS sie EINMAL zeigen + Operator-Bestätigung
  ("Codes saved") erzwingen, bevor er weiterklicken kann.

## Was NICHT in Spike-B kommt

(Steht im CHANGELOG explizit so:)
- Workspace-Switcher / Multi-Tenancy
- Secrets-Vault (UI + Backend bewusst ausgeschlossen)
- Cost-Tracking in $
- On-Call-Rotation + Incidents
- "Recover via runner SSH"-Login

## Reference-Files vom Design-Bundle

Hochrelevant für Pixel-Look:
- `app.jsx` (line 6-19) → Theme/Accent/Density-Tweaks
- `app.jsx` (line 159-208) → Alert-Modal-Pattern (reusable für Settings-Forms)
- `shell.jsx` (line 6-29) → NAV-Array (nav items + badges)
- `shell.jsx` (line 84-148) → UserMenu (Avatar + Items + Theme-Switch)
- `page-dashboard.jsx` (gesamt) → KPI-Grid + Sparklines + Heatmap
- `page-jobs.jsx` (line 33-77) → .split-Master-Detail
- `styles.css` → token-Werte (color/spacing/shadows)

Wenn der diff zum Designer-Bundle > 30% wird, ist die Translation falsch
gelaufen — der Bundle ist die Source-of-Truth für Look-and-Feel.

## Akzeptanz pro Sub-PR

Nach jeder PR:
- `npm run lint` (eslint, sollte clean sein)
- `npm run build` (tsc + vite, muss durchlaufen)
- Manueller Click-Through: alle Pages erreichbar, login → dashboard → jobs → einer öffnet
- Falls neuer Endpoint genutzt: `curl` gegen :4000 zeigt das richtige JSON

Nach Spike-B-Ende:
- `cargo test --workspace` muss noch grün sein (UI ändert kein Rust)
- `git log --oneline origin/main..HEAD` zeigt 8-10 fokussierte Commits
- README-Screenshot in `docs/screenshots/` aktualisieren (low-prio)
