# ADR-031 – Operational Archive and explicit HistorySpace transfer

**Status:** Accepted for the WorldDB 1.0 working contract  
**Task:** M0-04a  
**Decision owner:** Luna (contract author under the user's instruction to execute the plan)  
**Normative detail:** [Archive and HistorySpace transfer supplement](archive_transfer_contract.md)

## Context

Master §2.3.2 distinguishes Archive from Closure, Retraction, and Purge, but only says that Archive changes operational visibility. ADR-021 requires one parent per HistorySpace and says content transfer is an explicit provenance-preserving transaction or migration. The transfer's record identity, reference, conflict, and atomicity behavior were not yet defined.

## Decision

1. Archive is a reversible project-wide operational state per persisted history record. A typed, append-only `ArchiveTransition` records `Archive` or `Unarchive` on the shared Revision axis. Operational query projections exclude archived records before resolution; explicit include-archived projections may show them. Raw History always retains and returns the record when authorized. Archive does not mutate or retract history and does not cascade.
2. HistorySpace transfer copies a caller-selected, revision-pinned set of HistorySpace-owned records into a different existing HistorySpace in the same database. It assigns fresh, same-type IDs, records the source-to-copy maps, and resolves references explicitly. A copy whose source family is admitted by the closed `DerivedFrom` matrix receives `DerivedFrom(source, copy)`; Mask, ReplacementBoundary, and EventMask copies receive a dedicated typed `TransferLineage` record instead. EventRelations use a separate typed map and remapped endpoints because they are not generic Provenance endpoints.
3. Project-wide schema and identity records and existing Evidence/Provenance metadata are not copied or mutated. Domain values, schema references, and world/event times stay unchanged. Effective lifecycle and archive state are handled by explicit plan policies.
4. Transfer is one normal idempotent transaction. Commit revalidates the plan against current schema, permissions, target head, typed references, uniqueness, lifecycle, and Provenance graph. Failure has no partial target effects; conflicts are reported, never resolved through content deduplication or last-writer-wins.
5. Transfer never creates another HistorySpace parent or modifies source, parent, or sibling history. A parent cutoff is not expanded; copied records are local to the destination. Cross-database movement remains the existing Logical Export/Import protocol.

## Consequences

- The closed `RecordRef` family gains `ArchiveTransitionId`/`RecordRef::ArchiveTransition`; `ArchiveTargetRef` is a separate closed subset and cannot target an ArchiveTransition.
- The closed `RecordRef` family also gains the persistent `TransferLineageId`/`RecordRef::TransferLineage`. It is a project-wide transfer metadata record, not a Provenance endpoint or HistorySpace-owned content record; it can be archived under the ordinary archive-target rule.
- Archive metadata is historically queryable at `RecordedAsOf`, while its operational projection remains separate from fact validity, retraction, resolution, and physical deletion.
- Repeated Archive/Unarchive commands, stale target heads, invalid reference choices, and identity collisions produce explicit failures. Duplicate domain content remains distinct and can produce the normal `Conflict` outcome.
- ArchiveTransition remains strictly later than target creation. A transfer using `PreserveArchiveState` fails closed when a selected source is archived; callers may transfer with `StartUnarchived` and archive the copy in a later explicit transaction.
- M1-09a models the typed archive transition; M1-17b codecs it; M2-05a implements the archive projection; M2-02a models transfer and its conflicts; M4-05b commits Archive and transfer as transactions.

## Verification obligations

The supplement's state table, query modes, and transaction rules are the acceptance checklist. Follow-up tests must prove raw-history retention, no archive/retraction/purge equivalence, typed exhaustive references, idempotent atomic transfer, conflict visibility, correct lifecycle/archive copy policy, family-appropriate `DerivedFrom` or `TransferLineage`, and unchanged source/parent/sibling state.
