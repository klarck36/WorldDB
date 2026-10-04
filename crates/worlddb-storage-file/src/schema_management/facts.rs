//! Authorized, schema-checked, WAL-backed Assertion, Mask, and Boundary writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use worlddb_core::{
    ArchiveAction, ArchiveHistoryReferenceModel, ArchiveState, ArchiveTargetRecord,
    ArchiveTargetRef, ArchiveTransition, ArchiveTransitionId, Assertion,
    AssertionCorrectionCommand, AssertionDraft, AssertionHistoryRecord, AssertionId,
    AssertionPointIndexAccess, AssertionPointRequest, AssertionQueryStore, AssertionRetraction,
    AssertionRetractionId, AssertionValidity, AuditAction, AuditCommitContext, AuditObjectClass,
    AuditOutcome, AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity,
    AuthorizationDecision, AuthorizedAssertionMaskHistory, Bytes, CancellationToken, Capability,
    ContextKey, EntityTypeConstraint, EpistemicMode, Event, EventDraft, EventId, EventKindId,
    EventRetraction, EventRetractionId, FieldSelector, FullScanBudget, HistoricalQueryBinding,
    HistorySpaceId, HistorySpaceReferenceModel, LayerSelection, Lifecycle, Mask, MaskId,
    MaskRetraction, MaskRetractionId, MaskSelector, OperationId, PerspectiveScope, PolicyTarget,
    PredicateId, ProductiveQueryEngine, ProvenanceId, ProvenanceRelationship, QueryBudget,
    QueryBudgetLimits, QueryContext, QueryContextInput, QueryEngineOutput, Record, RecordRef,
    RecordedAsOf, RelationshipSelector, ReplacementBoundary, ReplacementBoundaryId,
    ReplacementBoundaryRetraction, ReplacementBoundaryRetractionId, ReplacementBoundarySource,
    ResolutionPreview, Revision, SchemaDefinition, SchemaMode, SecurityContext,
    SecurityPolicyVersion, SnapshotRef, Subject, ValidatedLayerSelection, Value, WorldTimeSelector,
    WriteReferenceSnapshot, encode_record, prepare_assertion_correction, prepare_event_correction,
    validate_assertion_batch, validate_value_for_predicate, validate_write_references,
};

use crate::{
    DatabaseLayout, HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference,
    RecoveryManager, SecurityPolicyHistoryStore, WalOperationStatus, WalPrepareLog, WriterLock,
};

use super::{
    FileProjectMetadataManager, FileSchemaManager, SchemaManagementError, cleanup_staged_schema,
    next_audit_sequence,
};

/// Receipt for one committed immutable factual record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FactPublicationReceipt {
    operation_id: OperationId,
    revision: Revision,
    record_ref: RecordRef,
}

impl FactPublicationReceipt {
    /// Stable idempotency identity supplied for this write.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Shared database revision assigned to the write.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Typed identity of the newly committed record.
    #[must_use]
    pub const fn record_ref(self) -> RecordRef {
        self.record_ref
    }
}

/// Receipt for one atomic Assertion correction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssertionCorrectionReceipt {
    operation_id: OperationId,
    revision: Revision,
    target: AssertionId,
    replacement: AssertionId,
    retraction: AssertionRetractionId,
    corrects: ProvenanceId,
}

impl AssertionCorrectionReceipt {
    /// Idempotency identity of the correction operation.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Shared commit revision of all three correction records.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Original Assertion explicitly retracted by this correction.
    #[must_use]
    pub const fn target(self) -> AssertionId {
        self.target
    }

    /// New immutable replacement Assertion.
    #[must_use]
    pub const fn replacement(self) -> AssertionId {
        self.replacement
    }

    /// Explicit Retraction record for the original Assertion.
    #[must_use]
    pub const fn retraction(self) -> AssertionRetractionId {
        self.retraction
    }

    /// Explanatory `Corrects(replacement, target)` Provenance record.
    #[must_use]
    pub const fn corrects(self) -> ProvenanceId {
        self.corrects
    }
}

/// Receipt for one atomic Event correction. It deliberately has no EventRetraction ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventCorrectionReceipt {
    operation_id: OperationId,
    revision: Revision,
    target: EventId,
    replacement: EventId,
    corrects: ProvenanceId,
}

/// Durable status of one logical factual-record operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactOperationStatus {
    /// No durable prepare for this identity exists.
    NotCommitted,
    /// The operation committed at the returned shared revision.
    Committed(Revision),
    /// A prepare exists but recovery has not established its outcome.
    Indeterminate,
}

impl EventCorrectionReceipt {
    /// Idempotency identity of the correction operation.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }
    /// Shared commit revision of the new Event and Corrects edge.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }
    /// Original Event, which remains active after this correction.
    #[must_use]
    pub const fn target(self) -> EventId {
        self.target
    }
    /// New immutable replacement Event.
    #[must_use]
    pub const fn replacement(self) -> EventId {
        self.replacement
    }
    /// Explanatory `Corrects(replacement, target)` Provenance record.
    #[must_use]
    pub const fn corrects(self) -> ProvenanceId {
        self.corrects
    }
}

/// The identity and creation revision of one Event visible in a factual snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventSnapshotRef {
    event_id: EventId,
    created_revision: Revision,
    history_space_id: HistorySpaceId,
    layer_id: worlddb_core::LayerId,
    event_kind_id: Option<EventKindId>,
}

impl EventSnapshotRef {
    /// Stable identity of the visible Event.
    #[must_use]
    pub const fn event_id(self) -> EventId {
        self.event_id
    }
    /// Revision that created the visible Event.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
    /// Owning HistorySpace of the visible Event.
    #[must_use]
    pub const fn history_space_id(self) -> HistorySpaceId {
        self.history_space_id
    }
    /// Explicit Layer of the visible Event.
    #[must_use]
    pub const fn layer_id(self) -> worlddb_core::LayerId {
        self.layer_id
    }
    /// EventKind only when its field-level read grant is present.
    #[must_use]
    pub const fn event_kind_id(self) -> Option<EventKindId> {
        self.event_kind_id
    }
}

/// Typed input fields for one new ReplacementBoundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplacementBoundaryDraft {
    context: ContextKey,
    subject: Subject,
    predicate_id: PredicateId,
    validity: Option<AssertionValidity>,
}

impl ReplacementBoundaryDraft {
    /// Creates an explicit boundary input without assigning storage identity
    /// or Transaction-Time revision.
    #[must_use]
    pub const fn new(
        context: ContextKey,
        subject: Subject,
        predicate_id: PredicateId,
        validity: Option<AssertionValidity>,
    ) -> Self {
        Self {
            context,
            subject,
            predicate_id,
            validity,
        }
    }

    /// Context in which the replacement set is authoritative.
    #[must_use]
    pub const fn context(self) -> ContextKey {
        self.context
    }

    /// Selected Entity subject.
    #[must_use]
    pub const fn subject(self) -> Subject {
        self.subject
    }

    /// Stable Predicate identity.
    #[must_use]
    pub const fn predicate_id(self) -> PredicateId {
        self.predicate_id
    }

    /// Optional world-time validity interval.
    #[must_use]
    pub const fn validity(self) -> Option<AssertionValidity> {
        self.validity
    }
}

/// Authorized factual records visible at one committed revision.
#[derive(Clone, Debug)]
pub struct FactSnapshot {
    revision: Revision,
    assertions: Vec<Assertion>,
    assertion_retractions: Vec<AssertionRetraction>,
    masks: Vec<Mask>,
    mask_retractions: Vec<MaskRetraction>,
    replacement_boundaries: Vec<ReplacementBoundary>,
    replacement_boundary_retractions: Vec<ReplacementBoundaryRetraction>,
    events: Vec<EventSnapshotRef>,
    event_retractions: Vec<EventRetraction>,
    archive_transitions: Vec<ArchiveTransition>,
    lifecycle_visible: bool,
}

/// Complete, explicit axes for one authorized Assertion resolution preview.
#[derive(Clone, Debug)]
pub struct FactResolutionPreviewRequest {
    /// HistorySpace whose immutable ancestry is queried.
    pub history_space_id: HistorySpaceId,
    /// Selected query layers, validated against the pinned Layer schema.
    pub layer_selection: LayerSelection,
    /// Assertion subject and Predicate slot.
    pub subject: Subject,
    /// Predicate identity for the selected slot.
    pub predicate_id: PredicateId,
    /// Perspective and epistemic partition.
    pub perspective_scope: PerspectiveScope,
    /// Epistemic partition paired with `perspective_scope`.
    pub epistemic_mode: EpistemicMode,
    /// Point or complete all-times selector.
    pub world_time: WorldTimeSelector,
}

impl FactSnapshot {
    /// Revision represented by the snapshot.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Immutable Assertion records visible at this revision.
    #[must_use]
    pub fn assertions(&self) -> &[Assertion] {
        &self.assertions
    }

    /// Visible Transaction-Time retractions for the returned Assertions.
    #[must_use]
    pub fn assertion_retractions(&self) -> &[AssertionRetraction] {
        &self.assertion_retractions
    }

    /// Immutable Mask records visible at this revision.
    #[must_use]
    pub fn masks(&self) -> &[Mask] {
        &self.masks
    }

    /// Visible Transaction-Time retractions for the returned Masks.
    #[must_use]
    pub fn mask_retractions(&self) -> &[MaskRetraction] {
        &self.mask_retractions
    }

    /// Immutable ReplacementBoundary records visible at this revision.
    #[must_use]
    pub fn replacement_boundaries(&self) -> &[ReplacementBoundary] {
        &self.replacement_boundaries
    }

    /// Visible Transaction-Time retractions for the returned Boundaries.
    #[must_use]
    pub fn replacement_boundary_retractions(&self) -> &[ReplacementBoundaryRetraction] {
        &self.replacement_boundary_retractions
    }

    /// Visible Event identities and creation revisions.
    #[must_use]
    pub fn events(&self) -> &[EventSnapshotRef] {
        &self.events
    }

    /// Visible Transaction-Time retractions for Events in this snapshot.
    #[must_use]
    pub fn event_retractions(&self) -> &[EventRetraction] {
        &self.event_retractions
    }

    /// Visible operational archive transitions for the returned factual records.
    #[must_use]
    pub fn archive_transitions(&self) -> &[ArchiveTransition] {
        &self.archive_transitions
    }

    /// Whether lifecycle state was readable at this snapshot.
    #[must_use]
    pub const fn lifecycle_visible(&self) -> bool {
        self.lifecycle_visible
    }
}

/// File-backed factual-record manager.
///
/// Each write validates against one current schema/catalog snapshot and binds
/// the data record, policy-history advance, manifest, WAL receipt, and required
/// audit record to one shared commit point.
pub struct FileFactManager<'a> {
    schema: FileSchemaManager<'a>,
}

impl<'a> FileFactManager<'a> {
    /// Opens and verifies the database for the authenticated principal.
    pub fn open(
        layout: DatabaseLayout,
        writer_lock: &'a WriterLock,
        principal: worlddb_core::PrincipalId,
    ) -> Result<Self, FactManagementError> {
        let schema =
            FileSchemaManager::open(layout, writer_lock, principal).map_err(map_schema_error)?;
        Ok(Self { schema })
    }

