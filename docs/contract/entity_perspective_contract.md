# WorldDB 1.0 supplement – Entity and Perspective

**Status:** accepted working contract for M0-04  
**Decision record:** [ADR-030](ADR-030-entity-perspective.md)  
**Scope:** fills the creation, typed-reference, type-assignment, metadata, history, retirement, and authorization details missing from Master §§2.3, 2.3.1, 3.1, 17, and 31.3. It does not change unrelated source gaps; the 52 HARD gaps in M0-02a remain open.

This supplement is appended to the end of the Master working copy by `build_contract_sources.py`; it does not change the physical line numbers of the source-bound `MAIN-L` rows.

## 1. Identity and scope

- `EntityId`, `EntityTypeId`, and `PerspectiveId` remain distinct 128-bit newtypes in the database/project namespace. Zero and all-`ff` values are invalid. ID bits do not define chronology, authority, or user-visible priority.
- Entity and Perspective catalog state is project-wide and recorded on the database's shared `Revision` axis. It is not owned by a `HistorySpace`, `Layer`, or `Perspective` scope. Assertions, events, and their ordinary domain facts retain their existing HistorySpace, Layer, world-time, and epistemic semantics.
- Entity and Perspective identities are catalog objects, not `RecordRef` variants. A domain record refers to an entity only through a typed `EntityId` slot. `PerspectiveScope::Perspective` carries a typed `PerspectiveId`. Neither is coerced to a string, generic UUID, `PrincipalId`, or generic `RecordRef`.
- Catalog reads at `RecordedAsOf` expose only definitions and lifecycle changes committed by that revision. HistorySpace inheritance never rewrites the project catalog. Existing HistorySpace and schema cutoff rules continue to govern the domain records that refer to these identities.

## 2. Entity contract

The project catalog stores an immutable identity row:

```text
EntityCatalogEntry {
  entity_id: EntityId,
  entity_type_id: EntityTypeId,
  created_revision: Revision
}
```

Creation requires exactly one existing `EntityTypeId`. The type assignment is permanent in 1.0; an entity is never retyped in place. The entity ID is generated for local creation. Import may provide an ID only through an explicit, validated remap plan; a collision fails atomically and never silently adopts an existing identity. Imported references use the same remap table.

`EntityTypeDefinition` is a project-schema record and uses the existing “Schema records” First-Class family:

```text
EntityTypeDefinition {
  entity_type_id: EntityTypeId,
  symbol: Symbol,
  description: Option<String>,
  lifecycle: Active | Deprecated | Retired,
  created_revision: Revision
}
```

Its ID remains stable. Creating an entity requires an Active type. A Deprecated type requires the existing schema opt-in, capability, and warning contract. A Retired type cannot receive a new assignment. Retiring a type does not retire existing entities or invalidate their already assigned type; active predicate/event definitions may continue to reference those entities under their own constraints and lifecycle.

An entity has no generic JSON/map metadata or implicit name field. Entity-specific names, descriptions, and other world facts are typed Assertions over declared predicates. Their HistorySpace, Layer, validity, security, evidence, and provenance therefore use the ordinary assertion contract. The immutable catalog row contains identity and type only.

Typed entity references are allowed in `Assertion.Subject`, `Value::Entity`, and schema-declared Event participants. Construction, import, decode, and commit validate that the referenced identity exists at the operation's revision and satisfies the applicable `EntityTypeConstraint`, event role, and authorization rules. An unknown or unauthorized reference is returned through the existing non-leaking NotFound/Forbidden policy; it is never accepted as an unresolved raw ID.

Entity retirement is a project-wide, irreversible lifecycle transition at one published `Revision`:

```text
EntityRetirement {
  entity_retirement_id: EntityRetirementId,
  entity_id: EntityId,
  created_revision: Revision
}
```

At and after the retirement revision, new assertions, Events, or other domain writes may not introduce a reference to that entity. Existing records are neither rewritten, closed, nor retracted by retirement. They remain readable under the ordinary historical, HistorySpace, and authorization rules. Retirement does not physically delete the catalog identity; physical removal remains the separate offline Purge operation. There is no reactivation or ID reuse in 1.0. A duplicate retirement fails as `AlreadyRetired`; corrections to old records remain explicit lifecycle actions.

## 3. Perspective contract

Perspective is a project definition and epistemic scope, never a security actor:

```text
PerspectiveDefinitionRevision {
  perspective_id: PerspectiveId,
  display_name: Option<String>,
  description: Option<String>,
  recorded_revision: Revision
}
```

Creation generates a new `PerspectiveId` and its first definition revision. Present display-name and description values must be non-empty valid UTF-8 within the shared field-size limits. Rename/description changes append a new definition revision under the same ID; they do not rewrite older revisions or change the meaning of existing Assertions. Missing display metadata is valid and clients may display the typed ID. A Perspective ID is never imported as a Principal or treated as permission to act.

`WorldState` always uses `PerspectiveScope::World`. `Knows`, `Believes`, and `Claims` require exactly one existing, active `PerspectiveId`; no current/default perspective is inferred. Those modes remain separate partitions and never imply each other. Every user may create multiple perspectives with otherwise identical metadata; IDs, not labels, define identity.

Perspective retirement uses its own immutable lifecycle record:

```text
PerspectiveRetirement {
  perspective_retirement_id: PerspectiveRetirementId,
  perspective_id: PerspectiveId,
  created_revision: Revision
}
```

