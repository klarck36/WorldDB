# M4-10 verification — snapshot leases and pinning

**Status:** DONE locally 2026-10-01. GitHub and remote Git operations were not used.

## Implementation

- Added an immutable `SnapshotBinding` that jointly captures DatabaseId/SnapshotId, data revision and RecordedAsOf, schema mode/revision/fingerprint, validated HistorySpace ancestry with parent cutoffs, historical layer definitions and selection, principal plus authorization mode and SecurityEpoch, and backend generation.
- Added a session-local `SnapshotRegistry` and non-cloneable `SnapshotLease`. `fork_reader()` registers a separate pin explicitly. The registry retains a backend generation while any reader or backup lease remains, including an expired lease until its owner drops it. Registry errors fail closed for reclamation checks.
- Interactive leases report a soft lifetime warning and fail with `SnapshotExpired` at the hard limit. Administrative ExactBackup leases can explicitly request a longer lifetime, within a configured maximum.
- Added `QueryContext::new_leased`, which checks that the lease matches the query's snapshot, data revision, read point, schema, HistorySpace, layer selection and security identity. The context owns the lease, cannot be cloned, and exposes a hard-limit check for query/commit boundaries.
- Dropping a lease updates only the in-memory registry. Physical segment reclamation, shutdown coordination and durable backend pin records remain later storage work.

## Verification results

- Snapshot lease tests: 8 passed, covering complete binding, immutable revision, independent forks, soft/hard limits, ExactBackup extension, expired fork rejection, duplicate IDs and generation retention/release.
- Leased QueryContext tests: 2 passed, covering matching pin ownership, hard expiry and revision mismatch rejection.
- Compile-fail doctest confirms `SnapshotLease` cannot be cloned.
- `cargo test --locked --workspace`: 363 Core, 7 Testkit, 2 backend-contract and 80 Rustdoc tests passed; 2 long CPU campaigns remained intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Plancheck and source check: passed; 236 tasks, 11 milestones, 253 invariants and 169 follow-up pairs valid.
- `cargo xtask verify`: 31 PASS, 1 expected M0-14 `ci-matrix` SKIP, 0 FAIL. M0-13 run `M0-13-20260930T232335Z-07d890ae5e` passed.

## Integration boundary

This is the M4 in-memory pin registry and engine reference path. A storage backend must connect generation pins to concrete segment references and shutdown/reclamation before claiming physical reclamation safety. Durable pin persistence and crash recovery remain later storage tasks.
