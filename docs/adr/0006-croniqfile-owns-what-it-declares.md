# ADR-0006: The Croniqfile owns what it declares; the API adopts rather than edits

- **Status:** Accepted
- **Date:** 2026-09-15
- **Deciders:** Sebastian Gieseler
- **Related:** [ADR-0001](0001-same-origin-dashboard.md), README *DSL vs. API:
  Source of Truth*, `docs/operations.md` *Reload vs. restart*

## Context

Croniq has two ways to define a job, a schedule or a calendar: the Croniqfile,
which is a file in someone's repository and is reloaded on `SIGHUP`, `--watch`
or `POST /v1/admin/reload-config`; and the API, which writes to the store and is
what the dashboard, the CLI and every SDK talk to.

Both are load-bearing. The Croniqfile is the reason Croniq fits a deployment
pipeline at all — a scheduler whose definitions live only in a database is one
that cannot be reviewed, diffed or rolled back with the service it schedules.
The API is the reason an operator can pause a job at 03:00 without opening an
editor and waiting for a deploy.

The two overlap on the same rows, and the overlap is not symmetric. A reload
re-reads the whole file and reapplies it. Anything the API wrote to a row the
file also declares is therefore **not durable**: it survives exactly until the
next reload, which may be a `--watch` save seconds later or a restart weeks
later. The failure mode is the worst shape a mutation can have — it appears to
work, the read-back confirms it, and it silently reverts at a time unrelated to
the edit.

Read is the easy half and was never in question: `GET /v1/jobs`,
`GET /v1/schedules` and `GET /v1/calendars` return the union of both sources,
each row tagged `managed_by: "dsl"` or `"api"`. Write is the decision.

## Decision

The Croniqfile owns every resource it declares. A write through the API to a
`managed_by: "dsl"` row is refused with `409 Conflict`, and the refusal names
the way forward: the `adopt` URL for that resource and the policy flag that
enables it. Where an operator genuinely wants the row to leave the file's
ownership, `POST …/adopt` copies it into the store with a fresh UUID and
`managed_by="api"`, and records the key in `dsl_adoptions` so the loader skips
it on every later reload. `POST …/unadopt` reverses that, and the next reload
reinstates the file's definition. Adoption is server-wide, opt-in and
default-off (`policy { dsl_adopt_on_mutate true }`).

## Alternatives considered

**Last write wins: let the API edit DSL rows.** The smallest amount of code and
the least surprising thing to click. Rejected because it makes the API's success
response a lie with a delayed fuse: the edit is real until the next reload, and
nothing at the moment of editing can say when that is. A mutation that reverts
on someone else's `git push` is worse than one that is refused.

**The file wins, always: no adoption at all.** Also coherent, and it was the
behaviour before adoption existed. Rejected because it makes one real situation
unreachable without editing the file — a job being migrated *out* of the
Croniqfile into API management, which is exactly what a team does when a job
stops being infrastructure and starts being something an operator tunes. The
workaround is to delete it from the file and recreate it through the API, which
loses the job's key for the length of one reload and with it its state row.

**Merge on reload: treat the file as a base and API writes as an overlay.** The
most powerful option and the one that keeps both surfaces fully live. Rejected
on cost and on legibility: it needs per-field provenance for every resource, a
conflict rule for every field, and an answer to "the file changed the field my
overlay also changes" that an operator can predict. The product would gain a
merge semantics that has to be documented, and no one asked for it.

**Adoption on by default.** Rejected separately from adoption itself. The flag
is server-wide, and a deployment that never wanted API writes to escape the
file should not acquire that ability by upgrading. Default-off means an upgrade
changes no behaviour at all.

## Consequences

- The dashboard has a disabled state that has to be explained rather than just
  greyed out, on every edit affordance of every DSL-managed row. It carries the
  file's ownership as a badge and puts the reason in the refusal, in the
  server's own words.
- `dsl_adoptions` is state that exists only to record an absence — the loader
  reads it to decide what *not* to load. It is easy to forget when reasoning
  about why a Croniqfile definition is not taking effect, and "adopted" is the
  first thing to check when a file edit appears to do nothing.
- Adoption gives a resource a new identity: a fresh UUID, and `managed_by`
  flipped. Anything that stored the synthetic `dsl:{name}` id for a calendar
  has a dangling reference afterwards.
- Two sources of truth remain two sources of truth. Every read path joins them,
  every write path branches on them, and every new resource type has to answer
  the ownership question again before it ships.
- The refusal is a `409` on a valid, authorised request, which reads as a
  server error to a client that does not know the model. This is why the error
  body carries the adopt URL and the flag name rather than a bare message.

## Enforced by

- `crates/croniq-server/src/api/jobs.rs`, `api/calendars.rs`,
  `api/schedules.rs` — the `409` on a DSL-managed row, the `adopt`/`unadopt`
  endpoints, and the `managed_by` tag on every read.
- `crates/croniq-server/src/loader.rs` and `reload.rs` — the loader consults
  `dsl_adoptions` and skips adopted keys, which is what makes an adoption
  outlive the next reload.
- `crates/croniq-store/src/migrations/003_definitions.sql` — `managed_by` and
  the `dsl_adoptions` table; `Store::is_adopted` is what the loader asks.
- `crates/croniq-server/src/api/jobs.rs` (`mod tests`) and
  `crates/croniq-server/tests/calendars.rs` (*Phase 2: adoption flow*) — the
  `409` while the policy is off, the adoption record written when it is on, the
  `409` on unadopting something that was never adopted, and an unadopt
  restoring the file's definition to the read surface.
- `ui/app/components/JobDetail.vue` and `CalendarDetail.vue` — the `dsl` badge,
  the disabled edit affordances, and the adopt action; the refusal is rendered
  in the server's wording rather than restated.
- `crates/croniq-mcp/src/tools.rs` — the MCP surface answers the same way, so
  an agent editing a job meets the same refusal a person does.
- `openapi.yaml` — `managed_by` and the adopt endpoints are part of the
  published contract.

## What this ADR does not say

Nothing here decides *which* resources belong in a Croniqfile. Alert channels,
API clients and auth settings are boot-only configuration with no API write
path at all, and that is a different decision — see `docs/operations.md`,
*Reload vs. restart*.

It also says nothing about what a reload applies. The set of blocks a reload
re-reads (jobs, calendars, triggers, `policy { }`) versus the ones read only at
boot is an operational contract documented where operators look for it, not a
standing constraint spread across the codebase.
