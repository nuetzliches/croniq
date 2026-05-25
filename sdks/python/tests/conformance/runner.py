"""Orchestrator for a single conformance case.

Starts the scripted mock server, wires the SDK against it, drives
``Runner.run`` until the case's expectations are satisfied (or the
``duration_max_ms`` deadline elapses), then asserts the recorded HTTP traffic.
"""

from __future__ import annotations

import asyncio
import contextlib
import time
from typing import TYPE_CHECKING

from croniq_runner import Runner, RunnerOptions
from tests.conformance.body_matcher import match_body
from tests.conformance.handler_sentinels import apply_to as apply_handlers
from tests.conformance.mock_server import MockServerHarness, RecordedRequest

if TYPE_CHECKING:
    from pytest_httpserver import HTTPServer

    from tests.conformance.case_spec import CaseSpec, ExpectationsSpec


async def run_case(httpserver: HTTPServer, spec: CaseSpec) -> None:
    mock = MockServerHarness(httpserver, spec.server_script)
    options = _build_options(spec, mock.base_url)
    runner = Runner(options)
    apply_handlers(runner, spec.handlers)

    deadline_s = (spec.expectations.duration_max_ms or 5000) / 1000.0
    start = time.monotonic()

    # Optional binding directive: cancel the runner partway through.
    cancel_at = (
        start + spec.shutdown_after_ms / 1000.0 if spec.shutdown_after_ms else None
    )

    run_task = asyncio.create_task(runner.run())

    has_max = any(e.max_count is not None for e in spec.expectations.http)

    try:
        while True:
            now = time.monotonic()
            elapsed = now - start
            if elapsed >= deadline_s:
                break

            if cancel_at is not None and now >= cancel_at and not runner._drain_event.is_set():
                runner.request_drain()
                cancel_at = None

            # max_count is a "ceiling over a time window" assertion — we
            # MUST let the full duration_max_ms elapse before checking,
            # otherwise a runner that violates the ceiling after our early
            # exit would pass trivially. Mirrors the .NET binding's
            # ExpectationsAreMet logic.
            if not has_max and _expectations_met(spec.expectations, mock.recorded):
                break

            await asyncio.sleep(0.05)
    finally:
        runner.request_drain()
        # Wait for the runner to wind down (drain + ack). Bound by a
        # post-deadline grace window so a misbehaving handler can't pin
        # the test forever.
        try:
            await asyncio.wait_for(run_task, timeout=2.0)
        except TimeoutError:
            run_task.cancel()
            with contextlib.suppress(asyncio.CancelledError, Exception):
                await run_task

    _assert_expectations(spec, mock.recorded)


def _build_options(spec: CaseSpec, server_url: str) -> RunnerOptions:
    cfg = spec.runner_config
    opts = RunnerOptions(server_url=server_url)
    if cfg.runner_id is not None:
        opts.runner_id = cfg.runner_id
    if cfg.runner_id_prefix is not None:
        opts.runner_id_prefix = cfg.runner_id_prefix
    opts.capabilities = list(cfg.capabilities)
    opts.tags = list(cfg.tags)
    if cfg.max_inflight is not None:
        opts.max_inflight = cfg.max_inflight
    if cfg.api_key is not None:
        opts.api_key = cfg.api_key
    if cfg.bearer_token is not None:
        opts.bearer_token = cfg.bearer_token
    if cfg.poll_timeout_ms is not None:
        opts.poll_timeout_ms = cfg.poll_timeout_ms
    if cfg.renew_interval_ms is not None:
        opts.renew_interval_ms = cfg.renew_interval_ms
    if cfg.drain_timeout_ms is not None:
        opts.drain_timeout_ms = cfg.drain_timeout_ms
    if cfg.poll_retry_delay_ms is not None:
        opts.poll_retry_delay_ms = cfg.poll_retry_delay_ms
    if cfg.capacity_backoff_ms is not None:
        opts.capacity_backoff_ms = cfg.capacity_backoff_ms
    return opts


def _expectations_met(
    expectations: ExpectationsSpec, recorded: list[RecordedRequest]
) -> bool:
    for ex in expectations.http:
        # If any expectation declares max_count, the case must wait the
        # full deadline (see comment in run_case).
        if ex.max_count is not None:
            return False
        matching = sum(
            1 for r in recorded if r.method.upper() == ex.method.upper() and r.path == ex.path
        )
        if ex.exact_count is not None and matching < ex.exact_count:
            return False
        if ex.min_count is not None and matching < ex.min_count:
            return False
    return True


def _assert_expectations(spec: CaseSpec, recorded: list[RecordedRequest]) -> None:
    for ex in spec.expectations.http:
        matches = [
            r for r in recorded if r.method.upper() == ex.method.upper() and r.path == ex.path
        ]
        if ex.exact_count is not None:
            assert len(matches) == ex.exact_count, (
                f"{ex.method} {ex.path}: expected exact_count={ex.exact_count}, "
                f"got {len(matches)}"
            )
        if ex.min_count is not None:
            assert len(matches) >= ex.min_count, (
                f"{ex.method} {ex.path}: expected min_count={ex.min_count}, "
                f"got {len(matches)}"
            )
        if ex.max_count is not None:
            assert len(matches) <= ex.max_count, (
                f"{ex.method} {ex.path}: expected max_count={ex.max_count}, "
                f"got {len(matches)}"
            )

        if ex.headers and matches:
            first = matches[0]
            for name, expected in ex.headers.items():
                lname = name.lower()
                assert lname in first.headers, (
                    f"{ex.method} {ex.path}: missing header '{name}'"
                )
                actual = first.headers[lname]
                if expected == "*":
                    assert actual, f"{ex.method} {ex.path}: header '{name}' was empty"
                else:
                    assert actual == expected, (
                        f"{ex.method} {ex.path}: header '{name}' expected '{expected}', got '{actual}'"
                    )

        if ex.body_match is not None and matches:
            body = matches[0].json_body()
            err = match_body(ex.body_match, body)
            assert err is None, f"{ex.method} {ex.path}: body mismatch — {err}"
