"""Public error types raised by the SDK."""

from __future__ import annotations

import httpx


class HandlerError(Exception):
    """Raise from a handler to control the ack failure message.

    Equivalent to throwing any other exception, except the message is
    user-supplied: regular ``Exception`` subclasses use ``str(exc)`` which
    leaks the class name for some builtins.
    """

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


class RunnerOwnershipDeniedError(Exception):
    """A work endpoint answered ``403 Forbidden``.

    The authenticated credential is bound to a different ``runner_id`` than
    the one this runner names in its requests (server issue #436). Unlike a
    ``409`` — where a duplicate deployment may release the identity on its
    own — this is **permanent**: retrying cannot clear it. :meth:`Runner.run`
    raises this instead of polling forever, so a misconfigured runner exits
    non-zero rather than looking merely idle (issue #437).

    The fix is an operator action: give the runner its own ``runner_id``, or
    release the existing binding with ``DELETE /v1/runners/{id}``.
    """

    def __init__(self, runner_id: str) -> None:
        super().__init__(
            "work ownership denied — the credential this runner authenticates with does "
            f"not own runner_id '{runner_id}'. The server answered 403 Forbidden on "
            "POST /v1/work/poll and will keep doing so: give this runner its own "
            "runner_id, or release the existing binding with DELETE /v1/runners/{id}."
        )
        self.runner_id = runner_id


class PollInstanceConflictError(Exception):
    """``POST /v1/work/poll`` answered ``409 Conflict`` too many times in a row.

    A ``409`` means a newer instance has taken this ``runner_id`` over
    (fencing, server issue #374). A single one is transient — the deposed
    instance may win the identity back — so the runner backs off and retries.
    A streak of them is not: it is a duplicate deployment, two processes
    started with the same fixed ``runner_id``. Retrying forever there leaves
    the misconfiguration behind a warning that scrolls past, so
    :meth:`Runner.run` raises this once
    :attr:`RunnerOptions.max_consecutive_poll_conflicts` consecutive
    conflicts have been seen (issue #134 sub-item 1).

    Distinct from :class:`RunnerOwnershipDeniedError`, which is a ``403`` and
    permanent from the first response.
    """

    def __init__(self, runner_id: str, consecutive_count: int) -> None:
        super().__init__(
            "poll instance conflict — another runner is already registered with "
            f"runner_id '{runner_id}'. Observed {consecutive_count} consecutive 409 "
            "Conflict responses on POST /v1/work/poll. Stop the duplicate process or "
            "rotate the runner_id."
        )
        self.runner_id = runner_id
        self.consecutive_count = consecutive_count


def is_instance_conflict(exc: BaseException) -> bool:
    """Return ``True`` when ``exc`` is a ``409`` from the poll endpoint.

    Counted rather than acted on immediately — see
    :class:`PollInstanceConflictError`.
    """
    return isinstance(exc, httpx.HTTPStatusError) and exc.response.status_code == 409


def is_ownership_denied(exc: BaseException) -> bool:
    """Return ``True`` when ``exc`` is a ``403`` from a work endpoint.

    Kept here rather than inline so the poll loop, the renew loop, the ack
    path and the log writer all decide the same way.
    """
    return isinstance(exc, httpx.HTTPStatusError) and exc.response.status_code == 403


class NoHandlerRegisteredError(Exception):
    """Server delivered a job key the runner has no handler for."""

    def __init__(self, job_key: str) -> None:
        super().__init__(f"no handler registered for job_key '{job_key}'")
        self.job_key = job_key
