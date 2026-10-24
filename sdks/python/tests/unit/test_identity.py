"""Tests for the runner-id resolver."""

from __future__ import annotations

from pathlib import Path

import pytest

from croniq_runner._identity import resolve_runner_id
from croniq_runner._options import RunnerOptions


def test_explicit_option_wins(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CRONIQ_RUNNER_DATA_DIR", str(tmp_path))
    monkeypatch.setenv("RUNNER_ID", "from-env")
    opts = RunnerOptions(runner_id="from-options")
    assert resolve_runner_id(opts) == "from-options"


def test_env_var_used_when_no_explicit(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CRONIQ_RUNNER_DATA_DIR", str(tmp_path))
    monkeypatch.setenv("RUNNER_ID", "from-env")
    assert resolve_runner_id(RunnerOptions()) == "from-env"


def test_state_file_used_when_no_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CRONIQ_RUNNER_DATA_DIR", str(tmp_path))
    monkeypatch.delenv("RUNNER_ID", raising=False)
    (tmp_path / "runner-id").write_text("stored-id\n", encoding="utf-8")
    assert resolve_runner_id(RunnerOptions()) == "stored-id"


def test_generated_id_uses_prefix_and_persists(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("CRONIQ_RUNNER_DATA_DIR", str(tmp_path))
    monkeypatch.delenv("RUNNER_ID", raising=False)
    opts = RunnerOptions(runner_id_prefix="pytest-runner")
    first = resolve_runner_id(opts)
    assert first.startswith("pytest-runner-")
    assert (tmp_path / "runner-id").read_text(encoding="utf-8").strip() == first
    # Second call must return the same persisted value.
    assert resolve_runner_id(RunnerOptions()) == first


def test_persist_failure_is_logged_at_warning(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, caplog: pytest.LogCaptureFixture
) -> None:
    # Review 2026-10-24, K-6: a state directory that cannot be written used to
    # be swallowed silently, so the runner minted a new runner_id on every
    # restart without any signal. A regular file in place of the directory
    # makes ``mkdir`` fail on every platform.
    blocker = tmp_path / "not-a-dir"
    blocker.write_text("x", encoding="utf-8")
    monkeypatch.setenv("CRONIQ_RUNNER_DATA_DIR", str(blocker / "state"))
    monkeypatch.delenv("RUNNER_ID", raising=False)

    with caplog.at_level("WARNING", logger="croniq_runner.identity"):
        fresh = resolve_runner_id(RunnerOptions(runner_id_prefix="pytest-runner"))

    assert fresh.startswith("pytest-runner-")
    warnings = [r for r in caplog.records if r.levelname == "WARNING"]
    assert len(warnings) == 1
    message = warnings[0].getMessage()
    assert "will not survive a restart" in message
    assert str(blocker / "state" / "runner-id") in message
