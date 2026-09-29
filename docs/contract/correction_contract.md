# WorldDB 1.0 supplement – Explicit assertion and event correction

**Status:** accepted working contract for M0-04e

**Decision record:** [ADR-035](ADR-035-correction-actions.md)

**Scope:** completes Master §§2.2, 2.3.2, 5.3, 8, 17, and 31.2 for the `CorrectAssertion` command and the distinct event-correction command. It makes the preserved `LEGACY-AST-04` intent from the Consolidation Trace executable as explicit commands. It adds no persisted First-Class type or WDB invariant ID and predates the M0-02a closure of all 52 HARD source gaps recorded in ADR-039; the two GUARDED gaps remain open.

## 1. Command boundary and shared transaction

`CorrectAssertion` and `CorrectEvent` are typed commands submitted through the ordinary transaction boundary. They are not mutable methods on an existing record and are not persisted as new record types. They use a pinned base Snapshot, one `OperationId`, normal schema validation, current commit authorization, OCC, WriteSet conflict checks, and the ordinary Commitpoint/Recovery protocol. A command uses one set of semantics across Rust, CLI, desktop IPC, import, and any future adapter.

The correction payload identifies the target and its observed creation Revision. The replacement is a complete typed draft with all fields required by the corresponding immutable record form; adapters may prefill fields from the target for editing, but the command contains the explicit final values. The engine assigns each new record's `created_revision` at commit. No caller may ask the engine to mutate the target, infer an omitted field, choose a different target after a conflict, or turn `Corrects` into a lifecycle action.

```text
CorrectAssertion {
  operation_id: OperationId,
  base_snapshot: SnapshotId,
  target: AssertionId,
  expected_target_created_revision: Revision,
  replacement: AssertionDraft,
  retraction_reason: AssertionRetraction.reason
}

AssertionDraft {
  history_space_id: HistorySpaceId,
  layer_id: LayerId,
  perspective_scope: PerspectiveScope,
  epistemic_mode: EpistemicMode,
  subject: Subject,
  predicate_id: PredicateId,
  value: Value,
  polarity: Polarity,
  validity: AssertionValidity
}

CorrectEvent {
  operation_id: OperationId,
  base_snapshot: SnapshotId,
  target: EventId,
  expected_target_created_revision: Revision,
  replacement: EventDraft
}

EventDraft {
  history_space_id: HistorySpaceId,
  layer_id: LayerId,
  event_kind_id: EventKindId,
  participants: Participants,
  attributes: Attributes,
  event_time: EventTime
}
```

`OperationId` names the logical correction once across retries; `TransactionId` names one execution attempt. The Snapshot pins the target, schema, HistorySpace, Layer, and security view. Writes validate against the Post-Transaction SchemaAt state. `expected_target_created_revision` must match the immutable target visible in the base Snapshot; the transaction ReadSet also records that the target is active and that no later Retraction was observed. The base is never refreshed silently.

## 2. `CorrectAssertion`: one explicit three-record effect

The replacement must target the same logical assertion slot as the original: same `HistorySpaceId`, `LayerId`, `PerspectiveScope`, `EpistemicMode`, `Subject`, and `PredicateId`. For Master `Corrects` endpoint compatibility, the Proposition-Slot key is the same closed tuple as `MaskSelector::Slot`: `(Subject, PredicateId, PerspectiveScope, EpistemicMode)`; retaining `HistorySpaceId` and `LayerId` is an additional `CorrectAssertion` command constraint that keeps the replacement in the target's exact context. `Value`, `Polarity`, and `AssertionValidity` may be corrected, but each replacement field is explicit and validated by the pinned Post-Transaction schema. A different subject, predicate, HistorySpace, Layer, Perspective, or EpistemicMode is not a `CorrectAssertion`; the caller must use separately named ordinary writes and cannot attach an incompatible `Corrects` edge.

At the single commitpoint the command creates exactly these three domain-history records (policy-required AuditRecord and transaction metadata remain governed by their separate contracts):

```text
new Assertion { AssertionId, replacement fields, created_revision: R }
AssertionRetraction { AssertionRetractionId, assertion_id: target, reason: retraction_reason,
                      created_revision: R }
ProvenanceEdge { ProvenanceId, from: new Assertion, to: target Assertion,
                 relation: Corrects, created_revision: R }
```

The `Corrects(new, old)` direction is the Master-defined direction in the provenance dependency graph. The typed endpoint compatibility rules still apply: both endpoints are Assertions in the same proposition slot and the correcting Assertion is later on Transaction Time. The edge records why the new record relates to the old; it does not itself retract, close, mask, supersede, rank, or change the Resolution of either Assertion. The explicit `AssertionRetraction` is the only effect that makes the original inactive from revision `R` onward. Raw history retains the original, its Retraction, the replacement, and the ProvenanceEdge.

