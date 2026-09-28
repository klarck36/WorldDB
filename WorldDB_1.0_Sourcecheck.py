"""Verify the immutable source ZIP, its byte-for-byte mirror, and the plan registers."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import zipfile
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parent
ARCHIVE = ROOT / "WorldDB_vNext_Lossless_Consolidation_Audit.zip"
SOURCE_DIR = ROOT / "docs" / "source"
MANIFEST = SOURCE_DIR / "source_manifest.json"
PLAN_CHECK = ROOT / "WorldDB_1.0_Plancheck.py"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> int:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    archive_bytes = ARCHIVE.read_bytes()
    if len(archive_bytes) != manifest["archive_size_bytes"]:
        raise SystemExit("Archive size differs from source manifest")
    if sha256(archive_bytes) != manifest["archive_sha256"]:
        raise SystemExit("Archive SHA-256 differs from source manifest")

    expected = manifest["files"]
    expected_names = [item["path"] for item in expected]
    if len(expected_names) != len(set(expected_names)):
        raise SystemExit("Duplicate source path in manifest")

    with zipfile.ZipFile(ARCHIVE) as archive:
        corrupt_member = archive.testzip()
        if corrupt_member is not None:
            raise SystemExit(f"ZIP CRC check failed for {corrupt_member}")
        actual_names = [info.filename for info in archive.infolist()]
        if actual_names != expected_names:
            raise SystemExit("ZIP member list or order differs from source manifest")

        mirror_names = sorted(
            path.name for path in SOURCE_DIR.iterdir()
            if path.is_file() and path.name != MANIFEST.name
        )
        if sorted(mirror_names) != sorted(expected_names):
            raise SystemExit("Source mirror file list differs from source manifest")

        for item in expected:
            name = item["path"]
            path = PurePosixPath(name)
            if path.is_absolute() or len(path.parts) != 1 or ".." in path.parts:
                raise SystemExit(f"Unsafe source path in manifest: {name}")
            archive_bytes_for_file = archive.read(name)
            mirror_bytes = (SOURCE_DIR / name).read_bytes()
            if archive_bytes_for_file != mirror_bytes:
                raise SystemExit(f"Source mirror is not byte-identical: {name}")
            if len(mirror_bytes) != item["size_bytes"]:
                raise SystemExit(f"Source byte count differs from manifest: {name}")
            if sha256(mirror_bytes) != item["sha256"]:
                raise SystemExit(f"Source SHA-256 differs from manifest: {name}")
            mirror_bytes.decode(item["encoding"])
            print(f"BYTE MATCH: docs/source/{name} ({len(mirror_bytes)} bytes, SHA-256 {item['sha256']})")

    print(f"ZIP OK: {ARCHIVE.name} (SHA-256 {manifest['archive_sha256']})")
    completed = subprocess.run(
        [sys.executable, "-X", "utf8", str(PLAN_CHECK)],
        cwd=ROOT,
        check=False,
        text=True,
        encoding="utf-8",
    )
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
