"""The Croniq runner: poll → dispatch → ack."""

from __future__ import annotations

import asyncio
import contextlib
import logging
import secrets
import time
from collections.abc import Callable, Coroutine
from dataclasses import dataclass, field
from typing import Any

import httpx

from croniq_runner._client import CroniqClient
from croniq_runner._context import ExecutionContext, _parse_timeout
from croniq_runner._errors import HandlerError, NoHandlerRegisteredError
from croniq_runner._identity import resolve_runner_id
from croniq_runner._options import RunnerOptions
from croniq_runner._otel import maybe_start_span
from croniq_runner._protocol import (
    AckRequest,
    PollRequest,
    RegisterJobRequest,
    RenewRequest,
    WorkAssignment,
)

_log = logging.getLogger("croniq_runner.runner")


Handler = Callable[[ExecutionContext], Coroutine[Any, Any, None]]


@dataclass(slots=True)
class _HandlerEntry:
    handler: Handler
    schedule: str | None = None
    timeout: str | None = None
    description: str | None = None


@dataclass(slots=True)
class _Inflight:
    cancellation: asyncio.Event
    task: asyncio.Task[None] = field(init=False)


class Runner:
    """Polls a Croniq server for work and dispatches handlers.

    Construct, register handlers, then ``await runner.run()`` — the call
    returns once the cancellation token fires *and* in-flight executions
    have either finished or the drain timeout elapses.
    """

    def __init__(
        self,
        options: RunnerOptions,
        *,
        client: CroniqClient | None = None,
    ) -> None:
        self._options = options
        self._client = client or CroniqClient(options)
        self._handlers: dict[str, _HandlerEntry] = {}
        self._default_handler: _HandlerEntry | None = None
        self._inflight: dict[str, _Inflight] = {}
        self._runner_id: str | None = None
        self._instance_id = secrets.token_hex(8)
        self._drain_event = asyncio.Event()
        self._stopped_polling = asyncio.Event()
        self._ran = False

    # ----- registration API ---------------------------------------------

    def add_handler(
        self,
        job_key: str,
        handler: Handler,
        *,
        schedule: str | None = None,
        timeout: str | None = None,
        description: str | None = None,
    ) -> Runner:
        """Register a handler for ``job_key``.

        If ``schedule`` is provided, the runner POSTs to ``/v1/jobs/register``
        once on startup to self-register the schedule.
        """
        self._handlers[job_key] = _HandlerEntry(
            handler=handler, schedule=schedule, timeout=timeout, description=description
        )
        return self

    def set_default_handler(self, handler: Handler) -> Runner:
        """Fallback handler invoked when no job-key match is registered."""
        self._default_handler = _HandlerEntry(handler=handler)
        return self

    @property
    def runner_id(self) -> str:
        if self._runner_id is None:
            raise RuntimeError("runner_id is only available after run() starts")
        return self._runner_id

    @property
    def inflight(self) -> list[str]:
        """Snapshot of currently in-flight execution IDs (diagnostic only)."""
        return list(self._inflight.keys())

    def request_drain(self) -> None:
        """Stop polling for new work. In-flight handlers keep running."""
        self._drain_event.set()

    # ----- main loop ----------------------------------------------------

    async def run(self) -> None:
        """Run the poll/dispatch/ack loop until cancellation."""
        if self._ran:
            raise RuntimeError("Runner.run may only be called once per instance")
        self._ran = True

        self._runner_id = resolve_runner_id(self._options)
        _log.info(
            "croniq runner starting: runner_id=%s capabilities=%s max_inflight=%d",
            self._runner_id,
            ",".join(self._options.capabilities),
            self._options.max_inflight,
        )

        await self._self_register_schedules()

        try:
            await self._poll_loop()
        except asyncio.CancelledError:
            # Caller cancelled us — switch to drain mode and let the finally
            # clause wait for handlers to finish.
            self._drain_event.set()
            raise
        finally:
            self._stopped_polling.set()
            await self._drain()
            if self._client is not None:
                await self._client.aclose()

    async def _self_register_schedules(self) -> None:
        for job_key, entry in self._handlers.items():
            if not entry.schedule:
                continue
            request = RegisterJobRequest(
                job_key=job_key,
                schedule=entry.schedule,
                timeout=entry.timeout,
                runner_id=self._runner_id,
                capabilities=list(self._options.capabilities),
                description=entry.description,
            )
            try:
                resp = await self._client.register_job(request)
                if resp and resp.status == "skipped_dsl_precedence":
                    _log.info(
                        "job %s is managed by the Croniqfile (DSL precedence) — schedule registration skipped",
                        job_key,
                    )
            except Exception as exc:  # noqa: BLE001 — non-fatal; runner can still poll
                _log.warning(
                    "self-register for job %s failed (%s) — runner will still poll", job_key, exc
                )

    async def _poll_loop(self) -> None:
        opts = self._options
        while not self._drain_event.is_set():
            if len(self._inflight) >= opts.max_inflight:
                await self._sleep_or_drain(opts.capacity_backoff_ms / 1000.0)
                continue

            request = PollRequest(
                runner_id=self._runner_id or "",
                capabilities=list(opts.capabilities),
                max_inflight=opts.max_inflight,
                inflight=list(self._inflight.keys()),
                instance_id=self._instance_id,
                tags=list(opts.tags),
            )

            try:
                response = await self._client.poll(request, timeout_ms=opts.poll_timeout_ms)
            except asyncio.CancelledError:
                raise
            except (httpx.HTTPError, Exception) as exc:  # noqa: BLE001
                _log.warning("poll failed (%s) — backing off %dms", exc, opts.poll_retry_delay_ms)
                await self._sleep_or_drain(opts.poll_retry_delay_ms / 1000.0)
                continue

            self._handle_cancellations(response.cancel)
            for assignment in response.work:
                self._spawn_handler(assignment)

            # Yield to the event loop so newly-spawned handler tasks (and
            # the renew loops they own) get a chance to make progress before
            # the next poll. A real server's HTTP call always yields via
            # socket I/O — but mock transports and tight retry loops can
            # otherwise starve handlers indefinitely.
            await asyncio.sleep(0)

    def _handle_cancellations(self, cancel_ids: list[str]) -> None:
        for execution_id in cancel_ids:
            inflight = self._inflight.get(execution_id)
            if inflight is not None:
                inflight.cancellation.set()
                _log.info("server requested cancellation of execution %s", execution_id)

    def _spawn_handler(self, assignment: WorkAssignment) -> None:
        if assignment.execution_id in self._inflight:
            # Server sent a duplicate — ignore.
            return
        cancellation = asyncio.Event()
        inflight = _Inflight(cancellation=cancellation)
        self._inflight[assignment.execution_id] = inflight
        task = asyncio.create_task(self._run_one(assignment, cancellation))
        inflight.task = task
        eid = assignment.execution_id

        def _cleanup(_t: asyncio.Task[None], _eid: str = eid) -> None:
            self._inflight.pop(_eid, None)

        task.add_done_callback(_cleanup)

    async def _run_one(self, assignment: WorkAssignment, cancellation: asyncio.Event) -> None:
        execution_id = assignment.execution_id
        job_key = assignment.job_key
        attempt = assignment.attempt
        timeout = _parse_timeout(assignment.timeout)

        ctx = ExecutionContext(
            execution_id=execution_id,
            job_key=job_key,
            attempt=attempt,
            metadata=assignment.metadata,
            timeout=timeout,
            runner_id=self._runner_id or "",
            runner_tags=list(self._options.tags),
            cancellation=cancellation,
            client=self._client,
            log_writer_options=self._options.log_writer,
        )

        entry = self._handlers.get(job_key) or self._default_handler
        status: str
        error: str | None = None
        start = time.perf_counter()

        # Lease-renewal heartbeat runs alongside the handler.
        renew_stop = asyncio.Event()
        renew_task = asyncio.create_task(self._renew_loop(execution_id, renew_stop))

        # Cancellation propagation: an asyncio.Event is more cooperative than
        # cancelling the handler task directly — handlers explicitly check
        # ``ctx.cancellation`` or use ``raise_if_cancelled()``. To make
        # ``await ctx.cancellation.wait()`` ergonomic AND to honour
        # ``asyncio.CancelledError`` for handlers that use cancel scopes /
        # ``asyncio.wait_for``, we wrap the handler in a task and cancel it
        # when the event fires.
        try:
            if entry is None:
                raise NoHandlerRegisteredError(job_key)
            with maybe_start_span(
                "croniq.execute", job_key=job_key, execution_id=execution_id,
                runner_id=self._runner_id or "", attempt=attempt,
            ):
                handler_task = asyncio.create_task(entry.handler(ctx))
                await _await_with_cancel(handler_task, cancellation)
            status = "success"
        except asyncio.CancelledError:
            status = "failure"
            if self._drain_event.is_set() and not cancellation.is_set():
                # Caller (run()) was cancelled and triggered the drain. The
                # current execution may still finish naturally — but if we got
                # here, the handler ran out of time / raised CancelledError.
                error = "runner draining"
            else:
                error = "cancelled by server"
        except HandlerError as exc:
            _log.warning("handler for %s (execution %s) raised: %s", job_key, execution_id, exc.message)
            status = "failure"
            error = exc.message
        except NoHandlerRegisteredError as exc:
            _log.error("%s", exc)
            status = "failure"
            error = str(exc)
        except Exception as exc:  # noqa: BLE001 — handler-boundary catch-all
            _log.warning("handler for %s (execution %s) raised", job_key, execution_id, exc_info=True)
            status = "failure"
            error = str(exc) or exc.__class__.__name__

        duration_ms = int((time.perf_counter() - start) * 1000)

        # Stop the renew loop *before* draining the log writer / acking.
        renew_stop.set()
        with contextlib.suppress(asyncio.CancelledError, Exception):
            await renew_task

        # Drain the streaming log writer if it was used. This is the
        # central guarantee from conformance case #09: every event the
        # handler queued must arrive at /events before the ack.
        if ctx._log_writer_created:
            try:
                await ctx.log_writer.aclose()
            except Exception:
                _log.warning(
                    "log_writer drain failed for execution %s", execution_id, exc_info=True
                )

        try:
            await self._client.ack(
                AckRequest(
                    runner_id=self._runner_id or "",
                    execution_id=execution_id,
                    status=status,
                    error=error,
                    duration_ms=duration_ms,
                    attempt=attempt,
                )
            )
        except Exception:
            _log.error("failed to ack execution %s", execution_id, exc_info=True)

    async def _renew_loop(self, execution_id: str, stop: asyncio.Event) -> None:
        interval = self._options.renew_interval_ms / 1000.0
        while not stop.is_set():
            try:
                await asyncio.wait_for(stop.wait(), timeout=interval)
                return
            except TimeoutError:
                pass
            try:
                await self._client.renew(
                    RenewRequest(runner_id=self._runner_id or "", execution_id=execution_id)
                )
            except asyncio.CancelledError:
                raise
            except Exception as exc:  # noqa: BLE001 — transient renew failure is non-fatal
                _log.debug("lease renew failed for execution %s: %s", execution_id, exc)

    async def _drain(self) -> None:
        if not self._inflight:
            return
        _log.info(
            "draining %d in-flight execution(s) (timeout %dms)",
            len(self._inflight),
            self._options.drain_timeout_ms,
        )
        deadline = time.monotonic() + self._options.drain_timeout_ms / 1000.0
        while self._inflight and time.monotonic() < deadline:
            await asyncio.sleep(0.05)
        if self._inflight:
            _log.warning(
                "drain timed out with %d execution(s) still in-flight — cancelling",
                len(self._inflight),
            )
            # Cancel via the per-execution cancellation event so handlers
            # that respect ``ctx.cancellation`` abort cleanly; the dispatcher
            # then acks with status=failure.
            for inflight in list(self._inflight.values()):
                inflight.cancellation.set()
            # Wait briefly for the tasks to finish their ack POSTs.
            await asyncio.sleep(0.2)
            # Hard-cancel anything still hanging on.
            for inflight in list(self._inflight.values()):
                inflight.task.cancel()
            for inflight in list(self._inflight.values()):
                with contextlib.suppress(asyncio.CancelledError, Exception):
                    await inflight.task

    async def _sleep_or_drain(self, seconds: float) -> None:
        """Sleep, but return early if drain was requested."""
        with contextlib.suppress(TimeoutError):
            await asyncio.wait_for(self._drain_event.wait(), timeout=seconds)


async def _await_with_cancel(task: asyncio.Task[Any], cancellation: asyncio.Event) -> None:
    """Await ``task`` but cancel it if ``cancellation`` fires first.

    The handler task ran on the event loop already — we just race its
    completion against the cancellation event. On cancel we forward the
    cancel into the handler task, then await it so any cleanup runs.
    """
    cancel_waiter = asyncio.create_task(cancellation.wait())
    try:
        done, _pending = await asyncio.wait(
            {task, cancel_waiter}, return_when=asyncio.FIRST_COMPLETED
        )
        if task in done:
            # Re-raise the handler's exception, if any.
            await task
            return
        # Cancellation fired first.
        task.cancel()
        try:
            await task
        except asyncio.CancelledError:
            raise
    finally:
        if not cancel_waiter.done():
            cancel_waiter.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await cancel_waiter
