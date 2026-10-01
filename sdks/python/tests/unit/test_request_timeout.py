"""``RunnerOptions.request_timeout_ms`` bounds every non-poll request (#795).

An ack that never returned kept its execution in the in-flight set every poll
reports, and the server renews the lease of everything in that set (#792). The
bound is applied per request, so it holds even for an injected
``httpx.AsyncClient`` built with ``timeout=None``.
"""

from __future__ import annotations

import asyncio
import time

import httpx
import pytest

from croniq_runner import RunnerOptions
from croniq_runner._client import CroniqClient
from croniq_runner._protocol import AckRequest, RenewRequest, WorkEvent


async def _never_answers(request: httpx.Request) -> httpx.Response:
    """A transport that accepts the request and never responds."""
    await asyncio.Event().wait()
    raise AssertionError("unreachable")


def _client(request_timeout_ms: int) -> CroniqClient:
    options = RunnerOptions(server_url="https://test", request_timeout_ms=request_timeout_ms)
    unbounded = httpx.AsyncClient(
        transport=httpx.MockTransport(_never_answers),
        base_url="https://test",
        timeout=None,
    )
    return CroniqClient(options, http=unbounded)


async def test_ack_gives_up_on_a_server_that_never_answers() -> None:
    client = _client(200)
    started = time.monotonic()
    with pytest.raises(TimeoutError):
        await client.ack(
            AckRequest(
                runner_id="r1", execution_id="e1", status="success", duration_ms=1, attempt=1
            )
        )
    assert time.monotonic() - started < 5.0


async def test_renew_and_event_push_are_bounded_too() -> None:
    client = _client(200)
    with pytest.raises(TimeoutError):
        await client.renew(RenewRequest(runner_id="r1", execution_id="e1"))
    with pytest.raises(TimeoutError):
        await client.push_events("e1", [WorkEvent(level="info", message="line")])


def test_default_is_thirty_seconds() -> None:
    assert RunnerOptions(server_url="https://test").request_timeout_ms == 30_000
