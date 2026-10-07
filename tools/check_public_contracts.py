"""Freeze and compare the public WorldDB 1.0 RC contract surfaces."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CONTRACT_VERSION = "1.0.0-rc.2"
BASELINE = ROOT / "contracts/public/v1.0.0-rc.2/manifest.json"
RULES = ROOT / "contracts/public/classification-rules.tsv"
LOGICAL_EXPORT_SEMANTICS = ROOT / "docs/contracts/logical-export-v2.md"

SOURCE_GROUPS: dict[str, tuple[str, ...]] = {
    "rust_api_v1": ("crates/worlddb-core/src/api.rs",),
    "wire_tags": (
        "crates/worlddb-core/src/wire.rs",
        "crates/worlddb-core/src/wire_records.rs",
        "crates/worlddb-core/src/record_refs.rs",
        "crates/worlddb-core/src/audit_wire.rs",
        "policy/record-wire-kinds.tsv",
        "policy/record-ref-wire-tags.tsv",
        "policy/audit-wire-kinds.tsv",
        "policy/audit-wire-values.tsv",
    ),
    "public_errors": (
        "crates/worlddb-core/src/api.rs",
        "crates/worlddb-core/src/errors.rs",
        "crates/worlddb-core/src/wire.rs",
    ),
    "persistent_format": (
        "crates/worlddb-core/src/wire.rs",
        "crates/worlddb-storage-file/src/segment.rs",
        "crates/worlddb-storage-file/src/security_segment.rs",
        "crates/worlddb-storage-file/src/manifest.rs",
    ),
    "logical_export": (
        "crates/worlddb-storage-file/src/logical_export.rs",
        "docs/contracts/logical-export-v2.md",
    ),
    "contract_policy": (
        "tools/check_public_contracts.py",
        "contracts/public/classification-rules.tsv",
        "docs/contracts/public-contracts-v1.0.0-rc.1.md",
        "docs/contracts/public-contracts-v1.0.0-rc.2.md",
    ),
}


class ContractError(ValueError):
    """Invalid source, baseline, or classification registry."""


def read_text(relative_path: str) -> str:
    return (ROOT / relative_path).read_text(encoding="utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_source_bytes(value: bytes) -> str:
    """Fingerprint source text independently of the checkout's line endings."""
    return sha256_bytes(value.replace(b"\r\n", b"\n"))


def source_fingerprints() -> dict[str, dict[str, str]]:
    return {
        surface: {
            path: sha256_source_bytes((ROOT / path).read_bytes())
            for path in paths
        }
        for surface, paths in SOURCE_GROUPS.items()
    }


def parse_uint_constant(source: str, name: str, type_name: str = "u16") -> int:
    match = re.search(
        rf"(?:pub\s+)?const\s+{re.escape(name)}\s*:\s*{re.escape(type_name)}\s*=\s*(\d+)\s*;",
        source,
    )
    if not match:
        raise ContractError(f"could not extract {name} from the public contract source")
    return int(match.group(1))


def parse_bytes_literal(source: str, name: str) -> str:
    match = re.search(
        rf"(?:pub\s+)?const\s+{re.escape(name)}\s*:[^=]+?=\s*\*?b\"([^\"]+)\"\s*;",
        source,
    )
    if not match:
        raise ContractError(f"could not extract byte string {name}")
    return match.group(1)


def parse_numeric_enum(source: str, enum_name: str) -> list[dict[str, Any]]:
    marker = re.search(rf"pub\s+enum\s+{re.escape(enum_name)}\s*\{{", source)
    if not marker:
        raise ContractError(f"could not find public enum {enum_name}")
    close = source.find("\n}", marker.end())
    if close < 0:
        raise ContractError(f"could not find end of public enum {enum_name}")
    body = source[marker.end():close]
    values: list[dict[str, Any]] = []
    for variant, raw_value in re.findall(
        r"^\s*(\w+)\s*=\s*(0x[0-9a-fA-F]+|\d+)\s*,", body, re.MULTILINE
    ):
        values.append({"variant": variant, "tag": int(raw_value, 0)})
    if not values:
        raise ContractError(f"public enum {enum_name} has no explicit numeric values")
    return values


