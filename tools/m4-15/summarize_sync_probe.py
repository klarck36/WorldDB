#!/usr/bin/env python3
"""Summarize CSV output from macos_sync_probe.c without hiding API errors."""

from __future__ import annotations

import csv
import errno
import json
import math
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Iterable


FIELDS = ("operation", "iteration", "return", "errno", "elapsed_ns")
MEASUREMENTS = ("fsync", "fullfsync")
FAULTS = ("fault_fsync_ebadf", "fault_fullfsync_ebadf")
OPERATIONS = frozenset((*MEASUREMENTS, *FAULTS))


def percentile(values: list[int], fraction: float) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, math.ceil(fraction * len(ordered)) - 1)
    return ordered[index]


def parse_rows(rows: Iterable[dict[str, str]]) -> list[dict[str, int | str]]:
    parsed: list[dict[str, int | str]] = []
    for line_number, row in enumerate(rows, start=2):
        if any(row.get(field) is None for field in FIELDS):
            raise ValueError(f"CSV row {line_number} is missing a required field")
        operation = row["operation"]
        if operation not in OPERATIONS:
            raise ValueError(f"CSV row {line_number} has unknown operation {operation!r}")
        try:
            iteration = int(row["iteration"])
            result = int(row["return"])
            error_number = int(row["errno"])
            elapsed_ns = int(row["elapsed_ns"])
        except (TypeError, ValueError) as error:
            raise ValueError(f"CSV row {line_number} has a non-integer numeric field") from error
        if iteration < 0 or error_number < 0 or elapsed_ns < 0 or result not in (0, -1):
            raise ValueError(f"CSV row {line_number} contains an out-of-range value")
        parsed.append(
            {
                "operation": operation,
                "iteration": iteration,
                "return": result,
                "errno": error_number,
                "elapsed_ns": elapsed_ns,
            }
        )
    return parsed


def build_report(rows: list[dict[str, int | str]]) -> dict[str, object]:
    by_operation: dict[str, list[dict[str, int | str]]] = defaultdict(list)
    for row in rows:
        by_operation[str(row["operation"])].append(row)

    missing_measurements = [name for name in MEASUREMENTS if not by_operation[name]]
    if missing_measurements:
        raise ValueError(f"CSV is missing measurement rows: {', '.join(missing_measurements)}")

    measurements: dict[str, object] = {}
    for operation in MEASUREMENTS:
        samples = by_operation[operation]
        failures = Counter(
            (int(row["return"]), int(row["errno"]))
            for row in samples
            if int(row["return"]) != 0
        )
        successful_latencies = [
            int(row["elapsed_ns"]) for row in samples if int(row["return"]) == 0
        ]
        measurements[operation] = {
            "samples": len(samples),
            "successes": len(successful_latencies),
            "failures": [
                {"return": result, "errno": error_number, "count": count}
                for (result, error_number), count in sorted(failures.items())
            ],
            "successful_latency_ns": {
                "median": statistics.median(successful_latencies) if successful_latencies else None,
                "p95": percentile(successful_latencies, 0.95),
                "p99": percentile(successful_latencies, 0.99),
                "min": min(successful_latencies) if successful_latencies else None,
                "max": max(successful_latencies) if successful_latencies else None,
            },
        }

    fault_controls: dict[str, object] = {}
    for operation in FAULTS:
        samples = by_operation[operation]
        expected = len(samples) == 1 and (
            int(samples[0]["return"]) == -1 and int(samples[0]["errno"]) == errno.EBADF
        )
        fault_controls[operation] = {
            "samples": len(samples),
            "expected_ebadf": expected,
            "observed": [
                {"return": int(row["return"]), "errno": int(row["errno"])}
                for row in samples
            ],
        }

    return {"measurements": measurements, "fault_controls": fault_controls}


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {Path(argv[0]).name} SAMPLES.csv", file=sys.stderr)
        return 2
    try:
        with Path(argv[1]).open(newline="", encoding="utf-8") as sample_file:
            reader = csv.DictReader(sample_file)
            if tuple(reader.fieldnames or ()) != FIELDS:
                raise ValueError(f"CSV header must be exactly: {','.join(FIELDS)}")
            report = build_report(parse_rows(reader))
    except (OSError, csv.Error, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    print(json.dumps(report, indent=2, sort_keys=True))
    controls_ok = all(
        bool(control["expected_ebadf"])
        for control in report["fault_controls"].values()
    )
    return 0 if controls_ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
