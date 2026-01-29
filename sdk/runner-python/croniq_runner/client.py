from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Awaitable, Callable, Dict, List, Optional, Tuple, Mapping
from urllib.parse import quote, urlparse

import requests
import asyncio
import importlib.util
import os
import sys
import tempfile
import json
import uuid
try:
    import grpc
    from grpc import aio
    from grpc_tools import protoc
except Exception:  # noqa: BLE001
    grpc = None
    aio = None
    protoc = None


class CroniqError(Exception):
    def __init__(self, status_code: int, message: str) -> None:
        super().__init__(message)
        self.status_code = status_code


class LeaseConflictError(CroniqError):
    pass


class LeaseNotFoundError(CroniqError):
    pass


class RunnerMismatchError(CroniqError):
    pass


class RunnerIdInUseError(CroniqError):
    pass


class RunnerJobRegistrationDeniedError(CroniqError):
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
        has_api_key = bool(api_key)
        has_bearer = bool(bearer_token)
        if has_api_key == has_bearer:
            raise ValueError("api_key or bearer_token is required (but not both)")

        self._base_url = base_url.rstrip("/")
        self._tenant_id = tenant_id
        self._environment = environment
        self._api_key = api_key
        self._bearer_token = bearer_token
        self._timeout_seconds = timeout_seconds

    def poll(
        self,
        runner_id: str,
        runner_instance_id: Optional[str] = None,
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
        if runner_instance_id:
            payload["runnerInstanceId"] = runner_instance_id
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

    def heartbeat(
        self,
        runner_id: str,
        runner_instance_id: Optional[str],
        environment_tag: str,
        metadata_json: Optional[str] = None,
        seen_at_utc: Optional[str] = None,
    ) -> None:
        payload: Dict[str, Any] = {
            "runnerId": runner_id,
            "environmentTag": environment_tag,
        }
        if runner_instance_id:
            payload["runnerInstanceId"] = runner_instance_id
        if metadata_json:
            payload["metadataJson"] = metadata_json
        if seen_at_utc:
            payload["seenAtUtc"] = seen_at_utc
        self._post("/runners/heartbeat", payload)

    def register_job(
        self,
        runner_id: str,
        runner_instance_id: Optional[str],
        environment_tag: str,
        job_key: str,
        description: Optional[str] = None,
        metadata: Optional[Dict[str, str]] = None,
    ) -> Dict[str, Any]:
        payload: Dict[str, Any] = {
            "runnerId": runner_id,
            "environmentTag": environment_tag,
            "jobKey": job_key,
        }
        if runner_instance_id:
            payload["runnerInstanceId"] = runner_instance_id
        if description:
            payload["description"] = description
        if metadata:
            payload["metadata"] = metadata
        response = self._post("/jobs:register", payload)
        return response.json() if response.content else {}

    def _post(self, suffix: str, payload: Dict[str, Any]) -> requests.Response:
        url = self._build_url(suffix)
        headers = {"Content-Type": "application/json"}
        if self._bearer_token:
            headers["Authorization"] = f"Bearer {self._bearer_token}"
        else:
            headers["X-Croniq-Key"] = self._api_key or ""

        response = requests.post(url, json=payload, headers=headers, timeout=self._timeout_seconds)
        if response.status_code == 403 and _is_runner_mismatch_response(response):
            raise RunnerMismatchError(response.status_code, response.text)
        if response.status_code == 403 and _is_runner_registration_denied_response(response):
            raise RunnerJobRegistrationDeniedError(response.status_code, response.text)
        if response.status_code == 404:
            raise LeaseNotFoundError(response.status_code, response.text)
        if response.status_code == 409:
            if _is_runner_id_in_use_response(response):
                raise RunnerIdInUseError(response.status_code, response.text)
            raise LeaseConflictError(response.status_code, response.text)
        if response.status_code >= 400:
            raise CroniqError(response.status_code, response.text)
        return response

    def _build_url(self, suffix: str) -> str:
        url = f"{self._base_url}/tenants/{quote(self._tenant_id)}/{suffix.lstrip('/')}"
        if self._environment:
            url = f"{url}?environment={quote(self._environment)}"
        return url


def _is_runner_mismatch_response(response: requests.Response) -> bool:
    try:
        payload = response.json()
    except Exception:  # noqa: BLE001
        payload = None
    if isinstance(payload, dict):
        title = payload.get("title") or payload.get("error")
        if isinstance(title, str) and title.lower() == "runner-mismatch":
            return True
    return "runner-mismatch" in response.text.lower()


def _is_runner_id_in_use_response(response: requests.Response) -> bool:
    try:
        payload = response.json()
    except Exception:  # noqa: BLE001
        payload = None
    if isinstance(payload, dict):
        title = payload.get("title") or payload.get("error")
        if isinstance(title, str) and title.lower() == "runner-id-in-use":
            return True
    return "runner-id-in-use" in response.text.lower()


def _is_runner_registration_denied_response(response: requests.Response) -> bool:
    try:
        payload = response.json()
    except Exception:  # noqa: BLE001
        payload = None
    if isinstance(payload, dict):
        title = payload.get("title") or payload.get("error")
        if isinstance(title, str) and title.lower() == "runner-registration-denied":
            return True
    return "runner-registration-denied" in response.text.lower()


def _get_optional(environment: Mapping[str, str], key: str) -> Optional[str]:
    value = environment.get(key)
    if value is None:
        return None
    value = value.strip()
    return value or None


def _parse_int(value: Optional[str]) -> Optional[int]:
    if value is None or not value.strip():
        return None
    try:
        return int(value)
    except ValueError as exc:
        raise ValueError(f"Invalid integer value: {value}") from exc


def _parse_bool(value: Optional[str]) -> bool:
    if value is None or not value.strip():
        return False
    normalized = value.strip().lower()
    if normalized in {"true", "1", "yes"}:
        return True
    if normalized in {"false", "0", "no"}:
        return False
    raise ValueError(f"Invalid boolean value: {value}")


def _parse_optional_bool(value: Optional[str]) -> Optional[bool]:
    if value is None or not value.strip():
        return None
    normalized = value.strip().lower()
    if normalized in {"true", "1", "yes"}:
        return True
    if normalized in {"false", "0", "no"}:
        return False
    raise ValueError(f"Invalid boolean value: {value}")


def _parse_list(value: Optional[str]) -> Optional[List[str]]:
    if value is None or not value.strip():
        return None
    items = [entry.strip() for entry in value.split(",") if entry.strip()]
    return items or None


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
class RunnerJobRegistration:
    description: Optional[str] = None
    metadata: Optional[Dict[str, str]] = None


@dataclass(frozen=True)
class RunnerConfig:
    base_url: str
    tenant_id: str
    environment: str
    runner_id: str
    runner_instance_id: Optional[str] = None
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
    heartbeat_interval_ms: int = 0
    heartbeat_metadata: Optional[Dict[str, Any]] = None
    parse_payload_json: bool = False
    register_jobs: bool = True
    outbox_path: Optional[str] = None
    outbox_max_entries: int = 500
    outbox_max_bytes: int = 1_000_000

    @staticmethod
    def from_env(env: Optional[Mapping[str, str]] = None) -> "RunnerConfig":
        environment = env or os.environ

        def required(key: str) -> str:
            value = environment.get(key, "").strip()
            if not value:
                raise ValueError(f"{key} is required")
            return value

        base_url = required("CRONIQ_API_BASEURL")
        tenant_id = required("CRONIQ_TENANT_ID")
        environment_tag = required("CRONIQ_ENVIRONMENT")
        runner_id = required("CRONIQ_RUNNER_ID")
        runner_instance_id = _get_optional(environment, "CRONIQ_RUNNER_INSTANCE_ID") or uuid.uuid4().hex

        api_key = environment.get("CRONIQ_API_KEY")
        bearer_token = environment.get("CRONIQ_BEARER_TOKEN")
        has_api_key = bool(api_key and api_key.strip())
        has_bearer = bool(bearer_token and bearer_token.strip())
        if has_api_key == has_bearer:
            raise ValueError("Set exactly one of CRONIQ_API_KEY or CRONIQ_BEARER_TOKEN")

        transport_mode = (environment.get("CRONIQ_TRANSPORT_MODE") or "auto").strip().lower()
        if transport_mode not in {"auto", "grpc", "polling"}:
            raise ValueError("CRONIQ_TRANSPORT_MODE must be auto, grpc, or polling")

        register_jobs = _parse_optional_bool(environment.get("CRONIQ_RUNNER_REGISTER_JOBS"))
        if register_jobs is None:
            register_jobs = True

        return RunnerConfig(
            base_url=base_url,
            tenant_id=tenant_id,
            environment=environment_tag,
            runner_id=runner_id,
            runner_instance_id=runner_instance_id,
            api_key=api_key.strip() if has_api_key else None,
            bearer_token=bearer_token.strip() if has_bearer else None,
            grpc_base_url=_get_optional(environment, "CRONIQ_GRPC_BASEURL"),
            transport_mode=transport_mode,
            allow_test_executions=_parse_bool(environment.get("CRONIQ_ALLOW_TEST_EXECUTIONS")),
            max_inflight=_parse_int(environment.get("CRONIQ_MAX_INFLIGHT")) or 1,
            capabilities=_parse_list(environment.get("CRONIQ_CAPABILITIES")),
            poll_batch_size=_parse_int(environment.get("CRONIQ_POLL_BATCH_SIZE")) or 1,
            poll_wait_ms=_parse_int(environment.get("CRONIQ_POLL_WAIT_MS")) or 25000,
            request_timeout_seconds=(_parse_int(environment.get("CRONIQ_REQUEST_TIMEOUT_MS")) or 60000) // 1000,
            renew_lead_ms=_parse_int(environment.get("CRONIQ_RENEW_LEAD_MS")) or 10000,
            retry_base_ms=_parse_int(environment.get("CRONIQ_RETRY_BASE_MS")) or 500,
            retry_max_ms=_parse_int(environment.get("CRONIQ_RETRY_MAX_MS")) or 10000,
            retry_max_attempts=_parse_int(environment.get("CRONIQ_RETRY_MAX_ATTEMPTS")),
            register_jobs=register_jobs,
        )


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
        has_api_key = bool(config.api_key)
        has_bearer = bool(config.bearer_token)
        if has_api_key == has_bearer:
            raise ValueError("api_key or bearer_token is required (but not both)")

        self._config = config
        self._grpc_available = _grpc_available()
        if self._config.transport_mode == "grpc" and not self._grpc_available:
            raise RuntimeError(
                "gRPC dependencies are not installed. Install sdk/runner-python/requirements.txt or set "
                "CRONIQ_TRANSPORT_MODE=polling."
            )
        self._runner_instance_id = config.runner_instance_id or uuid.uuid4().hex
        self._client = RunnerClient(
            base_url=config.base_url,
            tenant_id=config.tenant_id,
            environment=config.environment,
            api_key=config.api_key,
            bearer_token=config.bearer_token,
            timeout_seconds=config.request_timeout_seconds,
        )
        self._logger = RunnerLogger()
        self._handlers: Dict[str, Tuple[Callable[[RunnerExecutionContext, Any, RunnerLogger], Awaitable[None]], Optional[RunnerJobRegistration]]] = {}
        self._inflight: Dict[str, Lease] = {}
        self._renew_tasks: Dict[str, asyncio.Task[None]] = {}
        self._execution_tasks: Dict[str, asyncio.Task[None]] = {}
        self._abandoned: set[str] = set()
        self._queue: asyncio.Queue[Lease] = asyncio.Queue()
        self._running = False
        self._accepting_work = False
        self._draining = False
        self._grpc_stream: Optional[aio.StreamStreamCall] = None
        self._grpc_lock = asyncio.Lock()
        self._grpc_connected = asyncio.Event()
        self._grpc_modules = None
        self._outbox: List[Dict[str, Any]] = []
        self._outbox_lock = asyncio.Lock()
        self._outbox_path = config.outbox_path or os.path.join(os.getcwd(), ".croniq", "runner-outbox.jsonl")
        self._fatal_future: Optional[asyncio.Future[None]] = None

    def on_execute(
        self,
        job_key: str,
        handler: Callable[[RunnerExecutionContext, Any, RunnerLogger], Awaitable[None]],
        registration: Optional[RunnerJobRegistration] = None,
    ) -> None:
        if not job_key or not job_key.strip():
            raise ValueError("job_key is required")
        if handler is None:
            raise ValueError("handler is required")
        self._handlers[job_key.strip()] = (handler, registration)

    async def start(self) -> None:
        if not self._handlers:
            raise RuntimeError("on_execute handler must be registered for at least one job_key before start")
        self._running = True
        self._accepting_work = True
        self._draining = False

        await self._load_outbox()
        if self._config.register_jobs:
            await self._register_jobs()

        self._fatal_future = asyncio.get_running_loop().create_future()

        tasks: List[asyncio.Task[None]] = []
        if self._config.transport_mode != "polling" and self._grpc_available:
            tasks.append(asyncio.create_task(self._run_grpc()))
        elif self._config.transport_mode == "auto" and not self._grpc_available:
            self._logger.warn(
                "gRPC dependencies missing; falling back to polling",
                {"install": "sdk/runner-python/requirements.txt"},
            )
        if self._config.transport_mode != "grpc":
            tasks.append(asyncio.create_task(self._run_polling()))
        if self._config.heartbeat_interval_ms > 0:
            tasks.append(asyncio.create_task(self._heartbeat_loop()))
        tasks.append(asyncio.create_task(self._run_dispatch_loop()))
        tasks.append(asyncio.create_task(self._replay_outbox_loop()))

        aggregate = asyncio.gather(*tasks)
        try:
            done, _ = await asyncio.wait(
                [aggregate, self._fatal_future],
                return_when=asyncio.FIRST_COMPLETED,
            )

            if self._fatal_future in done:
                exc = self._fatal_future.exception()
                if not aggregate.done():
                    aggregate.cancel()
                    try:
                        await aggregate
                    except asyncio.CancelledError:
                        pass
                if exc:
                    raise exc
                return

            await aggregate
        finally:
            self._fatal_future = None

    async def stop(self) -> None:
        self._accepting_work = False
        self._running = False
        self._draining = False
        if self._grpc_stream is not None:
            self._grpc_stream.cancel()
            self._grpc_stream = None
        await self._send_disconnect_heartbeat()
        for task in self._renew_tasks.values():
            task.cancel()
        self._renew_tasks.clear()
        for task in self._execution_tasks.values():
            task.cancel()
        self._execution_tasks.clear()
        self._inflight.clear()
        self._abandoned.clear()
        while not self._queue.empty():
            try:
                self._queue.get_nowait()
            except asyncio.QueueEmpty:
                break
        self._grpc_connected.clear()

    async def drain(self, timeout_ms: int = 30000) -> None:
        if not self._running:
            return

        self._accepting_work = False
        self._draining = True

        if self._grpc_stream is not None:
            self._grpc_stream.cancel()
            self._grpc_stream = None

        deadline = asyncio.get_event_loop().time() + max(0, timeout_ms) / 1000
        while (self._inflight or not self._queue.empty()) and asyncio.get_event_loop().time() < deadline:
            await asyncio.sleep(0.1)

        if self._inflight or not self._queue.empty():
            await self._abandon_pending_leases()

        await self._send_disconnect_heartbeat()
        self._running = False

    def _fail_fatal(self, exc: Exception) -> None:
        self._accepting_work = False
        self._running = False
        self._draining = False
        if self._grpc_stream is not None:
            self._grpc_stream.cancel()
            self._grpc_stream = None
        for task in self._renew_tasks.values():
            task.cancel()
        self._renew_tasks.clear()
        for task in self._execution_tasks.values():
            task.cancel()
        self._execution_tasks.clear()
        self._inflight.clear()
        self._abandoned.clear()
        while not self._queue.empty():
            try:
                self._queue.get_nowait()
            except asyncio.QueueEmpty:
                break
        self._grpc_connected.clear()
        if self._fatal_future and not self._fatal_future.done():
            self._fatal_future.set_exception(exc)

    def _handle_runner_fatal(self, exc: Exception) -> bool:
        is_mismatch = isinstance(exc, RunnerMismatchError) or _is_grpc_runner_mismatch(exc)
        is_in_use = isinstance(exc, RunnerIdInUseError) or _is_grpc_runner_id_in_use(exc)
        if not is_mismatch and not is_in_use:
            return False

        label = "runner id in use" if is_in_use else "runner mismatch"
        self._logger.error(label, {"error": str(exc)})
        self._fail_fatal(exc)
        return True

    async def _run_grpc(self) -> None:
        attempt = 0
        while self._running:
            if not self._accepting_work:
                return
            try:
                await self._connect_grpc()
                attempt = 0
            except Exception as exc:  # noqa: BLE001
                if self._handle_runner_fatal(exc):
                    return
                attempt += 1
                if self._config.retry_max_attempts and attempt >= self._config.retry_max_attempts:
                    self._logger.error("gRPC reconnect exhausted", {"error": str(exc)})
                    return
                delay = self._next_delay(attempt)
                await asyncio.sleep(delay / 1000)

    async def _connect_grpc(self) -> None:
        if not self._grpc_available:
            raise RuntimeError(
                "gRPC dependencies are not installed. Install sdk/runner-python/requirements.txt or set "
                "CRONIQ_TRANSPORT_MODE=polling."
            )
        self._grpc_modules = self._grpc_modules or _load_grpc_modules()
        runner_pb2, runner_pb2_grpc = self._grpc_modules

        endpoint_raw = self._config.grpc_base_url or self._config.base_url
        endpoint, use_tls = _normalize_grpc_endpoint(endpoint_raw)
        if use_tls:
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
                    runner_instance_id=self._runner_instance_id,
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
            if not self._accepting_work:
                return

            if self._config.transport_mode == "auto" and self._grpc_connected.is_set():
                await asyncio.sleep(0.25)
                continue

            try:
                leases = await asyncio.to_thread(
                    self._client.poll,
                    runner_id=self._config.runner_id,
                    runner_instance_id=self._runner_instance_id,
                    batch_size=self._config.poll_batch_size,
                    wait_for_ms=self._config.poll_wait_ms,
                    allow_test_executions=self._config.allow_test_executions,
                    max_inflight=self._config.max_inflight,
                    capabilities=self._config.capabilities,
                )
                for lease in leases:
                    await self._queue.put(lease)
            except Exception as exc:  # noqa: BLE001
                if self._handle_runner_fatal(exc):
                    return
                self._logger.warn("poll failed", {"error": str(exc)})
                await asyncio.sleep(self._next_delay(1) / 1000)

    async def _heartbeat_loop(self) -> None:
        while self._running:
            try:
                if not self._config.environment:
                    self._logger.warn("heartbeat skipped; environment is required", {})
                else:
                    metadata_json = json.dumps(self._build_heartbeat_metadata())
                    await asyncio.to_thread(
                        self._client.heartbeat,
                        runner_id=self._config.runner_id,
                        runner_instance_id=self._runner_instance_id,
                        environment_tag=self._config.environment,
                        metadata_json=metadata_json,
                    )
            except Exception as exc:  # noqa: BLE001
                if self._handle_runner_fatal(exc):
                    return
                self._logger.warn("heartbeat failed", {"error": str(exc)})

            await asyncio.sleep(self._config.heartbeat_interval_ms / 1000)

    async def _run_dispatch_loop(self) -> None:
        while self._running:
            if len(self._inflight) >= self._config.max_inflight:
                await asyncio.sleep(0.05)
                continue

            try:
                lease = await asyncio.wait_for(self._queue.get(), timeout=0.5)
            except asyncio.TimeoutError:
                continue
            if lease.lease_id in self._inflight:
                continue

            self._inflight[lease.lease_id] = lease
            self._renew_tasks[lease.lease_id] = asyncio.create_task(self._renew_loop(lease))
            task = asyncio.create_task(self._execute_lease(lease))
            self._execution_tasks[lease.lease_id] = task

    async def _execute_lease(self, lease: Lease) -> None:
        entry = self._handlers.get(lease.job_key)
        if entry is None:
            self._logger.warn("no handler registered for jobKey", {"jobKey": lease.job_key})
            await self._ack_failure_internal(
                lease,
                error_type="handler-not-found",
                error_message="handler not registered",
                dead_letter_reason="handler-not-found",
                allow_outbox=True,
            )
            await self._complete_lease(lease)
            return
        handler, _ = entry

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
            await handler(context, payload, self._logger)
            if self._is_abandoned(lease.lease_id):
                self._logger.warn("lease abandoned during shutdown", {"leaseId": lease.lease_id})
                return
            await self._ack_success(lease)
        except asyncio.CancelledError:
            if self._is_abandoned(lease.lease_id):
                return
            await self._ack_failure_internal(
                lease,
                error_type="runner-shutdown",
                error_message="runner shutdown",
                dead_letter_reason="runner-shutdown",
                allow_outbox=False,
            )
        except Exception as exc:  # noqa: BLE001
            if self._is_abandoned(lease.lease_id):
                self._logger.warn("lease abandoned during shutdown", {"leaseId": lease.lease_id, "error": str(exc)})
                return
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
        except Exception as exc:  # noqa: BLE001
            if self._handle_runner_fatal(exc):
                return
            if allow_outbox:
                await self._enqueue_outbox({
                    "id": str(uuid.uuid4()),
                    "type": "ack_success",
                    "payload": {"lease": lease.to_dict()},
                    "attempts": 0,
                    "created_at": asyncio.get_event_loop().time(),
                })

    async def _ack_failure(self, lease: Lease, exc: Exception, allow_outbox: bool = True) -> None:
        await self._ack_failure_internal(
            lease,
            error_type="execution-failed",
            error_message=str(exc),
            dead_letter_reason="execution-failed",
            allow_outbox=allow_outbox,
        )

    async def _ack_failure_internal(
        self,
        lease: Lease,
        error_type: str,
        error_message: str,
        dead_letter_reason: str,
        allow_outbox: bool = True,
    ) -> None:
        if self._grpc_connected.is_set() and self._grpc_stream:
            await self._grpc_send(
                self._grpc_modules[0].RunnerMessage(
                    ack_failure=self._grpc_modules[0].WorkAckFailure(
                        execution_id=lease.execution_id,
                        lease_id=lease.lease_id,
                        error_type=error_type,
                        error_message=error_message,
                        dead_letter_reason=dead_letter_reason,
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
                dead_letter_reason=dead_letter_reason,
            )
        except Exception as exc:  # noqa: BLE001
            if self._handle_runner_fatal(exc):
                return
            if allow_outbox:
                await self._enqueue_outbox({
                    "id": str(uuid.uuid4()),
                    "type": "ack_failure",
                    "payload": {
                        "lease": lease.to_dict(),
                        "error_type": error_type,
                        "error_message": error_message,
                        "dead_letter_reason": dead_letter_reason,
                    },
                    "attempts": 0,
                    "created_at": asyncio.get_event_loop().time(),
                })

    async def _reject_test(self, lease: Lease, allow_outbox: bool = True) -> None:
        await self._ack_failure_internal(
            lease,
            error_type="test-not-allowed",
            error_message="test executions are disabled for this runner",
            dead_letter_reason="test-not-allowed",
            allow_outbox=allow_outbox,
        )

    async def _renew_loop(self, lease: Lease) -> None:
        while self._running and lease.lease_id in self._inflight:
            if self._is_abandoned(lease.lease_id):
                return
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
                if self._handle_runner_fatal(exc):
                    return
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
        except Exception as exc:  # noqa: BLE001
            if self._handle_runner_fatal(exc):
                return
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
        self._execution_tasks.pop(lease.lease_id, None)
        self._abandoned.discard(lease.lease_id)

    def _is_abandoned(self, lease_id: str) -> bool:
        return lease_id in self._abandoned

    async def _abandon_pending_leases(self) -> None:
        leases = list(self._inflight.values())
        for lease in leases:
            self._abandoned.add(lease.lease_id)

        for task in self._renew_tasks.values():
            task.cancel()
        self._renew_tasks.clear()

        for task in self._execution_tasks.values():
            task.cancel()
        self._execution_tasks.clear()

        while not self._queue.empty():
            try:
                lease = self._queue.get_nowait()
                self._abandoned.add(lease.lease_id)
                leases.append(lease)
            except asyncio.QueueEmpty:
                break

        for lease in leases:
            try:
                await self._ack_failure_internal(
                    lease,
                    error_type="runner-shutdown",
                    error_message="runner shutdown",
                    dead_letter_reason="runner-shutdown",
                    allow_outbox=False,
                )
            except Exception as exc:  # noqa: BLE001
                if self._handle_runner_fatal(exc):
                    return

    def _build_heartbeat_metadata(self) -> Dict[str, Any]:
        transport_state = "grpc" if self._grpc_connected.is_set() else "polling"
        metadata: Dict[str, Any] = {
            "runnerInstanceId": self._runner_instance_id,
            "transportMode": self._config.transport_mode,
            "transportState": transport_state,
            "allowTestExecutions": self._config.allow_test_executions,
            "maxInflight": self._config.max_inflight,
            "capabilities": self._config.capabilities or [],
        }
        if self._config.heartbeat_metadata:
            metadata.update(self._config.heartbeat_metadata)
        return metadata

    def _build_disconnect_metadata(self) -> Dict[str, Any]:
        metadata = self._build_heartbeat_metadata()
        metadata["transportState"] = "disconnected"
        metadata["disconnectedAtUtc"] = datetime.now(timezone.utc).isoformat()
        return metadata

    async def _send_disconnect_heartbeat(self) -> None:
        if not self._config.environment:
            return

        seen_at = datetime.now(timezone.utc)
        metadata_json = json.dumps(self._build_disconnect_metadata())
        try:
            await asyncio.to_thread(
                self._client.heartbeat,
                runner_id=self._config.runner_id,
                runner_instance_id=self._runner_instance_id,
                environment_tag=self._config.environment,
                metadata_json=metadata_json,
                seen_at_utc=seen_at.isoformat(),
            )
        except Exception as exc:  # noqa: BLE001
            self._logger.warn("disconnect heartbeat failed", {"error": str(exc)})

    async def _register_jobs(self) -> None:
        if not self._config.environment:
            raise RuntimeError("environment is required for job registration")

        for job_key, entry in self._handlers.items():
            _, registration = entry
            response = await asyncio.to_thread(
                self._client.register_job,
                runner_id=self._config.runner_id,
                runner_instance_id=self._runner_instance_id,
                environment_tag=self._config.environment,
                job_key=job_key,
                description=registration.description if registration else None,
                metadata=registration.metadata if registration else None,
            )

            if response and response.get("isActive") is False:
                self._logger.warn("job registration pending approval", {"jobKey": job_key})
            else:
                self._logger.info("job registration completed", {"jobKey": job_key})

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
                        await self._ack_failure_internal(
                            lease,
                            error_type=payload.get("error_type", "execution-failed"),
                            error_message=payload.get("error_message", "ack failed"),
                            dead_letter_reason=payload.get("dead_letter_reason", "execution-failed"),
                            allow_outbox=False,
                        )
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
                except Exception as exc:
                    if self._handle_runner_fatal(exc):
                        return
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


def _is_grpc_runner_mismatch(exc: Exception) -> bool:
    if grpc is None:
        return False
    if isinstance(exc, grpc.aio.AioRpcError):
        return exc.code() == grpc.StatusCode.PERMISSION_DENIED and "runner-mismatch" in (exc.details() or "").lower()
    return False


def _is_grpc_runner_id_in_use(exc: Exception) -> bool:
    if grpc is None:
        return False
    if isinstance(exc, grpc.aio.AioRpcError):
        return exc.code() == grpc.StatusCode.ALREADY_EXISTS and "runner-id-in-use" in (exc.details() or "").lower()
    return False


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
    if not _grpc_available():
        raise RuntimeError("gRPC dependencies are not installed. Install sdk/runner-python/requirements.txt.")
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


def _normalize_grpc_endpoint(raw: str) -> tuple[str, bool]:
    if not raw:
        return raw, False

    if raw.startswith("http://") or raw.startswith("https://"):
        try:
            parsed = urlparse(raw)
            host = parsed.hostname or raw
            if parsed.port:
                host = f"{host}:{parsed.port}"
            return host, parsed.scheme == "https"
        except Exception:  # noqa: BLE001
            return raw, raw.startswith("https://")

    return raw, False


def _grpc_available() -> bool:
    return grpc is not None and aio is not None and protoc is not None


def _import_module(name: str, path_value: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path_value)
    if spec is None or spec.loader is None:
        raise ImportError(f"Unable to load module {name} from {path_value}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module