At and after retirement, new `Knows`, `Believes`, or `Claims` records cannot use that Perspective. Existing assertions remain unchanged and may still be queried if their data and the caller are authorized. A historical query at an earlier `RecordedAsOf` sees the earlier active definition. Retirement is terminal; creating a replacement Perspective requires a new ID. It neither creates a Principal nor changes world-state records.

Both retirement records have their own concrete lifecycle ID and `RecordRef` variant. Their target field is a direct strongly typed `EntityId` or `PerspectiveId`, not a `LifecycleTargetRef`; other lifecycle-record targets keep using that existing closed subset. Entity and Perspective identities themselves remain outside `RecordRef`, `EvidenceTargetRef`, and `ProvenanceEndpointRef` in 1.0, matching Master §33: Evidence about an entity is attached through Assertions, and provenance targets concrete domain/lifecycle records.

The concrete lifecycle-ID family and the closed enum are extended from Master §§3.1/3.2 with exactly `EntityRetirementId`/`RecordRef::EntityRetirement(EntityRetirementId)` and `PerspectiveRetirementId`/`RecordRef::PerspectiveRetirement(PerspectiveRetirementId)`. These are additive completions to the existing lifecycle-ID list and exhaustive `RecordRef` enum. The two records use the generic Lifecycle Record endpoint eligibility in Master §§31.2.1/33; `ResultedFrom` remains disallowed. Each has a distinct `WireTag` assigned by the central format registry; neither may alias an existing tag. `LifecycleTargetRef` itself is unchanged, and each retirement record's target remains its direct typed `EntityId` or `PerspectiveId` field.

## 4. Authorization and operation boundary

The application/engine boundary exposes these distinct authorization actions for M0-04c to encode in the policy model:

| Action | Required for |
|---|---|
| `entity.create` | Add an entity catalog identity |
| `entity.read` | Return entity identity/type metadata |
| `entity.reference` | Introduce an EntityId into a new Assertion, Event, or import |
| `entity.retire` | Retire an entity |
| `perspective.create` | Add a Perspective definition |
| `perspective.read` | Return Perspective definition metadata |
| `perspective.update` | Append display-name/description metadata |
| `perspective.use` | Create or query a perspective-scoped record |
| `perspective.retire` | Retire a Perspective |

Each action is checked against the acting `PrincipalId` and current effective policy; a `PerspectiveId` grants no capability. Read and field authorization applies before redaction. Hidden objects do not produce distinguishable existence, count, or error behavior. A write must satisfy both its domain-operation permission and any entity-reference/Perspective-use permissions. UI state is not authority. M0-04c defines policy-record storage and capability evaluation; it may refine the policy representation, but it must preserve these operation boundaries.

## 5. Implementable commands and required outcomes

| Command | Positive case | Required rejection/fault case |
|---|---|---|
| `CreateEntity(entity_type_id, optional_import_id)` | Creates one typed catalog identity at the commit revision | Unknown/retired type, ID collision, malformed/sentinel ID, failed capability: no partial catalog row |
| `RetireEntity(entity_id)` | Appends one project-wide retirement and returns its typed lifecycle ID | Unknown/hidden/already retired entity or failed capability: no partial retirement |
| `CreatePerspective(metadata)` | Creates a new ID and initial definition revision | Invalid metadata, duplicate imported identity, or failed capability: no definition |
| `UpdatePerspective(id, metadata)` | Appends a definition revision under the same PerspectiveId | Unknown/hidden/retired ID, invalid metadata, or failed capability: no partial update |
| `RetirePerspective(id)` | Appends one project-wide retirement and returns its typed lifecycle ID | Unknown/hidden/already retired ID or failed capability: no partial retirement |
| Entity/Perspective reference in a domain write | Existing, active, authorized typed identity passes all schema constraints | Unknown, retired, cross-type, unauthorized, or constraint-invalid reference fails atomically |

The IDs and definition/lifecycle changes participate in the ordinary transaction commit and its one published Revision. They do not allocate a second counter or World-Time axis. Historical queries do not observe later catalog metadata or retirement. These rules do not resolve the separate `WDB-HIS-001` gap about exact Genesis/gapless/overflow wording; M0-02a remains responsible for that decision.

## 6. Existing-contract cross-check

This supplement preserves the following existing contracts:

- `WDB-ID-001/002/003`: IDs are typed, validated 128-bit identities; no sentinel or semantic time.
- `WDB-SCH-003/005/006/010`: stable schema IDs, typed entity constraints, structural validation, and guarded Deprecated/Retired schema writes.
- `WDB-EPI-001/002/003` and `WDB-SEC-001`: epistemic separation and no Perspective/Principal conversion.
- `WDB-REF-001/002` and `WDB-LFC-001/002`: closed RecordRef and concrete lifecycle IDs.
- `WDB-SEC-002/003/005`: authorization before redaction, non-interference, and explicit historical permission mode.
- Master §§2.1.1, 2.1.2, 2.3.1, 2.3.2, 3.1, 17, and 31.3: project-wide identities remain orthogonal to HistorySpace, Layer, Perspective scope, schema, and security.

New implementation tests must cover the positive/negative cases above, historical catalog views before and after a metadata change/retirement, ID/type compile-time separation, and non-interference for hidden entity/Perspective references. No broad Product/User-interface flow is fixed here; that remains M0-05.
