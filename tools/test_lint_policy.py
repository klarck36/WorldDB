from contextlib import redirect_stderr
from datetime import date
from io import StringIO
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import check_exceptions
import check_terminology
import check_unsafe_policy
from check_exceptions import validate_policy
from check_terminology import classify_text
from check_workspace_lints import workspace_inputs, validate_workspace


EXCEPTION_HEADER = "exception_id\towner\treason\texpires_on\n"


class ExceptionPolicyTests(unittest.TestCase):
    def test_empty_register_passes_when_there_are_no_local_allows(self):
        self.assertEqual(validate_policy(EXCEPTION_HEADER, {}, date(2026, 9, 29)), [])

    def test_expired_registered_exception_fails(self):
        register = EXCEPTION_HEADER + "WDB-EXC-0001\towner\ttemporary lint exception\t2026-09-28\n"
        source = '#[allow(dead_code, reason = "WDB-EXC-0001")]\nfn example() {}\n'
        errors = validate_policy(register, {"crates/example.rs": source}, date(2026, 9, 29))
        self.assertTrue(any("expired" in error for error in errors))

    def test_expired_exception_makes_the_verify_step_fail(self):
        with tempfile.TemporaryDirectory(prefix="worlddb-expired-exception-") as directory:
            root = Path(directory)
            (root / "policy").mkdir()
            (root / "crates").mkdir()
            register = root / "policy" / "exceptions.tsv"
            register.write_text(
                EXCEPTION_HEADER
                + "WDB-EXC-0004\towner\tfixture exception\t2000-01-01\n",
                encoding="utf-8",
            )
            (root / "crates" / "example.rs").write_text(
                '#[allow(dead_code, reason = "WDB-EXC-0004")]\nfn example() {}\n',
                encoding="utf-8",
            )
            diagnostics = StringIO()
            with patch.object(check_exceptions, "ROOT", root), patch.object(
                check_exceptions, "REGISTER", register
            ), redirect_stderr(diagnostics):
                exit_code = check_exceptions.main()
        self.assertEqual(exit_code, 1)
        self.assertIn("expired", diagnostics.getvalue())

    def test_unregistered_or_reasonless_local_allow_fails(self):
        source = '#[allow(dead_code, reason = "WDB-EXC-0002")]\nfn a() {}\n#[allow(unused)]\nfn b() {}\n'
        errors = validate_policy(EXCEPTION_HEADER, {"crates/example.rs": source}, date(2026, 9, 29))
        self.assertTrue(any("not registered" in error for error in errors))
        self.assertTrue(any("exactly one reason" in error for error in errors))

    def test_registered_exception_must_be_used(self):
        register = EXCEPTION_HEADER + "WDB-EXC-0003\towner\treason\t2026-12-31\n"
        errors = validate_policy(register, {}, date(2026, 9, 29))
        self.assertTrue(any("no Rust allow attribute" in error for error in errors))


class TerminologyPolicyTests(unittest.TestCase):
    def test_normative_legacy_type_name_is_unclassified(self):
        findings = classify_text("crates/worlddb-core/src/lib.rs", "pub struct BranchId;\n")
        self.assertTrue(any(term == "BranchId" and classification is None for _, term, classification in findings))

    def test_wrong_term_makes_the_verify_step_fail(self):
        with tempfile.TemporaryDirectory(prefix="worlddb-terminology-") as directory:
            root = Path(directory)
            source = root / "crates" / "example.rs"
            source.parent.mkdir(parents=True)
            source.write_text("pub struct BranchId;\n", encoding="utf-8")
            diagnostics = StringIO()
            with patch.object(check_terminology, "ROOT", root), redirect_stderr(diagnostics):
                exit_code = check_terminology.main()
        self.assertEqual(exit_code, 1)
        self.assertIn("unclassified legacy term BranchId", diagnostics.getvalue())

    def test_historical_and_negative_contexts_are_classified(self):
        historical = classify_text("crates/example.rs", "// Historical UI alias: BranchId.\n")
        negative = classify_text("crates/example.rs", "// Explicit negative rule: BranchId does not exist.\n")
        self.assertTrue(any(classification == "historical-explanation" for _, _, classification in historical))
        self.assertTrue(any(classification == "explicit-negative-rule" for _, _, classification in negative))

    def test_adr_and_compile_fail_contexts_are_classified(self):
        adr = classify_text("docs/contract/ADR-021-historyspace.md", "Decision: BranchId is not a domain type.\n")
        compile_fail = classify_text("tests/compile_fail/branch_id.rs", "pub struct BranchId;\n")
        self.assertTrue(any(classification == "adr-decision" for _, _, classification in adr))
        self.assertTrue(any(classification == "compile-fail" for _, _, classification in compile_fail))


