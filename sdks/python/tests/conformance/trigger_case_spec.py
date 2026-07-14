"""In-memory representation of a trigger (producer) conformance case.

Parses the ``trigger_config`` + ``trigger_calls`` shape defined by
``sdks/conformance/schema/trigger-case-schema.json`` (#287). Distinct from
:mod:`tests.conformance.case_spec` (the runner/consumer loop): a producer case
makes explicit ``trigger(...)`` calls instead of running a poll loop. The
scripted-server and HTTP-expectation halves are shared with the runner cases.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

from tests.conformance.case_spec import (
    ExpectationsSpec,
    ScriptEntrySpec,
    _to_expectations,
    _to_script_entry,
    _Yaml12Loader,
)


@dataclass(slots=True)
class TriggerConfigSpec:
    api_key: str | None = None
    bearer_token: str | None = None


@dataclass(slots=True)
class TriggerExpectSpec:
    # Subset match on the returned TriggerResult; execution_id "*" = any non-empty.
    response: dict[str, Any] | None = None
    # True → the call MUST surface as an error (raised exception), not a value.
    error: bool = False


@dataclass(slots=True)
class TriggerCallSpec:
    request: dict[str, Any] = field(default_factory=dict)
    expect: TriggerExpectSpec = field(default_factory=TriggerExpectSpec)


@dataclass(slots=True)
class TriggerCaseSpec:
    name: str = ""
    description: str | None = None
    trigger_config: TriggerConfigSpec = field(default_factory=TriggerConfigSpec)
    trigger_calls: list[TriggerCallSpec] = field(default_factory=list)
    server_script: list[ScriptEntrySpec] = field(default_factory=list)
    expectations: ExpectationsSpec = field(default_factory=ExpectationsSpec)


def load_trigger_case(path: Path) -> TriggerCaseSpec:
    """Parse one trigger conformance YAML into a :class:`TriggerCaseSpec`."""
    raw = yaml.load(path.read_text(encoding="utf-8"), Loader=_Yaml12Loader)  # noqa: S506 — _Yaml12Loader is a SafeLoader subclass
    if not isinstance(raw, dict):
        raise ValueError(f"{path}: top-level must be a mapping")
    return _to_case(raw)


def _to_case(d: dict[str, Any]) -> TriggerCaseSpec:
    return TriggerCaseSpec(
        name=str(d.get("name", "")),
        description=d.get("description"),
        trigger_config=_to_trigger_config(d.get("trigger_config") or {}),
        trigger_calls=[_to_trigger_call(c) for c in (d.get("trigger_calls") or [])],
        server_script=[_to_script_entry(e) for e in (d.get("server_script") or [])],
        expectations=_to_expectations(d.get("expectations") or {}),
    )


def _to_trigger_config(d: dict[str, Any]) -> TriggerConfigSpec:
    return TriggerConfigSpec(
        api_key=d.get("api_key"),
        bearer_token=d.get("bearer_token"),
    )


def _to_trigger_call(d: dict[str, Any]) -> TriggerCallSpec:
    expect_raw = d.get("expect") or {}
    return TriggerCallSpec(
        request=dict(d.get("request") or {}),
        expect=TriggerExpectSpec(
            response=expect_raw.get("response"),
            error=bool(expect_raw.get("error", False)),
        ),
    )
