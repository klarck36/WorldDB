# M7-09 – AuditCompleteBackup verification

## Scope

M7-09 adds a separately declared `AuditComplete` backup profile alongside the existing `ExactDatabase` profile. A data-only backup declares `audit_scope=Excluded` and cannot be accepted by `verify_audit_complete_backup`. An AuditComplete backup declares `audit_scope=Included` and carries the raw-read audit WAL plus `audit/AUDIT_MANIFEST` in the exact backup inventory.

The audit manifest binds the database ID, independent committed audit sequence and commit hash, WAL byte length and digest, and segment count. The top-level exact-backup manifest repeats the audit head, and the exact inventory digest (and optional keyed MAC) covers both audit files. Verification independently scans the copied WAL, checks its committed head, and cross-checks the audit manifest and item digests. The result exposes `audit_safe_sequence` separately from the WorldDB data revision.

The current storage generation writes raw-read audit history to one bounded WAL and has no immutable audit-segment writer or segment format. Therefore the supported segment count is zero. AuditComplete backup fails closed if any entry is present in `audit/segments`; it does not label an unknown or future segment as included. This is an explicit compatibility boundary, not a claim that a segment format was verified.

Creating this profile requires current `AuditRead` and `AuditExport` authorization. The live writer snapshot serializes with its appends; the offline WAL snapshot takes the independent audit writer lock and does not repair an incomplete tail. The copied WAL is checked before backup construction and again by the independent target verifier.

## Evidence

- `m7_09_audit_complete_backup_contract::data_and_audit_profiles_are_distinct_and_keep_independent_watermarks` confirms the data-only manifest markers (`profile=1`, `audit_scope=Excluded`), its lack of audit files, and rejection by the AuditComplete-only verifier. It pins the data revision at genesis and the audit sequence at 2 independently; a later audit append advances the source to sequence 3 while the backup and verifier still report sequence 2. A modified copied WAL fails verification.
- `m7_09_audit_complete_backup_contract::unexpected_audit_segment_files_fail_closed` confirms an unrecognized audit-segment file aborts the operation before a target is created.
- `m7_09_audit_complete_backup_contract::audit_complete_backup_requires_current_export_authorization` confirms a principal with `AuditRead` but without `AuditExport` is denied and no target is created.
- The full `worlddb-storage-file` package test suite passed (46 unit tests, 1 existing ignored campaign, and all integration suites); the workspace test suite passed (498 Core tests and 84 Rustdoc tests included).
- Storage-package Clippy passed with `-D warnings`. Full workspace Clippy, formatting, checks, Plancheck, Sourcecheck, and `cargo xtask verify` are recorded in the task register after completion.

## Platform boundary

Verified on Windows. Linux/macOS checks remain deferred to M9-07 as previously directed.
