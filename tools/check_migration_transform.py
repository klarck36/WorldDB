"""Enforce the migration transformer's closed, deterministic dependency boundary."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "crates" / "worlddb-core" / "src" / "migration_transform.rs"
ALLOWED_PATH_ROOTS = {
    "blake3",
    "crate",
    "fmt",
    "i128",
    "i64",
    "std",
    "super",
    "u8",
    "u64",
    "uuid",
}
FORBIDDEN_PATTERNS = {
    "clock access": re.compile(
        r"\bstd\s*::\s*time\s*::|\b(?:SystemTime|Instant)\s*::\s*now\b"
    ),
    "environment or filesystem access": re.compile(r"\bstd\s*::\s*(?:env|fs|process)\s*::"),
    "process-configured decode limits": re.compile(r"DecoderLimits\s*::\s*process_default\s*\("),
    "network access": re.compile(
        r"\b(?:std\s*::\s*net|tokio\s*::\s*net|reqwest|ureq|hyper|tonic)\s*::"
    ),
    "randomness": re.compile(r"\b(?:rand|rand_core|getrandom)\s*::|\bnew_v4\s*\("),
    "locale access": re.compile(
        r"\b(?:locale|icu_locale|unic_locale|unic_langid|locale_config)\s*::"
    ),
    "thread access": re.compile(r"\bstd\s*::\s*thread\s*::"),
    "AI or model client": re.compile(r"\b(?:openai|llm|ai_client)\s*::"),
    "injected callback": re.compile(r"\b(?:dyn\s+)?Fn(?:Mut|Once)?\s*(?:\(|<)"),
}
PATH_ROOT = re.compile(r"(?<![:\w])([a-z][a-z0-9_]*)\s*::")


def violations(source: str) -> list[str]:
    errors = [
        f"forbidden {name}"
        for name, pattern in FORBIDDEN_PATTERNS.items()
        if pattern.search(source)
    ]
    for root in sorted(set(PATH_ROOT.findall(source)) - ALLOWED_PATH_ROOTS):
        errors.append(f"unapproved external path root {root}::")
    return errors


def main() -> int:
    source = SOURCE.read_text(encoding="utf-8")
    errors = violations(source)
    if errors:
        for error in errors:
            print(f"MIGRATION TRANSFORM POLICY ERROR: {error}", file=sys.stderr)
        return 1
    print("MIGRATION TRANSFORM POLICY OK: closed deterministic dependency boundary")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