    /// Current committed shared revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.schema.revision()
    }

    /// Reads the durable status of an OperationId from the verified WAL.
    pub fn operation_status(
        &self,
        operation_id: OperationId,
    ) -> Result<FactOperationStatus, FactManagementError> {
        let status = WalPrepareLog::new(&self.schema.layout)
            .operation_status(self.schema.writer_lock, operation_id)
            .map_err(storage_error)?;
        Ok(match status {
            WalOperationStatus::NotCommitted => FactOperationStatus::NotCommitted,
            WalOperationStatus::Committed(receipt) => {
                FactOperationStatus::Committed(receipt.revision())
            }
            WalOperationStatus::Indeterminate => FactOperationStatus::Indeterminate,
        })
    }

    /// Creates one immutable Assertion after schema, context, reference, and
    /// current-policy validation.
    pub fn create_assertion(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        assertion_id: AssertionId,
        draft: AssertionDraft,
        allow_deprecated_schema: bool,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let context = draft.context();
        let target = context_target(context);
        self.authorize(
            Capability::AssertionCreate,
            field_target(context, FieldSelector::AssertionValue(draft.predicate_id())),
        )?;
        self.authorize(Capability::LayerWrite, target)?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(draft.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldWrite, field_target(context, field))?;
        }
        self.authorize(Capability::EntityReference, target)?;
        if matches!(
            context.perspective_scope(),
            worlddb_core::PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, target)?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_validity(
            metadata.schema(),
            Some(draft.validity()),
            allow_deprecated_schema,
        )?;
        let policy = self.policy_snapshot()?;
        let schema_assertions = validate_assertion_batch(
            vec![draft.clone()],
            metadata.schema(),
            policy,
            self.schema.principal,
            allow_deprecated_schema,
            worlddb_core::DecoderLimits::default(),
            |time| {
                metadata
                    .schema()
                    .resolve_time(time, allow_deprecated_schema)
                    .map_err(|_| worlddb_core::TemporalError::Overflow)
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let references = validate_write_references(
            vec![draft.clone()],
            vec![],
            &WriteReferenceSnapshot {
                history_spaces: metadata.history_spaces(),
                layers: metadata.layers(),
                entities: metadata.entities(),
                perspectives: metadata.perspectives(),
                schema: metadata.schema(),
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_validated_write_batch(&references, policy, self.schema.principal)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_deprecated_schema_warnings(
            &references,
            &schema_assertions,
            policy,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;

        self.ensure_identity_available(RecordRef::Assertion(assertion_id))?;
        let revision = self.next_revision()?;
        let assertion = Assertion::new(assertion_id, draft, revision);
        self.publish(
            expected_base,
            operation_id,
            Record::Assertion(assertion),
            RecordRef::Assertion(assertion_id),
            target,
        )
    }

    /// Corrects one visible Assertion as an atomic replacement, explicit
    /// Retraction, and `Corrects(replacement, target)` Provenance edge.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    pub fn correct_assertion(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        target_id: AssertionId,
        expected_target_created_revision: Revision,
        replacement_id: AssertionId,
        retraction_id: AssertionRetractionId,
        corrects_id: ProvenanceId,
        replacement: AssertionDraft,
        retraction_reason: String,
        allow_deprecated_schema: bool,
    ) -> Result<AssertionCorrectionReceipt, FactManagementError> {
        match self.operation_status(operation_id)? {
            FactOperationStatus::Committed(revision) => {
                return self.replay_assertion_correction(
                    expected_base,
                    operation_id,
                    revision,
                    target_id,
                    expected_target_created_revision,
                    replacement_id,
                    retraction_id,
                    corrects_id,
                    replacement,
                    retraction_reason,
                );
            }
            FactOperationStatus::Indeterminate => {
                return Err(FactManagementError::UnknownCommit(operation_id));
            }
            FactOperationStatus::NotCommitted => {}
        }
        self.check_base(expected_base)?;
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Assertion(assertion) if assertion.id() == target_id => {
                    Some(assertion.clone())
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Assertion is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::AssertionRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::AssertionCorrect, target_policy)?;
        self.authorize(Capability::AssertionCreate, target_policy)?;
        self.authorize(Capability::AssertionRetract, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target(
                target.context(),
                RecordRef::Assertion(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;

        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(target.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldRead, field_target(target.context(), field))
                .map_err(|_| {
                    FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
                })?;
        }
        let context = replacement.context();
        self.authorize(Capability::LayerWrite, context_target(context))?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(replacement.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldWrite, field_target(context, field))?;
        }
        self.authorize(Capability::EntityReference, context_target(context))?;
        if matches!(
            context.perspective_scope(),
            PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, context_target(context))?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_validity(
            metadata.schema(),
            Some(replacement.validity()),
            allow_deprecated_schema,
        )?;
        let policy = self.policy_snapshot()?;
        let schema_assertions = validate_assertion_batch(
            vec![replacement.clone()],
            metadata.schema(),
            policy,
            self.schema.principal,
            allow_deprecated_schema,
            worlddb_core::DecoderLimits::default(),
            |time| {
                metadata
                    .schema()
                    .resolve_time(time, allow_deprecated_schema)
                    .map_err(|_| worlddb_core::TemporalError::Overflow)
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let references = validate_write_references(
            vec![replacement.clone()],
            vec![],
            &WriteReferenceSnapshot {
                history_spaces: metadata.history_spaces(),
                layers: metadata.layers(),
                entities: metadata.entities(),
                perspectives: metadata.perspectives(),
                schema: metadata.schema(),
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_validated_write_batch(&references, policy, self.schema.principal)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_deprecated_schema_warnings(
            &references,
            &schema_assertions,
            policy,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;

        for identity in [
            RecordRef::Assertion(replacement_id),
            RecordRef::AssertionRetraction(retraction_id),
            RecordRef::Provenance(corrects_id),
        ] {
            self.ensure_identity_available(identity)?;
        }
        let retractions = existing_records
            .iter()
            .filter_map(|record| match record {
                Record::AssertionRetraction(value) => Some(value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let commit_revision = self.next_revision()?;
        let prepared = prepare_assertion_correction(
            &target,
            &retractions,
            RecordedAsOf::from_published_revision(expected_base),
            AssertionCorrectionCommand::new(
                expected_target_created_revision,
                replacement_id,
                replacement,
                retraction_id,
                retraction_reason,
                corrects_id,
                commit_revision,
            ),
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let receipt = AssertionCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: prepared.replacement().id(),
            retraction: prepared.retraction().id(),
            corrects: prepared.corrects().id(),
        };
        self.publish_batch(
            expected_base,
            operation_id,
            prepared.into_records().into_iter().collect(),
            RecordRef::Assertion(receipt.replacement),
            target_policy,
        )?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    fn replay_assertion_correction(
        &self,
        expected_base: Revision,
        operation_id: OperationId,
        commit_revision: Revision,
        target_id: AssertionId,
        expected_target_created_revision: Revision,
        replacement_id: AssertionId,
        retraction_id: AssertionRetractionId,
        corrects_id: ProvenanceId,
        replacement: AssertionDraft,
        retraction_reason: String,
    ) -> Result<AssertionCorrectionReceipt, FactManagementError> {
        if expected_base
            .next_commit()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?
            != commit_revision
        {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Assertion(assertion) if assertion.id() == target_id => {
                    Some(assertion.clone())
                }
                _ => None,
            })
            .ok_or(FactManagementError::IdempotencyMismatch)?;
        if target.created_revision() != expected_target_created_revision {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let target_policy = context_target(target.context());
        self.authorize(Capability::AssertionRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::AssertionCorrect, target_policy)?;
        self.authorize(Capability::AssertionCreate, target_policy)?;
        self.authorize(Capability::AssertionRetract, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target(
                target.context(),
                RecordRef::Assertion(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(target.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldRead, field_target(target.context(), field))
                .map_err(|_| {
                    FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
                })?;
        }
        let write_context = replacement.context();
        self.authorize(Capability::LayerWrite, context_target(write_context))?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(replacement.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldWrite, field_target(write_context, field))?;
        }
        self.authorize(Capability::EntityReference, context_target(write_context))?;
        if matches!(
            write_context.perspective_scope(),
            PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, context_target(write_context))?;
        }

        let retractions = existing_records
            .iter()
            .filter_map(|record| match record {
                Record::AssertionRetraction(value) => Some(value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let prepared = prepare_assertion_correction(
            &target,
            &retractions,
            RecordedAsOf::from_published_revision(expected_base),
            AssertionCorrectionCommand::new(
                expected_target_created_revision,
                replacement_id,
                replacement,
                retraction_id,
                retraction_reason,
                corrects_id,
                commit_revision,
            ),
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let records = prepared.into_records().into_iter().collect::<Vec<_>>();
        self.verify_replayed_records(commit_revision, &records)?;
        Ok(AssertionCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: replacement_id,
            retraction: retraction_id,
            corrects: corrects_id,
        })
    }

    /// Corrects one Event by committing a new Event and `Corrects(new, old)`.
    /// The old Event stays active; no EventRetraction is created here.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    pub fn correct_event(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        target_id: EventId,
        expected_target_created_revision: Revision,
        replacement_id: EventId,
        corrects_id: ProvenanceId,
        replacement: EventDraft,
    ) -> Result<EventCorrectionReceipt, FactManagementError> {
        match self.operation_status(operation_id)? {
            FactOperationStatus::Committed(revision) => {
                return self.replay_event_correction(
                    expected_base,
                    operation_id,
                    revision,
                    target_id,
                    expected_target_created_revision,
                    replacement_id,
                    corrects_id,
                    replacement,
                );
            }
            FactOperationStatus::Indeterminate => {
                return Err(FactManagementError::UnknownCommit(operation_id));
            }
            FactOperationStatus::NotCommitted => {}
        }
        self.check_base(expected_base)?;
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Event(event) if event.id() == target_id => Some(event.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Event is unavailable",
            ))?;
        let target_policy = event_target(&target);
        self.authorize(Capability::EventRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(
            Capability::HistorySpaceRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(
            Capability::LayerRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        if target.created_revision() != expected_target_created_revision {
            return Err(FactManagementError::Conflict);
        }
        self.authorize(Capability::EventCorrect, target_policy)?;
        self.authorize(Capability::EventCreate, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(
            Capability::LayerWrite,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;
        self.authorize(
            Capability::FieldRead,
            record_field_target(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                FieldSelector::EventKind,
            ),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        let target_retracted = existing_records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_id)
        });
        if target_retracted {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event is already retracted",
            ));
        }

        let context = (replacement.history_space_id(), replacement.layer_id());
        let write_target = context_target_parts(context.0, context.1);
        self.authorize(Capability::LayerWrite, write_target)?;
        self.authorize(Capability::EntityReference, write_target)?;
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(context.0, context.1, FieldSelector::EventKind),
        )?;
        for participant in replacement.participants().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    context.0,
                    context.1,
                    FieldSelector::EventParticipant(
                        replacement.event_kind_id(),
                        participant.role_id(),
                    ),
                ),
            )?;
        }
        for attribute in replacement.attributes().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    context.0,
                    context.1,
                    FieldSelector::EventAttribute(
                        replacement.event_kind_id(),
                        attribute.attribute_id(),
                    ),
                ),
            )?;
        }
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(
                context.0,
                context.1,
                FieldSelector::EventTime(replacement.event_kind_id()),
            ),
        )?;

        let metadata = self.write_validation_metadata()?;
        if metadata.revision() != expected_base {
            return Err(FactManagementError::Conflict);
        }
        let references = validate_write_references(
            vec![],
            vec![replacement.clone()],
            &WriteReferenceSnapshot {
                history_spaces: metadata.history_spaces(),
                layers: metadata.layers(),
                entities: metadata.entities(),
                perspectives: metadata.perspectives(),
                schema: metadata.schema(),
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_validated_write_batch(
            &references,
            self.policy_snapshot()?,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;

        self.ensure_identity_available(RecordRef::Event(replacement_id))?;
        self.ensure_identity_available(RecordRef::Provenance(corrects_id))?;
        let commit_revision = self.next_revision()?;
        let (replacement, edge, correction) = prepare_event_correction(
            &target,
            replacement_id,
            replacement,
            corrects_id,
            commit_revision,
            |target_kind, replacement_kind| target_kind == replacement_kind,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let receipt = EventCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: correction.replacement_id(),
            corrects: correction.corrects_id(),
        };
        self.publish_batch(
            expected_base,
            operation_id,
            vec![Record::Event(replacement), Record::Provenance(edge)],
            RecordRef::Event(receipt.replacement),
            target_policy,
        )?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    fn replay_event_correction(
        &self,
        expected_base: Revision,
        operation_id: OperationId,
        commit_revision: Revision,
        target_id: EventId,
        expected_target_created_revision: Revision,
        replacement_id: EventId,
        corrects_id: ProvenanceId,
        replacement: EventDraft,
    ) -> Result<EventCorrectionReceipt, FactManagementError> {
        if expected_base
            .next_commit()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?
            != commit_revision
        {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Event(event) if event.id() == target_id => Some(event.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::IdempotencyMismatch)?;
        if target.created_revision() != expected_target_created_revision {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let target_policy = event_target(&target);
        self.authorize(Capability::EventRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        let owning_context = context_target_parts(target.history_space_id(), target.layer_id());
        self.authorize(Capability::HistorySpaceRead, owning_context)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(Capability::LayerRead, owning_context)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(Capability::EventCorrect, target_policy)?;
        self.authorize(Capability::EventCreate, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(Capability::LayerWrite, owning_context)?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;
        self.authorize(
            Capability::FieldRead,
            record_field_target(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                FieldSelector::EventKind,
            ),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        if existing_records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_id)
        }) {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let write_context =
            context_target_parts(replacement.history_space_id(), replacement.layer_id());
        self.authorize(Capability::LayerWrite, write_context)?;
        self.authorize(Capability::EntityReference, write_context)?;
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(
                replacement.history_space_id(),
                replacement.layer_id(),
                FieldSelector::EventKind,
            ),
        )?;
        for participant in replacement.participants().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    replacement.history_space_id(),
                    replacement.layer_id(),
                    FieldSelector::EventParticipant(
                        replacement.event_kind_id(),
                        participant.role_id(),
                    ),
                ),
            )?;
        }
        for attribute in replacement.attributes().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    replacement.history_space_id(),
                    replacement.layer_id(),
                    FieldSelector::EventAttribute(
                        replacement.event_kind_id(),
                        attribute.attribute_id(),
                    ),
                ),
            )?;
        }
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(
                replacement.history_space_id(),
                replacement.layer_id(),
                FieldSelector::EventTime(replacement.event_kind_id()),
            ),
        )?;
        let (event, edge, correction) = prepare_event_correction(
            &target,
            replacement_id,
            replacement,
            corrects_id,
            commit_revision,
            |target_kind, replacement_kind| target_kind == replacement_kind,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.verify_replayed_records(
            commit_revision,
            &[Record::Event(event), Record::Provenance(edge)],
        )?;
        Ok(EventCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: correction.replacement_id(),
            corrects: correction.corrects_id(),
        })
    }

    /// Creates one immutable Mask. Proposition selectors receive the same
    /// schema and Entity-reference checks as their matching Assertion value.
    pub fn create_mask(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        mask_id: MaskId,
        context: ContextKey,
        selector: MaskSelector,
        validity: Option<AssertionValidity>,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let target = context_target(context);
        self.authorize(Capability::MaskCreate, target)?;
        self.authorize(Capability::LayerWrite, target)?;
        self.authorize(
            Capability::FieldWrite,
            field_target(context, FieldSelector::MaskSelector),
        )?;
        self.authorize(
            Capability::FieldWrite,
            field_target(context, FieldSelector::MaskValidity),
        )?;
        self.authorize(Capability::EntityReference, target)?;
        if matches!(
            context.perspective_scope(),
            worlddb_core::PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, target)?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_context(&metadata, context)?;
        validate_validity(metadata.schema(), validity, false)?;
        match &selector {
            MaskSelector::ExactAssertion(assertion_id) => {
                let assertion = self
                    .records_at_revision(self.revision())?
                    .into_iter()
                    .find_map(|record| match record {
                        Record::Assertion(assertion) if assertion.id() == *assertion_id => {
                            Some(assertion)
                        }
                        _ => None,
                    })
                    .ok_or(FactManagementError::InvalidCandidate(
                        "the selected Assertion is unavailable",
                    ))?;
                self.authorize(
                    Capability::AssertionRead,
                    context_target(assertion.context()),
                )?;
            }
            MaskSelector::Proposition(key) => {
                let predicate =
                    validate_subject_and_predicate(&metadata, key.subject(), key.predicate_id())?;
                validate_predicate_value(metadata.schema(), predicate, key.value())?;
                validate_entity_value(&metadata, predicate, key.value())?;
            }
            MaskSelector::Slot(slot) => {
                let predicate =
                    validate_subject_and_predicate(&metadata, slot.subject(), slot.predicate_id())?;
                if slot.perspective_scope() != context.perspective_scope()
                    || slot.epistemic_mode() != context.epistemic_mode()
                {
                    return Err(FactManagementError::InvalidCandidate(
                        "Mask slot and context partitions do not match",
                    ));
                }
                let _ = predicate;
            }
        }
        self.ensure_identity_available(RecordRef::Mask(mask_id))?;
        let revision = self.next_revision()?;
        let mask = Mask::new(mask_id, context, selector, validity, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::Mask(mask),
            RecordRef::Mask(mask_id),
            target,
        )
    }

    /// Creates one immutable MultiValueReplace boundary.
    pub fn create_replacement_boundary(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        boundary_id: ReplacementBoundaryId,
        draft: ReplacementBoundaryDraft,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let context = draft.context();
        let subject = draft.subject();
        let predicate_id = draft.predicate_id();
        let validity = draft.validity();
        let target = context_target(context);
        self.authorize(Capability::ReplacementBoundaryCreate, target)?;
        self.authorize(Capability::LayerWrite, target)?;
        for field in [
            FieldSelector::ReplacementBoundarySubject,
            FieldSelector::ReplacementBoundaryPredicate,
            FieldSelector::ReplacementBoundaryValidity,
        ] {
            self.authorize(Capability::FieldWrite, field_target(context, field))?;
        }
        self.authorize(Capability::EntityReference, target)?;
        if matches!(
            context.perspective_scope(),
            worlddb_core::PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, target)?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_context(&metadata, context)?;
        validate_validity(metadata.schema(), validity, false)?;
        let predicate = validate_subject_and_predicate(&metadata, subject, predicate_id)?;
        if predicate.resolution_policy() != worlddb_core::ResolutionPolicy::MultiValueReplace {
            return Err(FactManagementError::InvalidCandidate(
                "the Predicate does not use MultiValueReplace",
            ));
        }
        self.ensure_identity_available(RecordRef::ReplacementBoundary(boundary_id))?;
        let revision = self.next_revision()?;
        let boundary =
            ReplacementBoundary::new(boundary_id, context, subject, predicate, validity, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::ReplacementBoundary(boundary),
            RecordRef::ReplacementBoundary(boundary_id),
            target,
        )
    }

    /// Appends a separate explicit Transaction-Time EventRetraction.
    pub fn retract_event(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: EventRetractionId,
        target_id: EventId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::Event(value) if value.id() == target_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Event is unavailable",
            ))?;
        let target_policy = event_target(&target);
        self.authorize(Capability::EventRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(
            Capability::HistorySpaceRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(
            Capability::LayerRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::EventRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
            ),
        )?;
        if records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::EventRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction = EventRetraction::new(retraction_id, &target, reason, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::EventRetraction(retraction),
            RecordRef::EventRetraction(retraction_id),
            target_policy,
        )
    }

    /// Appends an explicit Transaction-Time retraction for one active Assertion.
    pub fn retract_assertion(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: AssertionRetractionId,
        target_id: AssertionId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::Assertion(value) if value.id() == target_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Assertion is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::AssertionRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::AssertionRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target(target.context(), RecordRef::Assertion(target_id)),
        )?;
        if records.iter().any(|record| {
            matches!(
                record,
                Record::AssertionRetraction(value) if value.assertion_id() == target_id
            )
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Assertion is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::AssertionRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction = AssertionRetraction::new(retraction_id, &target, reason, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::AssertionRetraction(retraction),
            RecordRef::AssertionRetraction(retraction_id),
            target_policy,
        )
    }

    /// Appends an explicit Transaction-Time retraction for one active Mask.
    pub fn retract_mask(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: MaskRetractionId,
        target_id: MaskId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::Mask(value) if value.id() == target_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Mask is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::MaskRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Mask is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Mask is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Mask is unavailable")
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::MaskRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target(target.context(), RecordRef::Mask(target_id)),
        )?;
        if records.iter().any(|record| {
            matches!(record, Record::MaskRetraction(value) if value.mask_id() == target_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Mask is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::MaskRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction =
            worlddb_core::MaskRetraction::new(retraction_id, &target, reason, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::MaskRetraction(retraction),
            RecordRef::MaskRetraction(retraction_id),
            target_policy,
        )
    }

    /// Appends an explicit Transaction-Time retraction for one active Boundary.
    pub fn retract_replacement_boundary(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: ReplacementBoundaryRetractionId,
        target_id: ReplacementBoundaryId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::ReplacementBoundary(value) if value.id() == target_id => {
                    Some(value.clone())
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected ReplacementBoundary is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::ReplacementBoundaryRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "the selected ReplacementBoundary is unavailable",
                )
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "the selected ReplacementBoundary is unavailable",
                )
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "the selected ReplacementBoundary is unavailable",
                )
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::ReplacementBoundaryRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target(
                target.context(),
                RecordRef::ReplacementBoundary(target_id),
            ),
        )?;
        if records.iter().any(|record| {
            matches!(
                record,
                Record::ReplacementBoundaryRetraction(value)
                    if value.replacement_boundary_id() == target_id
            )
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected ReplacementBoundary is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::ReplacementBoundaryRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction =
            ReplacementBoundaryRetraction::new(retraction_id, &target, reason, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::ReplacementBoundaryRetraction(retraction),
            RecordRef::ReplacementBoundaryRetraction(retraction_id),
            target_policy,
        )
    }

    /// Archives or unarchives one Assertion, Mask, ReplacementBoundary, or Event.
    /// The transition is append-only and is always committed strictly after
    /// the selected target's creation revision.
    pub fn transition_archive(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        transition_id: ArchiveTransitionId,
        target: ArchiveTargetRef,
        action: ArchiveAction,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let (target_ref, history_space, layer, target_created_revision, read_capability) = records
            .iter()
            .find_map(|record| match (target, record) {
                (ArchiveTargetRef::Assertion(id), Record::Assertion(value)) if id == value.id() => {
                    Some((
                        RecordRef::Assertion(id),
                        value.context().history_space_id(),
                        value.context().layer_id(),
                        value.created_revision(),
                        Capability::AssertionRead,
                    ))
                }
                (ArchiveTargetRef::Mask(id), Record::Mask(value)) if id == value.id() => Some((
                    RecordRef::Mask(id),
                    value.context().history_space_id(),
                    value.context().layer_id(),
                    value.created_revision(),
                    Capability::MaskRead,
                )),
                (ArchiveTargetRef::ReplacementBoundary(id), Record::ReplacementBoundary(value))
                    if id == value.id() =>
                {
                    Some((
                        RecordRef::ReplacementBoundary(id),
                        value.context().history_space_id(),
                        value.context().layer_id(),
                        value.created_revision(),
                        Capability::ReplacementBoundaryRead,
                    ))
                }
                (ArchiveTargetRef::Event(id), Record::Event(value)) if id == value.id() => Some((
                    RecordRef::Event(id),
                    value.history_space_id(),
                    value.layer_id(),
                    value.created_revision(),
                    Capability::EventRead,
                )),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected factual record is unavailable",
            ))?;
        let target_policy = context_target_parts(history_space, layer);
        self.authorize(read_capability, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected factual record is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected factual record is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected factual record is unavailable")
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        let permission = match action {
            ArchiveAction::Archive => Capability::Archive,
            ArchiveAction::Unarchive => Capability::Unarchive,
        };
        self.authorize(permission, record_target(target_ref))?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target_parts(history_space, layer, target_ref),
        )?;
        self.ensure_identity_available(RecordRef::ArchiveTransition(transition_id))?;

        let mut archive_targets = Vec::new();
        let mut transitions = Vec::new();
        for record in &records {
            match record {
                Record::Assertion(value) => archive_targets.push(ArchiveTargetRecord::new(
                    ArchiveTargetRef::Assertion(value.id()),
                    value.created_revision(),
                )),
                Record::Mask(value) => archive_targets.push(ArchiveTargetRecord::new(
                    ArchiveTargetRef::Mask(value.id()),
                    value.created_revision(),
                )),
                Record::ReplacementBoundary(value) => {
                    archive_targets.push(ArchiveTargetRecord::new(
                        ArchiveTargetRef::ReplacementBoundary(value.id()),
                        value.created_revision(),
                    ))
                }
                Record::Event(value) => archive_targets.push(ArchiveTargetRecord::new(
                    ArchiveTargetRef::Event(value.id()),
                    value.created_revision(),
                )),
                Record::ArchiveTransition(value)
                    if archive_target_record_ref(value.target()).is_some() =>
                {
                    transitions.push(*value);
                }
                _ => {}
            }
        }
        let history =
            ArchiveHistoryReferenceModel::new(archive_targets.clone(), transitions.clone())
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let prior_state = history
            .state_at(target, RecordedAsOf::from_published_revision(expected_base))
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let revision = self.next_revision()?;
        if revision <= target_created_revision {
            return Err(FactManagementError::InvalidRecordRevision);
        }
        let transition =
            ArchiveTransition::new(transition_id, target, action, prior_state, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::validate_archive_transition_transaction(
            worlddb_core::ArchiveTransitionValidation {
                base_revision: expected_base,
                commit_revision: revision,
                base: &history,
                target_records_after: history.targets().to_vec(),
                transition_additions: &[transition],
                policy: self.policy_snapshot()?,
                principal: self.schema.principal,
            },
            &[Record::ArchiveTransition(transition)],
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::ArchiveTransition(transition),
            RecordRef::ArchiveTransition(transition_id),
            target_policy,
        )
    }

    /// Reads factual record families at one committed shared revision.
    pub fn snapshot_at(&self, revision: Revision) -> Result<FactSnapshot, FactManagementError> {
        if revision > self.revision() {
            return Err(FactManagementError::RevisionNotPublished);
        }
        let records = self.records_at_revision(revision)?;
        let mut assertions = Vec::new();
        let mut masks = Vec::new();
        let mut replacement_boundaries = Vec::new();
        let mut events = Vec::new();
        for record in &records {
            match record {
                Record::Assertion(value) => {
                    let target = context_target(value.context());
                    if self.may_read(Capability::AssertionRead, target)?
                        && self.may_read(Capability::HistorySpaceRead, target)?
                        && self.may_read(Capability::LayerRead, target)?
                    {
                        assertions.push(value.clone());
                    }
                }
                Record::Mask(value) => {
                    let target = context_target(value.context());
                    if self.may_read(Capability::MaskRead, target)?
                        && self.may_read(Capability::HistorySpaceRead, target)?
                        && self.may_read(Capability::LayerRead, target)?
                    {
                        masks.push(value.clone());
                    }
                }
                Record::ReplacementBoundary(value) => {
                    let target = context_target(value.context());
                    if self.may_read(Capability::ReplacementBoundaryRead, target)?
                        && self.may_read(Capability::HistorySpaceRead, target)?
                        && self.may_read(Capability::LayerRead, target)?
                    {
                        replacement_boundaries.push(value.clone());
                    }
                }
                Record::Event(value) => {
                    let owner = context_target_parts(value.history_space_id(), value.layer_id());
                    if self.may_read(Capability::EventRead, event_target(value))?
                        && self.may_read(Capability::HistorySpaceRead, owner)?
                        && self.may_read(Capability::LayerRead, owner)?
                    {
                        let event_kind_id = self
                            .may_read(
                                Capability::FieldRead,
                                record_field_target(
                                    value.history_space_id(),
                                    value.layer_id(),
                                    RecordRef::Event(value.id()),
                                    FieldSelector::EventKind,
                                ),
                            )?
                            .then_some(value.event_kind_id());
                        events.push(EventSnapshotRef {
                            event_id: value.id(),
                            created_revision: value.created_revision(),
                            history_space_id: value.history_space_id(),
                            layer_id: value.layer_id(),
                            event_kind_id,
                        });
                    }
                }
                _ => {}
            }
        }
        let visible_refs = assertions
            .iter()
            .map(|value| RecordRef::Assertion(value.id()))
            .chain(masks.iter().map(|value| RecordRef::Mask(value.id())))
            .chain(
                replacement_boundaries
                    .iter()
                    .map(|value| RecordRef::ReplacementBoundary(value.id())),
            )
            .chain(
                events
                    .iter()
                    .map(|value| RecordRef::Event(value.event_id())),
            )
            .collect::<BTreeSet<_>>();
        let may_read_lifecycle =
            self.may_read(Capability::LifecycleRead, PolicyTarget::default())?;
        let mut assertion_retractions = Vec::new();
        let mut mask_retractions = Vec::new();
        let mut replacement_boundary_retractions = Vec::new();
        let mut event_retractions = Vec::new();
        let mut archive_transitions = Vec::new();
        if may_read_lifecycle {
            for record in &records {
                match record {
                    Record::AssertionRetraction(value)
                        if visible_refs.contains(&RecordRef::Assertion(value.assertion_id())) =>
                    {
                        assertion_retractions.push(value.clone());
                    }
                    Record::MaskRetraction(value)
                        if visible_refs.contains(&RecordRef::Mask(value.mask_id())) =>
                    {
                        mask_retractions.push(value.clone());
                    }
                    Record::ReplacementBoundaryRetraction(value)
                        if visible_refs.contains(&RecordRef::ReplacementBoundary(
                            value.replacement_boundary_id(),
                        )) =>
                    {
                        replacement_boundary_retractions.push(value.clone());
                    }
                    Record::EventRetraction(value)
                        if visible_refs.contains(&RecordRef::Event(value.event_id())) =>
                    {
                        event_retractions.push(value.clone());
                    }
                    Record::ArchiveTransition(value)
                        if archive_target_record_ref(value.target())
                            .is_some_and(|target| visible_refs.contains(&target)) =>
                    {
                        archive_transitions.push(*value);
                    }
                    _ => {}
                }
            }
        }
        Ok(FactSnapshot {
            revision,
            assertions,
            assertion_retractions,
            masks,
            mask_retractions,
            replacement_boundaries,
            replacement_boundary_retractions,
            events,
            event_retractions,
            archive_transitions,
            lifecycle_visible: may_read_lifecycle,
        })
    }

    /// Executes the productive M8 resolution preview over one authorized,
    /// revision-pinned factual and metadata snapshot.
    pub fn resolution_preview(
        &self,
        request: FactResolutionPreviewRequest,
    ) -> Result<QueryEngineOutput<ResolutionPreview>, FactManagementError> {
        self.authorize(Capability::QueryResolve, PolicyTarget::default())?;
        let revision = self.revision();
        let metadata = self.write_validation_metadata()?;
        if metadata.revision() != revision {
            return Err(FactManagementError::Conflict);
        }
        let facts = self.snapshot_at(revision)?;
        let records = self.records_at_revision(revision)?;
        let recorded_as_of = RecordedAsOf::from_published_revision(revision);
        let schema_binding = HistoricalQueryBinding::bind(
            &self.schema.schema_history,
            recorded_as_of,
            SchemaMode::Historical,
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let predicate = metadata
            .schema()
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::Predicate(predicate)
                    if predicate.predicate_id() == request.predicate_id =>
                {
                    Some(predicate)
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Predicate is unavailable in the pinned schema",
            ))?;
        let timeline_is_active = match request.world_time {
            WorldTimeSelector::AllTimes => true,
            WorldTimeSelector::At(time) => metadata
                .schema()
                .timeline(time.timeline().id())
                .is_some_and(|timeline| timeline.lifecycle() == Lifecycle::Active),
        };
        if !timeline_is_active {
            return Err(FactManagementError::InvalidCandidate(
                "the selected query time requires an Active registered Timeline",
            ));
        }
        if metadata
            .history_spaces()
            .definition(request.history_space_id)
            .is_none()
        {
            return Err(FactManagementError::InvalidCandidate(
                "the selected HistorySpace is unavailable",
            ));
        }
        let layers = ValidatedLayerSelection::resolve(metadata.layers(), request.layer_selection)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let budget_limits = QueryBudgetLimits::new(100_000, 500_000, 20_000)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let budget = QueryBudget::new(100_000, 500_000, 20_000, budget_limits)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let snapshot_id = worlddb_core::storage_internal::generate_project_bootstrap_id::<
            worlddb_core::SnapshotId,
        >()
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let context = QueryContext::new(QueryContextInput {
            snapshot: SnapshotRef::new(snapshot_id),
            snapshot_revision: revision,
            recorded_as_of,
            history_space: request.history_space_id,
            layers,
            world_time: request.world_time,
            perspective: request.perspective_scope,
            epistemic_mode: request.epistemic_mode,
            schema_binding,
            security: SecurityContext::new(
                self.schema.principal,
                worlddb_core::AuthorizationMode::Now,
            ),
            budget,
            cancellation: CancellationToken::new(),
        })
        .map_err(|error| FactManagementError::Query(error.to_string()))?;

        let mut assertion_commits =
            BTreeMap::<Revision, Vec<(HistorySpaceId, AssertionHistoryRecord)>>::new();
        for assertion in facts.assertions() {
            assertion_commits
                .entry(assertion.created_revision())
                .or_default()
                .push((
                    assertion.context().history_space_id(),
                    AssertionHistoryRecord::from_assertion(assertion.clone()),
                ));
        }
        for retraction in records.iter().filter_map(|record| match record {
            Record::AssertionRetraction(value)
                if facts
                    .assertions()
                    .iter()
                    .any(|assertion| assertion.id() == value.assertion_id()) =>
            {
                Some(value.clone())
            }
            _ => None,
        }) {
            let history_space_id = facts
                .assertions()
                .iter()
                .find(|assertion| assertion.id() == retraction.assertion_id())
                .map(|assertion| assertion.context().history_space_id())
                .ok_or(FactManagementError::InvalidCandidate(
                    "an Assertion lifecycle target is unavailable",
                ))?;
            assertion_commits
                .entry(retraction.created_revision())
                .or_default()
                .push((
                    history_space_id,
                    AssertionHistoryRecord::Retraction(retraction),
                ));
        }
        let history = HistorySpaceReferenceModel::from_published_snapshot(
            metadata.history_spaces().definitions().to_vec(),
            revision,
            assertion_commits.into_iter().collect(),
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;

        let mut archive_targets = Vec::new();
        for assertion in facts.assertions() {
            archive_targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::Assertion(assertion.id()),
                assertion.created_revision(),
            ));
        }
        for mask in facts.masks() {
            archive_targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::Mask(mask.id()),
                mask.created_revision(),
            ));
        }
        for boundary in facts.replacement_boundaries() {
            archive_targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::ReplacementBoundary(boundary.id()),
                boundary.created_revision(),
            ));
        }
        let visible_refs = facts
            .assertions()
            .iter()
            .map(|value| RecordRef::Assertion(value.id()))
            .chain(
                facts
                    .masks()
                    .iter()
                    .map(|value| RecordRef::Mask(value.id())),
            )
            .chain(
                facts
                    .replacement_boundaries()
                    .iter()
                    .map(|value| RecordRef::ReplacementBoundary(value.id())),
            )
            .collect::<BTreeSet<_>>();
        let archive_transitions = records
            .iter()
            .filter_map(|record| match record {
                Record::ArchiveTransition(value)
                    if archive_target_record_ref(value.target())
                        .is_some_and(|target| visible_refs.contains(&target)) =>
                {
                    Some(*value)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let archive = ArchiveHistoryReferenceModel::new(archive_targets, archive_transitions)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let mut archive_visible_masks = BTreeSet::new();
        let mut archive_visible_boundaries = BTreeSet::new();
        for mask in facts.masks() {
            if archive
                .state_at(ArchiveTargetRef::Mask(mask.id()), recorded_as_of)
                .map_err(|error| FactManagementError::Query(error.to_string()))?
                == ArchiveState::Unarchived
            {
                archive_visible_masks.insert(mask.id());
            }
        }
        for boundary in facts.replacement_boundaries() {
            if archive
                .state_at(
                    ArchiveTargetRef::ReplacementBoundary(boundary.id()),
                    recorded_as_of,
                )
                .map_err(|error| FactManagementError::Query(error.to_string()))?
                == ArchiveState::Unarchived
            {
                archive_visible_boundaries.insert(boundary.id());
            }
        }
        let policies = self.schema.policy_history.policy();
        let store = AssertionQueryStore::new(&history, &archive, metadata.layers(), policies);
        let no_mask_closures = [];
        let mask_retractions = records
            .iter()
            .filter_map(|record| match record {
                Record::MaskRetraction(value)
                    if facts
                        .masks()
                        .iter()
                        .any(|mask| mask.id() == value.mask_id()) =>
                {
                    Some(value.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let masks = AuthorizedAssertionMaskHistory::new(
            facts.masks(),
            &no_mask_closures,
            &mask_retractions,
            &archive_visible_masks,
        );
        let no_boundary_closures = [];
        let boundary_retractions = records
            .iter()
            .filter_map(|record| match record {
                Record::ReplacementBoundaryRetraction(value)
                    if facts
                        .replacement_boundaries()
                        .iter()
                        .any(|boundary| boundary.id() == value.replacement_boundary_id()) =>
                {
                    Some(value.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let boundaries = ReplacementBoundarySource::new(
            facts.replacement_boundaries(),
            &no_boundary_closures,
            &boundary_retractions,
            &archive_visible_boundaries,
        );
        let query = AssertionPointRequest::new(
            &context,
            masks,
            boundaries,
            AssertionPointIndexAccess::missing(),
            FullScanBudget::Available,
        );
        ProductiveQueryEngine::resolution_preview(
            store,
            query,
            worlddb_core::MultiValueSlot::new(request.subject, request.predicate_id),
            predicate,
            |left, right| temporal_value_equality(metadata.schema(), left, right),
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))
    }

    fn check_base(&self, expected_base: Revision) -> Result<(), FactManagementError> {
        if expected_base == self.revision() {
            Ok(())
        } else {
            Err(FactManagementError::Conflict)
        }
    }

    fn next_revision(&self) -> Result<Revision, FactManagementError> {
        self.schema
            .next_revision()
            .map_err(|error| FactManagementError::Storage(error.to_string()))
    }

    fn write_validation_metadata(
        &self,
    ) -> Result<worlddb_core::ProjectMetadataSnapshot, FactManagementError> {
        let manager = FileProjectMetadataManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        manager
            .snapshot_for_write_validation()
            .map_err(|error| FactManagementError::Storage(error.to_string()))
    }

    fn policy_snapshot(
        &self,
    ) -> Result<&worlddb_core::SecurityPolicySnapshot, FactManagementError> {
        self.schema
            .policy_history
            .policy()
            .latest_version()
            .map(|version| version.snapshot())
            .map_err(|error| FactManagementError::Storage(error.to_string()))
    }

    fn authorize(
        &self,
        capability: Capability,
        target: PolicyTarget,
    ) -> Result<(), FactManagementError> {
        if self
            .policy_snapshot()?
            .authorize(self.schema.principal, capability, target)
            == AuthorizationDecision::Allow
        {
            Ok(())
        } else {
            Err(FactManagementError::Unauthorized(capability))
        }
    }

    fn may_read(
        &self,
        capability: Capability,
        target: PolicyTarget,
    ) -> Result<bool, FactManagementError> {
        Ok(self
            .policy_snapshot()?
            .authorize(self.schema.principal, capability, target)
            == AuthorizationDecision::Allow)
    }

    fn ensure_identity_available(&self, identity: RecordRef) -> Result<(), FactManagementError> {
        let duplicate = self
            .records_at_revision(self.revision())?
            .iter()
            .any(|record| fact_record_ref(record) == Some(identity));
        if duplicate {
            Err(FactManagementError::DuplicateIdentity)
        } else {
            Ok(())
        }
    }

    fn records_at_revision(&self, revision: Revision) -> Result<Vec<Record>, FactManagementError> {
        let mut records = Vec::new();
        let store = HistorySegmentStore::new(self.schema.layout.clone());
        for reference in self
            .schema
            .manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
        {
            if reference.through_revision() > self.schema.manifest.revision() {
                return Err(FactManagementError::Storage(
                    "history reference exceeds the manifest revision".to_owned(),
                ));
            }
            let segment = store.read_segment(reference.id()).map_err(storage_error)?;
            if segment.content_digest() != reference.content_digest() {
                return Err(FactManagementError::Storage(
                    "history segment digest differs from its manifest".to_owned(),
                ));
            }
            for decoded in segment.records() {
                let record = decoded.record();
                let created = record_created_revision(record);
                let Some(created) = created else { continue };
                if created > reference.through_revision()
                    || created > self.schema.manifest.revision()
                {
                    return Err(FactManagementError::Storage(
                        "factual record exceeds its committed history segment".to_owned(),
                    ));
                }
                if created <= revision {
                    records.push(record.clone());
                }
            }
        }
        Ok(records)
    }

    fn verify_replayed_records(
        &self,
        revision: Revision,
        expected: &[Record],
    ) -> Result<(), FactManagementError> {
        let records = self.records_at_revision(revision)?;
        let mut actual = records
            .iter()
            .filter(|record| record_created_revision(record) == Some(revision))
            .map(|record| encode_record(record).map_err(storage_error))
            .collect::<Result<Vec<_>, _>>()?;
        let mut expected = expected
            .iter()
            .map(|record| encode_record(record).map_err(storage_error))
            .collect::<Result<Vec<_>, _>>()?;
        actual.sort();
        expected.sort();
        if actual == expected {
            Ok(())
        } else {
            Err(FactManagementError::IdempotencyMismatch)
        }
    }

    fn publish(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        record: Record,
        record_ref: RecordRef,
        policy_target: PolicyTarget,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.publish_batch(
            expected_base,
            operation_id,
            vec![record],
            record_ref,
            policy_target,
        )
    }

    fn publish_batch(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        records: Vec<Record>,
        record_ref: RecordRef,
        policy_target: PolicyTarget,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        if records.is_empty() {
            return Err(FactManagementError::InvalidRecordRevision);
        }
        self.check_base(expected_base)?;
        let wal = WalPrepareLog::new(&self.schema.layout);
        let live_head = wal
            .commit_head(self.schema.writer_lock)
            .map_err(storage_error)?
            .revision();
        if live_head != expected_base {
            return Err(FactManagementError::Conflict);
        }
        if wal
            .operation_status(self.schema.writer_lock, operation_id)
            .map_err(storage_error)?
            != WalOperationStatus::NotCommitted
        {
            return Err(FactManagementError::OperationAlreadyUsed);
        }

        let policy_version = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let target_revision = expected_base
            .next_commit()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        if records
            .iter()
            .any(|record| record_created_revision(record) != Some(target_revision))
        {
            return Err(FactManagementError::InvalidRecordRevision);
        }
        let sequence =
            next_audit_sequence(&wal, self.schema.writer_lock).map_err(map_schema_error)?;
        let policy_fingerprint = AuditPolicyFingerprint::new(Bytes::new(
            policy_version
                .snapshot()
                .effective_capability_fingerprint(self.schema.principal, policy_target)
                .to_vec(),
        ))
        .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id: worlddb_core::storage_internal::generate_factual_record_audit_record_id(
                )
                .map_err(storage_error)?,
                sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_factual_record_audit_operation_id()
                        .map_err(storage_error)?,
            },
            AuditRecordDetails {
                actor: self.schema.principal,
                action: AuditAction::FactualRecordWrite,
                object_class: AuditObjectClass::FactualRecord,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: target_revision,
                    operation_id,
                },
                security_epoch: policy_version.epoch(),
                policy_fingerprint,
            },
        );

        let history_store = HistorySegmentStore::new(self.schema.layout.clone());
        let history_receipt = history_store
            .stage_segment(self.schema.writer_lock, &records)
            .map_err(storage_error)?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            target_revision,
        );
        let security_store = SecurityPolicyHistoryStore::new(self.schema.layout.clone());
        let retention = self
            .schema
            .policy_history
            .audit_retention_at(expected_base)
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let next_policy_version = SecurityPolicyVersion::new(
            target_revision,
            policy_version.epoch(),
            policy_version.snapshot().clone(),
        );
        let security_receipt = match security_store.stage_version(
            self.schema.writer_lock,
            &next_policy_version,
            None,
            retention,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let cleanup = history_store.remove_staged_reference(history_reference);
                return Err(FactManagementError::Storage(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; staged factual history cleanup failed: {cleanup_error}")
                    }
                }));
            }
        };
        let security_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            security_receipt.id(),
            security_receipt.content_digest(),
            target_revision,
        );
        let mut next_references = self.schema.manifest.segments().to_vec();
        next_references.push(history_reference);
        next_references.push(security_reference);
        let staged = [history_reference, security_reference];
        match wal.commit_audited_manifest_snapshot(
            self.schema.writer_lock,
            operation_id,
            next_references,
            &staged,
            &audit_record,
        ) {
            Ok(receipt) if receipt.revision() == target_revision => {}
            Ok(_) => return Err(FactManagementError::UnknownCommit(operation_id)),
            Err(commit_error) => {
                let recovered = RecoveryManager::new(self.schema.layout.clone())
                    .recover(self.schema.writer_lock)
                    .map_err(|recovery_error| {
                        FactManagementError::UnknownCommitWithDiagnostic(
                            operation_id,
                            format!("{commit_error}; recovery: {recovery_error}"),
                        )
                    })?;
                let status = wal
                    .operation_status(self.schema.writer_lock, operation_id)
                    .map_err(storage_error)?;
                if !recovered.report().is_clean()
                    || !matches!(status, WalOperationStatus::Committed(receipt) if receipt.revision() == target_revision)
                {
                    if status == WalOperationStatus::NotCommitted {
                        cleanup_staged_schema(
                            &history_store,
                            history_reference,
                            &security_store,
                            security_reference,
                        );
                        return Err(FactManagementError::Storage(commit_error.to_string()));
                    }
                    return Err(FactManagementError::UnknownCommitWithDiagnostic(
                        operation_id,
                        commit_error.to_string(),
                    ));
                }
            }
        }

        let recovered = RecoveryManager::new(self.schema.layout.clone())
            .recover(self.schema.writer_lock)
            .map_err(|error| {
                FactManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
            })?;
        if !recovered.report().is_clean() {
            return Err(FactManagementError::UnknownCommit(operation_id));
        }
        let next_schema = FileSchemaManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| {
            FactManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
        })?;
        if next_schema.revision() != target_revision {
            return Err(FactManagementError::UnknownCommit(operation_id));
        }
        self.schema = next_schema;
        Ok(FactPublicationReceipt {
            operation_id,
            revision: target_revision,
            record_ref,
        })
    }
}

fn context_target(context: ContextKey) -> PolicyTarget {
    context_target_parts(context.history_space_id(), context.layer_id())
}

fn context_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
) -> PolicyTarget {
    PolicyTarget::new(Some(history_space), Some(layer), None, None, None)
}

fn event_target(event: &Event) -> PolicyTarget {
    PolicyTarget::new(
        Some(event.history_space_id()),
        Some(event.layer_id()),
        Some(RecordRef::Event(event.id())),
        None,
        None,
    )
}

fn field_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    field: FieldSelector,
) -> PolicyTarget {
    PolicyTarget::new(Some(history_space), Some(layer), None, Some(field), None)
}

fn record_field_target(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    record: RecordRef,
    field: FieldSelector,
) -> PolicyTarget {
    PolicyTarget::new(
        Some(history_space),
        Some(layer),
        Some(record),
        Some(field),
        None,
    )
}

fn relationship_target(
    context: ContextKey,
    record: RecordRef,
    relationship: ProvenanceRelationship,
) -> PolicyTarget {
    relationship_target_parts(
        context.history_space_id(),
        context.layer_id(),
        record,
        relationship,
    )
}

fn relationship_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    record: RecordRef,
    relationship: ProvenanceRelationship,
) -> PolicyTarget {
    PolicyTarget::new(
        Some(history_space),
        Some(layer),
        Some(record),
        None,
        Some(RelationshipSelector::Provenance(relationship)),
    )
}

