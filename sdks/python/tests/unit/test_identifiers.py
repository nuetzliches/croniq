"""Ingest validation and log hygiene for server-supplied identifiers (#441)."""

from __future__ import annotations

import asyncio
import json
import logging

import httpx
import pytest

from croniq_runner import Runner, RunnerOptions
from croniq_runner._client import CroniqClient
from croniq_runner._identifiers import (
    MAX_EXECUTION_ID_LENGTH,
    MAX_JOB_KEY_LENGTH,
    escape_control_chars,
    is_safe_execution_id,
    is_safe_job_key,
    preview_for_log,
    reject_assignment_reason,
    rejection_ack_error,
)

ESC = "\x1b"
CRLF_KEY = "billing:invoice\r\n2026-01-01 ERROR forged record"
ANSI_KEY = f"billing:{ESC}[31minvoice{ESC}[0m"


@pytest.mark.parametrize(
    "key",
    [
        "billing:invoice",
        "ops:health:eu-west",
        "ops:db-dump",
        "a:b",
        "ns:name.with.dots",
        "ns:name_with_underscore",
        "ns:path/segment",
        "ns:*",
        "ns:name+variant@host",
        "ns:what?",
    ],
)
def test_accepts_every_key_the_lexer_can_produce_unquoted(key: str) -> None:
    assert is_safe_job_key(key)


@pytest.mark.parametrize(
    "key",
    [
        # `job "billing:monthly invoice" { … }` is legal DSL: parse_job_key
        # accepts a QuotedString and enforces only the colon-part count. An
        # allowlist would strand these valid configurations, so interior spaces
        # and non-ASCII text must pass.
        "billing:monthly invoice",
        "ops:health check:eu-west",
        "berichte:monatsabschluss (märz)",
        "ops:1С-выгрузка",
        "ops:日次バッチ",
        "ops:deploy#42",
        "ops:a,b;c",
        "ops:100%-check",
        "ops:emoji-🚀",
        # A trailing or interior space cannot forge a record, so it is accepted.
        "billing:invoice ",
        "billing: invoice",
    ],
)
def test_accepts_keys_only_a_quoted_key_or_the_http_api_can_produce(key: str) -> None:
    assert is_safe_job_key(key)


@pytest.mark.parametrize(
    "key",
    [
        CRLF_KEY,
        ANSI_KEY,
        "billing:in\x00voice",
        "billing:in\tvoice",
        "billing:invoice\x7f",
        "billing:invoice\x9b",
        "",
        "a" * (MAX_JOB_KEY_LENGTH + 1),
    ],
)
def test_rejects_control_characters_and_out_of_bound_job_keys(key: str) -> None:
    assert not is_safe_job_key(key)


def test_job_key_length_bound_counts_scalar_values() -> None:
    # Python str is already a sequence of scalar values, so an astral character
    # counts once — pinned so a future byte-oriented rewrite cannot regress it.
    assert is_safe_job_key("🚀" * MAX_JOB_KEY_LENGTH)
    assert not is_safe_job_key("🚀" * (MAX_JOB_KEY_LENGTH + 1))


def test_job_key_length_bound_is_inclusive() -> None:
    assert is_safe_job_key("a" * MAX_JOB_KEY_LENGTH)


def test_non_strings_are_rejected() -> None:
    assert not is_safe_job_key(None)
    assert not is_safe_job_key(42)
    assert not is_safe_execution_id(None)


def test_execution_id_accepts_uuid_and_opaque_ids() -> None:
    assert is_safe_execution_id("6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77")
    assert is_safe_execution_id("exec-001")


@pytest.mark.parametrize(
    "value",
    ["exec-001\r\nforged", f"exec{ESC}[2J001", "", "a" * (MAX_EXECUTION_ID_LENGTH + 1)],
)
def test_execution_id_rejects_hostile_and_out_of_bound(value: str) -> None:
    assert not is_safe_execution_id(value)


