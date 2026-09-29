# WorldDB 1.0 supplement – Archive and HistorySpace transfer

**Status:** accepted working contract for M0-04a  
**Decision record:** [ADR-031](ADR-031-archive-transfer.md)  
**Scope:** completes Master §§2.1.1, 2.3.2, 15.2–15.3, 31.2 and 31.4 for operational Archive and explicit same-database HistorySpace content transfer. Archive and transfer never mutate source history.

## 1. Archive state and record form

Archive is a reversible, project-wide operational visibility state for one persisted history record. It does not close world-time validity, retract a record, remove bytes, change schema, or change HistorySpace ancestry. It applies to the selected record in every HistorySpace in the database, including inherited views.

Each state change is an immutable lifecycle record on the shared `Revision` axis:

```text
ArchiveTransition {
  archive_transition_id: ArchiveTransitionId,
  target: ArchiveTargetRef,
  action: Archive | Unarchive,
  created_revision: Revision
}
```

`ArchiveTargetRef` is an exhaustive, closed subset containing every current 1.0 `RecordRef` variant except `ArchiveTransition`; it has one typed case for each admitted variant and a validating `TryFrom<RecordRef>`. It therefore admits Assertion, Mask, ReplacementBoundary, Event, EventMask, EventRelation, Source, Evidence, Provenance, and every concrete lifecycle record, including Entity/Perspective retirement. It cannot target itself. It has no string, generic UUID, HistorySpace, schema, principal, transaction, snapshot, job, or audit escape hatch. Each transition has a concrete ID and its own `RecordRef::ArchiveTransition(ArchiveTransitionId)` variant, as required by WDB-LFC-002. It receives a distinct `WireTag` from the central format registry.

`ArchiveTransitionId` and `RecordRef::ArchiveTransition` add one concrete lifecycle ID and one case to the exhaustive lists in Master §§3.1/3.2. Neither aliases an existing ID or wire tag; decoder limits and the format registry are completed in M0-11.

The current state of a target at `RecordedAsOf` is the state after its latest visible transition; before its first transition it is `Unarchived`. `Archive` is valid only from `Unarchived`; `Unarchive` is valid only from `Archived`. Repeating the current state fails as `AlreadyArchived` or `NotArchived`. Concurrent changes to one target conflict through normal OCC validation; revision order or record-ID order is never used as last-writer-wins.

Archiving a lifecycle record hides only that record from an operational projection. It does not undo its effect on the target. No archive transition cascades to related records, masks, events, Evidence, or Provenance. It is a separate authorized transaction and does not imply Retraction, Closure, or Purge. Raw history retains both the target and every ArchiveTransition.

## 2. Operational visibility and raw history

The normal resolved/search/count/graph query uses `ArchiveVisibility::Operational`: after existing security filtering and before resolution or aggregation, it excludes records whose archive state is `Archived` at the bound revision. An explicit `ArchiveVisibility::IncludeArchived` can include them for an authorized caller. This filter does not change stored records, lifecycle effects, candidate ordering, ContextPrecedence, or HistorySpace inheritance. Resolution can therefore still return `Conflict` when distinct active records remain.

Raw History is a separate mode and always returns archived records alongside unarchived records when the caller is authorized to read them. Archive visibility never filters raw-history results. A query at a revision before an ArchiveTransition sees the prior state; a query after an Unarchive sees the target as operationally visible again. Count, Exists, search, and graph traversal use the same archive mode as the query that produced their input.

The actions `record.archive`, `record.unarchive`, and `archive.read` identify the authorization boundaries. M0-04c defines policy storage and evaluation while preserving these actions. UI state is not authority. Hidden targets and archive state follow the existing non-interference and Forbidden/NotFound mapping.

## 3. HistorySpace transfer plan

`TransferHistorySpaceContent(plan)` copies an explicit set of records visible in a pinned source `HistorySpace` snapshot into a different, existing target `HistorySpace` in the same `DatabaseId`. Source and target IDs must differ. Cross-database transfer uses Logical Export/Import instead. This command is not a new branch or a HistorySpace merge primitive.

The immutable, reviewable `TransferPlan` binds:

```text
TransferPlan {
  database_id: DatabaseId,
  source_history_space_id: HistorySpaceId,
  source_recorded_as_of: Revision,
  target_history_space_id: HistorySpaceId,
  expected_target_head: Revision,
  selected_records: NonEmptySet<HistorySpaceContentRef>,
  record_id_map: NonEmptyMap<HistorySpaceContentRef, HistorySpaceContentRef>,
  selected_event_relations: Set<EventRelationId>,
  event_relation_id_map: Map<EventRelationId, EventRelationId>,
  external_reference_decisions: Map<RecordRef, RetainVisible | Reject>,
  lifecycle_policy: CopyEffectiveLifecycle | OmitWithAcknowledgement,
  archive_policy: PreserveArchiveState | StartUnarchived,
  plan_fingerprint: Digest
}
```

`HistorySpaceContentRef` is a closed subset containing Assertion, Mask, ReplacementBoundary, Event, EventMask, and their concrete lifecycle records, except the project-wide `EventRelation` and `EventRelationRetraction` records. Only records in this subset are copied by `record_id_map`. Their new records keep their typed family and all domain values, World-Time/Event-Time values, EntityIds, PerspectiveIds, Schema IDs, and LayerIds. The HistorySpace field is set to the target. Project-wide Schema, Entity, EntityType, Perspective, Layer, Source, Evidence, Provenance, and Entity/Perspective lifecycle records are not copied or reidentified. Existing project-wide metadata remains unchanged.

