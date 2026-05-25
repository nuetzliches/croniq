"""Round-trip the Pydantic DTOs against the snake_case wire format."""

from __future__ import annotations

from croniq_runner._protocol import (
    AckRequest,
    PollRequest,
    PollResponse,
    RegisterJobRequest,
    RegisterJobResponse,
    RenewRequest,
    WorkAssignment,
    WorkEvent,
)


def test_poll_request_omits_none_instance_id() -> None:
    req = PollRequest(runner_id="r1", capabilities=["billing"], max_inflight=2)
    dumped = req.model_dump(mode="json", exclude_none=True)
    assert "instance_id" not in dumped
    assert dumped == {
        "runner_id": "r1",
        "capabilities": ["billing"],
        "max_inflight": 2,
        "inflight": [],
        "tags": [],
    }


def test_poll_request_with_instance_id() -> None:
    req = PollRequest(runner_id="r1", instance_id="inst-x", inflight=["e1"])
    dumped = req.model_dump(mode="json", exclude_none=True)
    assert dumped["instance_id"] == "inst-x"
    assert dumped["inflight"] == ["e1"]


def test_poll_response_round_trip() -> None:
    payload = {
        "work": [
            {
                "execution_id": "exec-1",
                "job_key": "billing:invoice",
                "fire_at": "2026-05-23T10:00:00Z",
                "attempt": 1,
                "metadata": {},
                "timeout": "5m",
            }
        ],
        "cancel": ["exec-2"],
    }
    parsed = PollResponse.model_validate(payload)
    assert len(parsed.work) == 1
    assert parsed.work[0].job_key == "billing:invoice"
    assert parsed.cancel == ["exec-2"]


def test_poll_response_defaults_for_empty_body() -> None:
    parsed = PollResponse.model_validate({})
    assert parsed.work == []
    assert parsed.cancel == []


def test_work_assignment_metadata_dict() -> None:
    wa = WorkAssignment(
        execution_id="e",
        job_key="j",
        fire_at="2026-05-23T10:00:00Z",
        attempt=1,
        metadata={"customer_id": "acme", "nested": {"k": 1}},
        timeout="1m",
    )
    assert wa.metadata["customer_id"] == "acme"
    assert wa.metadata["nested"]["k"] == 1


def test_ack_request_omits_optional_fields() -> None:
    ack = AckRequest(runner_id="r", execution_id="e", status="success", attempt=1)
    dumped = ack.model_dump(mode="json", exclude_none=True)
    assert "error" not in dumped
    assert "duration_ms" not in dumped


def test_ack_request_includes_error_when_set() -> None:
    ack = AckRequest(
        runner_id="r", execution_id="e", status="failure", error="boom", duration_ms=42, attempt=2
    )
    dumped = ack.model_dump(mode="json", exclude_none=True)
    assert dumped["error"] == "boom"
    assert dumped["duration_ms"] == 42


def test_renew_request_minimal() -> None:
    renew = RenewRequest(runner_id="r", execution_id="e")
    assert renew.model_dump(mode="json", exclude_none=True) == {
        "runner_id": "r",
        "execution_id": "e",
    }


def test_work_event_default_fields() -> None:
    ev = WorkEvent(message="hi")
    dumped = ev.model_dump(mode="json", exclude_none=True)
    assert dumped == {"message": "hi"}


def test_register_job_request_omits_optionals() -> None:
    req = RegisterJobRequest(job_key="billing:invoice", schedule="5m")
    dumped = req.model_dump(mode="json", exclude_none=True)
    assert dumped == {"job_key": "billing:invoice", "schedule": "5m", "capabilities": []}


def test_register_job_response_partial_body() -> None:
    parsed = RegisterJobResponse.model_validate({"job_key": "x", "status": "registered"})
    assert parsed.status == "registered"
    assert parsed.trigger_id is None