def test_reject_assignment_reason_names_the_field() -> None:
    assert reject_assignment_reason("exec-001", "billing:invoice") is None
    assert reject_assignment_reason("exec-001", "billing:monthly invoice") is None
    assert reject_assignment_reason("exec-001", CRLF_KEY) == "job_key"
    assert reject_assignment_reason("exec\r\n001", "billing:invoice") == "execution_id"
    # execution_id is checked first: it is what addresses the server, so when
    # both are bad the assignment is unackable and must be dropped.
    assert reject_assignment_reason("exec\r\n001", CRLF_KEY) == "execution_id"


def test_rejection_ack_error_names_the_field_and_escapes_the_value() -> None:
    message = rejection_ack_error("job_key", CRLF_KEY)
    assert "job_key" in message
    assert "\r" not in message
    assert "\n" not in message
    assert "\\u000d\\u000a" in message


def test_escape_control_chars() -> None:
    assert escape_control_chars("a\r\nb") == "a\\u000d\\u000ab"
    assert escape_control_chars(f"{ESC}[31mred") == "\\u001b[31mred"
    assert escape_control_chars("\x9b") == "\\u009b"
    assert escape_control_chars("billing:invoice — läuft") == "billing:invoice — läuft"


def test_preview_for_log_escapes_and_truncates() -> None:
    assert "\n" not in preview_for_log(CRLF_KEY)
    assert ESC not in preview_for_log(ANSI_KEY)
    assert len(preview_for_log("a" * 500)) <= 121


async def _run_one_assignment(
    work: dict,
) -> tuple[list[tuple[str, dict]], list[logging.LogRecord]]:
    """Drive the runner through a single poll that returns ``work``.

    Returns the recorded HTTP calls and the records the SDK logger emitted.
    """
    calls: list[tuple[str, dict]] = []
    poll_count = 0

    def transport_handler(req: httpx.Request) -> httpx.Response:
        nonlocal poll_count
        body = json.loads(req.content) if req.content else {}
        calls.append((req.url.path, body))
        if req.url.path == "/v1/work/poll":
            poll_count += 1
            if poll_count == 1:
                return httpx.Response(200, json={"work": [work], "cancel": []})
            return httpx.Response(200, json={"work": [], "cancel": []})
        if req.url.path == "/v1/work/ack":
            return httpx.Response(200, json={})
        return httpx.Response(404)

    options = RunnerOptions(
        server_url="https://test.invalid",
        api_key="testkey",
        poll_timeout_ms=500,
        poll_retry_delay_ms=100,
        drain_timeout_ms=1000,
        runner_id="r-test",
    )
    transport = httpx.MockTransport(transport_handler)
    http = httpx.AsyncClient(base_url=options.server_url, transport=transport)
    runner = Runner(options, client=CroniqClient(options, http=http))

    async def failing_handler(ctx) -> None:  # noqa: ANN001 — ExecutionContext, late-bound
        raise RuntimeError("billing service unreachable")

    runner.set_default_handler(failing_handler)
    runner.add_handler("billing:invoice", failing_handler)

    records: list[logging.LogRecord] = []

    class _Capture(logging.Handler):
        def emit(self, record: logging.LogRecord) -> None:
            records.append(record)

    capture = _Capture()
    sdk_log = logging.getLogger("croniq_runner.runner")
    sdk_log.addHandler(capture)
    previous_level = sdk_log.level
    sdk_log.setLevel(logging.DEBUG)

    async def stopper() -> None:
        for _ in range(20):
            await asyncio.sleep(0.05)
            if any(p == "/v1/work/ack" for p, _ in calls):
                break
        runner.request_drain()

    try:
        await asyncio.gather(runner.run(), stopper())
    finally:
        sdk_log.removeHandler(capture)
        sdk_log.setLevel(previous_level)

    return calls, records


