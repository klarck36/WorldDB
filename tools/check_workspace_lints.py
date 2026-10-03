"""Enforce workspace-wide Rust lint inheritance and the declared format profile."""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_MEMBERS = {
    "crates/worlddb-core": "worlddb-core",
    "crates/worlddb-storage-file": "worlddb-storage-file",
    "crates/worlddb-process-adapter": "worlddb-process-adapter",
    "crates/worlddb-cli": "worlddb-cli",
    "crates/worlddb-testkit": "worlddb-testkit",
    "xtask": "xtask",
}
REQUIRED_CLIPPY_LINTS = {
    "correctness",
    "suspicious",
    "panic",
    "unwrap_used",
    "expect_used",
    "indexing_slicing",
}


def validate_workspace(
    root_manifest_text: str,
    member_manifest_texts: dict[str, str],
    member_sources: dict[str, str],
    rustfmt_text: str,
) -> list[str]:
    errors: list[str] = []
    try:
        root = tomllib.loads(root_manifest_text)
    except tomllib.TOMLDecodeError as error:
        return [f"root Cargo.toml is invalid TOML: {error}"]
    try:
        rustfmt = tomllib.loads(rustfmt_text)
    except tomllib.TOMLDecodeError as error:
        return [f"rustfmt.toml is invalid TOML: {error}"]

    workspace = root.get("workspace", {})
    if set(workspace.get("members", [])) != set(EXPECTED_MEMBERS):
        errors.append("workspace members do not match the approved M0/M7 crate layout")
    lint_tables = workspace.get("lints", {})
    if lint_tables.get("rust", {}).get("unsafe_code") != "deny":
        errors.append("workspace rust lints must deny unsafe_code")
    clippy = lint_tables.get("clippy", {})
    for lint in sorted(REQUIRED_CLIPPY_LINTS):
        if clippy.get(lint) != "deny":
            errors.append(f"workspace clippy lint {lint} must be deny")

    for member_path, expected_name in EXPECTED_MEMBERS.items():
        manifest_text = member_manifest_texts.get(member_path)
        if manifest_text is None:
            errors.append(f"missing member manifest for {member_path}")
            continue
        try:
            manifest = tomllib.loads(manifest_text)
        except tomllib.TOMLDecodeError as error:
            errors.append(f"{member_path}/Cargo.toml is invalid TOML: {error}")
            continue
        package_name = manifest.get("package", {}).get("name")
        if package_name != expected_name:
            errors.append(f"{member_path} must declare package {expected_name}")
        if manifest.get("lints", {}).get("workspace") is not True:
            errors.append(f"{member_path} must explicitly inherit workspace lints")

    if "#![forbid(unsafe_code)]" not in member_sources.get("crates/worlddb-core/src/lib.rs", ""):
        errors.append("worlddb-core must forbid unsafe_code at crate level")
    if "#![deny(unsafe_code)]" not in member_sources.get(
        "crates/worlddb-storage-file/src/lib.rs", ""
    ):
        errors.append("worlddb-storage-file must deny unsafe_code and remain locally overridable")
    if "#![deny(unsafe_code)]" not in member_sources.get(
        "crates/worlddb-process-adapter/src/lib.rs", ""
    ):
        errors.append("worlddb-process-adapter must deny unsafe_code and remain locally overridable")
    if rustfmt.get("edition") != "2024":
        errors.append("rustfmt.toml must declare edition 2024")
    if not isinstance(rustfmt.get("max_width"), int) or rustfmt["max_width"] < 40:
        errors.append("rustfmt.toml must set a usable max_width")
    return errors


def workspace_inputs(root: Path) -> tuple[str, dict[str, str], dict[str, str], str]:
    root_manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
    member_manifests = {
        member: (root / member / "Cargo.toml").read_text(encoding="utf-8")
        for member in EXPECTED_MEMBERS
    }
    member_sources = {
        member: (root / member).read_text(encoding="utf-8")
        for member in (
            "crates/worlddb-core/src/lib.rs",
            "crates/worlddb-storage-file/src/lib.rs",
            "crates/worlddb-process-adapter/src/lib.rs",
        )
    }
    rustfmt = (root / "rustfmt.toml").read_text(encoding="utf-8")
    return root_manifest, member_manifests, member_sources, rustfmt


def main() -> int:
    try:
        inputs = workspace_inputs(ROOT)
    except OSError as error:
        print(f"WORKSPACE LINT ERROR: {error}", file=sys.stderr)
        return 1
    errors = validate_workspace(*inputs)
    if errors:
        for error in errors:
            print(f"WORKSPACE LINT ERROR: {error}", file=sys.stderr)
        return 1
    print("WORKSPACE LINTS OK: all members inherit deny policies; core forbids unsafe; platform adapters deny unsafe")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
