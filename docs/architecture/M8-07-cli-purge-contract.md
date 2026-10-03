# M8-07 – CLI purge contract

**Task:** M8-07

**Status:** implemented for the Windows development profile; Linux/macOS verification is deferred to M9-07.

## Commands

```text
worlddb-cli [--format human|jsonl] v1 purge plan <database> \
  --destination <new-database-dir> --report <new-report.json> \
  --target <typed-identity>... \
  --mode reject-if-referenced|cascade \
  --external-inventory complete|incomplete \
  [--known-copy <kind>:<blake3-hex>]...

worlddb-cli [--format human|jsonl] v1 purge run <database> \
  --destination <new-database-dir> --confirm-plan <fingerprint> \
  --target <typed-identity>... \
  --mode reject-if-referenced|cascade \
  --external-inventory complete|incomplete \
  [--known-copy <kind>:<blake3-hex>]...
```

Targets use the typed identity syntax already accepted by import mappings: named identity families use `family:<uuid>` and durable records use `record:<RecordRef wire tag>:<uuid>`. Modes and external-inventory status are mandatory. A known external copy uses one of `exact-backup`, `audit-complete-backup`, `logical-export`, or `sharing-export`, followed by its 64-character lower-case BLAKE3 digest.

## Plan and confirmation

Both commands require the current host principal to have `ProjectRead` and `Purge`; complete logical inventory also passes through the existing `DataExport` authorization boundary. Planning holds the source writer lock while it exports the full snapshot, inventories every local index generation, validates external-copy declarations, and computes the exact typed target and dependant closure.

`purge plan` writes a new path-free JSON report outside the source database and destination. The report lists target identities and records, dependants, all affected records, local index generations, known external copies, completeness declarations, the source artifact digest, and a stable plan fingerprint. It makes no secure-erasure claim. `reject-if-referenced` reports `approval_possible=false` when any dependant exists. `cascade` approves only the exact computed dependant closure.

`purge run` repeats the inventory under the source writer lock. The command requires explicit targets, mode, a new destination, and the external-copy inventory again, plus the exact plan fingerprint. The fingerprint binds the approved data and sidecar inventory; the destination is separately checked as a new, safe output path. Any source snapshot, index inventory, external declaration, or plan change prevents execution. A reject-if-referenced plan with dependants cannot run.

## Publication and source protection

The destination must be new, its parent must already exist, and it must not overlap the source database. The rewrite creates a fresh `DatabaseId`, reconstructs the retained logical snapshot, verifies the destination, and atomically publishes a Required `PurgePublication` audit record together with the final revision. `PURGE_REPORT` is stored inside the destination and binds both database IDs, the source revision and artifact digest, the plan fingerprint, removed identities, retained external-copy inventory, rebuilt index families, and audit identifiers. The CLI verifies the persisted report against the receipt before returning success.

The source is opened with an exclusive writer lock during each inventory phase and is never modified by plan or run. Destination and report paths must not pre-exist; no CLI option can select an in-place replacement. Output omits filesystem paths. External-copy completeness is an explicit operator declaration, and neither the report nor the CLI claims physical or secure erasure.

## Rewritten-snapshot verification detail

When a selected HistorySpace is removed, the destination no longer contains its definition. The rewrite verifier therefore exports only retained HistorySpaces while comparing the complete retained record set byte-for-byte with the source export minus the approved affected records. Retained child HistorySpaces remain valid because dependency closure includes required ancestry records.
