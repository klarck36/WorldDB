#![forbid(unsafe_code)]

//! Initial owner crate for WorldDB domain, resolution, and engine modules.
//!
//! Domain identifiers are distinct validated types. Their UUID timestamp bits
//! are operational data and never represent WorldDB time or authority.
//!
//! ```compile_fail
//! use worlddb_core::{AssertionId, EventId};
//! fn needs_event(_: EventId) {}
//! fn reject_cross_family_assignment(id: AssertionId) {
//!     needs_event(id);
//! }
//! ```
//!
//! Archive actions form their own closed operational type and do not accept
//! validity closure, retraction, or purge actions.
//!
//! ```compile_fail
//! use worlddb_core::ArchiveAction;
//! let _ = ArchiveAction::Purge;
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{ArchiveAction, AssertionValidityClosure};
//! fn closure_is_not_an_archive_action(closure: AssertionValidityClosure) {
//!     let _: ArchiveAction = closure;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{ArchiveAction, AssertionRetraction};
//! fn retraction_is_not_an_archive_action(retraction: AssertionRetraction) {
//!     let _: ArchiveAction = retraction;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{ArchiveTargetRef, ArchiveTransitionId};
//! fn archive_transition_cannot_target_itself(id: ArchiveTransitionId) {
//!     let _ = ArchiveTargetRef::ArchiveTransition(id);
//! }
//! ```
//!
//! A Mask has only its three specified selector forms; a ReplacementBoundary
//! is a separate directive and cannot be used as a Mask selector.
//!
//! ```compile_fail
//! use worlddb_core::MaskSelector;
//! let _ = MaskSelector::Wildcard;
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{MaskSelector, ReplacementBoundaryId};
//! fn boundary_is_not_a_mask_selector(id: ReplacementBoundaryId) {
//!     let _ = MaskSelector::ReplacementBoundary(id);
//! }
//! ```
//!
//! Assertion and lifecycle records expose read-only accessors; their fields
//! cannot be mutated after construction.
//!
//! ```compile_fail
//! use worlddb_core::{Assertion, Revision};
//! fn mutate_assertion(assertion: &mut Assertion) {
//!     assertion.created_revision = Revision::GENESIS;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{AssertionRetraction, Revision};
//! fn mutate_retraction(retraction: &mut AssertionRetraction) {
//!     retraction.created_revision = Revision::GENESIS;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{AssertionValidityClosure, WorldTime};
//! fn mutate_closure(closure: &mut AssertionValidityClosure, time: WorldTime) {
//!     closure.close_at_world_time = time;
//! }
//! ```
//!
//! Lifecycle record families also have distinct concrete ID types.
//!
//! ```compile_fail
//! use worlddb_core::{
//!     Assertion, AssertionRetractionId, AssertionValidityClosure,
//!     Revision, WorldTime,
//! };
//! fn use_retraction_id_as_closure_id(
//!     id: AssertionRetractionId,
//!     assertion: &Assertion,
//!     time: WorldTime,
//!     revision: Revision,
//! ) {
//!     let _ = AssertionValidityClosure::new(id, assertion, time, revision);
//! }
//! ```
//!
//! Session-local snapshots have a wire representation but do not implement
//! [`PersistentId`], so a persistence API can require that marker trait.
//!
//! ```compile_fail
//! use worlddb_core::{PersistentId, SnapshotId};
//! fn requires_persistent_id<T: PersistentId>() {}
//! fn reject_session_local_snapshot() {
//!     requires_persistent_id::<SnapshotId>();
//! }
//! ```
//!
//! Each legacy identity type below does not exist; this negative check keeps them absent:
//!
//! ```compile_fail
//! use worlddb_core::BranchId; // BranchId does not exist.
//! use worlddb_core::SchemaVersionId; // SchemaVersionId does not exist.
//! use worlddb_core::LifecycleRecordId; // LifecycleRecordId does not exist.
//! ```
//!
//! Backend file-format segment IDs are intentionally internal:
//!
//! ```compile_fail
//! use worlddb_core::SegmentId;
//! ```
//!
//! UUIDv7 timestamp bits are not exposed as domain time:
//!
//! ```compile_fail
//! use worlddb_core::EntityId;
//! fn reject_timestamp_as_domain_time(id: EntityId) -> u64 {
//!     id.timestamp_millis()
//! }
//! ```
//!
//! A revision cannot be used as `RecordedAsOf` without the published-history check:
//!
//! ```compile_fail
//! use worlddb_core::{RecordedAsOf, Revision};
//! fn reject_revision_as_recorded_as_of(revision: Revision) {
//!     let _: RecordedAsOf = revision;
//! }
//! ```
//!
//! A physical duration is not an absolute world-time coordinate:
//!
//! ```compile_fail
//! use worlddb_core::{Duration, WorldTime};
//! fn reject_duration_as_world_time(duration: Duration) {
//!     let _: WorldTime = duration;
//! }
//! ```
//!
//! Event time is not an absolute world-time coordinate:
//!
//! ```compile_fail
//! use worlddb_core::{EventTime, WorldTime};
//! fn reject_world_time_as_event_time(time: WorldTime) {
//!     let _: EventTime = time;
//! }
//! ```
//!
//! Events and Assertions are separate story records; an Event cannot become an Assertion implicitly:
//!
//! ```compile_fail
//! use worlddb_core::{Assertion, Event};
//! fn witness_event_does_not_create_knowledge(event: Event) {
//!     let _: Assertion = event;
//! }
//! ```
//!
//! EventMask and EventRetraction are distinct records, and their lifecycle
//! identifiers cannot be interchanged.
//!
//! ```compile_fail
//! use worlddb_core::{EventMask, EventRetraction};
//! fn mask_is_not_an_event_retraction(mask: EventMask) {
//!     let _: EventRetraction = mask;
//! }
//! ```
//!
//! An EventMask also cannot be used as an Assertion Mask; event visibility
//! and assertion visibility have separate projections and lifecycle families.
//!
//! ```compile_fail
//! use worlddb_core::{EventMask, Mask};
//! fn event_mask_does_not_become_assertion_mask(mask: EventMask) {
//!     let _: Mask = mask;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EventMaskRetractionId, EventRetractionId};
//! fn reject_cross_family_lifecycle_id(id: EventMaskRetractionId) {
//!     let _: EventRetractionId = id;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EventRetractionId, EventSpanClosureId};
//! fn reject_retraction_id_as_span_closure(id: EventRetractionId) {
//!     let _: EventSpanClosureId = id;
//! }
//! ```
//!
//! `After` is an input alias only; persisted EventRelation kinds are closed
//! and have no `After` variant.
//!
//! ```compile_fail
//! use worlddb_core::EventRelationKind;
//! let _ = EventRelationKind::After;
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EventRelationRetractionId, EventRetractionId};
//! fn reject_cross_family_relation_retraction_id(id: EventRetractionId) {
//!     let _: EventRelationRetractionId = id;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EventId, EventRelation};
//! fn mutate_event_relation(relation: &mut EventRelation, event: EventId) {
//!     relation.from_event = event;
//! }
//! ```
//!
//! EventRelation has no World-Time validity interval; corrections use an
//! independent EventRelationRetraction record.
//!
//! ```compile_fail
//! use worlddb_core::EventRelation;
//! fn relation_has_no_world_time_validity(relation: EventRelation) {
//!     let _ = relation.validity();
//! }
//! ```
//!
//! Evidence and Provenance endpoints are closed typed subsets. A Source or an
//! Evidence record cannot be used as an Evidence target, and EventRelation has
//! its separate relation-reference contract.
//!
//! Evidence remains explanatory metadata and cannot be converted into an
//! Assertion that would affect resolution.
//!
//! ```compile_fail
//! use worlddb_core::{Assertion, EvidenceHistoryEntry};
//! fn evidence_does_not_create_an_assertion(entry: EvidenceHistoryEntry<'_>) {
//!     let _: Assertion = entry;
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EvidenceTargetRef, SourceId};
//! fn source_is_not_an_evidence_target(id: SourceId) {
//!     let _ = EvidenceTargetRef::Source(id);
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EvidenceTargetRef, EvidenceId};
//! fn evidence_cannot_target_another_evidence(id: EvidenceId) {
//!     let _ = EvidenceTargetRef::Evidence(id);
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EvidenceTargetRef, EventRelationId};
//! fn event_relation_is_not_a_generic_evidence_target(id: EventRelationId) {
//!     let _ = EvidenceTargetRef::EventRelation(id);
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{EventRelationId, ProvenanceEndpointRef};
//! fn event_relation_uses_its_separate_endpoint_contract(id: EventRelationId) {
//!     let _ = ProvenanceEndpointRef::EventRelation(id);
//! }
//! ```
//!
//! Neither endpoint enum has an open `Other`/string escape hatch.
//!
//! ```compile_fail
//! use worlddb_core::ProvenanceEndpointRef;
//! let _ = ProvenanceEndpointRef::Other(String::from("custom"));
//! ```
//!
//! RecordRef is closed and excludes operational record families.
//!
//! ```compile_fail
//! use worlddb_core::RecordRef;
//! let _ = RecordRef::Other(String::from("extension"));
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{RecordRef, SecurityPolicyRecordId};
//! fn security_history_stays_outside_record_ref(id: SecurityPolicyRecordId) {
//!     let _ = RecordRef::PolicyRecord(id);
//! }
//! ```
//!
//! API references accept only registered closed reference types.
//!
//! ```compile_fail
//! use worlddb_core::{DatabaseBoundRef, DatabaseId};
//! fn arbitrary_text_is_not_a_database_reference(database: DatabaseId) {
//!     let _ = DatabaseBoundRef::new(database, String::from("Assertion:abc"));
//! }
//! ```
//!
//! Separate schema references cannot be implicitly erased into RecordRef.
//!
//! ```compile_fail
//! use worlddb_core::{LayerId, RecordRef, SchemaRecordRef};
//! fn schema_ref_is_not_domain_history(id: LayerId) {
//!     let schema = SchemaRecordRef::Layer(id);
//!     let _: RecordRef = schema;
//! }
//! ```
//!
//! Separate reference families also have no unknown variants.
//!
//! ```compile_fail
//! use worlddb_core::EventRelationProvenanceRef;
//! let _ = EventRelationProvenanceRef::Other(String::from("custom"));
//! ```
//!
//! ```compile_fail
//! use worlddb_core::SchemaRecordRef;
//! let _ = SchemaRecordRef::Other(String::from("custom"));
//! ```
//!
//! ```compile_fail
//! use worlddb_core::MigrationRecordRef;
//! let _ = MigrationRecordRef::Other(String::from("custom"));
//! ```
//!
//! ```compile_fail
//! use worlddb_core::SecurityRecordRef;
//! let _ = SecurityRecordRef::Other(String::from("custom"));
//! ```
//!
//! ```compile_fail
//! use worlddb_core::TransactionRef;
//! let _ = TransactionRef::Other(String::from("custom"));
//! ```
//!
//! ```compile_fail
//! use worlddb_core::AuditRecordRef;
//! let _ = AuditRecordRef::Other(String::from("custom"));
//! ```
//!
//! Assertion validity is not an absolute world-time coordinate either:
//!
//! ```compile_fail
//! use worlddb_core::{AssertionValidity, WorldTime};
//! fn reject_world_time_as_assertion_validity(time: WorldTime) {
//!     let _: AssertionValidity = time;
//! }
//! ```
//!
//! A `WorldTime` cannot be globally ordered without checking its Timeline:
//!
//! ```compile_fail
//! use worlddb_core::WorldTime;
//! fn reject_implicit_timeline_order(left: WorldTime, right: WorldTime) -> bool {
//!     left < right
//! }
//! ```
//!
//! Signed, unsigned, and decimal values do not compare through coercion:
//!
//! ```compile_fail
//! use worlddb_core::{Decimal, Int, UInt};
//! fn reject_signed_unsigned_equality(signed: Int, unsigned: UInt) -> bool {
//!     signed == unsigned
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{Decimal, Int};
//! fn reject_decimal_integer_equality(decimal: Decimal, integer: Int) -> bool {
//!     decimal == integer
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{Decimal, UInt};
//! fn reject_decimal_unsigned_equality(decimal: Decimal, integer: UInt) -> bool {
//!     decimal == integer
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::Decimal;
//! fn reject_float_coercion(value: f64) -> Decimal {
//!     value.into()
//! }
//!
//! The core value catalog is closed: it has no null, float, JSON, array,
//! map, or CalendarPeriod variant.
//!
//! ```compile_fail
//! use worlddb_core::Value;
//! let _ = Value::Null;
//! ```
//!
//! ```compile_fail
//! use worlddb_core::Value;
//! let _ = Value::Float(1.5);
//! ```
//!
//! ```compile_fail
//! use worlddb_core::Value;
//! let _ = Value::Json(String::from("{}"));
//! ```
//!
//! ```compile_fail
//! use worlddb_core::Value;
//! let _ = Value::Array(vec![Value::Bool(true)]);
//! ```
//!
//! ```compile_fail
//! use std::collections::BTreeMap;
//! use worlddb_core::Value;
//! let _ = Value::Map(BTreeMap::<String, Value>::new());
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{CalendarPeriod, Value};
//! let period = CalendarPeriod::new(0, 1, 0);
//! if let Ok(period) = period {
//!     let _ = Value::CalendarPeriod(period);
//! }
//! ```
//!
//! An explicit layer selection requires a non-empty typed set:
//!
//! ```compile_fail
//! use worlddb_core::LayerSelection;
//! let _ = LayerSelection::Explicit(Vec::new());
//! ```
//!
//! Perspective identities cannot be supplied as Entity IDs:
//!
//! ```compile_fail
//! use worlddb_core::{Entity, EntityTypeId, PerspectiveId, Revision};
//! fn reject_perspective_as_entity(
//!     perspective: PerspectiveId,
//!     entity_type: EntityTypeId,
//!     revision: Revision,
//! ) {
//!     let _ = Entity::new(perspective, entity_type, revision);
//! }
//! ```
//!
//! A Perspective identity cannot be supplied by a security Principal:
//!
//! ```compile_fail
//! use worlddb_core::{PerspectiveDefinitionRevision, PrincipalId, Revision};
//! fn reject_principal_as_perspective(principal: PrincipalId, revision: Revision) {
//!     let _ = PerspectiveDefinitionRevision::new(principal, None, None, revision);
//! }
//! ```
//!
//! A Perspective cannot be registered as an authenticated security Principal:
//!
//! ```compile_fail
//! use worlddb_core::{PerspectiveId, Principal};
//! fn perspective_is_not_authority(perspective: PerspectiveId) {
//!     let _principal = Principal::new(perspective);
//! }
//! ```
//!
//! Query security context requires a `PrincipalId`, never a `PerspectiveId`:
//!
//! ```compile_fail
//! use worlddb_core::{AuthorizationMode, PerspectiveId, SecurityContext};
//! fn perspective_cannot_authorize(perspective: PerspectiveId) {
//!     let _security = SecurityContext::new(perspective, AuthorizationMode::Now);
//! }
//! ```
//!
//! `Value` has no domain-wide ordering; query ordering must select typed
//! ordering rules for the operation and its schema snapshot.
//!
//! ```compile_fail
//! use worlddb_core::Value;
//! fn requires_ord<T: Ord>() {}
//! requires_ord::<Value>();
//! ```
//!
//! ```compile_fail
//! use worlddb_core::Value;
//! fn attempts_global_value_order(left: Value, right: Value) -> bool {
//!     left < right
//! }
//! ```
//!
//! Transaction, migration, job, and audit identities remain distinct:
//!
//! ```compile_fail
//! use worlddb_core::{OperationId, TransactionId, TransactionIdentity};
//! fn reject_operation_as_transaction(operation: OperationId) {
//!     let _ = TransactionIdentity::new(operation, operation);
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{MigrationId, MigrationRun, MigrationRunState};
//! fn reject_plan_id_as_run(migration: MigrationId) {
//!     let _ = MigrationRun::new(migration, migration, MigrationRunState::Planned);
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{AuditOperationId, OperationId};
//! fn reject_data_operation_as_audit_operation(operation: OperationId) -> AuditOperationId {
//!     operation
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{AuditSequence, Revision};
//! fn reject_audit_sequence_as_data_revision(sequence: AuditSequence) -> Revision {
//!     sequence
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{AuditRecordRef, AuditRecordId};
//! let _ = AuditRecordRef::Other(String::from("custom"));
//! ```
//!
//! Absence, failure, and domain state are not interchangeable values:
//!
//! ```compile_fail
//! use worlddb_core::JobStatus;
//! fn reject_absence_as_job_status() -> JobStatus {
//!     None
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::JobStatus;
//! fn reject_result_as_job_status() -> JobStatus {
//!     Err::<(), &str>("failed")
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::{MigrationRunState, TransactionState};
//! fn reject_migration_state_as_transaction_state(state: MigrationRunState) -> TransactionState {
//!     state
//! }
//! ```
//!
//! ```compile_fail
//! use worlddb_core::MigrationCategory;
//! let _ = MigrationCategory::Other;
//! ```
//! ```

