"""Run M0-13 checks and save machine-readable evidence outside the repository."""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import uuid
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from check_testkit_corpus import validate_corpus


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "docs" / "schemas" / "test-evidence.schema.json"
DEFAULT_SEED = 0x574F524C44444231
FAULT_MARKER = "FAULT_PROBE: injected fault at m0-13-replay-probe with seed"


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def normalized_seed() -> str:
    raw = os.environ.get("WORLDDB_TEST_SEED")
    if raw is None:
        value = DEFAULT_SEED
    else:
        try:
            value = int(raw, 16 if raw.lower().startswith("0x") else 10)
        except ValueError as error:
            raise ValueError("WORLDDB_TEST_SEED must be an unsigned decimal or 0x hexadecimal u64") from error
        if not 0 <= value < 1 << 64:
            raise ValueError("WORLDDB_TEST_SEED is outside the u64 range")
    return f"0x{value:016x}"


def cargo_command() -> str:
    located = shutil.which("cargo")
    if located:
        return located
    user_profile = os.environ.get("USERPROFILE")
    if user_profile:
        candidate = Path(user_profile) / ".cargo" / "bin" / ("cargo.exe" if os.name == "nt" else "cargo")
        if candidate.is_file():
            return str(candidate)
    raise FileNotFoundError("Cargo was not found on PATH or under %USERPROFILE%/.cargo/bin")


def rustc_command() -> str:
    located = shutil.which("rustc")
    if located:
        return located
    user_profile = os.environ.get("USERPROFILE")
    if user_profile:
        candidate = Path(user_profile) / ".cargo" / "bin" / ("rustc.exe" if os.name == "nt" else "rustc")
        if candidate.is_file():
            return str(candidate)
    raise FileNotFoundError("rustc was not found on PATH or under %USERPROFILE%/.cargo/bin")


