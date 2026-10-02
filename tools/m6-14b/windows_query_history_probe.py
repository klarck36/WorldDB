"""Run full-corpus Windows Point Resolution and paginated History probes."""

from __future__ import annotations

import argparse
import csv
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
from collections import defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "m6-14a"))
sys.path.insert(0, str(ROOT / "tools" / "m6-14b"))
from build_corpus import executable, local_hardware_profile  # noqa: E402
from verify_corpus import verify  # noqa: E402
from windows_probe import peak_working_set  # noqa: E402


RAW_NAME = "windows-query-history-probe.csv"
FIELDS = ("operation", "iteration", "page", "result_count", "elapsed_ns")
MIB = 1024 * 1024


def nearest_rank(values: list[int], fraction: float) -> int:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(fraction * len(ordered)) - 1)]


def validate_rows(path: Path) -> list[dict[str, int | str]]:
    rows: list[dict[str, int | str]] = []
    with path.open(newline="", encoding="utf-8") as stream:
        reader = csv.DictReader(stream)
        if tuple(reader.fieldnames or ()) != FIELDS:
            raise RuntimeError(f"unexpected raw probe fields in {path}")
        for item in reader:
            row: dict[str, int | str] = {
                "operation": item["operation"],
                "iteration": int(item["iteration"]),
                "page": int(item["page"]),
                "result_count": int(item["result_count"]),
                "elapsed_ns": int(item["elapsed_ns"]),
            }
            if int(row["elapsed_ns"]) <= 0 or int(row["result_count"]) < 0:
                raise RuntimeError("probe returned an invalid measurement")
            rows.append(row)
    if not rows:
        raise RuntimeError("Rust probe returned no measurements")
    for operation in (
        "point_resolution_warm",
        "point_resolution_cold_cpu_cache",
        "history_page_warm",
        "history_page_cold_cpu_cache",
    ):
        if not any(row["operation"] == operation for row in rows):
            raise RuntimeError(f"Rust probe omitted required operation {operation}")
    point_operations = {
        name: [row for row in rows if row["operation"] == name]
        for name in ("point_resolution_warm", "point_resolution_cold_cpu_cache")
    }
    if any(len(samples) != 30 for samples in point_operations.values()):
        raise RuntimeError("Point Resolution must have 30 warm and cold samples")
    pages_by_operation: dict[str, dict[int, int]] = {}
    for operation in ("history_page_warm", "history_page_cold_cpu_cache"):
        counts: dict[int, int] = defaultdict(int)
        for row in rows:
            if row["operation"] == operation:
                counts[int(row["page"])] += 1
        if len(counts) < 1_000 or set(counts.values()) != {30}:
            raise RuntimeError(f"{operation} needs 30 samples for every page (at least 1,000 pages)")
        pages_by_operation[operation] = counts
    if pages_by_operation["history_page_warm"] != pages_by_operation["history_page_cold_cpu_cache"]:
        raise RuntimeError("warm and cold page sample inventories differ")
    return rows


def summarize(
    rows: list[dict[str, int | str]], peak_rss: int
) -> tuple[dict[str, Any], list[dict[str, int | str]]]:
    summary: dict[str, Any] = {}
    page_summary: list[dict[str, int | str]] = []
    for operation in sorted({str(row["operation"]) for row in rows}):
        selected = [row for row in rows if row["operation"] == operation]
        latencies = [int(row["elapsed_ns"]) for row in selected]
        counts = [int(row["result_count"]) for row in selected]
        summary[operation] = {
            "samples": len(selected),
            "pages": len({int(row["page"]) for row in selected}) if operation.startswith("history_") else None,
            "result_count_min": min(counts),
            "result_count_max": max(counts),
            "latency_ns": {
                "p50": nearest_rank(latencies, 0.50),
                "p95": nearest_rank(latencies, 0.95),
                "p99": nearest_rank(latencies, 0.99),
                "minimum": min(latencies),
                "maximum": max(latencies),
            },
            "peak_working_set_bytes": peak_rss,
            "cache_state": (
                "CPU cache evicted with a 64 MiB memory-resident buffer; file/OS cache left intact"
                if operation.endswith("cold_cpu_cache")
                else "warm process and memory-resident corpus"
            ),
        }
        if operation.startswith("history_"):
            by_page: dict[int, list[int]] = defaultdict(list)
            for row in selected:
                by_page[int(row["page"])].append(int(row["elapsed_ns"]))
            for page, page_latencies in sorted(by_page.items()):
                page_summary.append(
                    {
                        "operation": operation,
                        "page": page,
                        "samples": len(page_latencies),
                        "p50_ns": nearest_rank(page_latencies, 0.50),
                        "p95_ns": nearest_rank(page_latencies, 0.95),
                        "p99_ns": nearest_rank(page_latencies, 0.99),
                        "minimum_ns": min(page_latencies),
                        "maximum_ns": max(page_latencies),
                    }
                )
    return summary, page_summary


