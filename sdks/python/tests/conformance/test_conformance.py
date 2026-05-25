"""One pytest per language-agnostic conformance case.

The cases live at ``<repo>/sdks/conformance/cases/*.yaml`` — adding a new
YAML automatically adds a new test.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from pytest_httpserver import HTTPServer

from tests.conformance.case_spec import CaseSpec, load_case
from tests.conformance.runner import run_case


def _cases_dir() -> Path:
    # sdks/python/tests/conformance/test_conformance.py → ../../../conformance/cases
    return Path(__file__).resolve().parents[3] / "conformance" / "cases"


def _discover_cases() -> list[Path]:
    d = _cases_dir()
    if not d.exists():
        return []
    return sorted(d.glob("*.yaml"))


@pytest.mark.parametrize(
    "case_path",
    _discover_cases(),
    ids=[p.name for p in _discover_cases()],
)
async def test_conformance(case_path: Path, httpserver: HTTPServer) -> None:
    spec: CaseSpec = load_case(case_path)
    await run_case(httpserver, spec)
