"""Unit tests for clean-checkout CI job preflight and result validation."""

from __future__ import annotations

import unittest

from check_ci_matrix import load_ci_matrix
from run_ci_job import host_os, parse_summary, validate_outcome


class CiJobEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.row = next(row for row in load_ci_matrix() if row["os"] == host_os())
        self.good = {
            "actual_os": self.row["os"],
            "actual_rustc": "rustc 1.85.0 (4d91de4e4 2025-02-17)",
            "commit": "a" * 40,
            "clean_before": True,
            "clean_after": True,
            "exit_code": 0,
            "summary": {"profile": "dev", "passed": 26, "skipped": 1, "failed": 0},
            "skipped_steps": {"ci-matrix"},
            "expected_skips": {"ci-matrix"},
        }

    def validate(self, **overrides):
        values = dict(self.good)
        values.update(overrides)
        return validate_outcome(self.row, **values)

    def test_parses_exact_single_verify_summary(self):
        output = "[PASS] item\nVERIFY SUMMARY: profile=dev passed=26 skipped=1 failed=0\n"
        self.assertEqual(parse_summary(output), {"profile": "dev", "passed": 26, "skipped": 1, "failed": 0})

    def test_missing_or_ambiguous_summary_is_rejected(self):
        self.assertIsNone(parse_summary("VERIFY SUMMARY: profile=dev passed=1 skipped=0 failed=0\n" * 2))

    def test_matching_clean_msrv_job_passes(self):
        self.assertEqual(self.validate(), [])

    def test_wrong_os_or_toolchain_is_rejected(self):
        errors = self.validate(actual_os="unknown", actual_rustc="rustc 1.86.0 (abc)")
        self.assertTrue(any("host" in error for error in errors))
        self.assertTrue(any("Rust 1.85.0" in error for error in errors))

    def test_dirty_checkout_and_failed_verify_are_rejected(self):
        errors = self.validate(clean_before=False, clean_after=False, exit_code=1)
        self.assertTrue(any("clean checkout" in error for error in errors))
        self.assertTrue(any("exited with 1" in error for error in errors))

    def test_failure_summary_and_unexpected_skip_are_rejected(self):
        errors = self.validate(
            summary={"profile": "dev", "passed": 24, "skipped": 2, "failed": 1},
            skipped_steps={"ci-matrix", "other"},
        )
        self.assertTrue(any("failed step" in error for error in errors))
        self.assertTrue(any("unexpected skipped" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
