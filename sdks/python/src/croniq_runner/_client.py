"""HTTP client over the Croniq Runner API.

Wraps ``httpx.AsyncClient`` with snake_case JSON encoding and auth header
injection. Per-request timeouts (notably for the long-poll on
``/v1/work/poll``) are explicit on each call so the client-level timeout can
stay generous.
"""

from __future__ import annotations

import logging
from types import TracebackType
from typing import Any
from urllib.parse import quote

import httpx

from croniq_runner._options import RunnerOptions
from croniq_runner._protocol import (
    AckRequest,
    PollRequest,
    PollResponse,
    RegisterJobRequest,
    RegisterJobResponse,
    RenewRequest,
    WorkEvent,
)

_log = logging.getLogger("croniq_runner.client")


class CroniqClient:
    """Async wire client. Owns the underlying :class:`httpx.AsyncClient`."""

    def __init__(self, options: RunnerOptions, *, http: httpx.AsyncClient | None = None) -> None:
        self._options = options
        # 35 s long-poll plus a small head-room; per-call timeouts override.
        default_timeout = httpx.Timeout(
            connect=10.0,
            read=40.0,
            write=10.0,
            pool=10.0,
        )
        # No ``headers=`` here: auth is applied per request instead (see
        # :meth:`_auth_headers`), so an injected client gets the configured
        # credential too.
        self._http = http or httpx.AsyncClient(
            base_url=options.server_url.rstrip("/"),
            timeout=default_timeout,
        )
        self._owns_client = http is None

    @staticmethod
    def _dump(payload: Any) -> dict[str, Any]:
        """Pydantic dump matching the server's snake_case + omit-None convention."""
        return payload.model_dump(mode="json", exclude_none=True)  # type: ignore[no-any-return]

    def _auth_headers(self) -> dict[str, str]:
        """Authorization header for one request. ApiKey wins over bearer.

        Applied per request rather than baked into the ``httpx.AsyncClient``
        at construction. Injecting a client (``http=``) is a documented path
        for mTLS, proxies and custom transports; baking the header in meant an
        injected client carried no credential at all, so every runner request
        went out unauthenticated — and if that client happened to carry its own
        broader ``Authorization``, :attr:`RunnerOptions.api_key` was silently
        ignored and the runner authenticated as somebody else. Per-request
        headers also override any header the injected client sets, so it can't
        smuggle in a second credential. Matches :class:`TriggerClient`.
        """
        if self._options.api_key:
            return {"Authorization": f"ApiKey {self._options.api_key}"}
        if self._options.bearer_token:
            return {"Authorization": f"Bearer {self._options.bearer_token}"}
        return {}

    async def __aenter__(self) -> CroniqClient:
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

    async def poll(self, request: PollRequest, *, timeout_ms: int) -> PollResponse:
        # Long-poll: the server may hold the connection for up to ~poll_timeout
        # before returning an empty body. Allow a small read head-room so a
        # graceful 200 doesn't race the client-side timeout.
        timeout = httpx.Timeout(
            connect=10.0,
            read=timeout_ms / 1000.0 + 5.0,
            write=10.0,
            pool=10.0,
        )
        resp = await self._http.post(
            "/v1/work/poll",
            json=self._dump(request),
            headers=self._auth_headers(),
            timeout=timeout,
        )
        resp.raise_for_status()
        return PollResponse.model_validate(resp.json())

    async def ack(self, request: AckRequest) -> None:
        resp = await self._http.post(
            "/v1/work/ack", json=self._dump(request), headers=self._auth_headers()
        )
        resp.raise_for_status()

    async def renew(self, request: RenewRequest) -> None:
        resp = await self._http.post(
            "/v1/work/renew", json=self._dump(request), headers=self._auth_headers()
        )
        resp.raise_for_status()

    async def push_events(self, execution_id: str, events: list[WorkEvent]) -> None:
        if not events:
            return
        body = [self._dump(ev) for ev in events]
        path = f"/v1/work/{quote(execution_id, safe='')}/events"
        resp = await self._http.post(path, json=body, headers=self._auth_headers())
        resp.raise_for_status()

    async def register_job(self, request: RegisterJobRequest) -> RegisterJobResponse | None:
        resp = await self._http.post(
            "/v1/jobs/register", json=self._dump(request), headers=self._auth_headers()
        )
        resp.raise_for_status()
        # Some server versions return 200 with no body; treat empty as None.
        if not resp.content:
            return None
        try:
            return RegisterJobResponse.model_validate(resp.json())
        except ValueError:
            # Body wasn't JSON / didn't match — non-fatal, we already got the 2xx.
            _log.debug("register_job: unexpected response body (%s)", resp.text[:200])
            return None