fn lifecycle_relationship_target(context: ContextKey, record: RecordRef) -> PolicyTarget {
    lifecycle_relationship_target_parts(context.history_space_id(), context.layer_id(), record)
}

fn lifecycle_relationship_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    record: RecordRef,
) -> PolicyTarget {
    PolicyTarget::new(
        Some(history_space),
        Some(layer),
        Some(record),
        None,
        Some(RelationshipSelector::LifecycleTarget),
    )
}

fn record_target(record: RecordRef) -> PolicyTarget {
    PolicyTarget::new(None, None, Some(record), None, None)
}

fn validate_retraction_reason(reason: &str) -> Result<(), FactManagementError> {
    if reason.trim().is_empty() {
        Err(FactManagementError::InvalidCandidate(
            "a non-empty retraction reason is required",
        ))
    } else {
        Ok(())
    }
}

fn field_target(context: ContextKey, field: FieldSelector) -> PolicyTarget {
    PolicyTarget::new(
        Some(context.history_space_id()),
        Some(context.layer_id()),
        None,
        Some(field),
        None,
    )
}

fn validate_context(
    metadata: &worlddb_core::ProjectMetadataSnapshot,
    context: ContextKey,
) -> Result<(), FactManagementError> {
    if metadata
        .history_spaces()
        .definition(context.history_space_id())
        .is_none()
    {
        return Err(FactManagementError::InvalidCandidate(
            "the selected HistorySpace is unavailable",
        ));
    }
    let layer = metadata.layers().definition(context.layer_id()).ok_or(
        FactManagementError::InvalidCandidate("the selected Layer is unavailable"),
    )?;
    if layer.lifecycle() == Lifecycle::Retired {
        return Err(FactManagementError::InvalidCandidate(
            "the selected Layer is retired",
        ));
    }
    if let worlddb_core::PerspectiveScope::Perspective(id) = context.perspective_scope() {
        if metadata.perspectives().is_retired(id)
            || metadata.perspectives().latest_definition(id).is_none()
        {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Perspective is unavailable",
            ));
        }
    }
    Ok(())
}

