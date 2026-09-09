# Croniq UI

React dashboard for Croniq — the distributed job scheduling platform.

## Stack

- React 19 + TypeScript
- Vite (build + HMR)
- Tailwind CSS 4
- TanStack Query (data fetching)
- Zustand (auth state)
- React Router v7

## Development

```sh
npm install
npm run dev       # http://localhost:5173
```

The Vite dev server proxies `/v1`, `/health`, and `/metrics` to
`http://localhost:4000` (override via `CRONIQ_API_ORIGIN`). Start
`croniq-server` in another terminal and the UI will talk to it through
the proxy without any CORS juggling.

## Build

```sh
npm run build     # Output in dist/
```

Serve the built files via `croniq-server --ui-dir ui/dist`. The UI uses
relative URLs by default, so it works in any deployment where the server
serves both the UI and the API. Set `VITE_API_URL` at build time only if
you deploy the UI on a different origin than the API.

## Tests

```sh
npm test          # unit tests (vitest)
npm run test:e2e  # browser smoke suite (Playwright)
```

The e2e suite brings up its own stack — it seeds a fresh database, starts
`croniq-server` over `Croniqfile.demo` on `127.0.0.1:4010`, and attaches one
demo runner. Build the binaries and the bundle once first:

```sh
cargo build -p croniq-cli -p croniq-server -p croniq-demo-runner \
  --bin croniq --bin croniq-server --bin croniq-demo-runner
npm run build
npx playwright install chromium
```

`CRONIQ_BIN_DIR` overrides where the binaries are looked up (default
`target/debug`), and `CRONIQ_E2E_PORT` the port. `npm run test:e2e:ui` opens
Playwright's UI mode for debugging a single spec.

Two things about the suite are worth knowing before extending it, both of
them consequences of how sessions work rather than of Playwright:

- **The whole suite shares one signed-in page** (a worker-scoped fixture).
  Playwright's usual `storageState` pattern does not work here, because
  `POST /v1/auth/refresh` rotates the refresh token — a saved state is
  single-use, and every test after the first replays a revoked token.
- **Logins are rate-limited** at 30 attempts per five minutes per IP, so
  specs should use the shared `app` fixture rather than logging in.
  `auth.spec.ts` is the exception, and it is deliberately small.

## Views

| View | Route | Description |
|---|---|---|
| Login | `/login` | Username + password authentication |
| Dashboard | `/` | Health stats, queue depth, recent executions |
| Jobs | `/jobs` | Job definitions CRUD |
| Job Detail | `/jobs/:key` | Schedules + executions for a job |
| Schedules | `/schedules` | Trigger definitions CRUD |
| Runners | `/runners` | Connected runners with status |
| Executions | `/executions` | Execution history with log viewer |
| Dead Letters | `/dead-letters` | Failed executions with detail panel |

### Class-name convention

`src/styles/*.css` is plain, unlayered CSS. Tailwind v4 emits its utilities in
`@layer utilities`, and unlayered CSS beats layered CSS regardless of source
order — so a rule here named after a Tailwind utility wins over it silently and
unconditionally. Component classes therefore carry a `cq-` prefix where the
name would otherwise collide (`cq-grid`, `cq-gap-6`, `cq-grow`).
`src/styles/no-tailwind-collision.test.ts` keeps it that way.

`scripts/style-snapshot.mjs` captures the computed layout properties of every
element on every page, so a CSS refactor can be shown not to change anything
visible rather than assumed not to:

```sh
node scripts/style-snapshot.mjs before.json
# …make the change, npm run build…
node scripts/style-snapshot.mjs after.json
node scripts/style-snapshot.mjs --diff before.json after.json
```
