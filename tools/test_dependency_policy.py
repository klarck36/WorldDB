"""Negative and fixture tests for M0-12 dependency/feature policy."""

from __future__ import annotations

import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from check_dependency_policy import (
    CRATES_IO_SOURCE,
    core_error_erasure_findings,
    validate_dependency_register,
    validate_feature_conflicts,
)


def metadata_fixture(*, enabled: set[str] | None = None) -> dict:
    enabled = enabled or set()
    workspace_id = "worlddb-fixture 0.1.0 (path+file:///fixture)"
    dependency_id = "fixture-lib 1.2.3 (registry+https://github.com/rust-lang/crates.io-index)"
    return {
        "workspace_members": [workspace_id],
        "packages": [
            {
                "id": workspace_id,
                "name": "worlddb-fixture",
                "version": "0.1.0",
                "rust_version": "1.85",
                "features": {},
                "targets": [],
            },
            {
                "id": dependency_id,
                "name": "fixture-lib",
                "version": "1.2.3",
                "source": CRATES_IO_SOURCE,
                "license": "MIT",
                "rust_version": "1.70",
                "features": {"cache": [], "sqlite": []},
                "targets": [{"kind": ["lib"]}],
            },
        ],
        "resolve": {
            "nodes": [
                {"id": workspace_id, "features": [], "deps": [{"pkg": dependency_id, "dep_kinds": [{"kind": None}]}]},
                {"id": dependency_id, "features": sorted(enabled), "deps": []},
            ]
        },
    }


def dependency_row(**overrides: str) -> dict[str, str]:
    row = {
        "name": "fixture-lib",
        "version": "1.2.3",
        "source": CRATES_IO_SOURCE,
        "boundary_benefit": "validated adapter boundary",
        "maintainer_release_review": "release cadence and maintainer activity reviewed",
        "license_review": "approved SPDX: MIT",
        "advisory_review": "cargo-deny advisory database checked",
        "transitive_external_nodes": "0",
        "msrv_review": "declared: 1.70; locked build on 1.85",
        "unsafe_review": "reviewed source for unsafe blocks",
        "build_procmacro_review": "none",
        "default_feature_review": "disabled and checked by cargo-deny",
        "feature_review": "all-features graph reviewed",
        "exit_plan": "replace with local adapter implementation",
        "update_owner": "Storage Maintainer",
        "reviewed_on": "2026-09-29",
    }
    row.update(overrides)
    return row


class DependencyRegisterTests(unittest.TestCase):
    def test_empty_workspace_has_no_external_register_rows(self):
        metadata = metadata_fixture()
        metadata["packages"] = metadata["packages"][:1]
        metadata["resolve"]["nodes"] = metadata["resolve"]["nodes"][:1]
        self.assertEqual(validate_dependency_register(metadata, []), [])

    def test_new_external_dependency_without_review_fails(self):
        errors = validate_dependency_register(metadata_fixture(), [])
        self.assertTrue(any("review is missing" in error for error in errors))

    def test_exact_reviewed_lockfile_package_passes(self):
        self.assertEqual(
            validate_dependency_register(metadata_fixture(), [dependency_row()]), []
        )

    def test_changed_locked_version_rejects_stale_review(self):
        errors = validate_dependency_register(
            metadata_fixture(), [dependency_row(version="1.2.2")]
        )
        self.assertTrue(any("no longer matches the lockfile" in error for error in errors))

    def test_transitive_size_must_match_resolved_graph(self):
        errors = validate_dependency_register(
            metadata_fixture(), [dependency_row(transitive_external_nodes="1")]
        )
        self.assertTrue(any("lockfile graph has 0" in error for error in errors))

    def test_dependency_above_workspace_msrv_fails(self):
        metadata = metadata_fixture()
        metadata["packages"][1]["rust_version"] = "1.86"
        errors = validate_dependency_register(metadata, [dependency_row()])
        self.assertTrue(any("above workspace MSRV 1.85" in error for error in errors))


class FeatureConflictTests(unittest.TestCase):
    def test_enabled_forbidden_pair_fails(self):
        metadata = metadata_fixture(enabled={"cache", "sqlite"})
        conflicts = [
            {
                "crate": "fixture-lib",
                "feature_a": "cache",
                "feature_b": "sqlite",
                "reason": "the two storage backends cannot be active together",
            }
        ]
        errors = validate_feature_conflicts(metadata, conflicts)
        self.assertTrue(any("forbidden feature combination cache + sqlite" in error for error in errors))

    def test_undeclared_conflict_feature_fails_closed(self):
        conflicts = [
            {
                "crate": "fixture-lib",
                "feature_a": "cache",
                "feature_b": "imaginary",
                "reason": "fixture conflict",
            }
        ]
        errors = validate_feature_conflicts(metadata_fixture(), conflicts)
        self.assertTrue(any("does not declare feature 'imaginary'" in error for error in errors))


class CoreErrorErasureTests(unittest.TestCase):
    def test_boxed_dynamic_error_in_core_is_rejected(self):
        findings = core_error_erasure_findings(
            {"crates/worlddb-core/src/lib.rs": "pub fn run() -> Result<(), Box<dyn std::error::Error>> { todo!() }"}
        )
        self.assertEqual(findings, ["crates/worlddb-core/src/lib.rs"])

    def test_typed_domain_error_is_allowed(self):
        self.assertEqual(
            core_error_erasure_findings(
                {"crates/worlddb-core/src/lib.rs": "pub fn run() -> Result<(), DomainError> { todo!() }"}
            ),
            [],
        )


