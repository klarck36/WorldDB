# M2-10 verification: MultiValueReplace and ReplacementBoundary

**Status:** PASS for MultiValueReplace in the index-free reference path. Checked 2026-09-30.

## Implemented

- `resolve_multi_value_replace` selects applicable ReplacementBoundary records using subject/Predicate, query Perspective/Epistemic partition, selected layers, HistorySpace ancestry, archive visibility, `RecordedAsOf`, world-time validity, closure, and retraction.
- The highest active Boundary `ContextPrecedence` cuts off candidates with strictly lower precedence. Equal-precedence Assertions stay in the complete set; higher-precedence Assertions remain visible.
- An active Boundary with no surviving Assertions produces `Known { values: [] }`. With no active Boundary and no candidates the result is `Unknown`.
- Boundary history validation rejects duplicate identities, unknown lifecycle targets, non-later lifecycle revisions, cross-timeline or out-of-validity closures, and a boundary that predates its Predicate definition.
- Resolution reuses the MultiValue value grouping, conflict, ValueKind, candidate-scope, and schema-aware temporal comparison behavior. It does not convert a negative Assertion into retraction.

## Verification

- Focused tests pass for Unknown versus explicit known-empty; lower-context cutoff; equal-precedence inclusion; ancestor-boundary preservation of higher-precedence values; closure and retraction disabling a boundary; and future or archived boundaries having no effect.
- `cargo test --locked --offline --workspace` — PASS: 199 Core tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` and `python -B WorldDB_1.0_Plancheck.py` — PASS.
- `cargo xtask verify` — PASS: 30 steps, one expected M0-14 `ci-matrix` skip, zero failures.

## Scope

This is the reference implementation for MultiValueReplace. Full persistent-history assembly, security-before-candidate integration, and indexed differential tests remain assigned to later tasks. Callers supply the query-pinned candidates and archive-visible boundary IDs.
