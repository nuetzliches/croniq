"""Tests for the context helpers."""

from __future__ import annotations

from datetime import timedelta

import pytest

from croniq_runner._context import _parse_timeout


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("30s", timedelta(seconds=30)),
        ("5m", timedelta(minutes=5)),
        ("1h", timedelta(hours=1)),
        ("2d", timedelta(days=2)),
        ("1.5m", timedelta(minutes=1.5)),
        ("  10s ", timedelta(seconds=10)),
        ("10S", timedelta(seconds=10)),
    ],
)
def test_parse_timeout_humane_forms(raw: str, expected: timedelta) -> None:
    assert _parse_timeout(raw) == expected


@pytest.mark.parametrize("raw", ["", None, "foo", "10x", "abc"])
def test_parse_timeout_falls_back_to_default(raw: str | None) -> None:
    assert _parse_timeout(raw) == timedelta(minutes=5)


def test_parse_timeout_custom_default() -> None:
    assert _parse_timeout(None, default=timedelta(seconds=42)) == timedelta(seconds=42)
