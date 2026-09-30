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
//! Evidence and Provenance endpoints are closed typed subsets. A Source or an
//! Evidence record cannot be used as an Evidence target, and EventRelation has
//! its separate relation-reference contract.
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

mod archive;
mod assertions;
mod audit;
mod audit_wire;
mod catalog;
mod context;
mod event_relations;
mod events;
mod ids;
mod jobs;
mod layers;
mod masks;
mod migration;
mod numbers;
mod record_refs;
mod schema;
mod source_provenance;
mod temporal;
mod transaction;
mod values;
mod wire;
mod wire_records;

pub use ids::{
    ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
    AuditOperationId, AuditRecordId, ClientRequestId, DOMAIN_ID_SCOPES, DatabaseId, DomainId,
    EntityId, EntityRetirementId, EntityTypeId, EventAttributeId, EventId, EventKindId,
    EventMaskId, EventMaskRetractionId, EventRelationId, EventRelationRetractionId,
    EventRetractionId, EventRoleId, EventSpanClosureId, EvidenceId, EvidenceRetractionId,
    HistorySpaceId, IdGenerationError, IdNamespace, IdPersistence, IdScope, IdValidationError,
    IdWire, JobId, LayerId, MaskId, MaskRetractionId, MaskValidityClosureId, MigrationId,
    MigrationRunId, MigrationStepId, OperationId, PersistentId, PerspectiveId,
    PerspectiveRetirementId, PredicateId, PrincipalId, ProvenanceId, ProvenanceRetractionId,
    ReplacementBoundaryId, ReplacementBoundaryRetractionId, ReplacementBoundaryValidityClosureId,
    Revision, RevisionError, SchemaRevision, SecurityEpoch, SecurityEpochError, SnapshotId,
    SourceId, TimelineId, TransactionId, WireId,
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
pub use assertions::{
    Assertion, AssertionDraft, AssertionRecordError, AssertionRetraction, AssertionValidityClosure,
    Polarity, Subject,
};
pub use catalog::{
    Entity, EntityCatalogError, EntityCatalogSnapshot, EntityRetirement, HistorySpaceCatalog,
    HistorySpaceDefinition, HistorySpaceError, PerspectiveCatalogError, PerspectiveCatalogSnapshot,
    PerspectiveDefinitionRevision, PerspectiveRetirement,
};
pub use context::{ContextError, ContextKey, EpistemicMode, PerspectiveScope};
pub use event_relations::{
    EventRelation, EventRelationBatch, EventRelationError, EventRelationInputKind,
    EventRelationKey, EventRelationKind, EventRelationRetraction,
};
pub use events::{
    Event, EventAttributeValue, EventAttributes, EventDraft, EventMask, EventMaskRetraction,
    EventParticipant, EventRecordError, EventRetraction, EventSpanClosure, Participants,
};
pub use jobs::{
    DeterminateJobProgress, JobBudget, JobBudgetError, JobDescriptor, JobKind, JobProgress,
    JobProgressError, JobStatus, JobTerminalState,
};
pub use layers::{LayerDefinition, LayerSchemaError, LayerSchemaSnapshot, LayerSelection};
pub use masks::{
    Mask, MaskRecordError, MaskRetraction, MaskSelector, MaskSlotSelector, MaskValidityClosure,
    PropositionKey, ReplacementBoundary, ReplacementBoundaryError, ReplacementBoundaryRetraction,
    ReplacementBoundaryValidityClosure,
};
pub use migration::{
    MigrationCategory, MigrationPlan, MigrationPlanError, MigrationRun, MigrationRunState,
    MigrationStepCommitIdentity,
};

pub use temporal::{
    AssertionValidity, Duration, EventTime, RecordedAsOf, TemporalError, TimeInterval, Timeline,
    WorldTime,
};

pub use transaction::{TransactionDescriptor, TransactionIdentity, TransactionState};

pub use numbers::{Decimal, DecimalError, Int, IntegerError, UInt};
pub use record_refs::{
    AuditRecordRef, DatabaseBoundRef, DatabaseReference, EventRelationProvenanceRef, JobRef,
    LifecycleTargetRef, MigrationRecordRef, RecordRef, RecordRefConversionError, RecordRefWireTag,
    SchemaRecordRef, SecurityRecordRef, SnapshotRef, TransactionRef, UnknownRecordRefWireTag,
};
pub use schema::{
    CalendarPeriod, Cardinality, ConstraintSet, DecimalFieldMetadata, EntityTypeConstraint,
    EntityTypeDefinition, EventAttributeDefinition, EventKindDefinition, EventRoleDefinition,
    EventTimeConstraint, EventTimeForm, InclusiveRange, Lifecycle, NonEmptySet,
    PredicateDefinition, PredicateDefinitionSpec, ResolutionPolicy, RoleCardinality,
    SchemaDefinitionError, TimeRange, ValueConstraint, ValueKind,
};
pub use source_provenance::{
    Evidence, EvidenceRelation, EvidenceRetraction, EvidenceTargetRef, ProvenanceEdge,
    ProvenanceEndpointRef, ProvenanceEndpointSide, ProvenanceRelation, ProvenanceRetraction,
    Source, SourceContentDigest, SourceEvidenceProvenanceError, SourceLocator, SourceMetadata,
    SourceMetadataEntry,
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
