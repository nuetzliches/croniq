"""Public error types raised by the SDK."""

from __future__ import annotations


class HandlerError(Exception):
    """Raise from a handler to control the ack failure message.

    Equivalent to throwing any other exception, except the message is
    user-supplied: regular ``Exception`` subclasses use ``str(exc)`` which
    leaks the class name for some builtins.
    """

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


class NoHandlerRegisteredError(Exception):
    """Server delivered a job key the runner has no handler for."""

    def __init__(self, job_key: str) -> None:
        super().__init__(f"no handler registered for job_key '{job_key}'")
        self.job_key = job_key
