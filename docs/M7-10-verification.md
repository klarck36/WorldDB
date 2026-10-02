# M7-10 – Restore verification

## Scope

M7-10 restores a fully verified `ExactDatabase` or `AuditComplete` backup as a clone in a new, unused database directory. The public `RestoreManager::restore_clone` path checks the current `BackupRestore` capability before reading the source. An `AuditComplete` backup additionally requires current `AuditRead` and `AuditExport` before its audit WAL or manifest is read. If a MAC key is supplied, a wrong key or invalid MAC fails closed.

The restore copies the declared inventory into a unique sibling staging directory. It verifies the staged files against the backup manifest and independently verifies the copied audit namespace. It then removes the backup-only root manifest and incomplete marker, assigns a fresh `DatabaseId`, recovers the staged data, and commits the `RestorePublication` manifest snapshot with its Required Audit Record in the same WAL prepare and commit marker. The complete staged directory is published with one same-volume, no-replace directory move. After publication, the destination is reopened and its storage state, committed audit record, clone identity, and separate audit lineage are checked again. An error after the directory move is reported as `PublishedOutcomeUnknown` with the destination and `OperationId` so a caller can reconcile visibility.

The copied `AuditComplete` raw-read WAL and its audit manifest remain a separate namespace. Their source database identity, audit sequence, commit hash, lengths, and digests are checked against the verified backup manifest independently from the clone's data revision and new `DatabaseId`. Same-identity disaster recovery is not exposed because this storage layer cannot enforce the required exclusivity against the original database.

## Evidence

- `m7_10_restore_contract::exact_backup_restores_as_a_new_id_with_atomic_restore_audit` restores a non-empty database snapshot, confirms a distinct clone identity and a single new revision, checks the new manifest, and verifies that `RestorePublication` has a committed Required Audit Record bound to the same revision and `OperationId`.
- `m7_10_restore_contract::audit_complete_restore_checks_permissions_and_preserves_separate_audit_lineage` confirms current `AuditRead` and `AuditExport` gates and byte-preserves the independent audit WAL and manifest while reporting their original safe sequence separately.
- `m7_10_restore_contract::existing_destinations_and_corrupt_backups_are_never_published_or_overwritten` confirms an existing directory remains byte-identical and a corrupt backup creates no destination.
- `m7_10_restore_contract::backup_restore_permission_is_checked_before_staging` confirms a denied restore leaves no target.
- `backup::restore::tests::publication_faults_leave_either_no_target_or_a_verified_audited_target` injects failures immediately before and after publication. Before publication there is no destination and the hidden stage is cleaned up; after publication the explicit uncertain result names the visible destination, whose storage and audit commit verify cleanly.
- Windows workspace tests, Clippy, formatting, workspace checks, Plancheck, Sourcecheck, `git diff --check`, and `cargo xtask verify` are recorded in the task register after completion.

## Platform boundary

Verified on Windows. Linux/macOS validation remains deferred to M9-07 as directed. No same-identity disaster-recovery mode is claimed.
