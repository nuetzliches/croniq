> **Superseded (2026-09-14).** This brief plans the dashboard rebuild as a
> series of PRs against the **React** tree that lived in `ui/src/`. That tree
> was removed in v0.38.0 and `ui/` now holds the Vue 3 rebuild — see
> [ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md). Every path, component
> and file name below refers to code that no longer exists.
>
> Kept as a record of what the rebuild was asked to be: the screen inventory,
> the endpoints each screen was meant to draw from, the design decisions taken
> in the designer chat (cards chrome, no cost tracking, no on-call rotation),
> and the pitfalls list — most of which still apply to the Vue tree, because
> they are about the API rather than the framework.

# UI spike brief — the Croniq dashboard rebuild (Spike B)

**Branch:** `feat/auth-foundation` — the backend foundation is complete; now
pull the dashboard along.

## Prerequisites (once)

```bash
# Node 20.19+ or 22.12+ — Vite 8 requires it
node -v                                      # must be >= 20.19

cd /home/sebastian/dev/croniq/ui
npm ci                                       # ~30s
docker compose up -d                         # backend on :4000
npm run dev                                  # Vite HMR on :4231 (proxied to :4230)
```

Health check before starting: `curl http://localhost:4000/health` →
`{"status":"ok",...}`. Login test: `admin / admin` through the old dashboard on
:4000.

## Where things stand

**Backend — all done, deployed in the container:**

- Six auth PRs A1–A6 plus the DSL addition A5b: multi-user with roles, TOTP/2FA,
  personal access tokens, OIDC, optional SMTP.
- Three stats PRs B1/B1b/B1c: the audit log and its recording, job stats,
  throughput, the failure heatmap, OpenAPI.
- 16 migrations, ~30 new REST endpoints, 539 tests green.
- Complete TypeScript types are in `ui/src/api/types.ts`.

**Dashboard — today:**

- LoginPage has the MFA step and the OIDC button (PR-B2, c14d37c), but neither
  is visible in the default flow.
- Otherwise unchanged: the old sidebar and topbar, the old pages.

**The design bundle from Claude Design** is at
`/tmp/croniq-design/croniq-ui/project/`:

- `app.jsx`, `shell.jsx`, `data.js`, `styles.css`
- `page-dashboard.jsx`, `page-jobs.jsx`, `page-executions.jsx`,
  `page-runners.jsx`, `page-insights.jsx`, `page-planner.jsx`,
  `page-secrets.jsx`, `page-misc.jsx`, `page-newjob.jsx`,
  `page-notifications.jsx`, `page-onboarding.jsx`
- `chats/chat1.md` — the designer chat transcript, with the reasoning

## Recommended order

The order follows "tokens → primitives → shell → pages". One commit per step, so
each diff stays readable.

### B-PR-1 · design tokens (~250 lines of CSS)

- `ui/src/styles/tokens.css`: port the oklch palette and the cards-chrome
  variables from `/tmp/croniq-design/croniq-ui/project/styles.css`.
- `ui/src/index.css` pulls in `@import "./styles/tokens.css"` directly after
  `@import "tailwindcss"`.
- `<html data-theme="dark"|"light">` as the hook.
- Acceptance: every existing page loads and does not look broken.

### B-PR-2 · primitives (~600 lines of TSX)

Purely presentational components — no state, no API binding:

```
ui/src/components/primitives/
  StatusPill.tsx         a status string → a coloured chip
  Sparkline.tsx          number[] → inline svg
  Donut.tsx              {value, max} → an svg ring
  RunBars.tsx            ('ok'|'warn'|'err')[] → a row of bars
  EmptyState.tsx         {icon, title, desc, actions} → a centred placeholder
  Toggle.tsx             an ARIA-conformant switch
  KPICard.tsx            {title, value, sub?, chart?, delta?} → a dashboard tile
  CopyBtn.tsx            copies {value} to the clipboard
  Avatar.tsx             an initials circle
```

All of them free of external dependencies, beyond `react` itself where needed.
The model is the inline `React.createElement` in the bundle — JSX that is easy
to port.

### B-PR-3 · the app shell (~800 lines)

The cards-chrome layout:

```
ui/src/layout/
  Sidebar.tsx            replaces the sidebar part of /Layout.tsx — as a card
  Topbar.tsx             brand + crumbs + the ⌘K trigger + bell + user menu
  CommandPalette.tsx     the ⌘K modal, fuzzy search over jobs/runners/actions
  LogTail.tsx            the ⌘J dock at the bottom: resizable, pause/follow/filter
  NotificationsPanel.tsx the bell popover (unread/all tabs)
  UserMenu.tsx           the avatar dropdown, bottom left: profile, theme, logout
```

The existing page components stay; only the layout around them is new.
Acceptance: every old page is still reachable, and mobile below 768 behaves
correctly (sidebar collapsed to 60px).

### B-PR-4 · LoginPage polish

Already functional via PR-B2. What to improve: an operator-console feel on the
left panel, perhaps. Leave the animated live console out — zero user value.
Instead:

- Segmented tabs, "Password / API Token / SSO", rather than two separate forms.
- A brand mark, a tagline and a teaser of three KPI cards is fine.
- Recovery via SSH: **no** — the backend cannot do it at all. A "Forgot
  password?" link to `/v1/auth/password-reset/request` instead.

### B-PR-5 · the dashboard (~600 lines)

Real data, from the PR-B1 endpoints:

