"""Mutation tests for the shared M8-26 native E2E suite contract."""

from __future__ import annotations

import copy
import unittest

from check_native_e2e_suite import load_suite, validate_suite


class NativeE2eSuiteTests(unittest.TestCase):
    def setUp(self):
        self.suite = load_suite()

    def validate(self, suite=None):
        return validate_suite(copy.deepcopy(self.suite if suite is None else suite))

    def test_checked_in_suite_has_complete_shared_scenario_coverage(self):
        self.assertEqual(self.validate(), [])

    def test_duplicate_case_id_is_rejected(self):
        suite = copy.deepcopy(self.suite)
        suite["cases"][1]["id"] = suite["cases"][0]["id"]
        self.assertTrue(any("duplicate case id" in error for error in self.validate(suite)))

    def test_missing_process_mode_is_rejected(self):
        suite = copy.deepcopy(self.suite)
        suite["cases"] = [case for case in suite["cases"] if case["id"] != "native_ipc_sidecar"]
        self.assertTrue(any("native_ipc must cover" in error for error in self.validate(suite)))

    def test_deferred_platform_cannot_claim_a_driver(self):
        suite = copy.deepcopy(self.suite)
        suite["cases"][0]["drivers"]["macos"] = "scripts/run-macos-smoke.sh"
        self.assertTrue(any("must be null while that platform is deferred" in error for error in self.validate(suite)))

    def test_case_driver_cannot_escape_the_suite_root(self):
        suite = copy.deepcopy(self.suite)
        suite["cases"][0]["drivers"]["windows"] = "../../outside.ps1"
        self.assertTrue(any("must not be absolute or escape" in error for error in self.validate(suite)))

    def test_unknown_scenario_is_rejected(self):
        suite = copy.deepcopy(self.suite)
        suite["cases"][0]["scenario"] = "browser_mock"
        self.assertTrue(any("not a registered native scenario" in error for error in self.validate(suite)))

    def test_malformed_scenario_type_is_rejected_without_crashing(self):
        suite = copy.deepcopy(self.suite)
        suite["cases"][0]["scenario"] = []
        self.assertTrue(any("not a registered native scenario" in error for error in self.validate(suite)))

    def test_schema_version_rejects_boolean_values(self):
        suite = copy.deepcopy(self.suite)
        suite["schema_version"] = True
        self.assertTrue(any("schema_version must be 1" in error for error in self.validate(suite)))

    def test_unavailable_windows_driver_is_rejected(self):
        suite = copy.deepcopy(self.suite)
        suite["cases"][0]["drivers"]["windows"] = "scripts/missing-smoke.ps1"
        self.assertTrue(any("does not exist" in error for error in self.validate(suite)))


if __name__ == "__main__":
    unittest.main()
