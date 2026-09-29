"""Validate the small, versioned M0-13 test corpus and its content hashes."""

from __future__ import annotations

import csv
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CORPUS_ROOT = ROOT / "crates" / "worlddb-testkit" / "testdata" / "m0-13"
MANIFEST_NAME = "manifest.json"
EXPECTED_FILES = {
    "fuzz-seeds-v1": ("fuzz-seeds", "fuzz-seeds/v1/seeds.tsv"),
    "golden-stream-v1": ("golden", "golden/v1/splitmix64.tsv"),
    "full-scan-v1": ("full-scan", "full-scan/v1/records.tsv"),
}


class CorpusError(ValueError):
    """A corpus manifest or fixture violates the checked-in contract."""


def load_manifest(root: Path) -> dict[str, Any]:
    try:
        value = json.loads((root / MANIFEST_NAME).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CorpusError(f"cannot read corpus manifest: {error}") from error
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise CorpusError("corpus manifest must be an object with schema_version 1")
    if value.get("corpus_version") != "m0-13-v1":
        raise CorpusError("unexpected corpus version")
    entries = value.get("files")
    if not isinstance(entries, list):
        raise CorpusError("corpus manifest files must be an array")
    return value


def validate_corpus(root: Path = CORPUS_ROOT) -> list[dict[str, str]]:
    """Validate exact versioned contents, paths, and SHA-256 hashes."""
    manifest = load_manifest(root)
    entries = manifest["files"]
    by_id: dict[str, dict[str, Any]] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise CorpusError("each corpus file entry must be an object")
        entry_id = entry.get("id")
        if not isinstance(entry_id, str) or not entry_id or entry_id in by_id:
            raise CorpusError("corpus file IDs must be non-empty and unique")
        by_id[entry_id] = entry
    if set(by_id) != set(EXPECTED_FILES):
        raise CorpusError(f"unexpected corpus file IDs: {sorted(by_id)}")

    resolved: list[dict[str, str]] = []
    root_resolved = root.resolve()
    for entry_id, (kind, relative_path) in EXPECTED_FILES.items():
        entry = by_id[entry_id]
        if entry.get("kind") != kind or entry.get("path") != relative_path:
            raise CorpusError(f"{entry_id}: kind or versioned path differs from the contract")
        relative = Path(relative_path)
        if relative.is_absolute() or ".." in relative.parts:
            raise CorpusError(f"{entry_id}: path escapes the corpus root")
        fixture = (root / relative).resolve()
        if root_resolved not in fixture.parents or not fixture.is_file():
            raise CorpusError(f"{entry_id}: fixture is missing or escapes the corpus root")
        actual_hash = hashlib.sha256(fixture.read_bytes()).hexdigest()
        expected_hash = entry.get("sha256")
        if not isinstance(expected_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", expected_hash):
            raise CorpusError(f"{entry_id}: SHA-256 must be 64 lowercase hexadecimal characters")
        if actual_hash != expected_hash:
            raise CorpusError(f"{entry_id}: SHA-256 mismatch")
        resolved.append(
            {
                "id": entry_id,
                "kind": kind,
                "path": str(fixture),
                "sha256": actual_hash,
            }
        )

    _validate_seed_file(root / EXPECTED_FILES["fuzz-seeds-v1"][1])
    _validate_golden_file(root / EXPECTED_FILES["golden-stream-v1"][1])
    _validate_full_scan_file(root / EXPECTED_FILES["full-scan-v1"][1])
    return resolved


def _read_tsv(path: Path, expected_header: tuple[str, ...]) -> list[dict[str, str]]:
    try:
        with path.open(encoding="utf-8", newline="") as stream:
            reader = csv.DictReader(stream, delimiter="\t")
            if tuple(reader.fieldnames or ()) != expected_header:
                raise CorpusError(f"{path.name}: unexpected TSV header")
            rows: list[dict[str, str]] = []
            for line_number, row in enumerate(reader, start=2):
                if None in row or any(value is None for value in row.values()):
                    raise CorpusError(f"{path.name}:{line_number}: wrong number of columns")
                rows.append({key: value.strip() for key, value in row.items()})
            return rows
    except OSError as error:
        raise CorpusError(f"cannot read {path.name}: {error}") from error


def _parse_seed(value: str, context: str) -> int:
    try:
        parsed = int(value, 16 if value.lower().startswith("0x") else 10)
    except ValueError as error:
        raise CorpusError(f"{context}: invalid seed {value!r}") from error
    if not 0 <= parsed < 1 << 64:
        raise CorpusError(f"{context}: seed is outside the u64 range")
    return parsed


def _validate_seed_file(path: Path) -> None:
    rows = _read_tsv(path, ("seed_id", "seed", "note"))
    if len(rows) != 8:
        raise CorpusError(f"{path.name}: expected 8 versioned seeds, found {len(rows)}")
    ids: set[str] = set()
    values: set[int] = set()
    for row in rows:
        seed_id = row["seed_id"]
        seed = _parse_seed(row["seed"], seed_id)
        if not seed_id or not row["note"] or seed_id in ids or seed in values:
            raise CorpusError(f"{path.name}: seed IDs and values must be non-empty and unique")
        ids.add(seed_id)
        values.add(seed)


def _validate_golden_file(path: Path) -> None:
    rows = _read_tsv(path, ("seed", "index", "value"))
    if len(rows) != 4:
        raise CorpusError(f"{path.name}: expected 4 golden vectors, found {len(rows)}")
    for row in rows:
        _parse_seed(row["seed"], path.name)
        if row["index"] != "0" or not re.fullmatch(r"[0-9a-f]{16}", row["value"]):
            raise CorpusError(f"{path.name}: malformed SplitMix64 golden vector")


def _validate_full_scan_file(path: Path) -> None:
    rows = _read_tsv(path, ("sequence", "payload"))
    if len(rows) != 16:
        raise CorpusError(f"{path.name}: expected 16 full-scan rows, found {len(rows)}")
    for expected_number, row in enumerate(rows, start=1):
        expected = f"{expected_number:04d}"
        if row["sequence"] != expected or row["payload"] != f"opaque-{expected}":
            raise CorpusError(f"{path.name}: full-scan row {expected_number} is malformed")


def main() -> int:
    try:
        resolved = validate_corpus()
    except CorpusError as error:
        print(f"TESTKIT CORPUS FAIL: {error}", file=sys.stderr)
        return 1
    print(f"TESTKIT CORPUS PASS: {len(resolved)} versioned files verified")
    for item in resolved:
        print(f"{item['id']} {item['sha256']} {item['path']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
