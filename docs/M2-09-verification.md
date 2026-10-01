# M2-09 verification: MultiValueOverlay

**Status:** PASS for MultiValueOverlay in the index-free reference path. Checked 2026-09-30.

## Implemented

- `resolve_multi_value_overlay` resolves one subject/Predicate slot against the pinned Predicate definition and visible candidates already filtered for historical visibility, security, archive state, and Masking.
- Overlay is a set union across every visible context. A child context does not silently replace values from its parent; equal-precedence peers are also retained. Neither Revision nor AssertionId selects a winning context.
- Equal value and polarity propositions coalesce with all contributor IDs sorted canonically. Distinct values remain separate, including explicitly negative values.
- When the same value has visible positive and negative assertions, the result is `Conflict`. Other non-conflicting values remain included in the result, so reporting one contradiction does not erase known members of the set.
- No remaining candidates yields `Unknown`, not an empty known set. ReplacementBoundary semantics are handled separately by M2-10.
- Predicate identity, resolution policy, candidate IDs, query/partition/layer scope, and ValueKind are checked. Time equality uses a caller-supplied pinned-schema comparator; unavailable comparison fails closed.

## Verification

- Five focused overlay tests pass: parent+child values union without precedence replacement; duplicate proposition contributors are coalesced in stable order; polarity conflict preserves other known values; Masking all candidates yields Unknown; insertion order does not change output and temporal values fail closed without schema comparison.
- `cargo test --locked --offline --workspace` — PASS: 194 Core tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` and `python -B WorldDB_1.0_Plancheck.py` — PASS.
- `cargo xtask verify` — PASS: 30 steps, one expected M0-14 `ci-matrix` skip, zero failures (M0-13 evidence run `M0-13-20260930T121606Z-0d9e5c24c6`).

## Scope

This is the reference implementation for MultiValueOverlay. It intentionally unions visible contexts and treats opposite polarities for one value as contradiction; explicit whole-set replacement, including a known empty set, remains M2-10. Indexed differential tests follow M6. Security must filter candidates before calling this resolver.