fn validate_subject_and_predicate(
    metadata: &worlddb_core::ProjectMetadataSnapshot,
    subject: Subject,
    predicate_id: PredicateId,
) -> Result<&worlddb_core::PredicateDefinition, FactManagementError> {
    let entity = metadata.entities().entity(subject.entity_id()).ok_or(
        FactManagementError::InvalidCandidate("the selected Entity is unavailable"),
    )?;
    if metadata.entities().is_retired(subject.entity_id()) {
        return Err(FactManagementError::InvalidCandidate(
            "the selected Entity is retired",
        ));
    }
    let schema = metadata.schema();
    let entity_type = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::EntityType(value)
                if value.entity_type_id() == entity.entity_type_id() =>
            {
                Some(value)
            }
            _ => None,
        });
    let Some(entity_type) = entity_type else {
        return Err(FactManagementError::InvalidCandidate(
            "the Entity type is unavailable in the selected schema",
        ));
    };
    if entity_type.lifecycle() == Lifecycle::Retired {
        return Err(FactManagementError::InvalidCandidate(
            "the Entity type is retired",
        ));
    }
    let predicate = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::Predicate(value) if value.predicate_id() == predicate_id => {
                Some(value)
            }
            _ => None,
        });
    let Some(predicate) = predicate else {
        return Err(FactManagementError::InvalidCandidate(
            "the selected Predicate is unavailable",
        ));
    };
    if predicate.lifecycle() != Lifecycle::Active {
        return Err(FactManagementError::InvalidCandidate(
            "new factual records require an Active Predicate",
        ));
    }
    if !entity_type_constraint_matches(predicate.subject_constraint(), entity.entity_type_id()) {
        return Err(FactManagementError::InvalidCandidate(
            "the Entity type does not satisfy the Predicate subject constraint",
        ));
    }
    Ok(predicate)
}

