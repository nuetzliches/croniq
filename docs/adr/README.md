# Architecture Decision Records

A record of the standing constraints that shape Croniq — the ones no single
file owns, and that someone will reasonably propose changing again.

## What belongs here

Croniq already documents its decisions in three places, and they are the
default. An ADR is the exception, not the new home for everything.

| Where | What it records | Example |
|---|---|---|
| Comments at the decision site | Why *this* code is like this | `api/refresh_cookie.rs`, `api/hardening.rs` |
| `CHANGELOG.md` | Why a change shipped, including the options rejected | the musl artefacts in [#577](https://github.com/nuetzliches/croniq/issues/577) |
| `docs/operations.md` | The operator-visible consequences, as they are *today* | "Where the dashboard keeps its tokens" |

Write an ADR only when **all three** of these hold:

1. **No single file owns it.** The constraint is enforced in several places at
   once, so a reader who finds one of them cannot recover the reasoning.
2. **A plausible alternative exists** that someone will propose again — not a
   detail with one sensible answer.
3. **Reversal is expensive.** Undoing it means touching code, deployments, or
   a published contract, not editing one function.

If only one or two hold, the reasoning belongs in a comment, the changelog
entry, or `operations.md`. A decision that already has a good comment at its
site does not need a second copy here.

## What an ADR is not

**Not a status page.** `operations.md` describes how Croniq behaves now and is
edited whenever that changes. An ADR describes a decision at the moment it was
taken and is **never edited to change its outcome**. When a decision is
reversed, write a new ADR and set the old one to `Superseded by ADR-NNNN`. The
record of having thought otherwise is the point.

**Not a design document.** Planning artefacts (`docs/vue-migration-plan.md`,
`docs/ui-architecture-decision.md`) explore options at length. An ADR is the
one-page residue: the decision, the alternatives that lost, and what it costs.

## Numbering

Same convention as the store migrations, for the same reason: monotonic, four
digits, assigned when the PR opens. When two ADR PRs are in flight at once, the
second to merge renumbers above the first. Numbers are never reused — a
superseded ADR keeps its number and its file.

## Format

Copy [`0000-template.md`](0000-template.md). Keep it to a page. Sections:
Status, Context, Decision, Alternatives considered, Consequences, Enforced by.

`Enforced by` is the section that earns an ADR its keep: it lists every place
the constraint actually lives, so the next reader does not have to reconstruct
it from four files the way this practice was started.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-same-origin-dashboard.md) | The dashboard is served from the same origin as the API | Accepted |
| [0002](0002-single-image-delivery.md) | One image serves the API, the dashboard and MCP | Accepted |
| [0003](0003-react-for-the-dashboard.md) | React + TypeScript for the dashboard | Superseded by 0004 |
| [0004](0004-vue-rebuild-for-the-dashboard.md) | Rebuild the dashboard in Vue 3 | Accepted |
