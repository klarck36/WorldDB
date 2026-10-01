# M2-08 verification: SingleValueReplace

**Status:** PASS for single-slot resolution in the index-free reference path. Checked 2026-09-30T14:06:03+02:00.

## Implemented

- `resolve_single_value_replace` accepts one subject/Predicate slot, its pinned `PredicateDefinition`, and candidates already filtered by historical visibility, security, archive state, and Masking.
- The pinned definition must match the requested Predicate and use `SingleValueReplace`. Preferred candidates are chosen solely by greatest `ContextPrecedence`; creation revision, AssertionId, and input order do not influence which value wins.
- With no matching candidate the result is `Unknown`. Equal-precedence candidates with one Proposition Equality value and polarity yield `Known`, retaining all contributor IDs in canonical order. Any value or polarity disagreement among peers yields `Conflict` with all competing IDs.
- Candidate partitions, selected layers, query identity, duplicate IDs, and schema ValueKind are checked. Temporal values use the selected-schema comparator; if it cannot establish equality, resolution returns a technical error rather than guessing.
- Canonical scalar equality is shared with the M2-07 mask projection so Mask selectors and resolution use the same Proposition Value rules.

## Verification

- Nine focused `single_value_resolution::tests` pass: empty input to Unknown, masking all candidates then resolving to Unknown, local-over-parent precedence independent of revision, equal-rank value conflict independent of input order, duplicate equal contributors, polarity conflict, temporal equality through schema callback and fail-closed callback error, schema policy rejection, and duplicate/mixed-scope/ValueKind rejection.
- `cargo test --locked --offline --workspace` — PASS: 189 Core tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` and `python -B WorldDB_1.0_Plancheck.py` — PASS.
- `cargo xtask verify` — PASS: 30 steps, one expected M0-14 `ci-matrix` skip, zero failures.

## Scope

This is the reference implementation for `SingleValueReplace`. `MultiValueOverlay` and `MultiValueReplace` remain M2-09/M2-10; production indexed differential tests follow M6. Security must filter candidates before this function is called.
