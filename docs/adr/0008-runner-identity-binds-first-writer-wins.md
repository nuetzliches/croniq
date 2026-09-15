# ADR-0008: A `runner_id` belongs to the credential that first used it

- **Status:** Accepted
- **Date:** 2026-09-15
- **Deciders:** Sebastian Gieseler
- **Related:** README *Runner identity in the work protocol*;
  `docs/operations.md` *Runner identity ownership*

## Context

Croniq's work protocol is pull-based, and every call in it carries the acting
`runner_id` in the request body: `POST /v1/work/poll`, `…/ack`, `…/renew`,
`…/{execution_id}/events`.

A `runner_id` is a name an operator picks — `shell-runner-vps-prod`,
`backup-runner`. It appears in logs, in the dashboard, in every poll response
and in alert bodies. It is not a secret and was never meant to be one. But
before this decision it was also the only thing the server used to decide
*which* runner was calling, so any holder of a `work:*` scope could name
someone else's runner and act as it: claim the work queued for it, ack its
executions, extend or drop its leases, and push log events into its runs.

That is a real privilege boundary in a product whose selling point is that
runners live inside other people's networks. A team hands a runner key to a
contractor's build agent; the contractor's agent can now name the production
runner. Nothing about the key said otherwise, because the key granted
"`work:*`", not "`work:* as this runner`".

The fix has to work without pre-provisioning. Runners register themselves by
polling; there is no enrolment step, and adding one would break every existing
deployment on upgrade and every autoscaled runner pool afterwards.

## Decision

The server binds a `runner_id` to the credential that first names it in the
work protocol, and refuses later work requests naming that id from any other
credential with `403 Forbidden`. The credential is the owning API client
(`client_id`), not the individual API key, so rotating a key leaves the binding
intact. `…/{execution_id}/events` is addressed by execution rather than by
runner, so it is fenced on the runner that claimed that execution instead.
Nothing is pre-provisioned: the table starts empty and every runner binds
itself on its first poll. Moving an id to a different credential is an explicit
operator action — `DELETE /v1/runners/{id}` releases the binding and the next
poll re-binds. The behaviour is `pull_api { runner_identity_binding strict }`,
the default, and can be set to `off`.

## Alternatives considered

**Pre-provision runners: an id has to be registered to a credential before
use.** The strongest model, and the one a reader reaches for first. Rejected on
what it costs: it breaks every existing deployment at the upgrade, and it
breaks autoscaling permanently — a runner pool that names itself
`worker-<hostname>` would need an API call before each new instance can work,
from something that holds more privilege than the runner does.

**Derive the runner id from the credential instead of accepting it.** No
binding table, no refusals, nothing to release. Rejected because it takes the
name away from the operator. A `runner_id` is how a person finds a runner in
the dashboard, pins a job to it (`assigned_runner_id`) and reads a log line;
`client_7f3a`-shaped names would make every operator-facing surface worse, and
a deployment that shares one key across a fleet could no longer tell its
runners apart at all.

**Last-writer-wins: the most recent credential takes the id.** Cheap, and it
self-heals after a re-key. Rejected because it is not a boundary at all — it
inverts into "whoever polled most recently wins", which is the same hijack with
an extra step, and it makes the incumbent runner's failure depend on a
stranger's timing.

**Do nothing; document that `work:*` keys are fully trusted.** Defensible for a
single-team deployment and it is what `runner_identity_binding off` preserves.
Rejected as the default because the shape Croniq is deployed in — a scheduler
reaching into networks it does not own — is exactly where one key per party is
the normal arrangement.

## Consequences

- **A refusal has to be fatal in the runner, and that is a second decision
  every SDK has to get right.** A runner that retried a `403` on its poll
  interval looked merely *idle* — the diagnostic trap issue #437 closed — so
  the SDKs stop on the first one and surface an error naming the `runner_id`
  and the fix. That is the opposite of the `409` of an instance takeover, where
  a deposed runner may legitimately win its identity back by retrying, and the
  two arrive at the same call site. The distinction is pinned for every
  language by conformance case `15-poll-403-ownership-fatal` (ADR-0009), not by
  each SDK's own judgement.
- **The first poll after an upgrade decides.** The table starts empty, so
  whoever polls first binds the id. Two runners genuinely sharing an id — which
  was always a misconfiguration — now resolves to one of them winning, rather
  than to both limping.
- **A legitimate re-key needs an operator.** Moving a runner to another team's
  client means `DELETE /v1/runners/{id}` first, which also deregisters the
  runner. There is no "transfer" that keeps the registration.
- **Binding is inert where it cannot mean anything.** With no auth configured
  or no store configured there is no credential to bind to and nowhere to
  record it, so the check does not run. A deployment can be running without the
  protection it believes it has, and only its configuration says which.
- **A store failure fails closed**: if the binding cannot be read or written,
  work requests get `503` rather than being waved through. That is a deliberate
  choice against availability — a scheduler that cannot tell who is calling
  should stop handing out work, and runners retry, so it self-heals.
- The refusal happens before the registry is touched, so a stranger's poll
  cannot extend the incumbent's lease, requeue its claims, or fence it out with
  a `409`. That ordering is load-bearing and easy to break in a refactor.

## Enforced by

- `crates/croniq-server/src/api/runner_identity.rs` — `authorize_runner` (the
  `403`, the `503` when the store cannot answer, and the
  `runner.identity_rejected` audit event), `authorize_execution` (the
  execution-scoped fence for `…/{execution_id}/events`), and `release_runner`.
- `crates/croniq-server/src/api/mod.rs` — the call sites, which put
  `authorize_runner` ahead of every registry mutation on `poll`, `ack` and
  `renew`; and `handle_delete_runner`, where `DELETE /v1/runners/{id}` releases
  the binding as well as deregistering.
- `crates/croniq-store/src/traits.rs` (`runner_identity_bind` / `_owner` /
  `_release`) and `migrations/024_runner_identities.sql` — the binding itself,
  implemented per backend so SQLite and Postgres cannot disagree about who owns
  an id.
- `crates/croniq-config/src/block_directives.rs` and `compile.rs` —
  `pull_api { runner_identity_binding }` and the `strict` default;
  `croniq-server/src/reload.rs` treats it as boot-only.
- The audit trail — `runner.identity_rejected`, target = the runner id, which
  is the only place a fenced-out runner is visible from the outside.
- `crates/croniq-runner-sdk/src/client.rs` (`work_endpoint_error` lifting `403`
  out of the transient bucket into `WorkOwnershipDenied`) and `runner.rs` (the
  run loop bailing on the first one) — with
  `sdks/conformance/cases/15-poll-403-ownership-fatal.yaml` holding the other
  four SDKs to the same answer (ADR-0009).

## What this ADR does not say

It does not make `runner_id` a secret. It stays operator-chosen and stays
visible everywhere it was visible before; the binding is about who may *act* as
it, not about who may know it.

It also decides nothing about what a runner is allowed to run once it is
authenticated. Capability matching (`require`, `prefer`) and job pinning
(`assigned_runner_id`) are scheduling concerns, and a correctly bound runner is
still subject to all of them.
