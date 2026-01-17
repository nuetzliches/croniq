# Croniq Schedule Calendars Concept

_Last updated: 2026-01-16_

## Objectives

- Introduce Quartz-like calendars as first-class entities that include or exclude schedule fire times.
- Keep existing trigger semantics intact; calendars are optional filters, not new trigger types.
- Support UI calendar views without persisting per-fire instances.
- Provide clear hooks for observability, validation, and tenant isolation.

## Terminology

| Term                  | Description                                                                 |
| --------------------- | --------------------------------------------------------------------------- |
| Calendar Definition   | Tenant and environment scoped entity that holds inclusion/exclusion rules.  |
| Calendar Rule         | A single rule inside a calendar (daily window, date list, cron rule, etc.). |
| Calendar Assignment   | Reference from a schedule trigger to a calendar definition.                 |
| Included Time         | A candidate fire time that passes the calendar rules.                       |
| Excluded Time         | A candidate fire time that is filtered out by the calendar rules.           |

## Data Model

- CalendarDefinition: `CalendarId`, `TenantId`, `EnvironmentTag`, `Name`, `Description`, `TimeZoneId`, `Mode` (Include|Exclude), `Rules[]`, `Enabled`, `CreatedAtUtc`, `UpdatedAtUtc`.
- CalendarRule: typed payload with `RuleType`, `SortOrder`, `IsEnabled`, and rule-specific fields.
- Schedule Trigger: add optional `CalendarId` reference.

## Rule Types (Initial Set)

- DailyWindow: start/end time-of-day, optional days-of-week.
- WeeklyWindow: one or more allowed weekdays.
- AnnualDateList: month/day pairs (holidays).
- DateList: explicit date values (one-offs).
- CronRule: cron expression evaluated in calendar time zone.

## Evaluation Semantics

1. Compute the next candidate occurrence using the schedule cron/once expression and trigger time zone.
2. If no calendar is assigned, accept the candidate.
3. If a calendar is assigned:
   - Mode = Exclude: any matching rule excludes the candidate.
   - Mode = Include: candidate must match at least one rule.
4. If excluded, advance to the next candidate and repeat (bounded by a max-iteration guard).
5. Respect `StartAtUtc` and `EndAtUtc` even after calendar filtering.

## API Surface

- `GET /tenants/{tenantId}/calendars`
- `POST /tenants/{tenantId}/calendars`
- `GET /tenants/{tenantId}/calendars/{calendarId}`
- `DELETE /tenants/{tenantId}/calendars/{calendarId}`
- Schedule upsert accepts optional `calendarId`.

## UI Impact

- New Calendars page (list + editor).
- Schedule editor exposes calendar assignment.
- Schedule calendar view shows occurrences after calendar filtering.

## Persistence

- Tables: `croniq.Calendars`, `croniq.CalendarRules` (or JSON rules column).
- Enforce tenant/environment scope at query time.
- Add indexes on `TenantId`, `EnvironmentTag`, and `Name`.

## Observability

- Add tags to schedule evaluation spans: `calendar.id`, `calendar.mode`, `calendar.rule_hits`.
- Metrics: skipped occurrences count, evaluation duration, calendar cache hits.

## Security & Validation

- Validate rule payloads and time zone ids.
- Reject invalid cron expressions in calendar rules.
- Enforce scopes: `calendars:read`, `calendars:write`.

## Testing

- Unit tests for rule evaluation and time zone handling.
- Integration tests for persistence CRUD and schedule evaluation with calendars.
- UI tests for schedule assignment and calendar views.

### Status (Implementation)

- Unit coverage: `CalendarDefinitionValidatorTests`, `CalendarEvaluatorTests`.
- API coverage: calendar CRUD endpoints plus schedule upsert with calendars.
- gRPC coverage: calendar-aware schedule upsert (success + not-found).
- Persistence coverage: calendar CRUD in SqlServer/Postgres provider tests.
- UI coverage: calendars list + editor implemented in Croniq.Ui; schedule editor calendar assignment added.
- UI tests for calendar assignment/views are still pending.

## Open Questions

1. Calendar time zone is required and fixed per calendar (no trigger inheritance).
2. Only one calendar assignment per schedule in v1.
3. Guard schedule filtering with max-iteration + max-lookahead defaults (10,000 iterations or 365 days).
4. No per-schedule overrides in v1; use calendar rules or dedicated calendars instead.

## Constraints & Follow-ups

- Fixed time zones require separate calendars per time zone; a future `InheritTriggerTimeZone` flag can relax this.
- Single-calendar assignment blocks cross-calendar composition; a future ordered assignment list can provide explicit precedence.
- Guarded evaluation can return no next occurrence for highly excluded schedules; future options can allow per-calendar limits.
- No per-schedule overrides means ad-hoc exclusions require calendar edits; future overlay rules can address temporary exceptions.

Document decisions here and mirror the finalized design into `docs/deep-dive/designs/schedule-calendars.md`.
