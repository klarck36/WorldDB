"""Validate the M9 fuzz target, runner, seed, and resource registries."""

from __future__ import annotations

import csv
import json
import re
import sys
from pathlib import Path
from typing import Iterable


ROOT = Path(__file__).resolve().parents[1]
TARGETS = "policy/fuzz-targets.tsv"
DECODERS = "policy/decoder-inventory.tsv"
SEEDS = "policy/decoder-seeds.tsv"
RUNNERS = "policy/fuzz-runners.tsv"
RESOURCES = "policy/fuzz-resource-profiles.tsv"
RUN_MANIFEST_SCHEMA = "docs/fuzz/run-manifest.schema.json"

TARGET_HEADER = [
    "target_id",
    "target_class",
    "source_entrypoint",
    "seed_corpus",
    "resource_profile",
    "runner_id",
    "followup_task",
]
RUNNER_HEADER = [
    "runner_id",
    "family",
    "build_profile",
    "build_command",
    "run_command",
    "target_selector_env",
    "duration_env",
    "seed_env",
    "seed_corpus_env",
    "report_env",
    "max_input_env",
    "input_timeout_env",
    "resource_profile",
    "manifest_schema",
    "implementation_task",
]
RESOURCE_HEADER = [
    "profile_id",
    "max_input_bytes",
    "max_process_rss_bytes",
    "max_temp_disk_bytes",
    "per_input_timeout_seconds",
    "campaign_duration_seconds",
    "job_wall_seconds",
    "workers",
]
DECODER_HEADER = ["decoder_id", "family", "seed_id"]
SEED_HEADER = ["seed_id", "mutator", "class", "description"]

VALID_TARGET_CLASSES = {
    "decoder",
    "cli_parser",
    "import_parser",
    "adapter_decoder",
    "ipc_decoder",
    "recovery_scanner",
}
VALID_FOLLOWUP_TASKS = {"M9-04a", "M9-04b", "M9-04c"}
VALID_BUILD_PROFILES = {"windows_msvc_locked", "windows_node22"}
EXPECTED_NON_CORE_TARGET_IDS = {
    "cli_arguments",
    "cli_import_mapping",
    "cli_migration_plan_json",
    "cli_migration_step_records",
    "cli_policy_history",
    "cli_adapter_manifest",
    "cli_adapter_payload",
    "cli_adapter_handshake",
    "cli_adapter_output",
    "cli_adapter_frame",
    "storage_format_probe",
    "storage_current_manifest",
    "storage_manifest_name",
    "storage_history_segment",
    "storage_security_segment",
    "storage_wal_segment",
    "storage_wal_recovery_prefix",
    "storage_wal_payloads",
    "storage_audit_wal",
    "storage_recovery_journal",
    "storage_recovery_pipeline",
    "storage_verify_pipeline",
    "storage_salvage_scanner",
    "storage_backup_manifest",
    "storage_index_generation",
    "storage_index_pointer",
    "storage_index_generation_name",
    "storage_compaction_pin_manifest",
    "storage_required_audit_payload",
    "storage_replay_payload",
    "storage_migration_run_journal",
    "storage_guarded_migration_journal",
    "storage_storage_upgrade_journal",
    "storage_logical_export",
    "storage_logical_import_plan",
    "storage_import_prepare",
    "storage_sharing_export",
    "engine_ipc_request",
    "engine_fact_command",
    "engine_schema_command",
    "engine_branch_layer_command",
    "engine_entity_command",
    "engine_perspective_command",
    "engine_security_policy_command",
    "engine_history_space_transfer_command",
    "engine_query_cursor",
    "engine_job_journal",
    "engine_stream_id",
    "desktop_sidecar_request_response",
    "desktop_backup_dto",
    "desktop_export_import_dto",
    "desktop_migration_plan",
    "desktop_purge_report",
    "desktop_transfer_ids",
}
DECODER_FAMILY_ROUTES = {
    family: "rust_core_decoder"
    for family in (
        "frame",
        "value",
        "tlv",
        "record",
        "record_ref",
        "batch",
        "audit_record",
        "raw_read_attempt",
        "int_text",
        "uint_text",
        "decimal_text",
        "symbol_text",
        "id_text",
        "revision_text",
        "schema_revision_text",
        "int_bytes",
        "uint_bytes",
        "decimal_bytes",
        "cursor_token",
        "migration_run_journal",
    )
} | {"typescript": "typescript_transport"}

ID_PATTERN = re.compile(r"^[a-z][a-z0-9_]*$")
ENV_PATTERN = re.compile(r"^[A-Z][A-Z0-9_]*$")


