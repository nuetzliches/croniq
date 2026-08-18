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
    # Top-level request-body keys that MUST NOT appear. Only the trigger
    # (producer) cases use this — runner cases leave it empty.
    body_absent: list[str] = field(default_factory=list)


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


# --- strict key sets ------------------------------------------------------
#
# One frozenset per node the loader parses, listing exactly the keys this
# binding implements. Anything else is a load-time error (see
# _reject_unknown_keys).
#
# Why not validate against schema/case-schema.json here: CI already does
# that for the whole corpus (the `Conformance YAML schema` job runs
# check-jsonschema against both schemas), and it answers a different
# question. Schema validation catches a key the *schema* does not allow.
# These sets catch a schema-legal key the *binding* has not implemented —
# the case #460 was filed for, where a new assertion key loads cleanly in
# every binding and is silently not asserted by the ones that never
# implemented it. Repeating the schema check here would add a dependency
# and still leave that hole open.
#
# The sets are therefore expected to *lag* the schema when a capability is
# .NET-only: runner_config's max_consecutive_poll_conflicts is in the schema
# but not here, because the Python SDK has no such option. A case using it
# must fail loudly rather than run with the option ignored.

_CASE_KEYS = frozenset(
    {
        "name",
        "description",
        "runner_config",
        "handlers",
        "server_script",
        "shutdown_after_ms",
        "expectations",
    }
)
_RUNNER_CONFIG_KEYS = frozenset(
    {
        "runner_id",
        "runner_id_prefix",
        "capabilities",
        "tags",
        "max_inflight",
        "api_key",
        "bearer_token",
        "poll_timeout_ms",
        "renew_interval_ms",
        "drain_timeout_ms",
        "poll_retry_delay_ms",
        "capacity_backoff_ms",
    }
)
_HANDLER_KEYS = frozenset(
    {
        "job_key",
        "is_default",
        "schedule",
        "behavior",
        "error_message",
        "duration_ms",
        "level",
        "message",
        "count",
        "interval_ms",
    }
)
_SCRIPT_ENTRY_KEYS = frozenset({"on", "match_count", "respond"})
_RESPOND_KEYS = frozenset({"status", "body", "delay_ms", "headers"})
_EXPECTATIONS_KEYS = frozenset({"duration_max_ms", "http"})
_HTTP_EXPECTATION_KEYS = frozenset(
    {
        "method",
        "path",
        "exact_count",
        "min_count",
        "max_count",
        "headers",
        "body_match",
    }
)
# Trigger cases additionally pin the omission of unset optionals. Runner cases
# must not use body_absent — case-schema.json does not declare it.
_TRIGGER_HTTP_EXPECTATION_KEYS = _HTTP_EXPECTATION_KEYS | {"body_absent"}


def _reject_unknown_keys(d: dict[str, Any], allowed: frozenset[str], ctx: str) -> None:
    """Fail loudly on a key this binding does not implement.

    A dropped key is invisible: the case loads, the assertion it carried is
    never evaluated, and the suite stays green precisely when the contract
    stopped being enforced.
    """
    unknown = sorted(set(d) - allowed)
    if unknown:
        raise ValueError(
            f"{ctx}: unrecognised key(s) {unknown}. This binding does not implement them — "
            f"either the case is wrong or the Python conformance harness needs updating. "
            f"Known keys: {sorted(allowed)}"
        )


def load_case(path: Path) -> CaseSpec:
    """Parse one conformance YAML into a :class:`CaseSpec`."""
    raw = yaml.load(path.read_text(encoding="utf-8"), Loader=_Yaml12Loader)  # noqa: S506 — _Yaml12Loader is a SafeLoader subclass
    if not isinstance(raw, dict):
        raise ValueError(f"{path}: top-level must be a mapping")
    return _to_case(raw)


def _to_case(d: dict[str, Any]) -> CaseSpec:
    _reject_unknown_keys(d, _CASE_KEYS, "case")
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
    _reject_unknown_keys(d, _RUNNER_CONFIG_KEYS, "runner_config")
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
    _reject_unknown_keys(d, _HANDLER_KEYS, f"handler {d.get('job_key', '?')!r}")
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
    _reject_unknown_keys(d, _SCRIPT_ENTRY_KEYS, f"server_script entry {d.get('on', '?')!r}")
    respond_raw = d.get("respond") or {}
    _reject_unknown_keys(respond_raw, _RESPOND_KEYS, f"respond of {d.get('on', '?')!r}")
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


def _to_expectations(
    d: dict[str, Any], http_keys: frozenset[str] = _HTTP_EXPECTATION_KEYS
) -> ExpectationsSpec:
    """Parse an ``expectations`` block.

    ``http_keys`` selects the allowed key set for the nested HTTP
    expectations: runner cases use the default, trigger cases pass
    :data:`_TRIGGER_HTTP_EXPECTATION_KEYS` to additionally allow
    ``body_absent``, which only trigger-case-schema.json declares.
    """
    _reject_unknown_keys(d, _EXPECTATIONS_KEYS, "expectations")
    return ExpectationsSpec(
        duration_max_ms=d.get("duration_max_ms"),
        http=[_to_http_expectation(e, http_keys) for e in (d.get("http") or [])],
    )


def _to_http_expectation(
    d: dict[str, Any], allowed: frozenset[str] = _HTTP_EXPECTATION_KEYS
) -> HttpExpectation:
    _reject_unknown_keys(
        d, allowed, f"http expectation {d.get('method', '?')} {d.get('path', '?')}"
    )
    return HttpExpectation(
        method=str(d.get("method", "GET")).upper(),
        path=str(d.get("path", "/")),
        exact_count=d.get("exact_count"),
        min_count=d.get("min_count"),
        max_count=d.get("max_count"),
        headers={k.lower(): v for k, v in (d.get("headers") or {}).items()},
        body_match=d.get("body_match"),
        body_absent=list(d.get("body_absent") or []),
    )
