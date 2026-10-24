"""The Croniq runner: poll → dispatch → ack."""

from __future__ import annotations

import asyncio
import contextlib
import logging
import random
import secrets
import time
from collections.abc import Callable, Coroutine
from dataclasses import dataclass, field
from typing import Any

import httpx

from croniq_runner._client import CroniqClient
from croniq_runner._context import (
    ExecutionContext,
    _parse_scheduled_for,
    _parse_timeout,
)
from croniq_runner._errors import (
    AuthFailedError,
    HandlerError,
    NoHandlerRegisteredError,
    PollInstanceConflictError,
    RunnerOwnershipDeniedError,
    is_instance_conflict,
    is_ownership_denied,
    is_unauthorized,
)
from croniq_runner._identifiers import (
    is_safe_execution_id,
    preview_for_log,
    reject_assignment_reason,
    rejection_ack_error,
)
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

# Delays before the second and third ack attempt (review 2026-10-24, K-1).
# Module-level so tests can shorten them. Three attempts in total, about 1 s and
# then 2 s apart — the same schedule in every SDK, pinned by conformance cases
# 23-26.
_ACK_RETRY_DELAYS_S: tuple[float, ...] = (1.0, 2.0)

# Spread of the poll retry delay (review 2026-10-24, K-9): 0.8-1.2x, so a fleet
# of runners that all saw the same outage does not come back in lockstep.
_POLL_RETRY_JITTER = 0.2


def _jittered(seconds: float) -> float:
    """``seconds`` scaled by a uniform factor in ``[0.8, 1.2]``."""
    # Spreading load, not a security decision, so the module-level PRNG is fine.
    return seconds * random.uniform(1.0 - _POLL_RETRY_JITTER, 1.0 + _POLL_RETRY_JITTER)


