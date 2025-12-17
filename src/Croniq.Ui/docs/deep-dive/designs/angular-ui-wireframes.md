# Croniq UI Wireframes

Text-based wireframes and component inventory for the Angular 21 + Tailwind admin UI. Use these as the blueprint when translating into Figma or Storybook stories.

## Notation

- `[]` denotes actionable controls (button, toggle, link).
- `{}` denotes dynamic data (numbers, tags, tenant names).
- `---` indicates split panes or section dividers.
- All layouts assume the global shell described below.

## Global Shell

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Croniq Logo        Tenant: {tenant}  Env: {env}      [Command ⌘K] [User ▾]   │
├───────────┬──────────────────────────────────────────────────────────────────┤
│ Nav Rail  │  Status Strip: Cluster {healthy/warn} · Queue {value} · Clock Δ  │
│ - Dashboard                                                                   │
│ - Schedules                                                                   │
│ - Jobs                                                                        │
│ - Webhooks                                                                    │
│ - Tenants & Keys                                                              │
│ - Observability                                                               │
│ - Settings                                                                    │
├───────────┴──────────────────────────────────────────────────────────────────┤
│ Notification Toast Region (top-right)                                         │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Dashboard

```
┌──────────────────────────┬──────────────────────────┬──────────────────────────┐
│ Queue Depth (sparkline)  │ Trigger Throughput       │ Policy Alerts            │
│ {current}  {trend}       │ {per min}                │ {count} w/ severity pill │
└──────────────────────────┴──────────────────────────┴──────────────────────────┘
┌───────────────────────────────┬──────────────────────────────────────────────┐
│ Upcoming Triggers (table)     │ Misfire Heat Map (calendar grid)            │
│ Job        Next Fire   SLA    │   color-coded per day, tooltip on hover     │
│ ---------- -------------- ----│                                              │
│ {jobKey}   {timestamp}   OK   │                                              │
└───────────────────────────────┴──────────────────────────────────────────────┘
┌───────────────────────────────┬──────────────────────────────────────────────┐
│ Dead-Letter Queue Snapshot    │ Webhook Health                              │
│ {count} items  [View →]       │ {active}/{failed} · Signature status        │
└───────────────────────────────┴──────────────────────────────────────────────┘
```

## Schedules

