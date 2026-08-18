"""Base-URL transport-security checks (issue #440).

``https://`` is always accepted; ``http://`` only for a loopback host (the
``http://localhost:4000`` quickstart path) or behind an explicit
``allow_insecure_http=True``, which additionally logs one loud warning.
Enforced at options-construction time for both the runner and the producer.
"""

from __future__ import annotations

import logging

import pytest

from croniq_runner import RunnerOptions, TriggerClientOptions
from croniq_runner._security import is_loopback_host

ACCEPTED_URLS = [
    "https://croniq.example.com",
    "https://croniq.example.com:4000",
    "http://localhost:4000",
    "http://LOCALHOST:4000",
    "http://127.0.0.1:4000",
    "http://127.10.20.30:4000",
    "http://[::1]:4000",
]

REJECTED_URLS = [
    "http://croniq.example.com",
    "http://croniq.example.com:4000",
    "http://10.0.0.5:4000",
    "http://[2001:db8::1]:4000",
]


@pytest.mark.parametrize("url", ACCEPTED_URLS)
def test_runner_options_accepts_secure_or_loopback_url(url: str) -> None:
    assert RunnerOptions(server_url=url).server_url == url


@pytest.mark.parametrize("url", ACCEPTED_URLS)
def test_trigger_options_accepts_secure_or_loopback_url(url: str) -> None:
    assert TriggerClientOptions(server_url=url).server_url == url


@pytest.mark.parametrize("url", REJECTED_URLS)
def test_runner_options_rejects_non_loopback_cleartext_url(url: str) -> None:
    with pytest.raises(ValueError, match="allow_insecure_http") as exc:
        RunnerOptions(server_url=url)
    # Actionable: names the offending URL and the option that was rejected.
    assert url in str(exc.value)
    assert "RunnerOptions" in str(exc.value)


@pytest.mark.parametrize("url", REJECTED_URLS)
def test_trigger_options_rejects_non_loopback_cleartext_url(url: str) -> None:
    with pytest.raises(ValueError, match="allow_insecure_http") as exc:
        TriggerClientOptions(server_url=url)
    assert url in str(exc.value)
    assert "TriggerClientOptions" in str(exc.value)


def test_default_quickstart_url_still_works() -> None:
    # The documented quickstart default must not regress.
    assert RunnerOptions().server_url == "http://localhost:4000"
    assert TriggerClientOptions().server_url == "http://localhost:4000"


def test_unsupported_scheme_is_rejected() -> None:
    with pytest.raises(ValueError, match="unsupported scheme"):
        RunnerOptions(server_url="ftp://croniq.example.com")


def test_blank_url_is_rejected() -> None:
    with pytest.raises(ValueError, match="non-empty URL"):
        RunnerOptions(server_url="   ")


def test_opt_in_accepts_cleartext_url_and_warns(caplog: pytest.LogCaptureFixture) -> None:
    url = "http://croniq.example.com:4000"

    with caplog.at_level(logging.WARNING, logger="croniq_runner.security"):
        options = RunnerOptions(server_url=url, allow_insecure_http=True)

    assert options.server_url == url
    warnings = [r for r in caplog.records if r.levelno == logging.WARNING]
    assert len(warnings) == 1
    assert "SECURITY" in warnings[0].getMessage()
    assert "cleartext" in warnings[0].getMessage()
    # Assert on the interpolation argument rather than searching the rendered
    # message for a URL substring — the latter trips CodeQL's
    # py/incomplete-url-substring-sanitization heuristic, and this is stricter.
    assert warnings[0].args == (url,)


def test_opt_in_accepts_cleartext_trigger_url_and_warns(
    caplog: pytest.LogCaptureFixture,
) -> None:
    with caplog.at_level(logging.WARNING, logger="croniq_runner.security"):
        options = TriggerClientOptions(
            server_url="http://croniq.example.com:4000", allow_insecure_http=True
        )

    assert options.allow_insecure_http is True
    assert len([r for r in caplog.records if r.levelno == logging.WARNING]) == 1


def test_loopback_url_does_not_warn(caplog: pytest.LogCaptureFixture) -> None:
    with caplog.at_level(logging.WARNING, logger="croniq_runner.security"):
        RunnerOptions(server_url="http://127.0.0.1:4000")
    assert caplog.records == []


@pytest.mark.parametrize(
    ("host", "expected"),
    [
        ("localhost", True),
        ("LocalHost", True),
        ("127.0.0.1", True),
        ("127.255.255.254", True),
        ("::1", True),
        ("[::1]", True),
        ("croniq.example.com", False),
        ("10.0.0.5", False),
        ("2001:db8::1", False),
        ("", False),
        (None, False),
    ],
)
def test_is_loopback_host(host: str | None, expected: bool) -> None:
    assert is_loopback_host(host) is expected
