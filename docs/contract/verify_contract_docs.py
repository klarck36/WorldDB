"""Standalone structural verifier for the provisional WorldDB contract docs."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path


MASTER_NAME = "WorldDB_Finaler_Vollstaendiger_Plan_vNext.md"
TOML_NAME = "invariants_vNext.toml"
REQUIRED_FIELDS = {
    "name",
    "id_type",
    "main_section",
    "invariant_ids",
    "wire",
    "storage",
    "lifecycle",
    "security",
    "eligibility",
}
EXPECTED_HEADER = [
    "First-Class Type",
    "ID Type",
    "Main Section",
    "Invariant IDs",
    "Wire",
    "Storage",
    "Lifecycle",
    "Security",
    "Evidence/Provenance Eligibility",
]
ID_PATTERN = re.compile(r"WDB-[A-Z]+-\d{3}")
REFERENCE_PATTERN = re.compile(
    r"WDB-([A-Z]+)-(\d{3})((?:\s*(?:[–-]\s*\d{3}|/\s*\d{3}))*)"
)


def split_table_row(line: str) -> list[str]:
    row = line.strip()
    if not (row.startswith("|") and row.endswith("|")):
        return []
    cells: list[str] = []
    cell: list[str] = []
    code_ticks: int | None = None
    index = 1
    while index < len(row) - 1:
        if row[index] == "`":
            end = index
            while end < len(row) - 1 and row[end] == "`":
                end += 1
            run = end - index
            if code_ticks is None:
                code_ticks = run
            elif code_ticks == run:
                code_ticks = None
            cell.append(row[index:end])
            index = end
        elif row[index] == "|" and code_ticks is None:
            cells.append("".join(cell).strip())
            cell = []
            index += 1
        else:
            cell.append(row[index])
            index += 1
    cells.append("".join(cell).strip())
    return cells


def expand_ids(value: str) -> list[str]:
    result: list[str] = []
    for family, first, suffixes in REFERENCE_PATTERN.findall(value):
        values = [int(first)]
        for separator, token in re.findall(r"(–|-|/)\s*(\d{3})", suffixes):
            end = int(token)
            if separator == "/":
                values.append(end)
            else:
                if end < values[-1]:
                    raise ValueError(f"descending invariant range in {value!r}")
                values.extend(range(values[-1] + 1, end + 1))
        result.extend(f"WDB-{family}-{number:03d}" for number in values)
    return result


def master_type_rows(master_path: Path) -> list[dict[str, object]]:
    lines = master_path.read_text(encoding="utf-8").splitlines()
    heading = next(
        (index for index, line in enumerate(lines) if line.strip() == "## 33. First-Class-Strukturregister"),
        None,
    )
    if heading is None:
        raise ValueError("Master §33 heading is missing")
    header_index = next(
        (index for index in range(heading + 1, len(lines)) if lines[index].strip().startswith("|")),
        None,
    )
    if header_index is None or split_table_row(lines[header_index]) != EXPECTED_HEADER:
        raise ValueError("Master §33 header differs from the expected nine fields")

    rows: list[dict[str, object]] = []
    for line in lines[header_index + 1 :]:
        if not line.strip().startswith("|"):
            break
        cells = split_table_row(line)
        if cells and all(re.fullmatch(r":?-{3,}:?", cell) for cell in cells):
            continue
        if len(cells) != len(EXPECTED_HEADER):
            raise ValueError(f"Master §33 row has {len(cells)} cells; expected 9")
        if not all(cell for cell in cells):
            raise ValueError(f"Master §33 has an empty value in type row {cells[0]!r}")
        rows.append(
            {
                "name": cells[0],
                "id_type": cells[1],
                "main_section": cells[2],
                "invariant_ids": expand_ids(cells[3]),
                "wire": cells[4],
                "storage": cells[5],
                "lifecycle": cells[6],
                "security": cells[7],
                "eligibility": cells[8],
            }
        )
    names = [str(row["name"]) for row in rows]
    if len(rows) != 22 or len(set(names)) != len(rows):
        raise ValueError(f"Master §33 must contain 22 uniquely named types; found {len(rows)}")
    return rows


def verify(root: Path) -> int:
    master_path = root / "docs" / "contract" / MASTER_NAME
    toml_path = root / "docs" / "contract" / TOML_NAME
    expected = master_type_rows(master_path)
    data = tomllib.loads(toml_path.read_text(encoding="utf-8"))
    registered = data.get("first_class_type")
    if not isinstance(registered, list):
        raise ValueError("TOML has no [[first_class_type]] records")

    invariant_ids = {
        row.get("id")
        for row in data.get("invariant", [])
        if isinstance(row, dict) and isinstance(row.get("id"), str)
    }
    names: list[str] = []
    for index, record in enumerate(registered, start=1):
        if not isinstance(record, dict):
            raise ValueError(f"first_class_type record {index} is not a TOML table")
        missing = REQUIRED_FIELDS - set(record)
        extra = set(record) - REQUIRED_FIELDS
        if missing or extra:
            raise ValueError(
                f"first_class_type record {index} has missing fields {sorted(missing)} "
                f"or unrecognized fields {sorted(extra)}"
            )
        for field in REQUIRED_FIELDS - {"invariant_ids"}:
            value = record[field]
            if not isinstance(value, str) or not value.strip():
                raise ValueError(f"first_class_type record {index} has blank/non-string {field}")
        ids = record["invariant_ids"]
        if (
            not isinstance(ids, list)
            or not ids
            or any(not isinstance(item, str) or not ID_PATTERN.fullmatch(item) for item in ids)
            or len(ids) != len(set(ids))
        ):
            raise ValueError(f"first_class_type record {index} has invalid invariant_ids")
        unknown = sorted(set(ids) - invariant_ids)
        if unknown:
            raise ValueError(f"first_class_type record {index} refers to unknown IDs {unknown}")
        names.append(record["name"])

    if len(registered) != len(expected):
        raise ValueError(
            f"Master §33 has {len(expected)} types but TOML registers {len(registered)}"
        )
    if len(set(names)) != len(names):
        raise ValueError("TOML first_class_type contains duplicate type names")
    for index, (source_row, register_row) in enumerate(zip(expected, registered), start=1):
        if source_row != register_row:
            raise ValueError(
                f"TOML first_class_type record {index} ({register_row['name']}) "
                "differs from its Master §33 row"
            )
    print(f"DOCS VERIFY OK: {len(registered)} First-Class types; required fields and Master §33 match")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="workspace root (also supports isolated verification fixtures)",
    )
    args = parser.parse_args()
    try:
        return verify(args.root.resolve())
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"DOCS VERIFY ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
