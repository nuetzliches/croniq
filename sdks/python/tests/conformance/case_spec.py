"""In-memory representation of a single conformance case YAML file.

Mirrors the .NET binding's ``CaseSpec`` so the YAMLs at
``sdks/conformance/cases/*.yaml`` can be read with snake_case keys and no
key-renaming logic.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml


class _Yaml12Loader(yaml.SafeLoader):
    """SafeLoader patched to treat ``on``/``off``/``yes``/``no`` as strings.

    PyYAML defaults to YAML 1.1 which coerces those tokens to booleans —
    that breaks our case files where ``on:`` is the conventional key for
    ``method + path``. The conformance suite (and ``openapi.yaml``) follow
    YAML 1.2 semantics, so we strip the legacy bool resolver.
    """


# Override the implicit-bool resolver to match YAML 1.2 (only ``true``/``false``,
# case-insensitive). This must run on _Yaml12Loader, not SafeLoader, so the
# upstream parser stays unchanged.
_Yaml12Loader.yaml_implicit_resolvers = {
    k: [(tag, regexp) for tag, regexp in v if tag != "tag:yaml.org,2002:bool"]
    for k, v in yaml.SafeLoader.yaml_implicit_resolvers.items()
}
_Yaml12Loader.add_implicit_resolver(
    "tag:yaml.org,2002:bool",
    __import__("re").compile(r"^(?:true|True|TRUE|false|False|FALSE)$"),
    list("tTfF"),
)


@dataclass(slots=True)
class RespondSpec:
    status: int = 200
    body: Any = None
    delay_ms: int | None = None
    headers: dict[str, str] = field(default_factory=dict)


@dataclass(slots=True)
class ScriptEntrySpec:
    on: str = ""
    match_count: int | None = None
    respond: RespondSpec = field(default_factory=RespondSpec)

    @property
    def method(self) -> str:
        return self.on.split(" ", 1)[0]

    @property
    def path(self) -> str:
        return self.on.split(" ", 1)[1]


@dataclass(slots=True)
class HandlerSpec:
    job_key: str = ""
    is_default: bool = False
    schedule: str | None = None
    behavior: str = "noop"
    error_message: str | None = None
    duration_ms: int | None = None
    level: str | None = None
    message: str | None = None
    count: int | None = None
    interval_ms: int | None = None


@dataclass(slots=True)
class HttpExpectation:
    method: str = "GET"
    path: str = "/"
    exact_count: int | None = None
    min_count: int | None = None
    max_count: int | None = None
    headers: dict[str, str] = field(default_factory=dict)
    body_match: Any = None


@dataclass(slots=True)
class ExpectationsSpec:
    duration_max_ms: int | None = None
    http: list[HttpExpectation] = field(default_factory=list)


@dataclass(slots=True)
class RunnerConfigSpec:
    runner_id: str | None = None
    runner_id_prefix: str | None = None
    capabilities: list[str] = field(default_factory=list)
    tags: list[str] = field(default_factory=list)
    max_inflight: int | None = None
    api_key: str | None = None
    bearer_token: str | None = None
    poll_timeout_ms: int | None = None
    renew_interval_ms: int | None = None
    drain_timeout_ms: int | None = None
    poll_retry_delay_ms: int | None = None
    capacity_backoff_ms: int | None = None


@dataclass(slots=True)
class CaseSpec:
    name: str = ""
    description: str | None = None
    runner_config: RunnerConfigSpec = field(default_factory=RunnerConfigSpec)
    handlers: list[HandlerSpec] = field(default_factory=list)
    server_script: list[ScriptEntrySpec] = field(default_factory=list)
    expectations: ExpectationsSpec = field(default_factory=ExpectationsSpec)
    shutdown_after_ms: int | None = None


def load_case(path: Path) -> CaseSpec:
    """Parse one conformance YAML into a :class:`CaseSpec`."""
    raw = yaml.load(path.read_text(encoding="utf-8"), Loader=_Yaml12Loader)  # noqa: S506 — _Yaml12Loader is a SafeLoader subclass
    if not isinstance(raw, dict):
        raise ValueError(f"{path}: top-level must be a mapping")
    return _to_case(raw)


def _to_case(d: dict[str, Any]) -> CaseSpec:
    return CaseSpec(
        name=str(d.get("name", "")),
        description=d.get("description"),
        runner_config=_to_runner_config(d.get("runner_config") or {}),
        handlers=[_to_handler(h) for h in (d.get("handlers") or [])],
        server_script=[_to_script_entry(e) for e in (d.get("server_script") or [])],
        expectations=_to_expectations(d.get("expectations") or {}),
        shutdown_after_ms=d.get("shutdown_after_ms"),
    )


def _to_runner_config(d: dict[str, Any]) -> RunnerConfigSpec:
    return RunnerConfigSpec(
        runner_id=d.get("runner_id"),
        runner_id_prefix=d.get("runner_id_prefix"),
        capabilities=list(d.get("capabilities") or []),
        tags=list(d.get("tags") or []),
        max_inflight=d.get("max_inflight"),
        api_key=d.get("api_key"),
        bearer_token=d.get("bearer_token"),
        poll_timeout_ms=d.get("poll_timeout_ms"),
        renew_interval_ms=d.get("renew_interval_ms"),
        drain_timeout_ms=d.get("drain_timeout_ms"),
        poll_retry_delay_ms=d.get("poll_retry_delay_ms"),
        capacity_backoff_ms=d.get("capacity_backoff_ms"),
    )


def _to_handler(d: dict[str, Any]) -> HandlerSpec:
    return HandlerSpec(
        job_key=str(d.get("job_key", "")),
        is_default=bool(d.get("is_default", False)),
        schedule=d.get("schedule"),
        behavior=str(d.get("behavior", "noop")),
        error_message=d.get("error_message"),
        duration_ms=d.get("duration_ms"),
        level=d.get("level"),
        message=d.get("message"),
        count=d.get("count"),
        interval_ms=d.get("interval_ms"),
    )


def _to_script_entry(d: dict[str, Any]) -> ScriptEntrySpec:
    respond_raw = d.get("respond") or {}
    return ScriptEntrySpec(
        on=str(d.get("on", "")),
        match_count=d.get("match_count"),
        respond=RespondSpec(
            status=int(respond_raw.get("status", 200)),
            body=respond_raw.get("body"),
            delay_ms=respond_raw.get("delay_ms"),
            headers=dict(respond_raw.get("headers") or {}),
        ),
    )


def _to_expectations(d: dict[str, Any]) -> ExpectationsSpec:
    return ExpectationsSpec(
        duration_max_ms=d.get("duration_max_ms"),
        http=[_to_http_expectation(e) for e in (d.get("http") or [])],
    )


def _to_http_expectation(d: dict[str, Any]) -> HttpExpectation:
    return HttpExpectation(
        method=str(d.get("method", "GET")).upper(),
        path=str(d.get("path", "/")),
        exact_count=d.get("exact_count"),
        min_count=d.get("min_count"),
        max_count=d.get("max_count"),
        headers={k.lower(): v for k, v in (d.get("headers") or {}).items()},
        body_match=d.get("body_match"),
    )