class CargoDenyNegativeTests(unittest.TestCase):
    def test_banned_dependency_makes_cargo_deny_fail(self):
        cargo = shutil.which("cargo")
        cargo_deny = shutil.which("cargo-deny")
        if cargo is None or cargo_deny is None:
            self.fail("cargo and cargo-deny must be installed for the dependency policy tests")

        with tempfile.TemporaryDirectory(prefix="worlddb-m0-12-banned-") as temporary:
            root = Path(temporary)
            bad = root / "bad"
            (bad / "src").mkdir(parents=True)
            (bad / "Cargo.toml").write_text(
                '[package]\nname = "anyhow"\nversion = "1.0.0"\nedition = "2024"\n',
                encoding="utf-8",
            )
            (bad / "src" / "lib.rs").write_text("pub fn fixture() {}\n", encoding="utf-8")
            manifest = root / "Cargo.toml"
            manifest.write_text(
                '[package]\nname = "worlddb-policy-fixture"\nversion = "0.1.0"\nedition = "2024"\n'
                '[dependencies]\nanyhow = { path = "bad", default-features = false }\n',
                encoding="utf-8",
            )
            (root / "src").mkdir()
            (root / "src" / "lib.rs").write_text("pub fn fixture() {}\n", encoding="utf-8")
            generated = subprocess.run(
                [cargo, "generate-lockfile", "--manifest-path", str(manifest)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(generated.returncode, 0, generated.stdout + generated.stderr)
            deny_config = Path(__file__).resolve().parents[1] / ".cargo" / "deny.toml"
            result = subprocess.run(
                [
                    cargo_deny,
                    "--manifest-path",
                    str(manifest),
                    "--config",
                    str(deny_config),
                    "--workspace",
                    "--locked",
                    "check",
                    "bans",
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("anyhow", result.stdout + result.stderr)

    def test_external_default_features_require_explicit_review(self):
        cargo = shutil.which("cargo")
        cargo_deny = shutil.which("cargo-deny")
        if cargo is None or cargo_deny is None:
            self.fail("cargo and cargo-deny must be installed for the dependency policy tests")

        with tempfile.TemporaryDirectory(prefix="worlddb-m0-12-default-feature-") as temporary:
            root = Path(temporary)
            dependency = root / "feature-lib"
            (dependency / "src").mkdir(parents=True)
            (dependency / "Cargo.toml").write_text(
                '[package]\nname = "fixture-lib"\nversion = "0.1.0"\nedition = "2024"\n'
                '[features]\ndefault = ["extra"]\nextra = []\n',
                encoding="utf-8",
            )
            (dependency / "src" / "lib.rs").write_text("pub fn fixture() {}\n", encoding="utf-8")
            manifest = root / "Cargo.toml"
            manifest.write_text(
                '[package]\nname = "worlddb-default-feature-fixture"\nversion = "0.1.0"\nedition = "2024"\n'
                '[dependencies]\nfixture-lib = { path = "feature-lib" }\n',
                encoding="utf-8",
            )
            (root / "src").mkdir()
            (root / "src" / "lib.rs").write_text("pub fn fixture() {}\n", encoding="utf-8")
            generated = subprocess.run(
                [cargo, "generate-lockfile", "--manifest-path", str(manifest)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(generated.returncode, 0, generated.stdout + generated.stderr)
            deny_config = Path(__file__).resolve().parents[1] / ".cargo" / "deny.toml"
            result = subprocess.run(
                [
                    cargo_deny,
                    "--manifest-path",
                    str(manifest),
                    "--config",
                    str(deny_config),
                    "--workspace",
                    "--locked",
                    "check",
                    "bans",
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("fixture-lib", result.stdout + result.stderr)


class CargoFeatureMatrixNegativeTests(unittest.TestCase):
    def test_all_features_rejects_an_incompatible_feature_pair(self):
        cargo = shutil.which("cargo")
        if cargo is None:
            self.fail("cargo must be installed for the feature matrix tests")

        with tempfile.TemporaryDirectory(prefix="worlddb-m0-12-feature-pair-") as temporary:
            root = Path(temporary)
            manifest = root / "Cargo.toml"
            manifest.write_text(
                '[package]\nname = "feature-pair-fixture"\nversion = "0.1.0"\nedition = "2024"\n'
                '[features]\ndefault = []\ncache-backend = []\nsql-backend = []\n',
                encoding="utf-8",
            )
            (root / "src").mkdir()
            (root / "src" / "lib.rs").write_text(
                '#[cfg(all(feature = "cache-backend", feature = "sql-backend"))]\n'
                'compile_error!("these backends cannot be enabled together");\n',
                encoding="utf-8",
            )
            generated = subprocess.run(
                [cargo, "generate-lockfile", "--manifest-path", str(manifest)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(generated.returncode, 0, generated.stdout + generated.stderr)
            environment = dict(__import__("os").environ)
            environment["CARGO_TARGET_DIR"] = str(root / "target")
            result = subprocess.run(
                [cargo, "check", "--manifest-path", str(manifest), "--locked", "--all-features"],
                capture_output=True,
                text=True,
                check=False,
                env=environment,
            )
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("these backends cannot be enabled together", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
