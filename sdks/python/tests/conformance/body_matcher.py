"""Subset matcher with a single wildcard token ("*").

Used to assert request bodies and headers without forcing tests to mirror
every SDK-emitted field.
"""

from __future__ import annotations

from typing import Any


def match_body(expected: Any, actual: Any, path: str = "$") -> str | None:
    """Return ``None`` on success, or a human-readable diff string on mismatch."""
    if expected is None:
        if actual is None:
            return None
        return f"{path}: expected null but got {type(actual).__name__}"

    if isinstance(expected, str) and expected == "*":
        if actual is None:
            return f"{path}: expected non-empty wildcard match but got null"
        if isinstance(actual, str) and actual == "":
            return f"{path}: expected non-empty string but got empty"
        return None

    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            return f"{path}: expected object but got {type(actual).__name__}"
        for k, v in expected.items():
            if k not in actual:
                return f"{path}.{k}: missing key"
            err = match_body(v, actual[k], f"{path}.{k}")
            if err is not None:
                return err
        return None

    if isinstance(expected, list):
        if not isinstance(actual, list):
            return f"{path}: expected array but got {type(actual).__name__}"
        if len(expected) != len(actual):
            return f"{path}: expected {len(expected)} item(s) but got {len(actual)}"
        for i, (e, a) in enumerate(zip(expected, actual, strict=True)):
            err = match_body(e, a, f"{path}[{i}]")
            if err is not None:
                return err
        return None

    if isinstance(expected, bool):
        # bool is a subclass of int — check before the numeric branch.
        if isinstance(actual, bool):
            return None if expected == actual else f"{path}: expected {expected} but got {actual}"
        return f"{path}: expected bool but got {type(actual).__name__}"

    if isinstance(expected, int | float):
        if isinstance(actual, bool) or not isinstance(actual, int | float):
            return f"{path}: expected number but got {type(actual).__name__}"
        if abs(actual - expected) < 1e-9:
            return None
        return f"{path}: expected {expected} but got {actual}"

    if isinstance(expected, str):
        if not isinstance(actual, str):
            return f"{path}: expected string '{expected}' but got {type(actual).__name__}"
        return None if expected == actual else f"{path}: expected '{expected}' but got '{actual}'"

    return f"{path}: unsupported expected type {type(expected).__name__}"
