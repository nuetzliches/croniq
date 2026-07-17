"""Pydantic DTOs for the Croniq runner wire protocol.

Field names mirror `openapi.yaml` exactly: snake_case on the wire, snake_case
in Python. Optional fields use ``= None`` defaults; ``model_dump(by_alias=True,
exclude_none=True)`` produces the JSON shape the server accepts.
"""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel, ConfigDict, Field


class _Model(BaseModel):
    model_config = ConfigDict(
        extra="ignore",
        populate_by_name=True,
        str_strip_whitespace=False,
    )


class PollRequest(_Model):
    """POST /v1/work/poll request body."""

    runner_id: str
    capabilities: list[str] = Field(default_factory=list)
    max_inflight: int = 1
    inflight: list[str] = Field(default_factory=list)
    instance_id: str | None = None
    tags: list[str] = Field(default_factory=list)


class WorkAssignment(_Model):
    """One unit of work returned by /v1/work/poll."""

    execution_id: str
    job_key: str
    fire_at: str
    # Original logical fire time (RFC 3339). None when the server predates the
    # field — the SDK must not fall back to fire_at.
    scheduled_for: str | None = None
    attempt: int
    metadata: dict[str, Any] = Field(default_factory=dict)
    timeout: str


class PollResponse(_Model):
    """POST /v1/work/poll response body."""

    work: list[WorkAssignment] = Field(default_factory=list)
    cancel: list[str] = Field(default_factory=list)


class AckRequest(_Model):
    """POST /v1/work/ack request body."""

    runner_id: str
    execution_id: str
    status: str  # "success" | "failure"
    error: str | None = None
    duration_ms: int | None = None
    attempt: int


class RenewRequest(_Model):
    """POST /v1/work/renew request body."""

    runner_id: str
    execution_id: str


class WorkEvent(_Model):
    """A structured log event pushed to /v1/work/{id}/events.

    The runner auto-enriches ``fields`` with ``job_key``, ``runner_id`` and
    (when set) ``runner_tags`` before the POST — caller-supplied keys win.
    """

    level: str | None = None
    message: str
    fields: dict[str, str] | None = None


class RegisterJobRequest(_Model):
    """POST /v1/jobs/register request body."""

    job_key: str
    schedule: str
    timezone: str | None = None
    timeout: str | None = None
    runner_id: str | None = None
    capabilities: list[str] = Field(default_factory=list)
    description: str | None = None


class RegisterJobResponse(_Model):
    """POST /v1/jobs/register response body."""

    job_key: str | None = None
    trigger_id: str | None = None
    status: str | None = None  # "registered" | "skipped_dsl_precedence"


class TriggerRequest(_Model):
    """POST /v1/trigger request body (producer side).

    Every field except ``job_key`` is optional; dumping with
    ``exclude_none=True`` omits the ones the caller left unset (the server
    treats an omitted optional and a ``null`` differently — see #283).
    ``metadata`` is arbitrary caller JSON forwarded to the handler verbatim,
    so its values are ``Any``, not strings.
    """

    job_key: str
    metadata: dict[str, Any] | None = None
    require: list[str] | None = None
    prefer: list[str] | None = None
    timeout: str | None = None
    idempotency_key: str | None = None


class TriggerResponse(_Model):
    """POST /v1/trigger response body.

    ``deduplicated`` is sent by servers with trigger-idempotency support
    (#279); older servers omit it and the field defaults to ``False``.
    """

    execution_id: str
    queued: int
    deduplicated: bool = False
