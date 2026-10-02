"""Negative tests for the checked-in M6-14a manifest contract."""

from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

from verify_corpus import CorpusError, verify


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "crates" / "worlddb-testkit" / "testdata" / "m6-14a" / "manifest.json"


class ManifestContractTests(unittest.TestCase):
    def test_checked_in_manifest_has_complete_full_profile_metadata(self) -> None:
        result = verify(MANIFEST, None, verify_raw_files=False)
        self.assertEqual(result["profile"], "full")
        self.assertEqual(result["workload"]["assertions"], 1_000_000)
        self.assertEqual(result["workload"]["provenance_edges"], 10_000_000)

    def test_changed_dataset_hash_is_rejected(self) -> None:
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        manifest["dataset_sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as temporary:
            mutated = Path(temporary) / "manifest.json"
            mutated.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaises(CorpusError):
                verify(mutated, None, verify_raw_files=False)

    def test_changed_file_count_is_rejected(self) -> None:
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        manifest["files"] = copy.deepcopy(manifest["files"])
        manifest["files"][1]["rows"] -= 1
        with tempfile.TemporaryDirectory() as temporary:
            mutated = Path(temporary) / "manifest.json"
            mutated.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaises(CorpusError):
                verify(mutated, None, verify_raw_files=False)


if __name__ == "__main__":
    unittest.main()