def filesystem_type(path: Path) -> str:
    if os.name == "nt":
        from ctypes import wintypes

        drive_root = Path(path.anchor)
        filesystem_name = ctypes.create_unicode_buffer(256)
        serial = wintypes.DWORD()
        maximum_component = wintypes.DWORD()
        flags = wintypes.DWORD()
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        get_volume_information = kernel32.GetVolumeInformationW
        get_volume_information.argtypes = [
            wintypes.LPCWSTR,
            wintypes.LPWSTR,
            wintypes.DWORD,
            ctypes.POINTER(wintypes.DWORD),
            ctypes.POINTER(wintypes.DWORD),
            ctypes.POINTER(wintypes.DWORD),
            wintypes.LPWSTR,
            wintypes.DWORD,
        ]
        get_volume_information.restype = wintypes.BOOL
        succeeded = get_volume_information(
            str(drive_root),
            None,
            0,
            ctypes.byref(serial),
            ctypes.byref(maximum_component),
            ctypes.byref(flags),
            filesystem_name,
            len(filesystem_name),
        )
        if succeeded:
            return filesystem_name.value or "unidentified"
        return f"unidentified (GetVolumeInformationW error {ctypes.get_last_error()})"

    if sys.platform.startswith("linux"):
        mount_file = Path("/proc/mounts")
        if mount_file.is_file():
            target = str(path.resolve())
            matches: list[tuple[int, str]] = []
            for line in mount_file.read_text(encoding="utf-8", errors="replace").splitlines():
                fields = line.split()
                if len(fields) >= 3:
                    mount = fields[1].replace("\\040", " ")
                    if target == mount or target.startswith(mount.rstrip("/") + "/"):
                        matches.append((len(mount), fields[2]))
            if matches:
                return max(matches)[1]

    if sys.platform == "darwin" and shutil.which("diskutil"):
        result = subprocess.run(
            ["diskutil", "info", str(path)],
            capture_output=True,
            check=False,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        for line in result.stdout.splitlines():
            if "Type (Bundle)" in line:
                return line.split(":", maxsplit=1)[-1].strip() or "unidentified"
    return "unidentified"


def command_result(
    test_id: str,
    command: list[str],
    environment: dict[str, str],
    run_dir: Path,
    log_prefix: str,
    acceptance: Any,
) -> tuple[dict[str, Any], bytes, bytes]:
    stdout_path = run_dir / f"{log_prefix}.stdout.txt"
    stderr_path = run_dir / f"{log_prefix}.stderr.txt"
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        stdout = completed.stdout
        stderr = completed.stderr
        return_code: int | None = completed.returncode
        diagnostic = ""
    except OSError as error:
        stdout = b""
        stderr = str(error).encode("utf-8", errors="replace")
        return_code = None
        diagnostic = f"could not start command: {error}"
    stdout_path.write_bytes(stdout)
    stderr_path.write_bytes(stderr)
    passed, details = acceptance(return_code, stdout, stderr)
    artifacts = [artifact_ref(stdout_path), artifact_ref(stderr_path)]
    record = {
        "test_id": test_id,
        "status": "PASS" if passed else "FAIL",
        "command": command,
        "exit_code": return_code,
        "artifacts": artifacts,
        "details": details or diagnostic,
    }
    return record, stdout, stderr


def artifact_ref(path: Path) -> dict[str, str]:
    return {"path": str(path.resolve()), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def zero_exit(return_code: int | None, _stdout: bytes, _stderr: bytes) -> tuple[bool, str]:
    return return_code == 0, "expected exit code 0" if return_code == 0 else f"expected exit code 0, got {return_code}"


def execute() -> tuple[int, Path]:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-root", type=Path, help="external directory for run artifacts")
    args = parser.parse_args()
    seed = normalized_seed()
    configured_root = os.environ.get("WORLDDB_TEST_RUNS_DIR")
    output_root = args.output_root or (Path(configured_root) if configured_root else None)
    if output_root is None:
        output_root = Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "WorldDB" / "test-runs" / "M0-13"
    output_root = output_root.resolve()
    if output_root == ROOT.resolve() or ROOT.resolve() in output_root.parents or output_root in ROOT.resolve().parents:
        raise ValueError("M0-13 run artifacts must be outside the synchronized repository")
    output_root.mkdir(parents=True, exist_ok=True)
    started_at = utc_now()
    requested_run_id = os.environ.get("WORLDDB_TEST_RUN_ID")
    run_id = requested_run_id or f"M0-13-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}-{uuid.uuid4().hex[:10]}"
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,100}", run_id):
        raise ValueError("WORLDDB_TEST_RUN_ID contains unsupported path characters")
    run_dir = output_root / run_id
    run_dir.mkdir()
    target_dir = run_dir / "cargo-target"

    environment = os.environ.copy()
    environment["WORLDDB_TEST_SEED"] = seed
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    cargo = cargo_command()
    rustc = rustc_command()
    test_records: list[dict[str, Any]] = []

    corpus_fixtures = validate_corpus()
    corpus_check_command = [sys.executable, "-B", "-X", "utf8", "tools/check_testkit_corpus.py"]
    test_record, _, _ = command_result(
        "versioned-corpus-integrity",
        corpus_check_command,
        environment,
        run_dir,
        "01-corpus-integrity",
        zero_exit,
    )
    test_records.append(test_record)

    python_tests_command = [
        sys.executable,
        "-B",
        "-X",
        "utf8",
        "-m",
        "unittest",
        "discover",
        "-s",
        "tools",
        "-p",
        "test_testkit.py",
        "-v",
    ]
    test_record, _, _ = command_result(
        "corpus-policy-tests",
        python_tests_command,
        environment,
        run_dir,
        "02-corpus-policy-tests",
        zero_exit,
    )
    test_records.append(test_record)

    rust_tests_command = [cargo, "test", "--locked", "--package", "worlddb-testkit"]
    test_record, _, _ = command_result(
        "testkit-rust-tests",
        rust_tests_command,
        environment,
        run_dir,
        "03-testkit-rust-tests",
        zero_exit,
    )
    test_records.append(test_record)

    fault_tests_command = [
        cargo,
        "test",
        "--locked",
        "--package",
        "worlddb-testkit",
        "--features",
        "fault-injection",
    ]
    test_record, _, _ = command_result(
        "fault-hook-unit-tests",
        fault_tests_command,
        environment,
        run_dir,
        "03b-fault-hook-unit-tests",
        zero_exit,
    )
    test_records.append(test_record)

    release_build_command = [cargo, "build", "--locked", "--release", "--package", "worlddb-testkit"]
    test_record, _, _ = command_result(
        "release-testkit-build",
        release_build_command,
        environment,
        run_dir,
        "04-release-testkit-build",
        zero_exit,
    )
    test_records.append(test_record)

    library_artifacts = sorted((target_dir / "release" / "deps").glob("libworlddb_testkit-*.rlib"))
    executable_name = "fault-probe.exe" if os.name == "nt" else "fault-probe"
    release_executables = list((target_dir / "release").rglob(executable_name)) if (target_dir / "release").exists() else []
    marker_bytes = b"WorldDB::fault-injection::M0-13"
    marker_found = any(marker_bytes in path.read_bytes() for path in library_artifacts if path.is_file())
    release_artifact_ok = bool(library_artifacts) and not release_executables and not marker_found
    artifact_details = (
        f"rlibs={len(library_artifacts)}; release_fault_probes={len(release_executables)}; "
        f"fault_marker_present={marker_found}"
    )
    artifact_log = run_dir / "05-release-hook-absence.txt"
    artifact_log.write_text(artifact_details + "\n", encoding="utf-8")
    test_records.append(
        {
            "test_id": "release-hook-absence",
            "status": "PASS" if release_artifact_ok else "FAIL",
            "command": ["inspect release rlib, feature marker, and fault-probe executable"],
            "exit_code": 0 if release_artifact_ok else 1,
            "artifacts": [artifact_ref(artifact_log)],
            "details": artifact_details,
        }
    )

    feature_tree_command = [
        cargo,
        "tree",
        "--locked",
        "--package",
        "worlddb-testkit",
        "--no-default-features",
        "--edges",
        "features",
    ]
    test_record, tree_stdout, tree_stderr = command_result(
        "release-feature-tree-excludes-hooks",
        feature_tree_command,
        environment,
        run_dir,
        "06-release-feature-tree",
        lambda code, out, err: (
            code == 0 and b"fault-injection" not in out.lower() and b"fault-injection" not in err.lower(),
            "default release feature graph excludes fault-injection"
            if code == 0 and b"fault-injection" not in out.lower() and b"fault-injection" not in err.lower()
            else "fault-injection appeared in the default feature graph or cargo tree failed",
        ),
    )
    test_records.append(test_record)

    release_fault_command = [
        cargo,
        "check",
        "--locked",
        "--release",
        "--package",
        "worlddb-testkit",
        "--features",
        "fault-injection",
    ]
    test_record, _, _ = command_result(
        "release-fault-feature-rejected",
        release_fault_command,
        environment,
        run_dir,
        "07-release-fault-feature-rejection",
        lambda code, out, err: (
            code is not None
            and code != 0
            and b"fault hooks cannot be built with a release profile" in out + err,
            "release compilation explicitly rejected fault-injection"
            if code is not None and code != 0 and b"fault hooks cannot be built with a release profile" in out + err
            else "release fault-injection did not fail with the expected compile-time guard",
        ),
    )
    test_records.append(test_record)

    replay_command = [
        cargo,
        "run",
        "--quiet",
        "--locked",
        "--package",
        "worlddb-testkit",
        "--features",
        "fault-injection",
        "--bin",
        "fault-probe",
        "--",
        "--seed",
        seed,
    ]

    def replay_acceptance(code: int | None, out: bytes, err: bytes) -> tuple[bool, str]:
        expected = f"{FAULT_MARKER} {seed}".encode("utf-8")
        passed = code == 73 and expected in err and not out
        return passed, "fault probe returned 73 and recorded the exact seed" if passed else "fault probe did not produce the expected seeded failure"

    replay_one, replay_stdout_one, replay_stderr_one = command_result(
        "fault-seed-replay-first",
        replay_command,
        environment,
        run_dir,
        "08-fault-seed-first",
        replay_acceptance,
    )
    replay_two, replay_stdout_two, replay_stderr_two = command_result(
        "fault-seed-replay-second",
        replay_command,
        environment,
        run_dir,
        "09-fault-seed-replay",
        replay_acceptance,
    )
    replayed_identically = (
        replay_one["exit_code"] == replay_two["exit_code"]
        and replay_stdout_one == replay_stdout_two
        and replay_stderr_one == replay_stderr_two
    )
    if not replayed_identically:
        replay_one["status"] = "FAIL"
        replay_two["status"] = "FAIL"
        replay_one["details"] = "first and replayed failure outputs differ"
        replay_two["details"] = "first and replayed failure outputs differ"
    test_records.extend([replay_one, replay_two])

    toolchain_record = collect_toolchain(rustc, cargo, environment, run_dir)
    source_snapshot = make_source_snapshot(run_dir, environment)
    commit = git_output(["git", "rev-parse", "HEAD"], environment).strip()
    dirty = bool(git_output(["git", "status", "--porcelain", "--untracked-files=all"], environment).strip())
    hardware_machine = platform.machine() or "unidentified"
    logical_cpus = os.cpu_count() or 1
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    if schema.get("$id") != "https://worlddb.invalid/schemas/test-evidence-v1.json":
        raise ValueError("unexpected test evidence schema identifier")

    finished_at = utc_now()
    all_passed = all(record["status"] == "PASS" for record in test_records)
    logs = sorted(
        path
        for path in run_dir.iterdir()
        if path.is_file() and path.name != "evidence.json"
    )
    evidence = {
        "schema_version": 1,
        "run_id": run_id,
        "task_id": "M0-13",
        "status": "PASS" if all_passed else "FAIL",
        "started_at": started_at,
        "finished_at": finished_at,
        "artifact_path": str(run_dir.resolve()),
        "repository": {"commit": commit, "dirty": dirty, "source_snapshot": source_snapshot},
        "environment": {
            "os": platform.system() or "unidentified",
            "os_version": platform.version() or platform.release() or "unidentified",
            "architecture": platform.machine() or "unidentified",
            "filesystem": {"root": str(output_root), "type": filesystem_type(output_root)},
            "hardware": {
                "machine": hardware_machine,
                "processor": platform.processor() or hardware_machine,
                "logical_cpus": logical_cpus,
            },
        },
        "toolchain": toolchain_record,
        "seed": seed,
        "fixtures": corpus_fixtures,
        "tests": test_records,
        "artifacts": [artifact_ref(path) for path in logs],
        "reproduction": {
            "command": replay_command,
            "expected_exit_code": 73,
            "replayed_identically": replayed_identically,
            "source_snapshot_path": source_snapshot["path"],
        },
    }
    manifest_path = run_dir / "evidence.json"
    manifest_path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    validate_evidence(evidence, run_dir)
    print(f"M0-13 evidence: {manifest_path}")
    print(f"M0-13 run: {run_id}; status={evidence['status']}; seed={seed}")
    return (0 if all_passed else 1), manifest_path


def collect_toolchain(rustc: str, cargo: str, environment: dict[str, str], run_dir: Path) -> dict[str, str]:
    rustc_version = subprocess.run(
        [rustc, "--version", "--verbose"], cwd=ROOT, env=environment, capture_output=True, check=False
    )
    cargo_version = subprocess.run(
        [cargo, "--version"], cwd=ROOT, env=environment, capture_output=True, check=False
    )
    rustc_text = (rustc_version.stdout + rustc_version.stderr).decode("utf-8", errors="replace").strip()
    cargo_text = (cargo_version.stdout + cargo_version.stderr).decode("utf-8", errors="replace").strip()
    (run_dir / "10-rustc-version.txt").write_text(rustc_text + "\n", encoding="utf-8")
    (run_dir / "11-cargo-version.txt").write_text(cargo_text + "\n", encoding="utf-8")
    if rustc_version.returncode != 0 or cargo_version.returncode != 0:
        raise RuntimeError("could not capture Rust and Cargo versions")
    host_match = re.search(r"^host: (.+)$", rustc_text, flags=re.MULTILINE)
    if not host_match:
        raise RuntimeError("rustc --version --verbose did not report its host target")
    return {"rustc": rustc_text.splitlines()[0], "cargo": cargo_text, "host_target": host_match.group(1)}


def git_output(command: list[str], environment: dict[str, str]) -> str:
    completed = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, check=False)
    if completed.returncode != 0:
        raise RuntimeError(f"could not collect repository metadata: {' '.join(command)}")
    return completed.stdout.decode("utf-8", errors="replace")


