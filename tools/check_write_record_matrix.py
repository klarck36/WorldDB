"""Check that every First-Class type and wire Record variant has a write-matrix row."""

from __future__ import annotations

import csv
import re
import sys
import tomllib
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/contract/invariants_vNext.toml"
MATRIX = ROOT / "policy/write-record-matrix.tsv"
RECORDS = ROOT / "crates/worlddb-core/src/wire_records.rs"


def main() -> int:
    contract = tomllib.loads(CONTRACT.read_text(encoding="utf-8"))
    expected_types = {entry["name"] for entry in contract["first_class_type"]}
    with MATRIX.open(encoding="utf-8-sig", newline="") as source:
        rows = list(csv.DictReader(source, delimiter="\t"))
    if len(rows) != len(expected_types):
        raise ValueError(
            f"matrix has {len(rows)} rows for {len(expected_types)} First-Class types"
        )
    actual_types = [row["first_class_type"] for row in rows]
    if set(actual_types) != expected_types or len(set(actual_types)) != len(actual_types):
        missing = sorted(expected_types - set(actual_types))
        extra = sorted(set(actual_types) - expected_types)
        raise ValueError(f"First-Class matrix mismatch; missing={missing}; extra={extra}")

    for row in rows:
        for field in ("write_surface", "validation_boundary", "atomic_path"):
            if not row[field].strip():
                raise ValueError(f"{row['first_class_type']} has an empty {field}")
        path = ROOT / row["write_surface"]
        if not path.is_file():
            raise ValueError(f"{row['first_class_type']} references missing file {path}")

    record_source = RECORDS.read_text(encoding="utf-8")
    match = re.search(r"pub enum Record\s*\{(.*?)\n\}", record_source, re.S)
    if match is None:
        raise ValueError("could not find the closed Record enum")
    expected_variants = set(re.findall(r"^\s*([A-Za-z][A-Za-z0-9_]*)\(", match[1], re.M))
    listed_variants: list[str] = []
    for row in rows:
        listed_variants.extend(
            variant.strip()
            for variant in row["record_variants"].split(",")
            if variant.strip()
        )
    counts = Counter(listed_variants)
    duplicates = sorted(name for name, count in counts.items() if count != 1)
    actual_variants = set(listed_variants)
    missing_variants = sorted(expected_variants - actual_variants)
    unknown_variants = sorted(actual_variants - expected_variants)
    if duplicates or missing_variants or unknown_variants:
        raise ValueError(
            "Record variant matrix mismatch; "
            f"duplicates={duplicates}; missing={missing_variants}; unknown={unknown_variants}"
        )

    print(
        "WRITE MATRIX OK: "
        f"{len(rows)} First-Class types and {len(expected_variants)} Record variants "
        "have exactly one registered write/validation path"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (KeyError, OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"WRITE MATRIX ERROR: {error}", file=sys.stderr)
        raise SystemExit(1) from error