def parse_tsv(relative_path: str) -> list[dict[str, str]]:
    with (ROOT / relative_path).open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if not reader.fieldnames:
            raise ContractError(f"{relative_path} has no TSV header")
        return [dict(row) for row in reader]


def parse_protocol_version(api_source: str) -> dict[str, int]:
    match = re.search(
        r"pub\s+const\s+CURRENT_PROTOCOL\s*:\s*ProtocolVersion\s*=\s*ProtocolVersion::new\((\d+)\s*,\s*(\d+)\)",
        api_source,
    )
    if not match:
        raise ContractError("could not extract api::v1::CURRENT_PROTOCOL")
    return {"major": int(match.group(1)), "minor": int(match.group(2))}


def parse_transport_codes(api_source: str) -> list[str]:
    match = re.search(
        r"pub\s+const\s+PUBLIC_CODES\s*:\s*&\[&str\]\s*=\s*&\[(.*?)\]\s*;",
        api_source,
        re.DOTALL,
    )
    if not match:
        raise ContractError("could not extract api::v1::PUBLIC_CODES")
    values = re.findall(r'"([^"\\]+)"', match.group(1))
    if not values or values != sorted(set(values)):
        raise ContractError("api::v1::PUBLIC_CODES is empty, duplicated, or noncanonical")
    return values


def parse_core_to_public_codes(api_source: str) -> list[dict[str, str]]:
    match = re.search(
        r"const\s+CORE_TO_PUBLIC_CODES\s*:\s*&\[\(&str,\s*&str\)\]\s*=\s*&\[(.*?)\]\s*;",
        api_source,
        re.DOTALL,
    )
    if not match:
        raise ContractError("could not extract CORE_TO_PUBLIC_CODES")
    pairs = re.findall(r'\(\s*"([^"\\]+)"\s*,\s*"([^"\\]+)"\s*,?\s*\)', match.group(1))
    result = [{"core_code": left, "transport_code": right} for left, right in pairs]
    core_codes = [row["core_code"] for row in result]
    if result != sorted(result, key=lambda row: row["core_code"]) or len(core_codes) != len(set(core_codes)):
        raise ContractError("CORE_TO_PUBLIC_CODES is duplicated or not in canonical core-code order")
    return result


def parse_logical_limit(source: str, name: str) -> int:
    match = re.search(
        rf"const\s+{re.escape(name)}\s*:\s*usize\s*=\s*([0-9_\s*]+)\s*;",
        source,
    )
    if not match:
        raise ContractError(f"could not extract logical-export limit {name}")
    expression = match.group(1)
    factors = re.findall(r"[0-9][0-9_]*", expression)
    if not factors or re.sub(r"[0-9_\s*]", "", expression):
        raise ContractError(f"logical-export limit {name} is not a numeric product")
    value = 1
    for factor in factors:
        value *= int(factor.replace("_", ""))
    return value


