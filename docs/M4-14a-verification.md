# M4-14a verification — OCC-/Phantom-Concurrency-Historien

## Delivered behavior

- Added a dependency-free deterministic OCC history generator to `occ_point.rs` tests. Five fixed 64-bit seeds each drive 64 batches of 2–5 transactions, with varied base revisions, point reads (including absent keys), inclusive ranges, Predicate/Schema/HistorySpace-head tokens, and writes that may overlap those dependencies.
- An independent serial reference model stores point and scope versions, range contents, and committed writes. Each generated historical read is compared to the model; the model independently predicts whether each attempted commit conflicts and which conflict facts apply, or which revision a successful commit receives.
- Every seed is run twice from a fresh store. The ordered commit/conflict trace, including commit revisions and reported conflict facts, must match exactly. Assertion diagnostics include the seed, batch, and transaction so a failure is replayable.

## Verification results

- `cargo test -p worlddb-core seeded_occ_histories_match_serial_reference_and_replay_exactly -- --nocapture`: PASS (5 seeds × 64 batches × 2 replays).
- `cargo test --locked --workspace`: PASS — 386 Core, 7 Testkit, 2 backend-contract, and 81 Rustdoc tests; 2 explicitly long-running campaigns remained ignored by their test annotations.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- Plancheck and Sourcecheck: PASS; 242 tasks, 11 milestones, 253 invariants and 169 follow-up pairs validate, all generated source snapshots match, and the audit ZIP passes its hash check.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. Fresh M0-13 evidence run `M0-13-20261001T011258Z-ebb3121295`: PASS.

## Scope

This task validates the point/range/scope OCC reference model and deterministic replay. Snapshot pin/lock ordering and queue/shutdown schedules remain in M4-14b and M4-14c; the M4-14 gate remains open until those tasks complete.
