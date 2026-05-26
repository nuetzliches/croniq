"""Scripted mock Croniq server for a conformance case.

Built on :mod:`pytest_httpserver`. We register one expectation per unique
``(method, path)`` group from the case's ``server_script`` and serve responses
via a callback that walks the per-route hit counter, preferring exact
``match_count`` matches over the fallthrough rule.

The harness also records every received request — body, headers, path,
method — so the case can assert on them afterwards.
"""

from __future__ import annotations

import json
import threading
import time
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from werkzeug.wrappers import Request, Response

if TYPE_CHECKING:
    from pytest_httpserver import HTTPServer

    from tests.conformance.case_spec import ScriptEntrySpec


@dataclass(slots=True)
class RecordedRequest:
    method: str
    path: str
    headers: dict[str, str] = field(default_factory=dict)
    body: bytes = b""

    def json_body(self) -> object | None:
        if not self.body:
            return None
        try:
            return json.loads(self.body.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            return None


class MockServerHarness:
    """Wraps a :class:`pytest_httpserver.HTTPServer` scripted from a case."""

    def __init__(self, httpserver: HTTPServer, script: list[ScriptEntrySpec]) -> None:
        self._httpserver = httpserver
        self._recorded: list[RecordedRequest] = []
        self._lock = threading.Lock()
        self._hits: dict[tuple[str, str], int] = {}
        self._script = script

        # Group rules by (method, path) so we can install one expectation
        # per group and let the callback pick the right rule for the
        # current hit.
        groups: dict[tuple[str, str], list[ScriptEntrySpec]] = {}
        for entry in script:
            key = (entry.method.upper(), entry.path)
            groups.setdefault(key, []).append(entry)

        for (method, path), entries in groups.items():
            # Closure capture by default argument so each handler sees its
            # own (method, path, entries) — not the loop's last value.
            httpserver.expect_request(path, method=method).respond_with_handler(
                lambda req, _m=method, _p=path, _e=entries: self._handle(req, _m, _p, _e)
            )

    @property
    def base_url(self) -> str:
        return self._httpserver.url_for("").rstrip("/")

    @property
    def recorded(self) -> list[RecordedRequest]:
        with self._lock:
            return list(self._recorded)

    def _handle(
        self,
        request: Request,
        method: str,
        path: str,
        entries: list[ScriptEntrySpec],
    ) -> Response:
        # Pull body once — accessing request.data twice is fine, but we want
        # it in our recorded log too.
        body = request.get_data()
        headers = {k.lower(): v for k, v in request.headers.items()}
        with self._lock:
            self._recorded.append(
                RecordedRequest(method=method, path=path, headers=headers, body=body)
            )
            key = (method, path)
            hit = self._hits.get(key, 0) + 1
            self._hits[key] = hit

        entry = next((e for e in entries if e.match_count == hit), None)
        if entry is None:
            entry = next((e for e in entries if e.match_count is None), None)

        if entry is None:
            return Response(
                json.dumps({"error": f"no rule for hit {hit} on {method} {path}"}),
                status=404,
                content_type="application/json",
            )

        respond = entry.respond
        if respond.delay_ms:
            time.sleep(respond.delay_ms / 1000.0)

        body_str = json.dumps(respond.body) if respond.body is not None else ""
        resp = Response(
            body_str,
            status=respond.status,
            content_type="application/json" if respond.body is not None else None,
        )
        for h_name, h_val in respond.headers.items():
            resp.headers[h_name] = h_val
        return resp
