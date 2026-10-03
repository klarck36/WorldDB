# M7-15 – Offline-Purge-Rewrite

M7-15 implements the approved M7-14 purge as a new database tree. It never removes records from
the source database in place. The destination receives a generated `DatabaseId`, the retained
logical history, a canonical `PURGE_REPORT`, and an atomic `PurgePublication` Required Audit Record.

## Revalidation and rewrite

The rewrite requires a complete logical export and an explicitly approved purge plan. It takes the
source writer lock, verifies that the source is clean at the approved revision, checks the current
manifest against the WAL head, and re-exports the full requested scope while the lock is held. The
canonical export bytes must match the reviewed artifact exactly. It then rescans all local index
generations and pointers and requires the caller's known external-artifact inventory and
completeness declaration to match the approved plan. The dependency closure and approval
fingerprint are recomputed before any destination is published.

The implementation creates a sibling staging directory beside the requested destination, so the
final directory rename stays on the same volume. It initializes a fresh storage layout and refuses
an identity collision with the source. Affected records are omitted by their exact typed
`PurgeRecordId`; retained records keep their original record bytes and revision. The destination
commits snapshots through the source revision and carries the source security policy state forward
at every revision. `MAX_REWRITE_REVISIONS` and `REPORT_LIMIT` bound the rewrite at 65,536 source
revisions and 128 MiB for the encoded report.

## Publication and Required Audit

The canonical `PURGE_REPORT` binds the source and destination identities, source and destination
revisions, source-export digest, approved-plan fingerprint, removed record identities and digests,
identity mappings, known external copies, external-inventory completeness, and index families to
rebuild. Retained identities map to their same typed identity; purged identities map to no
destination. Its secure-erasure claim bit is fixed to zero.

The destination's `PurgePublication` action and its Required Audit Record are committed through one
audited WAL manifest operation at the revision after the source snapshot. The staged destination
must then pass Storage Verify, exact retained-record comparison, audit binding checks, and a byte
comparison of the report. Only then does the implementation atomically rename the staging
directory to the requested destination and reopen and verify the published database.

Faults before the audit commit or before directory publication trigger staging cleanup on normal
error return and leave the destination absent. If a fault or durability error occurs after the
directory rename, the API reports `PublishedOutcomeUnknown` with the destination path and operation
identity. It does not present an ordinary success receipt; the visible target has already passed
the staged verification and contains the Required Audit Record.

The source writer lock remains held through destination publication and final verification. The
rewrite does not modify source records, manifests, or index generations. Known backups and exports
remain outside the rewritten database and are listed in the report. Purge makes no physical
erasure promise for copy-on-write storage, SSDs, cloud-synced directories, or external backups.

## Scope

The automated proof in this task runs on Windows. Linux/macOS-specific runs remain scheduled for
M9-07. M7-16 owns the broader process-crash and compatibility matrix across backup, restore,
migration, export, and purge phases.
