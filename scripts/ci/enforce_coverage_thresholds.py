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
    parser.add_argument("--core-threshold", type=float, default=80.0, help="Minimum coverage percentage for the core assembly.")
    parser.add_argument("--overall-threshold", type=float, default=70.0, help="Minimum overall coverage percentage.")
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


def extract_assembly(summary: dict, assembly_name: str) -> float:
    assemblies = summary.get("coverage", {}).get("assemblies", [])
    for assembly in assemblies:
        if assembly.get("name") == assembly_name:
            try:
                return float(assembly["coverage"])
            except (KeyError, TypeError, ValueError) as exc:
                raise ValueError(f"Invalid coverage value for assembly {assembly_name}") from exc
    raise ValueError(f"Assembly {assembly_name} not found in coverage summary")


def enforce_thresholds(overall: float, overall_threshold: float, core: float, core_threshold: float) -> None:
    errors: list[str] = []
    if overall < overall_threshold:
        errors.append(f"Overall coverage {overall:.2f}% is below the {overall_threshold:.2f}% gate")
    if core < core_threshold:
        errors.append(f"{core_threshold:.2f}% gate failed for core assembly: {core:.2f}%")

    print(f"Overall coverage: {overall:.2f}%")
    print(f"Core coverage: {core:.2f}%")

    if errors:
        for message in errors:
            print(message, file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    args = parse_args()
    summary_data = load_summary(args.summary)
    overall_value = extract_overall(summary_data)
    core_value = extract_assembly(summary_data, args.core_assembly)
    enforce_thresholds(overall=overall_value, overall_threshold=args.overall_threshold, core=core_value, core_threshold=args.core_threshold)