mod admin_raw;
mod archive;
mod archive_projection;
mod archive_transaction;
mod assertion_correction;
mod assertion_point_index;
mod assertion_projection;
mod assertions;
mod audit;
mod audit_wire;
mod candidate_scan;
mod catalog;
mod commit_cancellation;
mod context;
mod context_precedence;
mod cursor;
mod database_close;
mod diagnostics;
mod errors;
mod event_indexes;
mod event_projection;
mod event_relations;
mod events;
mod history_model;
mod ids;
mod index_generation;
mod job_supervisor;
mod jobs;
mod layers;
mod mask_projection;
mod mask_time_indexes;
mod masks;
mod migration;
mod multi_value_resolution;
#[cfg(test)]
mod non_interference;
mod numbers;
mod occ_point;
mod operation_status;
mod policy_audit;
mod project_metadata_transaction;
mod provenance_graph;
mod query_aggregate;
mod query_context;
mod query_engine;
mod query_graph;
mod query_page;
mod query_ports;
mod query_search;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "transport adapters may emit terminal cancellation, budget, and diagnostic items"
    )
)]
mod query_stream;
mod record_indexes;
mod record_refs;
mod reference_query;
mod resource_profile;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the internal replay-safe runner is consumed by a future designated worker"
    )
)]
mod retry;
mod revision_backend;
mod revision_history;
mod schema;
mod schema_history;
mod schema_write_validation;
mod security;
mod security_transaction;
mod single_value_resolution;
mod snapshot_lease;
mod source_evidence_projection;
mod source_provenance;
mod source_provenance_indexes;
mod storage_contract;
mod temporal;
mod transaction;
mod transaction_flow;
mod transfer_model;
mod transfer_reference_model;
mod transfer_transaction;
mod values;
mod wire;
mod wire_records;
mod write_authorization;
mod write_cross_record_validation;
mod write_reference_validation;
mod writer;

