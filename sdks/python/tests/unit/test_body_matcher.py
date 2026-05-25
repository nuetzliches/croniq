"""Tests for the conformance body matcher."""

from __future__ import annotations

from tests.conformance.body_matcher import match_body


def test_wildcard_matches_any_non_empty_string() -> None:
    assert match_body("*", "hello") is None
    assert match_body("*", "") is not None
    assert match_body("*", None) is not None


def test_wildcard_matches_any_non_null() -> None:
    assert match_body("*", 42) is None
    assert match_body("*", [1]) is None
    assert match_body("*", {"k": "v"}) is None


def test_subset_object_match_ignores_extra_keys() -> None:
    expected = {"runner_id": "*", "status": "success"}
    actual = {"runner_id": "r1", "status": "success", "extra": "ok", "duration_ms": 42}
    assert match_body(expected, actual) is None


def test_missing_key_returns_error() -> None:
    err = match_body({"runner_id": "*"}, {})
    assert err is not None and "missing key" in err


def test_string_mismatch_returns_error() -> None:
    err = match_body({"status": "success"}, {"status": "failure"})
    assert err is not None and "expected 'success'" in err


def test_nested_object_recurses() -> None:
    expected = {"work": [{"job_key": "billing:invoice"}]}
    actual = {"work": [{"job_key": "billing:invoice", "execution_id": "e1"}]}
    assert match_body(expected, actual) is None


def test_array_length_must_match() -> None:
    err = match_body([1, 2, 3], [1, 2])
    assert err is not None and "expected 3 item(s)" in err


def test_integer_match() -> None:
    assert match_body(1, 1) is None
    err = match_body(2, 1)
    assert err is not None and "expected 2 but got 1" in err


def test_bool_match() -> None:
    assert match_body(True, True) is None
    assert match_body(False, False) is None
    err = match_body(True, 1)  # bool ≠ int even though Python coerces
    assert err is not None
