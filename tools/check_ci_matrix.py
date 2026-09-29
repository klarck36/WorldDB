"""Check the provider-neutral M0-14 OS/MSRV/feature job contract."""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

from run_feature_matrix import load_matrix as load_feature_matrix


ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "policy" / "ci-matrix.tsv"
VERIFY_STEPS = ROOT / "tools" / "verify" / "steps.tsv"
TOOLCHAIN = ROOT / "rust-toolchain.toml"
EXPECTED_JOBS = {
    "linux": "linux-msrv",
    "windows": "windows-msrv",
    "macos": "macos-msrv",
}
EXPECTED_FEATURES = ("no-default", "default", "all-features")
EXPECTED_COMMAND = "cargo xtask verify"
EXPECTED_ARTIFACT = "tools/verify/steps.tsv"
FIELDS = (
    "job_id",
    "os",
    "rust_toolchain",
    "verify_profile",
    "feature_profiles",
    "checkout",
    "ci_entrypoint",
    "verify_command",
    "artifact_manifest",
)


def read_rows(path: Path, fields: tuple[str, ...]) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as stream:
        reader = csv.DictReader(
            (line for line in stream if not line.lstrip().startswith("#")),
            delimiter="\t",
        )
        if tuple(reader.fieldnames or ()) != fields:
            raise ValueError(f"{path.relative_to(ROOT)} has an unexpected header")
        rows: list[dict[str, str]] = []
        for line_number, row in enumerate(reader, start=2):
            if None in row or any(value is None for value in row.values()):
                raise ValueError(f"{path.relative_to(ROOT)}:{line_number} has the wrong number of fields")
            rows.append({key: value.strip() for key, value in row.items()})
        return rows


def load_ci_matrix() -> list[dict[str, str]]:
    return read_rows(MATRIX, FIELDS)


def rust_channel() -> str:
    content = TOOLCHAIN.read_text(encoding="utf-8")
    match = re.search(r'^channel\s*=\s*"([^"]+)"\s*$', content, flags=re.MULTILINE)
    if not match:
        raise ValueError("rust-toolchain.toml does not declare a channel")
    return match.group(1)


def verify_manifest_state() -> tuple[set[str], dict[str, str]]:
    profiles: set[str] = set()
    steps: dict[str, str] = {}
    for line_number, raw_line in enumerate(VERIFY_STEPS.read_text(encoding="utf-8").splitlines(), start=1):
        line = raw_line.removesuffix("\r")
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        if fields[0] == "profile" and len(fields) == 4:
            profiles.add(fields[1])
        elif fields[0] == "step" and len(fields) == 7:
            steps[fields[2]] = fields[3]
        else:
            raise ValueError(f"verify manifest line {line_number} is malformed")
    return profiles, steps


def validate_matrix(
    rows: list[dict[str, str]],
    *,
    declared_toolchain: str,
    feature_profiles: set[str],
    verify_profiles: set[str],
    verify_steps: dict[str, str],
) -> list[str]:
    errors: list[str] = []
    seen_jobs = [row.get("job_id", "") for row in rows]
    seen_os = [row.get("os", "") for row in rows]
    if len(rows) != len(EXPECTED_JOBS):
        errors.append(f"expected {len(EXPECTED_JOBS)} platform jobs, found {len(rows)}")
    if len(seen_jobs) != len(set(seen_jobs)):
        errors.append("CI job IDs must be unique")
    if len(seen_os) != len(set(seen_os)):
        errors.append("each required OS must have exactly one matrix row")
    if set(seen_os) != set(EXPECTED_JOBS):
        errors.append(f"required OS set differs: expected {sorted(EXPECTED_JOBS)}, found {sorted(set(seen_os))}")

    expected_feature_profiles = set(EXPECTED_FEATURES)
    if feature_profiles != expected_feature_profiles:
        errors.append("the executable feature matrix must define no-default, default, and all-features")
    if "dev" not in verify_profiles:
        errors.append("tools/verify/steps.tsv must define the dev verification profile")
    if verify_steps.get("feature-matrix") != "run":
        errors.append("the dev profile must execute the feature matrix")
    if "ci-matrix" not in verify_steps:
        errors.append("the visible ci-matrix entry must remain until a provider pipeline replaces it")

    for row in rows:
        os_name = row.get("os", "")
        job_id = row.get("job_id", "")
        context = job_id or os_name or "<row>"
        if EXPECTED_JOBS.get(os_name) != job_id:
            errors.append(f"{context}: job ID does not match its platform")
        if row.get("rust_toolchain") != declared_toolchain:
            errors.append(f"{context}: toolchain differs from rust-toolchain.toml ({declared_toolchain})")
        if row.get("verify_profile") != "dev" or row.get("verify_command") != EXPECTED_COMMAND:
            errors.append(f"{context}: required command is `cargo xtask verify` using profile dev")
        if row.get("ci_entrypoint") != "tools/run_ci_job.py":
            errors.append(f"{context}: the platform job must use tools/run_ci_job.py")
        feature_list = tuple(row.get("feature_profiles", "").split(","))
        if feature_list != EXPECTED_FEATURES:
            errors.append(f"{context}: feature profiles must be {','.join(EXPECTED_FEATURES)} in that order")
        if row.get("checkout") != "clean":
            errors.append(f"{context}: jobs must start from a clean checkout")
        if row.get("artifact_manifest") != EXPECTED_ARTIFACT:
            errors.append(f"{context}: job must archive {EXPECTED_ARTIFACT}")
    if not (ROOT / EXPECTED_ARTIFACT).is_file():
        errors.append(f"required step manifest is missing: {EXPECTED_ARTIFACT}")
    if not (ROOT / "tools" / "run_ci_job.py").is_file():
        errors.append("provider-neutral job entrypoint is missing: tools/run_ci_job.py")
    return errors


def validate() -> tuple[list[dict[str, str]], list[str]]:
    rows = load_ci_matrix()
    feature_rows = load_feature_matrix()
    feature_profiles = {name for name, _args in feature_rows}
    verify_profiles, verify_steps = verify_manifest_state()
    errors = validate_matrix(
        rows,
        declared_toolchain=rust_channel(),
        feature_profiles=feature_profiles,
        verify_profiles=verify_profiles,
        verify_steps=verify_steps,
    )
    return rows, errors


def main() -> int:
    try:
        rows, errors = validate()
    except (OSError, ValueError, csv.Error) as error:
        print(f"CI MATRIX CONTRACT ERROR: {error}", file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(f"CI MATRIX CONTRACT ERROR: {error}", file=sys.stderr)
        return 1
    print(
        "CI MATRIX CONTRACT OK: "
        f"{len(rows)} provider-neutral platform jobs, Rust {rust_channel()}, "
        "no-default/default/all-features, clean checkout, step-manifest artifact"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
