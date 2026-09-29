"""Mutation tests for the M0-14 provider-neutral CI matrix contract."""

from __future__ import annotations

import copy
import unittest

from check_ci_matrix import (
    EXPECTED_FEATURES,
    load_ci_matrix,
    rust_channel,
    validate_matrix,
    verify_manifest_state,
)
from run_feature_matrix import load_matrix


class CiMatrixContractTests(unittest.TestCase):
    def setUp(self):
        self.rows = load_ci_matrix()
        self.features = {name for name, _args in load_matrix()}
        self.verify_profiles, self.verify_steps = verify_manifest_state()

    def validate(self, rows=None, **overrides):
        values = {
            "declared_toolchain": rust_channel(),
            "feature_profiles": self.features,
            "verify_profiles": self.verify_profiles,
            "verify_steps": self.verify_steps,
        }
        values.update(overrides)
        return validate_matrix(copy.deepcopy(self.rows if rows is None else rows), **values)

    def test_checked_in_matrix_covers_required_platforms_and_profiles(self):
        self.assertEqual(self.validate(), [])

    def test_missing_required_os_is_rejected(self):
        self.assertTrue(any("expected 3 platform jobs" in error for error in self.validate(self.rows[:2])))

    def test_duplicate_platform_rows_are_rejected(self):
        rows = [*self.rows, dict(self.rows[0])]
        self.assertTrue(any("platform jobs" in error or "exactly one matrix row" in error for error in self.validate(rows)))

    def test_toolchain_must_match_the_pinned_channel(self):
        rows = copy.deepcopy(self.rows)
        rows[0]["rust_toolchain"] = "stable"
        self.assertTrue(any("toolchain differs" in error for error in self.validate(rows)))

    def test_all_feature_profiles_must_be_covered(self):
        rows = copy.deepcopy(self.rows)
        rows[0]["feature_profiles"] = ",".join(EXPECTED_FEATURES[:2])
        self.assertTrue(any("feature profiles must be" in error for error in self.validate(rows)))

    def test_job_must_start_clean_and_archive_the_step_manifest(self):
        rows = copy.deepcopy(self.rows)
        rows[0]["checkout"] = "working-tree"
        rows[1]["artifact_manifest"] = "artifacts/other.tsv"
        errors = self.validate(rows)
        self.assertTrue(any("clean checkout" in error for error in errors))
        self.assertTrue(any("must archive tools/verify/steps.tsv" in error for error in errors))

    def test_jobs_must_invoke_the_canonical_verify_command(self):
        rows = copy.deepcopy(self.rows)
        rows[2]["verify_command"] = "cargo test"
        self.assertTrue(any("cargo xtask verify" in error for error in self.validate(rows)))


if __name__ == "__main__":
    unittest.main()
