"""Generate and hash the versioned M6-14a workload corpus off-repository."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SEED = "0x574f524c44444232"
EXPECTED = {
    "entities.bin": (16, {"full": 100_000, "smoke": 256}),
    "assertions.bin": (48, {"full": 1_000_000, "smoke": 2_048}),
    "masks.bin": (16, None),
    "events.bin": (40, {"full": 10_000, "smoke": 64}),
    "history_spaces.bin": (16, {"full": 100, "smoke": 12}),
    "provenance_edges.bin": (36, {"full": 10_000_000, "smoke": 4_096}),
}
SCHEMA = {
    "byte_order": "little-endian",
    "entities.bin": ["id:u64", "kind:u32", "home_history_space:u16", "flags:u16"],
    "assertions.bin": [
        "id:u64",
        "entity_id:u32",
        "predicate_id:u32",
        "history_space_id:u16",
        "layer_id:u8",
        "polarity:u8",
        "value_id:u32",
        "valid_start:i64",
        "valid_end:i64",
        "flags:u32 (bit 0 means masked)",
        "reserved:u32",
    ],
    "masks.bin": ["id:u64", "assertion_id:u64"],
    "events.bin": [
        "id:u64",
        "event_kind:u16",
        "participant_role:u16",
        "participant_entity:u32",
        "history_space_id:u16",
        "timeline_id:u16",
        "start:i64",
        "end:i64",
        "flags:u32 (bit 0 means span)",
    ],
    "history_spaces.bin": [
        "id:u16",
        "parent_id:u16 (65535 means no parent)",
        "base_revision:u64",
        "write_skew:u8",
        "layer_count:u8",
        "reserved:u16",
    ],
    "provenance_edges.bin": [
        "id:u64",
        "from_record_kind:u16",
        "from_record_ordinal:u32",
        "to_record_kind:u16",
        "to_record_ordinal:u32",
        "relation:u16",
        "history_space_id:u16",
        "revision:u64",
        "flags:u32 (bit 0 means retracted)",
    ],
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def local_hardware_profile() -> dict[str, Any]:
    if os.name != "nt":
        raise RuntimeError("M6-14a is being generated on the current Windows host")
    powershell = shutil.which("powershell.exe") or shutil.which("powershell")
    if not powershell:
        raise RuntimeError("PowerShell is required to capture the Windows hardware profile")
    script = r"""
