# WorldDB 1.0 supplement – Security policy and capability evaluation

**Status:** accepted working contract for M0-04c

**Decision record:** [ADR-033](ADR-033-security-policy.md)

**Scope:** completes Master §§6–8, 14, 17–18 and 31.5–31.6 for policy records, principals, roles, capabilities, scope matching, historical security evaluation, and `SecurityEpoch`. The additive storage-format permission is reviewed in ADR-041. The contract adds no WDB invariant IDs and predates the M0-02a closure of all 52 HARD source gaps recorded in ADR-039; the two GUARDED gaps remain open.

## 1. Security history records

Security policy is a project-wide, append-only namespace on the shared `Revision` axis. It is not domain HistorySpace content and is not a `RecordRef`, Evidence, or Provenance endpoint. A normal database transaction may contain domain changes and security changes together; all become visible at the same single commit revision. Exactly one SecurityPolicyRecord contains the non-empty set of security changes for a transaction; a transaction without security changes has no such record.

```text
SecurityPolicyRecord {
  policy_record_id: SecurityPolicyRecordId,
  recorded_revision: Revision,
  actor: PrincipalId,
  security_epoch_after: SecurityEpoch,
  changes: NonEmptyList<SecurityPolicyChange>
}

SecurityPolicyChange =
    PrincipalRegistered { principal_id: PrincipalId }
  | PrincipalStateChanged { principal_id: PrincipalId, state: Active | Disabled | Retired }
  | RoleRegistered { role_id: RoleId, symbol: Symbol }
  | RoleRetired { role_id: RoleId }
  | RoleAssigned { assignment_id: RoleAssignmentId, principal_id: PrincipalId,
                   role_id: RoleId, scope: PolicyScope }
  | RoleAssignmentRevoked { assignment_id: RoleAssignmentId }
  | CapabilityRuleAdded { rule_id: PolicyRuleId, subject: PolicySubject,
                          capability: Capability, effect: Allow | Deny,
                          scope: PolicyScope }
  | CapabilityRuleRevoked { rule_id: PolicyRuleId }

PolicySubject = Principal(PrincipalId) | Role(RoleId)
SecurityEpoch { value: u64, established_at: Revision }
```

`PrincipalId`, `SecurityPolicyRecordId`, `RoleId`, `RoleAssignmentId`, and `PolicyRuleId` are distinct validated IDs. A principal identifies an authenticated actor supplied by a trusted host adapter; credentials and secrets are not stored in WorldDB policy records. The adapter cannot choose capabilities or act as another PrincipalId without an explicit authenticated delegation protocol; delegation and impersonation are outside 1.0.

Each change is immutable. State changes, role assignment revocation, and rule revocation append new policy history; they never rewrite or delete prior records. Retiring a Principal is terminal. A Disabled or Retired principal receives no capabilities. A retired role cannot be assigned, and its earlier assignments/rules remain visible to historical policy snapshots. Role registration rejects a duplicate RoleId or Symbol; RoleId and Role Symbol are stable and are not reused. Renaming requires a new RoleId and explicit assignment/rule migration. Roles do not inherit from other roles. A role's complete policy bundle is the canonical set of its active typed CapabilityRules; the evaluator never branches on the role's Symbol or name.

`GM` and `Player` are ordinary versioned Roles whose policy bundles contain explicit grants. Their names are not bypass flags. `AdminRawRead` is a separate Capability and is never implied by either role name. On the one-time `CreateProject` bootstrap, a trusted host-authenticated creator is registered as a Principal and the complete initial Role, assignment, and capability rules are written with the initial project policy snapshot. The bootstrap PolicyRecord actor is that authenticated creator PrincipalId; the bootstrap is allowed only when creating an empty project and cannot mutate an existing database. That initial state has no persistent superuser path; every later action uses the recorded evaluator. M0-05 defines the user-facing bootstrap flow and its explicit initial grants.

## 2. Closed capability catalog and scopes