def collect_current() -> dict[str, Any]:
    api_source = read_text("crates/worlddb-core/src/api.rs")
    wire_source = read_text("crates/worlddb-core/src/wire.rs")
    wire_records_source = read_text("crates/worlddb-core/src/wire_records.rs")
    record_refs_source = read_text("crates/worlddb-core/src/record_refs.rs")
    audit_wire_source = read_text("crates/worlddb-core/src/audit_wire.rs")
    errors_source = read_text("crates/worlddb-core/src/errors.rs")
    segment_source = read_text("crates/worlddb-storage-file/src/segment.rs")
    security_source = read_text("crates/worlddb-storage-file/src/security_segment.rs")
    manifest_source = read_text("crates/worlddb-storage-file/src/manifest.rs")
    export_source = read_text("crates/worlddb-storage-file/src/logical_export.rs")
    export_magic = parse_bytes_literal(export_source, "LOGICAL_EXPORT_MAGIC")
    version_match = re.search(r"\\x([0-9a-fA-F]{2})$", export_magic)
    if not version_match:
        raise ContractError("logical-export magic does not end in a version byte")
    export_version = int(version_match.group(1), 16)
    core_codes = sorted(
        set(re.findall(r'"(WDB-[A-Z0-9-]+)"', errors_source + "\n" + wire_source))
    )
    transport_codes = parse_transport_codes(api_source)
    core_to_transport = parse_core_to_public_codes(api_source)
    mapped_core_codes = {row["core_code"] for row in core_to_transport}
    if mapped_core_codes != set(core_codes):
        missing = sorted(set(core_codes) - mapped_core_codes)
        extra = sorted(mapped_core_codes - set(core_codes))
        raise ContractError(f"core error mapping inventory is incomplete; missing={missing}, extra={extra}")
    unknown_transport_codes = sorted(
        {row["transport_code"] for row in core_to_transport} - set(transport_codes)
    )
    if unknown_transport_codes:
        raise ContractError(f"core error mapping targets undefined transport codes: {unknown_transport_codes}")

    return {
        "schema_version": 1,
        "contract_version": CONTRACT_VERSION,
        "surfaces": {
            "rust_api_v1": {
                "protocol_version": parse_protocol_version(api_source),
            },
            "wire_tags": {
                "value_tags": parse_numeric_enum(wire_source, "ValueTag"),
                "record_kinds": parse_tsv("policy/record-wire-kinds.tsv"),
                "record_ref_tags": parse_tsv("policy/record-ref-wire-tags.tsv"),
                "audit_kinds": parse_tsv("policy/audit-wire-kinds.tsv"),
                "audit_values": parse_tsv("policy/audit-wire-values.tsv"),
                "record_kind_count": len(parse_numeric_enum(wire_records_source, "RecordKind")),
                "record_ref_enum": parse_numeric_enum(record_refs_source, "RecordRefWireTag"),
                "audit_wire_source_sha256": sha256_bytes(audit_wire_source.encode("utf-8")),
            },
            "public_errors": {
                "transport_codes": transport_codes,
                "core_codes": core_codes,
                "core_to_transport": core_to_transport,
            },
            "persistent_format": {
                "frame": {
                    "magic_literal": parse_bytes_literal(wire_source, "FRAME_MAGIC"),
                    "major": parse_uint_constant(wire_source, "FORMAT_MAJOR"),
                    "minor": parse_uint_constant(wire_source, "FORMAT_MINOR"),
                },
                "segment": {
                    "magic_literal": parse_bytes_literal(segment_source, "SEGMENT_MAGIC"),
                    "major": parse_uint_constant(segment_source, "SEGMENT_FORMAT_MAJOR"),
                    "minor": parse_uint_constant(segment_source, "SEGMENT_FORMAT_MINOR"),
                },
                "security_segment": {
                    "major": parse_uint_constant(security_source, "SECURITY_FORMAT_MAJOR"),
                    "minor": parse_uint_constant(security_source, "SECURITY_FORMAT_MINOR"),
                },
                "manifest_magic": parse_bytes_literal(manifest_source, "MANIFEST_MAGIC"),
                "current_pointer_magics": {
                    "v1": parse_bytes_literal(manifest_source, "CURRENT_V1_MAGIC"),
                    "v2": parse_bytes_literal(manifest_source, "CURRENT_V2_MAGIC"),
                },
            },
            "logical_export": {
                "format_version": export_version,
                "magic_literal": export_magic,
                "digest_context_literal": parse_bytes_literal(export_source, "LOGICAL_EXPORT_CONTEXT"),
                "max_bytes": parse_logical_limit(export_source, "LOGICAL_EXPORT_MAX_BYTES"),
                "max_records": parse_logical_limit(export_source, "LOGICAL_EXPORT_MAX_RECORDS"),
                "max_history_spaces": parse_logical_limit(export_source, "LOGICAL_EXPORT_MAX_SPACES"),
                "excluded_storage_classes": parse_numeric_enum(export_source, "LogicalExportStorageClass"),
                "semantics_sha256": sha256_bytes(LOGICAL_EXPORT_SEMANTICS.read_bytes()),
            },
        },
        "source_fingerprints": source_fingerprints(),
    }