```
┌──────────────────────────────┬──────────────────────────────────────────────┐
│ Filters: [Search job/key] [Status ▾] [Owner ▾]                              │
│ Table:                                                              [Create]│
│ ┌────────────┬─────────────┬───────────┬─────────────┬───────────┐           │
│ │ Job Key    │ Cron Expr   │ Next Fire │ Policy Set  │ Status    │           │
│ └────────────┴─────────────┴───────────┴─────────────┴───────────┘           │
│ Row actions: [View], [Clone], [Disable]                                     │
├──────────────────────────────┴──────────────────────────────────────────────┤
│ Detail Pane (tabs)                                                           │
│ Tabs: Overview | Timeline | Policy Diff | Audit                             │
│ Overview: metadata, tags, owner, last run stats                              │
│ Policy Diff: side-by-side JSON diff (current vs draft)                      │
│ Timeline: executions list w/ status chips                                    │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Jobs

```
┌───────────────────────┬──────────────────────────────────────────────────────┐
│ Facets: Provider, Tag │                                                       │
│ Search: [job name]    │  Job Registry Cards                                  │
│                       │  ┌────────────────────────────┐ ┌──────────────────┐  │
│                       │  │ Job: {name}                │ │ Job: {name}      │  │
│                       │  │ Lock: {partition}          │ │ ...              │  │
│                       │  │ Next: {timestamp}          │ │                  │  │
│                       │  │ [Trigger Now] [View Logs]  │ │ [Trigger Now]    │  │
│                       │  └────────────────────────────┘ └──────────────────┘  │
├───────────────────────┴──────────────────────────────────────────────────────┤
│ Job Drawer (opens from right)                                                │
│ Sections: Metadata · Last Executions · Telemetry                             │
│ Last Executions table with `View Trace` links                                │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Webhooks

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Metrics Strip: Requests/min · Success % · Dead-Letter count                  │
├──────────────────────────────┬──────────────────────────────────────────────┤
│ Hook List (grid)             │ Detail Pane                                  │
│ ┌───────────┬──────┬──────┐ │ Header: {hookKey} [{env}] [Rotate Secret]     │
│ │ Hook Key  │ Mode │ Auth │ │ Tabs: Overview | IP Rules | History | Payload │
│ └───────────┴──────┴──────┘ │ Overview: endpoints, rate limits, last call   │
│ Row chips: Signature opt-out │ IP Rules: editable table w/ CIDR + notes     │
│ indicator, Secret age, Alerts│ History: secret rotations timeline           │
└──────────────────────────────┴──────────────────────────────────────────────┘
```

## Webhook Dead-Letters

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Dead-Letters                                                                 │
│ Filters: [Search] [Status ▾] [Hook Key ▾]                                    │
│ Table:                                                                       │
│ ┌──────────────┬───────────┬───────────────┬───────────────┬──────────────┐  │
│ │ DeadLetterId │ Hook Key   │ Received      │ Last Error    │ Status       │  │
│ └──────────────┴───────────┴───────────────┴───────────────┴──────────────┘  │
│ Row actions: [View Payload] [Replay]                                         │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Tenants & API Keys

```
┌──────────────────────────────┬──────────────────────────────────────────────┐
│ Tenant list + quotas         │ Tenant detail                                │
│ [Create Tenant]              │ Tabs: Overview | API Keys | Policies | Audit │
│ ┌───────────┬──────────────┐ │ API Keys:                                    │
│ │ Tenant    │ Schedules    │ │ ┌───────────┬─────────┬─────────────┐        │
│ └───────────┴──────────────┘ │ │ Key Name  │ Scope   │ Last Rotated│        │
│                               │ └───────────┴─────────┴─────────────┘        │
│                               │ Row actions: [Rotate], [Disable], [Copy]    │
└──────────────────────────────┴──────────────────────────────────────────────┘
```

### Tenant detail: API Clients

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Tenant detail tabs: Overview | API Keys | API Clients | Policies | Audit      │
├──────────────────────────────────────────────────────────────────────────────┤
│ API Clients                                                                  │
│ [Upsert Client]                                                              │
│ ┌───────────┬──────────────┬───────────────┬───────────────┬──────────────┐  │
│ │ Client Id │ Name          │ Scopes        │ Last Issued   │ Status       │  │
│ └───────────┴──────────────┴───────────────┴───────────────┴──────────────┘  │
│ Row actions: [Issue Token] [Delete]                                          │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Tenant detail: Tenant Tokens

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Tenant detail tabs: Overview | API Keys | API Clients | Tokens | Policies    │
├──────────────────────────────────────────────────────────────────────────────┤
│ Tokens                                                                       │
│ [Issue Tenant Token]                                                         │
│ Output: one-time token display w/ [Copy]                                     │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Tenant detail: Deactivate tenant

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Tenant detail: Overview                                                      │
│ Danger zone: [Deactivate Tenant] (requires confirmation)                     │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Execution Viewer (shared)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Execution Details                                                            │
│ Header: {executionId} · {status} · {started} → {finished}                    │
├──────────────────────────────────────────────────────────────────────────────┤
│ Tabs: Summary | Logs                                                          │
│ Summary: job key, schedule trigger id, retry/misfire metadata                 │
│ Logs: scrollable text view with [Copy] / [Download]                           │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Component Checklist

| Surface        | Components Needed                                                                                                       | Notes                                                       |
| -------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| Global         | Command rail, tenant/env selector, status beacons, command palette modal, toast stack                                   | Palette triggered via `Ctrl/Cmd+K`, tie into router.        |
| Dashboard      | Metric cards, sparkline widget, table, calendar heat map, inline alerts                                                 | Sparkline + heat map should accept live data streams.       |
| Schedules      | Data grid, filter chips, tabbed detail pane, JSON diff viewer, timeline list, form drawer                               | Diff viewer should highlight policy overrides.              |
| Jobs           | Faceted search, card grid, slide-over drawer, execution timeline, trace link badge                                      | Drawer shares components with schedules timeline.           |
| Webhooks       | KPI strip, grid with status pills, detail tabs, editable table (IP rules), timeline of secret rotations, action buttons | Secret rotation CTA reuses admin action modal.              |
| Tenants        | Master-detail layout, tabbed content, API key table, quota visualizations                                               | API key table integrates copy-to-clipboard + rotation flow. |
| API Clients    | Master-detail tab, client table, token issuance flow (one-time token display), confirm delete                            | Maps to `api-clients/*` + `api-clients/*/tokens`.           |
| Dead-Letters   | Table, payload viewer, replay action with confirmation                                                                  | Maps to `webhooks/deadletters/*`.                           |
| Executions     | Execution detail viewer, logs viewer, navigation from schedules/jobs timelines                                           | Maps to `executions/*` + `executions/*/logs`.               |
| Observability  | Grafana embed wrapper, log pulse chart, filter bar                                                                      | Inline vs deep-link decision tracked in checklist.          |
| Modals/Dialogs | Confirm dialogs, rotation wizard, JSON editor, impersonation banner                                                     | All dialogs share focus management + telemetry hooks.       |

## Next Steps

1. Translate each wireframe into high-fidelity Figma boards, reusing the Croniq tokens.
2. Update `CHECKLIST-UI.md` once the wireframes and token inventory are approved.
3. Feed these layouts into Storybook tickets so component work can begin in parallel with scaffolding.
