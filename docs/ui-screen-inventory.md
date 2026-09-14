# Phase 0: screen inventory for the Vue rebuild

As of 2026-09-10. Result of the design phase from
[ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md). Basis: code analysis
of the React tree, no assumptions.

**Stack (decided):** the `nuts-customer-portal` stack — Vue 3 + Nuxt UI 4 +
Pinia + `@tanstack/vue-query` + `ofetch` + vue-router. With it, Nuxt UI's
theming replaces the project-owned oklch token system of the React tree; that is
deliberate and the reason this is a rebuild and not a port.

**Cut (decided):** screens are thought through again and simplified, not
carried over 1:1.

---

## Correction to ADR-0004

ADR-0004's scope guard says: *"The screen list is frozen at the ten routes
that exist today."* That was meant as protection against scope **growth**, but it
is worded too literally — merging is the opposite of that.

More precisely: what is frozen is the set of **capabilities**, not the set of
routes. The cut is open; every action reachable today and every piece of
information visible today must have a place in the new cut. The list
under *What must not be lost* is the binding version of that.

---

## Finding: one thing is built four times

`useExecutions` runs on **four of ten screens**:

| Place | What | Evidence |
|---|---|---|
| `JobsPage` → tab *Overview* | executions table, 12 rows, 5 columns | `JobsPage.tsx:966–997` |
| `JobsPage` → tab *Executions* | the same table, all rows, + `attempt`, `error` | `JobsPage.tsx:1181–1218` |
| `ExecutionsPage` | the same list, global, with filters | `?job_key=` |
| `RunnersPage` → detail | the same list, filtered by `runner_id`, `limit: 50` | `RunnersPage.tsx:195` |

The two job tabs render the same query with identical cell markup —
`ExecutionLink`, `StatusPill`, `RunnerLink`, `formatRelative`, `durationFmt` —
and differ in two columns and one `slice(0, 12)`. The
*Executions* tab additionally links from its header to `/executions?job_key=…`, that is,
to a third rendering of the same data.

The same pattern, smaller:

- `useAuditEvents` — in `JobsPage` (tab *Audit*) and in `settings/AuditTab`.
- `useAlertDeliveries` — in `JobsPage` (tab *Alerts*) and in `AlertsPage`.

That is not a coincidence but a pattern: **a detail page rebuilds a global
page, filtered.** Three of the six job tabs exist only for that reason.

---

## Proposal: one place per concept

Guiding rule — *one list per concept; detail pages link into it filtered,
instead of rebuilding it.*

### 1. Runs: the one execution view

`/executions` becomes the only place where executions appear as a list.
Filters: job, runner, status, time range — all in the URL, as is already the case
today for `state` and `job_key`.

That drops: the overview tab's excerpt, the executions tab and the
executions block in the runner detail. The job page instead shows a
status line ("last 5 runs", sparkline) and links into the filtered list.

**The route stays `/executions`.** A rename to `/runs` would be prettier and would break
bookmarks and the URL contracts that `ui/e2e/url-state.spec.ts` asserts — with no
return.

### 2. Job detail: from six tabs to two

| today | in future |
|---|---|
| Overview | **Overview** — definition, policy, status, recent runs as a summary |
| Executions | dropped → link into the runs list |
| Schedule | integrated into *Overview* (triggers are part of the definition) |
| DSL | **DSL** — stays on its own, it is a different representation of the same thing |
| Alerts | dropped → link into the alerts list, filtered by `job_key` |
| Audit | dropped → link into the audit list, filtered by entity |

### 3. Audit: one place

Today in settings *and* as a job tab. In future one view with an entity filter,
linked from both contexts. Whether it stays under settings or becomes
standalone depends on the navigation question below.

### 4. Alerts: separate configuration and deliveries

`AlertsPage` today mixes both — rule/channel configuration with overrides
(snooze, throttle, disable) and the delivery history. Those are two things: a
setting and a log. Proposal: configuration with the rest of the
settings, deliveries as a filterable list next to the runs.

---

## What must not be lost

The binding version of the scope guard. Every line is reachable today and
needs a place in the new cut — not necessarily the same one.

**Jobs:** create, edit, delete, enable/disable, trigger
manually, adopt/release (DSL-managed vs. API-managed), tags,
dead-letter policy, timeout, retries, forecast (next fire times),
job statistics, schedule management including calendar binding, DSL view.

**Runs:** list with filters (job, status, runner), detail with logs, cancel,
attempt and error text, links to job and runner.

**Dead Letters:** list, detail, replay (including the stale guard), single and
bulk deletion, retention/expiry, operator hint.

**Runners:** live list over SSE, tags, filters, detail with the runs that belong to it,
removal.

**Calendars:** CRUD, rule builder, adoption.

**Alerts:** rule and channel configuration, overrides (snooze, throttle,
disable, clear), delivery history.

**Console:** live tail of the server tracing, level filter, search, pause,
copy, NDJSON export. Admin-only.

**Settings:** profile including TOTP enrolment and PATs, users and invitations,
API clients and tokens, audit log.