def _is_retryable_ack_error(exc: BaseException) -> bool:
    """Transport errors (including request timeouts) and 5xx are worth a retry.

    A 4xx is the server's answer to this ack — 409 already terminal or no longer
    claimed by us, 404 gone, 403 not our runner_id — and resending the same body
    cannot change it.
    """
    if isinstance(exc, httpx.HTTPStatusError):
        return exc.response.status_code >= 500
    return isinstance(exc, httpx.TransportError | TimeoutError)


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
        # Set once a handler releases its in-flight slot; ends the
        # at-capacity backoff early (issue #845).
        self._slot_freed = asyncio.Event()
        self._stopped_polling = asyncio.Event()
        # Set once the drain timeout elapsed and the remaining handlers are
        # being cancelled; their failure ack then says "runner draining".
        self._drain_hard_cancel = False
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
        # Consecutive 409 Conflict responses on poll. Reset by a successful
        # poll or by any non-409 failure — see
        # RunnerOptions.max_consecutive_poll_conflicts.
        consecutive_conflicts = 0
        # Consecutive 401s, tracked separately: a run of conflicts must not
        # spend the auth budget, or a duplicate deployment would be reported
        # as an authentication failure.
        consecutive_auth_failures = 0
        while not self._drain_event.is_set():
            # Control-slot polling (issue #176): even at capacity we still
            # poll so the server can deliver cancels via PollResponse.cancel.
            # The server returns immediately when capacity=0 (no long-poll),
            # so capacity_backoff_ms paces the loop and prevents a stampede.
            #
            # One snapshot drives both the request and the capacity decision,
            # so the two can never disagree about how full the runner is
            # (the .NET/Go/Java SDKs once read them separately and raced —
            # issue #817).
            #
            # The slot-freed signal is cleared together with the snapshot (no
            # await in between), so it records exactly the handlers that finish
            # after it.
            self._slot_freed.clear()
            inflight_ids = list(self._inflight.keys())
            at_capacity = len(inflight_ids) >= opts.max_inflight

            request = PollRequest(
                runner_id=self._runner_id or "",
                capabilities=list(opts.capabilities),
                max_inflight=opts.max_inflight,
                inflight=inflight_ids,
                instance_id=self._instance_id,
                tags=list(opts.tags),
            )

            try:
                response = await self._client.poll(request, timeout_ms=opts.poll_timeout_ms)
            except asyncio.CancelledError:
                raise
            except (httpx.HTTPError, Exception) as exc:  # noqa: BLE001
                # A 403 is permanent (issue #437): the credential is bound
                # to another runner_id, so the next poll fails identically.
                # Stop with an actionable error instead of retrying on the
                # poll interval, which makes a fenced-out runner look idle.
                if is_ownership_denied(exc):
                    _log.error(
                        "fatal: poll refused with 403 Forbidden — this runner's credential "
                        "does not own runner_id. Give the runner its own runner_id, or "
                        "release the existing binding with DELETE /v1/runners/{id}",
                        extra={"runner_id": self._runner_id or ""},
                    )
                    raise RunnerOwnershipDeniedError(self._runner_id or "") from exc
                # A 401 says the key was rejected, and the client never
                # re-reads it, so every later poll presents the same dead
                # credential. Budgeted rather than fatal at once: rotation
                # hands over through an expiry window (server issue #471) and
                # a race around it should not kill a healthy runner (#473).
                if is_unauthorized(exc):
                    consecutive_auth_failures += 1
                    if consecutive_auth_failures >= opts.max_consecutive_auth_failures:
                        _log.error(
                            "fatal: poll refused with 401 Unauthorized %d times in a row — "
                            "the API key was rejected. It may have been revoked, or its "
                            "rotation grace window may have elapsed. Restart the runner "
                            "with the current key",
                            consecutive_auth_failures,
                            extra={"runner_id": self._runner_id or ""},
                        )
                        raise AuthFailedError(consecutive_auth_failures) from exc
                    _log.warning(
                        "poll returned 401 Unauthorized (%d/%d) — the API key was rejected; "
                        "retrying after %dms",
                        consecutive_auth_failures,
                        opts.max_consecutive_auth_failures,
                        opts.poll_retry_delay_ms,
                    )
                    # A 401 is not a 409, so it clears the conflict budget
                    # just like any other non-409 failure. This branch
                    # continues before reaching the reset below, so it has to
                    # do it here (issue #508).
                    consecutive_conflicts = 0
                    await self._sleep_or_drain(_jittered(opts.poll_retry_delay_ms / 1000.0))
                    continue
                # Anything that is not a 401 clears the auth budget: a 5xx or
                # a timeout says nothing about whether the credential is valid.
                consecutive_auth_failures = 0
                # A 409 means a newer instance has taken this runner_id over
                # (fencing, issue #374). One is transient — the deposed
                # instance may win it back — so we back off and retry. A
                # streak of them is a duplicate deployment, and retrying
                # forever hides it behind a warning that scrolls past
                # (issue #134 sub-item 1).
                if is_instance_conflict(exc):
                    consecutive_conflicts += 1
                    if consecutive_conflicts >= opts.max_consecutive_poll_conflicts:
                        _log.error(
                            "fatal: poll refused with 409 Conflict %d times in a row — another "
                            "runner is registered with this runner_id. Stop the duplicate "
                            "process or rotate the runner_id",
                            consecutive_conflicts,
                            extra={"runner_id": self._runner_id or ""},
                        )
                        raise PollInstanceConflictError(
                            self._runner_id or "", consecutive_conflicts
                        ) from exc
                    _log.warning(
                        "poll returned 409 Conflict (%d/%d) — another runner instance may be "
                        "active; retrying after %dms",
                        consecutive_conflicts,
                        opts.max_consecutive_poll_conflicts,
                        opts.poll_retry_delay_ms,
                    )
                else:
                    # Non-409 transient (5xx, network, timeout) — unrelated to
                    # instance ownership, so a recovered outage must not
                    # accumulate with later conflicts.
                    consecutive_conflicts = 0
                    _log.warning(
                        "poll failed (%s) — backing off %dms", exc, opts.poll_retry_delay_ms
                    )
                await self._sleep_or_drain(_jittered(opts.poll_retry_delay_ms / 1000.0))
                continue

            # Poll succeeded — the other instance must have died or released
            # the identity, so the conflict streak starts over. The auth
            # budget starts over with it: the credential just worked, so an
            # earlier 401 must not still count against a runner that has been
            # healthy since (issue #507).
            consecutive_conflicts = 0
            consecutive_auth_failures = 0

            self._handle_cancellations(response.cancel)

            # Delivered work is dispatched on an at-capacity poll too. By the
            # time the response arrives the server has committed the claim,
            # so dropping an assignment strands it as claimed until timeout +
            # grace; for a singleton job that blocks every later run (issue
            # #817). Briefly running over max_inflight is the lesser evil.
            for assignment in response.work:
                # Ingest guard: an assignment carrying a control character in
                # either identifier never reaches a handler, a log record, a
                # logger name or a telemetry attribute. See ``_identifiers``
                # for the rule and why it is a denylist.
                rejected = reject_assignment_reason(
                    assignment.execution_id, assignment.job_key
                )
                if rejected is not None:
                    await self._reject_assignment(assignment, rejected)
                    continue
                self._spawn_handler(assignment)

            # Yield to the event loop so newly-spawned handler tasks (and
            # the renew loops they own) get a chance to make progress before
            # the next poll. A real server's HTTP call always yields via
            # socket I/O — but mock transports and tight retry loops can
            # otherwise starve handlers indefinitely.
            await asyncio.sleep(0)

            if at_capacity:
                # The server returned immediately (no long-poll). Cancels and
                # any delivered work above are already processed; pace the
                # loop — but only until a handler finishes. With short
                # handlers a fixed sleep made the backoff, not the work, the
                # pace of dispatch (issue #845).
                await self._sleep_until_slot_freed(opts.capacity_backoff_ms / 1000.0)

    def _handle_cancellations(self, cancel_ids: list[str]) -> None:
        for execution_id in cancel_ids:
            # Cancel ids are server-supplied too. An unsafe one can never match
            # an in-flight key (those were validated on ingest), but checking
            # here keeps the value off the record below on any code path.
            if not is_safe_execution_id(execution_id):
                continue
            inflight = self._inflight.get(execution_id)
            if inflight is not None:
                inflight.cancellation.set()
                _log.info(
                    "server requested cancellation",
                    extra={"execution_id": execution_id},
                )

    async def _reject_assignment(self, assignment: WorkAssignment, field: str) -> None:
        """Handle a work assignment refused by the ingest guard.

        The two cases differ in what the runner can still tell the server:

        * **Unsafe ``execution_id``** — nothing. That value is what addresses an
          ack or renew, so there is no way to report anything about this
          execution. The assignment is dropped and the server's lease expires.
        * **Unsafe ``job_key``, valid ``execution_id``** — a failure ack. The
          handler never runs, but the execution completes with an error naming
          the offending field, so the operator sees a dead-lettered execution
          instead of one that is silently requeued by the stale-claim reaper and
          refused again on every later poll.

        Awaited rather than spawned: this path only triggers on malformed input,
        so pausing the loop for one small POST costs nothing and keeps the
        ordering observable.
        """
        offending = getattr(assignment, field)
        ackable = field == "job_key"
        # ``value`` is escaped and truncated: this is the one place a refused
        # value is rendered, and it is hostile by definition.
        _log.warning(
            "rejected work assignment with unsafe identifier",
            extra={
                "field": field,
                "value": preview_for_log(offending),
                "acked": ackable,
            },
        )
        if not ackable:
            return
        try:
            await self._client.ack(
                AckRequest(
                    runner_id=self._runner_id or "",
                    execution_id=assignment.execution_id,
                    status="failure",
                    error=rejection_ack_error(field, offending),
                    duration_ms=0,
                    attempt=assignment.attempt,
                )
            )
        except asyncio.CancelledError:
            raise
        except Exception:  # noqa: BLE001 — ack failure is non-fatal
            _log.warning(
                "failed to ack a rejected work assignment",
                exc_info=True,
                extra={"execution_id": assignment.execution_id},
            )

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
            self._slot_freed.set()

        task.add_done_callback(_cleanup)

    async def _run_one(self, assignment: WorkAssignment, cancellation: asyncio.Event) -> None:
        execution_id = assignment.execution_id
        job_key = assignment.job_key
        attempt = assignment.attempt
        timeout = _parse_timeout(assignment.timeout)

        ctx = ExecutionContext(
            execution_id=execution_id,
            job_key=job_key,
            scheduled_for=_parse_scheduled_for(assignment.scheduled_for),
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
            if self._drain_hard_cancel or (
                self._drain_event.is_set() and not cancellation.is_set()
            ):
                # Caller (run()) was cancelled and triggered the drain. The
                # current execution may still finish naturally — but if we got
                # here, the handler ran out of time / raised CancelledError.
                error = "runner draining"
            else:
                error = "cancelled by server"
        except HandlerError as exc:
            # Identifiers travel as fields, never interpolated into the message
            # — see ``_identifiers``. The same applies to every call below.
            _log.warning(
                "job handler raised: %s",
                exc.message,
                extra={"job_key": job_key, "execution_id": execution_id},
            )
            status = "failure"
            error = exc.message
        except NoHandlerRegisteredError as exc:
            _log.error(
                "no handler registered for job",
                extra={"job_key": job_key, "execution_id": execution_id},
            )
            status = "failure"
            error = str(exc)
        except Exception as exc:  # noqa: BLE001 — handler-boundary catch-all
            _log.warning(
                "job handler raised",
                exc_info=True,
                extra={"job_key": job_key, "execution_id": execution_id},
            )
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
                    "log_writer drain failed",
                    exc_info=True,
                    extra={"job_key": job_key, "execution_id": execution_id},
                )

        await self._ack_with_retry(
            AckRequest(
                runner_id=self._runner_id or "",
                execution_id=execution_id,
                status=status,
                error=error,
                duration_ms=duration_ms,
                attempt=attempt,
            ),
            job_key=job_key,
        )

    async def _ack_with_retry(self, request: AckRequest, *, job_key: str) -> None:
        """POST the ack, retrying a transport error or 5xx (review 2026-10-24, K-1).

        A handler that ran for twenty minutes used to lose its ack to a
        connection reset or a request timeout: the failure was logged, the
        execution left the in-flight set, the next poll reported it missing and
        the stale-claim reaper requeued a job that had already succeeded. The
        server fences the ack with a CAS, so sending it again is safe.

        Three attempts, about 1 s and then 2 s apart, each under its own request
        timeout. The execution stays in the in-flight set until this returns
        (the slot is released by the task's done callback), so polls keep
        reporting it meanwhile. A 4xx is never retried — see
        :func:`_is_retryable_ack_error`.
        """
        execution_id = request.execution_id
        attempts = len(_ACK_RETRY_DELAYS_S) + 1
        for attempt_no in range(1, attempts + 1):
            try:
                await self._client.ack(request)
                return
            except asyncio.CancelledError:
                raise
            except Exception as exc:  # noqa: BLE001 — ack failure must not kill the task
                if is_ownership_denied(exc):
                    _log.error(
                        "ack refused with 403 Forbidden — this runner's credential does not own "
                        "runner_id, so the execution stays claimed until its lease expires. Give "
                        "the runner its own runner_id, or release the existing binding with "
                        "DELETE /v1/runners/{id}",
                        exc_info=True,
                        extra={
                            "runner_id": self._runner_id or "",
                            "job_key": job_key,
                            "execution_id": execution_id,
                        },
                    )
                    return
                if not _is_retryable_ack_error(exc) or attempt_no == attempts:
                    _log.error(
                        "failed to ack execution (attempt %d/%d)",
                        attempt_no,
                        attempts,
                        exc_info=True,
                        extra={"job_key": job_key, "execution_id": execution_id},
                    )
                    return
                delay = _ACK_RETRY_DELAYS_S[attempt_no - 1]
                _log.warning(
                    "ack failed (attempt %d/%d: %s) — retrying in %.1fs",
                    attempt_no,
                    attempts,
                    exc.__class__.__name__,
                    delay,
                    extra={"job_key": job_key, "execution_id": execution_id},
                )
                await asyncio.sleep(delay)

    def _ack_budget_s(self) -> float:
        """Upper bound on how long :meth:`_ack_with_retry` can take."""
        attempts = len(_ACK_RETRY_DELAYS_S) + 1
        return attempts * self._options.request_timeout_ms / 1000.0 + sum(_ACK_RETRY_DELAYS_S)

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
                if is_ownership_denied(exc):
                    # Permanent (#436/#437): every later renew fails the same
                    # way and the lease will expire mid-handler.
                    _log.error(
                        "lease renew refused with 403 Forbidden — this runner's credential "
                        "does not own runner_id, so the lease will expire and the execution "
                        "be reclaimed. Give the runner its own runner_id, or release the "
                        "existing binding with DELETE /v1/runners/{id}",
                        extra={
                            "runner_id": self._runner_id or "",
                            "execution_id": execution_id,
                        },
                    )
                    continue
                # Since #447 renew is a real per-execution lease: 404 (no
                # longer leased here) and 409 (already terminal) are the
                # normal outcome of a renew racing our own completion, so
                # they stay at debug alongside the transient failures.
                _log.debug(
                    "lease renew failed: %s", exc, extra={"execution_id": execution_id}
                )

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
            self._drain_hard_cancel = True
            for inflight in list(self._inflight.values()):
                inflight.cancellation.set()
            # Then wait for those acks (review 2026-10-24, K-4). This used to
            # sleep 200 ms and cancel the whole task, ack included; a
            # CancelledError inside ``await ack()`` slips past ``except
            # Exception``, so a slow ack — or any ack retry — was lost and the
            # execution stayed ``claimed`` until the reaper found it. Only the
            # handler is cancelled (via the event above); the task gets the
            # full ack budget plus a second for the handler to unwind.
            tasks = [inflight.task for inflight in self._inflight.values()]
            _done, pending = await asyncio.wait(tasks, timeout=self._ack_budget_s() + 1.0)
            # Last resort: a handler that swallows its cancellation and keeps
            # running would otherwise hold shutdown open forever.
            for task in pending:
                task.cancel()
            for task in pending:
                with contextlib.suppress(asyncio.CancelledError, Exception):
                    await task

    async def _sleep_or_drain(self, seconds: float) -> None:
        """Sleep, but return early if drain was requested."""
        with contextlib.suppress(TimeoutError):
            await asyncio.wait_for(self._drain_event.wait(), timeout=seconds)

    async def _sleep_until_slot_freed(self, seconds: float) -> None:
        """Sleep, but return early if a slot frees or drain was requested."""
        waiters = [
            asyncio.create_task(self._slot_freed.wait()),
            asyncio.create_task(self._drain_event.wait()),
        ]
        try:
            await asyncio.wait(waiters, timeout=seconds, return_when=asyncio.FIRST_COMPLETED)
        finally:
            for waiter in waiters:
                waiter.cancel()


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
