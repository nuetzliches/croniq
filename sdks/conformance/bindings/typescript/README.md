# TypeScript binding for the Croniq runner conformance suite

Runs every YAML case in [`sdks/conformance/cases/`](../../cases) against
[`@nuetzliches/croniq-runner`](../../../typescript). One Vitest test per case; the case
filename is the test name.

## Run locally

```sh
cd sdks/conformance/bindings/typescript
npm install            # resolves @nuetzliches/croniq-runner from the local workspace
npm test
```

Set `CRONIQ_CONFORMANCE_DEBUG=1` to dump every request the runner emitted
to the mock server alongside test output.

## How it works

For each YAML case:

1. **Mock server** — a `node:http` server scripted from `server_script`.
   Rules are matched in order; `match_count: N` pins a rule to the Nth
   matching request, otherwise it's the fallthrough.
2. **Runner config** — `runner_config` keys are translated into
   `CroniqRunnerOptions`. `server_url` is pointed at the mock.
3. **Handlers** — each handler's `behavior` is mapped to one of the five
   sentinels (`noop`, `throw`, `sleep`, `log`, `stream_logs`). Handlers
   with a `schedule` are self-registered via `/v1/jobs/register` at startup.
4. **Drive** — the runner is started; if `shutdown_after_ms` is set, the
   runner's `AbortController` is fired after that delay. Otherwise the
   binding polls expectations and exits early once they're satisfied.
5. **Assert** — all expectations in `expectations.http` are checked
   post-hoc: count, headers (subset, case-insensitive), and body
   (subset, with `"*"` wildcard).
