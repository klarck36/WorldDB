"""Check reviewed dependency inventory, feature conflicts, and core error APIs."""

from __future__ import annotations

import csv
import datetime as dt
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEPENDENCIES = ROOT / "policy" / "dependencies.tsv"
FEATURE_CONFLICTS = ROOT / "policy" / "feature-conflicts.tsv"
DEPENDENCY_FIELDS = (
    "name",
    "version",
    "source",
    "boundary_benefit",
    "maintainer_release_review",
    "license_review",
    "advisory_review",
    "transitive_external_nodes",
    "msrv_review",
    "unsafe_review",
    "build_procmacro_review",
    "default_feature_review",
    "feature_review",
    "exit_plan",
    "update_owner",
    "reviewed_on",
)
CONFLICT_FIELDS = ("crate", "feature_a", "feature_b", "reason")
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
BOXED_ERROR = re.compile(r"\bBox\s*<\s*dyn\b[^>]*\bError\b[^>]*>", re.DOTALL)


def read_tsv(path: Path, fields: tuple[str, ...]) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as stream:
        reader = csv.DictReader(stream, delimiter="\t")
        if tuple(reader.fieldnames or ()) != fields:
            raise ValueError(f"{path.relative_to(ROOT)} has an unexpected header")
        rows: list[dict[str, str]] = []
        for line_number, row in enumerate(reader, start=2):
            if None in row or any(value is None for value in row.values()):
                raise ValueError(
                    f"{path.relative_to(ROOT)}:{line_number} has the wrong number of fields"
                )
            rows.append({key: value.strip() for key, value in row.items()})
        return rows


def package_key(package: dict[str, Any]) -> tuple[str, str, str]:
    return (package["name"], package["version"], package.get("source") or "")


def reachable_external_count(
    package_id: str, adjacency: dict[str, set[str]], external_ids: set[str]
) -> int:
    seen: set[str] = set()
    pending = list(adjacency.get(package_id, set()))
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        pending.extend(adjacency.get(current, set()) - seen)
    return len((seen - {package_id}) & external_ids)


def validate_dependency_register(
    metadata: dict[str, Any], rows: list[dict[str, str]]
) -> list[str]:
    errors: list[str] = []
    workspace_ids = set(metadata.get("workspace_members", []))
    packages = {package["id"]: package for package in metadata.get("packages", [])}
    external_packages = {
        package_id: package
        for package_id, package in packages.items()
        if package_id not in workspace_ids
    }
    for package_id, package in external_packages.items():
        source = package.get("source")
        if source is None:
            errors.append(
                f"{package['name']} {package['version']} is an unregistered local path dependency"
            )
        elif source != CRATES_IO_SOURCE:
            errors.append(
                f"{package['name']} {package['version']} uses unapproved source {source!r}"
            )

    nodes = metadata.get("resolve", {}).get("nodes", [])
    adjacency = {
        node["id"]: {dependency["pkg"] for dependency in node.get("deps", [])}
        for node in nodes
    }
    external_ids = set(external_packages)
    packages_by_key = {package_key(package): package_id for package_id, package in external_packages.items()}
    rows_by_key: dict[tuple[str, str, str], dict[str, str]] = {}
    for row in rows:
        key = (row["name"], row["version"], row["source"])
        if key in rows_by_key:
            errors.append(f"duplicate dependency review row for {key}")
        rows_by_key[key] = row

    missing = set(packages_by_key) - set(rows_by_key)
    stale = set(rows_by_key) - set(packages_by_key)
    for key in sorted(missing):
        errors.append(f"dependency review is missing for {key[0]} {key[1]} ({key[2]})")
    for key in sorted(stale):
        errors.append(f"dependency review no longer matches the lockfile: {key[0]} {key[1]} ({key[2]})")

    build_dependency_ids: set[str] = set()
    for node in nodes:
        for dependency in node.get("deps", []):
            kinds = dependency.get("dep_kinds", [])
            if any(kind.get("kind") == "build" for kind in kinds):
                build_dependency_ids.add(dependency["pkg"])

    required_fields = DEPENDENCY_FIELDS[3:]
    for key, row in rows_by_key.items():
        if key not in packages_by_key:
            continue
        package_id = packages_by_key[key]
        package = external_packages[package_id]
        for field in required_fields:
            if not row[field]:
                errors.append(f"{key[0]} {key[1]} review field {field!r} is empty")
        if row["transitive_external_nodes"]:
            try:
                recorded_count = int(row["transitive_external_nodes"])
            except ValueError:
                errors.append(f"{key[0]} {key[1]} transitive_external_nodes must be an integer")
            else:
                actual_count = reachable_external_count(package_id, adjacency, external_ids)
                if recorded_count != actual_count:
                    errors.append(
                        f"{key[0]} {key[1]} transitive_external_nodes is {recorded_count}; "
                        f"lockfile graph has {actual_count}"
                    )
        declared_license = package.get("license")
        if declared_license and declared_license not in row["license_review"]:
            errors.append(
                f"{key[0]} {key[1]} license_review must record the declared SPDX expression "
                f"{declared_license!r}"
            )
        if row["reviewed_on"]:
            try:
                reviewed_on = dt.date.fromisoformat(row["reviewed_on"])
            except ValueError:
                errors.append(f"{key[0]} {key[1]} reviewed_on must be an ISO date")
            else:
                if reviewed_on > dt.date.today():
                    errors.append(f"{key[0]} {key[1]} reviewed_on is in the future")

        compile_time_kinds = {
            kind
            for target in package.get("targets", [])
            for kind in target.get("kind", [])
            if kind in {"custom-build", "proc-macro"}
        }
        if package_id in build_dependency_ids:
            compile_time_kinds.add("build-dependency")
        build_review = row["build_procmacro_review"].strip().lower()
        if compile_time_kinds and not build_review.startswith("reviewed:"):
            errors.append(
                f"{key[0]} {key[1]} executes at build time ({', '.join(sorted(compile_time_kinds))}); "
                "build_procmacro_review must start with 'reviewed:'"
            )
        elif not compile_time_kinds and build_review not in {"none", "not-applicable"}:
            if not build_review.startswith("reviewed:"):
                errors.append(
                    f"{key[0]} {key[1]} build_procmacro_review must be 'none', 'not-applicable', "
                    "or start with 'reviewed:'"
                )

        project_msrs = {
            package.get("rust_version")
            for package_id, package in packages.items()
            if package_id in workspace_ids and package.get("rust_version")
        }
        crate_msrv = package.get("rust_version")
        if project_msrs and crate_msrv:
            project_msrv = sorted(project_msrs)[0]
            if parse_rust_version(crate_msrv) > parse_rust_version(project_msrv):
                errors.append(
                    f"{key[0]} {key[1]} declares rust-version {crate_msrv}, above workspace MSRV {project_msrv}"
                )
    return errors


