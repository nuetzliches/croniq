"""Producer-side client for firing Croniq jobs on demand.

Wraps ``POST /v1/trigger`` in an idiomatic async client. Independent of the
:class:`~croniq_runner.Runner` (consumer) side: a pure producer never needs to
poll, and it carries **its own** credentials — triggering requires the
``jobs:trigger`` (or ``admin``) scope, which runner poll keys typically do not
hold. Parity with the .NET ``ICroniqTriggerClient`` / ``CroniqTriggerClient``.
"""

from __future__ import annotations

from dataclasses import dataclass
from types import TracebackType
from typing import Any

import httpx

from croniq_runner._protocol import TriggerRequest, TriggerResponse
from croniq_runner._security import validate_server_url


@dataclass(slots=True)
class TriggerClientOptions:
    """Configuration for a :class:`TriggerClient`.

    Deliberately separate from :class:`~croniq_runner.RunnerOptions`: the
    trigger endpoint needs the ``jobs:trigger`` (or ``admin``) scope, distinct
    from a runner's poll scopes, so the producer supplies its own credentials.
    """

    server_url: str = "http://localhost:4000"
    """Base URL of the Croniq server.

    ``https://`` is required unless the host is loopback (``localhost``,
    ``127.0.0.0/8``, ``::1``) — the trigger credential is attached to every
    request and would otherwise travel in cleartext. See
    :attr:`allow_insecure_http`.
    """

    api_key: str | None = None
    """API key for ``Authorization: ApiKey <key>``. Takes precedence over bearer."""

    bearer_token: str | None = None
    """Bearer token for ``Authorization: Bearer <token>``."""

    request_timeout_ms: int = 30_000
    """Per-request timeout for trigger calls."""

    allow_insecure_http: bool = False
    """Opt in to a cleartext ``http://`` :attr:`server_url` on a non-loopback host.

    Off by default: without it such a URL is refused at construction time. When
    enabled the SDK still emits one loud startup warning, because the trigger
    credential then travels in cleartext on every call.
    """

    def __post_init__(self) -> None:
        validate_server_url(
            self.server_url,
            allow_insecure_http=self.allow_insecure_http,
            option_name="TriggerClientOptions",
        )


@dataclass(frozen=True, slots=True)
class TriggerResult:
    """Result of an on-demand job trigger (``POST /v1/trigger``)."""

    execution_id: str
    """Identifier of the execution the trigger resolved to."""

    queued: int
    """Server work-queue depth after the trigger was processed."""

    deduplicated: bool = False
    """``True`` when the server coalesced this trigger onto an existing
    execution because the request carried an ``idempotency_key`` it had already
    seen; :attr:`execution_id` then refers to that existing execution. Always
    ``False`` on servers without idempotency-key support (#279)."""


class TriggerClient:
    """Async producer client over ``POST /v1/trigger``.

    Owns the underlying :class:`httpx.AsyncClient` unless one is injected.
    Usable as an async context manager::

        async with TriggerClient(TriggerClientOptions(
            server_url="http://localhost:4000",
            api_key="croniq_...",  # jobs:trigger scope
        )) as client:
            result = await client.trigger("billing:invoice")
            print(result.execution_id, result.queued, result.deduplicated)
    """

    def __init__(
        self, options: TriggerClientOptions, *, http: httpx.AsyncClient | None = None
    ) -> None:
        self._options = options
        self._timeout = httpx.Timeout(options.request_timeout_ms / 1000.0)
        self._http = http or httpx.AsyncClient(
            base_url=options.server_url.rstrip("/"),
            timeout=self._timeout,
        )
        self._owns_client = http is None

    def _auth_headers(self) -> dict[str, str]:
        # ApiKey wins over bearer, matching CroniqClient and the .NET client.
        # Applied per request (not baked into the client) so an injected
        # client can't smuggle in a second Authorization header.
        if self._options.api_key:
            return {"Authorization": f"ApiKey {self._options.api_key}"}
        if self._options.bearer_token:
            return {"Authorization": f"Bearer {self._options.bearer_token}"}
        return {}

    async def __aenter__(self) -> TriggerClient:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        await self.aclose()

    async def aclose(self) -> None:
        if self._owns_client:
            await self._http.aclose()

    async def trigger(
        self,
        job_key: str,
        *,
        metadata: dict[str, Any] | None = None,
        require: list[str] | None = None,
        prefer: list[str] | None = None,
        timeout: str | None = None,
        idempotency_key: str | None = None,
    ) -> TriggerResult:
        """Fire a job immediately.

        The job's registered handler runs on the next eligible runner, exactly
        like a scheduled fire.

        :param job_key: Job key, e.g. ``"billing:invoice"``. Must be non-blank.
        :param metadata: Arbitrary JSON passed to the handler; merged over the
            job's DSL metadata. Keys starting with ``__`` are reserved.
        :param require: Capabilities a runner must have to be assigned this run.
        :param prefer: Capabilities used to prefer runners when several match.
        :param timeout: Execution timeout as a duration string (``"30s"``,
            ``"5m"``); the server default applies when omitted.
        :param idempotency_key: Optional dedup key. Servers with trigger
            idempotency coalesce repeat triggers carrying the same key onto the
            existing execution (see :attr:`TriggerResult.deduplicated`); older
            servers ignore it. Capped at 200 chars server-side.
        :returns: The created (or deduplicated) execution and queue depth.
        :raises ValueError: if ``job_key`` is blank.
        :raises httpx.HTTPStatusError: on any non-2xx response (unknown job,
            oversized idempotency key ``400``, queue-overflow ``429`` (#299),
            server error ``5xx``).
        """
        if not job_key or not job_key.strip():
            raise ValueError("job_key must be a non-empty string")

        request = TriggerRequest(
            job_key=job_key,
            metadata=metadata,
            require=require,
            prefer=prefer,
            timeout=timeout,
            idempotency_key=idempotency_key,
        )
        # exclude_none omits unset optionals so they never reach the wire as
        # `null`; mode="json" preserves nested metadata structure/types.
        body: dict[str, Any] = request.model_dump(mode="json", exclude_none=True)

        resp = await self._http.post(
            "/v1/trigger",
            json=body,
            headers=self._auth_headers(),
            timeout=self._timeout,
        )
        resp.raise_for_status()

        parsed = TriggerResponse.model_validate(resp.json())
        return TriggerResult(
            execution_id=parsed.execution_id,
            queued=parsed.queued,
            deduplicated=parsed.deduplicated,
        )
