# Schedule Calendars

::: info Status
Implemented. Last verified: 2026-01-18.
:::

This document describes the schedule calendar feature: a separate entity that includes or excludes candidate fire times for schedules.

## Goals

- Provide a tenant-scoped calendar entity that filters schedule occurrences.
- Keep existing trigger definitions as the source of "when to run" and apply calendars as optional filters.
- Support UI calendar views without persisting per-occurrence rows.
- Maintain deterministic evaluation across API, worker host, and UI preview paths.

## Non-goals

- Do not store every future occurrence in persistence.
- Do not add calendar logic to job code; evaluation stays inside the scheduling pipeline.
- Do not introduce external holiday providers in v1.

## Decisions (v1)

- CalendarDefinition.TimeZoneId is required and fixed per calendar (no per-trigger inheritance).
- Each schedule can reference at most one calendar; composition stays inside the calendar rule list.
- Calendar evaluation is guarded by both max-iteration and max-lookahead limits (defaults: 10,000 iterations or 365 days).
- Per-schedule overrides are not supported; use dedicated calendars or explicit rules instead.
- CronRule uses the existing CronExpression parser with the calendar time zone applied.

## Entity Model

CalendarDefinition:

- CalendarId (string, unique per tenant/environment)
- TenantId, EnvironmentTag
- Name, Description
- TimeZoneId (IANA or Windows id)
- Mode: Include | Exclude
- Enabled (bool)
- Rules: list of CalendarRule
- CreatedAtUtc, UpdatedAtUtc

CalendarRule:

- RuleId (string)
- RuleType (DailyWindow, WeeklyWindow, AnnualDateList, DateList, CronRule)
- SortOrder (int)
- IsEnabled (bool)
- Payload (rule-specific fields)

Schedule Trigger:

- Add optional CalendarId reference.

## Rule Types

DailyWindow:

- StartTime (HH:mm)
- EndTime (HH:mm)
- DaysOfWeek (optional list)

WeeklyWindow:

- DaysOfWeek (list)

AnnualDateList:

- MonthDay pairs (MM-dd)

DateList:

- Explicit dates (yyyy-MM-dd)

CronRule:

- CronExpression (same 6-field syntax as schedules)

## Evaluation Semantics

1. Compute the next candidate occurrence from the trigger schedule expression and trigger time zone.
2. If no calendar is assigned, accept the candidate.
3. If a calendar is assigned:
   - Disabled calendars are treated as "no calendar" (candidate accepted).
   - Convert the candidate into the calendar time zone.
   - Evaluate enabled rules in SortOrder.
   - Mode = Exclude: any matching rule excludes the candidate.
   - Mode = Include: candidate must match at least one rule.
4. If excluded, advance to the next candidate and repeat.
5. Guard against infinite loops with a maximum skip counter and log warnings when exceeded.
6. Apply StartAtUtc and EndAtUtc bounds after calendar filtering.
7. For @once schedules: if excluded, return no next occurrence and log that the calendar filtered the only candidate.

## API Surface

- `GET /tenants/{tenantId}/calendars`
- `POST /tenants/{tenantId}/calendars`
- `GET /tenants/{tenantId}/calendars/{calendarId}`
- `DELETE /tenants/{tenantId}/calendars/{calendarId}`
- Schedule upsert supports optional `calendarId` assignment.

## UI Behavior

- Calendars page to list, create, and edit calendars.
- Schedule editor allows assigning a calendar.
- Schedule calendar view renders occurrences after calendar filtering.
- Dashboard forecast applies calendar filters when attached to schedules.

## Persistence Strategy

- Tables:
  - `croniq.Calendars` (definition, metadata, mode, timezone, enabled)
  - `croniq.CalendarRules` (rule rows keyed by CalendarId)
- Alternative: rules stored as JSON in `croniq.Calendars` for simpler reads.
- Indices: `(TenantId, EnvironmentTag)`, `(TenantId, Name)`.

## Observability

- Add span tags: `calendar.id`, `calendar.mode`, `calendar.rule_hits`.
- Metrics: skipped occurrences, evaluation latency, cache hit ratio.

## Caching

- Cache calendar definitions per tenant/environment for schedule evaluation.
- Invalidate on create/update/delete via changefeed or TTL.

## Migration Notes

- Triggers without calendars are unchanged.
- Adding CalendarId to schedule responses is backward compatible for v1.

## Constraints & Future Extensions

- Fixed calendar time zones mean a calendar cannot adapt to per-schedule time zones. To bridge the gap, create dedicated calendars per time zone or introduce tenant defaults; a future option is an explicit `InheritTriggerTimeZone` flag.
- Single-calendar assignment blocks cross-calendar composition (ex: separate "holidays" + "maintenance"). Use multiple rules within one calendar now; a future enhancement can add ordered calendar assignments with explicit precedence.
- Guarded evaluation can return no next occurrence for heavily excluded schedules. Increase the guard limits if needed; a future enhancement can allow per-calendar limits or a wider lookahead mode.
- No per-schedule overrides means ad-hoc exclusions require calendar edits. Use DateList rules or schedule-specific calendars today; a future override layer can merge temporary rules ahead of the base calendar.
