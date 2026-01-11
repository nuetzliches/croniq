# Croniq UI Wireframes

This document defines the structural and functional design of the Croniq Admin UI.
It serves as the blueprint for the Angular implementation.

## Design Philosophy

- **Density**: High. This is an admin tool for engineers.
- **Navigation**: Sidebar-first with breadcrumb trails for deep navigation.
- **Context**: Explicit Tenant and Environment context.
- **Interactivity**: Optimistic UI updates, real-time status indicators.

## Global Shell

The application uses a persistent sidebar layout.

```
┌───────────┬──────────────────────────────────────────────────────────────────┐
│ CRONIQ    │ [Breadcrumbs / Path]                     [Search ⌘K] [User ▾]    │
│           │                                                                  │
│ [Tenant ▾]│                                                                  │
│ [Env ▾]   │                                                                  │
│           │                                                                  │
│ ── CORE ──│                                                                  │
│ Dashboard │                                                                  │
│ Jobs      │                                                                  │
│ Schedules │                                                                  │
│ Executions│                                                                  │
│           │                                                                  │
│ ── INFRA ─│                                                                  │
│ Runners   │                                                                  │
│ Webhooks  │                                                                  │
│ API Access│                                                                  │
│           │                                                                  │
│ ── SYS ── │                                                                  │
│ Settings  │                                                                  │
└───────────┴──────────────────────────────────────────────────────────────────┘
```

### Context Switchers

- **Tenant Switcher**: Dropdown to switch the active tenant context. Derived from `listTenants`.
- **Environment Switcher**: Dropdown to switch between `dev`, `prod`, etc.

## 1. Dashboard

High-level overview of the system health and activity.

```
┌──────────────────────────┬──────────────────────────┬──────────────────────────┐
│ Active Runners           │ Throughput (RPM)         │ Error Rate (Last 1h)     │
│ {count} [Healthy]        │ {value} {trend}          │ {value}%                 │
└──────────────────────────┴──────────────────────────┴──────────────────────────┘

┌───────────────────────────────────────────────┬────────────────────────────────┐
│ Recent Failures (Dead Letters)                │ Upcoming Schedules             │
│ ┌─────────────┬──────────────┬──────────────┐ │ ┌─────────────┬──────────────┐ │
│ │ Job         │ Reason       │ Time         │ │ │ Job         │ Fire Time    │ │
│ ├─────────────┼──────────────┼──────────────┤ │ ├─────────────┼──────────────┤ │
│ │ payment-sync│ Timeout      │ 2m ago       │ │ │ daily-report│ in 5m        │ │
│ │ email-send  │ 500 Error    │ 15m ago      │ │ │ cleanup     │ in 1h        │ │
│ └─────────────┴──────────────┴──────────────┘ │ └─────────────┴──────────────┘ │
│ [View All Dead Letters →]                     │ [View Calendar →]              │
└───────────────────────────────────────────────┴────────────────────────────────┘
```

Notes:

- Summary metrics include Workers, Runners, and Webhooks presence (workers appear before runners).

## 2. Jobs

Registry of all defined jobs.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Filters: [Search Name] [Namespace ▾]                                [Create] │
├──────────────────────────────────────────────────────────────────────────────┤
│ ┌──────────────┬─────────────┬───────────────┬───────────────┬─────────────┐ │
│ │ Job Key      │ Namespace   │ Triggers      │ Last Run      │ Actions     │ │
│ ├──────────────┼─────────────┼───────────────┼───────────────┼─────────────┤ │
│ │ invoice-gen  │ billing     │ 2 Schedules   │ {status}      │ [Trigger]   │ │
│ │              │             │               │               │ [Edit]      │ │
│ └──────────────┴─────────────┴───────────────┴───────────────┴─────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Job Details (Drawer/Page)

- **Metadata**: Description, Default Configuration.
- **Triggers**: List of associated Schedules.
- **History**: List of recent Executions for this job.
- **Definition**: JSON/YAML view of the job definition.

## 3. Schedules

