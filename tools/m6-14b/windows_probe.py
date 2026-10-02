"""Run the Windows M6-14b small-commit and large-WAL recovery probes."""

from __future__ import annotations

import argparse
import csv
import ctypes
import json
import math
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "m6-14a"))
from build_corpus import executable, local_hardware_profile  # noqa: E402
from verify_corpus import verify  # noqa: E402


FIELDS = ("operation", "iteration", "elapsed_ns", "bytes")
MIB = 1024 * 1024


class PROCESS_MEMORY_COUNTERS_EX(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong),
        ("PageFaultCount", ctypes.c_ulong),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
        ("PrivateUsage", ctypes.c_size_t),
    ]


def peak_working_set(process: subprocess.Popen[str]) -> int:
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    kernel32.OpenProcess.argtypes = (ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong)
    kernel32.OpenProcess.restype = ctypes.c_void_p
    kernel32.CloseHandle.argtypes = (ctypes.c_void_p,)
    psapi.GetProcessMemoryInfo.argtypes = (
        ctypes.c_void_p,
        ctypes.POINTER(PROCESS_MEMORY_COUNTERS_EX),
        ctypes.c_ulong,
    )
    psapi.GetProcessMemoryInfo.restype = ctypes.c_int
    handle = kernel32.OpenProcess(0x0400 | 0x0010, 0, process.pid)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    peak = 0
    try:
        counters = PROCESS_MEMORY_COUNTERS_EX()
        counters.cb = ctypes.sizeof(counters)
        while process.poll() is None:
            if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
                raise ctypes.WinError(ctypes.get_last_error())
            peak = max(peak, int(counters.PeakWorkingSetSize))
            time.sleep(0.01)
        if psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
            peak = max(peak, int(counters.PeakWorkingSetSize))
    finally:
        kernel32.CloseHandle(handle)
    return peak


def run_probe(output_dir: Path, provenance_path: Path) -> tuple[list[dict[str, Any]], int]:
    cargo = executable("cargo", ".exe")
    build = subprocess.run(
        [
            str(cargo),
            "build",
            "--locked",
            "--release",
            "-p",
            "worlddb-storage-file",
            "--example",
            "m6_14b_windows_probe",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if build.returncode:
        raise RuntimeError(build.stderr[-4000:] or "Rust probe build failed")
    binary = ROOT / "target" / "release" / "examples" / "m6_14b_windows_probe.exe"
    process = subprocess.Popen(
        [str(binary), str(output_dir), str(provenance_path)],
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
        raise RuntimeError(stderr[-4000:] or f"probe exited with {process.returncode}")
    rows: list[dict[str, Any]] = []
    reader = csv.DictReader(stdout.splitlines())
    if tuple(reader.fieldnames or ()) != FIELDS:
        raise RuntimeError("probe emitted an unexpected CSV header")
    for row in reader:
        rows.append(
            {
                "operation": row["operation"],
                "iteration": int(row["iteration"]),
                "elapsed_ns": int(row["elapsed_ns"]),
                "bytes": int(row["bytes"]),
            }
        )
    return rows, peak_rss


def nearest_rank(values: list[int] | list[float], fraction: float) -> int | float:
    ordered = sorted(values)
    index = max(0, math.ceil(fraction * len(ordered)) - 1)
    return ordered[index]


def summarize(rows: list[dict[str, Any]], peak_rss: int) -> dict[str, Any]:
    report: dict[str, Any] = {}
    for operation in sorted({row["operation"] for row in rows}):
        selected = [row for row in rows if row["operation"] == operation]
        latencies = [int(row["elapsed_ns"]) for row in selected]
        payload_bytes = int(selected[0]["bytes"])
        rates = [
            payload_bytes / (latency / 1_000_000_000) / MIB
            for latency in latencies
            if operation.startswith("recovery_") and latency > 0
        ]
        report[operation] = {
            "samples": len(selected),
            "payload_bytes": payload_bytes,
            "latency_ns": {
                "p50": nearest_rank(latencies, 0.50),
                "p95": nearest_rank(latencies, 0.95),
                "p99": nearest_rank(latencies, 0.99),
                "minimum": min(latencies),
                "maximum": max(latencies),
            },
            "throughput_mib_s": {
                "p50": statistics.median(rates) if rates else None,
                "p95": nearest_rank(sorted(rates), 0.95) if rates else None,
                "minimum": min(rates) if rates else None,
                "maximum": max(rates) if rates else None,
            },
            "peak_rss_working_set_bytes": peak_rss,
            "cache_state": "warm; Windows file-cache eviction was not forced",
        }
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--corpus-dir", required=True, type=Path)
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("the M6-14b storage probe only runs on Windows")

    output_dir = args.output_dir.resolve()
    local_app_data = Path(os.environ["LOCALAPPDATA"]).resolve()
    if not output_dir.is_relative_to(local_app_data):
        parser.error("output directory must be under LOCALAPPDATA")
    if any(part.casefold().startswith("onedrive") for part in output_dir.parts):
        parser.error("output directory must not be inside OneDrive")
    if output_dir.exists() and any(output_dir.iterdir()):
        parser.error(f"output directory must be absent or empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)

    corpus_dir = args.corpus_dir.resolve()
    default_manifest = ROOT / "crates" / "worlddb-testkit" / "testdata" / "m6-14a" / "manifest.json"
    manifest_path = args.manifest.resolve() if args.manifest else default_manifest
    manifest = verify(manifest_path, corpus_dir)
    provenance = corpus_dir / "provenance_edges.bin"
    profile = local_hardware_profile()
    rustc = executable("rustc", ".exe")
    rustc_version = subprocess.run(
        [str(rustc), "--version"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    ).stdout.strip()

    rows, peak_rss = run_probe(output_dir, provenance)
    for row in rows:
        row["peak_rss_bytes"] = peak_rss
    csv_path = output_dir / "windows-storage-probe.csv"
    with csv_path.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=(*FIELDS, "peak_rss_bytes"))
        writer.writeheader()
        writer.writerows(rows)
    summary = {
        "schema_version": 1,
        "probe": "m6-14b-windows-storage-v1",
        "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host": profile,
        "rustc": rustc_version,
        "corpus_version": manifest["corpus_version"],
        "corpus_seed": manifest["seed"],
        "corpus_dataset_sha256": manifest["dataset_sha256"],
        "peak_rss_sampling": "Windows PROCESS_MEMORY_COUNTERS_EX.PeakWorkingSetSize",
        "summary": summarize(rows, peak_rss),
        "raw_csv": str(csv_path),
        "platform_scope": "Windows only; Linux/macOS deferred",
        "cold_cache_note": "No cold-cache sample is claimed; OS file-cache eviction was not forced.",
    }
    summary_path = output_dir / "windows-storage-probe-summary.json"
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary["summary"], indent=2, sort_keys=True))
    print(f"raw_samples={csv_path}")
    print(f"summary={summary_path}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"windows_probe: {error}", file=sys.stderr)
        raise SystemExit(2) from error