class WorkspaceLintPolicyTests(unittest.TestCase):
    def test_workspace_configuration_and_member_inheritance_pass(self):
        root = Path(__file__).resolve().parents[1]
        self.assertEqual(validate_workspace(*workspace_inputs(root)), [])

    def test_member_without_workspace_lint_inheritance_fails(self):
        root = Path(__file__).resolve().parents[1]
        root_manifest, members, sources, rustfmt = workspace_inputs(root)
        members["crates/worlddb-cli"] = members["crates/worlddb-cli"].replace(
            "[lints]\nworkspace = true\n", ""
        )
        errors = validate_workspace(root_manifest, members, sources, rustfmt)
        self.assertTrue(any("crates/worlddb-cli must explicitly inherit" in error for error in errors))

    def test_missing_workspace_unsafe_deny_fails(self):
        root = Path(__file__).resolve().parents[1]
        root_manifest, members, sources, rustfmt = workspace_inputs(root)
        root_manifest = root_manifest.replace('unsafe_code = "deny"', 'unsafe_code = "allow"')
        errors = validate_workspace(root_manifest, members, sources, rustfmt)
        self.assertTrue(any("must deny unsafe_code" in error for error in errors))


class UnsafeBoundaryPolicyTests(unittest.TestCase):
    def test_unsafe_is_rejected_outside_the_platform_adapter(self):
        source = "fn read() { unsafe { let _ = 1; } }\n"
        errors = check_unsafe_policy.validate_unsafe_sources({"crates/worlddb-core/src/lib.rs": source})
        self.assertTrue(any("only in worlddb-storage-file" in error for error in errors))

    def test_unapproved_unsafe_makes_the_verify_step_fail(self):
        with tempfile.TemporaryDirectory(prefix="worlddb-unsafe-policy-") as directory:
            root = Path(directory)
            source = root / "crates" / "worlddb-core" / "src" / "lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text("fn read() { unsafe { let _ = 1; } }\n", encoding="utf-8")
            diagnostics = StringIO()
            with patch.object(check_unsafe_policy, "ROOT", root), redirect_stderr(diagnostics):
                exit_code = check_unsafe_policy.main()
        self.assertEqual(exit_code, 1)
        self.assertIn("unsafe code is allowed only", diagnostics.getvalue())

    def test_adapter_unsafe_requires_exception_safety_test_and_review_labels(self):
        source = "fn read() { unsafe { let _ = 1; } }\n"
        errors = check_unsafe_policy.validate_unsafe_sources(
            {"crates/worlddb-storage-file/src/lib.rs": source}
        )
        self.assertTrue(any("WDB-EXC allow attribute" in error for error in errors))
        self.assertTrue(any("SAFETY" in error for error in errors))
        self.assertTrue(any("TEST" in error for error in errors))
        self.assertTrue(any("REVIEW" in error for error in errors))

    def test_fully_documented_adapter_unsafe_passes_local_checks(self):
        source = (
            '#[allow(unsafe_code, reason = "WDB-EXC-0001")]\n'
            '// SAFETY: the pointer is checked before dereference.\n'
            '// TEST: storage_adapter_reads_validated_slice.\n'
            '// REVIEW: platform-storage-owner.\n'
            'fn read() { unsafe { let _ = 1; } }\n'
        )
        self.assertEqual(
            check_unsafe_policy.validate_unsafe_sources(
                {"crates/worlddb-storage-file/src/lib.rs": source}
            ),
            [],
        )


if __name__ == "__main__":
    unittest.main()
