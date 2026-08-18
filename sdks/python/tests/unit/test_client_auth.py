"""Auth-header tests for :class:`croniq_runner._client.CroniqClient`.

The credential is applied per request rather than baked into the underlying
``httpx.AsyncClient``, so it survives an injected client — the documented path
for mTLS, proxies and custom transports. Regression cover for the case where
``CroniqClient(options, http=...)`` sent every runner request unauthenticated.
Uses ``httpx.MockTransport``; no network is touched.
"""

from __future__ import annotations

import httpx

from croniq_runner import RunnerOptions
from croniq_runner._client import CroniqClient
from croniq_runner._protocol import (
    AckRequest,
    PollRequest,
    RegisterJobRequest,
    RenewRequest,
    WorkEvent,
)


class _Recorder:
    """Captures every request and replies with a scripted response."""

    def __init__(self, body: object | None = None) -> None:
        self.body = body if body is not None else {}
        self.requests: list[httpx.Request] = []

    def handle(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        return httpx.Response(200, json=self.body)

    @property
    def last(self) -> httpx.Request:
        assert self.requests, "no request was recorded"
        return self.requests[-1]


def _injected(rec: _Recorder, **headers: str) -> httpx.AsyncClient:
    """An externally supplied client, as a user wiring mTLS or a proxy would."""
    return httpx.AsyncClient(
        transport=httpx.MockTransport(rec.handle),
        base_url="https://test",
        headers=headers,
    )


async def test_injected_client_still_sends_the_configured_api_key() -> None:
    rec = _Recorder(body={"work": []})
    options = RunnerOptions(server_url="https://test", api_key="croniq_injected_key")

    async with CroniqClient(options, http=_injected(rec)) as client:
        await client.poll(PollRequest(runner_id="runner-1"), timeout_ms=1000)

    assert rec.last.headers["authorization"] == "ApiKey croniq_injected_key"


async def test_injected_client_still_sends_the_configured_bearer_token() -> None:
    rec = _Recorder(body={"work": []})
    options = RunnerOptions(server_url="https://test", bearer_token="jwt-token")

    async with CroniqClient(options, http=_injected(rec)) as client:
        await client.poll(PollRequest(runner_id="runner-1"), timeout_ms=1000)

    assert rec.last.headers["authorization"] == "Bearer jwt-token"


async def test_configured_key_overrides_the_injected_clients_own_header() -> None:
    # The dangerous shape: the injected client carries a broader credential of
    # its own. RunnerOptions.api_key must win, so the runner authenticates as
    # the identity the operator configured for it — not as whoever the shared
    # client belongs to.
    rec = _Recorder(body={"work": []})
    options = RunnerOptions(server_url="https://test", api_key="croniq_runner_key")
    http = _injected(rec, Authorization="ApiKey croniq_admin_key")

    async with CroniqClient(options, http=http) as client:
        await client.poll(PollRequest(runner_id="runner-1"), timeout_ms=1000)

    assert rec.last.headers["authorization"] == "ApiKey croniq_runner_key"
    assert "croniq_admin_key" not in rec.last.headers["authorization"]


async def test_api_key_wins_over_bearer_token() -> None:
    rec = _Recorder(body={"work": []})
    options = RunnerOptions(server_url="https://test", api_key="k", bearer_token="t")

    async with CroniqClient(options, http=_injected(rec)) as client:
        await client.poll(PollRequest(runner_id="runner-1"), timeout_ms=1000)

    assert rec.last.headers["authorization"] == "ApiKey k"


async def test_no_credential_configured_sends_no_authorization_header() -> None:
    # Fail closed rather than inventing a header — the server answers 401 and
    # the runner's retry loop reports it.
    rec = _Recorder(body={"work": []})
    options = RunnerOptions(server_url="https://test")

    async with CroniqClient(options, http=_injected(rec)) as client:
        await client.poll(PollRequest(runner_id="runner-1"), timeout_ms=1000)

    assert "authorization" not in rec.last.headers


async def test_every_endpoint_carries_the_credential() -> None:
    # Auth is per request, so each call site has to opt in individually — this
    # asserts none was missed.
    rec = _Recorder(body={})
    options = RunnerOptions(server_url="https://test", api_key="croniq_key")

    async with CroniqClient(options, http=_injected(rec)) as client:
        await client.poll(PollRequest(runner_id="runner-1"), timeout_ms=1000)
        await client.ack(
            AckRequest(runner_id="runner-1", execution_id="exec-1", status="success", attempt=1)
        )
        await client.renew(RenewRequest(runner_id="runner-1", execution_id="exec-1"))
        await client.push_events("exec-1", [WorkEvent(level="info", message="hi")])
        await client.register_job(RegisterJobRequest(job_key="demo:job", schedule="every 5m"))

    paths = [r.url.path for r in rec.requests]
    assert paths == [
        "/v1/work/poll",
        "/v1/work/ack",
        "/v1/work/renew",
        "/v1/work/exec-1/events",
        "/v1/jobs/register",
    ]
    for request in rec.requests:
        assert request.headers["authorization"] == "ApiKey croniq_key", request.url.path


async def test_owned_client_does_not_bake_the_credential_into_default_headers() -> None:
    # The self-built client is constructed without `headers=`; auth reaches the
    # wire only through the per-request path exercised above. Pinning this keeps
    # the owned and injected clients on one code path, so the injected one can't
    # silently drift back to unauthenticated.
    options = RunnerOptions(server_url="https://test", api_key="croniq_owned_key")
    async with CroniqClient(options) as client:
        assert "authorization" not in client._http.headers
