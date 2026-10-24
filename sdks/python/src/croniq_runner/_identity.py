"""Runner ID resolution.

Resolution order:

1. ``RunnerOptions.runner_id`` (explicit override).
2. ``$RUNNER_ID`` environment variable.
3. State file under ``$CRONIQ_RUNNER_DATA_DIR`` / ``$XDG_STATE_HOME/croniq`` /
   ``~/.local/state/croniq``.
4. A freshly generated ``{prefix}-{hex8}`` value, persisted to the state file.
"""

from __future__ import annotations

import logging
import os
import secrets
from pathlib import Path

from croniq_runner._options import RunnerOptions

_log = logging.getLogger("croniq_runner.identity")


def resolve_runner_id(options: RunnerOptions) -> str:
    if options.runner_id:
        return options.runner_id

    env = os.environ.get("RUNNER_ID")
    if env:
        return env

    data_dir = _data_dir()
    state_file = data_dir / "runner-id"
    if state_file.exists():
        try:
            stored = state_file.read_text(encoding="utf-8").strip()
            if stored:
                return stored
        except OSError as exc:
            _log.warning(
                "could not read the persisted runner_id from %s (%s) — generating a new one",
                state_file,
                exc,
            )

    fresh = f"{options.runner_id_prefix}-{secrets.token_hex(4)}"
    try:
        data_dir.mkdir(parents=True, exist_ok=True)
        state_file.write_text(fresh, encoding="utf-8")
    except OSError as exc:
        # Non-fatal — the runner can still operate with a non-persisted ID, but
        # it regenerates on the next start, and under ADR-0008 every restart
        # then binds a fresh identity. That used to happen silently (review
        # 2026-10-24, K-6): a container without a writable state directory
        # looked healthy while minting a new runner_id on every restart.
        _log.warning(
            "could not persist runner_id to %s (%s) — the runner_id will not survive a "
            "restart; set RUNNER_ID, or point CRONIQ_RUNNER_DATA_DIR at a writable directory",
            state_file,
            exc,
        )
    return fresh


def _data_dir() -> Path:
    override = os.environ.get("CRONIQ_RUNNER_DATA_DIR")
    if override:
        return Path(override)
    xdg = os.environ.get("XDG_STATE_HOME")
    if xdg:
        return Path(xdg) / "croniq"
    return Path.home() / ".local" / "state" / "croniq"