fn entity_type_constraint_matches(
    constraint: EntityTypeConstraint,
    actual: worlddb_core::EntityTypeId,
) -> bool {
    matches!(constraint, EntityTypeConstraint::AnyEntity)
        || matches!(constraint, EntityTypeConstraint::Exact(expected) if expected == actual)
}

fn validate_entity_value(
    metadata: &worlddb_core::ProjectMetadataSnapshot,
    predicate: &worlddb_core::PredicateDefinition,
    value: &Value,
) -> Result<(), FactManagementError> {
    let Value::Entity(entity_id) = value else {
        return Ok(());
    };
    let entity =
        metadata
            .entities()
            .entity(*entity_id)
            .ok_or(FactManagementError::InvalidCandidate(
                "the referenced Entity is unavailable",
            ))?;
    if metadata.entities().is_retired(*entity_id) {
        return Err(FactManagementError::InvalidCandidate(
            "the referenced Entity is retired",
        ));
    }
    let entity_type =
        metadata
            .schema()
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::EntityType(value)
                    if value.entity_type_id() == entity.entity_type_id() =>
                {
                    Some(value)
                }
                _ => None,
            });
    if !entity_type.is_some_and(|value| value.lifecycle() != Lifecycle::Retired) {
        return Err(FactManagementError::InvalidCandidate(
            "the referenced Entity type is unavailable",
        ));
    }
    let constraint = predicate
        .object_constraint()
        .ok_or(FactManagementError::InvalidCandidate(
            "the Predicate has no Entity object constraint",
        ))?;
    if !entity_type_constraint_matches(constraint, entity.entity_type_id()) {
        return Err(FactManagementError::InvalidCandidate(
            "the referenced Entity type does not satisfy the Predicate object constraint",
        ));
    }
    Ok(())
}