def make_source_snapshot(run_dir: Path, environment: dict[str, str]) -> dict[str, str]:
    listing = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        env=environment,
        capture_output=True,
        check=False,
    )
    if listing.returncode != 0:
        raise RuntimeError("could not enumerate source files for the reproducibility snapshot")
    relative_paths = [item for item in listing.stdout.split(b"\0") if item]
    snapshot_path = run_dir / "source-snapshot.zip"
    with zipfile.ZipFile(snapshot_path, mode="w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for encoded in relative_paths:
            relative = Path(os.fsdecode(encoded))
            source = (ROOT / relative).resolve()
            if ROOT.resolve() not in source.parents or not source.is_file():
                raise RuntimeError(f"source snapshot entry is missing or escapes the repository: {relative}")
            archive.write(source, arcname=relative.as_posix())
    return artifact_ref(snapshot_path)


def validate_evidence(evidence: dict[str, Any], run_dir: Path) -> None:
    required = {
        "schema_version",
        "run_id",
        "task_id",
        "status",
        "started_at",
        "finished_at",
        "artifact_path",
        "repository",
        "environment",
        "toolchain",
        "seed",
        "fixtures",
        "tests",
        "artifacts",
        "reproduction",
    }
    if set(evidence) != required or evidence["schema_version"] != 1:
        raise ValueError("evidence manifest does not satisfy the version 1 top-level schema")
    if evidence["status"] not in {"PASS", "FAIL"} or evidence["task_id"] != "M0-13":
        raise ValueError("evidence manifest has an invalid task or status")
    if not Path(evidence["artifact_path"]).is_dir():
        raise ValueError("evidence artifact directory does not resolve")
    if not isinstance(evidence["repository"]["dirty"], bool):
        raise ValueError("repository dirty field must be a boolean")
    verify_artifact(evidence["repository"]["source_snapshot"])
    if evidence["reproduction"]["source_snapshot_path"] != evidence["repository"]["source_snapshot"]["path"]:
        raise ValueError("reproduction metadata does not reference the source snapshot")
    for fixture in evidence["fixtures"]:
        path = Path(fixture["path"])
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != fixture["sha256"]:
            raise ValueError(f"fixture evidence path or hash does not resolve: {path}")
    for record in evidence["tests"]:
        if record["status"] not in {"PASS", "FAIL"}:
            raise ValueError(f"invalid result status in {record['test_id']}")
        for artifact in record["artifacts"]:
            verify_artifact(artifact)
    for artifact in evidence["artifacts"]:
        verify_artifact(artifact)
    manifest_path = run_dir / "evidence.json"
    if not manifest_path.is_file():
        raise ValueError("machine-readable evidence manifest was not written")


def verify_artifact(artifact: dict[str, str]) -> None:
    path = Path(artifact["path"])
    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != artifact["sha256"]:
        raise ValueError(f"evidence artifact path or hash does not resolve: {path}")


def main() -> int:
    try:
        exit_code, _manifest = execute()
        return exit_code
    except Exception as error:
        print(f"M0-13 evidence runner failed before completing a manifest: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
