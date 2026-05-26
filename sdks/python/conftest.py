"""Pytest configuration shared by unit and conformance tests."""

from __future__ import annotations

import sys
from pathlib import Path

# Ensure ``tests`` is importable as a package when pytest is invoked from
# ``sdks/python/`` (so ``tests.conformance.case_spec`` resolves).
sys.path.insert(0, str(Path(__file__).parent))