def load_rules() -> dict[tuple[str, str], dict[str, str]]:
    rows = parse_tsv("contracts/public/classification-rules.tsv")
    required = {"component", "change_kind", "classification", "version_rule", "rationale"}
    rules: dict[tuple[str, str], dict[str, str]] = {}
    for row in rows:
        if set(row) != required:
            raise ContractError("classification-rules.tsv has an unexpected header")
        if row["classification"] not in {"BREAKING", "ADDITIVE"}:
            raise ContractError(f"invalid classification for {row['component']}:{row['change_kind']}")
        key = (row["component"], row["change_kind"])
        if key in rules:
            raise ContractError(f"duplicate classification rule {key}")
        rules[key] = row
    return rules


def _rule_class(rules: dict[tuple[str, str], dict[str, str]], component: str, kind: str) -> str:
    rule = rules.get((component, kind))
    if not rule:
        raise ContractError(f"no classification rule for {component}:{kind}")
    return rule["classification"]


def _change(
    rules: dict[tuple[str, str], dict[str, str]],
    component: str,
    kind: str,
    item: str,
    old: Any,
    new: Any,
) -> dict[str, Any]:
    rule = rules.get((component, kind))
    if not rule:
        raise ContractError(f"no classification rule for {component}:{kind}")
    return {
        "component": component,
        "item": item,
        "change_kind": kind,
        "classification": rule["classification"],
        "version_rule": rule["version_rule"],
        "rationale": rule["rationale"],
        "old": old,
        "new": new,
    }


def _key_for_row(row: dict[str, Any], key_fields: tuple[str, ...]) -> tuple[str, ...]:
    try:
        return tuple(str(row[field]) for field in key_fields)
    except KeyError as exc:
        raise ContractError(f"wire registry row lacks key field {exc.args[0]}") from exc


def _diff_rows(
    component: str,
    old_rows: list[dict[str, Any]],
    new_rows: list[dict[str, Any]],
    key_fields: tuple[str, ...],
    rules: dict[tuple[str, str], dict[str, str]],
) -> list[dict[str, Any]]:
    old_by_key = {_key_for_row(row, key_fields): row for row in old_rows}
    new_by_key = {_key_for_row(row, key_fields): row for row in new_rows}
    changes: list[dict[str, Any]] = []
    for key in sorted(set(old_by_key) | set(new_by_key)):
        old = old_by_key.get(key)
        new = new_by_key.get(key)
        if old == new:
            continue
        kind = "closed_entry_added" if old is None else "closed_entry_removed" if new is None else "closed_entry_changed"
        changes.append(_change(rules, component, kind, ":".join(key), old, new))
    return changes


def _diff_string_set(
    component: str,
    label: str,
    old_values: list[str],
    new_values: list[str],
    rules: dict[tuple[str, str], dict[str, str]],
    add_kind: str,
    remove_kind: str,
) -> list[dict[str, Any]]:
    changes: list[dict[str, Any]] = []
    old_set, new_set = set(old_values), set(new_values)
    for value in sorted(new_set - old_set):
        changes.append(_change(rules, component, add_kind, f"{label}:{value}", None, value))
    for value in sorted(old_set - new_set):
        changes.append(_change(rules, component, remove_kind, f"{label}:{value}", value, None))
    return changes


