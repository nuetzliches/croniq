"""Minimal Croniq runner — polls a local server, prints when work arrives.

Run with::

    pip install croniq-runner
    CRONIQ_API_KEY=... python examples/quickstart.py
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import os

from croniq_runner import ExecutionContext, LogLevel, Runner, RunnerOptions


async def hello(ctx: ExecutionContext) -> None:
    ctx.logger.info(
        "received job (execution_id=%s attempt=%d)", ctx.execution_id, ctx.attempt
    )
    await ctx.log("hello from the python sdk", level=LogLevel.INFO)


async def billing_invoice(ctx: ExecutionContext) -> None:
    customer = ctx.metadata.get("customer_id", "<unknown>")
    ctx.logger.info("generating invoice for %s", customer)
    async with asyncio.timeout(60):
        # ... your business logic here ...
        await asyncio.sleep(0.1)


async def main() -> None:
    logging.basicConfig(level=logging.INFO)

    runner = Runner(
        RunnerOptions(
            server_url=os.environ.get("CRONIQ_URL", "http://localhost:4000"),
            api_key=os.environ.get("CRONIQ_API_KEY"),
            capabilities=["billing"],
            tags=["lang=python", "env=dev"],
            max_inflight=5,
        )
    )
    runner.add_handler("hello:world", hello)
    runner.add_handler("billing:invoice", billing_invoice, schedule="5m")

    with contextlib.suppress(KeyboardInterrupt):
        await runner.run()


if __name__ == "__main__":
    asyncio.run(main())
