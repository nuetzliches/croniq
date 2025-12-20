#!/usr/bin/env python3
"""Validate ReportGenerator summary coverage thresholds."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Ensure coverage gates pass.")
    parser.add_argument("summary", type=Path, help="Path to ReportGenerator Summary.json")
    parser.add_argument("--core-assembly", default="Croniq.Core", help="Assembly name to enforce dedicated gate for.")
    parser.add_argument("--core-threshold", type=float, default=73.0, help="Minimum coverage percentage for the core assembly.")
    parser.add_argument("--overall-threshold", type=float, default=75.0, help="Minimum overall coverage percentage.")
    parser.add_argument("--core-branch-threshold", type=float, default=None, help="Minimum branch coverage percentage for the core assembly.")
    parser.add_argument("--overall-branch-threshold", type=float, default=None, help="Minimum overall branch coverage percentage.")
    return parser.parse_args()


def load_summary(summary_path: Path) -> dict:
    if not summary_path.is_file():
        raise FileNotFoundError(f"Coverage summary not found at {summary_path}")
    with summary_path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def extract_overall(summary: dict) -> float:
    try:
        return float(summary["summary"]["linecoverage"])
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError("Unable to read overall coverage from summary JSON") from exc


def extract_overall_branch(summary: dict) -> float:
    try:
        return float(summary["summary"]["branchcoverage"])
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError("Unable to read overall branch coverage from summary JSON") from exc


def extract_assembly(summary: dict, assembly_name: str) -> float:
    assemblies = summary.get("coverage", {}).get("assemblies", [])
    for assembly in assemblies:
        if assembly.get("name") == assembly_name:
            try:
                return float(assembly["coverage"])
            except (KeyError, TypeError, ValueError) as exc:
                raise ValueError(f"Invalid coverage value for assembly {assembly_name}") from exc
    raise ValueError(f"Assembly {assembly_name} not found in coverage summary")


def extract_assembly_branch(summary: dict, assembly_name: str) -> float:
    assemblies = summary.get("coverage", {}).get("assemblies", [])
    for assembly in assemblies:
        if assembly.get("name") == assembly_name:
            try:
                return float(assembly["branchcoverage"])
            except (KeyError, TypeError, ValueError) as exc:
                raise ValueError(f"Invalid branch coverage value for assembly {assembly_name}") from exc
    raise ValueError(f"Assembly {assembly_name} not found in coverage summary")


def enforce_thresholds(
    overall: float,
    overall_threshold: float,
    core: float,
    core_threshold: float,
    overall_branch: float | None,
    overall_branch_threshold: float | None,
    core_branch: float | None,
    core_branch_threshold: float | None) -> None:
    errors: list[str] = []
    if overall < overall_threshold:
        errors.append(f"Overall coverage {overall:.2f}% is below the {overall_threshold:.2f}% gate")
    if core < core_threshold:
        errors.append(f"{core_threshold:.2f}% gate failed for core assembly: {core:.2f}%")
    if overall_branch_threshold is not None:
        if overall_branch is None:
            errors.append("Overall branch coverage is missing from the summary")
        elif overall_branch < overall_branch_threshold:
            errors.append(
                f"Overall branch coverage {overall_branch:.2f}% is below the {overall_branch_threshold:.2f}% gate")
    if core_branch_threshold is not None:
        if core_branch is None:
            errors.append("Branch coverage for the core assembly is missing from the summary")
        elif core_branch < core_branch_threshold:
            errors.append(
                f"{core_branch_threshold:.2f}% branch gate failed for core assembly: {core_branch:.2f}%")

    print(f"Overall coverage: {overall:.2f}%")
    if overall_branch is not None:
        print(f"Overall branch coverage: {overall_branch:.2f}%")
    print(f"Core coverage: {core:.2f}%")
    if core_branch is not None:
        print(f"Core branch coverage: {core_branch:.2f}%")

    if errors:
        for message in errors:
            print(message, file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    args = parse_args()
    summary_data = load_summary(args.summary)
    overall_value = extract_overall(summary_data)
    overall_branch_value = None
    if args.overall_branch_threshold is not None:
        overall_branch_value = extract_overall_branch(summary_data)
    core_value = extract_assembly(summary_data, args.core_assembly)
    core_branch_value = None
    if args.core_branch_threshold is not None:
        core_branch_value = extract_assembly_branch(summary_data, args.core_assembly)
    enforce_thresholds(
        overall=overall_value,
        overall_threshold=args.overall_threshold,
        core=core_value,
        core_threshold=args.core_threshold,
        overall_branch=overall_branch_value,
        overall_branch_threshold=args.overall_branch_threshold,
        core_branch=core_branch_value,
        core_branch_threshold=args.core_branch_threshold)