`EventRelation` has no HistorySpace field. A plan may therefore select any relation visible at the source revision separately from `selected_records`. Each selected relation gets a fresh `EventRelationId`; its endpoints are mapped to copied Events or explicitly retained only when the original Event is visible from the target HistorySpace at commit. Its relation kind is preserved and the normal event-relation graph validation runs on the complete post-transaction state. A selected relation's effective `EventRelationRetraction` follows `lifecycle_policy` and targets the mapped relation ID. Unselected relations remain unchanged and are not implied copies. Relation lineage is inspectable through the explicit relation ID map and the `DerivedFrom` edges on its mapped Event endpoints; EventRelation itself is not admitted to generic `ProvenanceEndpointRef`.

Every copied record receives a fresh ID of the same concrete type. The complete source-to-target ID map is visible in the plan and outcome. An ID collision, duplicate mapping, type-changing mapping, or mapping to an already existing record fails; the engine never silently adopts or deduplicates an existing identity. References among copied records are rewritten by the explicit map. A reference outside the selected set is retained only when the plan explicitly says `RetainVisible` and validation proves that the referenced record is visible and admissible from the target HistorySpace at commit. Otherwise the transfer fails before publication.

The plan includes every effective Closure/Retraction record needed to preserve the selected content at `source_recorded_as_of`, unless `OmitWithAcknowledgement` is selected and the preview reports the resulting lifecycle difference. Archive state is not a domain lifecycle effect. `PreserveArchiveState` creates an ArchiveTransition for each copied record or selected EventRelation whose source is archived; `StartUnarchived` leaves each copy unarchived. The preview lists those transitions explicitly.

For every copied `HistorySpaceContentRef`, the same transaction creates a `ProvenanceEdge` with `relation: DerivedFrom`, `from: source_record_ref`, and `to: copied_record_ref`. This records the exact origin of each new identity. Existing Evidence and Provenance edges remain attached to their original records and are not rewritten or copied implicitly. A caller can add separate, explicitly authorized Evidence or Provenance records for a copy. The new lineage edge remains project-wide meta-history and is returned in the outcome; a context-bound provenance query emits it only when both endpoints are visible and authorized in that query context. EventRelation lineage uses the separate rule above because generic Provenance does not accept EventRelation endpoints. If an edge, endpoint, cycle, or duplicate constraint makes the lineage invalid, the entire transfer fails.

## 4. Transaction, conflict, and outcome rules

The transfer is one ordinary validated transaction and publishes at most one shared `Revision`. Its only writes are the planned target-local copies, selected remapped EventRelations, required lifecycle copies, explicitly planned archive transitions, and their `DerivedFrom` lineage edges. It uses the normal `OperationId` idempotency contract. A retry with the same ID and payload returns the same receipt; a changed payload with that ID fails.

At commit, the engine revalidates the plan fingerprint, source snapshot, target head, all typed references, current schema, capabilities, ID uniqueness, lifecycle effects, and Provenance graph against the complete post-transaction state. A changed target head yields `TransactionConflict` and requires a new preview. Any rejected reference, schema or lifecycle violation, ID collision, authorization failure, or graph conflict leaves the target unchanged. The source, its parent, the target's parent and every sibling remain unchanged.

Transfer copies records; it does not infer identity from equal content. Equal Assertions or Events may be distinct valid records. They are not silently collapsed. Normal query resolution reports resulting `Conflict` where the existing schema and resolution policy require it. The preview reports structural faults separately from resolution conflicts; resolving a domain conflict requires a separate explicit record or migration decision.

An Outcome returns the committed Revision and the typed source-to-copy maps for `HistorySpaceContentRef` and selected EventRelations. It reports any expected `Known`/`Unknown`/`Conflict` projection changes from the preview. It never claims the transfer succeeded before durable transaction commit.

## 5. HistorySpace and invariants cross-check

- A transfer is explicit content copy between existing HistorySpaces. It never adds a parent, changes `base_revision`, moves a source record, changes schema identity, or mutates committed source/parent/sibling history.
- A Child continues to see its Parent only through its original `base_revision`. A transferred record in the Child is a new local record; it does not expand the inheritance cutoff.
- Source and target snapshots, target-head precondition, typed reference map, lifecycle policy, archive policy, and resulting ID map make each transfer reviewable and reproducible.
- Archive state is an operational projection. Closure remains world-time validity, Retraction remains transaction-time lifecycle, and Purge remains an offline rewrite to a new database with a PurgeReport.
- The contract preserves WDB-BRA-001–005, WDB-LFC-001/002, WDB-TX-001/003–006, WDB-PRV-002–013, WDB-REF-001–005, WDB-HIS-004, WDB-PRG-001/002, and WDB-EXP-002.

Implementation tests must cover Archive/Unarchive transitions at revisions on both sides of each change; operational, include-archived, and raw-history queries; every operation's duplicate/fault cases; valid transfer to a child and sibling; ID and reference collisions; preserved lifecycle effects and archive policy; duplicate facts that resolve to Conflict; `DerivedFrom` lineage; target-head conflict; graph/authorization failure atomicity; and unchanged source, parent, and sibling HistorySpaces.
