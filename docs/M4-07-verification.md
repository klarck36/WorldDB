# M4-07 verification — range, predicate, schema and head dependencies

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- Extended the OCC reference store with range dependencies. A range read returns the point values visible at its pinned revision and records normalized inclusive/exclusive bounds. The commit change log detects later writes inside that range, including newly inserted keys that were absent at the base.
- Added generation dependencies for normalized predicate fingerprints, stable schema-object keys, and exact HistorySpace head/ancestry keys. Writers declare the generation tokens affected by their staged writes; readers revalidate their observed token versions at commit.
- Range and generation conflicts return the existing typed `CommitOutcome::Conflict` with `ReadDependencyChanged` or `WriteTargetChanged`. The report does not expose query keys or values. Scope and point changes publish with the same revision through a candidate-state swap.

## Verification results

- Focused `occ_point` tests: 7 passed, including that writes outside the registered range do not conflict, including a range phantom and predicate/schema/HistorySpace generation changes.
- `cargo test --locked --workspace`: 342 Core, 7 Testkit, 2 backend-contract and 79 Rustdoc tests passed; 2 long CPU campaigns remained intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Plancheck: 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs valid.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL; the M0-13 evidence run passed.

## Integration boundary

The model requires the write path to supply the complete affected predicate/schema/HistorySpace tokens. It demonstrates conflict detection once those tokens are registered; authoritative index maintenance and wiring every production writer to those tokens remain integration work for the storage/write engine. This in-memory model does not provide durable atomicity.