async def test_hostile_job_key_is_never_dispatched_but_is_acked_as_failure() -> None:
    """A valid execution_id means the runner can still report the problem.

    Dropping silently would leave the execution to the stale-claim reaper, which
    requeues it — and the next poll would refuse it again, forever.
    """
    calls, records = await _run_one_assignment(
        {
            "execution_id": "exec-hostile",
            "job_key": CRLF_KEY + ANSI_KEY,
            "fire_at": "2026-05-23T10:00:00Z",
            "attempt": 1,
            "metadata": {},
            "timeout": "5m",
        }
    )

    acks = [b for p, b in calls if p == "/v1/work/ack"]
    assert len(acks) == 1
    assert acks[0]["execution_id"] == "exec-hostile"
    assert acks[0]["status"] == "failure"
    # The error names the field and carries the value escaped, so the
    # dead-letter row explains itself without carrying a live payload.
    assert "job_key" in acks[0]["error"]
    assert "\r" not in acks[0]["error"]
    assert "\n" not in acks[0]["error"]
    assert ESC not in acks[0]["error"]
    # The handler never ran: no renew was issued for it.
    assert [p for p, _ in calls if p == "/v1/work/renew"] == []

    rejections = [r for r in records if "unsafe identifier" in r.getMessage()]
    assert len(rejections) == 1
    assert rejections[0].field == "job_key"  # type: ignore[attr-defined]
    assert rejections[0].acked is True  # type: ignore[attr-defined]
    assert "\r" not in rejections[0].value  # type: ignore[attr-defined]
    assert ESC not in rejections[0].value  # type: ignore[attr-defined]


async def test_hostile_execution_id_is_dropped_without_an_ack() -> None:
    """Nothing addresses the server here, so there is nothing to report."""
    calls, records = await _run_one_assignment(
        {
            "execution_id": f"exec{ESC}[2J\r\n001",
            "job_key": "billing:invoice",
            "fire_at": "2026-05-23T10:00:00Z",
            "attempt": 1,
            "metadata": {},
            "timeout": "5m",
        }
    )

    assert [p for p, _ in calls if p == "/v1/work/ack"] == []
    rejections = [r for r in records if "unsafe identifier" in r.getMessage()]
    assert len(rejections) == 1
    assert rejections[0].field == "execution_id"  # type: ignore[attr-defined]
    assert rejections[0].acked is False  # type: ignore[attr-defined]


async def test_legitimate_assignment_round_trips_and_logs_identifiers_as_fields() -> None:
    calls, records = await _run_one_assignment(
        {
            "execution_id": "6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77",
            "job_key": "billing:invoice",
            "fire_at": "2026-05-23T10:00:00Z",
            "attempt": 1,
            "metadata": {},
            "timeout": "5m",
        }
    )

    acks = [b for p, b in calls if p == "/v1/work/ack"]
    assert len(acks) == 1
    assert acks[0]["execution_id"] == "6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77"
    assert acks[0]["status"] == "failure"

    warned = [r for r in records if r.levelno == logging.WARNING and "handler raised" in r.getMessage()]
    assert len(warned) == 1
    # Identifiers travel as record attributes …
    assert warned[0].job_key == "billing:invoice"  # type: ignore[attr-defined]
    assert warned[0].execution_id == "6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77"  # type: ignore[attr-defined]
    # … and appear in no rendered message the SDK produced.
    for record in records:
        assert "billing:invoice" not in record.getMessage()
        assert "6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77" not in record.getMessage()


def test_job_logger_name_is_fixed_and_does_not_embed_the_job_key() -> None:
    """The per-job logger namespace was an unbounded, server-controlled cache."""
    from datetime import timedelta

    from croniq_runner._context import ExecutionContext
    from croniq_runner._options import LogWriterOptions

    ctx = ExecutionContext(
        execution_id="exec-1",
        job_key="billing:invoice",
        scheduled_for=None,
        attempt=1,
        metadata={},
        timeout=timedelta(minutes=5),
        runner_id="r-test",
        runner_tags=[],
        cancellation=asyncio.Event(),
        client=None,  # type: ignore[arg-type]
        log_writer_options=LogWriterOptions(),
    )

    assert ctx.logger.logger.name == "croniq_runner.job"
    assert "billing:invoice" not in ctx.logger.logger.name
    assert ctx.logger.extra["job_key"] == "billing:invoice"
