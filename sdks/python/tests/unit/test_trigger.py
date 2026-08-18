"""Wire-level unit tests for :class:`croniq_runner.TriggerClient`.

Covers request shape (snake_case, omission of unset optionals, nested metadata),
response parsing (including the forward-compatible ``deduplicated`` flag), auth
header selection, and error propagation (non-2xx incl. the #299 queue-overflow
``429``). Uses ``httpx.MockTransport`` so no network is touched.
"""

from __future__ import annotations

import json

import httpx
import pytest

from croniq_runner import TriggerClient, TriggerClientOptions, TriggerResult


class _Recorder:
    """Captures the last request and replies with a scripted response."""

    def __init__(self, status: int = 200, body: dict | None = None) -> None:
        self.status = status
        self.body = body if body is not None else {"execution_id": "exec-1", "queued": 1}
        self.request: httpx.Request | None = None
        self.request_json: object | None = None

    def handle(self, request: httpx.Request) -> httpx.Response:
        self.request = request
        self.request_json = json.loads(request.content) if request.content else None
        return httpx.Response(self.status, json=self.body)


def _client(rec: _Recorder, **opts: object) -> TriggerClient:
    http = httpx.AsyncClient(transport=httpx.MockTransport(rec.handle), base_url="https://test")
    options = TriggerClientOptions(server_url="https://test", **opts)  # type: ignore[arg-type]
    return TriggerClient(options, http=http)


async def test_posts_snake_case_body_to_trigger_endpoint() -> None:
    rec = _Recorder(body={"execution_id": "exec-1", "queued": 3})
    client = _client(rec, api_key="croniq_trigger_key")

    result = await client.trigger(
        "billing:invoice-generate",
        metadata={"invoice_id": "inv_42"},
        require=["billing"],
        prefer=["eu-central"],
        timeout="10m",
        idempotency_key="evt-123",
    )

    assert rec.request is not None
    assert rec.request.url.path == "/v1/trigger"
    assert rec.request.method == "POST"
    body = rec.request_json
    assert isinstance(body, dict)
    assert body["job_key"] == "billing:invoice-generate"
    assert body["metadata"] == {"invoice_id": "inv_42"}
    assert body["require"] == ["billing"]
    assert body["prefer"] == ["eu-central"]
    assert body["timeout"] == "10m"
    assert body["idempotency_key"] == "evt-123"

    assert result == TriggerResult("exec-1", 3, False)


async def test_omits_unset_optional_fields() -> None:
    rec = _Recorder(body={"execution_id": "exec-1", "queued": 1})
    client = _client(rec, api_key="k")

    await client.trigger("etl:data-sync")

    # Only job_key on the wire — no metadata/require/prefer/timeout/idempotency_key,
    # and crucially none of them sent as `null`.
    assert rec.request_json == {"job_key": "etl:data-sync"}


async def test_metadata_nested_and_typed_values_preserved() -> None:
    rec = _Recorder(body={"execution_id": "e", "queued": 1})
    client = _client(rec, api_key="k")

    await client.trigger(
        "email:send",
        metadata={"user_id": "u-42", "attempt": 2, "flags": {"urgent": True}},
    )

    body = rec.request_json
    assert isinstance(body, dict)
    # Nested object + typed values (int, bool) survive verbatim — not stringified.
    assert body["metadata"] == {"user_id": "u-42", "attempt": 2, "flags": {"urgent": True}}


async def test_missing_deduplicated_flag_defaults_to_false() -> None:
    # Older servers don't send `deduplicated` at all.
    rec = _Recorder(body={"execution_id": "exec-1", "queued": 0})
    client = _client(rec, api_key="k")

    result = await client.trigger("etl:data-sync")

    assert result.deduplicated is False


async def test_deduplicated_flag_is_surfaced() -> None:
    rec = _Recorder(body={"execution_id": "exec-1", "queued": 0, "deduplicated": True})
    client = _client(rec, api_key="k")

    result = await client.trigger("etl:data-sync", idempotency_key="evt-1")

    assert result.deduplicated is True
    assert result.execution_id == "exec-1"


@pytest.mark.parametrize("status", [400, 404, 429, 500])
async def test_non_2xx_status_raises(status: int) -> None:
    # 400 = oversized idempotency_key, 429 = per-job queue overflow (#299),
    # 500 = server error. All surface as an error, never a phantom success.
    rec = _Recorder(status=status, body={"error": "nope"})
    client = _client(rec, api_key="k")

    with pytest.raises(httpx.HTTPStatusError):
        await client.trigger("some:job")


async def test_blank_job_key_raises_before_sending() -> None:
    rec = _Recorder()
    client = _client(rec, api_key="k")

    with pytest.raises(ValueError, match="job_key"):
        await client.trigger("   ")

    assert rec.request is None  # rejected locally, no round-trip


async def test_apikey_auth_header_sent() -> None:
    rec = _Recorder()
    client = _client(rec, api_key="croniq_producer_only_key")

    await client.trigger("billing:invoice")

    assert rec.request is not None
    assert rec.request.headers["authorization"] == "ApiKey croniq_producer_only_key"


async def test_bearer_auth_header_sent() -> None:
    rec = _Recorder()
    client = _client(rec, bearer_token="tok-123")

    await client.trigger("billing:invoice")

    assert rec.request is not None
    assert rec.request.headers["authorization"] == "Bearer tok-123"


async def test_apikey_takes_precedence_over_bearer() -> None:
    rec = _Recorder()
    client = _client(rec, api_key="key-1", bearer_token="tok-1")

    await client.trigger("billing:invoice")

    assert rec.request is not None
    assert rec.request.headers["authorization"] == "ApiKey key-1"


async def test_no_auth_header_when_unconfigured() -> None:
    rec = _Recorder()
    client = _client(rec)

    await client.trigger("billing:invoice")

    assert rec.request is not None
    assert "authorization" not in rec.request.headers


async def test_owned_client_closed_on_context_exit() -> None:
    async with TriggerClient(TriggerClientOptions(server_url="http://localhost:1")) as client:
        http = client._http
    assert http.is_closed


async def test_injected_client_not_closed() -> None:
    http = httpx.AsyncClient(base_url="http://test")
    async with TriggerClient(TriggerClientOptions(), http=http):
        pass
    assert not http.is_closed
    await http.aclose()
