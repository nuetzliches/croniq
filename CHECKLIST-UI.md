# Croniq UI Checklist

Derived from [docs/deep-dive/designs/angular-ui-concept.md](docs/deep-dive/designs/angular-ui-concept.md). Update these tasks as the Angular 21 + Tailwind admin UI progresses.

## Delivery Phases

- [ ] Design Spike – produce wireframes and refreshed design tokens aligned with the concept doc.
- [ ] Design Spike – finalize Tailwind theme plus typography approvals with stakeholders.
- [ ] Scaffolding & Auth – initialize the Angular workspace in `src/Croniq.Ui`, configure MCP helper tasks, and wire the OIDC stub with the tenant switcher.
- [ ] MVP Data Surfaces – deliver the dashboard metrics (stubbed), schedules read-only grid, and job registry view.
- [ ] Admin Controls – implement CRUD for schedules, webhooks, and API keys, including dead-letter replay wiring.
- [ ] Observability & Polish – embed Grafana/log pulse views and complete the accessibility plus localization review.
