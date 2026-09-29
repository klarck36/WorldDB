"""Run the versioned default/no-default/all-feature workspace build matrix."""

from __future__ import annotations

import csv
import shlex
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "policy" / "feature-matrix.tsv"
EXPECTED = {
    "no-default": {"--no-default-features"},
    "default": set(),
    "all-features": {"--all-features"},
}


def load_matrix() -> list[tuple[str, list[str]]]:
    with MATRIX.open(encoding="utf-8", newline="") as stream:
        reader = csv.DictReader(stream, delimiter="\t")
        if tuple(reader.fieldnames or ()) != ("profile", "args"):
            raise ValueError("policy/feature-matrix.tsv has an unexpected header")
        rows = [(row["profile"], shlex.split(row["args"])) for row in reader]
    names = [name for name, _ in rows]
    if len(names) != len(set(names)) or set(names) != set(EXPECTED):
        raise ValueError("feature matrix must define no-default, default, and all-features once")
    for name, args in rows:
        if args[:1] != ["check"] or "--locked" not in args or "--workspace" not in args:
            raise ValueError(f"feature matrix row {name!r} must use locked workspace cargo check")
        if "--all-targets" not in args or "--features" in args:
            raise ValueError(f"feature matrix row {name!r} must cover all targets without a custom feature list")
        if name == "no-default" and "--no-default-features" not in args:
            raise ValueError("no-default matrix row must disable default features")
        if name == "default" and any(flag in args for flag in ("--no-default-features", "--all-features")):
            raise ValueError("default matrix row must use Cargo's default feature selection")
        if name == "all-features" and "--all-features" not in args:
            raise ValueError("all-features matrix row must enable all features")
    return rows


def main() -> int:
    try:
        rows = load_matrix()
    except (OSError, ValueError, csv.Error) as error:
        print(f"FEATURE MATRIX ERROR: {error}", file=sys.stderr)
        return 1

    failures = 0
    for name, args in rows:
        command = ["cargo", *args]
        print(f"[RUN ] feature-matrix/{name}: {' '.join(command)}", flush=True)
        try:
            completed = subprocess.run(command, cwd=ROOT, check=False)
        except OSError as error:
            print(f"[FAIL] feature-matrix/{name}: {error}", file=sys.stderr)
            failures += 1
            continue
        if completed.returncode == 0:
            print(f"[PASS] feature-matrix/{name}", flush=True)
        else:
            print(
                f"[FAIL] feature-matrix/{name}: cargo exited {completed.returncode}",
                file=sys.stderr,
            )
            failures += 1

    print(
        f"FEATURE MATRIX SUMMARY: passed={len(rows) - failures} failed={failures}",
        flush=True,
    )
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
