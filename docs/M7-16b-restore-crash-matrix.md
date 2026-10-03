# M7-16b – Restore process-crash matrix

**Status:** DONE on Windows. Linux/macOS execution remains deferred to M9-07.

## Process crash cases

`backup::restore::tests::process_crash_before_or_after_atomic_publication_leaves_only_absent_or_verified_audited_target` starts the real restore path in a child test process and terminates it with exit code 86 at both restore checkpoints. The matrix runs against both an ExactDatabase backup and an AuditComplete backup.

| Backup profile and crash point | Reopen result |
|---|---|
| ExactDatabase, before atomic publish | Destination path is absent. |
| ExactDatabase, after atomic publish and parent-directory sync | Destination opens under a new DatabaseId; recovery completes, StorageVerifier is clean, CURRENT matches the safe revision, and one committed RestorePublication Required Audit record binds the same revision and operation. |
| AuditComplete, before atomic publish | Destination path is absent. |
| AuditComplete, after atomic publish and parent-directory sync | Same verified clone and atomic restore audit checks pass; the copied AuditComplete manifest and raw-read audit WAL bytes match the verified backup lineage. |

The test also runs a child restore attempt against an existing destination. Restore returns `TargetAlreadyExists`, and the sentinel file and directory contents remain unchanged. After every simulated process exit, both backup manifest and inventory digests still match their original verified values, and the source database passes StorageVerifier.

The post-publish crash occurs after the atomic directory move and parent-directory sync but before the restore method's final reopen/verification. The parent test therefore exercises recovery and verification from the process-visible published destination, including the Required Audit record.

## Verification

Targeted Windows command:

```text
cargo test --locked -p worlddb-storage-file backup::restore::tests::process_crash_before_or_after_atomic_publication_leaves_only_absent_or_verified_audited_target -- --nocapture
```

Result: 1 matrix test passed; it exercises four child-process exits plus the existing-destination child attempt across both backup profiles. Full `cargo xtask verify` passed with 38 PASS, 1 expected M0-14 ci-matrix SKIP, and 0 FAIL. Plancheck passed with 252 tasks, 253 invariants, and 209 follow-up pairs; Sourcecheck, `cargo fmt --all -- --check`, and `git diff --check HEAD` passed. Linux/macOS execution remains deferred to M9-07.

Commit 3ed95ec was pushed to origin/codex/worlddb-project-integration; the remote branch ref was verified at that commit.
