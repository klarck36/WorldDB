"""Integrity and mutation tests for the M0-13 deterministic test corpus."""

from __future__ import annotations

import hashlib
import json
import shutil
import tempfile
import unittest
from pathlib import Path

from check_testkit_corpus import CORPUS_ROOT, CorpusError, validate_corpus


class TestkitCorpusTests(unittest.TestCase):
    def test_checked_in_versioned_corpus_is_valid(self):
        resolved = validate_corpus()
        self.assertEqual(len(resolved), 3)
        self.assertTrue(all(Path(item["path"]).is_file() for item in resolved))

    def test_changed_fixture_is_rejected_by_content_hash(self):
        with tempfile.TemporaryDirectory() as temporary:
            copy = Path(temporary) / "m0-13"
            shutil.copytree(CORPUS_ROOT, copy)
            fixture = copy / "golden" / "v1" / "splitmix64.tsv"
            fixture.write_text(fixture.read_text(encoding="utf-8") + "# tampered\n", encoding="utf-8")
            with self.assertRaisesRegex(CorpusError, "SHA-256 mismatch"):
                validate_corpus(copy)

    def test_corpus_path_cannot_escape_its_versioned_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            copy = Path(temporary) / "m0-13"
            shutil.copytree(CORPUS_ROOT, copy)
            manifest_path = copy / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["files"][0]["path"] = "../outside.tsv"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(CorpusError, "versioned path differs"):
                validate_corpus(copy)

    def test_unique_seed_constraint_is_enforced(self):
        with tempfile.TemporaryDirectory() as temporary:
            copy = Path(temporary) / "m0-13"
            shutil.copytree(CORPUS_ROOT, copy)
            fixture = copy / "fuzz-seeds" / "v1" / "seeds.tsv"
            text = fixture.read_text(encoding="utf-8")
            lines = text.splitlines()
            first = lines[1].split("\t")
            duplicate = lines[2].split("\t")
            duplicate[1] = first[1]
            lines[2] = "\t".join(duplicate)
            fixture.write_text("\n".join(lines) + "\n", encoding="utf-8")
            manifest_path = copy / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            seed_entry = next(item for item in manifest["files"] if item["id"] == "fuzz-seeds-v1")
            seed_entry["sha256"] = hashlib.sha256(fixture.read_bytes()).hexdigest()
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(CorpusError, "unique"):
                validate_corpus(copy)


if __name__ == "__main__":
    unittest.main()
