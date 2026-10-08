from __future__ import annotations

import copy
import json
import unittest
from pathlib import Path

from check_public_contracts import (
    BASELINE,
    ContractError,
    collect_current,
    compare_contracts,
    load_rules,
    parse_logical_limit,
    parse_core_to_public_codes,
    sha256_bytes,
    sha256_source_bytes,
)


class PublicContractClassificationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.current = collect_current()
        cls.rules = load_rules()

    def changes(self, mutate):
        candidate = copy.deepcopy(self.current)
        mutate(candidate)
        return compare_contracts(self.current, candidate, self.rules)

    def test_checked_in_rc_baseline_matches_every_tracked_source(self) -> None:
        self.assertTrue(BASELINE.exists(), "M9-01 must check in the immutable RC snapshot")
        baseline = json.loads(BASELINE.read_text(encoding="utf-8"))
        self.assertEqual([], compare_contracts(baseline, self.current, self.rules))

    def test_new_public_error_code_is_additive(self) -> None:
        changes = self.changes(
            lambda candidate: candidate["surfaces"]["public_errors"]["transport_codes"].append("NewSafeCode")
        )
        self.assertIn(("code_added", "ADDITIVE"), {(row["change_kind"], row["classification"]) for row in changes})

    def test_removed_public_error_code_is_breaking(self) -> None:
        changes = self.changes(
            lambda candidate: candidate["surfaces"]["public_errors"]["transport_codes"].pop()
        )
        self.assertIn(("code_removed", "BREAKING"), {(row["change_kind"], row["classification"]) for row in changes})

    def test_existing_core_error_remap_is_breaking(self) -> None:
        changes = self.changes(
            lambda candidate: candidate["surfaces"]["public_errors"]["core_to_transport"][0].__setitem__(
                "transport_code", "Internal"
            )
        )
        self.assertIn(("code_mapping_changed", "BREAKING"), {(row["change_kind"], row["classification"]) for row in changes})

    def test_multiline_core_error_mapping_is_included(self) -> None:
        parsed = parse_core_to_public_codes(
            'const CORE_TO_PUBLIC_CODES: &[(&str, &str)] = &[\n'
            '    (\n        "WDB-VALIDATION-UNSUPPORTED-OPERATION",\n'
            '        "UnsupportedOperation",\n    ),\n];'
        )
        self.assertEqual(
            [{"core_code": "WDB-VALIDATION-UNSUPPORTED-OPERATION", "transport_code": "UnsupportedOperation"}],
            parsed,
        )

    def test_closed_wire_tag_addition_is_breaking(self) -> None:
        changes = self.changes(
            lambda candidate: candidate["surfaces"]["wire_tags"]["record_ref_tags"].append(
                {"tag": "26", "variant": "NewReference", "id_type": "NewReferenceId"}
            )
        )
        self.assertIn(("closed_entry_added", "BREAKING"), {(row["change_kind"], row["classification"]) for row in changes})

    def test_format_major_is_breaking_and_compatible_minor_is_additive(self) -> None:
        breaking = self.changes(
            lambda candidate: candidate["surfaces"]["persistent_format"]["frame"].__setitem__("major", 2)
        )
        additive = self.changes(
            lambda candidate: candidate["surfaces"]["persistent_format"]["frame"].__setitem__("minor", 1)
        )
        self.assertIn(("major_changed", "BREAKING"), {(row["change_kind"], row["classification"]) for row in breaking})
        self.assertIn(("minor_increased", "ADDITIVE"), {(row["change_kind"], row["classification"]) for row in additive})

    def test_api_minor_addition_is_additive(self) -> None:
        changes = self.changes(
            lambda candidate: candidate["surfaces"]["rust_api_v1"]["protocol_version"].__setitem__("minor", 1)
        )
        self.assertIn(("protocol_minor_increased", "ADDITIVE"), {(row["change_kind"], row["classification"]) for row in changes})

    def test_existing_logical_export_semantic_change_is_breaking(self) -> None:
        changes = self.changes(
            lambda candidate: candidate["surfaces"]["logical_export"].__setitem__("semantics_sha256", "0" * 64)
        )
        self.assertIn(("semantics_changed", "BREAKING"), {(row["change_kind"], row["classification"]) for row in changes})

    def test_unstructured_api_source_drift_is_conservatively_breaking(self) -> None:
        changes = self.changes(
            lambda candidate: candidate["source_fingerprints"]["rust_api_v1"].__setitem__("source.rs", "0" * 64)
        )
        self.assertIn(("source_fingerprint_changed", "BREAKING"), {(row["change_kind"], row["classification"]) for row in changes})

    def test_source_fingerprints_normalize_crlf_without_changing_lone_cr(self) -> None:
        lf = b"first line\nsecond line\nthird\rline\n"
        crlf = b"first line\r\nsecond line\r\nthird\rline\r\n"
        self.assertEqual(sha256_bytes(lf), sha256_source_bytes(crlf))

    def test_logical_export_limits_accept_rust_digit_separators(self) -> None:
        self.assertEqual(1_000_000, parse_logical_limit("const LIMIT: usize = 1_000_000;", "LIMIT"))
        self.assertEqual(512 * 1024 * 1024, parse_logical_limit("const LIMIT: usize = 512 * 1024 * 1024;", "LIMIT"))

    def test_logical_export_limit_rejects_non_numeric_expressions(self) -> None:
        with self.assertRaises(ContractError):
            parse_logical_limit("const LIMIT: usize = BASE * 1024;", "LIMIT")


if __name__ == "__main__":
    unittest.main()
