"""Fire a Croniq job on demand with the producer client.

The runner (see quickstart.py) is the *consumer* side. This is the *producer*:
`TriggerClient` wraps `POST /v1/trigger` so an application can fire a job in
response to an event, independently of the Croniqfile schedule. It carries its
own credentials — the endpoint needs the ``jobs:trigger`` (or ``admin``) scope,
which runner poll keys typically do not hold.

Run with::

    pip install croniq-runner
    CRONIQ_TRIGGER_API_KEY=... python examples/trigger.py
"""

from __future__ import annotations

import asyncio
import os

from croniq_runner import TriggerClient, TriggerClientOptions


async def main() -> None:
    async with TriggerClient(
        TriggerClientOptions(
            server_url=os.environ.get("CRONIQ_URL", "http://localhost:4000"),
            api_key=os.environ.get("CRONIQ_TRIGGER_API_KEY"),
        )
    ) as client:
        result = await client.trigger(
            "billing:invoice",
            metadata={"customer_id": "acme", "invoice_id": "inv_42"},
            # Optional dedup key: a redelivery/retry of the same event coalesces
            # onto the existing execution instead of enqueuing a duplicate.
            idempotency_key="evt-2026-07-14-001",
        )
        print(
            f"triggered: execution_id={result.execution_id} "
            f"queued={result.queued} deduplicated={result.deduplicated}"
        )


if __name__ == "__main__":
    asyncio.run(main())
