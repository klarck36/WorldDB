"""Check the actual initial Cargo workspace dependency direction."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_CRATES = {
    "worlddb-core": set(),
    "worlddb-storage-file": {"worlddb-core"},
    "worlddb-cli": {"worlddb-core", "worlddb-storage-file"},
    "worlddb-testkit": {"worlddb-core"},
    "xtask": set(),
}

# These core dependencies are reviewed in policy/dependencies.tsv and their
# task evidence; rusqlite is the optional M5-20 testkit reference adapter.
EXPECTED_EXTERNALS = {
    "worlddb-core": {"blake3", "getrandom", "uuid"},
    "worlddb-storage-file": {"blake3", "fs4", "getrandom", "windows-sys"},
    "worlddb-cli": set(),
    "worlddb-testkit": {"rusqlite"},
    "xtask": set(),
}


def validate_graph(
    dependencies: dict[str, set[str]], external: dict[str, set[str]] | None = None
) -> list[str]:
    external = external or {}
    errors: list[str] = []
    actual_crates = set(dependencies)
    expected_crates = set(EXPECTED_CRATES)

    if actual_crates != expected_crates:
        errors.append(
            "workspace members differ: "
            f"missing={sorted(expected_crates - actual_crates)}, "
            f"unexpected={sorted(actual_crates - expected_crates)}"
        )

    for crate, actual_dependencies in dependencies.items():
        allowed = EXPECTED_CRATES.get(crate, set())
        forbidden = actual_dependencies - allowed
        if forbidden:
            errors.append(f"{crate} has forbidden workspace dependencies: {sorted(forbidden)}")
        unexpected_external = external.get(crate, set())
        allowed_external = EXPECTED_EXTERNALS.get(crate, set())
        unreviewed_external = unexpected_external - allowed_external
        missing_external = allowed_external - unexpected_external
        if unreviewed_external:
            errors.append(
                f"{crate} has unreviewed external dependencies: {sorted(unreviewed_external)}"
            )
        if missing_external:
            errors.append(
                f"{crate} is missing expected reviewed external dependencies: {sorted(missing_external)}"
            )

    return errors


def load_workspace_graph() -> tuple[dict[str, set[str]], dict[str, set[str]]]:
    completed = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(ROOT / "Cargo.toml"),
            "--format-version",
            "1",
            "--no-deps",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(completed.stdout)
    dependencies: dict[str, set[str]] = {}
    external: dict[str, set[str]] = {}

    for package in metadata["packages"]:
        crate = package["name"]
        dependencies[crate] = set()
        external[crate] = set()
        for dependency in package["dependencies"]:
            if dependency.get("path"):
                dependencies[crate].add(dependency["name"])
            else:
                external[crate].add(dependency["name"])

    return dependencies, external


def main() -> int:
    try:
        dependencies, external = load_workspace_graph()
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"CRATE GRAPH ERROR: {error}", file=sys.stderr)
        return 1

    errors = validate_graph(dependencies, external)
    if errors:
        for error in errors:
            print(f"CRATE GRAPH ERROR: {error}", file=sys.stderr)
        return 1

    print("CRATE GRAPH OK")
    for crate in sorted(dependencies):
        edges = sorted(dependencies[crate])
        print(f"{crate} -> {', '.join(edges) if edges else '(none)'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