`Capability` is a closed typed enum, not a string, pattern, callback, or user-defined code:

```text
Capability =
    ProjectRead
  | SchemaRead | SchemaManage
  | HistorySpaceRead | HistorySpaceCreate | HistorySpaceTransfer
  | LayerRead | LayerWrite | LayerManage
  | AssertionRead | AssertionCreate | AssertionCorrect | AssertionRetract
  | EventRead | EventCreate | EventCorrect | EventRetract | EventSpanClose
  | MaskRead | MaskCreate | MaskRetract
  | EventMaskRead | EventMaskCreate | EventMaskRetract
  | ReplacementBoundaryRead | ReplacementBoundaryCreate | ReplacementBoundaryRetract
  | LifecycleRead
  | SourceRead | SourceCreate | SourceSupersede
  | EvidenceRead | EvidenceCreate | EvidenceRetract
  | ProvenanceRead | ProvenanceCreate | ProvenanceRetract
  | Archive | Unarchive
  | Entity(EntityAction) | Perspective(PerspectiveAction)
  | FieldRead | FieldWrite
  | RelationshipRead | RelationshipCreate | RelationshipRetract
  | QueryResolve | QuerySearch | QueryFullText | QueryExplain
  | QueryGraphTraverse | QueryAggregate
  | RawHistoryRead | AdminRawRead
  | MigrationPlan | MigrationExecute
  | DataImport | DataExport | BackupCreate | BackupRestore | Purge
  | JobRead | JobCancel | JobManage
  | SecurityPolicyRead | SecurityPolicyManage | SecurityPermissionHistoryRead
  | AuditRead | AuditExport | AuditConfigure | StorageFormatUpgrade

EntityAction = Create | Read | Reference | Retire
PerspectiveAction = Create | Read | Update | Use | Retire
```

The Entity and Perspective actions include the exact operation boundaries in M0-04. The remaining variants cover 1.0 record, query, schema, lifecycle, migration, backup, raw-history, policy, and audit operations. A new operation, record class, or relationship kind requires an explicit additive Capability variant and compatibility review. Unknown variants fail closed. A CapabilityRule is valid only for an operation/resource pairing allowed by this catalog; invalid pairings are rejected when the policy record is validated.

The typed operation/resource map is closed as follows:

| Capability family | Permitted target | Required composition |
|---|---|---|
| `ProjectRead` | project metadata/open | no domain read is implied |
| `SchemaRead`, `SchemaManage` | project schema records | data reads/writes still need their own rights |
| `HistorySpace*`, `Layer*` | exact HistorySpace or Layer | record operation and LayerWrite are both required for a write |
| Assertion, Event, Mask, EventMask, ReplacementBoundary, Source, Evidence, Provenance read/create/retract actions | matching record class | reads require class Read; writes require the exact action |
| `SourceSupersede` | a new Source plus explicit lineage | also requires `SourceCreate`; never edits the old Source |
| `LifecycleRead`, `Archive`, `Unarchive` | exact lifecycle record or ArchiveTargetRef | lifecycle records also require visibility of the typed target |
| `Entity(action)`, `Perspective(action)` | exact catalog identity/action | preserves M0-04's reference/use conjunction |
| `FieldRead/Write` | `PolicyScope.field` selector | record-class Read/write action is also required |
| `RelationshipRead/Create/Retract` | `PolicyScope.relationship` selector | Read requires endpoint visibility; Create/Retract is the exact relation action and requires endpoint reference/record rights |
| `Query*` | exact query operation | corresponding class/field read rights are still required |
| `RawHistoryRead` | raw record stream | corresponding class Read rights are required |
| `AdminRawRead` | raw administrative projection | both RawHistoryRead and AdminRawRead are required; neither implies the other |
| `StorageFormatUpgrade` | one exact database/project | requires the explicit current grant plus `BackupCreate` and `BackupRestore` and a verified Safe Restore Point; implies no schema migration or backup permission |
| `JobRead/Cancel/Manage` | exact Job operation | the owner needs the exact operation Capability; cross-owner read/cancel additionally requires JobManage |
| `Migration*`, import/export, backup/restore, `Purge` | exact administrative operation | target-scope and all underlying read/write rights are required |
| `SecurityPolicy*`, `SecurityPermissionHistoryRead`, `Audit*` | security or audit namespace | each operation is independent; no one variant implies another |

