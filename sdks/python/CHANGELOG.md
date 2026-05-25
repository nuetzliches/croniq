# Changelog

All notable changes to the Python runner SDK are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the package adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

### Added

- Initial release of `croniq-runner` for Python 3.11+.
- Async-first runner (`Runner.run`) over `httpx.AsyncClient`.
- Pydantic v2 DTOs mirroring `openapi.yaml` snake_case wire format.
- Streaming `LogWriter` backed by a bounded `asyncio.Queue` with batching
  (32 events / 200 ms / max 100 per POST) and drain-before-ack guarantee.
- Server-side cancellation via `PollResponse.cancel` honoured per-execution.
- Lease-renewal heartbeat at `renew_interval` while a handler is in flight.
- Self-registration via `POST /v1/jobs/register` for handlers declared with
  a `schedule=` argument.
- Authentication: `Authorization: ApiKey <key>` (preferred) or
  `Authorization: Bearer <token>`.
- Conformance binding under `tests/conformance/` driving the language-agnostic
  YAML suite at [`sdks/conformance/cases/`](../conformance/cases) — one pytest
  per case, runs against `pytest-httpserver`.
- Optional OpenTelemetry tracing via the `croniq-runner[otel]` extra; spans
  emitted around each execution when `opentelemetry-api` is importable.
