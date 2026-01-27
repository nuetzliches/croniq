import os
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

sdk_root = Path(__file__).resolve().parents[4] / "sdk" / "runner-python"
sys.path.insert(0, str(sdk_root))

from croniq_runner import WorkEvent, RunnerClient


def env(key: str, default: str) -> str:
    value = os.getenv(key)
    if value is None or value.strip() == "":
        return default
    return value


def main() -> None:
    base_url = env("CRONIQ_API_BASEURL", "http://localhost:5080")
    tenant_id = env("CRONIQ_TENANT_ID", "default")
    environment = env("CRONIQ_ENVIRONMENT", "dev")
    api_key = env("CRONIQ_API_KEY", "")
    runner_id = env("CRONIQ_RUNNER_ID", "default")

    client = RunnerClient(
        base_url=base_url,
        tenant_id=tenant_id,
        environment=environment,
        api_key=api_key,
    )

    print("Croniq HTTP runner (python)")
    print(f"- base_url:    {base_url}")
    print(f"- tenant_id:   {tenant_id}")
    print(f"- environment: {environment}")
    print(f"- runner_id:   {runner_id}")

    stop_event = threading.Event()

    def heartbeat_loop() -> None:
        while not stop_event.is_set():
            try:
                client.heartbeat(
                    runner_id=runner_id,
                    environment_tag=environment,
                    seen_at_utc=datetime.now(timezone.utc).isoformat(),
                )
            except Exception:
                pass
            stop_event.wait(15)

    threading.Thread(target=heartbeat_loop, daemon=True).start()

    while True:
        leases = client.poll(runner_id=runner_id, batch_size=1, wait_for_ms=25000)
        for lease in leases:
            print(
                f"claimed lease: jobKey={lease.job_key} triggerId={lease.trigger_id} leaseId={lease.lease_id}"
            )
            if lease.execution_mode or lease.invocation_source:
                mode = lease.execution_mode or "normal"
                source = lease.invocation_source or "schedule"
                print(f"- intent: mode={mode} source={source}")
            client.events(
                runner_id=runner_id,
                lease=lease,
                events=[
                    WorkEvent(
                        message=f"processing execution {lease.execution_id}",
                        level="Information",
                        event_type="runner",
                    )
                ],
            )
            client.ack(runner_id=runner_id, lease=lease, succeeded=True)
            print(f"acked lease: leaseId={lease.lease_id}")


if __name__ == "__main__":
    main()
