from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import quote

import requests


class CroniqError(Exception):
    def __init__(self, status_code: int, message: str) -> None:
        super().__init__(message)
        self.status_code = status_code


class LeaseConflictError(CroniqError):
    pass


class LeaseNotFoundError(CroniqError):
    pass


@dataclass(frozen=True)
class Lease:
    execution_id: str
    lease_id: str
    trigger_id: str
    job_key: str
    fire_at_utc: str
    lease_expires_at_utc: str
    payload: Optional[str]

    @staticmethod
    def from_dict(data: Dict[str, Any]) -> "Lease":
        return Lease(
            execution_id=data["executionId"],
            lease_id=data["leaseId"],
            trigger_id=data["triggerId"],
            job_key=data["jobKey"],
            fire_at_utc=data["fireAtUtc"],
            lease_expires_at_utc=data["leaseExpiresAtUtc"],
            payload=data.get("payload"),
        )

    def to_dict(self) -> Dict[str, Any]:
        return {
            "executionId": self.execution_id,
            "leaseId": self.lease_id,
            "triggerId": self.trigger_id,
            "jobKey": self.job_key,
            "fireAtUtc": self.fire_at_utc,
            "leaseExpiresAtUtc": self.lease_expires_at_utc,
            "payload": self.payload,
        }


@dataclass(frozen=True)
class WorkEvent:
    message: str
    level: Optional[str] = None
    timestamp_utc: Optional[str] = None
    properties: Optional[Dict[str, str]] = None
    event_type: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        payload: Dict[str, Any] = {"message": self.message}
        if self.level:
            payload["level"] = self.level
        if self.timestamp_utc:
            payload["timestampUtc"] = self.timestamp_utc
        if self.properties:
            payload["properties"] = self.properties
        if self.event_type:
            payload["eventType"] = self.event_type
        return payload


class WorkerClient:
    def __init__(
        self,
        base_url: str,
        tenant_id: str,
        environment: str,
        api_key: Optional[str] = None,
        bearer_token: Optional[str] = None,
        timeout_seconds: int = 60,
    ) -> None:
        if not base_url:
            raise ValueError("base_url is required")
        if not tenant_id:
            raise ValueError("tenant_id is required")
        if not api_key and not bearer_token:
            raise ValueError("api_key or bearer_token is required")

        self._base_url = base_url.rstrip("/")
        self._tenant_id = tenant_id
        self._environment = environment
        self._api_key = api_key
        self._bearer_token = bearer_token
        self._timeout_seconds = timeout_seconds

    def poll(self, runner_id: str, batch_size: int = 1, wait_for_ms: int = 0) -> List[Lease]:
        payload = {"runnerId": runner_id, "batchSize": batch_size, "waitForMs": wait_for_ms}
        response = self._post("/work/poll", payload)
        data = response.json()
        return [Lease.from_dict(item) for item in data.get("leases") or []]

    def renew(self, runner_id: str, lease: Lease) -> Tuple[bool, Optional[Lease]]:
        payload = {"runnerId": runner_id, "lease": lease.to_dict()}
        try:
            response = self._post("/work/renew", payload)
        except LeaseNotFoundError:
            return False, None

        data = response.json()
        if not data.get("renewed"):
            return False, None

        updated = data.get("lease")
        return True, Lease.from_dict(updated) if updated else None

    def ack(
        self,
        runner_id: str,
        lease: Lease,
        succeeded: bool,
        next_fire_time_utc: Optional[str] = None,
        dead_letter_reason: Optional[str] = None,
    ) -> None:
        payload: Dict[str, Any] = {
            "runnerId": runner_id,
            "lease": lease.to_dict(),
            "succeeded": succeeded,
        }
        if next_fire_time_utc:
            payload["nextFireTimeUtc"] = next_fire_time_utc
        if dead_letter_reason:
            payload["deadLetterReason"] = dead_letter_reason
        self._post("/work/ack", payload)

    def events(self, runner_id: str, lease: Lease, events: List[WorkEvent]) -> None:
        payload = {
            "runnerId": runner_id,
            "lease": lease.to_dict(),
            "events": [event.to_dict() for event in events],
        }
        self._post(f"/work/{lease.execution_id}:events", payload)

    def _post(self, suffix: str, payload: Dict[str, Any]) -> requests.Response:
        url = self._build_url(suffix)
        headers = {"Content-Type": "application/json"}
        if self._bearer_token:
            headers["Authorization"] = f"Bearer {self._bearer_token}"
        else:
            headers["X-Croniq-Key"] = self._api_key or ""

        response = requests.post(url, json=payload, headers=headers, timeout=self._timeout_seconds)
        if response.status_code == 404:
            raise LeaseNotFoundError(response.status_code, response.text)
        if response.status_code == 409:
            raise LeaseConflictError(response.status_code, response.text)
        if response.status_code >= 400:
            raise CroniqError(response.status_code, response.text)
        return response

    def _build_url(self, suffix: str) -> str:
        url = f"{self._base_url}/tenants/{quote(self._tenant_id)}/{suffix.lstrip('/')}"
        if self._environment:
            url = f"{url}?environment={quote(self._environment)}"
        return url
