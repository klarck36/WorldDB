"""Tests for the migration transformer's static capability boundary."""

from __future__ import annotations

import unittest

from check_migration_transform import violations


class MigrationTransformPolicyTests(unittest.TestCase):
    def test_closed_transform_imports_only_approved_path_roots(self) -> None:
        source = "use crate::ids::TimelineId; use std::fmt; blake3::Hasher::new();"
        self.assertEqual(violations(source), [])

    def test_clock_random_network_locale_ai_and_callbacks_are_rejected(self) -> None:
        forbidden = (
            "SystemTime::now(); rand::random(); reqwest::get(url); "
            "locale::current(); openai::complete(); fn transform(f: impl Fn());"
        )
        errors = violations(forbidden)
        self.assertIn("forbidden clock access", errors)
        self.assertIn("unapproved external path root rand::", errors)
        self.assertIn("forbidden network access", errors)
        self.assertIn("forbidden locale access", errors)
        self.assertIn("forbidden AI or model client", errors)
        self.assertIn("forbidden injected callback", errors)

    def test_filesystem_and_environment_apis_are_rejected(self) -> None:
        errors = violations("std::env::var(\"TZ\"); std::fs::read(path);")
        self.assertIn("forbidden environment or filesystem access", errors)


if __name__ == "__main__":
    unittest.main()
