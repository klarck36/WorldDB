# M7-16a – Backup process-crash matrix

**Status:** DONE on Windows; Linux/macOS execution remains deferred to M9-07.

## Crash points

`backup::tests::process_crash_at_each_backup_boundary_leaves_only_incomplete_or_verified_target` launches the real backup path in a child test process and exits it with code 86 from private checkpoints. It covers:

1. Target database layout created, before the incomplete marker.
2. Incomplete marker bytes written, before file sync; then after marker file sync.
3. First destination item created but still empty.
4. First 8-byte chunk written to the first item, before the item sync. The small test-only chunk limit guarantees the source item is only partially copied; ordinary backups keep the 64-KiB production buffer.
5. Every item after its file sync, enumerated from the pinned backup inventory.
6. Manifest bytes written before file sync; then after manifest file sync and after target-directory sync.
7. Prepublication verification complete while the incomplete marker is still present.
8. Completion marker removed; then after target-directory sync and after final independent verification.

After each child exit, the parent independently checks the target. Every pre-publication point must return `IncompleteTarget`; the three points after marker removal must pass `verify_exact_backup` with the original database identity, revision, item count, and clean storage report. Retrying the same target always returns `TargetAlreadyExists`, so uncertain callers cannot overwrite either partial bytes or a completed backup.

The source is reopened and independently verified after every crash; its identity and revision remain unchanged. Reclamation then scans durable backup-pin files. The second test, `backup::tests::process_crash_releases_stale_pin_after_reopen_without_reclaiming_live_snapshot_segment`, pauses a child while its pin is live, retires the pinned segment, proves reclamation retains it, exits the child, and proves reopen removes the stale pin and safely reclaims the now-unreferenced segment.

## Verification

Targeted Windows command:

```text
cargo test --locked -p worlddb-storage-file backup::tests -- --nocapture
```

Result: 2 passed, 0 failed. Full `cargo xtask verify` passed with 38 PASS, 1 expected M0-14 ci-matrix SKIP, and 0 FAIL. Plancheck, Sourcecheck, and `git diff --check HEAD` passed. Linux/macOS execution remains deferred to M9-07.