def run_probe(corpus_dir: Path, output_dir: Path) -> tuple[str, int]:
    cargo = executable("cargo", ".exe")
    build = subprocess.run(
        [
            str(cargo),
            "test",
            "--locked",
            "--release",
            "-p",
            "worlddb-core",
            "--test",
            "m6_14b_windows_bench",
            "--no-run",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if build.returncode:
        raise RuntimeError(build.stderr[-6000:] or "Rust benchmark build failed")
    matches = re.findall(
        r"Executable .+? \(([^)]+\.exe)\)", f"{build.stdout}\n{build.stderr}"
    )
    if not matches:
        raise RuntimeError("Cargo did not report the M6-14b benchmark executable")
    binary = Path(matches[-1])
    if not binary.is_absolute():
        binary = ROOT / binary
    if not binary.is_file():
        raise RuntimeError(f"benchmark executable was not found: {binary}")

    environment = os.environ.copy()
    environment["WORLDDB_M6_14A_CORPUS_DIR"] = str(corpus_dir)
    environment["WORLDDB_M6_14B_OUTPUT_DIR"] = str(output_dir)
    process = subprocess.Popen(
        [
            str(binary),
            "--ignored",
            "--exact",
            "m6_14b_windows_full_corpus_query_and_history_probe",
            "--nocapture",
            "--test-threads=1",
        ],
        cwd=ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    try:
        peak_rss = peak_working_set(process)
        stdout, stderr = process.communicate()
    except BaseException:
        process.kill()
        process.communicate()
        raise
    if process.returncode:
        raise RuntimeError(stderr[-6000:] or stdout[-6000:] or f"probe exited with {process.returncode}")
    raw_path = output_dir / RAW_NAME
    if not raw_path.is_file():
        raise RuntimeError("Rust benchmark did not write its raw sample file")
    return stdout.strip(), peak_rss


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--corpus-dir", required=True, type=Path)
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("the M6-14b query/history probe only runs on Windows")

    local_app_data = Path(os.environ["LOCALAPPDATA"]).resolve()
    output_dir = args.output_dir.resolve()
    if not output_dir.is_relative_to(local_app_data):
        parser.error("probe output must be under LOCALAPPDATA")
    if any(part.casefold().startswith("onedrive") for part in output_dir.parts):
        parser.error("probe output must not be inside OneDrive")
    if output_dir.exists() and any(output_dir.iterdir()):
        parser.error(f"output directory must be absent or empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)

    corpus_dir = args.corpus_dir.resolve()
    manifest_path = (
        args.manifest.resolve()
        if args.manifest
        else ROOT / "crates" / "worlddb-testkit" / "testdata" / "m6-14a" / "manifest.json"
    )
    manifest = verify(manifest_path, corpus_dir)
    hardware = local_hardware_profile()
    rustc = executable("rustc", ".exe")
    rustc_version = subprocess.run(
        [str(rustc), "--version"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    ).stdout.strip()

    rust_output, peak_rss = run_probe(corpus_dir, output_dir)
    rows = validate_rows(output_dir / RAW_NAME)
    summary, per_page = summarize(rows, peak_rss)
    raw_csv = output_dir / "windows-query-history.csv"
    with (output_dir / RAW_NAME).open(newline="", encoding="utf-8") as source:
        raw_rows = list(csv.DictReader(source))
    with raw_csv.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=(*FIELDS, "peak_rss_bytes"))
        writer.writeheader()
        for row in raw_rows:
            writer.writerow({**row, "peak_rss_bytes": peak_rss})

    page_csv = output_dir / "windows-query-history-page-summary.csv"
    with page_csv.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(
            stream,
            fieldnames=("operation", "page", "samples", "p50_ns", "p95_ns", "p99_ns", "minimum_ns", "maximum_ns"),
        )
        writer.writeheader()
        writer.writerows(per_page)

    report = {
        "schema_version": 1,
        "probe": "m6-14b-windows-query-history-v1",
        "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host": hardware,
        "rustc": rustc_version,
        "corpus_version": manifest["corpus_version"],
        "corpus_seed": manifest["seed"],
        "corpus_dataset_sha256": manifest["dataset_sha256"],
        "assertions_loaded": manifest["workload"]["assertions"],
        "selected_history_space": 100,
        "revision_mapping": "source base revision i*1000 maps to compact published revision i-1; each space's assertions publish at i",
        "point_cold_cache_note": "Cold label denotes eviction of CPU cache with a touched 64 MiB buffer; the OS/file cache was not evicted.",
        "history_cold_cache_note": "Each page sample starts after CPU-cache eviction; corpus and source index remain resident and Windows file cache is untouched.",
        "peak_rss_sampling": "Windows PROCESS_MEMORY_COUNTERS_EX.PeakWorkingSetSize on the benchmark test process",
        "peak_working_set_bytes": peak_rss,
        "summary": summary,
        "raw_csv": str(raw_csv),
        "per_page_csv": str(page_csv),
        "rust_output": rust_output,
        "platform_scope": "Windows only; Linux/macOS deferred by user instruction",
    }
    summary_path = output_dir / "windows-query-history-summary.json"
    summary_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    artifact_dir = ROOT / "docs" / "perf" / "M6-14b"
    artifact_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(raw_csv, artifact_dir / raw_csv.name)
    shutil.copy2(page_csv, artifact_dir / page_csv.name)
    shutil.copy2(summary_path, artifact_dir / summary_path.name)
    print(json.dumps(summary, indent=2, sort_keys=True))
    print(f"raw_samples={artifact_dir / raw_csv.name}")
    print(f"per_page_summary={artifact_dir / page_csv.name}")
    print(f"summary={artifact_dir / summary_path.name}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"windows_query_history_probe: {error}", file=sys.stderr)
        raise SystemExit(2) from error
