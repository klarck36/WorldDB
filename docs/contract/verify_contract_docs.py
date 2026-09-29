"""Standalone structural verifier for the provisional WorldDB contract docs."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path


MASTER_NAME = "WorldDB_Finaler_Vollstaendiger_Plan_vNext.md"
TOML_NAME = "invariants_vNext.toml"
SPEC_NAME = "entity_perspective_contract.md"
ADR_NAME = "ADR-030-entity-perspective.md"
ARCHIVE_TRANSFER_SPEC_NAME = "archive_transfer_contract.md"
ARCHIVE_TRANSFER_ADR_NAME = "ADR-031-archive-transfer.md"
CONSTRAINT_TIME_SPEC_NAME = "constraint_time_contract.md"
CONSTRAINT_TIME_ADR_NAME = "ADR-032-constraint-time.md"
ERRATA_NAME = "source-errata.json"
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
REQUIRED_CONTRACT_CLAUSES = (
    "EntityCatalogEntry {",
    "EntityTypeDefinition {",
    "Assertion.Subject",
    "Value::Entity",
    "EntityRetirementId",
    "RecordRef::EntityRetirement(EntityRetirementId)",
    "PerspectiveDefinitionRevision {",
    "PerspectiveScope::World",
    "PerspectiveRetirementId",
    "RecordRef::PerspectiveRetirement(PerspectiveRetirementId)",
    "`entity.create`",
    "`entity.reference`",
    "`perspective.use`",
    "Master §§3.1/3.2",
    "`LifecycleTargetRef` itself is unchanged",
    "52 HARD gaps in M0-02a remain open",
    "WDB-HIS-001",
)
REQUIRED_ARCHIVE_TRANSFER_CLAUSES = (
    "ArchiveTransition {",
    "ArchiveTargetRef",
    "RecordRef::ArchiveTransition(ArchiveTransitionId)",
    "ArchiveVisibility::Operational",
    "Raw History",
    "HistorySpaceContentRef",
    "selected_event_relations",
    "record_id_map",
    "CopyEffectiveLifecycle",
    "PreserveArchiveState",
    "relation: DerivedFrom",
    "expected_target_head",
    "TransactionConflict",
    "never adds a parent",
    "EventRelation itself is not admitted to generic `ProvenanceEndpointRef`",
    "Master §§3.1/3.2",
)
REQUIRED_CONSTRAINT_TIME_CLAUSES = (
    "ConstraintSet { rules: List<ValueConstraint> }",
    "ValueConstraint =",
    "BoolSet(NonEmptySet<Bool>)",
    "wrong scalar type",
    "Only one rule of each variant may occur in a set",
    "EntityTypeConstraint",
    "DecimalRange",
    "TimelineDefinition {",
    "TimeUnitDefinition {",
    "nanoseconds_per_tick: UInt64NonZero",
    "Time { timeline_id: TimelineId, ticks: i128, unit: Symbol }",
    "IncomparableTimeline",
    "duplicate TimelineId, Timeline Symbol, or TimeUnit Symbol",
    "CalendarPeriod { years: UInt32, months: UInt8, days: UInt32 }",
    "months` is canonical in `0..=11`",
    "EventTimeConstraint { form:",
    "EventKindDefinition.time_constraint.max_calendar_span",
    "EventSpanClosure",
    "CalendarRelativeWindow",
    "[shift(anchor, period, Past), anchor)",
    "Migration dry-run and execution call the same validator and transformer",
    "CalendarPeriodOverflow",
    "1970-01-01T00:00:00Z",
    "start < end",
    "Gregorian leap/month-end arithmetic",
)
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
    spec_path = root / "docs" / "contract" / SPEC_NAME
    adr_path = root / "docs" / "contract" / ADR_NAME
    archive_transfer_spec_path = root / "docs" / "contract" / ARCHIVE_TRANSFER_SPEC_NAME
    archive_transfer_adr_path = root / "docs" / "contract" / ARCHIVE_TRANSFER_ADR_NAME
    constraint_time_spec_path = root / "docs" / "contract" / CONSTRAINT_TIME_SPEC_NAME
    constraint_time_adr_path = root / "docs" / "contract" / CONSTRAINT_TIME_ADR_NAME
    errata_path = root / "docs" / "contract" / ERRATA_NAME
    master_text = master_path.read_text(encoding="utf-8")
    spec_text = spec_path.read_text(encoding="utf-8").strip()
    adr_text = adr_path.read_text(encoding="utf-8")
    archive_transfer_spec_text = archive_transfer_spec_path.read_text(encoding="utf-8").strip()
    archive_transfer_adr_text = archive_transfer_adr_path.read_text(encoding="utf-8")
    constraint_time_spec_text = constraint_time_spec_path.read_text(encoding="utf-8").strip()
    constraint_time_adr_text = constraint_time_adr_path.read_text(encoding="utf-8")
    missing_clauses = [clause for clause in REQUIRED_CONTRACT_CLAUSES if clause not in spec_text]
    if missing_clauses:
        raise ValueError(f"M0-04 supplement is missing required contract clauses: {missing_clauses}")
    missing_transfer_clauses = [
        clause for clause in REQUIRED_ARCHIVE_TRANSFER_CLAUSES if clause not in archive_transfer_spec_text
    ]
    if missing_transfer_clauses:
        raise ValueError(
            f"M0-04a supplement is missing required contract clauses: {missing_transfer_clauses}"
        )
    missing_time_clauses = [
        clause for clause in REQUIRED_CONSTRAINT_TIME_CLAUSES if clause not in constraint_time_spec_text
    ]
    if missing_time_clauses:
        raise ValueError(
            f"M0-04b supplement is missing required contract clauses: {missing_time_clauses}"
        )
    master_normalized = master_text.replace("\r\n", "\n").rstrip()
    expected_suffix = (
        spec_text + "\n\n" + archive_transfer_spec_text + "\n\n" + constraint_time_spec_text
    )
    if not master_normalized.endswith(expected_suffix):
        raise ValueError(
            "Master working copy does not end with the exact M0-04, M0-04a, and M0-04b supplements"
        )
    for heading in (
        "## 2. Entity contract",
        "## 3. Perspective contract",
        "## 4. Authorization and operation boundary",
        "## 5. Implementable commands and required outcomes",
        "## 6. Existing-contract cross-check",
    ):
        if heading not in spec_text:
            raise ValueError(f"M0-04 supplement is missing required section {heading!r}")
    if "**Status:** Accepted for the WorldDB 1.0 working contract" not in adr_text:
        raise ValueError("ADR-030 is not marked accepted")
    if "[Entity/Perspective supplement](entity_perspective_contract.md)" not in adr_text:
        raise ValueError("ADR-030 does not link the normative supplement")
    for heading in (
        "## 1. Archive state and record form",
        "## 2. Operational visibility and raw history",
        "## 3. HistorySpace transfer plan",
        "## 4. Transaction, conflict, and outcome rules",
        "## 5. HistorySpace and invariants cross-check",
    ):
        if heading not in archive_transfer_spec_text:
            raise ValueError(f"M0-04a supplement is missing required section {heading!r}")
    if "**Status:** Accepted for the WorldDB 1.0 working contract" not in archive_transfer_adr_text:
        raise ValueError("ADR-031 is not marked accepted")
    if "[Archive and HistorySpace transfer supplement](archive_transfer_contract.md)" not in archive_transfer_adr_text:
        raise ValueError("ADR-031 does not link the normative supplement")
    for heading in (
        "## 1. Closed `ConstraintSet`",
        "## 2. Project-wide Timeline and TimeUnit registries",
        "## 3. `CalendarPeriod`",
        "### Allowed schema use",
        "### Allowed query use",
        "### Allowed migration use",
        "## 4. Validation, query, and migration agreement",
    ):
        if heading not in constraint_time_spec_text:
            raise ValueError(f"M0-04b supplement is missing required section {heading!r}")
    if "**Status:** Accepted for the WorldDB 1.0 working contract" not in constraint_time_adr_text:
        raise ValueError("ADR-032 is not marked accepted")
    if "[Constraints and time registration supplement](constraint_time_contract.md)" not in constraint_time_adr_text:
        raise ValueError("ADR-032 does not link the normative supplement")

    errata = json.loads(errata_path.read_text(encoding="utf-8"))
    expected_additions = {
        "M0-04": (spec_path, adr_path),
        "M0-04a": (archive_transfer_spec_path, archive_transfer_adr_path),
        "M0-04b": (constraint_time_spec_path, constraint_time_adr_path),
    }
    additions = {item.get("task"): item for item in errata.get("contract_additions", [])}
    for task, (addition_spec, addition_adr) in expected_additions.items():
        addition = additions.get(task)
        if not addition:
            raise ValueError(f"source-errata.json lacks the {task} contract addition")
        if addition.get("specification_sha256") != hashlib.sha256(addition_spec.read_bytes()).hexdigest():
            raise ValueError(f"{task} specification hash differs from source-errata.json")
        if addition.get("decision_record_sha256") != hashlib.sha256(addition_adr.read_bytes()).hexdigest():
            raise ValueError(f"{task} decision record hash differs from source-errata.json")
    correction_ids = {item.get("id") for item in errata.get("corrections", [])}
    for correction_id in (
        "ERR-M0-04-ENTITY-PERSPECTIVE",
        "ERR-M0-04A-ARCHIVE-TRANSFER",
        "ERR-M0-04B-CONSTRAINT-TIME",
    ):
        if correction_id not in correction_ids:
            raise ValueError(f"source-errata.json lacks {correction_id}")

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
    by_name = {str(row["name"]): row for row in expected}
    for name, expected_fields in {
        "Entity": {
            "main_section": "§§2.3, 3, M0-04 supplement",
            "wire": "typed 16-byte EntityId + project catalog entry",
            "storage": "immutable EntityId/EntityTypeId; entity facts are Assertions",
            "lifecycle": "Active → Retired at shared Revision; no retype/delete/reuse",
            "security": "entity action/reference + record/field policy",
        },
        "Perspective": {
            "main_section": "§§2.1.2, 2.3.1, M0-04 supplement",
            "wire": "typed 16-byte PerspectiveId",
            "storage": "revisioned project definition; optional name/description",
            "lifecycle": "Active → Retired at shared Revision",
            "security": "perspective action/use policy; never Principal",
        },
    }.items():
        row = by_name.get(name)
        if row is None or any(row[field] != value for field, value in expected_fields.items()):
            raise ValueError(f"Master §33 {name} row does not reflect the M0-04 contract")
    print(f"DOCS VERIFY OK: {len(registered)} First-Class types; M0-04, M0-04a, and M0-04b contracts match Master §33")
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
