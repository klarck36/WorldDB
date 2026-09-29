"""Validate registered lint exceptions and every Rust allow attribute."""

from __future__ import annotations

import re
import sys
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REGISTER = ROOT / "policy" / "exceptions.tsv"
HEADER = ("exception_id", "owner", "reason", "expires_on")
EXCEPTION_ID = re.compile(r"WDB-EXC-[0-9]{4}\Z")
ALLOW_START = re.compile(r"#\s*!?\s*\[\s*allow\b")
ALLOW_ATTRIBUTE = re.compile(r"#\s*!?\s*\[\s*allow\s*\((?P<body>[^]]*)\)\s*\]")
REASON_ID = re.compile(r'\breason\s*=\s*"(WDB-EXC-[0-9]{4})"')


def validate_policy(
    register_text: str,
    sources: dict[str, str],
    today: date,
) -> list[str]:
    errors: list[str] = []
    lines = register_text.splitlines()
    if not lines or tuple(lines[0].split("\t")) != HEADER:
        errors.append("exception register header must be: " + "\t".join(HEADER))
        return errors

    registered: dict[str, str] = {}
    for line_number, line in enumerate(lines[1:], start=2):
        if not line.strip() or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != len(HEADER):
            errors.append(f"register line {line_number}: expected four tab-separated fields")
            continue
        exception_id, owner, reason, expires_on = fields
        if not EXCEPTION_ID.fullmatch(exception_id):
            errors.append(f"register line {line_number}: invalid exception id {exception_id!r}")
        if exception_id in registered:
            errors.append(f"register line {line_number}: duplicate exception id {exception_id}")
        else:
            registered[exception_id] = f"register line {line_number}"
        if not owner.strip():
            errors.append(f"register line {line_number}: owner is required")
        if not reason.strip():
            errors.append(f"register line {line_number}: reason is required")
        try:
            expiry = date.fromisoformat(expires_on)
        except ValueError:
            errors.append(f"register line {line_number}: expires_on must be YYYY-MM-DD")
        else:
            if expiry < today:
                errors.append(f"register line {line_number}: exception {exception_id} expired on {expiry}")

    used: set[str] = set()
    for path, source in sorted(sources.items()):
        for line_number, line in enumerate(source.splitlines(), start=1):
            stripped = line.lstrip()
            if stripped.startswith(("//", "*")) or not ALLOW_START.search(line):
                continue
            attribute = ALLOW_ATTRIBUTE.search(line)
            if attribute is None:
                errors.append(
                    f"{path}:{line_number}: allow attributes must be on one line so the exception can be audited"
                )
                continue
            reason_ids = REASON_ID.findall(attribute.group("body"))
            if len(reason_ids) != 1:
                errors.append(
                    f"{path}:{line_number}: every #[allow] requires exactly one reason = \"WDB-EXC-NNNN\""
                )
                continue
            exception_id = reason_ids[0]
            used.add(exception_id)
            if exception_id not in registered:
                errors.append(
                    f"{path}:{line_number}: {exception_id} is not registered in policy/exceptions.tsv"
                )

    for exception_id in sorted(registered.keys() - used):
        errors.append(f"{exception_id} is registered but no Rust allow attribute references it")
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
        register_text = REGISTER.read_text(encoding="utf-8")
        sources = rust_sources(ROOT)
    except OSError as error:
        print(f"EXCEPTION POLICY ERROR: {error}", file=sys.stderr)
        return 1
    errors = validate_policy(register_text, sources, date.today())
    if errors:
        for error in errors:
            print(f"EXCEPTION POLICY ERROR: {error}", file=sys.stderr)
        return 1
    allow_count = sum(len(ALLOW_START.findall(source)) for source in sources.values())
    print(f"EXCEPTIONS OK: {allow_count} local allow attribute(s); register valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
