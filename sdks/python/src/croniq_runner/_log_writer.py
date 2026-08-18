"""Streaming :class:`LogWriter`.

Bounded :class:`asyncio.Queue` plus a single background flusher coroutine.
Matches the Rust and .NET SDKs:

* batch-by-size  (default 32 events)
* batch-by-time  (default 200 ms)
* per-POST cap   (default 100 events)
* drain on close, bounded by ``LogWriterOptions.shutdown_timeout_ms``.
"""

from __future__ import annotations

import asyncio
import contextlib
import enum
import logging
from typing import TYPE_CHECKING

from croniq_runner._errors import is_ownership_denied
from croniq_runner._options import LogWriterOptions
from croniq_runner._protocol import WorkEvent

if TYPE_CHECKING:
    from croniq_runner._client import CroniqClient

_log = logging.getLogger("croniq_runner.log_writer")


class LogLevel(enum.StrEnum):
    """Severity strings accepted by the server."""

    TRACE = "trace"
    DEBUG = "debug"
    INFO = "info"
    WARN = "warn"
    ERROR = "error"


class _Enrichment:
    __slots__ = ("_base",)

    def __init__(self, job_key: str, runner_id: str, runner_tags: list[str]) -> None:
        base: dict[str, str] = {"job_key": job_key, "runner_id": runner_id}
        if runner_tags:
            base["runner_tags"] = ",".join(runner_tags)
        self._base = base

    def enrich(self, ev: WorkEvent) -> WorkEvent:
        # Caller-supplied keys win — only inject what's missing.
        merged: dict[str, str] = dict(self._base)
        if ev.fields:
            merged.update(ev.fields)
        return ev.model_copy(update={"fields": merged})


# Sentinel for "flush this and complete the future when done".
class _FlushMarker:
    __slots__ = ("future",)

    def __init__(self, future: asyncio.Future[None]) -> None:
        self.future = future


_Command = WorkEvent | _FlushMarker


