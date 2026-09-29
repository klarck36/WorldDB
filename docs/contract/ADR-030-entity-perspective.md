# ADR-030 – Entity and Perspective identity lifecycle

**Status:** Accepted for the WorldDB 1.0 working contract  
**Task:** M0-04  
**Decision owner:** Luna (contract author under the user's instruction to execute the plan)  
**Normative detail:** [Entity/Perspective supplement](entity_perspective_contract.md)

## Context

Master §33 lists Entity and Perspective as First-Class types, but their creation commands, Entity type assignment, metadata history, retirement behavior, and permission boundaries were incomplete. Reusing Assertion fields as identity metadata, treating a Perspective as a Principal, or making Entity/Perspective generic `RecordRef`s would contradict existing schema, epistemic, and security boundaries.

## Decision

1. Entity and Perspective identities are typed project-catalog objects on the shared Revision axis. They are not HistorySpace- or Layer-owned and are not generic `RecordRef`, Evidence, or Provenance endpoints.
2. Entity creation assigns exactly one existing `EntityTypeId`; the assignment is immutable in 1.0. Entity facts and labels are typed Assertions rather than untyped metadata.
3. Perspective metadata is optional, explicitly revisioned catalog metadata. PerspectiveId remains an epistemic scope and is never a security principal or capability.
4. Entity and Perspective retirement are separate, irreversible, append-only lifecycle records with concrete IDs and `RecordRef` variants. Retirement forbids new references/writes of the affected class without mutating historical records or deleting identity.
5. CRUD/use authorization boundaries are explicit action scopes. Capability evaluation, policy record representation, and field-level policy storage remain in M0-04c, which must implement these boundaries without changing their meaning.
6. `RecordedAsOf` controls historical catalog metadata and lifecycle visibility. Domain data continues to follow existing HistorySpace, Layer, world-time, schema, and authorization rules.

## Consequences

- The Master working-copy First-Class rows for Entity and Perspective are clarified; the 22-type TOML registry is regenerated from that working copy.
- `EntityRetirementId` and `PerspectiveRetirementId` extend the concrete lifecycle-ID set and `RecordRef`. Their target fields remain direct `EntityId` and `PerspectiveId` values; the existing closed `LifecycleTargetRef` is unchanged.
- Entity retyping, retirement reactivation, identity reuse, silent default Perspectives, and implicit Perspective-to-Principal mapping are unavailable in 1.0.
- Metadata/lifecycle histories share the common Revision and transaction commit; no new counter or temporal axis is introduced.
- ADR-039 later confirms the stronger WDB-HIS-001 rule and closes all 52 HARD source gaps; the two GUARDED gaps remain open.

## Verification obligations

The supplement's command table is the behavioral acceptance checklist for Entity and Perspective APIs. Required automated follow-up includes typed-ID compile-fail coverage, invalid-reference and type-constraint failures, capability denial/non-interference, duplicate/terminal retirement rejection, historical views at revisions around each catalog change, and successful reads of existing records after retirement.