```
KPIs:             GET /health + /v1/jobs (count) + /v1/executions/throughput?window=24h
Sparklines:       /v1/executions/throughput (rolling 24h ok counts)
Failure heatmap:  /v1/insights/failures?days=28
Activity feed:    /v1/audit?limit=10
Top failing:      aggregate /v1/jobs/{key}/stats over the top five jobs by failure rate
Upcoming:         /v1/dashboard/forecast (already exists, used with a different style)
Runner fleet:     /v1/runners plus a donut for inflight/slots
```

New hooks in `ui/src/api/hooks.ts`:

- `useAuditEvents(filter)` — `GET /v1/audit`
- `useJobStats(jobKey, days)` — `GET /v1/jobs/{key}/stats`
- `useThroughput(window)` — `GET /v1/executions/throughput`
- `useFailureHeatmap(days)` — `GET /v1/insights/failures`

### B-PR-6 · the jobs master/detail split

Replace `JobsPage` plus `JobDetailPage` with a single split page:

```
ui/src/pages/JobsPage.tsx
  .split
    .master (the job list, with search and a tag filter)
    .detail (tabs: Overview / Runs / Schedule / DSL / Alerts / Audit)
```

Hovering a job row peeks a mini card with the success rate and the last fire.
The master list has a sticky header and its own scroll container. The detail
tabs are separate sub-components, all under `ui/src/pages/jobs/`.

### B-PR-7 · flesh out the settings tabs

New tabs in `ui/src/pages/SettingsPage.tsx`:

```
Tabs:    API Clients | API Keys | Webhooks | Audit | Org | Profile
         + Users (new — an admin sees the list, invite, and an edit dialog)
         + Profile gains TOTP setup and PAT management
```

The backend endpoints all exist (`/v1/users`, `/v1/invitations`,
`/v1/users/me/tokens`, `/v1/users/me/totp/*`).

### B-PR-8 (optional) · executions throughput bars and log-viewer tabs

Replaces `ExecutionsPage.tsx` with stacked bars, a log filter bar, and
stdout/stderr/env/raw tabs. Lower priority — the old page works.

### B-PR-9 (optional) · the insights page (reliability and latency only)

**No cost tab**, **no impact map** — decided in the spike briefing. The heatmap
comes from `/v1/insights/failures`, and p50/p95/p99 from `/v1/jobs/{key}/stats`
for the slowest jobs.

### B-PR-10 (optional) · alert rules and channels

**No on-call tab**, **no incidents tab**. Needs backend work first:
`/v1/alerts/rules` and `/v1/alerts/channels` do **not** exist yet.

## Specifics and pitfalls

- **Theme storage**: the localStorage key `croniq_theme`, holding `light` or
  `dark`. The toggle sets `document.documentElement.dataset.theme`.
- **Cards chrome is the default** — decided in the designer chat. No more lined
  style.
- **MfaRequiredResponse** has to be narrowed through the `isMfaRequired(res)`
  discriminator in every variant of the login-flow component — see
  LoginPage.tsx.
- **The OIDC button** shows only when `GET /v1/auth/oidc/config` reports
  `enabled: true`.
- **The audit page** is for `users:admin` / `admin` only. The `useUser()` hook
  (which does not exist yet) has to read `role` out of the decoded JWT.
- **Decoding the JWT in the dashboard**: this may not exist yet. Add the pattern
  in `ui/src/auth/store.ts` — base64-decode the middle part and pull out
  `user_id`, `role` and `auth_method`.
- **PAT auth for testing from the browser**: PATs are
  `Authorization: Bearer croniq_pat_...` and work for CLI testing, but not for
  the dashboard itself, which uses a JWT.
- **`/v1/users/me/totp/setup`** returns `recovery_codes` (ten of them) as a
  `Vec<String>`. The profile tab MUST show them ONCE and force an operator
  acknowledgement ("codes saved") before letting them click on.

## What Spike B does not include

(The CHANGELOG says so explicitly.)

- A workspace switcher and multi-tenancy
- A secrets vault — excluded deliberately, in both the dashboard and the backend
- Cost tracking in dollars
- On-call rotation and incidents
- The "recover via runner SSH" login

## Reference files from the design bundle

Highly relevant for the pixel-level look:

- `app.jsx` (lines 6–19) → theme, accent and density tweaks
- `app.jsx` (lines 159–208) → the alert modal pattern, reusable for settings
  forms
- `shell.jsx` (lines 6–29) → the NAV array (nav items and badges)
- `shell.jsx` (lines 84–148) → the user menu (avatar, items, theme switch)
- `page-dashboard.jsx` (all of it) → the KPI grid, sparklines and heatmap
- `page-jobs.jsx` (lines 33–77) → the `.split` master/detail
- `styles.css` → the token values (colour, spacing, shadows)

If the diff against the designer bundle grows past 30%, the translation has gone
wrong — the bundle is the source of truth for look and feel.

## Acceptance, per sub-PR

After each PR:

- `npm run lint` (eslint; should be clean)
- `npm run build` (tsc plus vite; must complete)
- A manual click-through: every page reachable, login → dashboard → jobs → open
  one
- If a new endpoint is used: `curl` against :4000 shows the right JSON

After Spike B ends:

- `cargo test --workspace` must still be green (the dashboard changes no Rust)
- `git log --oneline origin/main..HEAD` shows 8–10 focused commits
- Update the README screenshot in `docs/screenshots/` (low priority)
