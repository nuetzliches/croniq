"""Transport-security checks applied to the configured base URL.

Both the runner (:class:`~croniq_runner.RunnerOptions`) and the producer
(:class:`~croniq_runner.TriggerClientOptions`) attach the API key as an
``Authorization`` header on every request. Over ``http://`` that credential
travels in cleartext — and because httpx honours ``HTTP_PROXY`` by default it
may traverse an intermediary in the clear as well.

The rule (identical in the .NET, Java, Go and TypeScript SDKs): ``https://``
is always fine, ``http://`` is fine for a loopback host — that is the
``http://localhost:4000`` quickstart path — and ``http://`` against any other
host is refused unless the caller explicitly passes ``allow_insecure_http=True``.
Refusal happens at options-construction time so a misconfiguration fails fast
rather than on the first poll.
"""

from __future__ import annotations

import ipaddress
import logging
from urllib.parse import urlsplit

__all__ = ["is_loopback_host", "validate_server_url"]

_log = logging.getLogger("croniq_runner.security")


def is_loopback_host(host: str | None) -> bool:
    """Return ``True`` for ``localhost``, ``127.0.0.0/8`` and ``::1``.

    ``host`` is expected to be :attr:`urllib.parse.SplitResult.hostname`, which
    already lowercases the host and strips the brackets from the IPv6 literal
    form (``[::1]`` → ``::1``); the brackets are stripped again here so a raw
    host string works too.
    """
    if not host:
        return False
    candidate = host.strip().strip("[]").lower()
    if candidate == "localhost":
        return True
    try:
        return ipaddress.ip_address(candidate).is_loopback
    except ValueError:
        return False


def validate_server_url(
    server_url: str, *, allow_insecure_http: bool, option_name: str
) -> None:
    """Validate a configured base URL, raising on an insecure configuration.

    :param server_url: The configured base URL.
    :param allow_insecure_http: Caller's explicit opt-in to cleartext HTTP.
    :param option_name: Options class name, used to make the error actionable.
    :raises ValueError: if the URL is blank, carries an unsupported scheme, or
        is a non-loopback ``http://`` URL without ``allow_insecure_http=True``.
    """
    if not server_url or not server_url.strip():
        raise ValueError(f"{option_name}.server_url must be a non-empty URL")

    parts = urlsplit(server_url)
    scheme = parts.scheme.lower()

    if scheme == "https":
        return

    if scheme != "http":
        raise ValueError(
            f"{option_name}.server_url {server_url!r} has unsupported scheme "
            f"{parts.scheme!r}; use https:// (or http:// for a loopback host)"
        )

    if is_loopback_host(parts.hostname):
        return

    if not allow_insecure_http:
        raise ValueError(
            f"{option_name}.server_url {server_url!r} uses cleartext http:// with the "
            f"non-loopback host {parts.hostname!r}: the API key would be sent in the "
            "clear on every request, and through any configured HTTP proxy. Use "
            "https://, or pass allow_insecure_http=True to accept that risk explicitly."
        )

    _log.warning(
        "SECURITY: Croniq is configured against the cleartext URL %r with "
        "allow_insecure_http=True. The API key is transmitted in cleartext on every "
        "request and is readable by anyone on the network path (including HTTP "
        "proxies). Use https:// in production.",
        server_url,
    )
