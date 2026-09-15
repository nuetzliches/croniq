# ADR-0009: Runner SDK behaviour is defined by one wire-level suite, not per language

- **Status:** Accepted
- **Date:** 2026-09-15
- **Deciders:** Sebastian Gieseler
- **Related:** issues
  [#176](https://github.com/nuetzliches/croniq/issues/176),
  [#287](https://github.com/nuetzliches/croniq/issues/287),
  [#437](https://github.com/nuetzliches/croniq/issues/437);
  [ADR-0008](0008-runner-identity-binds-first-writer-wins.md),
  `sdks/conformance/README.md`

## Context

Croniq ships six runner SDKs — Rust, Go, Python, TypeScript, Java and .NET —
and the runner is not a thin HTTP client. It is a loop with behaviour that the
server depends on and that `openapi.yaml` cannot express:

- a cancellation arriving on a poll response has to be acked as a *failure*,
  not dropped (cases 04 / 04a);
- log events have to be flushed **before** the ack, or a completed run is
  missing its last lines (case 09);
- a `409` on a poll is a deposed instance that may legitimately win its
  identity back, so it retries — up to a ceiling (cases 11, 16);
- a `403` is permanent and has to stop the runner (case 15, ADR-0008);
- a `401` budget resets on success and clears the conflict streak (cases 17–19);
- hostile identifiers from the server are rejected rather than echoed
  (cases 13, 14).

Each of those is a rule about *what requests appear on the wire, in what
order*. Six independent test suites means six independent readings of the same
rule, and a divergence between them is invisible until an operator reports that
their Python runner loses the tail of its logs and their Go runner does not.

The problem is worse than drift. Every one of those rules was discovered, not
designed — they are what the issues above are about — and an SDK written
afterwards has no way to know they exist. The author's tests will test what the
author thought of.

## Decision

The wire-level behaviour of a Croniq runner is defined once, in
`sdks/conformance/`, as YAML cases that script a mock Croniq server and assert
the resulting request stream — method, path, body, ordering and count. Every
runner SDK, present or future, passes that same bundle; a language earns its
place by supplying a thin binding that loads the cases, stands up the mock, and
runs the real SDK against it. The cases live beside `openapi.yaml` rather than
inside any SDK, they are schema-checked in CI, and a change to them runs
against every language.

## Alternatives considered

**Per-SDK test suites, with the Rust SDK as the written reference.** What the
project did before, and cheaper per SDK. Rejected because a written reference is
a reference nobody diffs against. The rules that matter are ordering rules, and
prose describing an ordering is exactly the thing five authors read five ways.

**A shared suite written in one language, with the other SDKs driven through a
subprocess or FFI.** Less per-language scaffolding than a binding each.
Rejected: it makes the harness the thing under test, and it tests each SDK
through an adapter no user of that SDK will ever use. The binding approach
configures the *real* SDK against a *real* HTTP server, which is the shape the
behaviour has in production.

**Contract tests generated from `openapi.yaml`.** Attractive because the spec
already exists. Rejected because the spec describes requests and responses, and
none of the rules above is about a single request — "flush before ack" and
"retry to a ceiling" are properties of a sequence. A generated suite would
cover the half that never broke.

**Assert against a real `croniq-server` instead of a mock.** More realistic and
it would catch server-side drift too. Rejected for the cases: a mock can be
scripted into the states that matter — a `403` on every poll, a `409` ceiling,
a hostile identifier — which a real server will not produce on demand, and
those states are the whole point. The real server is exercised by the e2e
suites instead.

## Consequences

- **Six bindings to keep alive.** Each is a small adapter, and each is another
  thing a toolchain upgrade can break: six CI workflows carry a conformance job
  and a schema-validation step against the case schemas.
- **A new rule is a six-language change.** Adding a case is cheap; making six
  SDKs pass it is not, and a case merged before the SDKs are ready turns every
  SDK's CI red at once. New cases land with their implementations.
- **The suite can only see the wire.** How a fatal `403` is *reported* — an
  exception type, an error return, a log line — is language-specific and stays
  in each SDK's own tests. Case 15 can assert that exactly one poll happened;
  it cannot assert that the message named the fix.
- **Cases assert counts against timing windows.** Case 15 proves "fatal" by
  capping the polls inside a two-second window at one. That is the only
  observable proof available, and it is a shape that can flake on a loaded CI
  runner in a way a pure assertion would not.
- **The Rust SDK is the exception, and it is the reference.** It runs the
  trigger (producer) cases in `tests/trigger_conformance.rs`, but not the runner
  (consumer) bundle — its loop is covered by its own unit tests instead. So the
  implementation the cases were derived from is the one place they are not the
  gate. That is a gap, not a decision, and it is the first thing to close if a
  divergence ever shows up in Rust.

## Enforced by

- `sdks/conformance/cases/` (the runner loop) and `cases-trigger/` (the
  producer side, issue #287) — the cases themselves, beside `openapi.yaml`.
- `sdks/conformance/schema/case-schema.json` and `trigger-case-schema.json` —
  validated in CI by every SDK workflow, so a malformed case fails before it
  reaches a binding.
- The bindings: `sdks/conformance/bindings/typescript/`, `sdks/go/conformance/`,
  `sdks/python/tests/conformance/`, `sdks/java/conformance-tests/`,
  `sdks/dotnet/tests/Croniq.Runner.Sdk.Conformance.Tests/`, and
  `crates/croniq-runner-sdk/tests/trigger_conformance.rs` for the producer
  cases.
- `.github/workflows/{go,python,typescript,java,dotnet}-sdk-ci.yml` — each
  treats `sdks/conformance/**` as a path that triggers its own build, so a case
  change cannot merge having been tested against one language.
- `sdks/conformance/README.md` — the case format and how a new language is
  wired up, which is what makes "present or future" a claim rather than a hope.

## What this ADR does not say

It does not make the SDKs identical. Naming, error types, concurrency
primitives and packaging are each language's own business; the suite constrains
what goes on the socket, not what the API looks like to the person using it.

It also says nothing about feature parity — whether every SDK has, say, a
trigger client or structured log helpers. That is tracked per feature, usually
as follow-up issues per language, and an SDK missing a feature passes the cases
for the features it has.
