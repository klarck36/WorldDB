# M2-04 verification: layer overlay and context precedence

**Status:** PASS for the index-free precedence reference model. Checked 2026-09-30T12:58:10+02:00.

## Implemented

- `ContextPrecedence` is an ordered pair of HistorySpace ancestry distance and the LayerRank read from the supplied immutable `LayerSchemaSnapshot`. The ordering reverses ancestry distance so the query's local HistorySpace wins before any ancestor, then compares LayerRank in the same origin. Equal coordinates remain equal; there is no write-time or record-order tie breaker.
- `for_context` rejects unknown spaces/layers and record origins outside the query HistorySpace ancestry. It uses the caller's pinned layer snapshot; historical `BaseOnly` and snapshot selection are independently covered by `LayerSchemaSnapshot` tests, and schema history selects that snapshot at Historical/Current/Explicit modes.
- Assertion context comparison requires equal `PerspectiveScope` and `EpistemicMode` before ranking. Cross-partition comparisons fail with `DifferentPartition`; neither field is a precedence coordinate. Event contexts use a separate comparison entry point with no assertion epistemic fields.
- Precedence accepts no security policy, rights, or principal argument. Layer authorization therefore cannot modify its domain ordering; security filtering remains a separate later resolver concern.

## Verification

- `cargo test --locked --offline --workspace` — PASS: 160 core unit tests, workspace integration/unit suites, and 66 Rustdoc tests; 2 long-running campaigns intentionally ignored.
- Focused ContextPrecedence tests — PASS: HistorySpace specificity defeats an arbitrarily higher ancestor LayerRank; LayerRank decides within one origin; equal coordinates compare equal; partition mismatch and descendant origin fail closed.
- Existing pinned-snapshot tests — PASS: `layers::tests::selections_resolve_only_against_the_pinned_snapshot` and `layers::tests::base_switch_is_a_new_atomic_historical_state_and_rank_revisions_keep_ids`.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python -B WorldDB_1.0_Plancheck.py` — PASS; plan DAG and references validate.
- `cargo xtask verify` — PASS: 30 steps passed, 1 expected M0-14 `ci-matrix` skip, 0 failures.

## Scope

This completes M2's precedence primitive and comparison partition guard. Applying the partition constraint to all masking operations remains in M2-07; security-filtered non-interference remains M3-15. Query-context/snapshot pinning integration follows M3-07. The M2 model does not evaluate candidates or mutate stored records.
