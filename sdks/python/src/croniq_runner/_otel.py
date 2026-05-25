"""Optional OpenTelemetry tracing.

Importing ``opentelemetry.trace`` is wrapped in a try/except so the SDK has
zero hard dependency on OTel — installing ``croniq-runner[otel]`` is enough
to activate tracing.
"""

from __future__ import annotations

import contextlib
from collections.abc import Iterator
from typing import Any

try:
    from opentelemetry import trace as _otel_trace

    _tracer = _otel_trace.get_tracer("croniq-runner")
    _HAS_OTEL = True
except ImportError:  # pragma: no cover — exercised in the "no extras" install
    _tracer = None  # type: ignore[assignment]
    _HAS_OTEL = False


@contextlib.contextmanager
def maybe_start_span(name: str, /, **attributes: Any) -> Iterator[None]:
    """Start an OTel span if ``opentelemetry-api`` is installed; otherwise a no-op."""
    if not _HAS_OTEL or _tracer is None:
        yield
        return
    attrs = {f"croniq.{k}": _to_attr_value(v) for k, v in attributes.items() if v is not None}
    with _tracer.start_as_current_span(name, attributes=attrs):
        yield


def _to_attr_value(v: Any) -> Any:
    # OTel attribute values must be primitives or sequences thereof.
    if isinstance(v, str | int | float | bool):
        return v
    if isinstance(v, list | tuple):
        return [str(x) for x in v]
    return str(v)
