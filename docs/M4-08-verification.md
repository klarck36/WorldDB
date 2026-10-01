# M4-08 verification — conflict reports and bounded replay-safe retry

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- Added `ConflictReport::filtered`, which returns only facts selected by the caller's authorization filter. Conflict reports contain the closed `ConflictFact` values and never carry record keys or values.
- Added a crate-private retry runner for internal replay-safe work. It accepts the same immutable authoritative input for each attempt, retries only `CommitOutcome::Conflict`, stops on commit or any non-conflict error, and allows at most two total attempts.
- User transactions and migrations cannot call the internal runner and remain manual-retry operations. An internal caller must rebuild its read/write set and repeat validation from current state; external side effects are excluded by the helper's contract.

## Verification results

- Focused retry and conflict-filter tests: 5 passed, including stable input across retry, commit short-circuit, conflict limit and hidden-fact filtering.
- `cargo test --locked --workspace`: 348 Core, 7 Testkit, 2 backend-contract and 79 Rustdoc tests passed; 2 long CPU campaigns remained intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Plancheck: 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs valid.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL.

## Integration boundary

No current product operation is designated ReplaySafe yet, so the internal runner is not wired to a user or migration write path. Future internal worker call sites must establish that replay has no external side effects and revalidates against a fresh snapshot. Durable commit uncertainty still requires OperationId status resolution in M4-09.
