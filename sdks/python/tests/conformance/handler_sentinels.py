"""Translate a YAML handler ``behavior`` into a Croniq handler coroutine.

Every conformance binding must implement the same sentinels:
``noop``, ``throw``, ``sleep``, ``log``, ``stream_logs``.
"""

from __future__ import annotations

import asyncio
from collections.abc import Callable, Coroutine
from typing import Any

from croniq_runner import ExecutionContext, HandlerError, LogLevel, Runner
from tests.conformance.case_spec import HandlerSpec


def apply_to(runner: Runner, handlers: list[HandlerSpec]) -> None:
    for spec in handlers:
        handler = _make_handler(spec)
        if spec.is_default:
            runner.set_default_handler(handler)
        else:
            runner.add_handler(spec.job_key, handler, schedule=spec.schedule)


def _make_handler(spec: HandlerSpec) -> Callable[[ExecutionContext], Coroutine[Any, Any, None]]:
    behavior = spec.behavior
    if behavior == "noop":
        async def noop(ctx: ExecutionContext) -> None:
            return None
        return noop

    if behavior == "throw":
        msg = spec.error_message or "thrown by conformance handler"
        async def throw(ctx: ExecutionContext) -> None:
            raise HandlerError(msg)
        return throw

    if behavior == "sleep":
        duration = (spec.duration_ms or 0) / 1000.0
        async def sleep(ctx: ExecutionContext) -> None:
            # Cooperative sleep — fires immediately on cancellation event.
            try:
                await asyncio.wait_for(ctx.cancellation.wait(), timeout=duration)
            except TimeoutError:
                return
            # If we got here, cancellation fired.
            raise asyncio.CancelledError("cancelled during sleep")
        return sleep

    if behavior == "log":
        level = _parse_level(spec.level)
        count = spec.count or 1
        message = spec.message or ""
        async def log(ctx: ExecutionContext) -> None:
            for _ in range(count):
                ctx.logger.log(level, message)
        return log

    if behavior == "stream_logs":
        count = spec.count or 1
        interval = (spec.interval_ms or 0) / 1000.0
        log_level = _parse_log_level(spec.level)
        async def stream(ctx: ExecutionContext) -> None:
            writer = ctx.log_writer
            for i in range(count):
                await writer.write(f"line {i + 1}", level=log_level)
                if interval > 0 and i + 1 < count:
                    await asyncio.sleep(interval)
        return stream

    raise NotImplementedError(f"unknown handler behavior '{behavior}'")


def _parse_level(level: str | None) -> int:
    """Map a YAML level string to a stdlib logging int."""
    import logging
    return {
        "trace": logging.DEBUG,
        "debug": logging.DEBUG,
        "info": logging.INFO,
        "warn": logging.WARNING,
        "error": logging.ERROR,
    }.get((level or "info").lower(), logging.INFO)


def _parse_log_level(level: str | None) -> LogLevel:
    return {
        "trace": LogLevel.TRACE,
        "debug": LogLevel.DEBUG,
        "info": LogLevel.INFO,
        "warn": LogLevel.WARN,
        "error": LogLevel.ERROR,
    }.get((level or "info").lower(), LogLevel.INFO)
