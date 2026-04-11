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

Set `VITE_API_URL` to point to the Croniq server (default: `http://localhost:4000`).

## Build

```sh
npm run build     # Output in dist/
```

Serve the built files via `croniq-server --ui-dir ui/dist`.

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