def _diff_format(
    old: dict[str, Any],
    new: dict[str, Any],
    rules: dict[tuple[str, str], dict[str, str]],
) -> list[dict[str, Any]]:
    changes: list[dict[str, Any]] = []
    for name in ("frame", "segment", "security_segment"):
        previous, current = old[name], new[name]
        if previous["major"] != current["major"]:
            changes.append(_change(rules, "persistent_format", "major_changed", f"{name}.major", previous["major"], current["major"]))
        if previous["minor"] != current["minor"]:
            kind = "minor_increased" if current["minor"] > previous["minor"] else "minor_decreased"
            changes.append(_change(rules, "persistent_format", kind, f"{name}.minor", previous["minor"], current["minor"]))
        if previous.get("magic_literal") != current.get("magic_literal") and previous.get("magic_literal") is not None:
            changes.append(_change(rules, "persistent_format", "magic_changed", f"{name}.magic_literal", previous["magic_literal"], current["magic_literal"]))
    for name in ("manifest_magic", "current_pointer_magics"):
        if old[name] != new[name]:
            changes.append(_change(rules, "persistent_format", "magic_changed", name, old[name], new[name]))
    return changes


def compare_contracts(
    old: dict[str, Any],
    new: dict[str, Any],
    rules: dict[tuple[str, str], dict[str, str]],
) -> list[dict[str, Any]]:
    changes: list[dict[str, Any]] = []
    if old.get("contract_version") != new.get("contract_version"):
        changes.append(_change(
            rules, "contract_policy", "contract_version_changed", "contract_version",
            old.get("contract_version"), new.get("contract_version"),
        ))

    old_api = old["surfaces"]["rust_api_v1"]
    new_api = new["surfaces"]["rust_api_v1"]
    old_protocol, new_protocol = old_api["protocol_version"], new_api["protocol_version"]
    if old_protocol["major"] != new_protocol["major"]:
        changes.append(_change(rules, "rust_api_v1", "protocol_major_changed", "protocol.major", old_protocol["major"], new_protocol["major"]))
    if old_protocol["minor"] != new_protocol["minor"]:
        kind = "protocol_minor_increased" if new_protocol["minor"] > old_protocol["minor"] else "protocol_minor_decreased"
        changes.append(_change(rules, "rust_api_v1", kind, "protocol.minor", old_protocol["minor"], new_protocol["minor"]))

    old_wire, new_wire = old["surfaces"]["wire_tags"], new["surfaces"]["wire_tags"]
    for field, key_fields in (
        ("value_tags", ("tag",)),
        ("record_kinds", ("kind_hex",)),
        ("record_ref_tags", ("tag",)),
        ("audit_kinds", ("kind_hex",)),
        ("audit_values", ("family", "code")),
        ("record_ref_enum", ("tag",)),
    ):
        changes.extend(_diff_rows("wire_tags", old_wire[field], new_wire[field], key_fields, rules))
    if old_wire["record_kind_count"] != new_wire["record_kind_count"]:
        changes.append(_change(rules, "wire_tags", "closed_entry_changed", "RecordKind::ALL count", old_wire["record_kind_count"], new_wire["record_kind_count"]))

    old_errors, new_errors = old["surfaces"]["public_errors"], new["surfaces"]["public_errors"]
    changes.extend(_diff_string_set("public_errors", "transport_code", old_errors["transport_codes"], new_errors["transport_codes"], rules, "code_added", "code_removed"))
    changes.extend(_diff_string_set("public_errors", "core_code", old_errors["core_codes"], new_errors["core_codes"], rules, "code_added", "code_removed"))
    old_map = {row["core_code"]: row["transport_code"] for row in old_errors["core_to_transport"]}
    new_map = {row["core_code"]: row["transport_code"] for row in new_errors["core_to_transport"]}
    for key in sorted(set(old_map) | set(new_map)):
        before, after = old_map.get(key), new_map.get(key)
        if before == after:
            continue
        kind = "code_mapping_added" if before is None else "code_mapping_removed" if after is None else "code_mapping_changed"
        changes.append(_change(rules, "public_errors", kind, key, before, after))

    changes.extend(_diff_format(old["surfaces"]["persistent_format"], new["surfaces"]["persistent_format"], rules))

    old_export, new_export = old["surfaces"]["logical_export"], new["surfaces"]["logical_export"]
    if old_export["format_version"] != new_export["format_version"]:
        changes.append(_change(rules, "logical_export", "version_changed", "LogicalExport format version", old_export["format_version"], new_export["format_version"]))
    for field in ("magic_literal", "digest_context_literal", "max_bytes", "max_records", "max_history_spaces", "excluded_storage_classes"):
        if old_export[field] != new_export[field]:
            changes.append(_change(rules, "logical_export", "metadata_changed", field, old_export[field], new_export[field]))
    if old_export["semantics_sha256"] != new_export["semantics_sha256"]:
        changes.append(_change(rules, "logical_export", "semantics_changed", "LogicalExport v2 semantics", old_export["semantics_sha256"], new_export["semantics_sha256"]))

    has_change = {change["component"] for change in changes}
    old_fingerprints = old.get("source_fingerprints", {})
    new_fingerprints = new.get("source_fingerprints", {})
    for surface in sorted(set(old_fingerprints) | set(new_fingerprints)):
        if old_fingerprints.get(surface) != new_fingerprints.get(surface) and surface not in has_change:
            changes.append(_change(
                rules,
                surface,
                "source_fingerprint_changed",
                "tracked implementation sources",
                old_fingerprints.get(surface),
                new_fingerprints.get(surface),
            ))
    return changes


