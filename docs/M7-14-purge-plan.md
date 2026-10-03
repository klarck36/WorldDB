# M7-14 – Purge-Plan

M7-14 adds a deterministic, read-only preview for the administrative purge flow. The preview is
bound to one canonical logical-export artifact, its `DatabaseId`, snapshot revision, and BLAKE3
digest. It never changes the source database and it does not publish a destination database.

## Reference closure

The planner uses the same typed reference extraction as logical import. Each exported record is
assigned a typed identity and a digest of its exact canonical record frame. Schema revisions that
share a stable schema identity remain distinguishable by their frame digest. For each requested
identity the planner finds its defining records, then repeatedly adds records that refer to any
identity already selected for removal. Every identity owned by an added record is considered in
the next closure step. The visited-identity set makes cycles terminate deterministically.

The graph covers the existing typed references for HistorySpaces and schema, Entities, Assertions,
Masks and replacement boundaries, Events and relations, archive transitions, lifecycle records,
Sources, Evidence, Provenance, and TransferLineage. An unresolved reference, absent target, record
without an identity, or duplicate selector fails closed.

`RejectIfReferenced` approval succeeds only if the dependant closure is empty. A
`PurgeCascadePlan` must name exactly every computed dependant: omitted and unrelated records are
rejected. The target records themselves are already explicit in the original target list and are
not repeated in the cascade list.

## Scope and sidecar inventory

The planner rejects partial revision ranges, missing revision-bearing record classes, unselected
HistorySpaces, missing HistorySpace definitions, and unresolved references. It expects the caller
to supply a privileged complete snapshot export; M7-15 must revalidate that artifact against the
current database while holding the offline rewrite boundary.

`IndexGenerationStore::inventory_all` takes the database writer lock, scans every local index file,
validates each immutable generation and current pointer, and rejects unknown files, malformed
names, corrupt generations, links, and dangling pointers. A verified inventory is tied to its
source `DatabaseId`; only such an inventory can approve a rewrite. Every registered index family
is marked for rebuilding in the new database.

Known exact backups, audit-complete backups, logical exports, and sharing exports are recorded by
kind and artifact digest. The caller declares whether its search for external copies was complete.
Listed copies remain outside the rewritten database and are not altered by this preview or by the
later offline rewrite. The report makes no secure-erasure claim for old backups or copy-on-write,
cloud-synced, or SSD media.

## M7-15 boundary

This milestone only produces an approved plan and stable fingerprint. M7-15 must re-check the live
source `DatabaseId`, revision, artifact digest, authorization, and inventories before rewriting to
a new database identity. It must retain the old database unchanged until the new destination has
been verified and the required audit publication boundary succeeds.