The star in this table is explanatory notation only; it is not a persisted wildcard or Capability variant. Each row expands only to the explicitly declared enum values.

```text
PolicyScope {
  history_space: Option<HistorySpaceId>,
  layer: Option<LayerId>,
  record: Option<RecordRef>,
  field: Option<FieldSelector>,
  relationship: Option<RelationshipSelector>
}

FieldSelector =
    AssertionSubject | AssertionPredicate | AssertionValue(PredicateId)
  | AssertionPolarity | AssertionValidity | AssertionPerspective | AssertionEpistemicMode
  | MaskSelector | MaskValidity
  | ReplacementBoundarySubject | ReplacementBoundaryPredicate | ReplacementBoundaryValidity
  | EventKind | EventParticipant(EventKindId, EventRoleId)
  | EventAttribute(EventKindId, EventAttributeId) | EventTime(EventKindId)
  | EventMaskTarget
  | SourceKind | SourceLocator | SourceContentDigest | SourceMetadata

RelationshipSelector =
    EventRelation(Before | SameTime | Causes)
  | Evidence(Supports | Contradicts | Documents)
  | Provenance(Corrects | DerivedFrom | ResultedFrom)
  | LifecycleTarget
```

An all-empty scope is explicit project scope. Every populated selector must match the access target; selectors combine with AND. HistorySpace scope matches the owning HistorySpace of each record, not the selected child view, parent, or descendant. Layer and Record match the target's exact typed IDs. There is no implicit ancestry inheritance, nearest-scope rule, wildcard string, or automatic propagation. A RoleAssignment scope and CapabilityRule scope must both match for the role rule to apply.

Field-level permissions in 1.0 cover the closed `FieldSelector` variants above, including Source Locator, ContentDigest, and Metadata as separate field classes. Assertion values are selected by PredicateId; Event participants and attributes use their historical role/attribute IDs. A selector without a schema-defined ID is valid only for its exact non-schema field. Relationship permissions cover the closed EventRelation, Evidence, Provenance, and LifecycleTarget families above. Policy scopes are validated against the selected schema snapshot; an unknown or retired referenced field, layer, or schema ID cannot grant access.

## 3. Effective capability evaluation

The evaluator receives an authenticated PrincipalId, one typed operation, the target's resource identifiers, the selected data snapshot, and a `SecurityEvaluationMode`. It evaluates only the policy snapshot required by that mode. It performs these steps in order:

1. Resolve the PrincipalId and its state. Missing, Disabled, or Retired principals are denied.
2. Collect active direct rules for the Principal and rules from each active, matching RoleAssignment. Role assignments do not confer capabilities beyond their intersection with the selected Role's active rules.
3. Match the requested typed Capability and every scope selector exactly. Unknown actions, scopes, IDs, or policy-history states fail closed.
4. If any matching rule is `Deny`, deny. Otherwise allow only when at least one matching rule is `Allow`. No matching rule means deny. There is no default allow, role priority, last-writer-wins, or owner bypass.
5. For an operation requiring multiple permissions, require every listed permission. One grant cannot substitute for another required operation, field, relationship, endpoint, schema, or identity-reference permission.

All active RoleAssignments are unioned before the Deny check; a deny from any matching Principal or Role rule wins over every grant, including a more narrowly scoped grant. A narrower rule therefore cannot override a wider deny. To permit an exception, the denying rule must be revoked or narrowed in a new policy transaction. This gives policy merges one deterministic result independent of insertion order.

