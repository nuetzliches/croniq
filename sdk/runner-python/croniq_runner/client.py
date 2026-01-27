from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Dict, List, Optional, Tuple
from urllib.parse import quote

import requests
import asyncio
import importlib.util
import os
import sys
import tempfile
import json
import uuid
import grpc
from grpc import aio
from grpc_tools import protoc


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
    execution_mode: Optional[str] = None
    invocation_source: Optional[str] = None

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
            execution_mode=data.get("executionMode"),
            invocation_source=data.get("invocationSource"),
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
            "executionMode": self.execution_mode,
            "invocationSource": self.invocation_source,
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


class RunnerClient:
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

    def poll(
        self,
        runner_id: str,
        batch_size: int = 1,
        wait_for_ms: int = 0,
        allow_test_executions: Optional[bool] = None,
        max_inflight: Optional[int] = None,
        capabilities: Optional[List[str]] = None,
    ) -> List[Lease]:
        payload: Dict[str, Any] = {
            "runnerId": runner_id,
            "batchSize": batch_size,
            "waitForMs": wait_for_ms,
        }
        if allow_test_executions is not None:
            payload["allowTestExecutions"] = allow_test_executions
        if max_inflight is not None:
            payload["maxInflight"] = max_inflight
        if capabilities:
            payload["capabilities"] = capabilities
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


@dataclass(frozen=True)
class RunnerExecutionContext:
    execution_id: str
    lease_id: str
    trigger_id: str
    job_key: str
    fire_at_utc: str
    lease_expires_at_utc: str
    execution_mode: Optional[str] = None
    invocation_source: Optional[str] = None
    emit_event: Optional[Callable[["WorkEvent"], Awaitable[None]]] = None


@dataclass(frozen=True)
class RunnerConfig:
    base_url: str
    tenant_id: str
    environment: str
    runner_id: str
    api_key: Optional[str] = None
    bearer_token: Optional[str] = None
    grpc_base_url: Optional[str] = None
    transport_mode: str = "auto"
    allow_test_executions: bool = False
    max_inflight: int = 1
    capabilities: Optional[List[str]] = None
    poll_batch_size: int = 1
    poll_wait_ms: int = 25000
    request_timeout_seconds: int = 60
    renew_lead_ms: int = 10000
    retry_base_ms: int = 500
    retry_max_ms: int = 10000
    retry_max_attempts: Optional[int] = None
    parse_payload_json: bool = False
    outbox_path: Optional[str] = None
    outbox_max_entries: int = 500
    outbox_max_bytes: int = 1_000_000


class RunnerLogger:
    def info(self, message: str, data: Optional[Dict[str, Any]] = None) -> None:
        print(message, data or {})

    def warn(self, message: str, data: Optional[Dict[str, Any]] = None) -> None:
        print(message, data or {})

    def error(self, message: str, data: Optional[Dict[str, Any]] = None) -> None:
        print(message, data or {})