pub use ids::{
    ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
    AuditOperationId, AuditRecordId, ClientRequestId, DOMAIN_ID_SCOPES, DatabaseId, DomainId,
    EntityId, EntityRetirementId, EntityTypeId, EventAttributeId, EventId, EventKindId,
    EventMaskId, EventMaskRetractionId, EventRelationId, EventRelationRetractionId,
    EventRetractionId, EventRoleId, EventSpanClosureId, EvidenceId, EvidenceRetractionId,
    HistorySpaceId, IdGenerationError, IdNamespace, IdPersistence, IdScope, IdValidationError,
    IdWire, JobId, LayerId, MaskId, MaskRetractionId, MaskValidityClosureId, MigrationId,
    MigrationRunId, MigrationStepId, OperationId, PersistentId, PerspectiveId,
    PerspectiveRetirementId, PolicyRuleId, PredicateId, PrincipalId, ProvenanceId,
    ProvenanceRetractionId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
    ReplacementBoundaryValidityClosureId, Revision, RevisionError, RoleAssignmentId, RoleId,
    SchemaRevision, SecurityEpoch, SecurityEpochError, SecurityPolicyRecordId, SnapshotId,
    SourceId, TimelineId, TransactionId, TransferLineageId, WireId,
};

#[doc(hidden)]
pub mod storage_internal {
    //! Internal types shared with the file-storage adapter.

