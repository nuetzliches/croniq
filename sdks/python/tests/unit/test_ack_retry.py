"""Ack retry (review 2026-10-24, K-1), drain-timeout ack (K-4), poll jitter (K-9)."""

from __future__ import annotations

import asyncio
import json
from collections.abc import Callable

import httpx
import pytest

from croniq_runner import Runner, RunnerOptions
from croniq_runner import _runner as runner_module
from croniq_runner._client import CroniqClient

_WORK = {
    "work": [
        {
            "execution_id": "e1",
            "job_key": "job:a",
            "fire_at": "2026-10-24T10:00:00Z",
            "attempt": 1,
            "metadata": {},
            "timeout": "5m",
        }
    ],
    "cancel": [],
}


@pytest.fixture(autouse=True)
def _fast_ack_retries(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(runner_module, "_ACK_RETRY_DELAYS_S", (0.01, 0.02))


def _runner(
    ack: Callable[[int], httpx.Response],
    acks: list[dict],
    **overrides: int,
) -> Runner:
    """Runner whose first poll assigns ``e1``; ``ack(n)`` answers the n-th ack."""
    polls = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal polls
        if req.url.path == "/v1/work/poll":
            polls += 1
            return httpx.Response(200, json=_WORK if polls == 1 else {"work": [], "cancel": []})
        if req.url.path == "/v1/work/ack":
            acks.append(json.loads(req.content))
            return ack(len(acks))
        return httpx.Response(200, json={})

    settings: dict[str, int] = {
        "poll_timeout_ms": 500,
        "poll_retry_delay_ms": 100,
        "capacity_backoff_ms": 20,
        "drain_timeout_ms": 1000,
        **overrides,
    }
    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        runner_id="r-test",
        **settings,
    )
    http = httpx.AsyncClient(base_url=options.server_url, transport=httpx.MockTransport(handler))
    return Runner(options, client=CroniqClient(options, http=http))


async def _run_until_idle(runner: Runner, acks: list[dict], settle: float = 0.3) -> None:
    async def stopper() -> None:
        for _ in range(100):
            await asyncio.sleep(0.02)
            if acks:
                break
        # Leave room for every retry the runner might still send.
        await asyncio.sleep(settle)
        runner.request_drain()

    await asyncio.gather(runner.run(), stopper())


async def _noop(_ctx: object) -> None:
    return None


async def test_ack_5xx_then_200_sends_two_acks() -> None:
    acks: list[dict] = []
    runner = _runner(lambda n: httpx.Response(503 if n == 1 else 200, json={}), acks)
    runner.add_handler("job:a", _noop)
    await _run_until_idle(runner, acks)
    assert len(acks) == 2
    assert all(a["execution_id"] == "e1" and a["status"] == "success" for a in acks)


async def test_ack_4xx_is_not_retried() -> None:
    acks: list[dict] = []
    runner = _runner(lambda _n: httpx.Response(409, json={}), acks)
    runner.add_handler("job:a", _noop)
    await _run_until_idle(runner, acks)
    assert len(acks) == 1


async def test_ack_retries_stop_after_three_attempts(caplog: pytest.LogCaptureFixture) -> None:
    acks: list[dict] = []
    runner = _runner(lambda _n: httpx.Response(500, json={}), acks)
    runner.add_handler("job:a", _noop)
    with caplog.at_level("WARNING", logger="croniq_runner.runner"):
        await _run_until_idle(runner, acks)
    assert len(acks) == 3
    levels = [r.levelname for r in caplog.records if "ack" in r.getMessage()]
    assert levels == ["WARNING", "WARNING", "ERROR"]


async def test_ack_transport_error_is_retried() -> None:
    acks: list[dict] = []

    def ack(n: int) -> httpx.Response:
        if n == 1:
            raise httpx.ConnectError("connection reset")
        return httpx.Response(200, json={})

    runner = _runner(ack, acks)
    runner.add_handler("job:a", _noop)
    await _run_until_idle(runner, acks)
    assert len(acks) == 2


async def test_execution_stays_inflight_until_the_retried_ack_lands() -> None:
    acks: list[dict] = []
    inflight_during_retry: list[list[str]] = []
    holder: dict[str, Runner] = {}

    def ack(n: int) -> httpx.Response:
        inflight_during_retry.append(holder["r"].inflight)
        return httpx.Response(503 if n == 1 else 200, json={})

    runner = _runner(ack, acks)
    holder["r"] = runner
    runner.add_handler("job:a", _noop)
    await _run_until_idle(runner, acks)
    assert inflight_during_retry == [["e1"], ["e1"]]
    assert runner.inflight == []


async def test_drain_timeout_still_delivers_the_failure_ack_with_retry(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # Review 2026-10-24, K-4: the drain used to cancel the whole worker task
    # 200 ms after the hard cancel, killing an ack (and any retry) mid-flight.
    acks: list[dict] = []
    runner = _runner(
        lambda n: httpx.Response(503 if n == 1 else 200, json={}),
        acks,
        drain_timeout_ms=100,
    )
    # Retry delay longer than the old 200 ms window.
    monkeypatch.setattr(runner_module, "_ACK_RETRY_DELAYS_S", (0.4, 0.4))

    started = asyncio.Event()

    async def stuck(ctx: object) -> None:
        started.set()
        await asyncio.sleep(30)

    runner.add_handler("job:a", stuck)

    async def stopper() -> None:
        await asyncio.wait_for(started.wait(), timeout=5)
        runner.request_drain()

    await asyncio.wait_for(asyncio.gather(runner.run(), stopper()), timeout=10)
    assert len(acks) == 2
    assert all(a["status"] == "failure" for a in acks)
    assert acks[0]["error"] == "runner draining"


def test_poll_retry_jitter_stays_within_twenty_percent() -> None:
    samples = [runner_module._jittered(1.0) for _ in range(2000)]
    assert min(samples) >= 0.8
    assert max(samples) <= 1.2
    # Not a constant: the point is to spread a fleet out.
    assert max(samples) - min(samples) > 0.2