`AuthorizationRequest` names the operation explicitly; it never derives permission from the renderer, menu state, Perspective, EpistemicMode, Layer precedence, record values, or caller-supplied booleans. The engine validates requests and rechecks them against current policy at commit. A write requires its domain-operation Capability plus each applicable record, field, relationship, Entity reference, and Perspective use Capability. Failure aborts the whole transaction before any effect.

## 4. Record, field, relationship, and query enforcement

For a HistorySpace-owned record read, the caller needs `HistorySpaceRead` for its owning HistorySpace, `LayerRead` for its Layer, and the record-class read Capability. Project-wide records have no implicit HistorySpace or Layer grant. A write to a HistorySpace/Layer-owned record needs its exact domain-operation Capability scoped to the target and `LayerWrite` for the record's Layer; creation of a new HistorySpace is scoped to its parent (or explicit project scope for a root). A HistorySpace transfer evaluates source and target rights separately and requires both. These checks use the record's source HistorySpace and Layer, not an inherited child view.

The read Capability for a record class (such as `AssertionRead`, `EventRead`, `SourceRead`, or `EvidenceRead`) controls whether a record may enter a caller-visible candidate stream. It is checked before Masking, Resolution, Search result assembly, aggregation, Explain, and serialization. Every field required to evaluate a candidate, including an Assertion Value used for resolution, must also pass FieldRead before candidate creation; otherwise that candidate is excluded and its hidden value cannot affect any outcome. Unauthorized records are excluded before counts or conflict details are computed. Record-class read and operation Capabilities are separate; QueryGraphTraverse, QueryFullText, QueryAggregate, or AdminRawRead cannot be inferred from ordinary read access.

FieldRead with a matching `PolicyScope.field` is required for every field value used in a projection, filter, sort, grouping, search, Explain, or aggregate input. Query compilation checks all referenced field permissions before scanning records; an unauthorized field never influences a result, count, order, cursor, error detail, or timing-dependent early exit. For an explicitly projected field declared by the selected schema or record type, a caller without FieldRead receives the same fixed `Redacted` field state whether the record has a value or not; the marker reveals no value, presence, length, hash, or encoding. FieldWrite with a matching field scope is required for each supplied field; a denied or unknown field rejects the complete transaction.

RelationshipRead with a matching `PolicyScope.relationship` requires visible Read permission for every endpoint and endpoint field used by the response. RelationshipCreate or RelationshipRetract with a matching relationship scope is the exact relation operation and requires permission to reference every endpoint. Graph traversal expands only visible nodes and edges. A missing or hidden endpoint does not produce a hidden count, partial edge, different cursor, or distinguishable public error. Provenance uses the endpoint/field-right intersection required by WDB-PRV-010.

Entity references require `entity.reference` and the relevant record write; a Perspective-scoped action requires `perspective.use` and the relevant operation right. These checks apply equally to CLI, API, IPC, import, migration, and desktop requests. `Authorization` precedes Redaction; a redacted DTO state never grants access and cannot be used as a query or write value.

When object existence is protected, public mapping returns the same `NotFound`/`Forbidden` class, response shape, and safe fields for an unknown and an unauthorized target. It does not reveal hidden IDs in Counts, Explain, Conflict, Search, Graph, or Error payloads. This is a non-interference requirement; 1.0 does not claim a strict constant-time guarantee for complex queries.

## 5. Historical policy and `SecurityEpoch`

`PolicySnapshotAt(r)` is the immutable projection of all committed SecurityPolicyRecords through shared Revision `r`, including principal states, role definitions, assignments, rules, and its SecurityEpoch. It is derived/cacheable; a cache can be rebuilt from the policy history. Missing or corrupt required policy history is `SecurityHistoryCorrupt` and fails closed.

