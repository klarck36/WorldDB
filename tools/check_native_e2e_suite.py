"""Fail-closed validation for the shared native desktop E2E case catalog."""

from __future__ import annotations

import json
import re
from pathlib import Path, PurePosixPath
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
ODE_ROOT = ROOT / "experiments" / "ode-002"
SUITE_PATH = ODE_ROOT / "e2e" / "native-suite.json"
PLATFORM_FILESYSTEMS = {"windows": "NTFS", "macos": "APFS", "linux": "ext4"}
SCENARIO_MODES = {
    "native_ipc": {"in-process", "sidecar"},
    "competing_process": {"in-process", "sidecar"},
    "keyboard_navigation": {"in-process", "sidecar"},
    "commit_crash_recovery": {"in-process", "sidecar"},
}
SUITE_KEYS = {"schema_version", "suite_id", "suite_version", "task_id", "platform_profiles", "cases"}
PROFILE_KEYS = {"filesystem", "runner", "state"}
CASE_KEYS = {"id", "scenario", "scope", "drivers", "modes", "required"}
PLATFORMS = set(PLATFORM_FILESYSTEMS)


def load_suite(path: Path = SUITE_PATH) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _resolve_repo_path(value: Any, root: Path, description: str, errors: list[str]) -> Path | None:
    if not isinstance(value, str) or not value.strip():
        errors.append(f"{description} must be a non-empty repository-relative path")
        return None
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or "\\" in value or ":" in value:
        errors.append(f"{description} must not be absolute or escape its repository root")
        return None
    resolved = (root / Path(*path.parts)).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError:
        errors.append(f"{description} escapes its repository root")
        return None
    return resolved


def validate_suite(suite: Any, suite_root: Path = ODE_ROOT) -> list[str]:
    errors: list[str] = []
    if not isinstance(suite, dict):
        return ["suite must be a JSON object"]
    if set(suite) != SUITE_KEYS:
        errors.append(f"suite fields must be exactly {sorted(SUITE_KEYS)}")
    if type(suite.get("schema_version")) is not int or suite.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if suite.get("suite_id") != "worlddb-ode-native-e2e":
        errors.append("suite_id must be worlddb-ode-native-e2e")
    if not isinstance(suite.get("suite_version"), str) or not suite["suite_version"].strip():
        errors.append("suite_version must be a non-empty string")
    if suite.get("task_id") != "M8-26":
        errors.append("task_id must be M8-26")

    profiles = suite.get("platform_profiles")
    if not isinstance(profiles, dict) or set(profiles) != PLATFORMS:
        errors.append("platform_profiles must declare windows, macos, and linux exactly once")
        profiles = profiles if isinstance(profiles, dict) else {}
    for platform, filesystem in PLATFORM_FILESYSTEMS.items():
        profile = profiles.get(platform)
        if not isinstance(profile, dict):
            errors.append(f"platform_profiles.{platform} must be an object")
            continue
        state = profile.get("state")
        expected_keys = PROFILE_KEYS | ({"deferred_until"} if state == "deferred" else set())
        if set(profile) != expected_keys:
            errors.append(f"platform_profiles.{platform} fields must be exactly {sorted(expected_keys)}")
        if profile.get("filesystem") != filesystem:
            errors.append(f"platform_profiles.{platform}.filesystem must be {filesystem}")
        if not isinstance(state, str) or state not in {"available", "deferred"}:
            errors.append(f"platform_profiles.{platform}.state must be available or deferred")
        elif state == "available":
            runner = _resolve_repo_path(profile.get("runner"), suite_root, f"platform_profiles.{platform}.runner", errors)
            if runner is not None and not runner.is_file():
                errors.append(f"platform_profiles.{platform}.runner does not exist: {profile.get('runner')}")
        else:
            if profile.get("runner") is not None:
                errors.append(f"deferred platform {platform} must not declare a runnable suite entry point")
            if not isinstance(profile.get("deferred_until"), str) or not profile["deferred_until"].strip():
                errors.append(f"deferred platform {platform} must declare deferred_until")

    cases = suite.get("cases")
    if not isinstance(cases, list) or not cases:
        errors.append("cases must be a non-empty array")
        return errors
    seen_ids: set[str] = set()
    seen_pairs: set[tuple[str, str]] = set()
    coverage = {scenario: set() for scenario in SCENARIO_MODES}
    for index, case in enumerate(cases):
        label = f"cases[{index}]"
        if not isinstance(case, dict):
            errors.append(f"{label} must be an object")
            continue
        if set(case) != CASE_KEYS:
            errors.append(f"{label} fields must be exactly {sorted(CASE_KEYS)}")
        case_id = case.get("id")
        if not isinstance(case_id, str) or not re.fullmatch(r"[a-z][a-z0-9_]*", case_id):
            errors.append(f"{label}.id must match [a-z][a-z0-9_]*")
        elif case_id in seen_ids:
            errors.append(f"duplicate case id {case_id}")
        else:
            seen_ids.add(case_id)
        scenario = case.get("scenario")
        if not isinstance(scenario, str) or scenario not in SCENARIO_MODES:
            errors.append(f"{label}.scenario is not a registered native scenario")
        if not isinstance(case.get("scope"), str) or not case["scope"].strip():
            errors.append(f"{label}.scope must be a non-empty string")
        if case.get("required") is not True:
            errors.append(f"{label}.required must be true for every core native E2E case")

        modes = case.get("modes")
        if not isinstance(modes, list) or not modes or any(
            not isinstance(mode, str) or mode not in {"in-process", "sidecar"} for mode in modes
        ):
            errors.append(f"{label}.modes must be a non-empty list of supported process modes")
        elif len(set(modes)) != len(modes):
            errors.append(f"{label}.modes must not contain duplicates")
        elif isinstance(scenario, str) and scenario in SCENARIO_MODES:
            for mode in modes:
                pair = (scenario, mode)
                if mode not in SCENARIO_MODES[scenario]:
                    errors.append(f"{label} declares unsupported mode {mode} for {scenario}")
                if pair in seen_pairs:
                    errors.append(f"scenario {scenario} / mode {mode} is declared more than once")
                seen_pairs.add(pair)
                coverage[scenario].add(mode)

        drivers = case.get("drivers")
        if not isinstance(drivers, dict) or set(drivers) != PLATFORMS:
            errors.append(f"{label}.drivers must declare windows, macos, and linux exactly once")
            continue
        for platform in PLATFORM_FILESYSTEMS:
            profile = profiles.get(platform)
            state = profile.get("state") if isinstance(profile, dict) else None
            driver = drivers.get(platform)
            if state == "deferred":
                if driver is not None:
                    errors.append(f"{label}.drivers.{platform} must be null while that platform is deferred")
                continue
            if state == "available":
                path = _resolve_repo_path(driver, suite_root, f"{label}.drivers.{platform}", errors)
                if path is not None and not path.is_file():
                    errors.append(f"{label}.drivers.{platform} does not exist: {driver}")

    for scenario, expected_modes in SCENARIO_MODES.items():
        if coverage[scenario] != expected_modes:
            errors.append(
                f"scenario {scenario} must cover {sorted(expected_modes)} exactly once; "
                f"found {sorted(coverage[scenario])}"
            )
    return errors


def main() -> int:
    try:
        suite = json.loads(SUITE_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"NATIVE E2E SUITE ERROR: {error}")
        return 1
    errors = validate_suite(suite)
    if errors:
        for error in errors:
            print(f"NATIVE E2E SUITE ERROR: {error}")
        return 1
    print("NATIVE E2E SUITE OK: shared scenarios and platform drivers are consistent")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
