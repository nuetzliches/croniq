#!/usr/bin/env python3
"""
Validate licenses in a Syft SPDX JSON SBOM against an allow-list.

Usage:
  python scripts/ci/check-licenses.py path/to/sbom.json path/to/allowed-licenses.json [--fail-on-missing]
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Iterable, List, Set


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Verify package licenses from a Syft SPDX JSON SBOM.")
    parser.add_argument("sbom", type=Path, help="Path to Syft SPDX JSON file.")
    parser.add_argument("allowlist", type=Path, help="Path to allowed license identifiers (JSON array).")
    parser.add_argument("--fail-on-missing", action="store_true", help="Fail when a package has no detectable license.")
    return parser.parse_args()


def load_allowlist(path: Path) -> Set[str]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, list):
        raise ValueError("Allowlist must be a JSON array of SPDX identifiers.")
    return {str(item).strip() for item in data if str(item).strip()}


def extract_licenses(package: dict) -> List[str]:
    """
    Returns a list of license tokens extracted from the package entry.
    Preference order: licenseConcluded -> licenseDeclared -> licenseInfoFromFiles.
    Expressions are split on common SPDX operators (AND/OR/WITH/parentheses).
    """
    candidate = (
        package.get("licenseConcluded")
        or package.get("licenseDeclared")
        or (package.get("licenseInfoFromFiles") or [None])[0]
    )
    if not candidate:
        return []

    tokens: List[str] = []
    # Replace SPDX operators with spaces, strip parentheses, then split
    cleaned = (
        str(candidate)
        .replace("(", " ")
        .replace(")", " ")
        .replace("WITH", " ")
        .replace("with", " ")
        .replace("AND", " ")
        .replace("and", " ")
        .replace("OR", " ")
        .replace("or", " ")
    )
    for part in cleaned.split():
        part = part.strip()
        if part:
            tokens.append(part)

    # Treat NOASSERTION / NONE as "missing" so we can ignore them unless the caller
    # explicitly opts into --fail-on-missing.
    ignore_tokens = {"NOASSERTION", "NONE", "NONEFOUND", "UNKNOWN"}
    filtered = [t for t in tokens if t.upper() not in ignore_tokens]
    return filtered


def find_violations(packages: Iterable[dict], allowlist: Set[str], fail_on_missing: bool) -> List[dict]:
    violations: List[dict] = []
    for pkg in packages:
        licenses = extract_licenses(pkg)
        if not licenses and not fail_on_missing:
            continue
        if not licenses and fail_on_missing:
            violations.append(
                {
                    "name": pkg.get("name"),
                    "version": pkg.get("versionInfo"),
                    "license": None,
                    "reason": "missing",
                }
            )
            continue

        unknown = [lic for lic in licenses if lic not in allowlist]
        if unknown:
            violations.append(
                {
                    "name": pkg.get("name"),
                    "version": pkg.get("versionInfo"),
                    "license": pkg.get("licenseConcluded") or pkg.get("licenseDeclared"),
                    "tokens": unknown,
                    "reason": "disallowed",
                }
            )
    return violations


def main() -> int:
    args = parse_args()
    sbom = json.loads(args.sbom.read_text(encoding="utf-8"))
    allowlist = load_allowlist(args.allowlist)

    packages = sbom.get("packages") or []
    violations = find_violations(packages, allowlist, args.fail_on_missing)

    if violations:
        print("License validation failed:", file=sys.stderr)
        for v in violations:
            print(
                f"- {v['name']}@{v.get('version')} -> {v.get('license')} (reason: {v['reason']}, tokens: {v.get('tokens')})",
                file=sys.stderr,
            )
        return 1

    print(f"License validation passed. Packages checked: {len(packages)}; allowed licenses: {len(allowlist)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
