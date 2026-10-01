# M4-06 verification — point read/write dependencies

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- Added `OccPointStore` and consuming `OccPointTransaction` as an executable point-OCC reference model. Each read records the observed point version or absence at its base revision; each write captures the same base state for Write/Write validation.
- Commit rechecks read and write dependencies against current per-key versions. A changed read target returns `CommitOutcome::Conflict(ReadDependencyChanged)`; a changed write target returns `Conflict(WriteTargetChanged)`. Conflict reports expose only typed facts, not keys or values.
- Multiple writes publish under one new revision. The store constructs a complete candidate map and swaps it only after all writes are assembled. Disjoint point writes may both commit even when they began at the same base; same-point write races do not use Last-Write-Wins.
- Duplicate writes to a key inside one transaction and empty write sets are rejected. The transaction binds an `OperationId` in its receipt; durable idempotency and operation-status recovery remain M4-09.

## Verification results

- Focused `occ_point` tests: 4 passed, covering Write/Write, Write/Read, absent-point Read/Write, disjoint writes and duplicate staged writes.
- `cargo test --locked --workspace`: 340 Core, 7 Testkit, 2 backend-contract and 79 Rustdoc tests passed; 2 long CPU campaigns remained intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Plancheck: 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs valid.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL.

## Integration boundary

This is the in-memory point-dependency model for M4-06. Range/predicate phantoms, schema versions and HistorySpace heads follow in M4-07; caller-visible conflict filtering and bounded replay follow in M4-08. It does not claim durable atomicity or replace the production backend's compare-and-publish protocol.