$ErrorActionPreference = 'Stop'
$computer = Get-CimInstance Win32_ComputerSystem
$operatingSystem = Get-CimInstance Win32_OperatingSystem
$processors = @(Get-CimInstance Win32_Processor)
$letter = $env:LOCALAPPDATA.Substring(0, 1)
$volume = Get-Volume -DriveLetter $letter -ErrorAction Stop
$partition = Get-Partition -DriveLetter $letter | Select-Object -First 1
$disk = Get-Disk -Number $partition.DiskNumber -ErrorAction Stop
$profile = [ordered]@{
  os_caption = [string]$operatingSystem.Caption
  os_version = [string]$operatingSystem.Version
  os_build = [string]$operatingSystem.BuildNumber
  cpu_models = [string](@($processors | Select-Object -ExpandProperty Name | Sort-Object -Unique) -join '; ')
  cpu_packages = [int]$processors.Count
  physical_cores = [int](($processors | Measure-Object -Property NumberOfCores -Sum).Sum)
  logical_processors = [int](($processors | Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum)
  total_memory_bytes = [uint64]$computer.TotalPhysicalMemory
  drive_letter = [string]("$letter`:")
  file_system = [string]$volume.FileSystem
  drive_type = [string]$volume.DriveType
  storage_model = [string]$disk.FriendlyName
  storage_bus = [string]$disk.BusType
  storage_media = [string]$disk.MediaType
  storage_size_bytes = [uint64]$disk.Size
  volume_free_bytes = [uint64]$volume.SizeRemaining
  output_location = 'LOCALAPPDATA, outside the repository and OneDrive path'
}
$profile | ConvertTo-Json -Compress -Depth 4
"""
    result = subprocess.run(
        [powershell, "-NoProfile", "-NonInteractive", "-Command", script],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    if not lines:
        raise RuntimeError("PowerShell returned no hardware profile")
    profile = json.loads(lines[-1])
    if profile.get("file_system", "").upper() != "NTFS" or profile.get("drive_type", "").lower() != "fixed":
        raise RuntimeError("corpus output must be on a fixed NTFS volume")
    return profile


def executable(name: str, suffix: str) -> Path:
    located = shutil.which(name)
    if located:
        return Path(located)
    fallback = Path.home() / ".cargo" / "bin" / f"{name}{suffix}"
    if fallback.is_file():
        return fallback
    raise RuntimeError(f"cannot locate {name}; install or expose the Rust toolchain")


def normalize_seed(value: str) -> str:
    radix = 16 if value.lower().startswith("0x") else 10
    digits = value[2:] if radix == 16 else value
    parsed = int(digits, radix)
    if not 0 <= parsed <= 0xFFFF_FFFF_FFFF_FFFF:
        raise ValueError("seed must be an unsigned 64-bit integer")
    return f"0x{parsed:016x}"


def require_local_output(path: Path) -> Path:
    resolved = path.expanduser().resolve()
    local_app_data = Path(os.environ.get("LOCALAPPDATA", "")).resolve()
    repository = ROOT.resolve()
    if not local_app_data.is_dir() or local_app_data == Path.cwd():
        raise RuntimeError("LOCALAPPDATA is unavailable or invalid")
    try:
        resolved.relative_to(repository)
    except ValueError:
        pass
    else:
        raise RuntimeError("corpus data must not be written into the repository")
    try:
        resolved.relative_to(local_app_data)
    except ValueError as error:
        raise RuntimeError("corpus data must be under the local, unsynced LOCALAPPDATA root") from error
    if any(part.casefold().startswith("onedrive") for part in resolved.parts):
        raise RuntimeError("corpus data path must not be inside OneDrive")
    return resolved


def record_count(name: str, profile: str, mask_count: int) -> int:
    expected = EXPECTED[name][1]
    if expected is None:
        return mask_count
    return expected[profile]


def build_manifest(
    output_dir: Path,
    profile: str,
    seed: str,
    generation_output: str,
    hardware: dict[str, Any],
    rustc_version: str,
    cargo_version: str,
) -> dict[str, Any]:
    summary = dict(
        item.split("=", 1)
        for item in generation_output.strip().split()
        if "=" in item
    )
    if summary.get("corpus_version") != "m6-14a-v1" or summary.get("profile") != profile:
        raise RuntimeError("corpus generator output does not match the requested profile")
    if summary.get("seed", "").lower() != seed.lower():
        raise RuntimeError("corpus generator did not preserve the requested seed")
    mask_count = int(summary.get("masks", "-1"))
    assertion_count = record_count("assertions.bin", profile, mask_count)
    if not 0 <= mask_count <= assertion_count:
        raise RuntimeError("generator returned an invalid mask count")
    if profile == "full" and not 0.19 <= mask_count / assertion_count <= 0.21:
        raise RuntimeError("full corpus mask share is outside the declared 20% ± 1% band")

    entries: list[dict[str, Any]] = []
    for name, (record_size, _) in EXPECTED.items():
        path = output_dir / name
        rows = record_count(name, profile, mask_count)
        byte_count = path.stat().st_size
        if byte_count != rows * record_size:
            raise RuntimeError(
                f"{name} has {byte_count} bytes; expected {rows * record_size}"
            )
        entries.append(
            {
                "id": name.removesuffix(".bin"),
                "file": name,
                "rows": rows,
                "record_size_bytes": record_size,
                "bytes": byte_count,
                "sha256": sha256(path),
            }
        )
    aggregate = hashlib.sha256()
    for entry in entries:
        aggregate.update(entry["file"].encode("ascii"))
        aggregate.update(b"\0")
        aggregate.update(entry["sha256"].encode("ascii"))
        aggregate.update(b"\n")
    manifest: dict[str, Any] = {
        "schema_version": 1,
        "corpus_version": "m6-14a-v1",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "profile": profile,
        "seed": seed.lower(),
        "generator": {
            "name": "worlddb-testkit m6-14a-corpus",
            "version": "1.0",
            "rng": "SplitMix64 v1 with per-file domain separators",
            "rustc": rustc_version.strip(),
            "cargo": cargo_version.strip(),
        },
        "schema": SCHEMA,
        "workload": {
            "history_spaces": 100 if profile == "full" else 12,
            "assertions": assertion_count,
            "entities": record_count("entities.bin", profile, mask_count),
            "events": record_count("events.bin", profile, mask_count),
            "provenance_edges": record_count("provenance_edges.bin", profile, mask_count),
            "mask_records": mask_count,
            "observed_mask_share": mask_count / assertion_count,
            "skew": {
                "assertion_entity": "80% drawn from IDs 1..128; remainder uniform",
                "assertion_predicate": "75% drawn from IDs 1..16; remainder uniform over 1..1024",
                "assertion_value": "85% drawn from IDs 1..256; remainder uniform over 1..16384",
                "event_kind": "75% drawn from IDs 1..4; remainder uniform over 1..256",
                "provenance_endpoint_kinds": "70% assertion, 20% entity, 8% event, 2% mask",
                "history_space_graph": "complete binary parent tree with fixed root and per-space base revision",
            },
        },
        "files": entries,
        "dataset_sha256": aggregate.hexdigest(),
        "hardware_profile": hardware,
    }
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--seed", default=DEFAULT_SEED)
    parser.add_argument("--profile", choices=("full", "smoke"), default="full")
    parser.add_argument("--skip-build", action="store_true", help="reuse the release binary already built")
    args = parser.parse_args()

    output_dir = require_local_output(args.output_dir)
    seed = normalize_seed(args.seed)
    if output_dir.exists() and any(output_dir.iterdir()):
        raise RuntimeError(f"output directory is not empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)
    hardware = local_hardware_profile()
    free_bytes = shutil.disk_usage(output_dir).free
    minimum_free = 2 * 400 * 1024 * 1024
    if free_bytes < minimum_free:
        raise RuntimeError(f"less than 800 MiB is free on the corpus volume: {free_bytes} bytes")

    cargo = executable("cargo", ".exe" if os.name == "nt" else "")
    rustc = executable("rustc", ".exe" if os.name == "nt" else "")
    if not args.skip_build:
        subprocess.run(
            [
                str(cargo),
                "build",
                "--locked",
                "--release",
                "-p",
                "worlddb-testkit",
                "--bin",
                "m6-14a-corpus",
            ],
            cwd=ROOT,
            check=True,
        )
    suffix = ".exe" if os.name == "nt" else ""
    generator = ROOT / "target" / "release" / f"m6-14a-corpus{suffix}"
    run = subprocess.run(
        [str(generator), "--output-dir", str(output_dir), "--seed", seed, "--profile", args.profile],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if run.returncode != 0:
        raise RuntimeError(run.stderr.strip() or f"generator failed with exit status {run.returncode}")
    print(run.stdout.strip())
    manifest = build_manifest(
        output_dir,
        args.profile,
        seed,
        run.stdout,
        hardware,
        subprocess.run([str(rustc), "--version"], check=True, capture_output=True, text=True).stdout,
        subprocess.run([str(cargo), "--version"], check=True, capture_output=True, text=True).stdout,
    )
    manifest_path = output_dir / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=True, indent=2) + "\n", encoding="utf-8")
    print(f"manifest={manifest_path}")
    print(f"dataset_sha256={manifest['dataset_sha256']}")
    for entry in manifest["files"]:
        print(f"sha256:{entry['file']}={entry['sha256']}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"build_corpus: {error}", file=sys.stderr)
        raise SystemExit(2) from error
