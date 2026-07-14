"""Orchestrator for a single trigger (producer) conformance case.

Starts the scripted mock server, points a :class:`~croniq_runner.TriggerClient`
at it (with the case's own credentials — never a runner's), makes each declared
``trigger(...)`` call in order, asserts the surfaced result (or error), then
asserts the recorded HTTP traffic (counts, headers, body subset, omitted keys).
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import httpx

from croniq_runner import TriggerClient, TriggerClientOptions
from tests.conformance.body_matcher import match_body
from tests.conformance.mock_server import MockServerHarness, RecordedRequest

if TYPE_CHECKING:
    from pytest_httpserver import HTTPServer

    from tests.conformance.case_spec import ExpectationsSpec
    from tests.conformance.trigger_case_spec import TriggerCallSpec, TriggerCaseSpec


async def run_trigger_case(httpserver: HTTPServer, spec: TriggerCaseSpec) -> None:
    mock = MockServerHarness(httpserver, spec.server_script)
    options = TriggerClientOptions(
        server_url=mock.base_url,
        api_key=spec.trigger_config.api_key,
        bearer_token=spec.trigger_config.bearer_token,
    )

    async with TriggerClient(options) as client:
        for call in spec.trigger_calls:
            await _run_call(client, call)

    _assert_expectations(spec.expectations, mock.recorded)


async def _run_call(client: TriggerClient, call: TriggerCallSpec) -> None:
    req = call.request
    job_key = req["job_key"]
    kwargs = {k: v for k, v in req.items() if k != "job_key"}

    if call.expect.error:
        # The contract is "surfaced as an error" regardless of the SDK's idiom.
        # This binding forwards non-2xx as httpx.HTTPStatusError and rejects a
        # blank job_key locally as ValueError — accept either.
        raised = False
        try:
            await client.trigger(job_key, **kwargs)
        except (httpx.HTTPError, ValueError):
            raised = True
        assert raised, f"trigger({job_key!r}) was expected to error but returned a value"
        return

    result = await client.trigger(job_key, **kwargs)

    expected = call.expect.response or {}
    if "execution_id" in expected:
        if expected["execution_id"] == "*":
            assert result.execution_id, f"trigger({job_key!r}): expected non-empty execution_id"
        else:
            assert result.execution_id == expected["execution_id"]
    if "queued" in expected:
        assert result.queued == expected["queued"]
    if "deduplicated" in expected:
        assert result.deduplicated is expected["deduplicated"]


def _assert_expectations(
    expectations: ExpectationsSpec, recorded: list[RecordedRequest]
) -> None:
    for ex in expectations.http:
        matches = [
            r for r in recorded if r.method.upper() == ex.method.upper() and r.path == ex.path
        ]
        if ex.exact_count is not None:
            assert len(matches) == ex.exact_count, (
                f"{ex.method} {ex.path}: expected exact_count={ex.exact_count}, got {len(matches)}"
            )
        if ex.min_count is not None:
            assert len(matches) >= ex.min_count, (
                f"{ex.method} {ex.path}: expected min_count={ex.min_count}, got {len(matches)}"
            )
        if ex.max_count is not None:
            assert len(matches) <= ex.max_count, (
                f"{ex.method} {ex.path}: expected max_count={ex.max_count}, got {len(matches)}"
            )

        if ex.headers and matches:
            first = matches[0]
            for name, want in ex.headers.items():
                lname = name.lower()
                assert lname in first.headers, f"{ex.method} {ex.path}: missing header '{name}'"
                actual = first.headers[lname]
                if want == "*":
                    assert actual, f"{ex.method} {ex.path}: header '{name}' was empty"
                else:
                    assert actual == want, (
                        f"{ex.method} {ex.path}: header '{name}' expected '{want}', got '{actual}'"
                    )

        if ex.body_match is not None and matches:
            body = matches[0].json_body()
            err = match_body(ex.body_match, body)
            assert err is None, f"{ex.method} {ex.path}: body mismatch — {err}"

        if ex.body_absent and matches:
            body = matches[0].json_body()
            assert isinstance(body, dict), (
                f"{ex.method} {ex.path}: body_absent needs a JSON object body"
            )
            for key in ex.body_absent:
                assert key not in body, (
                    f"{ex.method} {ex.path}: key '{key}' must be omitted but was present"
                )
