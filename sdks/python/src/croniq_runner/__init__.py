"""Async Python runner SDK for Croniq.

See https://github.com/nuetzliches/croniq for the server, OpenAPI spec, and
SDKs in other languages.

Public surface — every other symbol is an implementation detail and may move
without a major-version bump:

    Runner              — the poll/dispatch/ack loop
    RunnerOptions       — runner configuration
    ExecutionContext    — handed to each handler
    LogLevel            — string enum mirroring the server's log-level set
    LogWriter           — streaming log channel (use via `ExecutionContext.log_writer`)
    WorkEvent           — structured log event for `LogWriter.write`
    HandlerError        — handler raises this to control the failure message
    PollInstanceConflictError — poll kept conflicting: a duplicate runner_id (409)
    RunnerOwnershipDeniedError — a work endpoint refused this runner's credential (403)
    TriggerClient       — producer client for firing jobs via POST /v1/trigger
    TriggerClientOptions— trigger-client configuration (its own credentials)
    TriggerResult       — result of a trigger call

Quick start::

    import os

    from croniq_runner import Runner, RunnerOptions

    async def hello(ctx):
        ctx.logger.info("hello from %s", ctx.job_key)

    runner = Runner(RunnerOptions(server_url="http://localhost:4000",
                                   api_key=os.environ["CRONIQ_API_KEY"]))
    runner.add_handler("hello:world", hello)
    await runner.run()
"""

from croniq_runner._context import ExecutionContext
from croniq_runner._errors import (
    HandlerError,
    PollInstanceConflictError,
    RunnerOwnershipDeniedError,
)
from croniq_runner._log_writer import LogLevel, LogWriter
from croniq_runner._options import LogWriterOptions, RunnerOptions
from croniq_runner._protocol import WorkEvent
from croniq_runner._runner import Runner
from croniq_runner._trigger import TriggerClient, TriggerClientOptions, TriggerResult

__all__ = [
    "ExecutionContext",
    "HandlerError",
    "LogLevel",
    "LogWriter",
    "LogWriterOptions",
    "PollInstanceConflictError",
    "Runner",
    "RunnerOptions",
    "RunnerOwnershipDeniedError",
    "TriggerClient",
    "TriggerClientOptions",
    "TriggerResult",
    "WorkEvent",
]

__version__ = "0.1.0"
