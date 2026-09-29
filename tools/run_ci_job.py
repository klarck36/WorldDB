"""Run one clean-checkout M0-14 job and archive its step manifest and evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from check_ci_matrix import EXPECTED_FEATURES, load_ci_matrix, rust_channel


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "docs" / "schemas" / "ci-job-evidence.schema.json"
SUMMARY_PATTERN = re.compile(
    r"VERIFY SUMMARY: profile=(\S+) passed=(\d+) skipped=(\d+) failed=(\d+)"
)
SKIP_PATTERN = re.compile(r"^\[SKIP\]\s+(\S+)", flags=re.MULTILINE)


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def host_os() -> str:
    value = platform.system().lower()
    return {"darwin": "macos"}.get(value, value)


def git_text(args: list[str], environment: dict[str, str]) -> str:
    result = subprocess.run(
        ["git", *args], cwd=ROOT, env=environment, capture_output=True, check=False
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {result.stderr.decode('utf-8', errors='replace')}")
    return result.stdout.decode("utf-8", errors="replace")


def checkout_is_clean(environment: dict[str, str]) -> bool:
    return not git_text(["status", "--porcelain", "--untracked-files=all"], environment).strip()


def tool_path(name: str) -> str:
    located = shutil.which(name)
    if located:
        return located
    user_profile = os.environ.get("USERPROFILE")
    cargo_home = os.environ.get("CARGO_HOME")
    candidates: list[Path] = []
    if cargo_home:
        candidates.append(Path(cargo_home) / "bin" / (f"{name}.exe" if os.name == "nt" else name))
    if user_profile:
        candidates.append(Path(user_profile) / ".cargo" / "bin" / (f"{name}.exe" if os.name == "nt" else name))
    for candidate in candidates:
        if candidate.is_file():
            return str(candidate)
    raise FileNotFoundError(f"{name} was not found on PATH or in the Cargo bin directory")


def capture_version(command: list[str], cwd: Path, environment: dict[str, str]) -> tuple[int, str]:
    result = subprocess.run(command, cwd=cwd, env=environment, capture_output=True, check=False)
    text = (result.stdout + result.stderr).decode("utf-8", errors="replace").strip()
    return result.returncode, text


def parse_summary(output: str) -> dict[str, Any] | None:
    matches = list(SUMMARY_PATTERN.finditer(output))
    if len(matches) != 1:
        return None
    match = matches[0]
    return {
        "profile": match.group(1),
        "passed": int(match.group(2)),
        "skipped": int(match.group(3)),
        "failed": int(match.group(4)),
    }


def expected_skip_steps() -> set[str]:
    skips: set[str] = set()
    for raw_line in (ROOT / "tools" / "verify" / "steps.tsv").read_text(encoding="utf-8").splitlines():
        if not raw_line or raw_line.startswith("#"):
            continue
        fields = raw_line.split("\t")
        if fields[0] == "step" and len(fields) == 7 and fields[1] == "dev" and fields[3] == "skip":
            skips.add(fields[2])
    return skips


def validate_outcome(
    row: dict[str, str],
    *,
    actual_os: str,
    actual_rustc: str,
    commit: str,
    clean_before: bool,
    clean_after: bool,
    exit_code: int | None,
    summary: dict[str, Any] | None,
    skipped_steps: set[str],
    expected_skips: set[str],
) -> list[str]:
    errors: list[str] = []
    if actual_os != row["os"]:
        errors.append(f"expected {row['os']} host, found {actual_os}")
    expected_rustc_prefix = f"rustc {row['rust_toolchain']} "
    if not actual_rustc.startswith(expected_rustc_prefix):
        errors.append(f"expected Rust {row['rust_toolchain']}, found {actual_rustc.splitlines()[0] if actual_rustc else 'unavailable'}")
    if not re.fullmatch(r"[0-9a-f]{40,64}", commit):
        errors.append("git commit is missing or malformed")
    if not clean_before or not clean_after:
        errors.append("job must start and finish with a clean checkout")
    if exit_code != 0:
        errors.append(f"cargo xtask verify exited with {exit_code}")
    if summary is None:
        errors.append("cargo xtask verify summary is missing or ambiguous")
    else:
        if summary["profile"] != row["verify_profile"]:
            errors.append(f"expected verify profile {row['verify_profile']}, found {summary['profile']}")
        if summary["failed"] != 0:
            errors.append(f"verify reported {summary['failed']} failed step(s)")
        if summary["skipped"] != len(skipped_steps):
            errors.append("verify summary skip count differs from the recorded skip lines")
    if skipped_steps != expected_skips:
        errors.append(f"unexpected skipped steps: expected {sorted(expected_skips)}, found {sorted(skipped_steps)}")
    return errors


def artifact_ref(path: Path) -> dict[str, str]:
    return {"path": str(path.resolve()), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def run(job_id: str, artifact_root: Path) -> tuple[int, Path]:
    rows = {row["job_id"]: row for row in load_ci_matrix()}
    if job_id not in rows:
        raise ValueError(f"unknown CI job {job_id!r}; expected one of {sorted(rows)}")
    row = rows[job_id]
    artifact_root = artifact_root.resolve()
    if artifact_root == ROOT.resolve() or ROOT.resolve() in artifact_root.parents or artifact_root in ROOT.resolve().parents:
        raise ValueError("CI job artifacts must be outside the repository")
    artifact_root.mkdir(parents=True, exist_ok=True)
    started_at = utc_now()
    run_id = f"M0-14-{job_id}-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}-{uuid.uuid4().hex[:8]}"
    artifact_dir = artifact_root / run_id
    artifact_dir.mkdir()
    target_dir = artifact_root / f"{run_id}-target"
    target_dir.mkdir()

    environment = os.environ.copy()
    environment["WORLDDB_VERIFY_PROFILE"] = row["verify_profile"]
    environment["WORLDDB_PYTHON"] = sys.executable
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    test_runs_root = artifact_root.parent / "testkit-evidence"
    environment["WORLDDB_TEST_RUNS_DIR"] = str(test_runs_root)
    environment["WORLDDB_TEST_RUN_ID"] = f"M0-13-{job_id}-{uuid.uuid4().hex[:8]}"

    cargo = tool_path("cargo")
    rustc = tool_path("rustc")
    rustc_code, rustc_version = capture_version([rustc, "--version", "--verbose"], ROOT, environment)
    cargo_code, cargo_version = capture_version([cargo, "--version"], ROOT, environment)
    host_match = re.search(r"^host: (.+)$", rustc_version, flags=re.MULTILINE)
    target = host_match.group(1) if host_match else "unavailable"
    commit = git_text(["rev-parse", "HEAD"], environment).strip()
    clean_before = checkout_is_clean(environment)

    steps_source = ROOT / row["artifact_manifest"]
    steps_copy = artifact_dir / "steps.tsv"
    if steps_source.is_file():
        shutil.copyfile(steps_source, steps_copy)
    else:
        steps_copy.write_text("MISSING\n", encoding="utf-8")

    command = [cargo, "xtask", "verify"]
    command_exit: int | None = None
    stdout = b""
    stderr = b""
    if clean_before and rustc_code == 0 and cargo_code == 0:
        try:
            completed = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, check=False)
            command_exit = completed.returncode
            stdout = completed.stdout
            stderr = completed.stderr
        except OSError as error:
            stderr = str(error).encode("utf-8", errors="replace")
    else:
        stderr = b"preflight failed: clean checkout and Rust/Cargo toolchain are required"

    stdout_path = artifact_dir / "verify.stdout.txt"
    stderr_path = artifact_dir / "verify.stderr.txt"
    stdout_path.write_bytes(stdout)
    stderr_path.write_bytes(stderr)
    combined_output = (stdout + b"\n" + stderr).decode("utf-8", errors="replace")
    summary = parse_summary(combined_output)
    skipped_steps = set(SKIP_PATTERN.findall(combined_output))
    clean_after = checkout_is_clean(environment)
    expected_skips = expected_skip_steps()
    errors = validate_outcome(
        row,
        actual_os=host_os(),
        actual_rustc=rustc_version.splitlines()[0] if rustc_version else "",
        commit=commit,
        clean_before=clean_before,
        clean_after=clean_after,
        exit_code=command_exit,
        summary=summary,
        skipped_steps=skipped_steps,
        expected_skips=expected_skips,
    )
    if rustc_code != 0:
        errors.append("rustc version command failed")
    if cargo_code != 0:
        errors.append("cargo version command failed")
    if not steps_copy.is_file() or steps_copy.read_text(encoding="utf-8") != steps_source.read_text(encoding="utf-8"):
        errors.append("archived step manifest does not match the checked-out manifest")

    artifacts = [artifact_ref(path) for path in (steps_copy, stdout_path, stderr_path)]
    finished_at = utc_now()
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "run_id": run_id,
        "job_id": job_id,
        "status": "PASS" if not errors else "FAIL",
        "started_at": started_at,
        "finished_at": finished_at,
        "artifact_path": str(artifact_dir.resolve()),
        "os": host_os(),
        "architecture": platform.machine() or "unidentified",
        "rust_toolchain": rustc_version.splitlines()[0] if rustc_version else "unavailable",
        "cargo": cargo_version or "unavailable",
        "target": target,
        "commit": commit,
        "checkout_clean_before": clean_before,
        "checkout_clean_after": clean_after,
        "feature_profiles": list(EXPECTED_FEATURES),
        "verify_command": "cargo xtask verify",
        "exit_code": command_exit,
        "failure_reasons": errors,
        "verify_summary": summary,
        "skipped_steps": sorted(skipped_steps),
        "artifacts": artifacts,
    }
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    if schema.get("$id") != "https://worlddb.invalid/schemas/ci-job-evidence-v1.json":
        raise ValueError("unexpected CI job evidence schema identifier")
    manifest_path = artifact_dir / "ci-job.json"
    manifest_path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    validate_evidence(evidence, artifact_dir)
    print(f"CI job evidence: {manifest_path}")
    for error in errors:
        print(f"CI JOB FAIL: {error}", file=sys.stderr)
    if summary is not None:
        print(
            f"CI JOB SUMMARY: job={job_id} os={host_os()} target={target} "
            f"passed={summary['passed']} skipped={summary['skipped']} failed={summary['failed']}"
        )
    else:
        print(f"CI JOB SUMMARY: job={job_id} status=FAIL (verification summary unavailable)")
    return (0 if not errors else 1), manifest_path


def validate_evidence(evidence: dict[str, Any], artifact_dir: Path) -> None:
    required = {
        "schema_version",
        "run_id",
        "job_id",
        "status",
        "started_at",
        "finished_at",
        "artifact_path",
        "os",
        "architecture",
        "rust_toolchain",
        "cargo",
        "target",
        "commit",
        "checkout_clean_before",
        "checkout_clean_after",
        "feature_profiles",
        "verify_command",
        "exit_code",
        "failure_reasons",
        "verify_summary",
        "skipped_steps",
        "artifacts",
    }
    if set(evidence) != required or evidence["schema_version"] != 1:
        raise ValueError("CI evidence does not satisfy the version 1 top-level schema")
    if evidence["status"] not in {"PASS", "FAIL"}:
        raise ValueError("CI evidence status must be PASS or FAIL")
    if not Path(evidence["artifact_path"]).is_dir() or Path(evidence["artifact_path"]) != artifact_dir.resolve():
        raise ValueError("CI evidence artifact directory does not resolve")
    for artifact in evidence["artifacts"]:
        path = Path(artifact["path"])
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != artifact["sha256"]:
            raise ValueError(f"CI evidence artifact path/hash does not resolve: {path}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--job-id", required=True)
    parser.add_argument("--artifact-root", type=Path)
    args = parser.parse_args()
    configured_root = args.artifact_root or (
        Path(os.environ["WORLDDB_CI_ARTIFACT_ROOT"])
        if os.environ.get("WORLDDB_CI_ARTIFACT_ROOT")
        else Path(tempfile.gettempdir()) / "WorldDB" / "ci-jobs"
    )
    try:
        exit_code, _manifest = run(args.job_id, configured_root)
        return exit_code
    except Exception as error:
        print(f"CI JOB ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