    pub use crate::ids::SegmentId;

    /// Creates the database identity for a new salvage fork through the core UUIDv7 policy.
    #[doc(hidden)]
    pub fn generate_salvage_fork_database_id()
    -> Result<crate::ids::DatabaseId, crate::ids::IdGenerationError> {
        crate::ids::generate_id()
    }

    /// Creates a WAL identity for a storage-only maintenance commit.
    #[doc(hidden)]
    pub fn generate_storage_maintenance_operation_id()
    -> Result<crate::ids::OperationId, crate::ids::IdGenerationError> {
        crate::ids::generate_id()
    }

    /// Creates an audit identity for one raw-read page attempt.
    #[doc(hidden)]
    pub fn generate_raw_read_audit_record_id()
    -> Result<crate::ids::AuditRecordId, crate::ids::IdGenerationError> {
        crate::ids::generate_id()
    }

    /// Creates a fresh audit-operation identity; retries intentionally get a new value.
    #[doc(hidden)]
    pub fn generate_raw_read_audit_operation_id()
    -> Result<crate::ids::AuditOperationId, crate::ids::IdGenerationError> {
        crate::ids::generate_id()
    }
}

pub use security::{
    AuthorizationDecision, Capability, CapabilityGrant, CapabilityRule, EvidenceRelationship,
    FieldSelector, GrantEffect, PolicyBundle, PolicyBundleError, PolicyEventRelationKind,
    PolicyScope, PolicySubject, PolicyTarget, Principal, PrincipalState, ProvenanceRelationship,
    RelationshipSelector, RoleAssignment, RoleDefinition, RoleDefinitionError, SecurityPolicyError,
    SecurityPolicyHistory, SecurityPolicyHistoryError, SecurityPolicySnapshot,
    SecurityPolicyVersion, SecurityPolicyView,
};
pub use security_transaction::{
    SecurityPolicyChange, SecurityPolicyChangeRequest, SecurityPolicyCommitBatch,
    SecurityPolicyRecord, SecurityPolicyRecordError, SecurityPolicyTransactionError,
    SecurityPolicyTransactionOutcome, commit_security_policy_change,
};

