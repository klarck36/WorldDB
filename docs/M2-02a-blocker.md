# M2-02a resolution: typed TransferLineage preserves the closed Provenance matrix

**Status:** DONE after the user confirmed the dedicated TransferLineage record for the three excluded DerivedFrom source families. Checked 2026-09-30.

## Conflict

The accepted transfer contract in `docs/contract/archive_transfer_contract.md` §3 requires a `DerivedFrom` edge from the source record to its copy for every copied `HistorySpaceContentRef`.

The closed `DerivedFrom` source matrix recorded by `WDB-REF-005` excludes `Mask`, `ReplacementBoundary`, and `EventMask`. The implementation enforces that rule in `ProvenanceEndpointRef::allowed_as_derived_from`; `source_provenance::tests::derived_from_rejects_exactly_the_disallowed_from_families` verifies those three exclusions. The rule is anchored in the accepted matrix in Master §31.2.1 and ADR-026, and WDB-REF-005 is already DONE. ADR-039 separately confirms all 52 HARD source statements as normative; that confirmation does not authorize an unreviewed change to this closed endpoint matrix.

Therefore a transfer containing any of those three record families cannot use a `DerivedFrom` edge for that source. Bypassing the constructor would leave runtime and wire validation inconsistent.

## EventRelation lifecycle policy constraint (implemented fail-closed)

The accepted transfer contract in `docs/contract/archive_transfer_contract.md` §§3–4 requires a selected EventRelation and its effective EventRelationRetraction to follow the selected `lifecycle_policy`, while transfer publishes at most one shared `Revision`.

`CopyEffectiveLifecycle` cannot recreate an effective relation retraction in that same revision: `EventRelationRetraction::new` requires its lifecycle revision to be strictly later than its target relation. The existing contract already provides `OmitWithAcknowledgement`. The preview lists the omitted relation retraction, requires acknowledgement, and rejects the copy-all policy before commit with `EffectiveEventRelationRetractionNeedsOmissionPolicy`. The copied relation remains active only when the caller explicitly chooses and acknowledges that lifecycle difference.

This is a modelled policy constraint, not a second normative decision. No revision- or lifecycle-constructor rule is bypassed.

## Accepted user decision

The user selected **“Eigener TransferLineage-Record (Empfohlen)”**. The closed `DerivedFrom` matrix remains unchanged. `TransferLineage` stores its own typed ID, source/target HistorySpaces, source/copy identities and shared commit revision; it is not a generic Provenance endpoint.

Transfer lineage is now family-specific: source families accepted by the existing matrix produce `DerivedFrom`; Mask, ReplacementBoundary and EventMask produce `TransferLineage`. The record is represented in the domain type, `RecordRef`, archive target subset, wire codec and registries. It does not become a `HistorySpaceContentRef` and cannot be used as an Evidence target or Provenance endpoint.

## Archive revision constraint (resolved under M4-05b)

The user chose the strict rule that ArchiveTransition must remain later than target creation. A one-revision transfer therefore cannot honor `PreserveArchiveState` for an archived source. The transfer preview still identifies those targets, but commit now fails closed with `ArchivePreservationRequiresLaterRevision`; callers can use `StartUnarchived` and archive the copy in a later explicit transaction. The accepted supplement and generated Master text state this rule. No same-revision exception is introduced.

## Verification

`cargo test --locked --offline -p worlddb-core source_provenance::tests::derived_from_rejects_exactly_the_disallowed_from_families` — 1 passed, 0 failed. This confirms the current matrix. After the preview implementation, workspace tests, Clippy, formatting, and plan checks pass; `cargo xtask verify` reports 30 PASS, the expected visible `ci-matrix` SKIP for M0-14, and 0 FAIL (PATH includes the installed Cargo toolchain).

M2-02a is complete: all three excluded source families use the dedicated TransferLineage record; codec registries, contract generation, workspace checks, Clippy, formatting and plan checks pass. M2-03 and M2-04 are also complete; M2-05 is next.

## Independent implementation progress

Implemented locally in `crates/worlddb-core/src/transfer_model.rs` and `crates/worlddb-core/src/transfer_reference_model.rs`: the exact 14-variant `HistorySpaceContentRef` subset; complete fingerprinted plans and fresh same-family ID maps; explicit external-reference policy; staged atomic publication with target-head, reference-visibility, identity, and sibling-isolation checks; selected EventRelation mapping; effective lifecycle handling; strict fail-closed archive preservation; fail-closed DerivedFrom lineage; and a read-only preview listing effective lifecycle omissions and planned archive targets. Omission-policy commits require an acknowledgement equal to the current plan preview. Twelve focused transfer-model tests are included. The workspace test suite, Clippy, formatting, plan check, dependency policy, and local verifier passed before the M4-05b transaction integration.

The DerivedFrom conflict is resolved by the accepted typed TransferLineage design. The preview covers lifecycle omissions and archive differences expressible by this generic reference model; domain projection deltas and production graph/schema validation remain engine-level work. These scope limits do not change the completed transfer reference model contract; M2-03 is gated on final M2-02a checks.
