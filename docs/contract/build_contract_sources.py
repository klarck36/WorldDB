"""Build or verify provisional contract copies against the immutable source mirror."""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import re
import sys
import tomllib
import zipfile
from pathlib import Path


SOURCE_MANIFEST_REL = Path("docs/source/source_manifest.json")
MASTER_NAME = "WorldDB_Finaler_Vollstaendiger_Plan_vNext.md"
INVARIANTS_NAME = "WorldDB_Invariantenregister_vNext.md"
TOML_NAME = "invariants_vNext.toml"
MATRIX_NAME = "WorldDB_1.0_Invariantenabdeckung.tsv"
ARCHIVE_NAME = "WorldDB_vNext_Lossless_Consolidation_Audit.zip"
CONTRACT_DIR = Path("docs/contract")
GAPS_NAME = "source_gaps.tsv"
ERRATA_NAME = "source-errata.json"
INVARIANT_ID_RE = re.compile(r"WDB-[A-Z]+-\d{3}")
REFERENCE_RE = re.compile(
    r"WDB-([A-Z]+)-(\d{3})((?:\s*(?:[–-]\s*\d{3}|/\s*\d{3}))*)"
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def split_markdown_table_row(line: str) -> list[str]:
    """Split a pipe table row without treating pipes inside code spans as delimiters."""
    row = line.strip()
    if not (row.startswith("|") and row.endswith("|")):
        return []
    row = row[1:-1]
    cells: list[str] = []
    cell: list[str] = []
    code_ticks: int | None = None
    index = 0
    while index < len(row):
        if row[index] == "`":
            end = index
            while end < len(row) and row[end] == "`":
                end += 1
            run = end - index
            if code_ticks is None:
                code_ticks = run
            elif code_ticks == run:
                code_ticks = None
            cell.append(row[index:end])
            index = end
            continue
        if row[index] == "|" and code_ticks is None:
            cells.append("".join(cell).strip())
            cell = []
            index += 1
            continue
        cell.append(row[index])
        index += 1
    cells.append("".join(cell).strip())
    return cells


def markdown_invariants(path: Path) -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        cells = split_markdown_table_row(line)
        if len(cells) < 4 or not INVARIANT_ID_RE.fullmatch(cells[0]):
            continue
        invariant_id = cells[0]
        if invariant_id in result:
            raise ValueError(f"Duplicate ID in source invariant register: {invariant_id}")
        result[invariant_id] = {
            "class": cells[1],
            "statement": cells[2],
            "primary_tests": cells[3],
        }
    return result


def expand_references(text: str) -> list[str]:
    """Expand WDB-FAM-001/003 and WDB-FAM-001–004 forms in source order."""
    result: list[str] = []
    seen: set[str] = set()
    for family, first, suffixes in REFERENCE_RE.findall(text):
        values = [int(first)]
        for separator, token in re.findall(r"(–|-|/)\s*(\d{3})", suffixes):
            end = int(token)
            if separator == "/":
                values.append(end)
            else:
                if end < values[-1]:
                    raise ValueError(f"Descending invariant range in {text!r}")
                values.extend(range(values[-1] + 1, end + 1))
        for value in values:
            invariant_id = f"WDB-{family}-{value:03d}"
            if invariant_id not in seen:
                seen.add(invariant_id)
                result.append(invariant_id)
    return result


def normalized_master_line(master_lines: list[str], line_number: int) -> str:
    if not 1 <= line_number <= len(master_lines):
        raise ValueError(f"Master line out of range: {line_number}")
    line = master_lines[line_number - 1].strip()
    cells = split_markdown_table_row(line)
    return " | ".join(cells) if cells else line


def replace_toml_field(block: str, field: str, value: str) -> str:
    pattern = re.compile(rf"(?m)^{re.escape(field)}\s*=\s*.*$")
    replacement = f"{field} = {value}"
    updated, count = pattern.subn(lambda _: replacement, block, count=1)
    if count != 1:
        raise ValueError(f"Expected one {field!r} field in TOML table")
    return updated


def toml_string(value: str) -> str:
    # JSON basic-string escapes are also valid TOML basic-string escapes.
    return json.dumps(value, ensure_ascii=False)


def toml_string_array(values: list[str]) -> str:
    return "[" + ", ".join(toml_string(value) for value in values) + "]"


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def tsv_bytes(fieldnames: list[str], rows: list[dict[str, str]]) -> bytes:
    output = io.StringIO(newline="")
    writer = csv.DictWriter(
        output,
        fieldnames=fieldnames,
        delimiter="\t",
        lineterminator="\n",
        extrasaction="raise",
    )
    writer.writeheader()
    writer.writerows(rows)
    return output.getvalue().encode("utf-8")


def load_verified_sources(root: Path) -> tuple[dict[str, bytes], dict, bytes]:
    source_manifest = json.loads((root / SOURCE_MANIFEST_REL).read_text(encoding="utf-8"))
    archive_path = root / ARCHIVE_NAME
    archive_bytes = archive_path.read_bytes()
    if sha256(archive_bytes) != source_manifest["archive_sha256"]:
        raise ValueError("Source ZIP SHA-256 differs from M0-01 manifest")
    if len(archive_bytes) != source_manifest["archive_size_bytes"]:
        raise ValueError("Source ZIP size differs from M0-01 manifest")

    source_dir = root / "docs" / "source"
    source_files: dict[str, bytes] = {}
    with zipfile.ZipFile(archive_path) as archive:
        bad_member = archive.testzip()
        if bad_member is not None:
            raise ValueError(f"Source ZIP CRC check failed at {bad_member}")
        names = [item["path"] for item in source_manifest["files"]]
        if archive.namelist() != names:
            raise ValueError("ZIP member list differs from M0-01 manifest")
        for item in source_manifest["files"]:
            name = item["path"]
            original = archive.read(name)
            mirror = (source_dir / name).read_bytes()
            if original != mirror:
                raise ValueError(f"Immutable source mirror differs from ZIP: {name}")
            if sha256(original) != item["sha256"] or len(original) != item["size_bytes"]:
                raise ValueError(f"Source file hash or size differs from manifest: {name}")
            original.decode("utf-8")
            source_files[name] = original
    return source_files, source_manifest, archive_bytes


def build_outputs(root: Path) -> tuple[dict[Path, bytes], dict[str, int]]:
    source_files, source_manifest, _ = load_verified_sources(root)
    matrix_rows = read_tsv(root / MATRIX_NAME)
    matrix = {row["invariant_id"]: row for row in matrix_rows}
    if len(matrix_rows) != 253 or len(matrix) != 253:
        raise ValueError("Invariant TSV must contain exactly 253 unique IDs")

    source_register = markdown_invariants(
        root / "docs" / "source" / INVARIANTS_NAME
    )
    if set(source_register) != set(matrix):
        raise ValueError(
            "Invariant TSV and Markdown register IDs differ: "
            f"markdown-only={sorted(set(source_register) - set(matrix))}, "
            f"tsv-only={sorted(set(matrix) - set(source_register))}"
        )
    for invariant_id, row in matrix.items():
        source = source_register[invariant_id]
        if row["class"] != source["class"]:
            raise ValueError(f"Class mismatch between TSV and Markdown: {invariant_id}")
        if row["source_statement"] != source["statement"]:
            raise ValueError(f"Source statement mismatch between TSV and Markdown: {invariant_id}")

    master_text = source_files[MASTER_NAME].decode("utf-8")
    master_lines = master_text.splitlines()
    source_toml = tomllib.loads(source_files[TOML_NAME].decode("utf-8"))
    invariant_by_id = {row["id"]: row for row in source_toml.get("invariant", [])}
    if len(invariant_by_id) != 253 or set(invariant_by_id) != set(matrix):
        raise ValueError("TOML invariant IDs differ from the 253-ID source matrix")

    original_binding_omissions: list[tuple[str, list[str]]] = []
    original_missing_ids: set[str] = set()
    for binding in source_toml.get("main_rule_binding", []):
        missing = [
            invariant_id
            for invariant_id in expand_references(binding["statement"])
            if invariant_id not in set(binding["invariant_ids"])
        ]
        if missing:
            original_binding_omissions.append((binding["key"], missing))
            original_missing_ids.update(missing)

    for invariant_id, row in matrix.items():
        source = source_register[invariant_id]
        toml_row = invariant_by_id[invariant_id]
        if row["class"] != toml_row["class"] or row["class"] != source["class"]:
            raise ValueError(f"Class mismatch in TOML/Markdown/TSV: {invariant_id}")
        if invariant_id != "WDB-LAY-011" and toml_row["statement"] != row["source_statement"]:
            raise ValueError(f"Unexpected TOML statement mismatch: {invariant_id}")

    contract_files: dict[str, bytes] = dict(source_files)
    toml_text = source_files[TOML_NAME].decode("utf-8")
    sections = list(re.finditer(r"(?m)^\[\[", toml_text))
    boundaries = [match.start() for match in sections] + [len(toml_text)]
    chunks: list[str] = []
    invariant_index = 0
    binding_index = 0
    corrected_layout = False
    binding_changes: list[dict] = []
    line_corrections: list[dict] = []
    array_additions_total = 0

    for start, end in zip(boundaries, boundaries[1:]):
        block = toml_text[start:end]
        header_match = re.match(r"\[\[([^\]]+)\]\]", block)
        if not header_match:
            chunks.append(block)
            continue
        table_name = header_match.group(1)
        if table_name == "invariant":
            rows = source_toml["invariant"]
            if invariant_index >= len(rows):
                raise ValueError("More invariant tables in TOML text than parsed values")
            source_row = rows[invariant_index]
            invariant_index += 1
            invariant_id = source_row["id"]
            if invariant_id == "WDB-LAY-011":
                block = replace_toml_field(
                    block,
                    "statement",
                    toml_string(matrix[invariant_id]["source_statement"]),
                )
                block = replace_toml_field(
                    block,
                    "implementations",
                    toml_string_array([source_register[invariant_id]["primary_tests"]]),
                )
                corrected_layout = True
        elif table_name == "main_rule_binding":
            rows = source_toml["main_rule_binding"]
            if binding_index >= len(rows):
                raise ValueError("More main_rule_binding tables in TOML text than parsed values")
            source_row = rows[binding_index]
            binding_index += 1
            key = source_row["key"]
            key_match = re.fullmatch(r"MAIN-L(\d+)", key)
            if not key_match:
                raise ValueError(f"Malformed main-rule key: {key}")
            line_number = int(key_match.group(1))
            master_statement = normalized_master_line(master_lines, line_number)
            if source_row["statement"] != master_statement:
                line_corrections.append(
                    {
                        "key": key,
                        "master_line": line_number,
                        "original_statement": source_row["statement"],
                        "corrected_statement": master_statement,
                    }
                )
            references = expand_references(master_statement)
            if not references:
                raise ValueError(f"No WDB references found on bound source line {key}")
            unknown = sorted(set(references) - set(matrix))
            if unknown:
                raise ValueError(f"Unknown invariant IDs on {key}: {unknown}")
            current_ids = source_row["invariant_ids"]
            if len(current_ids) != len(set(current_ids)):
                raise ValueError(f"Duplicate invariant IDs on {key}")
            extras = sorted(set(current_ids) - set(references))
            if extras:
                raise ValueError(f"Binding array has IDs absent from master line {key}: {extras}")
            retained_order = [item for item in references if item in set(current_ids)]
            if current_ids != retained_order:
                raise ValueError(f"Binding array order differs from source order on {key}")
            added = [item for item in references if item not in set(current_ids)]
            if source_row["statement"] != master_statement or current_ids != references:
                block = replace_toml_field(block, "statement", toml_string(master_statement))
                block = replace_toml_field(block, "invariant_ids", toml_string_array(references))
                binding_changes.append(
                    {
                        "key": key,
                        "master_line": line_number,
                        "added_invariant_ids": added,
                    }
                )
            array_additions_total += len(added)
        chunks.append(block)

    if invariant_index != len(source_toml.get("invariant", [])):
        raise ValueError("Not all invariant TOML tables were visited")
    if binding_index != len(source_toml.get("main_rule_binding", [])):
        raise ValueError("Not all main_rule_binding TOML tables were visited")
    if not corrected_layout:
        raise ValueError("Expected WDB-LAY-011 source-field correction was not applied")
    corrected_toml = "".join(chunks)
    corrected_doc = tomllib.loads(corrected_toml)
    corrected_invariants = {row["id"]: row for row in corrected_doc["invariant"]}
    for invariant_id, row in matrix.items():
        parsed = corrected_invariants[invariant_id]
        if parsed["class"] != row["class"] or parsed["statement"] != row["source_statement"]:
            raise ValueError(f"Corrected TOML invariant still differs from source: {invariant_id}")
        if ", ".join(parsed["implementations"]) != source_register[invariant_id]["primary_tests"]:
            raise ValueError(f"TOML implementation/test field mismatch: {invariant_id}")

    bindings = corrected_doc.get("main_rule_binding", [])
    if len(bindings) != 149 or len({row["key"] for row in bindings}) != 149:
        raise ValueError("Corrected TOML must contain 149 unique main_rule_binding entries")

    anchors: dict[str, list[str]] = {invariant_id: [] for invariant_id in matrix}
    for row in bindings:
        match = re.fullmatch(r"MAIN-L(\d+)", row["key"])
        assert match is not None
        line_number = int(match.group(1))
        master_statement = normalized_master_line(master_lines, line_number)
        if row["statement"] != master_statement:
            raise ValueError(f"Binding text does not exactly match Master line {row['key']}")
        references = expand_references(master_statement)
        if row["invariant_ids"] != references:
            raise ValueError(f"Expanded ID array differs from Master line {row['key']}")
        for invariant_id in references:
            anchors[invariant_id].append(row["key"])

    gap_rows: list[dict[str, str]] = []
    for invariant_id in sorted(matrix):
        row = matrix[invariant_id]
        keys = anchors[invariant_id]
        main_lines = [key.removeprefix("MAIN-L") for key in keys]
        if invariant_id == "WDB-HIS-001":
            note = (
                "Register statement is shorter than Master §§2.1/3.1; preserve it verbatim and "
                "resolve the stronger Genesis/gapless/overflow wording in M0-02a."
            )
        elif keys:
            note = "Direct WDB-ID reference on the listed normalized Master line(s)."
        else:
            note = "No direct WDB-ID reference on any of the 149 bound Master main-text lines."
        gap_rows.append(
            {
                "invariant_id": invariant_id,
                "class": row["class"],
                "source_statement": row["source_statement"],
                "anchor_status": "ANCHORED" if keys else "NORMATIVE_SOURCE_GAP",
                "master_lines": ";".join(main_lines),
                "binding_keys": ";".join(keys),
                "note": note,
            }
        )
    gap_bytes = tsv_bytes(
        [
            "invariant_id",
            "class",
            "source_statement",
            "anchor_status",
            "master_lines",
            "binding_keys",
            "note",
        ],
        gap_rows,
    )

    invariant_ids = set(matrix)
    anchored_ids = {key for key, values in anchors.items() if values}
    gap_ids = invariant_ids - anchored_ids
    if len(anchored_ids) + len(gap_ids) != 253:
        raise ValueError("Anchor and source-gap classification does not cover all 253 IDs")
    gap_class_counts = {
        class_name: sum(
            1
            for row in gap_rows
            if row["class"] == class_name and row["anchor_status"] == "NORMATIVE_SOURCE_GAP"
        )
        for class_name in ("HARD", "GUARDED")
    }

    contract_files[TOML_NAME] = corrected_toml.encode("utf-8")
    output_files: dict[Path, bytes] = {
        CONTRACT_DIR / name: payload for name, payload in contract_files.items()
    }
    output_files[CONTRACT_DIR / GAPS_NAME] = gap_bytes

    source_items = []
    for name, original in source_files.items():
        working = contract_files[name]
        source_items.append(
            {
                "working_file": name,
                "source_file": f"docs/source/{name}",
                "source_sha256": sha256(original),
                "working_sha256": sha256(working),
                "byte_identical": original == working,
            }
        )
    errata = {
        "manifest_version": 1,
        "status": "PROVISIONAL",
        "created_date": "2026-09-29",
        "source_archive": {
            "path": ARCHIVE_NAME,
            "sha256": source_manifest["archive_sha256"],
            "source_inventory": "docs/source/source_manifest.json",
            "scope_note": (
                "The original standalone v3.1 specification is not present in the supplied ZIP; "
                "this manifest covers only the supplied sources."
            ),
        },
        "reproduction": "python -X utf8 docs/contract/build_contract_sources.py; python -X utf8 docs/contract/build_contract_sources.py --verify-only",
        "working_copies": source_items,
        "corrections": [
            {
                "id": "ERR-M0-02-LAY-011",
                "file": TOML_NAME,
                "invariant_id": "WDB-LAY-011",
                "change": (
                    "Restored the complete statement from the Markdown invariant register and "
                    "separated its primary-test cell from the split pipe-table text."
                ),
                "authority": [
                    f"docs/source/{INVARIANTS_NAME}",
                    "docs/source/WorldDB_Finaler_Vollstaendiger_Plan_vNext.md §33",
                ],
            },
            {
                "id": "ERR-M0-02-PRV-001",
                "file": TOML_NAME,
                "binding_key": "MAIN-L1162",
                "change": "Replaced stale WDB-PRV-001–010 with the exact Master row WDB-PRV-001–013.",
                "authority": f"docs/source/{MASTER_NAME}:1162",
            },
            {
                "id": "ERR-M0-02-AUD-001",
                "file": TOML_NAME,
                "binding_key": "MAIN-L1173",
                "change": "Replaced stale WDB-AUD-001–011 with the exact Master row WDB-AUD-001–013.",
                "authority": f"docs/source/{MASTER_NAME}:1173",
            },
            {
                "id": "ERR-M0-02-BINDINGS",
                "file": TOML_NAME,
                "change": (
                    "Expanded every slash/range reference on each bound Master line and made "
                    "the per-line invariant_ids array exactly cover that line."
                ),
                "changed_bindings": binding_changes,
                "added_id_references_total": array_additions_total,
            },
        ],
        "reconciliation_with_2026_09_28_plan_audit": {
            "historical_count": {
                "incomplete_binding_rows": 33,
                "missing_id_references": 44,
                "distinct_missing_ids": 43,
            },
            "reproduced_from_immutable_toml": {
                "incomplete_binding_rows": len(original_binding_omissions),
                "missing_id_references": sum(len(values) for _, values in original_binding_omissions),
                "distinct_missing_ids": len(original_missing_ids),
            },
            "note": (
                "The prior audit's 44/43 tally differs from the source-derived expansion recorded here. "
                "The complete per-binding additions are retained in corrections[ERR-M0-02-BINDINGS]; "
                "the dated audit entry is not silently rewritten."
            ),
        },
        "source_discrepancies_for_m0_02a": [
            {
                "invariant_id": "WDB-HIS-001",
                "registry_statement": matrix["WDB-HIS-001"]["source_statement"],
                "master_lines": [73, 235],
                "difference": (
                    "Master §§2.1/3.1 additionally require gapless published revisions, Genesis 0, "
                    "first commit 1, and reserving Revision::MAX for overflow safety."
                ),
                "mechanical_action": "Preserved the registry wording; no semantic strengthening was inferred.",
            }
        ],
        "classification_method": {
            "anchor_rule": (
                "ANCHORED means the invariant ID occurs in at least one of the 149 MAIN-L lines "
                "after exact source-line normalization. NORMATIVE_SOURCE_GAP means no such direct ID anchor exists."
            ),
            "gap_list": f"docs/contract/{GAPS_NAME}",
            "counts": {
                "invariant_ids": 253,
                "main_rule_binding_entries": len(bindings),
                "main_rule_lines_compared": len(bindings),
                "original_binding_rows_with_omissions": len(original_binding_omissions),
                "original_omitted_id_references": sum(
                    len(values) for _, values in original_binding_omissions
                ),
                "original_distinct_omitted_ids_across_bindings": len(original_missing_ids),
                "stale_binding_statements_corrected": len(line_corrections),
                "binding_cells_expanded": len(binding_changes),
                "binding_id_references_added": array_additions_total,
                "anchored_ids": len(anchored_ids),
                "normative_source_gaps": len(gap_ids),
                "normative_source_gaps_by_class": gap_class_counts,
            },
        },
        "verification_limits": [
            "This is a structural source comparison; a text reference is not a product test.",
            "Open normative source gaps require an explicit M0-02a decision.",
            "The Master line index is the immutable line-number basis identified by MAIN-L keys.",
        ],
    }
    output_files[CONTRACT_DIR / ERRATA_NAME] = (
        json.dumps(errata, ensure_ascii=False, indent=2) + "\n"
    ).encode("utf-8")

    expected_names = {path.name for path in output_files}
    unexpected = [
        path.name
        for path in (root / CONTRACT_DIR).iterdir()
        if path.name != Path(__file__).name and path.name not in expected_names
    ]
    if unexpected:
        raise ValueError(f"Unexpected file(s) in docs/contract: {sorted(unexpected)}")

    metrics = {
        "invariant_ids": 253,
        "main_rule_binding_entries": len(bindings),
        "original_binding_rows_with_omissions": len(original_binding_omissions),
        "original_omitted_id_references": sum(
            len(values) for _, values in original_binding_omissions
        ),
        "original_distinct_omitted_ids_across_bindings": len(original_missing_ids),
        "stale_binding_statements_corrected": len(line_corrections),
        "binding_cells_expanded": len(binding_changes),
        "binding_id_references_added": array_additions_total,
        "anchored_ids": len(anchored_ids),
        "normative_source_gaps": len(gap_ids),
        "normative_source_gaps_HARD": gap_class_counts["HARD"],
        "normative_source_gaps_GUARDED": gap_class_counts["GUARDED"],
    }
    return output_files, metrics


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="workspace root (primarily for isolated verification fixtures)",
    )
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="compare generated contract artifacts without writing them",
    )
    args = parser.parse_args()
    root = args.root.resolve()
    outputs, metrics = build_outputs(root)

    if args.verify_only:
        failures = []
        for relative_path, expected in outputs.items():
            actual_path = root / relative_path
            if not actual_path.is_file():
                failures.append(f"missing {relative_path.as_posix()}")
            elif actual_path.read_bytes() != expected:
                failures.append(f"differs from source-derived output: {relative_path.as_posix()}")
        if failures:
            for failure in failures:
                print("ERROR:", failure, file=sys.stderr)
            return 1
        print("CONTRACT SOURCES OK: all provisional copies, errata, and 253-ID classifications reproduce")
    else:
        for relative_path, payload in outputs.items():
            destination = root / relative_path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(payload)
        print("CONTRACT SOURCES BUILT: provisional copies and exact source-derived classifications written")

    print(
        "COUNTS: "
        + ", ".join(f"{key}={value}" for key, value in metrics.items())
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
