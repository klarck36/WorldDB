"""Classify legacy domain-ID terminology in contract and product sources."""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCAN_DIRECTORIES = (
    "crates",
    "xtask",
    "docs/contract",
    "docs/source",
    "docs/product",
    "docs/architecture",
)
SCAN_FILES = ("WorldDB_1.0_Invariantenabdeckung.tsv",)
TEXT_SUFFIXES = {".md", ".rs", ".toml", ".tsv"}
TERMS = ("BranchId", "SchemaVersionId", "LifecycleRecordId")
TERM_PATTERNS = {term: re.compile(rf"\b{re.escape(term)}\b") for term in TERMS}
ADR_FILE = re.compile(r"(?:^|/)ADR-[0-9]{3}-[^/]+\.md\Z", re.IGNORECASE)
ADR_REGISTER = re.compile(r"(?:^|/)WorldDB_ADRs_vNext\.md\Z", re.IGNORECASE)
ADR_REFERENCE = re.compile(r"\bADR-[0-9]{3}\b", re.IGNORECASE)
HISTORICAL_MARKERS = (
    "histor",
    "legacy",
    "alt-ids",
    "altterminologie",
    "frühere",
    "fruehere",
    "previous",
    "earlier",
    "retired type",
    "verworf",
)
NEGATIVE_MARKERS = (
    "existiert nicht",
    "existieren nicht",
    "does not exist",
    "do not exist",
    "not exist",
    "no separate",
    "keinen ",
    "keine ",
    "kein ",
    "not allowed",
    "forbidden",
    "forbid",
    "verboten",
    "ausgeschlossen",
    "excluded",
)


def classify_occurrence(path: str, line: str, start: int, term: str) -> str | None:
    normalized_path = path.replace("\\", "/")
    lower_path = normalized_path.lower()
    if any(marker in lower_path for marker in ("/compile_fail/", "/compile-fail/", "/tests/ui/fail/")):
        return "compile-fail"

    context = line[max(0, start - 140) : min(len(line), start + len(term) + 180)].lower()
    if ADR_FILE.search(normalized_path) or ADR_REGISTER.search(normalized_path) or ADR_REFERENCE.search(context):
        return "adr-decision"
    if any(marker in context for marker in HISTORICAL_MARKERS):
        return "historical-explanation"
    if any(marker in context for marker in NEGATIVE_MARKERS):
        return "explicit-negative-rule"
    return None


def classify_text(path: str, source: str) -> list[tuple[int, str, str | None]]:
    occurrences: list[tuple[int, str, str | None]] = []
    for line_number, line in enumerate(source.splitlines(), start=1):
        for term, pattern in TERM_PATTERNS.items():
            occurrences.extend(
                (line_number, term, classify_occurrence(path, line, match.start(), term))
                for match in pattern.finditer(line)
            )
    return occurrences


def source_files(root: Path) -> list[Path]:
    files = [root / name for name in SCAN_FILES if (root / name).is_file()]
    for directory in SCAN_DIRECTORIES:
        base = root / directory
        if base.exists():
            files.extend(path for path in base.rglob("*") if path.is_file() and path.suffix in TEXT_SUFFIXES)
    return sorted(set(files))


def main() -> int:
    errors: list[str] = []
    categories: Counter[str] = Counter()
    for path in source_files(ROOT):
        relative = path.relative_to(ROOT).as_posix()
        try:
            source = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as error:
            print(f"TERMINOLOGY ERROR: cannot read {relative}: {error}", file=sys.stderr)
            return 1
        for line_number, term, classification in classify_text(relative, source):
            if classification is None:
                errors.append(f"{relative}:{line_number}: unclassified legacy term {term}")
            else:
                categories[classification] += 1
    if errors:
        for error in errors:
            print(f"TERMINOLOGY ERROR: {error}", file=sys.stderr)
        return 1
    summary = ", ".join(f"{name}={count}" for name, count in sorted(categories.items())) or "none"
    print(f"TERMINOLOGY OK: {sum(categories.values())} occurrence(s) classified ({summary})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
