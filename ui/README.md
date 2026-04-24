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