Before `R`, historical queries see the original without this correction. At and after `R`, ordinary lifecycle projection treats the original as retracted and the new Assertion as a newly recorded candidate, subject to its own World-Time validity, schema, Security, Masking, and Resolution rules. `CorrectAssertion` does not assert that the replacement is necessarily the resolved answer: ordinary Resolution may still yield `Unknown` or `Conflict` because of other candidates or context.

The target must be visible to the caller, match the expected creation Revision, and be active at the base Snapshot. A visible target already retracted at that Snapshot fails as `CommitError::Validation(ValidationError::InvalidReference)`. A stale expected Revision or a target that becomes retracted or otherwise conflicts before commit produces the ordinary visible `CommitOutcome::Conflict`; the caller must preview/rebase and explicitly resubmit. A hidden or unauthorized target keeps the protected Master mapping. Archive state is orthogonal: correction does not unarchive, archive, or otherwise alter any ArchiveTransition. Any change to archive state is a separately named operation.

## 3. `CorrectEvent`: new Event and explanatory edge only

The replacement Event must retain the target's `HistorySpaceId` and `LayerId`. Its `EventKindId` must be identical to the target or be an explicitly schema-compatible successor under the pinned Post-Transaction SchemaAt state, as required by the Master `Corrects` endpoint matrix. Participants, Attributes, and EventTime are complete explicit replacement values and pass ordinary EventKind constraints, role/cardinality rules, temporal validation, and EventTime limits.

At one commitpoint `CorrectEvent` creates exactly two domain-history records: one new immutable Event and one `ProvenanceEdge { from: new Event, to: target Event, relation: Corrects }`, both at the same Revision. Policy-required AuditRecord and transaction metadata remain governed by their separate contracts. It creates no `EventRetraction`, `EventSpanClosure`, `EventMask`, or `EventRelation`. The old Event remains active and queryable; both old and new Events remain distinct records. `Corrects` is explanatory Provenance: it does not retract, deduplicate, temporally order, or causally connect Events. It does not imply `Before`, `SameTime`, or `Causes`.

An EventRetraction is a separate explicit lifecycle command. It may accompany another command only when the caller explicitly includes that Retraction command and its reason in the same ordinary transaction; `CorrectEvent` never synthesizes one. The UI must show that the old Event remains active unless that separate action is explicitly selected.

## 4. Authorization and validation boundaries

The specialized operation Capability is required in addition to the exact record-class writes produced by the command. `CorrectAssertion` requires `AssertionCorrect`, `AssertionCreate`, and `AssertionRetract`; `CorrectEvent` requires `EventCorrect` and `EventCreate`. These capabilities are conjunctive and none implies another for a separate command. Event correction does not require `EventRetract` because it creates no Retraction. Every effect still passes the M0-04c checks for target reads, fields, references, HistorySpace/Layer, relationship, and endpoints. Both commands separately require `ProvenanceCreate` and `RelationshipCreate(Provenance(Corrects))` for their edge. All writes use `AuthorizationNow`; historical `AuthorizationAtRevision` cannot authorize a correction.

| Check | `CorrectAssertion` | `CorrectEvent` |
|---|---|---|
| Operation | `AssertionCorrect` | `EventCorrect` |
| Target read | `AssertionRead`, owning `HistorySpaceRead` and `LayerRead`; `FieldRead` for every target field needed to construct/check the replacement | `EventRead`, owning `HistorySpaceRead` and `LayerRead`; `FieldRead` for every target field needed to construct/check the replacement |
| New record | `AssertionCreate`; `FieldWrite` for each supplied Assertion field; `LayerWrite` for the target Layer; `entity.reference` for every Entity reference in its Subject or Value; `perspective.use` when its scope is a Perspective | `EventCreate`; `FieldWrite` for each supplied Event field; `LayerWrite` for the target Layer; `entity.reference` for every Entity participant or Entity-valued attribute |
| `Corrects` edge | `ProvenanceCreate` and `RelationshipCreate(Provenance(Corrects))`; both typed endpoints must be referenceable and visible to this operation | `ProvenanceCreate` and `RelationshipCreate(Provenance(Corrects))`; both typed endpoints must be referenceable and visible to this operation |
| Retraction | `AssertionRetract` authorizes the explicit required AssertionRetraction; it does not by itself authorize `CorrectAssertion` | No EventRetraction is generated and `EventRetract` is not required |