**Cross-cutting:** login including MFA and OIDC **as well as password recovery
(the request *and* the landing page) and invitation acceptance** — these three were missing in
the first version of this list, and the two landing pages were even missing from the
shipping dashboard: the server sends out `…/password-reset/confirm` and
`…/invitations/accept`, and neither tree served the routes (pass 12).
Theme switching, sidebar state,
command palette, the dead-letter counter in the topbar, and maintenance mode in
**both** halves — the banner for everyone, the controls (manual on/off,
scheduled window, note) admin-only in the topbar. The first version of this
list named only the banner; the controls were missing and would thereby have quietly been
dropped.

---

## Decided (2026-09-10)

All four open questions answered.

**Dead Letters stay their own screen.** Technically the filter would be possible
— `ExecutionState::Dead` exists and `/v1/executions?state=dead` already works
today. What settled it was the role, not the technology: dead letters
are the only surface in the product that is a to-do list. Mixing a work list
into a browsing list makes the work invisible — you would have to set the
filter first to see that something is pending. The topbar badge keeps
its target; the run detail of a dead run links here.

**The dashboard becomes a status board plus a failure excerpt.** Numbers, throughput, heatmap,
plus a narrow excerpt of only the most recent failures with a link into the
filtered runs list. No general "recent runs" block — that would be the
fourth rendering of the same table. The excerpt is deliberately a different
representation (compact, failures only), not a shrunken clone.

**Calendars stay in the main navigation.** They belong to jobs in domain terms, not
to server administration, and the rule builder is too big for a
settings tab.

**The console stays its own screen**, shown by role as it is today.
Server tracing and run logs are both called "logs" and are different things;
merging them produces exactly the confusion that gets in the way when debugging.

## Resulting cut

```
Operations      Dashboard   Runs   Runners   Dead Letters
Configuration   Jobs        Calendars   Alerts
System          Console (admin)   Settings
```

| Route | Content |
|---|---|
| `/` | status board + failure excerpt |
| `/executions`, `/executions/:id` | the one run list; job/runner/state/time filters in the URL |
| `/runners` | live list + detail **without** an executions block |
| `/dead-letters` | work list: replay, delete, bulk action, retention |
| `/jobs`, `/jobs/:key` | master/detail, **two** tabs (overview including schedule, DSL) |
| `/calendars` | CRUD + rule builder |
| `/alerts` | rules, channels, overrides |
| `/console` | live tail, admin-only |
| `/settings` | profile, users, API clients, audit |
| `/login` | including MFA and OIDC |

**It is still ten routes.** The gain is not in the number of
screens but in the content that was dropped: job detail from six to two tabs,
the executions block in the runner detail gone, audit and alert deliveries with one
place each instead of two, the dashboard without a general run list. Four renderings of the
executions table become one.

## Decided (2026-09-11): alerts

The last open question — does the delivery history stay with the rules or does
it become a filterable list next to the runs? — was answered while building the screen,
and with **neither of the two versions as posed**.

The objection from the stocktake was that the React page *mixes* two things:
configuration and log, flat on one page. But the answer
to mixing is not separation onto different screens, but
structure. The questions run across the boundary: you snooze a rule
*because of* what the log shows, and you read the log to
find out which rule went off. Two screens put a
navigation step between question and answer.

So **one screen, three views**, each a clean list of one thing, all
three addressable:

| Route | Content |
|---|---|
| `/alerts` | the rules, with their overrides |
| `/alerts/rules/:name` | one rule: configuration, override, its deliveries |
| `/alerts/channels` | where deliveries go, and which rule uses which channel |
| `/alerts/deliveries` | what actually went out, filterable (rule, job, status) |

**Deliveries stay out of `/executions`**, for the same reason that
dead letters kept their own screen: a job run and an
alert delivery are different things that happen to have the same shape. Merging
them would mean "200 runs" means two different things.

Two things the channel view makes visible that were mute before: a
channel that no rule uses (`unused`), and a rule that names a channel
that does not exist — the compiler keeps the reference verbatim and warns only
at fire time, so the rule looks configured and delivers nowhere.

## Build order

The cut is settled; the scaffold is next. Order by dependency,
not by size:

1. **Scaffold + data layer** — Vite/Vue project, Nuxt UI 4, Pinia stores with
   identical localStorage keys, vue-query over `ofetch`, dev proxy and the
   WASM hooks carried over unchanged.
2. **Shell + auth** — navigation per the cut above, router guard *plus*
   a watch on `isAuthenticated` (Vue guards only fire on navigation), login
   including MFA.
3. **Runs list** — the screen that replaces three others. First, because job and
   runner detail link into it instead of rebuilding it.
4. **Dashboard, runners, dead letters** — they build on the same list building blocks.
5. **Jobs** — the thickest screen, benefits most from finished
   building blocks. ✓ (pass 6 in `ui-visual-design.md`)
6. **Calendars** ✓ (pass 7), **alerts** ✓ (pass 9),
   **settings** ✓ (pass 10), **console** ✓ (pass 11).

**The order has been worked through.** Every route of the cut above is
built; the `NotBuiltYet` placeholder is removed. What remains open is the
acceptance criterion itself — see below.

The acceptance criterion per step remains the Playwright suite
([ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md), scope guard 2). It
asserts routes, session, URL contracts and both SSE surfaces — none of it
appearance.