def parse_rust_version(value: str) -> tuple[int, int, int]:
    match = re.match(r"^(\d+)\.(\d+)(?:\.(\d+))?", value)
    if match is None:
        raise ValueError(f"invalid Rust version {value!r}")
    return (int(match.group(1)), int(match.group(2)), int(match.group(3) or 0))


def validate_feature_conflicts(
    metadata: dict[str, Any], rows: list[dict[str, str]]
) -> list[str]:
    errors: list[str] = []
    packages_by_name: dict[str, list[dict[str, Any]]] = {}
    for package in metadata.get("packages", []):
        packages_by_name.setdefault(package["name"], []).append(package)
    nodes_by_id = {
        node["id"]: set(node.get("features", []))
        for node in metadata.get("resolve", {}).get("nodes", [])
    }
    seen: set[tuple[str, str, str]] = set()
    for row in rows:
        crate = row["crate"]
        feature_a = row["feature_a"]
        feature_b = row["feature_b"]
        reason = row["reason"]
        if not crate or not feature_a or not feature_b or not reason:
            errors.append("feature conflict row has an empty field")
            continue
        key = (crate, *sorted((feature_a, feature_b)))
        if key in seen:
            errors.append(f"duplicate feature conflict row for {crate}: {feature_a} + {feature_b}")
            continue
        seen.add(key)
        matches = packages_by_name.get(crate, [])
        if not matches:
            errors.append(f"feature conflict refers to unknown crate {crate!r}")
            continue
        for package in matches:
            declared = set(package.get("features", {}))
            for feature in (feature_a, feature_b):
                if feature not in declared:
                    errors.append(f"{crate} does not declare feature {feature!r}")
            if not {feature_a, feature_b} <= declared:
                continue
            enabled = nodes_by_id.get(package["id"], set())
            if {feature_a, feature_b} <= enabled:
                errors.append(
                    f"{crate} enables forbidden feature combination {feature_a} + {feature_b}: {reason}"
                )
    return errors


def core_error_erasure_findings(sources: dict[str, str]) -> list[str]:
    return [
        name
        for name, source in sorted(sources.items())
        if BOXED_ERROR.search(source)
    ]


def load_metadata() -> dict[str, Any]:
    completed = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(ROOT / "Cargo.toml"),
            "--locked",
            "--format-version",
            "1",
            "--all-features",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def main() -> int:
    try:
        metadata = load_metadata()
        dependency_rows = read_tsv(DEPENDENCIES, DEPENDENCY_FIELDS)
        conflict_rows = read_tsv(FEATURE_CONFLICTS, CONFLICT_FIELDS)
        errors = validate_dependency_register(metadata, dependency_rows)
        errors.extend(validate_feature_conflicts(metadata, conflict_rows))
        core_sources = {
            str(path.relative_to(ROOT)): path.read_text(encoding="utf-8")
            for path in sorted((ROOT / "crates" / "worlddb-core" / "src").rglob("*.rs"))
        }
        findings = core_error_erasure_findings(core_sources)
        errors.extend(
            f"core source exposes universal boxed Error erasure: {path}"
            for path in findings
        )
    except (OSError, ValueError, csv.Error, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"DEPENDENCY POLICY ERROR: {error}", file=sys.stderr)
        return 1

    if errors:
        for error in errors:
            print(f"DEPENDENCY POLICY ERROR: {error}", file=sys.stderr)
        return 1

    external_count = sum(
        package_id not in set(metadata.get("workspace_members", []))
        for package_id, package in (
            (package["id"], package) for package in metadata.get("packages", [])
        )
    )
    print(
        "DEPENDENCY POLICY OK: "
        f"{external_count} locked external package(s), "
        f"{len(conflict_rows)} forbidden feature pair(s), "
        f"{len(core_sources)} core Rust source file(s) checked"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
