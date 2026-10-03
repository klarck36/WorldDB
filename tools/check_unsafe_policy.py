"""Require local evidence and review metadata for every adapter unsafe construct."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
UNSAFE_CONSTRUCT = re.compile(r"\bunsafe\s*(?:\{|fn\b|impl\b|trait\b|extern\b)")
EXCEPTION_ALLOW = re.compile(
    r'#\[allow\([^]]*\bunsafe_code\b[^]]*reason\s*=\s*"(WDB-EXC-[0-9]{4})"[^]]*\)\]'
)
REQUIRED_MARKERS = ("// SAFETY:", "// TEST:", "// REVIEW:")
WINDOW_LINES = 12
APPROVED_ADAPTER_ROOTS = (
    "crates/worlddb-storage-file/",
    "crates/worlddb-process-adapter/",
)


def validate_unsafe_sources(sources: dict[str, str]) -> list[str]:
    errors: list[str] = []
    for path, source in sorted(sources.items()):
        lines = source.splitlines()
        for index, line in enumerate(lines):
            code = line.split("//", 1)[0]
            if not UNSAFE_CONSTRUCT.search(code):
                continue
            location = f"{path}:{index + 1}"
            normalized_path = path.replace("\\", "/")
            if not normalized_path.startswith(APPROVED_ADAPTER_ROOTS):
                errors.append(f"{location}: unsafe code is allowed only in approved platform adapters")
                continue
            start = max(0, index - WINDOW_LINES)
            context_lines = lines[start : index + 1]
            context = "\n".join(context_lines)
            if EXCEPTION_ALLOW.search(context) is None:
                errors.append(f"{location}: unsafe code needs a local WDB-EXC allow attribute")
            for marker in REQUIRED_MARKERS:
                if not any(marker in candidate for candidate in context_lines):
                    errors.append(f"{location}: unsafe code needs a nearby {marker[:-1]} label")
    return errors


def rust_sources(root: Path) -> dict[str, str]:
    files: list[Path] = []
    for directory in ("crates", "xtask", "tests"):
        base = root / directory
        if base.exists():
            files.extend(base.rglob("*.rs"))
    return {
        path.relative_to(root).as_posix(): path.read_text(encoding="utf-8")
        for path in sorted(files)
    }


def main() -> int:
    try:
        sources = rust_sources(ROOT)
    except OSError as error:
        print(f"UNSAFE POLICY ERROR: {error}", file=sys.stderr)
        return 1
    errors = validate_unsafe_sources(sources)
    if errors:
        for error in errors:
            print(f"UNSAFE POLICY ERROR: {error}", file=sys.stderr)
        return 1
    print("UNSAFE POLICY OK: no unapproved unsafe constructs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