pub use audit::{
    AuditAction, AuditCommitContext, AuditFingerprintError, AuditObjectClass, AuditOutcome,
    AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity,
    AuditScopeFingerprint, AuditSequence, AuditSequenceError, PageOrdinal, RawReadAttempt,
    RawReadAttemptIdentity, RawReadAttemptScope,
};
pub use audit_wire::{
    AuditCodecError, AuditRecordKind, decode_audit_record, decode_audit_record_with_limits,
    decode_raw_read_attempt, decode_raw_read_attempt_with_limits, encode_audit_record,
    encode_raw_read_attempt,
};

pub use archive::{
    ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition, ArchiveTransitionError,
};
pub use archive_projection::{
    ArchiveHistoryReferenceModel, ArchiveProjectionError, ArchiveTargetRecord,
};
pub use archive_transaction::{ArchiveTransactionError, validate_archive_transition_transaction};
pub use assertion_correction::{
    AssertionCorrectionCommand, AssertionCorrectionCommitError,
    AssertionCorrectionCommitValidationError, AssertionCorrectionError,
    PreparedAssertionCorrection, commit_assertion_correction, prepare_assertion_correction,
};
pub use assertion_point_index::{
    AssertionEntityHistoryQuery, AssertionIndexHit, AssertionPointHistoryIndex,
    AssertionPointIndexError, AssertionPointQuery, AssertionPredicateHistoryQuery,
};
pub use assertion_projection::{AssertionLifecycleProjection, AssertionProjectionError};
pub use assertions::{
    Assertion, AssertionDraft, AssertionRecordError, AssertionRetraction, AssertionValidityClosure,
    Polarity, Subject,
};
pub use candidate_scan::{
    AssertionCandidate, AssertionCandidateQuery, AssertionHistoryRecord, CandidateScanError,
    full_scan_assertion_candidates, full_scan_authorized_assertion_candidates,
};
pub use catalog::{
    Entity, EntityCatalogError, EntityCatalogSnapshot, EntityRetirement, HistorySpaceCatalog,
    HistorySpaceDefinition, HistorySpaceError, PerspectiveCatalogError, PerspectiveCatalogSnapshot,
    PerspectiveDefinitionRevision, PerspectiveRetirement,
};
pub use commit_cancellation::{
    CancellationRequestDisposition, CommitCancellation, CommitCancellationState, CommitpointError,
    CommitpointPermit,
};
pub use context::{ContextError, ContextKey, EpistemicMode, PerspectiveScope};
pub use context_precedence::{ContextPrecedence, ContextPrecedenceError};
pub use cursor::{
    CursorInsertRequest, CursorSecurityContext, CursorStateError, CursorStateStore,
    CursorStoreLimits, CursorToken, QueryHash,
};
pub use database_close::{
    DatabaseCloseOwner, DatabaseCloseReport, NoTelemetryFlusher, TelemetryFlushStatus,
    TelemetryFlusher,
};
pub use diagnostics::{
    BoundedDiagnostics, DiagnosticCode, DiagnosticCounter, DiagnosticEvent, DiagnosticEventError,
    DiagnosticEventKind, DiagnosticField, DiagnosticFieldKey, DiagnosticPort, DiagnosticQueueError,
    DiagnosticRedaction, DiagnosticSpanName, MAX_DIAGNOSTIC_FIELDS, MAX_DIAGNOSTIC_QUEUE_CAPACITY,
    SafeDiagnosticValue, StableDiagnosticHash,
};
pub use errors::{
    BackupError, CommitError, CommitOutcome, CommitReceipt, ConflictFact, ConflictReport,
    ErrorFacts, ExportError, IntegrityImpact, InternalError, JobError, MigrationError, OpenError,
    PublicErrorCode, PublicErrorDto, QueryError, RecoveryAction, RecoveryError,
    ResourceLookupFailure, RetryHint, Retryability, SecurityError, Severity, StorageError,
    StorageFailureClass, StorageOperation, ValidationError, map_resource_lookup_error,
    recovery_action, to_public_error, to_public_job_error,
};
pub use event_indexes::{
    EventIndexError, EventMaskIndex, EventRelationIndex, EventSearchIndex, EventTimeIndex,
};
pub use event_projection::{
    EventCandidate, EventCandidateQuery, EventCorrection, EventHistory, EventProjectionError,
    EventTimeFilter, full_scan_authorized_event_candidates, full_scan_event_candidates,
    prepare_event_correction,
};
pub use event_relations::{
    EventGraphError, EventGraphProjection, EventRelation, EventRelationBatch, EventRelationError,
    EventRelationHistory, EventRelationInputKind, EventRelationKey, EventRelationKind,
    EventRelationRetraction, project_active_event_relations,
    project_authorized_active_event_relations, validate_event_graph_transaction,
};
pub use events::{
    Event, EventAttributeValue, EventAttributes, EventDraft, EventMask, EventMaskRetraction,
    EventParticipant, EventRecordError, EventRetraction, EventSpanClosure, Participants,
};
pub use history_model::{HistorySpaceModelError, HistorySpaceReferenceModel};
pub use jobs::{
    DeterminateJobProgress, JobBudget, JobBudgetError, JobDescriptor, JobKind, JobProgress,
    JobProgressError, JobStatus, JobTerminalState, TaskFailure, TaskRole, observe_task_join,
};
pub use layers::{LayerDefinition, LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
pub use mask_projection::{
    AssertionMaskContext, AssertionMaskProjection, AuthorizedAssertionMaskHistory,
    MaskProjectionError, apply_assertion_masks, apply_authorized_assertion_masks,
    apply_authorized_assertion_masks_with_trace,
};
pub use mask_time_indexes::{
    AssertionValidityIndex, ContextPrecedenceIndex, MaskSelectorIndex, MaskSelectorIndexError,
    ReplacementBoundaryIndex, ReplacementBoundaryIndexError, ValidityIndexError, ValidityTarget,
};
pub use masks::{
    Mask, MaskRecordError, MaskRetraction, MaskSelector, MaskSlotSelector, MaskValidityClosure,
    PropositionKey, ReplacementBoundary, ReplacementBoundaryError, ReplacementBoundaryRetraction,
    ReplacementBoundaryValidityClosure,
};
pub use migration::{
    MigrationCategory, MigrationPlan, MigrationPlanError, MigrationRun, MigrationRunState,
    MigrationStepCommitIdentity,
};
pub use multi_value_resolution::{
    MultiValueConflict, MultiValueEntry, MultiValueOutcome, MultiValueReplaceContext,
    MultiValueReplaceTrace, MultiValueResolutionError, MultiValueSlot, ReplacementBoundaryHistory,
    resolve_multi_value_overlay, resolve_multi_value_replace,
    resolve_multi_value_replace_with_trace,
};

pub use temporal::{
    AssertionValidity, Duration, EventTime, RecordedAsOf, TemporalError, TimeInterval, Timeline,
    WorldTime,
};

pub use transaction::{TransactionDescriptor, TransactionIdentity, TransactionState};
pub use transaction_flow::{
    CancellableCommitError, MixedRecordCommitError, Open, OpenTransaction, TransactionBeginError,
    Validated, ValidatedTransaction, WriteTransaction, commit_mixed_record_batch,
};
pub use transfer_model::{
    ExternalReferenceDecision, HistorySpaceContentRef, HistorySpaceContentRefError,
    TransferArchivePolicy, TransferLifecyclePolicy, TransferLineage, TransferLineageError,
    TransferPlan, TransferPlanError, TransferPlanSpec,
};
pub use transfer_reference_model::{
    TransferPreview, TransferPreviewAcknowledgement, TransferReceipt, TransferRecord,
    TransferReferenceModel, TransferReferenceModelError,
};
pub use transfer_transaction::{
    TransferTransactionError, TransferTransactionOutcome, commit_transfer_record_batch,
};

pub use admin_raw::{
    AdminRawAuditAuthorizer, AdminRawAuditError, AdminRawAuthorizeRequest, AdminRawError,
    release_admin_raw_page,
};
pub use index_generation::{
    FullScanBudget, IndexAccessPlan, IndexAvailability, IndexBuildVersion, IndexFallbackReason,
    IndexFamily, IndexFormatVersion, IndexGenerationMetadata, IndexMetadataError,
    IndexQueryRequirement, IndexRevisionCoverage, IndexSchemaVersion, plan_index_access,
};
pub use job_supervisor::{
    JobCompletion, JobControl, JobPool, JobResumeMetadata, JobShutdownError, JobShutdownReport,
    JobSnapshot, JobSpec, JobStateError, JobSubmitError, JobSupervisor, JobSupervisorLimitError,
    JobSupervisorLimits,
};
pub use numbers::{Decimal, DecimalError, Int, IntegerError, UInt};
pub use occ_point::{OccPointError, OccPointStore, OccPointTransaction, OccScopeKey};
pub use operation_status::{
    OperationAttempt, OperationBeginOutcome, OperationStatus, OperationStatusError,
    OperationStatusJournal,
};
pub use policy_audit::{
    AuditAccessPermissions, AuditRetentionError, AuditRetentionPolicy, InMemoryPolicyState,
    InMemoryRequiredAuditPort, MAX_IN_MEMORY_AUDIT_RECORDS, PolicyAuditError, RequiredAuditError,
    RequiredAuditPort, apply_required_policy_change,
};
pub use project_metadata_transaction::{
    ProjectMetadataCandidate, ProjectMetadataSnapshot, ProjectMetadataValidationError,
    validate_project_metadata_transaction,
};
pub use provenance_graph::{
    GraphValidationBudget, ProvenanceDependencyEdge, ProvenanceGraphError,
    ProvenanceGraphProjection, project_authorized_provenance_edges,
    validate_provenance_graph_transaction,
};
pub use query_aggregate::{
    AggregateError, AggregateGroupValue, AggregateResult, AggregateSpec, GroupValueKey,
    GroupedCountRow, ResolvedAggregateRow, aggregate_visible_resolved,
};
pub use query_context::{
    AuthorizationMode, BudgetDimension, CancellationToken, MAX_QUERY_CANDIDATES, MAX_QUERY_RESULTS,
    MAX_QUERY_WORK_UNITS, QueryBudget, QueryBudgetError, QueryBudgetLimits, QueryContext,
    QueryContextBinding, QueryContextError, QueryContextInput, SecurityContext, SnapshotSelector,
    ValidatedLayerSelection, WorldTimeSelector,
};
pub use query_engine::{
    AssertionPointIndexAccess, AssertionPointRequest, AssertionQueryStore, ProductiveQueryEngine,
    QueryEngineError, QueryEngineOutput, QueryExecutionPath, ReplacementBoundarySource,
    ResolutionFailure,
};
pub use query_graph::{
    GraphCandidateSet, GraphCyclePolicy, GraphDirection, GraphEdge, GraphError, GraphNode,
    GraphRelationshipKind, GraphResult, GraphSpec, TraversedGraphEdge,
    full_scan_authorized_graph_traversal,
};
pub use query_page::{PageExecution, PageOperation, PageRequest, PageRequestError, QueryPage};
pub use query_ports::{
    OwnedQueryResult, QueryPortError, bind_authorized_explain, bind_authorized_resolved_view,
    full_scan_owned_authorized_raw_history,
};
pub use query_search::{
    QuerySearchError, SearchDocument, SearchHit, SearchMatch, SearchSpec, SearchTextField,
    SearchToken, full_scan_token_search,
};
pub use record_indexes::{
    LifecycleIndex, LifecycleIndexEntry, LookupIndexError, OperationIdIndex, OperationIdIndexEntry,
    OperationIndexStatus, RecordIdIndex, RecordIdIndexEntry, SchemaIdRevisionIndex,
    SchemaIdRevisionIndexEntry,
};
pub use record_refs::{
    AuditRecordRef, DatabaseBoundRef, DatabaseReference, EventRelationProvenanceRef, JobRef,
    LifecycleTargetRef, MigrationRecordRef, RecordRef, RecordRefConversionError, RecordRefWireTag,
    SchemaRecordRef, SecurityRecordRef, SnapshotRef, TransactionRef, UnknownRecordRefWireTag,
};
pub use reference_query::{
    ExplainStage, ExplainStageError, ExplainStageKind, HistoricalQueryBinding, RawHistoryError,
    RawHistoryRow, ReferenceExplain, ReferenceExplainError, ResolvedOutcome, ResolvedView,
    ResolvedViewError, canonicalize_raw_history_rows, full_scan_authorized_raw_history,
    full_scan_raw_history,
};
pub use resource_profile::{
    MAX_PROCESS_HARD_MEMORY_BYTES, MemoryReservation, ProcessMemoryBudget, ProcessResourceProfile,
    RESOURCE_PROFILE_VERSION_V1, ResourceBudgetError, ResourceClass, ResourceClassLimits,
    configure_process_resource_profile, process_memory_budget,
};
pub use revision_backend::{CancellablePublishError, InMemoryRevisionBackend, RevisionBackend};
pub use revision_history::{
    HistoricalRead, InMemoryRevisionLog, PublishedCommit, RevisionLogError,
};
pub use schema::{
    CalendarPeriod, Cardinality, ConstraintSet, DecimalFieldMetadata, EntityTypeConstraint,
    EntityTypeDefinition, EventAttributeDefinition, EventKindDefinition, EventRoleDefinition,
    EventTimeConstraint, EventTimeForm, InclusiveRange, Lifecycle, NonEmptySet,
    PredicateDefinition, PredicateDefinitionSpec, ResolutionPolicy, RoleCardinality,
    SchemaDefinitionError, TimeRange, ValueConstraint, ValueKind,
};
pub use schema_history::{
    SchemaDefinition, SchemaHistoryError, SchemaHistoryReferenceModel, SchemaMode, SchemaSnapshot,
};
pub use schema_write_validation::{
    DeprecatedSchemaWriteWarning, SchemaWriteValidationError, ValidatedAssertionBatch,
    validate_assertion_batch,
};
pub use single_value_resolution::{
    SingleValueOutcome, SingleValueResolutionError, SingleValueSlot, resolve_single_value_replace,
};
pub use snapshot_lease::{
    HistorySpaceView, SnapshotBinding, SnapshotBindingInput, SnapshotError, SnapshotLease,
    SnapshotLifetimeLimits, SnapshotLifetimeStatus, SnapshotPinPurpose, SnapshotRegistry,
    SnapshotSecurityBinding,
};
pub use source_evidence_projection::{
    EvidenceHistoryEntry, EvidenceHistoryStatus, EvidenceTargetHistoryEntry, SourceEvidenceError,
    SourceEvidenceHistory, SourceEvidenceProjection, SourceEvidenceQuery,
    full_scan_authorized_source_evidence, full_scan_source_evidence,
    validate_source_evidence_post_transaction,
};
pub use source_provenance::{
    Evidence, EvidenceRelation, EvidenceRetraction, EvidenceTargetRef, ProvenanceEdge,
    ProvenanceEdgeHistory, ProvenanceEndpointRef, ProvenanceEndpointSide, ProvenanceRelation,
    ProvenanceRetraction, Source, SourceContentDigest, SourceEvidenceProvenanceError,
    SourceLocator, SourceMetadata, SourceMetadataEntry, project_active_provenance_edges,
};
pub use source_provenance_indexes::{
    EvidenceNeighborhoodIndex, ProvenanceAdjacencyIndex, ProvenanceIndexDirection,
    SourceProvenanceIndexError, full_scan_authorized_evidence_neighborhood_index,
    full_scan_authorized_provenance_adjacency_index,
};
pub use storage_contract::{
    DurabilityLevel, ProductionStorage, StorageBackend, StorageCapabilities, StorageFeature,
    StorageFeatureSet, StorageRequirementError,
};
pub use values::{Bytes, Symbol, SymbolError, Time, Value};
pub use wire::{
    CHECKSUM_LEN, DecodeResource, DecoderLimits, FORMAT_MAJOR, FORMAT_MINOR, FRAME_HEADER_LEN,
    FRAME_MAGIC, Frame, FrameError, FrameHeader, TlvDecoder, TlvEncoder, TlvField, ValueTag,
    WireError, decode_frame, decode_frame_with_limits, decode_id, decode_value,
    decode_value_with_limits, encode_frame, encode_id, encode_value,
};
pub use wire_records::{
    DecodedRecord, Record, RecordCodecError, RecordKind, decode_record,
    decode_record_batch_with_limits, decode_record_ref, decode_record_with_limits,
    encode_decoded_record, encode_record, encode_record_ref, encode_record_with_flags,
};
pub use write_authorization::{
    WriteAuthorizationError, authorize_deprecated_schema_warnings, authorize_validated_write_batch,
};
pub use write_cross_record_validation::{
    ValidatedWriteCrossRecordState, WriteCrossRecordCandidate, WriteCrossRecordValidationError,
    validate_write_cross_record_state,
};
pub use write_reference_validation::{
    ReferenceCatalog, ValidatedWriteReferenceBatch, WriteReferenceSnapshot,
    WriteReferenceValidationError, validate_write_references,
};
pub use writer::{
    DatabaseHandle, SingleWriter, SnapshotPublicationError, SnapshotPublisher, SnapshotRead,
    WriterCloseError, WriterCloseReport, WriterCoordinator, WriterReply, WriterUnavailable,
    spawn_single_writer,
};
