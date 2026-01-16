# Schedule Calendars (Quartz-like)

This document describes the planned schedule calendar feature that mirrors Quartz.NET's calendar concept: a separate entity that includes or excludes candidate fire times for schedules.

## Goals

- Provide a tenant-scoped calendar entity that filters schedule occurrences.
- Keep existing trigger definitions as the source of "when to run" and apply calendars as optional filters.
- Support UI calendar views without persisting per-occurrence rows.
- Maintain deterministic evaluation across API, worker host, and UI preview paths.

## Non-goals

- Do not store every future occurrence in persistence.
- Do not add calendar logic to job code; evaluation stays inside the scheduling pipeline.
- Do not introduce external holiday providers in v1.

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
   - Convert the candidate into the calendar time zone.
   - Evaluate enabled rules in SortOrder.
   - Mode = Exclude: any matching rule excludes the candidate.
   - Mode = Include: candidate must match at least one rule.
4. If excluded, advance to the next candidate and repeat.
5. Guard against infinite loops with a maximum skip counter and log warnings when exceeded.
6. Apply StartAtUtc and EndAtUtc bounds after calendar filtering.
7. For @once schedules: if excluded, return no next occurrence and log that the calendar filtered the only candidate.

## API Surface (Planned)

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

## Open Questions

1. Should calendar time zone default to the trigger time zone if omitted?
2. Do we allow multiple calendars per trigger (composition) or single assignment only?
3. What should be the max skip iteration limit per schedule evaluation?
4. Should cron-based calendar rules reuse the trigger scheduler parser or a dedicated evaluator?
