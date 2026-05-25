"""Unit tests for the streaming LogWriter."""

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

import pytest

from croniq_runner._log_writer import LogLevel, LogWriter, _Enrichment
from croniq_runner._options import LogWriterOptions

if TYPE_CHECKING:
    from croniq_runner._protocol import WorkEvent


class FakeClient:
    """Stand-in for :class:`CroniqClient` that records pushed events."""

    def __init__(self) -> None:
        self.batches: list[list[WorkEvent]] = []
        self.delays: float = 0.0
        self._lock = asyncio.Lock()

    async def push_events(self, execution_id: str, events: list[WorkEvent]) -> None:  # noqa: ARG002
        async with self._lock:
            if self.delays:
                await asyncio.sleep(self.delays)
            self.batches.append(list(events))


def _enrichment() -> _Enrichment:
    return _Enrichment("billing:invoice", "runner-1", ["lang=python"])


async def test_writer_flushes_on_size_threshold() -> None:
    client = FakeClient()
    options = LogWriterOptions(
        channel_capacity=64,
        batch_size_threshold=3,
        batch_time_threshold_ms=10_000,  # effectively disabled for this test
        max_batch_per_post=100,
        shutdown_timeout_ms=2000,
    )
    writer = LogWriter(client, "exec-1", _enrichment(), options)

    for i in range(5):
        await writer.write(f"line {i}")

    await writer.aclose()

    flat = [ev for batch in client.batches for ev in batch]
    assert len(flat) == 5
    # Enrichment must inject job_key + runner_id (caller-supplied wins; none here).
    assert all(ev.fields and ev.fields["job_key"] == "billing:invoice" for ev in flat)
    assert all(ev.fields and ev.fields["runner_id"] == "runner-1" for ev in flat)
    assert all(ev.fields and ev.fields["runner_tags"] == "lang=python" for ev in flat)


async def test_writer_explicit_flush_waits_for_drain() -> None:
    client = FakeClient()
    options = LogWriterOptions(batch_time_threshold_ms=5000)
    writer = LogWriter(client, "exec-1", _enrichment(), options)

    await writer.write("first")
    await writer.flush()  # must wait for the POST

    flat = [ev for batch in client.batches for ev in batch]
    assert len(flat) == 1
    assert flat[0].message == "first"

    await writer.aclose()


async def test_writer_drain_on_close_emits_everything() -> None:
    client = FakeClient()
    options = LogWriterOptions(
        batch_size_threshold=100,
        batch_time_threshold_ms=5000,
        shutdown_timeout_ms=2000,
    )
    writer = LogWriter(client, "exec-1", _enrichment(), options)

    for i in range(7):
        await writer.write(f"line {i}", level=LogLevel.WARN)

    await writer.aclose()

    flat = [ev for batch in client.batches for ev in batch]
    assert [ev.message for ev in flat] == [f"line {i}" for i in range(7)]
    assert all(ev.level == "warn" for ev in flat)


async def test_writer_respects_max_batch_per_post() -> None:
    client = FakeClient()
    options = LogWriterOptions(
        batch_size_threshold=100,
        batch_time_threshold_ms=5000,
        max_batch_per_post=3,
    )
    writer = LogWriter(client, "exec-1", _enrichment(), options)

    for i in range(10):
        await writer.write(f"line {i}")
    await writer.aclose()

    # 10 events / 3 per POST = 4 batches.
    assert all(len(b) <= 3 for b in client.batches)
    flat = [ev for batch in client.batches for ev in batch]
    assert len(flat) == 10


async def test_writer_caller_fields_win_over_enrichment() -> None:
    client = FakeClient()
    writer = LogWriter(client, "exec-1", _enrichment(), LogWriterOptions(batch_size_threshold=1))

    await writer.write("hello", fields={"job_key": "overridden"})
    await writer.aclose()

    flat = [ev for batch in client.batches for ev in batch]
    assert flat[0].fields and flat[0].fields["job_key"] == "overridden"
    assert flat[0].fields["runner_id"] == "runner-1"


async def test_writer_close_is_idempotent() -> None:
    client = FakeClient()
    writer = LogWriter(client, "exec-1", _enrichment(), LogWriterOptions())
    await writer.write("x")
    await writer.aclose()
    await writer.aclose()  # second close is a no-op
    # Writes after close are silently dropped, no exception.
    await writer.write("after-close")


@pytest.mark.parametrize(
    ("level", "expected"),
    [
        (LogLevel.TRACE, "trace"),
        (LogLevel.DEBUG, "debug"),
        (LogLevel.INFO, "info"),
        (LogLevel.WARN, "warn"),
        (LogLevel.ERROR, "error"),
        ("custom", "custom"),
    ],
)
async def test_log_levels_pass_through(level: LogLevel | str, expected: str) -> None:
    client = FakeClient()
    writer = LogWriter(client, "exec-1", _enrichment(), LogWriterOptions(batch_size_threshold=1))
    await writer.write("hello", level=level)
    await writer.aclose()
    flat = [ev for batch in client.batches for ev in batch]
    assert flat[0].level == expected
