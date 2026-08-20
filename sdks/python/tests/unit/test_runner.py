"""Black-box tests for :class:`Runner` against an httpx mock transport."""

from __future__ import annotations

import asyncio
import json
import re
from collections.abc import Callable

import httpx

from croniq_runner import (
    AuthFailedError,
    HandlerError,
    PollInstanceConflictError,
    Runner,
    RunnerOptions,
    RunnerOwnershipDeniedError,
)
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


async def test_runner_stops_on_poll_403() -> None:
    """A 403 on poll is permanent — the runner must bail, not retry.

    Counterpart to the 409 story: a conflict is transient and retried on the
    poll interval, whereas a 403 says the credential is bound to a different
    runner_id and no retry can change that (issue #437).
    """
    polls = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal polls
        if req.url.path == "/v1/work/poll":
            polls += 1
            return httpx.Response(
                403, json={"error": "runner_id is bound to another credential"}
            )
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=50,
        drain_timeout_ms=500,
        runner_id="r-denied",
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    try:
        await asyncio.wait_for(runner.run(), timeout=2.0)
    except RunnerOwnershipDeniedError as exc:
        assert exc.runner_id == "r-denied"
        assert "DELETE /v1/runners/{id}" in str(exc)
    else:
        raise AssertionError("expected RunnerOwnershipDeniedError")

    assert polls == 1, f"expected exactly 1 poll (403 is fatal), got {polls}"


async def test_runner_stops_after_consecutive_poll_409s() -> None:
    """A streak of 409s is a duplicate deployment — the runner must bail.

    A single conflict is transient (the deposed instance may win its identity
    back) and case 11 pins that it is retried. A sustained one is two
    processes sharing a fixed runner_id, and retrying forever leaves the
    misconfiguration behind a warning that scrolls past (issue #134
    sub-item 1).
    """
    polls = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal polls
        if req.url.path == "/v1/work/poll":
            polls += 1
            return httpx.Response(409, json={"error": "runner instance conflict"})
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=20,
        drain_timeout_ms=500,
        runner_id="r-duplicate",
        max_consecutive_poll_conflicts=3,
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    try:
        await asyncio.wait_for(runner.run(), timeout=2.0)
    except PollInstanceConflictError as exc:
        assert exc.runner_id == "r-duplicate"
        assert exc.consecutive_count == 3
        assert "rotate the runner_id" in str(exc)
    else:
        raise AssertionError("expected PollInstanceConflictError")

    assert polls == 3, f"expected exactly 3 polls (the configured ceiling), got {polls}"


async def test_runner_survives_a_single_poll_401() -> None:
    """One 401 must not be fatal.

    Rotation hands over by installing the new key and giving the old one an
    expiry (server issue #471). A runner that died on a single 401 would turn
    a narrow race around that handover into an outage.
    """
    polls = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal polls
        if req.url.path == "/v1/work/poll":
            polls += 1
            if polls == 1:
                return httpx.Response(401, json={"error": "unauthorized"})
            return httpx.Response(200, json={"work": [], "cancel": []})
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=20,
        drain_timeout_ms=500,
        runner_id="r-rotating",
        max_consecutive_auth_failures=3,
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    task = asyncio.create_task(runner.run())
    await asyncio.sleep(0.2)
    runner.request_drain()
    await asyncio.wait_for(task, timeout=2.0)

    assert polls >= 2, f"expected the runner to poll again after the 401, got {polls}"


async def test_runner_stops_after_consecutive_poll_401s() -> None:
    """A streak of 401s is a credential that is gone.

    The key is read once and never re-read, so retrying presents the same dead
    credential forever: the process stayed up, looked healthy, did nothing,
    and never exited non-zero, so nothing ever restarted it (issue #473).
    """
    polls = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal polls
        if req.url.path == "/v1/work/poll":
            polls += 1
            return httpx.Response(401, json={"error": "unauthorized"})
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=20,
        drain_timeout_ms=500,
        runner_id="r-revoked",
        max_consecutive_auth_failures=3,
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    try:
        await asyncio.wait_for(runner.run(), timeout=2.0)
    except AuthFailedError as exc:
        assert exc.consecutive_count == 3
        assert "Restart the runner" in str(exc)
    else:
        raise AssertionError("expected AuthFailedError")

    assert polls == 3, f"expected exactly 3 polls (the configured ceiling), got {polls}"


async def test_auth_streak_resets_on_non_401() -> None:
    """A 500 says nothing about whether the credential is valid."""
    polls = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal polls
        if req.url.path == "/v1/work/poll":
            polls += 1
            # 401, 500, 401, … — never two 401s in a row, so a ceiling of 2
            # must not trip.
            if polls % 2 == 1:
                return httpx.Response(401, json={"error": "unauthorized"})
            return httpx.Response(500, json={"error": "boom"})
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=20,
        drain_timeout_ms=500,
        runner_id="r-flaky",
        max_consecutive_auth_failures=2,
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    task = asyncio.create_task(runner.run())
    await asyncio.sleep(0.3)
    runner.request_drain()
    await asyncio.wait_for(task, timeout=2.0)

    assert polls >= 4, f"the runner must have kept polling, got {polls}"


def test_max_consecutive_auth_failures_rejects_zero() -> None:
    """0 would make the runner exit on its first 401 — refuse it up front."""
    try:
        RunnerOptions(server_url="https://test.invalid", max_consecutive_auth_failures=0)
    except ValueError as exc:
        assert "max_consecutive_auth_failures" in str(exc)
    else:
        raise AssertionError("expected ValueError")


async def test_poll_conflict_streak_resets_on_non_409() -> None:
    """Only *consecutive* conflicts count towards the ceiling.

    A 500 in between is unrelated to instance ownership — a server restart, a
    proxy hiccup — so it clears the counter rather than letting an unlucky mix
    of failures add up to a fatal error.
    """
    statuses = [409, 500, 409, 200]
    polls = 0

    def handler(req: httpx.Request) -> httpx.Response:
        nonlocal polls
        if req.url.path == "/v1/work/poll":
            status = statuses[min(polls, len(statuses) - 1)]
            polls += 1
            if status == 200:
                return httpx.Response(200, json={"work": [], "cancel": []})
            return httpx.Response(status, json={"error": "nope"})
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=20,
        drain_timeout_ms=500,
        runner_id="r-flaky",
        max_consecutive_poll_conflicts=2,
    )
    transport = httpx.MockTransport(handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    async def stopper() -> None:
        while polls < 4:
            await asyncio.sleep(0.01)
        runner.request_drain()

    await asyncio.wait_for(asyncio.gather(runner.run(), stopper()), timeout=2.0)

    assert polls >= 4, f"expected at least 4 polls, got {polls}"


def test_max_consecutive_poll_conflicts_rejects_zero() -> None:
    """0 would make the runner exit on its first 409 — refuse it up front."""
    try:
        RunnerOptions(server_url="https://test.invalid", max_consecutive_poll_conflicts=0)
    except ValueError as exc:
        assert "max_consecutive_poll_conflicts" in str(exc)
    else:
        raise AssertionError("expected ValueError")


def test_runner_id_property_unavailable_before_run() -> None:
    runner = Runner(RunnerOptions(runner_id="r"))
    try:
        _ = runner.runner_id
    except RuntimeError as exc:
        assert re.search(r"only available after run", str(exc))
    else:
        raise AssertionError("expected RuntimeError")
