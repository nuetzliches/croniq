"""The loaders must reject a key they do not implement.

A loader that silently drops unrecognised keys has no failure mode of its own:
the suite goes green exactly when the contract stops being enforced (#460).
These tests provoke the silence and assert that it is now noisy — the same
anti-rot role the ``UNSUPPORTED``-entry guard plays for #453.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from tests.conformance.case_spec import load_case
from tests.conformance.trigger_case_spec import load_trigger_case

MINIMAL_CASE = """
name: strictness probe
runner_config:
  capabilities: ["work"]
handlers:
  - job_key: "work:probe"
    behavior: noop
server_script:
  - on: "POST /v1/work/poll"
    respond:
      status: 200
      body: { work: [], cancel: [] }
expectations:
  duration_max_ms: 500
  http:
    - method: POST
      path: /v1/work/poll
      min_count: 1
"""

MINIMAL_TRIGGER_CASE = """
name: strictness probe
trigger_config:
  api_key: "croniq_testkey"
trigger_calls:
  - request:
      job_key: "work:probe"
    expect:
      response:
        execution_id: "*"
server_script:
  - on: "POST /v1/trigger"
    respond:
      status: 200
      body: { execution_id: "exec-001", queued: 1, deduplicated: false }
expectations:
  duration_max_ms: 500
  http:
    - method: POST
      path: /v1/trigger
      exact_count: 1
"""


def _write(tmp_path: Path, text: str) -> Path:
    path = tmp_path / "case.yaml"
    path.write_text(text, encoding="utf-8")
    return path


def _inject(text: str, anchor: str, addition: str, indent: int | None = None) -> str:
    """Insert ``addition`` after ``anchor`` at column ``indent``.

    The indent selects *which* mapping gains the key, so it cannot always be
    read off the anchor line: a key inside a ``- `` list item sits two columns
    right of the item's dash, and closing a nested block means dedenting below
    the anchor. Defaults to the anchor's own indentation.
    """
    assert anchor in text, f"fixture must contain {anchor!r}"
    if indent is None:
        indent = len(anchor) - len(anchor.lstrip())
    return text.replace(anchor, f"{anchor}\n{' ' * indent}{addition}", 1)


# One entry per level a runner case nests — an unknown key must be caught at
# each of them, not merely at the top.
@pytest.mark.parametrize(
    ("where", "anchor", "indent"),
    [
        ("case", "name: strictness probe", None),
        ("runner_config", '  capabilities: ["work"]', None),
        ("handler", "    behavior: noop", None),
        # A key of the list item itself, two columns right of its dash.
        ("server_script entry", '  - on: "POST /v1/work/poll"', 4),
        ("respond", "      status: 200", None),
        ("expectations", "  duration_max_ms: 500", None),
        ("http expectation", "      min_count: 1", None),
    ],
)
def test_load_case_rejects_a_key_the_binding_does_not_model(
    tmp_path: Path, where: str, anchor: str, indent: int | None
) -> None:
    yaml_text = _inject(MINIMAL_CASE, anchor, "not_a_real_key: 1", indent)

    with pytest.raises(ValueError, match="not_a_real_key") as excinfo:
        load_case(_write(tmp_path, yaml_text))

    assert where in str(excinfo.value)


@pytest.mark.parametrize(
    ("where", "anchor", "indent"),
    [
        ("trigger case", "name: strictness probe", None),
        ("trigger_config", '  api_key: "croniq_testkey"', None),
        ("trigger_calls request", '      job_key: "work:probe"', None),
        # Same anchor, two indents: dedenting to 6 closes `response:` and adds
        # the key to `expect`; staying at 8 adds it to `response` itself.
        ("expect", '        execution_id: "*"', 6),
        ("expect.response", '        execution_id: "*"', None),
        ("http expectation", "      exact_count: 1", None),
    ],
)
def test_load_trigger_case_rejects_a_key_the_binding_does_not_model(
    tmp_path: Path, where: str, anchor: str, indent: int | None
) -> None:
    yaml_text = _inject(MINIMAL_TRIGGER_CASE, anchor, "not_a_real_key: 1", indent)

    with pytest.raises(ValueError, match="not_a_real_key") as excinfo:
        load_trigger_case(_write(tmp_path, yaml_text))

    assert where in str(excinfo.value)


def test_body_absent_is_runner_forbidden_and_trigger_allowed(tmp_path: Path) -> None:
    """``body_absent`` is declared by trigger-case-schema.json only.

    The two loaders share one ``HttpExpectation`` dataclass, so without
    per-loader key sets a runner case could quietly carry a trigger-only key.
    """
    runner_text = _inject(MINIMAL_CASE, "      min_count: 1", "body_absent: [metadata]")
    with pytest.raises(ValueError, match="body_absent"):
        load_case(_write(tmp_path, runner_text))

    trigger_text = _inject(
        MINIMAL_TRIGGER_CASE, "      exact_count: 1", "body_absent: [metadata]"
    )
    spec = load_trigger_case(_write(tmp_path, trigger_text))
    assert spec.expectations.http[0].body_absent == ["metadata"]


def test_loaders_accept_the_known_vocabulary(tmp_path: Path) -> None:
    """Counterweight: strictness must not reject what the corpus legitimately uses.

    Also keeps the fixtures above honest — one that failed to load on its own
    would make every negative test pass for the wrong reason.
    """
    case = load_case(_write(tmp_path, MINIMAL_CASE))
    assert case.name == "strictness probe"
    assert len(case.handlers) == 1

    trigger = load_trigger_case(_write(tmp_path, MINIMAL_TRIGGER_CASE))
    assert trigger.name == "strictness probe"
    assert len(trigger.trigger_calls) == 1
