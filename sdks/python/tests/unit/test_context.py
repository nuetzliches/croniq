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


def test_parse_scheduled_for_rfc3339() -> None:
    from datetime import UTC, datetime

    from croniq_runner._context import _parse_scheduled_for

    got = _parse_scheduled_for("2026-06-01T06:00:00Z")
    assert got == datetime(2026, 6, 1, 6, 0, 0, tzinfo=UTC)


def test_parse_scheduled_for_absent_is_none() -> None:
    from croniq_runner._context import _parse_scheduled_for

    assert _parse_scheduled_for(None) is None


def test_parse_scheduled_for_unparseable_is_none() -> None:
    from croniq_runner._context import _parse_scheduled_for

    assert _parse_scheduled_for("not-a-date") is None
