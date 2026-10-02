"""Verify M6-14a manifest counts, canonical dataset hash, and generated file hashes."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


PROFILE_COUNTS = {
    "full": {
        "entities.bin": (100_000, 16),
        "assertions.bin": (1_000_000, 48),
        "events.bin": (10_000, 40),
        "history_spaces.bin": (100, 16),
        "provenance_edges.bin": (10_000_000, 36),
    },
    "smoke": {
        "entities.bin": (256, 16),
        "assertions.bin": (2_048, 48),
        "events.bin": (64, 40),
        "history_spaces.bin": (12, 16),
        "provenance_edges.bin": (4_096, 36),
    },
}
FIXED_ROWS = {
    "entities.bin": 16,
    "assertions.bin": 48,
    "masks.bin": 16,
    "events.bin": 40,
    "history_spaces.bin": 16,
    "provenance_edges.bin": 36,
}
SCHEMA_FIELDS = {
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


class CorpusError(ValueError):
    """An M6-14a manifest or corpus violates its versioned contract."""


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def verify(
    manifest_path: Path,
    data_root: Path | None,
    allow_smoke: bool = False,
    verify_raw_files: bool = True,
) -> dict[str, Any]:
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CorpusError(f"cannot read manifest: {error}") from error
    if not isinstance(manifest, dict):
        raise CorpusError("manifest must be a JSON object")
    if manifest.get("schema_version") != 1 or manifest.get("corpus_version") != "m6-14a-v1":
        raise CorpusError("unexpected M6-14a manifest version")
    if manifest.get("schema") != SCHEMA_FIELDS:
        raise CorpusError("manifest schema differs from the M6-14a-v1 record contract")
    profile = manifest.get("profile")
    if profile not in PROFILE_COUNTS or (profile == "smoke" and not allow_smoke):
        raise CorpusError("manifest must describe a full corpus; smoke requires --allow-smoke")
    expected_counts = PROFILE_COUNTS[profile]
    seed = manifest.get("seed")
    if not isinstance(seed, str) or not re.fullmatch(r"0x[0-9a-f]{16}", seed):
        raise CorpusError("manifest seed must be a normalized 64-bit hexadecimal value")

    workload = manifest.get("workload")
    if not isinstance(workload, dict):
        raise CorpusError("manifest workload is missing")
    for filename, (rows, record_size) in expected_counts.items():
        entry_rows = workload.get(filename.removesuffix(".bin"))
        if entry_rows != rows:
            raise CorpusError(f"{filename}: expected {rows} rows, found {entry_rows!r}")
        entry = next(
            (item for item in manifest.get("files", []) if isinstance(item, dict) and item.get("file") == filename),
            None,
        )
        if not isinstance(entry, dict) or entry.get("rows") != rows:
            raise CorpusError(f"{filename}: file entry row count is missing or incorrect")
        if entry.get("record_size_bytes") != record_size:
            raise CorpusError(f"{filename}: record size differs from the versioned schema")

    entries = manifest.get("files")
    if not isinstance(entries, list) or len(entries) != len(FIXED_ROWS):
        raise CorpusError("manifest must contain exactly six dataset files")
    files_by_name = {entry.get("file"): entry for entry in entries if isinstance(entry, dict)}
    if set(files_by_name) != set(FIXED_ROWS):
        raise CorpusError("manifest dataset file inventory differs from M6-14a-v1")

    expected_mask_rows = workload.get("mask_records")
    if not isinstance(expected_mask_rows, int) or expected_mask_rows <= 0:
        raise CorpusError("mask_records must be a positive integer")
    assertion_total = expected_counts["assertions.bin"][0]
    if not 0.19 <= expected_mask_rows / assertion_total <= 0.21:
        raise CorpusError("mask share must remain within 20% ± 1%")
    if workload.get("observed_mask_share") != expected_mask_rows / assertion_total:
        raise CorpusError("observed_mask_share differs from the stored mask record count")

    if verify_raw_files and data_root is None:
        raise CorpusError("data_root is required when verifying generated corpus files")
    aggregate = hashlib.sha256()
    root = data_root.resolve() if data_root is not None else None
    for filename, record_size in FIXED_ROWS.items():
        entry = files_by_name[filename]
        rows = expected_mask_rows if filename == "masks.bin" else entry.get("rows")
        if filename != "masks.bin" and rows != expected_counts[filename][0]:
            raise CorpusError(f"{filename}: expected {expected_counts[filename][0]} rows")
        if not isinstance(rows, int) or rows < 0:
            raise CorpusError(f"{filename}: invalid row count")
        expected_bytes = rows * record_size
        if entry.get("bytes") != expected_bytes:
            raise CorpusError(f"{filename}: file size does not match the manifest row count")
        if entry.get("record_size_bytes") != record_size:
            raise CorpusError(f"{filename}: record size is not canonical")
        expected_hash = entry.get("sha256")
        if not isinstance(expected_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", expected_hash):
            raise CorpusError(f"{filename}: SHA-256 is malformed")
        if verify_raw_files:
            relative = Path(filename)
            if relative.is_absolute() or ".." in relative.parts:
                raise CorpusError(f"{filename}: manifest path escapes the data root")
            path = (root / relative).resolve()
            if root not in path.parents or not path.is_file():
                raise CorpusError(f"{filename}: corpus file is missing or escapes its root")
            actual_bytes = path.stat().st_size
            if actual_bytes != expected_bytes:
                raise CorpusError(f"{filename}: file size differs from the manifest")
            if digest_file(path) != expected_hash:
                raise CorpusError(f"{filename}: SHA-256 mismatch")
        aggregate.update(filename.encode("ascii"))
        aggregate.update(b"\0")
        aggregate.update(expected_hash.encode("ascii"))
        aggregate.update(b"\n")

    if manifest.get("dataset_sha256") != aggregate.hexdigest():
        raise CorpusError("aggregate dataset SHA-256 mismatch")
    hardware = manifest.get("hardware_profile")
    if not isinstance(hardware, dict):
        raise CorpusError("hardware profile is missing")
    required_hardware = {
        "os_caption",
        "os_version",
        "os_build",
        "cpu_models",
        "physical_cores",
        "logical_processors",
        "total_memory_bytes",
        "drive_letter",
        "file_system",
        "drive_type",
        "storage_model",
        "volume_free_bytes",
        "output_location",
    }
    if not required_hardware.issubset(hardware):
        raise CorpusError("hardware profile lacks required host or storage fields")
    if hardware["file_system"].upper() != "NTFS" or hardware["drive_type"].lower() != "fixed":
        raise CorpusError("hardware profile does not describe a fixed NTFS volume")
    if "LOCALAPPDATA" not in hardware["output_location"]:
        raise CorpusError("hardware profile does not confirm local unsynchronized output")
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("data_root", nargs="?", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--allow-smoke", action="store_true")
    parser.add_argument("--metadata-only", action="store_true")
    args = parser.parse_args()
    if not args.metadata_only and args.data_root is None:
        parser.error("data_root is required unless --metadata-only is selected")
    default_manifest = (
        args.data_root / "manifest.json"
        if args.data_root is not None
        else Path(__file__).resolve().parents[2]
        / "crates"
        / "worlddb-testkit"
        / "testdata"
        / "m6-14a"
        / "manifest.json"
    )
    manifest_path = args.manifest or default_manifest
    manifest = verify(
        manifest_path,
        args.data_root,
        allow_smoke=args.allow_smoke,
        verify_raw_files=not args.metadata_only,
    )
    print(
        "M6-14a corpus OK: "
        f"seed={manifest['seed']} "
        f"dataset_sha256={manifest['dataset_sha256']} "
        f"files={len(manifest['files'])} "
        f"mask_rows={manifest['workload']['mask_records']}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, CorpusError) as error:
        print(f"verify_corpus: {error}", file=sys.stderr)
        raise SystemExit(2) from error