fn temporal_value_equality(
    schema: &worlddb_core::SchemaSnapshot,
    left: &Value,
    right: &Value,
) -> Result<bool, ()> {
    Ok(match (left, right) {
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Int(left), Value::Int(right)) => left == right,
        (Value::UInt(left), Value::UInt(right)) => left == right,
        (Value::Decimal(left), Value::Decimal(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Symbol(left), Value::Symbol(right)) => left == right,
        (Value::Entity(left), Value::Entity(right)) => left == right,
        (Value::Duration(left), Value::Duration(right)) => left == right,
        (Value::Bytes(left), Value::Bytes(right)) => left == right,
        (Value::Time(left), Value::Time(right)) => {
            schema.resolve_time(left, false).map_err(|_| ())?
                == schema.resolve_time(right, false).map_err(|_| ())?
        }
        _ => false,
    })
}

fn validate_predicate_value(
    schema: &worlddb_core::SchemaSnapshot,
    predicate: &worlddb_core::PredicateDefinition,
    value: &Value,
) -> Result<(), FactManagementError> {
    if let Value::Time(time) = value {
        schema
            .resolve_time(time, false)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
    }
    for constraint in predicate.constraints().rules() {
        if let worlddb_core::ValueConstraint::TimeRange(range) = constraint {
            for endpoint in [range.min(), range.max()].into_iter().flatten() {
                schema
                    .resolve_time(endpoint, false)
                    .map_err(|error| FactManagementError::Validation(error.to_string()))?;
            }
        }
    }
    validate_value_for_predicate(predicate, value, |time| {
        schema
            .resolve_time(time, false)
            .map_err(|_| worlddb_core::TemporalError::Overflow)
    })
    .map_err(|error| FactManagementError::Validation(error.to_string()))
}

fn validate_validity(
    schema: &worlddb_core::SchemaSnapshot,
    validity: Option<AssertionValidity>,
    allow_deprecated: bool,
) -> Result<(), FactManagementError> {
    let Some(validity) = validity else {
        return Ok(());
    };
    let timeline_id = validity.interval().timeline().id();
    let timeline = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::Timeline(value) if value.timeline_id() == timeline_id => Some(value),
            _ => None,
        });
    if timeline.is_some_and(|value| {
        value.lifecycle() == Lifecycle::Active
            || (allow_deprecated && value.lifecycle() == Lifecycle::Deprecated)
    }) {
        Ok(())
    } else {
        Err(FactManagementError::InvalidCandidate(
            "validity requires an Active registered Timeline",
        ))
    }
}

fn record_created_revision(record: &Record) -> Option<Revision> {
    match record {
        Record::Assertion(value) => Some(value.created_revision()),
        Record::AssertionRetraction(value) => Some(value.created_revision()),
        Record::Mask(value) => Some(value.created_revision()),
        Record::MaskRetraction(value) => Some(value.created_revision()),
        Record::ReplacementBoundary(value) => Some(value.created_revision()),
        Record::ReplacementBoundaryRetraction(value) => Some(value.created_revision()),
        Record::Event(value) => Some(value.created_revision()),
        Record::EventRetraction(value) => Some(value.created_revision()),
        Record::Provenance(value) => Some(value.created_revision()),
        Record::ArchiveTransition(value) => Some(value.created_revision()),
        _ => None,
    }
}

fn fact_record_ref(record: &Record) -> Option<RecordRef> {
    Some(match record {
        Record::Assertion(value) => RecordRef::Assertion(value.id()),
        Record::AssertionRetraction(value) => RecordRef::AssertionRetraction(value.id()),
        Record::Mask(value) => RecordRef::Mask(value.id()),
        Record::MaskRetraction(value) => RecordRef::MaskRetraction(value.id()),
        Record::ReplacementBoundary(value) => RecordRef::ReplacementBoundary(value.id()),
        Record::ReplacementBoundaryRetraction(value) => {
            RecordRef::ReplacementBoundaryRetraction(value.id())
        }
        Record::Event(value) => RecordRef::Event(value.id()),
        Record::EventRetraction(value) => RecordRef::EventRetraction(value.id()),
        Record::Provenance(value) => RecordRef::Provenance(value.id()),
        Record::ArchiveTransition(value) => RecordRef::ArchiveTransition(value.id()),
        _ => return None,
    })
}

fn archive_target_record_ref(target: ArchiveTargetRef) -> Option<RecordRef> {
    Some(match target {
        ArchiveTargetRef::Assertion(id) => RecordRef::Assertion(id),
        ArchiveTargetRef::Mask(id) => RecordRef::Mask(id),
        ArchiveTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ArchiveTargetRef::Event(id) => RecordRef::Event(id),
        _ => return None,
    })
}

fn map_schema_error(error: SchemaManagementError) -> FactManagementError {
    match error {
        SchemaManagementError::Unauthorized(capability) => {
            FactManagementError::Unauthorized(capability)
        }
        SchemaManagementError::Conflict => FactManagementError::Conflict,
        SchemaManagementError::OperationAlreadyUsed => FactManagementError::OperationAlreadyUsed,
        SchemaManagementError::UnknownCommit(operation_id) => {
            FactManagementError::UnknownCommit(operation_id)
        }
        SchemaManagementError::UnknownCommitWithDiagnostic(operation_id, diagnostic) => {
            FactManagementError::UnknownCommitWithDiagnostic(operation_id, diagnostic)
        }
        other => FactManagementError::Storage(other.to_string()),
    }
}

fn storage_error(error: impl fmt::Display) -> FactManagementError {
    FactManagementError::Storage(error.to_string())
}

/// Factual-record read, validation, authorization, or publication failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FactManagementError {
    /// The current host Principal lacks the named permission.
    Unauthorized(Capability),
    /// The live database head differs from the expected base revision.
    Conflict,
    /// A requested historical revision is not committed.
    RevisionNotPublished,
    /// A typed input or reference violates the selected schema/catalog.
    Validation(String),
    /// The pinned resolution preview could not be completed safely.
    Query(String),
    /// A selected typed record, context, or schema definition is unavailable.
    InvalidCandidate(&'static str),
    /// The stable typed record identity already exists.
    DuplicateIdentity,
    /// The supplied OperationId has already been used.
    OperationAlreadyUsed,
    /// The supplied OperationId is committed with a different correction payload.
    IdempotencyMismatch,
    /// The record's Transaction-Time revision is not the next commit revision.
    InvalidRecordRevision,
    /// Commit outcome cannot be proved.
    UnknownCommit(OperationId),
    /// Commit outcome is unknown with recovery diagnostics.
    UnknownCommitWithDiagnostic(OperationId, String),
    /// Storage, recovery, audit, or referenced history failed closed.
    Storage(String),
}

impl fmt::Display for FactManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized(capability) => {
                write!(formatter, "missing required capability {capability:?}")
            }
            Self::Conflict => {
                formatter.write_str("database changed; reload factual data before retrying")
            }
            Self::RevisionNotPublished => {
                formatter.write_str("requested revision is not committed")
            }
            Self::Validation(reason) => {
                write!(formatter, "factual record validation failed: {reason}")
            }
            Self::Query(reason) => write!(formatter, "resolution preview failed: {reason}"),
            Self::InvalidCandidate(reason) => formatter.write_str(reason),
            Self::DuplicateIdentity => {
                formatter.write_str("factual record identity already exists")
            }
            Self::OperationAlreadyUsed => formatter.write_str("OperationId is already used"),
            Self::IdempotencyMismatch => {
                formatter.write_str("OperationId is committed with a different payload")
            }
            Self::InvalidRecordRevision => {
                formatter.write_str("factual record does not use the next shared revision")
            }
            Self::UnknownCommit(operation_id) => write!(
                formatter,
                "factual commit outcome is unknown for {operation_id}"
            ),
            Self::UnknownCommitWithDiagnostic(operation_id, reason) => write!(
                formatter,
                "factual commit outcome is unknown for {operation_id}: {reason}"
            ),
            Self::Storage(reason) => {
                write!(formatter, "factual storage operation failed: {reason}")
            }
        }
    }
}

impl std::error::Error for FactManagementError {}

#[cfg(test)]
mod tests {
    use worlddb_core::{
        ArchiveAction, ArchiveTargetRef, AssertionDraft, AssertionValidity, AuditAction,
        AuditCommitContext, AuditObjectClass, AuditPolicyFingerprint, AuditRecord,
        AuditRecordDetails, AuditRecordIdentity, AuditSequence, Bytes, Capability, Cardinality,
        ConstraintSet, ContextKey, EntityTypeConstraint, EntityTypeDefinition, EntityTypeId,
        EventDraft, EventKindDefinition, EventKindId, EventTime, EventTimeConstraint,
        EventTimeForm, LayerDefinition, LayerId, LayerSchemaSnapshot, Lifecycle, MaskSelector,
        OperationId, Polarity, PredicateDefinition, PredicateDefinitionSpec, PredicateId, Record,
        RecordRef, ResolutionPolicy, Revision, SchemaMode, SchemaRevision, SecurityPolicyVersion,
        Subject, Symbol, TimeInterval, Timeline, TimelineCalendarProfile, TimelineDefinition,
        TimelineId, Value, ValueKind, WorldTime,
    };

    use crate::{
        DatabaseLayout, FileEntityManager, FileSchemaManager, HistorySegmentStore,
        ManifestSegmentKind, ManifestSegmentReference, ManifestStore, RecoveryManager,
        SecurityPolicyHistoryStore, StorageVerifier, WalPrepareLog, WriterLock,
    };

    use super::super::tests::{TempArea, create_project, id};
    use super::{FactManagementError, FileFactManager as Manager};

    struct Fixture {
        _area: TempArea,
        layout: DatabaseLayout,
        lock: WriterLock,
        principal: worlddb_core::PrincipalId,
        history_space_id: worlddb_core::HistorySpaceId,
        layer_id: LayerId,
        predicate_id: PredicateId,
        timeline_id: TimelineId,
        event_kind_id: EventKindId,
        entity_id: worlddb_core::EntityId,
    }

    fn capabilities(
        include_assertion_create: bool,
        include_query_resolve: bool,
        include_assertion_correction: bool,
    ) -> Vec<Capability> {
        let mut values = vec![
            Capability::SchemaRead,
            Capability::SchemaManage,
            Capability::HistorySpaceRead,
            Capability::LayerRead,
            Capability::LayerWrite,
            Capability::EntityRead,
            Capability::EntityCreate,
            Capability::EntityReference,
            Capability::PerspectiveRead,
            Capability::PerspectiveUse,
            Capability::FieldRead,
            Capability::FieldWrite,
            Capability::AssertionRead,
            Capability::MaskCreate,
            Capability::MaskRead,
            Capability::ReplacementBoundaryCreate,
            Capability::ReplacementBoundaryRead,
        ];
        if include_assertion_create {
            values.push(Capability::AssertionCreate);
        }
        if include_query_resolve {
            values.push(Capability::QueryResolve);
        }
        if include_assertion_correction {
            values.extend([
                Capability::AssertionCorrect,
                Capability::AssertionRetract,
                Capability::EventRead,
                Capability::EventCreate,
                Capability::EventCorrect,
                Capability::EventRetract,
                Capability::MaskRetract,
                Capability::ReplacementBoundaryRetract,
                Capability::ProvenanceCreate,
                Capability::LifecycleRead,
                Capability::Archive,
                Capability::Unarchive,
                Capability::RelationshipCreate,
            ]);
        }
        values
    }

