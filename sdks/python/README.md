# Croniq Runner SDK for Python

[![PyPI](https://img.shields.io/pypi/v/croniq-runner.svg)](https://pypi.org/project/croniq-runner/)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in async Python. The SDK polls a Croniq server for work, dispatches your handlers, streams structured logs back, and reports completion — idiomatic `asyncio` + `httpx` + Pydantic v2.

## Install

```sh
pip install croniq-runner
# Optional: OpenTelemetry tracing
pip install "croniq-runner[otel]"
```

Python 3.11+ required (`asyncio.TaskGroup`, `tomllib`).

## Quick start

```python
import asyncio
from croniq_runner import Runner, RunnerOptions

async def hello(ctx):
    ctx.logger.info("hello from %s (attempt %d)", ctx.job_key, ctx.attempt)
    await ctx.log("emitting a structured event", fields={"customer": "acme"})

async def main():
    runner = Runner(RunnerOptions(
        server_url="http://localhost:4000",
        api_key="croniq_...",
        capabilities=["billing"],
        tags=["lang=python", "env=dev"],
        max_inflight=5,
    ))
    runner.add_handler("hello:world", hello)
    await runner.run()

asyncio.run(main())
```

Stop the runner with `Ctrl-C` or by calling `runner.request_drain()` from
another coroutine — in-flight handlers get up to `drain_timeout_ms` to finish
before the loop returns.

## Features

- **`asyncio`-first** — every public coroutine returns awaitably; no sync surface.
- **Pydantic v2 DTOs** mirroring `openapi.yaml` snake_case wire format.
- **Two-tier logging**:
  - `ctx.logger` — standard `logging.Logger` scoped with `execution_id`, `job_key`, `runner_id`, `attempt`.
  - `ctx.log_writer` — streaming channel backed by a bounded `asyncio.Queue` with batching (32 events / 200 ms / max 100 per POST), drained before the ack.
- **Server-side cancellation** — `PollResponse.cancel` is honoured via `ctx.cancellation` (an `asyncio.Event`). Handlers should `await ctx.cancellation.wait()` between checkpoints, or just use `await asyncio.sleep(...)` — the runner cancels the underlying task when the event fires.
- **Lease renewal** — periodic `POST /v1/work/renew` heartbeat for each in-flight execution.
- **Self-registration** — pass `schedule="5m"` to `add_handler` and the runner POSTs to `/v1/jobs/register` on startup.
- **OpenTelemetry** — opt-in via the `[otel]` extra; spans wrap each handler invocation when `opentelemetry-api` is importable. Zero dependency otherwise.

## Capabilities vs Tags

A common pitfall: **don't put implementation details into capabilities**. Capabilities drive server-side job routing (`require`/`prefer` in the Croniqfile). Tags are filter-only — they show up in the UI and operational views but don't influence routing.

| Good capability | Bad capability |
|---|---|
| `billing`, `reporting`, `gpu`, `sandboxed` | `python`, `linux-x64`, `dotnet` |

If your runner is Python-based, that belongs in **tags** (`lang=python`, `platform=linux-x64`), not capabilities — so a future Rust- or .NET-runner with the same business capabilities can take over without rewriting Croniqfile entries.

## Handler API

A handler is any `async def fn(ctx: ExecutionContext) -> None`. The `ctx`
exposes:

| Attribute | Meaning |
|-----------|---------|
| `execution_id` | Server-assigned execution identifier |
| `job_key` | E.g. `"billing:invoice"` |
| `attempt` | 1-based attempt counter (incremented on retry) |
| `metadata` | Raw `dict` from the server (job-specific schema) |
| `timeout` | `datetime.timedelta` declared by the server |
| `runner_id`, `runner_tags` | This runner's identity |
| `cancellation` | `asyncio.Event` — fires on host shutdown or server-initiated cancel |
| `logger` | `logging.Logger` pre-scoped with execution identifiers |
| `log_writer` | Streaming `LogWriter` (created lazily on first access) |

Two ways to control the ack failure message:

```python
from croniq_runner import HandlerError

async def my_handler(ctx):
    if not data_available():
        raise HandlerError("upstream feed unavailable")  # ack.error = "upstream feed unavailable"
```

Any other exception's `str(exc)` is forwarded as the error message.

## Configuration

| Option | Default | Meaning |
|--------|---------|---------|
| `server_url` | `http://localhost:4000` | Croniq server base URL |
| `runner_id` | resolved at start | Stable runner identifier — see resolution order in `_identity.py` |
| `api_key` / `bearer_token` | `None` | Auth header (`ApiKey` preferred when both set) |
| `capabilities` | `[]` | Capabilities advertised to the server |
| `tags` | `[]` | Free-form `key=value` tags |
| `max_inflight` | `5` | Concurrent in-flight executions |
| `poll_timeout_ms` | `35_000` | Per-request long-poll timeout |
| `renew_interval_ms` | `15_000` | Lease-renewal heartbeat interval |
| `drain_timeout_ms` | `30_000` | Wait budget for handlers on shutdown |
| `poll_retry_delay_ms` | `5_000` | Back-off after a failed poll |
| `capacity_backoff_ms` | `500` | Idle delay at `max_inflight` |
| `log_writer` | `LogWriterOptions()` | Streaming-log tunables |

## Streaming logs example

```python
from croniq_runner import LogLevel

async def long_job(ctx):
    async with ctx.log_writer as writer:  # type: ignore[reportInvalidUsage]
        async for line in slow_generator():
            await writer.write(line, level=LogLevel.INFO)
```

You don't actually need `async with` — the runner drains the writer before
the ack regardless. Calling `aclose()` yourself just lets you control *when*
the drain happens (e.g. before a downstream API call that should see the
events first).

## Conformance suite

The Python binding for the [language-agnostic conformance
suite](../conformance/README.md) lives under `tests/conformance/`. Every
case is one `pytest` parameter; run them with:

```sh
pip install -e ".[dev]"
pytest tests/conformance
```

The cases live at `sdks/conformance/cases/*.yaml` and are loaded by file —
adding a new YAML automatically adds a new test.

## Development

```sh
python -m venv .venv && source .venv/bin/activate
pip install -e ".[dev,otel]"
ruff check .
mypy
pytest
```

## Releasing

Releases run via [`.github/workflows/python-sdk-release.yml`](../../.github/workflows/python-sdk-release.yml) and upload to PyPI through Trusted Publishing (OIDC — no API token in the repo).

1. Bump `version = "X.Y.Z"` in [`pyproject.toml`](pyproject.toml).
2. Add a `## [X.Y.Z]` section to [`CHANGELOG.md`](CHANGELOG.md).
3. Commit, push to `main`.
4. Tag and push:
   ```sh
   git tag python-sdk-vX.Y.Z
   git push origin python-sdk-vX.Y.Z
   ```
5. The workflow builds, verifies the tag matches `pyproject.toml`, publishes to PyPI, then attaches the wheel + sdist to a GitHub Release.

One-time PyPI setup (project owner): add a Pending Publisher on [pypi.org](https://pypi.org/manage/account/publishing/) with project `croniq-runner`, owner `nuetzliches`, repository `croniq`, workflow `python-sdk-release.yml`, environment `pypi`.

## License

Dual-licensed under MIT or Apache-2.0, matching the rest of the Croniq
repository.