Time-based triggers management.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ View: [List] [Calendar]                                             [Create] │
├──────────────────────────────────────────────────────────────────────────────┤
│ ┌──────────────┬─────────────┬───────────────┬───────────────┬─────────────┐ │
│ │ Trigger ID   │ Job Key     │ Cron          │ Next Fire     │ Status      │ │
│ ├──────────────┼─────────────┼───────────────┼───────────────┼─────────────┤ │
│ │ t-12345      │ invoice-gen │ 0 0 * * *     │ Tomorrow 00:00│ [Enabled]   │ │
│ └──────────────┴─────────────┴───────────────┴───────────────┴─────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
```

## 4. Executions

Global execution history and status.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Filters: [Job Key] [Status ▾] [Date Range]                                   │
├──────────────────────────────────────────────────────────────────────────────┤
│ ┌──────────────┬─────────────┬───────────────┬───────────────┬─────────────┐ │
│ │ Execution ID │ Job Key     │ Started       │ Duration      │ Status      │ │
│ ├──────────────┼─────────────┼───────────────┼───────────────┼─────────────┤ │
│ │ exec-abc     │ invoice-gen │ 10:00:01      │ 2.5s          │ [Success]   │ │
│ │ exec-xyz     │ email-send  │ 10:05:00      │ -             │ [Running]   │ │
│ └──────────────┴─────────────┴───────────────┴───────────────┴─────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Execution Details

- **Summary**: ID, Job, Trigger, Timestamps, Runner ID.
- **Logs**: Live-streaming logs (NDJSON).
- **Events**: Structured events (WorkEvents).
- **Payload**: Input payload (if any).

## 5. Runners (New)

Infrastructure view of connected workers.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Active Runners: {count}                                                      │
├──────────────────────────────────────────────────────────────────────────────┤
│ ┌──────────────┬─────────────┬───────────────┬───────────────┐               │
│ │ Runner ID    │ Last Seen   │ Metadata      │ Status        │               │
│ ├──────────────┼─────────────┼───────────────┼───────────────┤               │
│ │ worker-01    │ Just now    │ v1.2.0, us-e1 │ [Active]      │               │
│ │ worker-02    │ 5m ago      │ v1.1.0, eu-w1 │ [Warning]     │               │
│ └──────────────┴─────────────┴───────────────┴───────────────┘               │
└──────────────────────────────────────────────────────────────────────────────┘
```

## 6. Webhooks

Inbound webhook management.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ [Register Endpoint]                                                          │
├──────────────────────────────────────────────────────────────────────────────┤
│ ┌──────────────┬─────────────┬───────────────┬───────────────┐               │
│ │ Hook Key     │ Target Job  │ Security      │ Actions       │               │
│ ├──────────────┼─────────────┼───────────────┼───────────────┤               │
│ │ stripe-in    │ payment-proc│ Signed        │ [Rotate Secret]               │
│ │              │             │ IP Whitelist  │ [IP Rules]    │               │
│ └──────────────┴─────────────┴───────────────┴───────────────┘               │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Webhook Details

- **Overview**: Endpoint URL, Secret management.
- **IP Rules**: CIDR allow-list management.
- **Dead Letters**: Failed webhook deliveries (Replay capability).

## 7. API Access

Management of programmatic access.

### API Clients

- List of registered clients.
- **Actions**: Issue Token, Revoke, Manage Scopes.

### API Keys

- List of long-lived API keys.
- **Actions**: Issue New Key, Rotate Key, Revoke.

## 8. Settings / Tenant

- **Tenant Metadata**: Name, ID.
- **Deactivate Tenant**: Danger zone.

## Component Inventory

| Component         | Usage                                                     |
| ----------------- | --------------------------------------------------------- |
| **StatusBadge**   | Visual indicator for Success, Failure, Running, Warning.  |
| **CodeBlock**     | For displaying JSON payloads, Logs, and Cron expressions. |
| **DataTable**     | Sortable, filterable table with row actions.              |
| **Drawer**        | Slide-over panel for details (Jobs, Executions).          |
| **ConfirmDialog** | For destructive actions (Delete, Revoke).                 |
| **MetricCard**    | Dashboard summary widgets.                                |
| **LogViewer**     | Virtualized list for execution logs.                      |