def write_baseline() -> None:
    if BASELINE.exists():
        raise ContractError(f"refusing to overwrite immutable public-contract baseline: {BASELINE}")
    current = collect_current()
    BASELINE.parent.mkdir(parents=True, exist_ok=True)
    BASELINE.write_text(json.dumps(current, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-baseline", action="store_true", help="create the initial immutable RC baseline; refuses to replace one")
    parser.add_argument("--json", action="store_true", help="emit a machine-readable diff report")
    args = parser.parse_args()
    try:
        rules = load_rules()
        if args.write_baseline:
            write_baseline()
            print(f"PUBLIC CONTRACT BASELINE WRITTEN: {BASELINE.relative_to(ROOT)} ({CONTRACT_VERSION})")
            return 0
        if not BASELINE.exists():
            raise ContractError(f"frozen public-contract baseline is missing: {BASELINE}")
        baseline = json.loads(BASELINE.read_text(encoding="utf-8"))
        current = collect_current()
        changes = compare_contracts(baseline, current, rules)
        report = {
            "baseline_version": baseline.get("contract_version"),
            "current_version": current.get("contract_version"),
            "changes": changes,
            "summary": {
                "breaking": sum(change["classification"] == "BREAKING" for change in changes),
                "additive": sum(change["classification"] == "ADDITIVE" for change in changes),
            },
        }
        if args.json:
            print(json.dumps(report, indent=2, sort_keys=True))
        elif changes:
            for change in changes:
                print(
                    f"{change['classification']}: {change['component']} {change['item']} "
                    f"({change['change_kind']}); {change['version_rule']}"
                )
            print(
                f"PUBLIC CONTRACT DIFF: {report['summary']['breaking']} BREAKING, "
                f"{report['summary']['additive']} ADDITIVE"
            )
        else:
            print(
                f"PUBLIC CONTRACTS OK: {CONTRACT_VERSION}; no differences from the frozen RC baseline; "
                "all tracked public-contract sources and registries match"
            )
        return 1 if changes else 0
    except (ContractError, OSError, json.JSONDecodeError) as exc:
        print(f"PUBLIC CONTRACT CHECK FAILED: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