`AuthorizationNow` is the default for every read, including data reads at an older `RecordedAsOf`: current active policy controls access to historical records. `AuthorizationAtRevision(r)` is a separate explicit mode. Before opening it, the current policy must grant `SecurityPermissionHistoryRead`; this permission is checked at the current SecurityEpoch. The requested historical evaluation then uses only `PolicySnapshotAt(r)`. The target revision must be committed and its complete policy history available. Neither mode falls back to the other or to a neighboring revision.

`SecurityEpoch` is an unsigned u64 newtype initialized to 0 with the explicit initial project policy snapshot. Exactly one increment is committed for every later transaction containing one or more SecurityPolicyChanges; transactions without policy changes leave it unchanged. A transaction with multiple policy changes increments once. The new epoch, all policy records, the shared Revision, and required AuditRecord become visible at the same commitpoint. If increment would overflow, policy mutation fails as `SecurityEpochExhausted`; it never wraps, saturates, or resets.

Every Snapshot pins its data revision, policy snapshot, `SecurityEvaluationMode`, and corresponding SecurityEpoch. A cursor binds the PrincipalId, effective-capability fingerprint, data Snapshot, policy mode, current SecurityEpoch, and—when using `AuthorizationAtRevision`—the historical epoch. Before every page the engine checks AuthorizationNow and all bound epochs again. Any policy change increments the epoch and invalidates existing cursors with the uniform `CursorInvalidated` outcome. A cached authorization or query plan is reusable only when all policy, capability, resource-scope, schema, and epoch fingerprints match.

## 6. Policy mutations, audit, and operations

Every SecurityPolicyRecord is created through a typed policy operation inside a normal `WriteTransaction`; there is no direct file edit, renderer privilege, or side-channel mutation. Except for the one-time empty-project bootstrap in §1, the acting Principal needs current `SecurityPolicyManage`. Commit revalidates that permission, all affected IDs and scopes, post-transaction policy consistency, and the current SecurityEpoch. If another policy transaction commits first, the writer re-evaluates under the new policy; it proceeds only if still authorized and valid. Self-revocation is allowed and takes effect at the same commitpoint; it does not create an implicit replacement administrator.

Every committed security-policy change requires its safe AuditRecord in the same atomic durability unit, as required by Master §§18.2 and 31.6. The audit records Actor PrincipalId, action, affected object class, result, commit Revision/OperationId, SecurityEpoch, and policy fingerprint; it omits capability secrets and sensitive field values. Audit append, sync, or required-record failure aborts the policy transaction. A rejected attempt is audited only when the active AuditPolicy requires it. Pure AdminRawRead continues to use the separate durable RawReadAttempt protocol and produces no data Revision.

SecurityPolicyRead and AuditRead/Export/Configure are separate Capabilities. Policy and audit history are never exposed by ordinary RawHistoryRead. Public DTOs expose only policy fields the caller may read; diagnostic events omit Principal names, Values, query text, and paths by default under the Master telemetry contract.

## 7. Existing-contract cross-check

This supplement preserves:

- `WDB-SEC-001–005`: Principal/Perspective separation, authorization before redaction/resolution, non-interference, graph visibility, and explicit historical permission mode.
- `WDB-RES-003`, `WDB-PRV-010`, `WDB-API-007/008/009`, and `WDB-AUD-001–013`: filter-before-resolution, endpoint-right intersection, cursor reauthorization/invalidation, and durable audit.
- M0-04 operation boundaries for Entity and Perspective, including `entity.reference` and `perspective.use`.
- Shared Revision commits, Snapshot pinning, closed `RecordRef`, schema-at-revision, and the rule that UI state is never authority.

Implementation verification must cover allow/deny precedence, role union and revocation, every scope dimension, every Capability catalog group, field projection/filter/sort denial, relationship endpoint intersection, query non-interference, historical policy modes, SecurityEpoch atomicity/overflow, cursor invalidation, policy history corruption, audit fault atomicity, and cross-boundary parity for Rust API/CLI/IPC/desktop. There is no security engine or UI yet; these are M3/M4/M5/M6 and M8 implementation tests.