class CroniqRunner:
    def __init__(self, config: RunnerConfig) -> None:
        if not config.base_url:
            raise ValueError("base_url is required")
        if not config.tenant_id:
            raise ValueError("tenant_id is required")
        if not config.runner_id:
            raise ValueError("runner_id is required")
        if config.transport_mode not in {"auto", "grpc", "polling"}:
            raise ValueError("transport_mode must be auto, grpc, or polling")

        self._config = config
        self._client = RunnerClient(
            base_url=config.base_url,
            tenant_id=config.tenant_id,
            environment=config.environment,
            api_key=config.api_key,
            bearer_token=config.bearer_token,
            timeout_seconds=config.request_timeout_seconds,
        )
        self._logger = RunnerLogger()
        self._handler: Optional[Callable[[RunnerExecutionContext, Any, RunnerLogger], Awaitable[None]]] = None
        self._inflight: Dict[str, Lease] = {}
        self._renew_tasks: Dict[str, asyncio.Task[None]] = {}
        self._queue: asyncio.Queue[Lease] = asyncio.Queue()
        self._running = False
        self._grpc_stream: Optional[aio.StreamStreamCall] = None
        self._grpc_lock = asyncio.Lock()
        self._grpc_connected = asyncio.Event()
        self._grpc_modules = None
        self._outbox: List[Dict[str, Any]] = []
        self._outbox_lock = asyncio.Lock()
        self._outbox_path = config.outbox_path or os.path.join(os.getcwd(), ".croniq", "runner-outbox.jsonl")

    def on_execute(self, handler: Callable[[RunnerExecutionContext, Any, RunnerLogger], Awaitable[None]]) -> None:
        self._handler = handler

    async def start(self) -> None:
        if not self._handler:
            raise RuntimeError("on_execute handler must be registered before start")
        self._running = True

        await self._load_outbox()

        tasks: List[asyncio.Task[None]] = []
        if self._config.transport_mode != "polling":
            tasks.append(asyncio.create_task(self._run_grpc()))
        if self._config.transport_mode != "grpc":
            tasks.append(asyncio.create_task(self._run_polling()))
        tasks.append(asyncio.create_task(self._run_dispatch_loop()))
        tasks.append(asyncio.create_task(self._replay_outbox_loop()))

        await asyncio.gather(*tasks)

    async def stop(self) -> None:
        self._running = False
        if self._grpc_stream is not None:
            self._grpc_stream.cancel()
            self._grpc_stream = None
        for task in self._renew_tasks.values():
            task.cancel()
        self._renew_tasks.clear()
        self._inflight.clear()
        self._grpc_connected.clear()

    async def _run_grpc(self) -> None:
        attempt = 0
        while self._running:
            try:
                await self._connect_grpc()
                attempt = 0
            except Exception as exc:  # noqa: BLE001
                attempt += 1
                if self._config.retry_max_attempts and attempt >= self._config.retry_max_attempts:
                    self._logger.error("gRPC reconnect exhausted", {"error": str(exc)})
                    return
                delay = self._next_delay(attempt)
                await asyncio.sleep(delay / 1000)

    async def _connect_grpc(self) -> None:
        self._grpc_modules = self._grpc_modules or _load_grpc_modules()
        runner_pb2, runner_pb2_grpc = self._grpc_modules

        endpoint = self._config.grpc_base_url or self._config.base_url
        if endpoint.startswith("https://"):
            channel = aio.secure_channel(endpoint, grpc.ssl_channel_credentials())
        else:
            channel = aio.insecure_channel(endpoint)

        metadata = []
        if self._config.bearer_token:
            metadata.append(("authorization", f"Bearer {self._config.bearer_token}"))
        elif self._config.api_key:
            metadata.append(("x-croniq-key", self._config.api_key))

        stub = runner_pb2_grpc.RunnerStub(channel)
        stream = stub.Connect(metadata=metadata)
        self._grpc_stream = stream
        self._grpc_connected.clear()

        capabilities = {cap.strip(): "true" for cap in (self._config.capabilities or []) if cap.strip()}
        await stream.write(
            runner_pb2.RunnerMessage(
                hello=runner_pb2.RunnerHello(
                    runner_id=self._config.runner_id,
                    max_inflight=self._config.max_inflight,
                    allow_test_executions=self._config.allow_test_executions,
                    capabilities=capabilities,
                )
            )
        )

        async for response in stream:
            if response and response.hello and not self._grpc_connected.is_set():
                self._grpc_connected.set()
            if response and response.assigned:
                lease = _lease_from_grpc(response.assigned)
                await self._queue.put(lease)

        self._grpc_connected.clear()

    async def _run_polling(self) -> None:
        while self._running:
            if self._config.transport_mode == "auto" and self._grpc_connected.is_set():
                await asyncio.sleep(0.25)
                continue

            try:
                leases = await asyncio.to_thread(
                    self._client.poll,
                    runner_id=self._config.runner_id,
                    batch_size=self._config.poll_batch_size,
                    wait_for_ms=self._config.poll_wait_ms,
                    allow_test_executions=self._config.allow_test_executions,
                    max_inflight=self._config.max_inflight,
                    capabilities=self._config.capabilities,
                )
                for lease in leases:
                    await self._queue.put(lease)
            except Exception as exc:  # noqa: BLE001
                self._logger.warn("poll failed", {"error": str(exc)})
                await asyncio.sleep(self._next_delay(1) / 1000)

    async def _run_dispatch_loop(self) -> None:
        while self._running:
            if len(self._inflight) >= self._config.max_inflight:
                await asyncio.sleep(0.05)
                continue

            lease = await self._queue.get()
            if lease.lease_id in self._inflight:
                continue

            self._inflight[lease.lease_id] = lease
            self._renew_tasks[lease.lease_id] = asyncio.create_task(self._renew_loop(lease))
            asyncio.create_task(self._execute_lease(lease))

    async def _execute_lease(self, lease: Lease) -> None:
        context = RunnerExecutionContext(
            execution_id=lease.execution_id,
            lease_id=lease.lease_id,
            trigger_id=lease.trigger_id,
            job_key=lease.job_key,
            fire_at_utc=lease.fire_at_utc,
            lease_expires_at_utc=lease.lease_expires_at_utc,
            execution_mode=lease.execution_mode,
            invocation_source=lease.invocation_source,
            emit_event=lambda event: self._send_events(lease, [event], allow_outbox=True),
        )

        if not self._config.allow_test_executions and (lease.execution_mode or "").lower() == "test":
            await self._reject_test(lease)
            await self._complete_lease(lease)
            return

        payload = _parse_payload(lease.payload, self._config.parse_payload_json)
        try:
            await self._handler(context, payload, self._logger)  # type: ignore[misc]
            await self._ack_success(lease)
        except Exception as exc:  # noqa: BLE001
            await self._ack_failure(lease, exc)
        finally:
            await self._complete_lease(lease)

    async def _ack_success(self, lease: Lease, allow_outbox: bool = True) -> None:
        if self._grpc_connected.is_set() and self._grpc_stream:
            await self._grpc_send(
                self._grpc_modules[0].RunnerMessage(
                    ack_success=self._grpc_modules[0].WorkAckSuccess(
                        execution_id=lease.execution_id,
                        lease_id=lease.lease_id,
                    )
                )
            )
            return

        try:
            await asyncio.to_thread(
                self._client.ack,
                runner_id=self._config.runner_id,
                lease=lease,
                succeeded=True,
            )
        except Exception:  # noqa: BLE001
            if allow_outbox:
                await self._enqueue_outbox({
                    "id": str(uuid.uuid4()),
                    "type": "ack_success",
                    "payload": {"lease": lease.to_dict()},
                    "attempts": 0,
                    "created_at": asyncio.get_event_loop().time(),
                })

    async def _ack_failure(self, lease: Lease, exc: Exception, allow_outbox: bool = True) -> None:
        if self._grpc_connected.is_set() and self._grpc_stream:
            await self._grpc_send(
                self._grpc_modules[0].RunnerMessage(
                    ack_failure=self._grpc_modules[0].WorkAckFailure(
                        execution_id=lease.execution_id,
                        lease_id=lease.lease_id,
                        error_type="execution-failed",
                        error_message=str(exc),
                    )
                )
            )
            return

        try:
            await asyncio.to_thread(
                self._client.ack,
                runner_id=self._config.runner_id,
                lease=lease,
                succeeded=False,
                dead_letter_reason="execution-failed",
            )
        except Exception:  # noqa: BLE001
            if allow_outbox:
                await self._enqueue_outbox({
                    "id": str(uuid.uuid4()),
                    "type": "ack_failure",
                    "payload": {
                        "lease": lease.to_dict(),
                        "error_type": "execution-failed",
                        "error_message": str(exc),
                        "dead_letter_reason": "execution-failed",
                    },
                    "attempts": 0,
                    "created_at": asyncio.get_event_loop().time(),
                })

    async def _reject_test(self, lease: Lease, allow_outbox: bool = True) -> None:
        if self._grpc_connected.is_set() and self._grpc_stream:
            await self._grpc_send(
                self._grpc_modules[0].RunnerMessage(
                    ack_failure=self._grpc_modules[0].WorkAckFailure(
                        execution_id=lease.execution_id,
                        lease_id=lease.lease_id,
                        error_type="test-not-allowed",
                        error_message="test executions are disabled for this runner",
                        dead_letter_reason="test-not-allowed",
                    )
                )
            )
            return

        try:
            await asyncio.to_thread(
                self._client.ack,
                runner_id=self._config.runner_id,
                lease=lease,
                succeeded=False,
                dead_letter_reason="test-not-allowed",
            )
        except Exception:  # noqa: BLE001
            if allow_outbox:
                await self._enqueue_outbox({
                    "id": str(uuid.uuid4()),
                    "type": "ack_failure",
                    "payload": {
                        "lease": lease.to_dict(),
                        "error_type": "test-not-allowed",
                        "error_message": "test executions are disabled for this runner",
                        "dead_letter_reason": "test-not-allowed",
                    },
                    "attempts": 0,
                    "created_at": asyncio.get_event_loop().time(),
                })

    async def _renew_loop(self, lease: Lease) -> None:
        while self._running and lease.lease_id in self._inflight:
            delay = _renew_delay_ms(lease.lease_expires_at_utc, self._config.renew_lead_ms)
            await asyncio.sleep(delay / 1000)
            try:
                renewed, updated = await asyncio.to_thread(
                    self._client.renew,
                    runner_id=self._config.runner_id,
                    lease=lease,
                )
                if renewed and updated:
                    self._inflight[lease.lease_id] = updated
                    lease = updated
            except Exception as exc:  # noqa: BLE001
                self._logger.warn("renew failed", {"error": str(exc), "leaseId": lease.lease_id})

    async def _grpc_send(self, message: Any) -> None:
        if not self._grpc_stream:
            return
        async with self._grpc_lock:
            await self._grpc_stream.write(message)

    async def _send_events(self, lease: Lease, events: List[WorkEvent], allow_outbox: bool) -> None:
        if self._grpc_connected.is_set() and self._grpc_stream:
            runner_pb2 = self._grpc_modules[0]
            await self._grpc_send(
                runner_pb2.RunnerMessage(
                    events=runner_pb2.WorkEvents(
                        execution_id=lease.execution_id,
                        lease_id=lease.lease_id,
                        events=[
                            runner_pb2.WorkEvent(
                                message=event.message,
                                level=event.level or "",
                                timestamp_utc=int(_iso_to_epoch_ms(event.timestamp_utc)) if event.timestamp_utc else 0,
                                properties=event.properties or {},
                                event_type=event.event_type or "",
                            )
                            for event in events
                        ],
                    )
                )
            )
            return

        try:
            await asyncio.to_thread(
                self._client.events,
                runner_id=self._config.runner_id,
                lease=lease,
                events=events,
            )
        except Exception:  # noqa: BLE001
            if allow_outbox:
                await self._enqueue_outbox({
                    "id": str(uuid.uuid4()),
                    "type": "events",
                    "payload": {
                        "lease": lease.to_dict(),
                        "events": [event.to_dict() for event in events],
                    },
                    "attempts": 0,
                    "created_at": asyncio.get_event_loop().time(),
                })

    async def _complete_lease(self, lease: Lease) -> None:
        task = self._renew_tasks.pop(lease.lease_id, None)
        if task:
            task.cancel()
        self._inflight.pop(lease.lease_id, None)

    async def _load_outbox(self) -> None:
        try:
            if not os.path.exists(self._outbox_path):
                return
            with open(self._outbox_path, "r", encoding="utf-8") as handle:
                lines = [line.strip() for line in handle.readlines() if line.strip()]
            self._outbox = [json.loads(line) for line in lines]
        except Exception:
            self._outbox = []

    async def _persist_outbox(self) -> None:
        os.makedirs(os.path.dirname(self._outbox_path), exist_ok=True)
        with open(self._outbox_path, "w", encoding="utf-8") as handle:
            handle.write("\n".join(json.dumps(item) for item in self._outbox))

        try:
            if os.path.getsize(self._outbox_path) > self._config.outbox_max_bytes and len(self._outbox) > 1:
                self._outbox = self._outbox[len(self._outbox) // 2 :]
                with open(self._outbox_path, "w", encoding="utf-8") as handle:
                    handle.write("\n".join(json.dumps(item) for item in self._outbox))
        except OSError:
            pass

    async def _enqueue_outbox(self, entry: Dict[str, Any]) -> None:
        async with self._outbox_lock:
            self._outbox.append(entry)
            if len(self._outbox) > self._config.outbox_max_entries:
                self._outbox = self._outbox[-self._config.outbox_max_entries:]
            await self._persist_outbox()

    async def _remove_outbox(self, entry_id: str) -> None:
        async with self._outbox_lock:
            self._outbox = [item for item in self._outbox if item.get("id") != entry_id]
            await self._persist_outbox()

    async def _mark_outbox_failed(self, entry_id: str) -> None:
        async with self._outbox_lock:
            for entry in self._outbox:
                if entry.get("id") == entry_id:
                    entry["attempts"] = entry.get("attempts", 0) + 1
                    break
            await self._persist_outbox()

    async def _replay_outbox_loop(self) -> None:
        while self._running:
            if not self._outbox:
                await asyncio.sleep(1)
                continue

            for entry in list(self._outbox):
                try:
                    if entry.get("type") == "ack_success":
                        lease = Lease.from_dict(entry["payload"]["lease"])
                        await self._ack_success(lease, allow_outbox=False)
                    elif entry.get("type") == "ack_failure":
                        payload = entry["payload"]
                        lease = Lease.from_dict(payload["lease"])
                        await self._ack_failure(lease, Exception(payload.get("error_message", "ack failed")), allow_outbox=False)
                    elif entry.get("type") == "events":
                        payload = entry["payload"]
                        lease = Lease.from_dict(payload["lease"])
                        events = [
                            WorkEvent(
                                message=event["message"],
                                level=event.get("level"),
                                timestamp_utc=event.get("timestampUtc"),
                                properties=event.get("properties"),
                                event_type=event.get("eventType"),
                            )
                            for event in payload.get("events", [])
                        ]
                        await self._send_events(lease, events, allow_outbox=False)

                    await self._remove_outbox(entry["id"])
                except Exception:
                    await self._mark_outbox_failed(entry["id"])
                    await asyncio.sleep(self._next_delay(entry.get("attempts", 1)) / 1000)

    def _next_delay(self, attempt: int) -> int:
        base = min(self._config.retry_max_ms, self._config.retry_base_ms * (2 ** max(0, attempt - 1)))
        jitter = base * 0.2 * (os.urandom(1)[0] / 255)
        return int(base + jitter)


def _parse_payload(payload: Optional[str], parse_json: bool) -> Any:
    if payload is None:
        return None
    if not parse_json:
        return payload
    try:
        import json

        return json.loads(payload)
    except Exception:  # noqa: BLE001
        return payload


def _renew_delay_ms(expires_at_utc: str, lead_ms: int) -> int:
    expires = int(_iso_to_epoch_ms(expires_at_utc))
    return max(1000, expires - int(_epoch_ms()) - lead_ms)


def _epoch_ms() -> int:
    import time

    return int(time.time() * 1000)


def _iso_to_epoch_ms(iso_value: str) -> int:
    from datetime import datetime

    parsed = datetime.fromisoformat(iso_value.replace("Z", "+00:00"))
    return int(parsed.timestamp() * 1000)


def _lease_from_grpc(assigned: Any) -> Lease:
    return Lease(
        execution_id=assigned.execution_id,
        lease_id=assigned.lease_id,
        trigger_id=assigned.trigger_id,
        job_key=assigned.job_key,
        fire_at_utc=_epoch_ms_to_iso(assigned.fire_at_utc),
        lease_expires_at_utc=_epoch_ms_to_iso(assigned.lease_expires_at_utc),
        payload=assigned.payload or None,
        execution_mode=assigned.execution_mode or None,
        invocation_source=assigned.invocation_source or None,
    )


def _epoch_ms_to_iso(value: int) -> str:
    from datetime import datetime, timezone

    return datetime.fromtimestamp(value / 1000, tz=timezone.utc).isoformat()


def _load_grpc_modules() -> Tuple[Any, Any]:
    proto_dir = os.path.join(os.path.dirname(__file__), "protos")
    proto_path = os.path.join(proto_dir, "runner.proto")
    out_dir = os.path.join(tempfile.gettempdir(), "croniq_runner_proto")
    os.makedirs(out_dir, exist_ok=True)

    pb2_path = os.path.join(out_dir, "runner_pb2.py")
    pb2_grpc_path = os.path.join(out_dir, "runner_pb2_grpc.py")

    if not os.path.exists(pb2_path) or not os.path.exists(pb2_grpc_path):
        protoc.main(
            [
                "grpc_tools.protoc",
                f"-I{proto_dir}",
                f"--python_out={out_dir}",
                f"--grpc_python_out={out_dir}",
                proto_path,
            ]
        )

    if out_dir not in sys.path:
        sys.path.insert(0, out_dir)

    runner_pb2 = _import_module("runner_pb2", pb2_path)
    runner_pb2_grpc = _import_module("runner_pb2_grpc", pb2_grpc_path)
    return runner_pb2, runner_pb2_grpc


def _import_module(name: str, path_value: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path_value)
    if spec is None or spec.loader is None:
        raise ImportError(f"Unable to load module {name} from {path_value}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module
