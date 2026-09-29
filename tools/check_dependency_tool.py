"""Require the repository-pinned cargo-deny executable."""

from __future__ import annotations

import csv
import re
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "policy" / "tool-versions.tsv"


def read_policy() -> tuple[str, str]:
    with POLICY.open(encoding="utf-8", newline="") as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    if len(rows) != 1 or set(rows[0]) != {"tool", "version", "install_rust_version"}:
        raise ValueError(f"{POLICY.relative_to(ROOT)} must contain exactly one tool row")
    row = rows[0]
    if not all(row.values()):
        raise ValueError("cargo-deny tool policy has an empty field")
    return row["tool"], row["version"]


def main() -> int:
    try:
        tool, expected = read_policy()
    except (OSError, ValueError, csv.Error) as error:
        print(f"DEPENDENCY TOOL ERROR: {error}", file=sys.stderr)
        return 1

    executable = shutil.which(tool)
    if executable is None:
        print(
            f"DEPENDENCY TOOL ERROR: {tool} is missing; install the pinned tool as described in "
            "docs/architecture/M0-12-dependency-policy.md",
            file=sys.stderr,
        )
        return 1

    try:
        completed = subprocess.run(
            [executable, "--version"], check=True, capture_output=True, text=True
        )
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"DEPENDENCY TOOL ERROR: cannot run {tool}: {error}", file=sys.stderr)
        return 1

    match = re.fullmatch(r"cargo-deny\s+(\S+)\s*", completed.stdout.strip())
    if match is None or match.group(1) != expected:
        found = completed.stdout.strip() or "<no version output>"
        print(
            f"DEPENDENCY TOOL ERROR: expected cargo-deny {expected}, found {found}",
            file=sys.stderr,
        )
        return 1

    print(f"DEPENDENCY TOOL OK: cargo-deny {expected}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
