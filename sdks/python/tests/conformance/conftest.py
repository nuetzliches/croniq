"""Fixtures for the conformance bindings."""

from __future__ import annotations

from collections.abc import Iterator

import pytest
from pytest_httpserver import HTTPServer


@pytest.fixture
def httpserver() -> Iterator[HTTPServer]:
    """A *threaded* mock server, one per case.

    pytest-httpserver's own fixture serves one request at a time, so a
    ``delay_ms`` on one rule stalls every other request behind it. Case 20
    (#795) holds the ack for 5 s and expects polls to keep arriving meanwhile
    — against the single-threaded server whether they did was a race, which
    macOS lost. The mocks of the other bindings already serve concurrently.
    """
    server = HTTPServer(threaded=True)
    server.start()
    try:
        yield server
    finally:
        server.clear()
        if server.is_running():
            server.stop()
