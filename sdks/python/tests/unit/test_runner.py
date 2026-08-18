"""Black-box tests for :class:`Runner` against an httpx mock transport."""

from __future__ import annotations

import asyncio
import json
import re
from collections.abc import Callable

import httpx

from croniq_runner import HandlerError, Runner, RunnerOptions
from croniq_runner._client import CroniqClient


def _build_client(handler: Callable[[httpx.Request], httpx.Response]) -> CroniqClient:
    transport = httpx.MockTransport(handler)
    options = RunnerOptions(server_url="https://test.invalid", api_key="testkey")
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    return CroniqClient(options, http=http)


async def test_runner_acks_success_after_handler_completes() -> None:
    calls: list[tuple[str, dict]] = []
    poll_count = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal poll_count
        body = json.loads(req.content) if req.content else {}
        calls.append((req.url.path, body))
        if req.url.path == "/v1/work/poll":
            poll_count += 1
            if poll_count == 1:
                return httpx.Response(
                    200,
                    json={
                        "work": [
                            {
                                "execution_id": "e1",
                                "job_key": "billing:invoice",
                                "fire_at": "2026-05-23T10:00:00Z",
                                "attempt": 1,
                                "metadata": {},
                                "timeout": "5m",
                            }
                        ],
                        "cancel": [],
                    },
                )
            return httpx.Response(200, json={"work": [], "cancel": []})
        if req.url.path == "/v1/work/ack":
            return httpx.Response(200, json={})
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=100,
        drain_timeout_ms=1000,
        runner_id="r-test",
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    async def my_handler(ctx) -> None:  # noqa: ANN001 — ExecutionContext, late-bound
        pass

    runner.add_handler("billing:invoice", my_handler)

    async def stopper() -> None:
        # Give the runner time to pick up + ack the work.
        for _ in range(20):
            await asyncio.sleep(0.05)
            if any(p == "/v1/work/ack" for p, _ in calls):
                break
        runner.request_drain()

    await asyncio.gather(runner.run(), stopper())

    ack_calls = [c for c in calls if c[0] == "/v1/work/ack"]
    assert len(ack_calls) == 1
    ack_body = ack_calls[0][1]
    assert ack_body["execution_id"] == "e1"
    assert ack_body["status"] == "success"
    assert ack_body["attempt"] == 1
    assert "duration_ms" in ack_body


async def test_runner_acks_failure_when_handler_raises() -> None:
    calls: list[tuple[str, dict]] = []
    poll_count = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal poll_count
        body = json.loads(req.content) if req.content else {}
        calls.append((req.url.path, body))
        if req.url.path == "/v1/work/poll":
            poll_count += 1
            if poll_count == 1:
                return httpx.Response(
                    200,
                    json={
                        "work": [
                            {
                                "execution_id": "e-fail",
                                "job_key": "billing:invoice",
                                "fire_at": "2026-05-23T10:00:00Z",
                                "attempt": 2,
                                "metadata": {},
                                "timeout": "5m",
                            }
                        ],
                        "cancel": [],
                    },
                )
            return httpx.Response(200, json={"work": [], "cancel": []})
        return httpx.Response(200, json={})

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="k",
        poll_timeout_ms=500,
        poll_retry_delay_ms=100,
        drain_timeout_ms=1000,
        runner_id="r-test",
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    async def failing(ctx) -> None:  # noqa: ANN001
        raise HandlerError("upstream gone")

    runner.add_handler("billing:invoice", failing)

    async def stopper() -> None:
        for _ in range(20):
            await asyncio.sleep(0.05)
            if any(p == "/v1/work/ack" for p, _ in calls):
                break
        runner.request_drain()

    await asyncio.gather(runner.run(), stopper())

    acks = [body for path, body in calls if path == "/v1/work/ack"]
    assert len(acks) == 1
    assert acks[0]["status"] == "failure"
    assert acks[0]["error"] == "upstream gone"
    assert acks[0]["attempt"] == 2


async def test_runner_self_registers_job_with_schedule() -> None:
    calls: list[tuple[str, dict]] = []

    def handler(req: httpx.Request) -> httpx.Response:
        body = json.loads(req.content) if req.content else {}
        calls.append((req.url.path, body))
        if req.url.path == "/v1/jobs/register":
            return httpx.Response(201, json={"job_key": "x", "status": "registered"})
        if req.url.path == "/v1/work/poll":
            return httpx.Response(200, json={"work": [], "cancel": []})
        return httpx.Response(200, json={})

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="k",
        capabilities=["a", "b"],
        poll_timeout_ms=200,
        poll_retry_delay_ms=100,
        drain_timeout_ms=500,
        runner_id="r-test",
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    async def noop(ctx) -> None:  # noqa: ANN001
        pass

    runner.add_handler("billing:invoice", noop, schedule="5m", description="desc")

    async def stopper() -> None:
        for _ in range(20):
            await asyncio.sleep(0.05)
            if any(p == "/v1/jobs/register" for p, _ in calls):
                break
        runner.request_drain()

    await asyncio.gather(runner.run(), stopper())

    regs = [body for path, body in calls if path == "/v1/jobs/register"]
    assert len(regs) == 1
    assert regs[0]["job_key"] == "billing:invoice"
    assert regs[0]["schedule"] == "5m"
    assert regs[0]["runner_id"] == "r-test"
    assert regs[0]["capabilities"] == ["a", "b"]
    assert regs[0]["description"] == "desc"


async def test_authorization_header_set_from_api_key() -> None:
    seen_auth: list[str | None] = []

    def handler(req: httpx.Request) -> httpx.Response:
        seen_auth.append(req.headers.get("Authorization"))
        return httpx.Response(200, json={"work": [], "cancel": []})

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="secret123",
        poll_timeout_ms=200,
        poll_retry_delay_ms=100,
        drain_timeout_ms=500,
        runner_id="r-test",
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(
        base_url=options.server_url,
        transport=transport,
        headers={"Authorization": "ApiKey secret123"},
    )
    runner = Runner(options, client=CroniqClient(options, http=http))

    async def stopper() -> None:
        await asyncio.sleep(0.1)
        runner.request_drain()

    await asyncio.gather(runner.run(), stopper())

    assert all(h == "ApiKey secret123" for h in seen_auth if h is not None)
    assert any(h == "ApiKey secret123" for h in seen_auth)


def test_runner_id_property_unavailable_before_run() -> None:
    runner = Runner(RunnerOptions(runner_id="r"))
    try:
        _ = runner.runner_id
    except RuntimeError as exc:
        assert re.search(r"only available after run", str(exc))
    else:
        raise AssertionError("expected RuntimeError")
