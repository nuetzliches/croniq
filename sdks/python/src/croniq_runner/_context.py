"""Execution context handed to each handler."""

from __future__ import annotations

import asyncio
import logging
import re
from datetime import datetime, timedelta
from typing import TYPE_CHECKING, Any

from croniq_runner._log_writer import LogLevel, LogWriter, _Enrichment
from croniq_runner._protocol import WorkEvent

if TYPE_CHECKING:
    from croniq_runner._client import CroniqClient
    from croniq_runner._options import LogWriterOptions


_DURATION_RE = re.compile(r"^\s*(\d+(?:\.\d+)?)\s*([smhd])\s*$", re.IGNORECASE)


def _parse_timeout(raw: str | None, default: timedelta = timedelta(minutes=5)) -> timedelta:
    """Parse a humane duration string like ``"15m"`` / ``"30s"`` / ``"1h"``."""
    if not raw:
        return default
    m = _DURATION_RE.match(raw)
    if not m:
        return default
    value = float(m.group(1))
    unit = m.group(2).lower()
    if unit == "s":
        return timedelta(seconds=value)
    if unit == "m":
        return timedelta(minutes=value)
    if unit == "h":
        return timedelta(hours=value)
    if unit == "d":
        return timedelta(days=value)
    return default


def _parse_scheduled_for(raw: str | None) -> datetime | None:
    """Parse the server's ``scheduled_for`` (RFC 3339) into a datetime.

    Returns ``None`` when the field is absent (older server) or unparseable —
    never falls back to fire_at, which would reintroduce the wrong-logical-time
    bug. Accepts a trailing ``Z`` (mapped to ``+00:00``) for older Pythons.
    """
    if not raw:
        return None
    try:
        return datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError:
        return None


class _JobLoggerAdapter(logging.LoggerAdapter):  # type: ignore[type-arg]
    """Attach the execution's identifiers to every record as fields.

    The identifiers stay out of the message text — a ``job_key`` carrying CRLF
    would otherwise forge a log record, and one carrying ANSI escapes would
    reach the operator's terminal raw. Rendering them is the host ``logging``
    configuration's job: a JSON formatter picks them up from the record, and a
    plain ``%(message)s`` formatter ignores them, exactly as it does for any
    other field an application attaches. The SDK does not escape them a second
    time; :mod:`croniq_runner._identifiers` rejects hostile values on ingest so
    the value on the record is already within a printable charset.
    """

    def process(self, msg, kwargs):  # type: ignore[no-untyped-def]
        extra = dict(self.extra or {})
        extra.update(kwargs.get("extra") or {})
        kwargs["extra"] = extra
        return msg, kwargs


class ExecutionContext:
    """Context handed to a job handler for one execution.

    The handler receives the work assignment payload, a Python logger
    pre-scoped with execution identifiers, a streaming :class:`LogWriter`
    for shipping logs to the Croniq server, and a
    :class:`asyncio.CancelledError`-raising :attr:`cancellation` event that
    fires on host shutdown or server-side cancellation.
    """

    def __init__(
        self,
        *,
        execution_id: str,
        job_key: str,
        scheduled_for: datetime | None,
        attempt: int,
        metadata: dict[str, Any],
        timeout: timedelta,
        runner_id: str,
        runner_tags: list[str],
        cancellation: asyncio.Event,
        client: CroniqClient,
        log_writer_options: LogWriterOptions,
    ) -> None:
        self.execution_id = execution_id
        self.job_key = job_key
        #: The trigger's original logical fire time — stable across retries and
        #: dead-letter replays. Use this (not ``datetime.now()``) for
        #: time-relative job logic like "the month being reported". ``None``
        #: when the server predates the field; never falls back to fire_at.
        self.scheduled_for = scheduled_for
        self.attempt = attempt
        self.metadata = metadata
        self.timeout = timeout
        self.runner_id = runner_id
        self.runner_tags = runner_tags
        self.cancellation = cancellation
        # One fixed logger for every job, with the identifiers attached as
        # fields. Deriving the logger *name* from ``job_key`` — as this did
        # before #441 — hands the server control of a namespace: ``getLogger``
        # caches every name forever (plus a ``PlaceHolder`` per dot-separated
        # ancestor), so a server delivering many distinct keys grew the process
        # without bound, and a key chosen to land under a namespace the
        # operator configured with ``propagate=False`` evaded log filtering.
        # Validating the key on ingest bounds its charset but not the *number*
        # of distinct keys, so the cache stays unbounded either way — a single
        # logger is both the simpler and the complete fix.
        self.logger = _JobLoggerAdapter(
            logging.getLogger("croniq_runner.job"),
            {"job_key": job_key, "execution_id": execution_id, "runner_id": runner_id,
             "attempt": attempt},
        )
        self._enrichment = _Enrichment(job_key, runner_id, runner_tags)
        self._client = client
        self._log_writer_options = log_writer_options
        self._log_writer: LogWriter | None = None

    @property
    def log_writer(self) -> LogWriter:
        """Streaming log writer for this execution. Created lazily."""
        if self._log_writer is None:
            self._log_writer = LogWriter(
                self._client,
                self.execution_id,
                self._enrichment,
                self._log_writer_options,
            )
        return self._log_writer

    @property
    def _log_writer_created(self) -> bool:
        return self._log_writer is not None

    async def log(
        self,
        message: str,
        *,
        level: LogLevel | str = LogLevel.INFO,
        fields: dict[str, str] | None = None,
    ) -> None:
        """Push a single structured event inline (POST awaited).

        For high-volume scenarios prefer :attr:`log_writer`.
        """
        level_str = level.value if isinstance(level, LogLevel) else level
        ev = WorkEvent(level=level_str, message=message, fields=fields)
        await self._client.push_events(self.execution_id, [self._enrichment.enrich(ev)])

    def raise_if_cancelled(self) -> None:
        """Raise :class:`asyncio.CancelledError` if cancellation has fired.

        Use inside long-running handlers between cooperative checkpoints.
        """
        if self.cancellation.is_set():
            raise asyncio.CancelledError("execution cancelled")