All listed rights are conjunctive. A missing grant or any matching Deny aborts the full command. Field and endpoint authorization is checked before relevant values enter validation, preview, or conflict details. When object existence is protected, missing and unauthorized targets use the same safe public mapping. UI enablement and preview are advisory; the engine repeats target-state, capability, FieldRead/FieldWrite, relation, schema, and endpoint checks at commit. A SecurityEpoch or policy change before commit causes the normal authorization failure; no part of the correction is published.

## 5. Model, engine, and UI behavior

The model exposes two different typed commands and two typed receipts; it has no generic `Correct(record)` operation that could hide which records are produced. An adapter sends the complete command through the ordinary transaction API. It cannot send only a `Corrects` edge and ask the engine to infer whether a Retraction is intended.

Before confirmation, the UI displays the target, the complete proposed record, and the exact effects. For `CorrectAssertion`, the preview shows the new Assertion, the explicit Retraction of the original, and `Corrects(new, old)` as one atomic action. For `CorrectEvent`, it shows the new Event and `Corrects(new, old)` and explicitly states that the original Event remains active. Any separate EventRetraction has its own control, reason, permission check, and clearly separate effect. The UI may prefill a replacement from the target, but every value that will be written is present in the command payload.

The preview is not a commit guarantee: schema head, target lifecycle state, endpoint graph, SecurityEpoch, and current capabilities are revalidated at commit. Denied or protected data is not included in previews. The UI does not claim that a Correction is an in-place edit, that `Corrects` retracts, or that an Event correction changes chronology. It reports success only after the complete receipt is known. On `UnknownCommitOutcome`, it queries the same OperationId and does not offer a fresh OperationId retry while status is unresolved.

## 6. Atomicity, retry, and fault outcomes

1. Parse, target resolution, expected-Revision check, active-state check, replacement validation, exact Corrects endpoint compatibility, reference checks, Provenance duplicate/cycle validation, budgets, and authorization all complete before the transaction can publish.
2. `CorrectAssertion` has one write set containing the new Assertion, the explicit AssertionRetraction, and the Corrects ProvenanceEdge. `CorrectEvent` has one write set containing the new Event and the Corrects ProvenanceEdge. Each set, any AuditRecord required by the active audit policy, the OperationId dedup entry, and the CommitReceipt share the ordinary Master commitpoint and one Revision. There is no partial visibility.
3. A failure before the commitpoint leaves every target and all correction effects unchanged. Validation, reference, authorization, budget, ID-collision, Provenance-cycle, required-audit, storage, and cancellation failures cannot publish only a subset. Cancellation before the commitpoint aborts the full write set; after the Master commitpoint it is `too_late` and the commit is reconciled normally. A concurrent target-state change is the normal `TransactionConflict`, not a silent retry or new target selection.
4. After the commitpoint, all required records are durable together. A crash before the response is resolved with `commit_status(operation_id)`. Repeating the same payload with the same OperationId returns the original receipt; a changed payload with that ID is `IdempotencyMismatch`. A client must not create a new OperationId until the old one is known `NotCommitted`.
5. The successful receipt includes the common Revision, OperationId, target ID, new record ID, each generated Retraction/Provenance ID that applies, and the durability outcome. It is the only success signal the UI may present. Unknown commit outcome is never displayed as failure or success until reconciled.

Schema or endpoint incompatibility uses the Master `ValidationError` family, authorization uses the `CommitError::Authorization` mapping, OCC changes use `CommitOutcome::Conflict`, and an uncertain commit uses `CommitError::UnknownCommitOutcome`. Hidden targets preserve the Master not-found/unauthorized mapping. New correction-specific error aliases do not erase these underlying distinctions.

## 7. Existing-contract cross-check

- Master §2.2/§2.3.2 owns immutable records and separates AssertionValidityClosure, AssertionRetraction, Archive, and Purge. This command uses an explicit Retraction only for Assertion correction.
- Master §31.2 and ADR-026 own Corrects direction, endpoint family/slot compatibility, later Transaction Time, duplicate rules, and the shared Provenance cycle graph. The correction command creates no alternative relation semantics.
- Master §8–§10 owns atomic commit, OperationId idempotency, OCC, conflict, recovery, and Snapshot behavior; both commands use those mechanisms without a special commit path.
- M0-04c owns the closed AssertionCorrect/EventCorrect capabilities, operation/resource composition, current policy recheck, field/endpoint rights, and safe error mapping.
- The Consolidation Trace `LEGACY-AST-04` is preserved as the Assertion three-record command. Master `WDB-EVT-012` remains distinct: an Event correction adds a new Event and `Corrects`, while EventRetraction is not implied.
- No new persisted type or WDB invariant ID is introduced. Model/property, authorization, UI-preview, retry, and crash/fault tests remain implementation obligations in M2/M3/M4/M5/M8.
