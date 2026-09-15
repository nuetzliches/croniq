# ADR-0007: Every zone is declared, never inherited from the host or the caller

- **Status:** Accepted
- **Date:** 2026-09-15
- **Deciders:** Sebastian Gieseler
- **Related:** issues
  [#426](https://github.com/nuetzliches/croniq/issues/426),
  [#427](https://github.com/nuetzliches/croniq/issues/427),
  [#450](https://github.com/nuetzliches/croniq/issues/450);
  `docs/operations.md` *Which timezone applies where*

## Context

Three things in a Croniqfile carry a timezone: a job's wall-clock schedule
(`every day at 02:00`), a job's `window` / `not_before` / `not_after`
directives, and a `calendar { }` block's rules. Each of them turns a clock
reading into an instant, and getting it wrong does not produce an error — it
produces a job that fires at the wrong time, which is the one failure this
product exists to prevent.

Two questions had to be answered, and the second is the one with a real
alternative.

**Where does an unstated zone come from?** The obvious answer is the host's
`TZ`. It is what `cron` does, it needs no syntax, and it is right on a laptop.
It is also why the same Croniqfile fires at 02:00 in three different instants
across a staging box, a container and a developer's machine — and why nothing
in the repository records which one was meant.

**Whose zone does a calendar's rules read on?** A calendar is consulted by
jobs, so the natural-looking answer is "the job's" — the calendar inherits the
zone of whoever is asking. That is what issue #450 was opened about. The
alternative is that a calendar declares its own zone and every consumer reads
it in that zone, whatever zone the consumer is in.

These are not settled in one place. The resolution order lives in the DSL
compiler, the evaluation lives in the scheduler, the rejection of an unknown
name lives in `validate`, in the API and in the loader, and the effective zone
is surfaced in the dashboard.

## Decision

Croniq resolves every zone from the configuration and never from the host: a
schedule option, then the job's `timezone`, then `defaults { timezone }`, then
UTC. Neither the job's zone nor a calendar's ever falls back to the process's
`TZ`. A `calendar { }` block resolves its own zone — `calendar { timezone }`,
then `defaults { timezone }`, then UTC — and evaluates its rules in it for
every consumer, regardless of the zone of the job doing the consulting. An IANA
name Croniq cannot resolve is an error at every entry point rather than a
silent fallback, and the zone that was actually resolved is reported rather
than left implied.

## Alternatives considered

**Fall back to the host's `TZ`.** Familiar, matches `cron`, needs no syntax.
Rejected because it makes a Croniqfile mean different things in different
environments while looking identical in review, and the difference only shows
up as a job firing an hour or eight hours off. UTC as the final fallback is
worse for a laptop and better for everything else: it is wrong in a way that is
the same everywhere and visible in the reported zone.

**A calendar inherits the consuming job's zone.** The reading most people
expect, and locally the most convenient — a job in `America/New_York` asking
about "business days" probably means New York business days. Rejected because
it makes a calendar mean a different set of instants per consumer, so
`GET /v1/calendars` and the dashboard cannot say what zone a calendar is in,
and a shared calendar stops being shared. "This holiday calendar is Austrian"
has to hold for every job that references it, including a job in New York.

**Treat a missing zone on a wall-clock job as an error.** Tempting, because
UTC is a guess. Rejected: it would reject Croniqfiles that work today, and the
common case — a fleet that is entirely UTC — would have to say so on every job.
`croniq validate` warns instead, naming the job and saying its rules are read as
UTC, and exit stays `0`.

**Accept an unknown IANA name and fall back to UTC.** Rejected at every entry
point. A typo in a zone name is not a smaller problem than a typo in a cron
expression, and a silent fallback converts it into a schedule that runs at the
wrong time forever. The one deliberate exception is a
`calendar_definitions.timezone` row written before that column was validated:
it is logged at `WARN` and read as UTC, because pausing every job that consults
the calendar is a worse answer to old data.

## Consequences

- A job in one zone consulting a calendar in another asks about the
  *calendar's* day. `report:nightly` at 22:00 New York time against a
  `Europe/Vienna` calendar is asking about the Vienna day, which is already
  tomorrow — so Friday 22:00 in New York is Saturday in Vienna and the gate
  stays shut. This is correct and it surprises people. It is worked through in
  `docs/operations.md` for exactly that reason.
- Each zone follows its own DST switch, so a job and the calendar it consults
  are three weeks out of step every spring. There is no "the" offset to reason
  with; a run that looks an hour off in those weeks is usually this.
- UTC as the final fallback means an unzoned Croniqfile behaves correctly and
  unhelpfully: it does what it says everywhere, which is rarely what a first-time
  user meant by `02:00`. The warning from `croniq validate` is the whole of the
  mitigation.
- Every surface that shows a schedule has to show a zone with it, or it is
  showing an ambiguous time. That is a standing obligation on new UI, not a
  one-off.
- Croniq carries a zone database. Zone rules change by legislation, so a
  released binary can be wrong about a future instant in a way no test here
  will catch.

## Enforced by

- `crates/croniq-config/src/timezone.rs` — resolution and the rejection of an
  unresolvable IANA name.
- `crates/croniq-config/src/compile.rs`, `block_directives.rs` — the resolution
  order for jobs, for `window` / `not_before` / `not_after`, and for
  `calendar { }`, which takes its own rather than a consumer's.
- `crates/croniq-config/src/validate.rs` — the warnings for a wall-clock job
  (#427) and for a calendar with rules (#450) that resolved to UTC from
  nowhere, at exit `0`.
- `crates/croniq-scheduler/src/calendar.rs`, `schedule.rs`, `forecast.rs` — the
  evaluation, where the calendar's zone is the one used.
- `crates/croniq-server/src/api/calendars.rs` (`check_timezone_valid`, the
  `unknown_timezone` `400`) and `api/jobs.rs` (the same rejection on job
  registration) — an unknown zone arriving through the API is refused rather
  than stored.
- `crates/croniq-server/src/loader.rs` — a load fault on an unknown zone, and
  the `WARN`-and-UTC exception for a pre-validation
  `calendar_definitions.timezone` row.
- `ui/app/components/JobDetail.vue` and `ui/app/pages/CalendarsView.vue` — the
  effective zone beside the next fire, and per calendar (`UTC` when unset), so
  the resolved answer is visible rather than implied.

## What this ADR does not say

It does not say which zone anything *should* be in. That is the operator's
choice and Croniq has no opinion beyond making the choice explicit.

It also says nothing about the zone a timestamp is *displayed* in. Stored
instants are UTC and the dashboard renders them in the browser's zone; that is
a presentation decision that can change without touching when anything fires.
