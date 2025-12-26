import os
import socket
import sys
import time
from typing import Any, Dict, List, Optional

import requests


def env(key: str, default: Optional[str] = None) -> Optional[str]:
    value = os.getenv(key)
    if value is None or value.strip() == "":
        return default
    return value


def required_env(key: str) -> str:
    value = env(key)
    if value is None:
        raise RuntimeError(f"Missing required env var: {key}")
    return value


def post_json(url: str, api_key: str, body: Dict[str, Any]) -> requests.Response:
    headers = {
        "X-Croniq-Key": api_key,
        "Content-Type": "application/json",
    }
    return requests.post(url, json=body, headers=headers, timeout=60)


def main() -> int:
    base_url = required_env("CRONIQ_API_BASEURL").rstrip("/")
    tenant_id = env("CRONIQ_CORE_TENANT_ID", "default")
    environment_tag = env("CRONIQ_CORE_ENVIRONMENT", "dev")
    api_key = required_env("CRONIQ_SMOKE_API_KEY")

    runner_id = env(
        "CRONIQ_WORKER_RUNNER_ID",
        f"py-{socket.gethostname()}-{os.getpid()}",
    )

    batch_size = int(env("CRONIQ_WORKER_BATCH_SIZE", "1") or "1")
    wait_for_ms = int(env("CRONIQ_WORKER_WAIT_FOR_MS", "25000") or "25000")
    simulate_seconds = float(env("CRONIQ_WORKER_SIMULATE_SECONDS", "0") or "0")
    renew_every_seconds = float(env("CRONIQ_WORKER_RENEW_EVERY_SECONDS", "20") or "20")

    poll_url = f"{base_url}/tenants/{tenant_id}/work/poll?environment={environment_tag}"
    renew_url = f"{base_url}/tenants/{tenant_id}/work/renew?environment={environment_tag}"
    ack_url = f"{base_url}/tenants/{tenant_id}/work/ack?environment={environment_tag}"

    print(f"Croniq HTTP worker (python)")
    print(f"- base_url:   {base_url}")
    print(f"- tenant_id:  {tenant_id}")
    print(f"- env:        {environment_tag}")
    print(f"- runner_id:  {runner_id}")
    print(f"- batch_size: {batch_size}")
    print(f"- wait_for_ms:{wait_for_ms}")

    while True:
        body = {"runnerId": runner_id, "batchSize": batch_size, "waitForMs": wait_for_ms}
        resp = post_json(poll_url, api_key, body)
        if resp.status_code >= 400:
            raise RuntimeError(f"poll failed: {resp.status_code} {resp.text}")

        leases: List[Dict[str, Any]] = resp.json().get("leases") or []
        if not leases:
            continue

        for lease in leases:
            job_key = lease.get("jobKey")
            trigger_id = lease.get("triggerId")
            lease_id = lease.get("leaseId")
            print(f"claimed lease: jobKey={job_key} triggerId={trigger_id} leaseId={lease_id}")

            # Simulate work (optional). Renew lease periodically while processing.
            if simulate_seconds > 0:
                start = time.monotonic()
                next_renew = start + renew_every_seconds
                while True:
                    elapsed = time.monotonic() - start
                    if elapsed >= simulate_seconds:
                        break

                    if time.monotonic() >= next_renew:
                        renew_body = {"runnerId": runner_id, "lease": lease}
                        renew_resp = post_json(renew_url, api_key, renew_body)
                        if renew_resp.status_code == 200:
                            updated = renew_resp.json().get("lease")
                            if updated:
                                lease = updated
                        elif renew_resp.status_code == 404:
                            print("lease renewal rejected (not found); will still attempt ack")
                        else:
                            raise RuntimeError(f"renew failed: {renew_resp.status_code} {renew_resp.text}")

                        next_renew = time.monotonic() + renew_every_seconds

                    time.sleep(0.25)

            ack_body = {"runnerId": runner_id, "lease": lease, "succeeded": True}
            ack_resp = post_json(ack_url, api_key, ack_body)
            if ack_resp.status_code != 204:
                raise RuntimeError(f"ack failed: {ack_resp.status_code} {ack_resp.text}")

            print(f"acked lease: leaseId={lease.get('leaseId')}")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\nshutting down")
        raise
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        raise