class FuzzInventoryError(ValueError):
    """Invalid fuzz inventory, resource profile, seed, or runner definition."""


def parse_tsv(relative_path: str, expected_header: list[str]) -> list[dict[str, str]]:
    path = ROOT / relative_path
    try:
        with path.open(encoding="utf-8", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t", strict=True)
            if reader.fieldnames != expected_header:
                raise FuzzInventoryError(
                    f"{relative_path}: expected header {expected_header}, got {reader.fieldnames}"
                )
            rows: list[dict[str, str]] = []
            for line_number, row in enumerate(reader, start=2):
                if None in row or any(value is None for value in row.values()):
                    raise FuzzInventoryError(f"{relative_path}:{line_number}: wrong column count")
                if any(value == "" for value in row.values()):
                    raise FuzzInventoryError(f"{relative_path}:{line_number}: empty field")
                rows.append(dict(row))
            return rows
    except (OSError, csv.Error) as error:
        raise FuzzInventoryError(f"could not read {relative_path}: {error}") from error


def _unique(rows: Iterable[dict[str, str]], key: str, label: str) -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    for row in rows:
        value = row[key]
        if value in result:
            raise FuzzInventoryError(f"duplicate {label}: {value}")
        result[value] = row
    return result


def _checked_path(value: str, *, must_be_file: bool | None = None) -> Path:
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise FuzzInventoryError(f"path must be a safe workspace-relative path: {value}")
    resolved = ROOT / path
    if must_be_file is True and not resolved.is_file():
        raise FuzzInventoryError(f"required source or seed file does not exist: {value}")
    if must_be_file is False and not resolved.is_dir():
        raise FuzzInventoryError(f"required seed directory does not exist: {value}")
    if must_be_file is None and not resolved.exists():
        raise FuzzInventoryError(f"required file does not exist: {value}")
    return resolved


def _validate_resource_rows(rows: list[dict[str, str]]) -> dict[str, dict[str, str]]:
    resources = _unique(rows, "profile_id", "resource profile")
    for profile_id, row in resources.items():
        if not ID_PATTERN.fullmatch(profile_id):
            raise FuzzInventoryError(f"invalid resource profile identifier: {profile_id}")
        for field in RESOURCE_HEADER[1:]:
            try:
                value = int(row[field])
            except ValueError as error:
                raise FuzzInventoryError(f"{profile_id}: {field} must be an integer") from error
            if value <= 0:
                raise FuzzInventoryError(f"{profile_id}: {field} must be positive")
        if int(row["campaign_duration_seconds"]) != 86_400:
            raise FuzzInventoryError(f"{profile_id}: each campaign must request 24 hours")
        if int(row["job_wall_seconds"]) < int(row["campaign_duration_seconds"]) + 600:
            raise FuzzInventoryError(
                f"{profile_id}: wall limit must include a 10-minute campaign shutdown/reporting grace"
            )
        if int(row["workers"]) != 1:
            raise FuzzInventoryError(f"{profile_id}: independent jobs must use one worker")
    return resources


def _validate_runner_rows(
    rows: list[dict[str, str]], resources: dict[str, dict[str, str]]
) -> dict[str, dict[str, str]]:
    runners = _unique(rows, "runner_id", "runner")
    for runner_id, row in runners.items():
        if not ID_PATTERN.fullmatch(runner_id):
            raise FuzzInventoryError(f"invalid runner identifier: {runner_id}")
        if row["build_profile"] not in VALID_BUILD_PROFILES:
            raise FuzzInventoryError(f"{runner_id}: unknown build profile {row['build_profile']}")
        for field in ("build_command", "run_command"):
            if not row[field].startswith(("cargo test ", "pnpm ")):
                raise FuzzInventoryError(f"{runner_id}: {field} is not a supported command")
        for field in ("duration_env", "seed_env"):
            if not ENV_PATTERN.fullmatch(row[field]):
                raise FuzzInventoryError(f"{runner_id}: invalid environment variable in {field}")
        if row["seed_corpus_env"] != "-" and not ENV_PATTERN.fullmatch(row["seed_corpus_env"]):
            raise FuzzInventoryError(f"{runner_id}: invalid seed corpus environment variable")
        for field in ("report_env", "max_input_env", "input_timeout_env"):
            if not ENV_PATTERN.fullmatch(row[field]):
                raise FuzzInventoryError(f"{runner_id}: invalid environment variable in {field}")
        if row["target_selector_env"] != "-" and not ENV_PATTERN.fullmatch(
            row["target_selector_env"]
        ):
            raise FuzzInventoryError(f"{runner_id}: invalid target selector environment variable")
        if row["resource_profile"] not in resources:
            raise FuzzInventoryError(
                f"{runner_id}: unknown resource profile {row['resource_profile']}"
            )
        if row["manifest_schema"] != RUN_MANIFEST_SCHEMA:
            raise FuzzInventoryError(f"{runner_id}: run manifest schema is not canonical")
        _checked_path(row["manifest_schema"], must_be_file=True)
        if row["implementation_task"] not in {"M9-04", *VALID_FOLLOWUP_TASKS}:
            raise FuzzInventoryError(f"{runner_id}: no implementation task is assigned")
    return runners


def _validate_seed_refs(
    value: str, target_id: str, max_input_bytes: int
) -> None:
    for reference in value.split(";"):
        path_part = reference.split("#", maxsplit=1)[0]
        resolved = _checked_path(path_part)
        files = [item for item in resolved.rglob("*") if item.is_file()] if resolved.is_dir() else [resolved]
        if not files:
            raise FuzzInventoryError(f"{target_id}: seed directory is empty: {path_part}")
        for seed_file in files:
            if seed_file.suffix.lower() in {".rs", ".ts", ".js", ".py"}:
                raise FuzzInventoryError(
                    f"{target_id}: source code cannot serve as a raw seed corpus: {seed_file.relative_to(ROOT)}"
                )
            if seed_file.stat().st_size > max_input_bytes:
                raise FuzzInventoryError(
                    f"{target_id}: seed exceeds max_input_bytes ({max_input_bytes}): {seed_file.relative_to(ROOT)}"
                )
            if seed_file.suffix.lower() == ".json":
                try:
                    json.loads(seed_file.read_text(encoding="utf-8"))
                except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
                    raise FuzzInventoryError(
                        f"{target_id}: JSON seed is invalid: {seed_file.relative_to(ROOT)}"
                    ) from error
            if seed_file.suffix.lower() == ".hex":
                content = "".join(seed_file.read_text(encoding="ascii").split())
                if not content or len(content) % 2 or not re.fullmatch(r"[0-9a-fA-F]+", content):
                    raise FuzzInventoryError(
                        f"{target_id}: hex seed must contain an even number of hexadecimal digits: {seed_file.relative_to(ROOT)}"
                    )


def _validate_source_refs(value: str, target_id: str) -> None:
    for reference in value.split(";"):
        if "#" not in reference:
            raise FuzzInventoryError(f"{target_id}: source reference must be path#entrypoint")
        path_part, symbol = reference.split("#", maxsplit=1)
        source_path = _checked_path(path_part, must_be_file=True)
        source = source_path.read_text(encoding="utf-8")
        token = symbol.rsplit("::", maxsplit=1)[-1]
        if token not in source:
            raise FuzzInventoryError(
                f"{target_id}: entry point {symbol!r} was not found in {path_part}"
            )


def validate_tables(
    target_rows: list[dict[str, str]],
    decoder_rows: list[dict[str, str]],
    runner_rows: list[dict[str, str]],
    resource_rows: list[dict[str, str]],
    seed_rows: list[dict[str, str]],
) -> tuple[int, int]:
    resources = _validate_resource_rows(resource_rows)
    runners = _validate_runner_rows(runner_rows, resources)
    target_map = _unique(target_rows, "target_id", "fuzz target")
    decoder_map = _unique(decoder_rows, "decoder_id", "decoder target")
    seed_map = _unique(seed_rows, "seed_id", "seed strategy")

    missing_targets = sorted(EXPECTED_NON_CORE_TARGET_IDS - target_map.keys())
    if missing_targets:
        raise FuzzInventoryError(f"required parser targets are missing: {missing_targets}")
    if len(target_map) != len(EXPECTED_NON_CORE_TARGET_IDS):
        raise FuzzInventoryError(
            f"expected {len(EXPECTED_NON_CORE_TARGET_IDS)} non-core parser targets, found {len(target_map)}"
        )
    if len(decoder_map) != 92:
        raise FuzzInventoryError(f"expected 92 core/TypeScript decoder targets, found {len(decoder_map)}")

    used_runners: set[str] = set()
    for family, runner_id in DECODER_FAMILY_ROUTES.items():
        if runner_id not in runners:
            raise FuzzInventoryError(f"decoder family {family} has no runner {runner_id}")
    for decoder_id, row in decoder_map.items():
        runner_id = DECODER_FAMILY_ROUTES.get(row["family"])
        if runner_id is None:
            raise FuzzInventoryError(f"{decoder_id}: decoder family {row['family']} has no route")
        used_runners.add(runner_id)
        if "/" not in row["seed_id"]:
            raise FuzzInventoryError(f"{decoder_id}: seed_id must name a corpus and seed")
        corpus = row["seed_id"].split("/", maxsplit=1)[0]
        corpus_paths = {
            "value": "crates/worlddb-core/tests/data/wire-v1.0-golden.tsv",
            "frame": "crates/worlddb-core/tests/data/wire-v1.0-golden.tsv",
            "record": "crates/worlddb-core/tests/data/record-v1.0-golden.tsv",
            "record_ref": "crates/worlddb-core/tests/data/record-ref-v1.0-golden.tsv",
            "audit": "crates/worlddb-core/tests/data/audit-v1.0-golden.tsv",
            "text": "crates/worlddb-core/tests/data/text-parser-v1.0-golden.tsv",
            "core_bytes": "crates/worlddb-core/tests/data/core-bytes-v1.0-golden.tsv",
            "generated": "policy/fuzz-seeds/core/migration-run-journal.hex",
            "typescript": "bindings/typescript/test/data/transport-v1.0-golden.tsv",
        }
        if corpus not in corpus_paths:
            raise FuzzInventoryError(f"{decoder_id}: no seed corpus is registered for {corpus}")
        _validate_seed_refs(
            corpus_paths[corpus],
            decoder_id,
            int(resources[runners[runner_id]["resource_profile"]]["max_input_bytes"]),
        )

    required_seed_ids = {
        "canonical",
        "empty_input",
        "truncate_last",
        "flip_one_bit",
        "append_junk",
        "zero_one_byte",
        "nonminimal_prefix",
        "oversized_declared_length",
        "tight_resource_limits",
    }
    missing_seed_ids = sorted(required_seed_ids - seed_map.keys())
    if missing_seed_ids:
        raise FuzzInventoryError(f"decoder mutation strategies are missing: {missing_seed_ids}")

    for target_id, row in target_map.items():
        if not ID_PATTERN.fullmatch(target_id):
            raise FuzzInventoryError(f"invalid fuzz target identifier: {target_id}")
        if row["target_class"] not in VALID_TARGET_CLASSES:
            raise FuzzInventoryError(f"{target_id}: unknown target class {row['target_class']}")
        if row["resource_profile"] not in resources:
            raise FuzzInventoryError(
                f"{target_id}: unknown resource profile {row['resource_profile']}"
            )
        if row["runner_id"] not in runners:
            raise FuzzInventoryError(f"{target_id}: unknown runner {row['runner_id']}")
        if row["followup_task"] not in VALID_FOLLOWUP_TASKS:
            raise FuzzInventoryError(f"{target_id}: invalid follow-up task")
        expected_followup = {
            "import_parser": "M9-04b",
            "recovery_scanner": "M9-04c",
        }.get(row["target_class"], "M9-04a")
        if row["followup_task"] != expected_followup:
            raise FuzzInventoryError(
                f"{target_id}: target class {row['target_class']} must route to {expected_followup}"
            )
        _validate_source_refs(row["source_entrypoint"], target_id)
        _validate_seed_refs(
            row["seed_corpus"],
            target_id,
            int(resources[row["resource_profile"]]["max_input_bytes"]),
        )
        used_runners.add(row["runner_id"])

    unused = sorted(set(runners) - used_runners)
    if unused:
        raise FuzzInventoryError(f"runners have no assigned target: {unused}")
    if len(target_map) < 40:
        raise FuzzInventoryError("fewer than 40 non-core parser targets are registered")
    return len(decoder_map), len(target_map)


def verify() -> tuple[int, int]:
    decoder_rows = parse_tsv(DECODERS, DECODER_HEADER)
    seed_rows = parse_tsv(SEEDS, SEED_HEADER)
    return validate_tables(
        parse_tsv(TARGETS, TARGET_HEADER),
        decoder_rows,
        parse_tsv(RUNNERS, RUNNER_HEADER),
        parse_tsv(RESOURCES, RESOURCE_HEADER),
        seed_rows,
    )


def main() -> int:
    try:
        decoder_count, entrypoint_count = verify()
    except FuzzInventoryError as error:
        print(f"FUZZ TARGET INVENTORY ERROR: {error}", file=sys.stderr)
        return 1
    print(
        "FUZZ TARGET INVENTORY OK: "
        f"{decoder_count} core/TypeScript decoder targets, {entrypoint_count} additional parser targets, "
        "24-hour resource profiles and runner routes are complete"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