    fn build_fixture(
        include_assertion_create: bool,
        include_query_resolve: bool,
    ) -> Result<Fixture, String> {
        build_fixture_with_correction_rights(include_assertion_create, include_query_resolve, false)
    }

    fn build_fixture_with_correction_rights(
        include_assertion_create: bool,
        include_query_resolve: bool,
        include_assertion_correction: bool,
    ) -> Result<Fixture, String> {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(
            &root,
            &capabilities(
                include_assertion_create,
                include_query_resolve,
                include_assertion_correction,
            ),
        )?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let (history_space_id, layer_id) = seed_initial_metadata(&layout, &lock, principal)?;

        let entity_type_id = id::<EntityTypeId>(101)?;
        let predicate_id = id::<PredicateId>(102)?;
        let timeline_id = id::<TimelineId>(103)?;
        let event_kind_id = id::<EventKindId>(112)?;
        let mut schema = FileSchemaManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let entity_type = EntityTypeDefinition::new(
            entity_type_id,
            Symbol::new("person").map_err(|error| error.to_string())?,
            None,
            Lifecycle::Active,
            Revision::GENESIS,
        );
        let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol: Symbol::new("name").map_err(|error| error.to_string())?,
            subject_constraint: EntityTypeConstraint::Exact(entity_type_id),
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy: ResolutionPolicy::MultiValueReplace,
            constraints: ConstraintSet::new(vec![]).map_err(|error| error.to_string())?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: Revision::GENESIS,
        })
        .map_err(|error| error.to_string())?;
        let timeline = TimelineDefinition::new(
            timeline_id,
            Symbol::new("world_clock").map_err(|error| error.to_string())?,
            TimelineCalendarProfile::None,
            Lifecycle::Active,
            Revision::GENESIS,
        );
        let event_kind = EventKindDefinition::new(
            event_kind_id,
            Symbol::new("happening").map_err(|error| error.to_string())?,
            vec![],
            vec![],
            EventTimeConstraint::new(EventTimeForm::InstantOrSpan, None)
                .map_err(|error| error.to_string())?,
            Lifecycle::Active,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
        let schema_receipt = schema
            .publish(
                schema.revision(),
                id::<OperationId>(104)?,
                vec![
                    Record::EntityTypeDefinition(entity_type),
                    Record::PredicateDefinition(predicate),
                    Record::TimelineDefinition(timeline),
                    Record::EventKindDefinition(event_kind),
                ],
            )
            .map_err(|error| error.to_string())?;
        let entity_id = id::<worlddb_core::EntityId>(105)?;
        FileEntityManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?
            .create(
                schema_receipt.revision(),
                id::<OperationId>(106)?,
                entity_id,
                entity_type_id,
                false,
            )
            .map_err(|error| error.to_string())?;

        Ok(Fixture {
            _area: area,
            layout,
            lock,
            principal,
            history_space_id,
            layer_id,
            predicate_id,
            timeline_id,
            event_kind_id,
            entity_id,
        })
    }

    fn seed_initial_metadata(
        layout: &DatabaseLayout,
        lock: &WriterLock,
        principal: worlddb_core::PrincipalId,
    ) -> Result<(worlddb_core::HistorySpaceId, LayerId), String> {
        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| String::from("project manifest is missing"))?;
        let policy_ids = manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
            .map(|reference| reference.id())
            .collect::<Vec<_>>();
        let policy_history = SecurityPolicyHistoryStore::new(layout.clone())
            .load_history(manifest.revision(), &policy_ids)
            .map_err(|error| error.to_string())?;
        let policy_version = policy_history
            .policy()
            .latest_version()
            .map_err(|error| error.to_string())?
            .clone();
        let revision = manifest
            .revision()
            .next_commit()
            .map_err(|error| error.to_string())?;
        let history_space_id = id::<worlddb_core::HistorySpaceId>(107)?;
        let layer_id = id::<LayerId>(108)?;
        let history_space =
            worlddb_core::HistorySpaceDefinition::new(history_space_id, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?;
        let base_layer = LayerDefinition::new(
            layer_id,
            Symbol::new("base").map_err(|error| error.to_string())?,
            None,
            0,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(revision),
        );
        let layers = LayerSchemaSnapshot::new(
            SchemaRevision::from_published_revision(revision),
            vec![base_layer.clone()],
            layer_id,
        )
        .map_err(|error| error.to_string())?;
        let records = vec![
            Record::HistorySpaceDefinition(history_space),
            Record::LayerDefinition(base_layer),
            Record::LayerSchemaSnapshot(layers),
        ];
        let history_store = HistorySegmentStore::new(layout.clone());
        let history_receipt = history_store
            .stage_segment(lock, &records)
            .map_err(|error| error.to_string())?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            revision,
        );
        let policy_store = SecurityPolicyHistoryStore::new(layout.clone());
        let next_version = SecurityPolicyVersion::new(
            revision,
            policy_version.epoch(),
            policy_version.snapshot().clone(),
        );
        let retention = policy_history
            .audit_retention_at(manifest.revision())
            .map_err(|error| error.to_string())?;
        let policy_receipt = policy_store
            .stage_version(lock, &next_version, None, retention)
            .map_err(|error| error.to_string())?;
        let policy_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            policy_receipt.id(),
            policy_receipt.content_digest(),
            revision,
        );
        let operation_id = id::<OperationId>(109)?;
        let audit = AuditRecord::new(
            AuditRecordIdentity {
                record_id: id::<worlddb_core::AuditRecordId>(110)?,
                sequence: AuditSequence::new(2),
                audit_operation_id: id::<worlddb_core::AuditOperationId>(111)?,
            },
            AuditRecordDetails {
                actor: principal,
                action: AuditAction::SchemaManagement,
                object_class: AuditObjectClass::SchemaDefinition,
                outcome: worlddb_core::AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision,
                    operation_id,
                },
                security_epoch: policy_version.epoch(),
                policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(
                    policy_version
                        .snapshot()
                        .effective_capability_fingerprint(
                            principal,
                            worlddb_core::PolicyTarget::default(),
                        )
                        .to_vec(),
                ))
                .map_err(|error| error.to_string())?,
            },
        );
        let mut references = manifest.segments().to_vec();
        references.extend([history_reference, policy_reference]);
        WalPrepareLog::new(layout)
            .commit_audited_manifest_snapshot(
                lock,
                operation_id,
                references,
                &[history_reference, policy_reference],
                &audit,
            )
            .map_err(|error| error.to_string())?;
        let recovered = RecoveryManager::new(layout.clone())
            .recover(lock)
            .map_err(|error| error.to_string())?;
        if !recovered.report().is_clean() {
            return Err("test metadata bootstrap requires recovery".to_owned());
        }
        Ok((history_space_id, layer_id))
    }

    fn make_context(fixture: &Fixture) -> Result<ContextKey, String> {
        ContextKey::new(
            fixture.history_space_id,
            fixture.layer_id,
            worlddb_core::PerspectiveScope::World,
            worlddb_core::EpistemicMode::WorldState,
        )
        .map_err(|error| error.to_string())
    }

    fn event_draft(
        fixture: &Fixture,
        manager: &Manager<'_>,
        nanoseconds: i128,
    ) -> Result<EventDraft, String> {
        let schema_manager =
            FileSchemaManager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
                .map_err(|error| error.to_string())?;
        let schema = schema_manager
            .schema_at(SchemaMode::Current, manager.revision())
            .map_err(|error| error.to_string())?;
        let kind = schema
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                worlddb_core::SchemaDefinition::EventKind(value)
                    if value.event_kind_id() == fixture.event_kind_id =>
                {
                    Some(value)
                }
                _ => None,
            })
            .ok_or_else(|| "test EventKind disappeared".to_owned())?;
        EventDraft::new(
            fixture.history_space_id,
            fixture.layer_id,
            kind,
            vec![],
            vec![],
            EventTime::Instant(WorldTime::from_nanoseconds(
                Timeline::new(fixture.timeline_id),
                nanoseconds,
            )),
        )
        .map_err(|error| error.to_string())
    }

    fn publish_test_event(
        fixture: &Fixture,
        manager: &mut Manager<'_>,
        event_id: worlddb_core::EventId,
        operation_id: OperationId,
        nanoseconds: i128,
    ) -> Result<super::FactPublicationReceipt, String> {
        let revision = manager.next_revision().map_err(|error| error.to_string())?;
        let event = worlddb_core::Event::new(
            event_id,
            event_draft(fixture, manager, nanoseconds)?,
            revision,
        )
        .map_err(|error| error.to_string())?;
        let target = worlddb_core::PolicyTarget::new(
            Some(fixture.history_space_id),
            Some(fixture.layer_id),
            Some(RecordRef::Event(event_id)),
            None,
            None,
        );
        manager
            .publish(
                manager.revision(),
                operation_id,
                Record::Event(event),
                RecordRef::Event(event_id),
                target,
            )
            .map_err(|error| error.to_string())
    }

    #[test]
    fn assertion_correction_commits_replacement_retraction_and_corrects_together()
    -> Result<(), String> {
        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let context = make_context(&fixture)?;
        let target_id = id::<worlddb_core::AssertionId>(151)?;
        let target = manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(150)?,
                target_id,
                AssertionDraft::new(
                    context,
                    Subject::new(fixture.entity_id),
                    fixture.predicate_id,
                    Value::String("Original".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        let base = target.revision();
        let operation_id = id::<OperationId>(152)?;
        let replacement_id = id::<worlddb_core::AssertionId>(154)?;
        let retraction_id = id::<worlddb_core::AssertionRetractionId>(155)?;
        let corrects_id = id::<worlddb_core::ProvenanceId>(156)?;
        let replacement = AssertionDraft::new(
            context,
            Subject::new(fixture.entity_id),
            fixture.predicate_id,
            Value::String("Replacement".to_owned()),
            Polarity::Negative,
            validity(&fixture)?,
        );
        let retraction_reason = "corrected the original value".to_owned();
        let receipt = manager
            .correct_assertion(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                retraction_id,
                corrects_id,
                replacement.clone(),
                retraction_reason.clone(),
                false,
            )
            .map_err(|error| error.to_string())?;
        let replay = manager
            .correct_assertion(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                retraction_id,
                corrects_id,
                replacement.clone(),
                retraction_reason.clone(),
                false,
            )
            .map_err(|error| error.to_string())?;
        if replay != receipt
            || manager
                .operation_status(operation_id)
                .map_err(|error| error.to_string())?
                != super::FactOperationStatus::Committed(receipt.revision())
        {
            return Err(
                "same OperationId and payload did not return the original correction receipt"
                    .into(),
            );
        }
        let changed_payload = AssertionDraft::new(
            context,
            Subject::new(fixture.entity_id),
            fixture.predicate_id,
            Value::String("Changed retry payload".to_owned()),
            Polarity::Negative,
            validity(&fixture)?,
        );
        if !matches!(
            manager.correct_assertion(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                retraction_id,
                corrects_id,
                changed_payload,
                retraction_reason,
                false,
            ),
            Err(FactManagementError::IdempotencyMismatch)
        ) {
            return Err("a changed payload reused the original correction OperationId".into());
        }
        if receipt.revision() != base.next_commit().map_err(|error| error.to_string())?
            || receipt.target() != target_id
            || manager.revision() != receipt.revision()
        {
            return Err(
                "correction receipt did not bind its target and one shared revision".into(),
            );
        }

        let manifest = ManifestStore::new(fixture.layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "manifest disappeared after correction".to_owned())?;
        let history_store = HistorySegmentStore::new(fixture.layout.clone());
        let correction_segment = manifest
            .segments()
            .iter()
            .find(|reference| {
                reference.kind() == ManifestSegmentKind::History
                    && reference.through_revision() == receipt.revision()
            })
            .ok_or_else(|| "correction history segment is missing".to_owned())?;
        let segment = history_store
            .read_segment(correction_segment.id())
            .map_err(|error| error.to_string())?;
        let records = segment
            .records()
            .iter()
            .map(|decoded| decoded.record())
            .collect::<Vec<_>>();
        if records.len() != 3
            || !records.iter().any(|record| {
                matches!(record, Record::Assertion(value) if value.id() == receipt.replacement())
            })
            || !records.iter().any(|record| {
                matches!(record, Record::AssertionRetraction(value)
                    if value.id() == receipt.retraction() && value.assertion_id() == receipt.target())
            })
            || !records.iter().any(|record| {
                matches!(record, Record::Provenance(value)
                    if value.id() == receipt.corrects()
                        && value.from() == worlddb_core::ProvenanceEndpointRef::Assertion(receipt.replacement())
                        && value.to() == worlddb_core::ProvenanceEndpointRef::Assertion(receipt.target()))
            })
        {
            return Err("correction did not publish exactly its three-record effect".into());
        }
        let audit_records = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?;
        if audit_records
            .iter()
            .filter(|entry| {
                entry.operation_id() == receipt.operation_id()
                    && entry.revision() == receipt.revision()
            })
            .count()
            != 1
        {
            return Err("correction did not commit one matching Required Audit record".into());
        }

        drop(manager);
        let reopened = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let snapshot = reopened
            .snapshot_at(receipt.revision())
            .map_err(|error| error.to_string())?;
        if snapshot.assertions().len() != 2
            || snapshot.assertion_retractions().len() != 1
            || snapshot
                .assertion_retractions()
                .first()
                .is_none_or(|retraction| retraction.assertion_id() != receipt.target())
        {
            return Err(
                "correction records did not survive reopen with their explicit Retraction".into(),
            );
        }
        let report = StorageVerifier::new(fixture.layout.clone())
            .verify(&fixture.lock)
            .map_err(|error| error.to_string())?;
        if !report.is_clean() {
            return Err("corrected database did not verify cleanly".into());
        }
        Ok(())
    }

    #[test]
    fn event_correction_is_two_records_and_keeps_retraction_separate() -> Result<(), String> {
        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let target_id = id::<worlddb_core::EventId>(158)?;
        let target = publish_test_event(
            &fixture,
            &mut manager,
            target_id,
            id::<OperationId>(157)?,
            10,
        )?;
        let base = target.revision();
        let replacement_id = id::<worlddb_core::EventId>(159)?;
        let corrects_id = id::<worlddb_core::ProvenanceId>(160)?;
        let replacement = event_draft(&fixture, &manager, 20)?;
        let operation_id = id::<OperationId>(161)?;
        let correction = manager
            .correct_event(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                corrects_id,
                replacement.clone(),
            )
            .map_err(|error| error.to_string())?;
        let replay = manager
            .correct_event(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                corrects_id,
                replacement.clone(),
            )
            .map_err(|error| error.to_string())?;
        if replay != correction {
            return Err(
                "same Event correction OperationId did not return its original receipt".into(),
            );
        }
        if !matches!(
            manager.correct_event(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                corrects_id,
                event_draft(&fixture, &manager, 21)?,
            ),
            Err(FactManagementError::IdempotencyMismatch)
        ) {
            return Err(
                "a changed Event payload reused the original correction OperationId".into(),
            );
        }
        if correction.target() != target_id
            || correction.replacement() != replacement_id
            || correction.revision() != base.next_commit().map_err(|error| error.to_string())?
        {
            return Err("Event correction receipt did not bind the exact two-record effect".into());
        }
        let manifest = ManifestStore::new(fixture.layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "manifest disappeared after Event correction".to_owned())?;
        let reference = manifest
            .segments()
            .iter()
            .find(|segment| {
                segment.kind() == ManifestSegmentKind::History
                    && segment.through_revision() == correction.revision()
            })
            .ok_or_else(|| "Event correction segment is missing".to_owned())?;
        let segment = HistorySegmentStore::new(fixture.layout.clone())
            .read_segment(reference.id())
            .map_err(|error| error.to_string())?;
        let records = segment
            .records()
            .iter()
            .map(|decoded| decoded.record())
            .collect::<Vec<_>>();
        if records.len() != 2
            || !records.iter().any(|record| {
                matches!(record, Record::Event(value) if value.id() == replacement_id)
            })
            || !records.iter().any(|record| {
                matches!(record, Record::Provenance(value)
                    if value.id() == corrects_id
                        && value.from() == worlddb_core::ProvenanceEndpointRef::Event(replacement_id)
                        && value.to() == worlddb_core::ProvenanceEndpointRef::Event(target_id))
            })
        {
            return Err("Event correction did not publish only Event and Corrects records".into());
        }
        let snapshot = manager
            .snapshot_at(correction.revision())
            .map_err(|error| error.to_string())?;
        if snapshot.events().len() != 2 || !snapshot.event_retractions().is_empty() {
            return Err("Event correction implicitly retracted its original Event".into());
        }
        let explicit_retraction = manager
            .retract_event(
                correction.revision(),
                id::<OperationId>(162)?,
                id::<worlddb_core::EventRetractionId>(163)?,
                target_id,
                "separate explicit lifecycle decision".to_owned(),
            )
            .map_err(|error| error.to_string())?;
        let after_retraction = manager
            .snapshot_at(explicit_retraction.revision())
            .map_err(|error| error.to_string())?;
        if after_retraction.event_retractions().len() != 1
            || after_retraction
                .event_retractions()
                .first()
                .is_none_or(|retraction| retraction.event_id() != target_id)
        {
            return Err("explicit EventRetraction was not recorded as its own commit".into());
        }
        let archived = manager
            .transition_archive(
                explicit_retraction.revision(),
                id::<OperationId>(164)?,
                id::<worlddb_core::ArchiveTransitionId>(165)?,
                ArchiveTargetRef::Event(replacement_id),
                ArchiveAction::Archive,
            )
            .map_err(|error| error.to_string())?;
        let after_archive = manager
            .snapshot_at(archived.revision())
            .map_err(|error| error.to_string())?;
        if after_archive.archive_transitions().len() != 1
            || after_archive
                .archive_transitions()
                .first()
                .is_none_or(|transition| {
                    transition.target() != ArchiveTargetRef::Event(replacement_id)
                })
        {
            return Err("Event archive did not create a separate target transition".into());
        }
        Ok(())
    }

    fn validity(fixture: &Fixture) -> Result<AssertionValidity, String> {
        let interval = TimeInterval::new(Timeline::new(fixture.timeline_id), None, None)
            .map_err(|error| error.to_string())?;
        Ok(AssertionValidity::new(interval))
    }

    #[test]
    fn assertion_mask_and_boundary_commit_with_required_audit_and_survive_reopen()
    -> Result<(), String> {
        let fixture = build_fixture(true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let base = manager.revision();
        let context = make_context(&fixture)?;
        let subject = Subject::new(fixture.entity_id);
        let validity = validity(&fixture)?;
        let assertion_id = id::<worlddb_core::AssertionId>(120)?;
        let assertion = manager
            .create_assertion(
                base,
                id::<OperationId>(121)?,
                assertion_id,
                AssertionDraft::new(
                    context,
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                    validity,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        let mask = manager
            .create_mask(
                assertion.revision(),
                id::<OperationId>(122)?,
                id::<worlddb_core::MaskId>(123)?,
                context,
                MaskSelector::Proposition(worlddb_core::PropositionKey::new(
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                )),
                None,
            )
            .map_err(|error| error.to_string())?;
        let boundary = manager
            .create_replacement_boundary(
                mask.revision(),
                id::<OperationId>(124)?,
                id::<worlddb_core::ReplacementBoundaryId>(125)?,
                super::ReplacementBoundaryDraft::new(context, subject, fixture.predicate_id, None),
            )
            .map_err(|error| error.to_string())?;

        let audits = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?;
        let factual = audits
            .iter()
            .filter(|entry| entry.record().action() == AuditAction::FactualRecordWrite)
            .collect::<Vec<_>>();
        if factual.len() != 3
            || factual.iter().any(|entry| {
                entry.record().object_class() != AuditObjectClass::FactualRecord
                    || entry.record().commit_context()
                        != (AuditCommitContext::Committed {
                            revision: entry.revision(),
                            operation_id: entry.operation_id(),
                        })
            })
            || !factual.iter().any(|entry| {
                entry.revision() == assertion.revision()
                    && entry.operation_id() == assertion.operation_id()
            })
            || !factual.iter().any(|entry| {
                entry.revision() == mask.revision() && entry.operation_id() == mask.operation_id()
            })
            || !factual.iter().any(|entry| {
                entry.revision() == boundary.revision()
                    && entry.operation_id() == boundary.operation_id()
            })
        {
            return Err(
                "factual records and Required Audit did not share their commits".to_owned(),
            );
        }

        drop(manager);
        let reopened = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let snapshot = reopened
            .snapshot_at(boundary.revision())
            .map_err(|error| error.to_string())?;
        let assertion_matches = snapshot.assertions().first().is_some_and(|assertion| {
            assertion.id() == assertion_id
                && matches!(assertion.value(), Value::String(value) if value == "Alice")
        });
        if snapshot.assertions().len() != 1
            || snapshot.masks().len() != 1
            || snapshot.replacement_boundaries().len() != 1
            || !assertion_matches
        {
            return Err("factual records did not survive a manager reopen".to_owned());
        }
        let report = StorageVerifier::new(fixture.layout.clone())
            .verify(&fixture.lock)
            .map_err(|error| error.to_string())?;
        if !report.is_clean() {
            return Err("factual-record commits did not verify cleanly".to_owned());
        }
        Ok(())
    }

    #[test]
    fn invalid_and_unauthorized_assertions_leave_revision_and_audit_unchanged() -> Result<(), String>
    {
        let fixture = build_fixture(true, true)?;
        let context = make_context(&fixture)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let base = manager.revision();
        let audit_count = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?
            .len();
        let invalid = manager.create_assertion(
            base,
            id::<OperationId>(130)?,
            id::<worlddb_core::AssertionId>(131)?,
            AssertionDraft::new(
                context,
                Subject::new(fixture.entity_id),
                fixture.predicate_id,
                Value::Bool(true),
                Polarity::Positive,
                validity(&fixture)?,
            ),
            false,
        );
        if !matches!(invalid, Err(FactManagementError::Validation(_))) || manager.revision() != base
        {
            return Err("schema-invalid assertion was not rejected before commit".to_owned());
        }
        let after_invalid = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?
            .len();
        if after_invalid != audit_count {
            return Err("rejected assertion left an audit or data commit".to_owned());
        }

        let denied = build_fixture(false, true)?;
        let denied_context = make_context(&denied)?;
        let mut denied_manager =
            Manager::open(denied.layout.clone(), &denied.lock, denied.principal)
                .map_err(|error| error.to_string())?;
        let denied_base = denied_manager.revision();
        let denied_audit_count = WalPrepareLog::new(&denied.layout)
            .committed_required_audit_records(&denied.lock)
            .map_err(|error| error.to_string())?
            .len();
        let unauthorized = denied_manager.create_assertion(
            denied_base,
            id::<OperationId>(132)?,
            id::<worlddb_core::AssertionId>(133)?,
            AssertionDraft::new(
                denied_context,
                Subject::new(denied.entity_id),
                denied.predicate_id,
                Value::String("Bob".to_owned()),
                Polarity::Positive,
                validity(&denied)?,
            ),
            false,
        );
        if !matches!(
            unauthorized,
            Err(FactManagementError::Unauthorized(
                Capability::AssertionCreate
            ))
        ) || denied_manager.revision() != denied_base
        {
            return Err("assertion without AssertionCreate was not rejected".to_owned());
        }
        let denied_after = WalPrepareLog::new(&denied.layout)
            .committed_required_audit_records(&denied.lock)
            .map_err(|error| error.to_string())?
            .len();
        if denied_after != denied_audit_count {
            return Err("unauthorized assertion left an audit or data commit".to_owned());
        }
        Ok(())
    }

    #[test]
    fn resolution_preview_uses_persisted_facts_and_requires_query_resolve() -> Result<(), String> {
        let fixture = build_fixture(true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let context = make_context(&fixture)?;
        let subject = Subject::new(fixture.entity_id);
        manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(140)?,
                id::<worlddb_core::AssertionId>(141)?,
                AssertionDraft::new(
                    context,
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;

        let preview = manager
            .resolution_preview(super::FactResolutionPreviewRequest {
                history_space_id: fixture.history_space_id,
                layer_selection: worlddb_core::LayerSelection::BaseOnly,
                subject,
                predicate_id: fixture.predicate_id,
                perspective_scope: worlddb_core::PerspectiveScope::World,
                epistemic_mode: worlddb_core::EpistemicMode::WorldState,
                world_time: worlddb_core::WorldTimeSelector::AllTimes,
            })
            .map_err(|error| error.to_string())?;
        if !matches!(
            preview.query().value(),
            worlddb_core::ResolutionPreview::AllTimes { slices } if !slices.is_empty()
        ) {
            return Err(format!(
                "the all-times preview did not resolve the persisted Assertion: {:?}",
                preview.query().value()
            ));
        }

        let denied = build_fixture(true, false)?;
        let denied_manager = Manager::open(denied.layout.clone(), &denied.lock, denied.principal)
            .map_err(|error| error.to_string())?;
        let denied = denied_manager.resolution_preview(super::FactResolutionPreviewRequest {
            history_space_id: denied.history_space_id,
            layer_selection: worlddb_core::LayerSelection::BaseOnly,
            subject: Subject::new(denied.entity_id),
            predicate_id: denied.predicate_id,
            perspective_scope: worlddb_core::PerspectiveScope::World,
            epistemic_mode: worlddb_core::EpistemicMode::WorldState,
            world_time: worlddb_core::WorldTimeSelector::AllTimes,
        });
        if !matches!(
            denied,
            Err(FactManagementError::Unauthorized(Capability::QueryResolve))
        ) {
            return Err("resolution preview without QueryResolve was not rejected".to_owned());
        }
        Ok(())
    }
}
