# M4-14b verification — SnapshotPin-/Lockordnungs-Schedules

## Schedules and guarantees

- A fixed fork-then-release schedule verifies that the child retains the exact immutable parent binding, remains live within its own budget, and continues pinning the backend generation after the parent drops. The generation becomes reclaimable in the reference registry only after the child drops.
- Expired leases cannot create new reader pins; the generation stays pinned while the expired owner still exists and is released after its Drop.
- A barrier-started race between two pins of one SnapshotId completes within bounded channel waits. Exactly one lease is admitted; the other gets `SnapshotIdReused`. The released snapshot ID remains unavailable for reuse.
- Four barrier-started forks concurrently call `fork_reader`, `binding`, and `lifetime_status`. Each sees the exact original binding and a valid budget; all workers finish within the timeout, each child releases only its own pin, and the parent remains pinned until it drops.
- The public non-Clone guarantee remains covered by the `SnapshotLease` compile-fail doctest.

## Verification results

- SnapshotLease tests: 11 passed, including the new release-order and barrier-synchronized registry schedules.
- `cargo test --locked --workspace`: PASS — 389 Core, 7 Testkit, 2 backend-contract, and 81 Rustdoc tests; 2 explicitly long-running campaigns remained ignored by their test annotations.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check`: PASS.
- Plancheck and Sourcecheck: PASS; 242 tasks, 11 milestones, 253 invariants and 169 follow-up pairs validate, generated source snapshots match, and the audit ZIP passes its hash check.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. Fresh M0-13 evidence run `M0-13-20261001T011959Z-b39bf0e269`: PASS.

## Boundary

These schedules exercise the in-memory SnapshotRegistry. Concrete segment reclamation, durable pin persistence and crash recovery remain backend work in M5/M7.
