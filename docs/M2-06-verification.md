# M2-06 verification: index-free candidate full scan

**Status:** PASS for assertion candidate collection in the slow reference model. Checked 2026-09-30T13:32:55+02:00.

## Implemented

- `AssertionHistoryRecord` stores assertion, validity-closure, and retraction records in `HistorySpaceReferenceModel`. `full_scan_assertion_candidates` calls the existing branch-aware raw history scan at `RecordedAsOf`, so parent records obey every fixed ancestor cutoff and future parent commits do not leak into children.
- The scan resolves `BaseOnly`, `AllActive`, or `Explicit` against the supplied pinned `LayerSchemaSnapshot`, checks the query's Perspective/Epistemic pair, and selects only records in that partition and layer set. Unrelated partitions are removed before time comparison, so an incomparable Timeline in an unrelated partition cannot break this query.
- Stored creation revisions and assertion Context HistorySpace IDs are checked against the owning history row. Relevant closures and retractions are projected with the M2-05 model at the selected `RecordedAsOf` and `WorldTime`. Ordinary archive visibility is applied from the M2-05a history; archived assertions remain available through raw history.
- Each candidate carries its source HistorySpace and M2-04 precedence coordinates. The result is canonicalized by AssertionId, and the scan contains no index or resolution write-order behavior.

## Verification

- Four focused `candidate_scan::tests` pass: branch cutoff plus layer/time filtering; partition isolation and Archive filtering while raw history retains both targets; stable candidate set under reversed storage order; and fail-closed invalid partition, missing archive inventory, and stored revision mismatch.
- `cargo test --locked --offline --workspace` — PASS: 174 core unit tests and 66 Rustdoc tests; two long-running campaigns intentionally ignored.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python -B WorldDB_1.0_Plancheck.py` — PASS; 236 tasks, 11 milestones, 253 invariants and 161 follow-up pairs validate.

## Scope

This is the assertion candidate oracle for resolution work. It does not apply Masking or a ResolutionPolicy; Masking is M2-07 and value resolution follows M2-08–M2-10. Security filtering remains a separately scheduled pre-resolution requirement; event candidate collection follows M2-12a. Indexed differential verification remains M6-13.