class LogWriter:
    """Streaming log channel for one execution.

    Created lazily by :class:`ExecutionContext` — the background flusher only
    spawns once a handler actually emits an event.
    """

    def __init__(
        self,
        client: CroniqClient,
        execution_id: str,
        enrichment: _Enrichment,
        options: LogWriterOptions,
    ) -> None:
        self._client = client
        self._execution_id = execution_id
        self._enrichment = enrichment
        self._options = options
        self._queue: asyncio.Queue[_Command] = asyncio.Queue(maxsize=options.channel_capacity)
        self._closed = False
        # Sentinel that signals "no more events" to the flusher.
        self._close_marker: object = object()
        self._flusher_task = asyncio.create_task(self._flusher_loop())

    async def write(
        self,
        message: str,
        *,
        level: LogLevel | str = LogLevel.INFO,
        fields: dict[str, str] | None = None,
    ) -> None:
        """Enqueue a log message. Awaits if the bounded queue is full."""
        if self._closed:
            return
        level_str = level.value if isinstance(level, LogLevel) else level
        ev = WorkEvent(level=level_str, message=message, fields=fields)
        await self._queue.put(ev)

    async def write_event(self, event: WorkEvent) -> None:
        """Enqueue a pre-built :class:`WorkEvent`. Awaits on backpressure."""
        if self._closed:
            return
        await self._queue.put(event)

    async def flush(self) -> None:
        """Wait until everything queued so far has been POSTed."""
        if self._closed:
            return
        future: asyncio.Future[None] = asyncio.get_running_loop().create_future()
        await self._queue.put(_FlushMarker(future))
        await future

    async def aclose(self) -> None:
        """Drain the queue and shut the flusher down, bounded by ``shutdown_timeout_ms``."""
        if self._closed:
            return
        self._closed = True
        # Signal close by enqueueing the marker. ``put`` instead of
        # ``put_nowait`` so a saturated queue doesn't lose the marker — the
        # flusher will drain enough room for it.
        await self._queue.put(self._close_marker)  # type: ignore[arg-type]
        try:
            await asyncio.wait_for(
                self._flusher_task,
                timeout=self._options.shutdown_timeout_ms / 1000.0,
            )
        except TimeoutError:
            _log.warning(
                "log_writer drain timed out after %dms (execution %s)",
                self._options.shutdown_timeout_ms,
                self._execution_id,
            )
            self._flusher_task.cancel()
            # Best-effort: await the cancellation so we don't leak the task.
            with contextlib.suppress(asyncio.CancelledError, Exception):
                await self._flusher_task

    async def _flusher_loop(self) -> None:
        buffer: list[WorkEvent] = []
        pending_flushes: list[asyncio.Future[None]] = []
        time_threshold = self._options.batch_time_threshold_ms / 1000.0
        try:
            while True:
                try:
                    cmd = await asyncio.wait_for(self._queue.get(), timeout=time_threshold)
                except TimeoutError:
                    # Time-based flush.
                    if buffer:
                        await self._flush_buffer(buffer)
                    self._complete_pending(pending_flushes)
                    continue

                if cmd is self._close_marker:
                    # Drain everything that arrived before close and exit.
                    while not self._queue.empty():
                        nxt = self._queue.get_nowait()
                        if isinstance(nxt, _FlushMarker):
                            pending_flushes.append(nxt.future)
                        elif nxt is self._close_marker:
                            continue
                        else:
                            buffer.append(nxt)
                    await self._flush_buffer(buffer)
                    self._complete_pending(pending_flushes)
                    return

                if isinstance(cmd, _FlushMarker):
                    pending_flushes.append(cmd.future)
                else:
                    buffer.append(cmd)
                    if len(buffer) >= self._options.batch_size_threshold:
                        await self._flush_buffer(buffer)

                # Drain anything else immediately available so a fast burst
                # doesn't get fragmented into many tiny POSTs.
                while not self._queue.empty():
                    nxt = self._queue.get_nowait()
                    if nxt is self._close_marker:
                        # Re-enqueue at the head conceptually: handle on next loop turn.
                        # Simpler: stop draining and let the outer loop see it on
                        # the next ``await self._queue.get()``.
                        await self._queue.put(nxt)
                        break
                    if isinstance(nxt, _FlushMarker):
                        pending_flushes.append(nxt.future)
                    else:
                        buffer.append(nxt)
                        if len(buffer) >= self._options.batch_size_threshold:
                            await self._flush_buffer(buffer)

                # An explicit flush request always forces a flush.
                if pending_flushes:
                    await self._flush_buffer(buffer)
                    self._complete_pending(pending_flushes)
        except asyncio.CancelledError:
            # Hard-cancel during shutdown: drop the remainder.
            for f in pending_flushes:
                if not f.done():
                    f.cancel()
            raise

    async def _flush_buffer(self, buffer: list[WorkEvent]) -> None:
        while buffer:
            take = min(len(buffer), self._options.max_batch_per_post)
            chunk = [self._enrichment.enrich(e) for e in buffer[:take]]
            del buffer[:take]
            try:
                await self._client.push_events(self._execution_id, chunk)
            except Exception as exc:  # noqa: BLE001 — surface as warning, keep draining
                if is_ownership_denied(exc):
                    # Permanent (#436/#437) — every later batch is lost too,
                    # so the operator must see this rather than wonder why
                    # the execution produced no output.
                    _log.error(
                        "log_writer: batch POST refused with 403 Forbidden — this runner's "
                        "credential does not own its runner_id, so no log event will reach "
                        "the server (%d event(s) dropped, execution %s). Give the runner its "
                        "own runner_id, or release the existing binding with "
                        "DELETE /v1/runners/{id}",
                        len(chunk),
                        self._execution_id,
                    )
                    continue
                _log.warning(
                    "log_writer: batch POST failed — %d event(s) dropped (execution %s): %s",
                    len(chunk),
                    self._execution_id,
                    exc,
                )

    @staticmethod
    def _complete_pending(pending: list[asyncio.Future[None]]) -> None:
        for f in pending:
            if not f.done():
                f.set_result(None)
        pending.clear()
