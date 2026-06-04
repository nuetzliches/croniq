"""Pytest configuration shared by unit and conformance tests."""

from __future__ import annotations

import socket
import sys
from pathlib import Path

# Ensure ``tests`` is importable as a package when pytest is invoked from
# ``sdks/python/`` (so ``tests.conformance.case_spec`` resolves).
sys.path.insert(0, str(Path(__file__).parent))

# The werkzeug WSGI server (via pytest_httpserver) calls socket.getfqdn() on
# bind to set a cosmetic server_name. That triggers a reverse-DNS lookup which
# hangs for ~30s on GitHub's macOS runners (no PTR record for the loopback
# host), timing out the whole conformance suite. The value is never asserted
# on, so skip the lookup and echo the host back.
def _fast_getfqdn(name: str = "") -> str:
    return name or "localhost"


socket.getfqdn = _fast_getfqdn
