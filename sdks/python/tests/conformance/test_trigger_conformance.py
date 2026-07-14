"""One pytest per language-agnostic trigger (producer) conformance case.

The cases live at ``<repo>/sdks/conformance/cases-trigger/*.yaml`` (#287) —
adding a new YAML automatically adds a new test. Discovery tolerates the
directory being absent (e.g. if the shared trigger suite has not landed yet):
the parametrisation is then empty and the module contributes no tests.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from pytest_httpserver import HTTPServer

from tests.conformance.trigger_case_spec import TriggerCaseSpec, load_trigger_case
from tests.conformance.trigger_runner import run_trigger_case


def _cases_dir() -> Path:
    # sdks/python/tests/conformance/test_trigger_conformance.py
    #   → ../../../conformance/cases-trigger
    return Path(__file__).resolve().parents[3] / "conformance" / "cases-trigger"


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
async def test_trigger_conformance(case_path: Path, httpserver: HTTPServer) -> None:
    spec: TriggerCaseSpec = load_trigger_case(case_path)
    await run_trigger_case(httpserver, spec)
