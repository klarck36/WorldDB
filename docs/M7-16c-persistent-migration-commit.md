# M7-16c – Persistent file-store migration commits

**Status:** Complete on Windows, 2026-10-03 06:01 CEST
**Task:** `M7-16c`
**Implementation:** `crates/worlddb-storage-file/src/migration_commit.rs`

## Result

`FileMigrationCommitBackend` implements the guarded core migration contract on the file store's real WAL and immutable-history-segment path. It is opened while holding the database's exclusive `WriterLock`; opening first runs recovery, verifies that `CURRENT` matches the verified WAL head, checks every referenced history digest, and reconstructs stored migration markers from the Required Audit stream.

Each migration step stages one history segment and a full manifest snapshot. The WAL prepare stores the exact `MigrationAuditCommit::canonical_action_payload`, the Required Audit record, and a separate replay snapshot. One synced WAL commit marker publishes all three at the same revision and OperationId. The audit action and record therefore cannot become visible independently of the step batch.

The new version-2 Required Audit envelope is additive. Existing version-1 payloads continue to decode unchanged. Recovery selects the embedded replay snapshot, while the audit view retains the canonical action payload. A verified marker must match its audit action's migration, run, step, operation, plan, input and decision fingerprints as well as its committed revision.

The adapter compares every expected base with the live WAL head before staging. It restores migration-operation markers after reopen and derives their exact commit revisions from the matching Required Audit context. If the WAL marker outcome cannot be reconciled after recovery, the backend returns `OutcomeUnknown(OperationId)` and blocks further writes until reopen. Unauthenticated, missing, mismatched, or unsupported history fails closed.

This adapter supports reads at Genesis and at the current head. Reads of older non-genesis revisions return a backend error because compaction can merge records into a segment whose reference does not preserve each record's original revision. The guarded migration executor reads at the current OCC base. M7-16d must confirm that production call path through the database integration.

## Windows evidence

`migration_commit::tests::guarded_steps_and_canonical_audit_actions_reopen_from_file_store_commits` executes two Restrictive steps through `execute_guarded_migration`, confirms each marker and Required Audit record share the target revision and operation, closes and reopens the database, and verifies both markers and audits are reconstructed. It also confirms that an unsupported older snapshot read is rejected.

The WAL unknown-outcome tests cover both reconciliation results. `wal::tests::commit_sync_fault_returns_unknown_outcome_without_a_receipt` confirms that a marker found after recovery is synced, re-read, and accepted as committed in the same process. `wal::tests::recovered_unknown_outcome_without_marker_is_proven_not_committed` confirms that successful tail recovery can prove the operation absent and clear its process-local indeterminate guard.

`cargo xtask verify` passed on Windows at 2026-10-03 06:01 CEST: **38 passed, 1 expected `ci-matrix` skip, 0 failed**. This includes formatting, workspace checks, Clippy, all 78 storage-file unit tests and their integration contracts, core contracts, and repository policy checks. Linux/macOS execution remains deferred to M9-07 per project direction.

## Next

M7-16d exercises this adapter through a real file-store database handle, including guarded authorization, Breaking restore proof, OCC, journal status, and audit binding. M7-16e owns the process-crash/reopen matrix.
