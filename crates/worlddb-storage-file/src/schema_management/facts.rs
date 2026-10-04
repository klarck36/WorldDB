//! Authorized, schema-checked, WAL-backed Assertion, Mask, and Boundary writes.

use std::fmt;

use worlddb_core::{
    Assertion, AssertionDraft, AssertionId, AssertionValidity, AuditAction, AuditCommitContext,
    AuditObjectClass, AuditOutcome, AuditPolicyFingerprint, AuditRecord, AuditRecordDetails,
    AuditRecordIdentity, AuthorizationDecision, Bytes, Capability, ContextKey,
    EntityTypeConstraint, FieldSelector, Lifecycle, Mask, MaskId, MaskSelector, OperationId,
    PolicyTarget, PredicateId, Record, RecordRef, ReplacementBoundary, ReplacementBoundaryId,
    Revision, SchemaDefinition, SecurityPolicyVersion, Subject, Value, WriteReferenceSnapshot,
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
    masks: Vec<Mask>,
    replacement_boundaries: Vec<ReplacementBoundary>,
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

    /// Immutable Mask records visible at this revision.
    #[must_use]
    pub fn masks(&self) -> &[Mask] {
        &self.masks
    }

    /// Immutable ReplacementBoundary records visible at this revision.
    #[must_use]
    pub fn replacement_boundaries(&self) -> &[ReplacementBoundary] {
        &self.replacement_boundaries
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

    /// Reads factual record families at one committed shared revision.
    pub fn snapshot_at(&self, revision: Revision) -> Result<FactSnapshot, FactManagementError> {
        if revision > self.revision() {
            return Err(FactManagementError::RevisionNotPublished);
        }
        let records = self.records_at_revision(revision)?;
        let mut assertions = Vec::new();
        let mut masks = Vec::new();
        let mut replacement_boundaries = Vec::new();
        for record in records {
            match record {
                Record::Assertion(value) => {
                    if self.may_read(Capability::AssertionRead, context_target(value.context()))? {
                        assertions.push(value);
                    }
                }
                Record::Mask(value) => {
                    if self.may_read(Capability::MaskRead, context_target(value.context()))? {
                        masks.push(value);
                    }
                }
                Record::ReplacementBoundary(value) => {
                    if self.may_read(
                        Capability::ReplacementBoundaryRead,
                        context_target(value.context()),
                    )? {
                        replacement_boundaries.push(value);
                    }
                }
                _ => {}
            }
        }
        Ok(FactSnapshot {
            revision,
            assertions,
            masks,
            replacement_boundaries,
        })
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
        let duplicate = self.records_at_revision(self.revision())?.iter().any(|record| {
            matches!(
                (identity, record),
                (RecordRef::Assertion(id), Record::Assertion(value)) if id == value.id()
            ) || matches!(
                (identity, record),
                (RecordRef::Mask(id), Record::Mask(value)) if id == value.id()
            ) || matches!(
                (identity, record),
                (RecordRef::ReplacementBoundary(id), Record::ReplacementBoundary(value)) if id == value.id()
            )
        });
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
                let created = match record {
                    Record::Assertion(value) => Some(value.created_revision()),
                    Record::Mask(value) => Some(value.created_revision()),
                    Record::ReplacementBoundary(value) => Some(value.created_revision()),
                    _ => None,
                };
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

    fn publish(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        record: Record,
        record_ref: RecordRef,
        policy_target: PolicyTarget,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
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
        if record_created_revision(&record) != Some(target_revision) {
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
            .stage_segment(self.schema.writer_lock, &[record])
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
    PolicyTarget::new(
        Some(context.history_space_id()),
        Some(context.layer_id()),
        None,
        None,
        None,
    )
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
        Record::Mask(value) => Some(value.created_revision()),
        Record::ReplacementBoundary(value) => Some(value.created_revision()),
        _ => None,
    }
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
    /// A selected typed record, context, or schema definition is unavailable.
    InvalidCandidate(&'static str),
    /// The stable typed record identity already exists.
    DuplicateIdentity,
    /// The supplied OperationId has already been used.
    OperationAlreadyUsed,
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
            Self::InvalidCandidate(reason) => formatter.write_str(reason),
            Self::DuplicateIdentity => {
                formatter.write_str("factual record identity already exists")
            }
            Self::OperationAlreadyUsed => formatter.write_str("OperationId is already used"),
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
        AssertionDraft, AssertionValidity, AuditAction, AuditCommitContext, AuditObjectClass,
        AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity,
        AuditSequence, Bytes, Capability, Cardinality, ConstraintSet, ContextKey,
        EntityTypeConstraint, EntityTypeDefinition, EntityTypeId, LayerDefinition, LayerId,
        LayerSchemaSnapshot, Lifecycle, MaskSelector, OperationId, Polarity, PredicateDefinition,
        PredicateDefinitionSpec, PredicateId, Record, ResolutionPolicy, Revision, SchemaRevision,
        SecurityPolicyVersion, Subject, Symbol, TimeInterval, Timeline, TimelineCalendarProfile,
        TimelineDefinition, TimelineId, Value, ValueKind,
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
        entity_id: worlddb_core::EntityId,
    }

    fn capabilities(include_assertion_create: bool) -> Vec<Capability> {
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
        values
    }

    fn build_fixture(include_assertion_create: bool) -> Result<Fixture, String> {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(&root, &capabilities(include_assertion_create))?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let (history_space_id, layer_id) = seed_initial_metadata(&layout, &lock, principal)?;

        let entity_type_id = id::<EntityTypeId>(101)?;
        let predicate_id = id::<PredicateId>(102)?;
        let timeline_id = id::<TimelineId>(103)?;
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
        let schema_receipt = schema
            .publish(
                schema.revision(),
                id::<OperationId>(104)?,
                vec![
                    Record::EntityTypeDefinition(entity_type),
                    Record::PredicateDefinition(predicate),
                    Record::TimelineDefinition(timeline),
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

    fn validity(fixture: &Fixture) -> Result<AssertionValidity, String> {
        let interval = TimeInterval::new(Timeline::new(fixture.timeline_id), None, None)
            .map_err(|error| error.to_string())?;
        Ok(AssertionValidity::new(interval))
    }

    #[test]
    fn assertion_mask_and_boundary_commit_with_required_audit_and_survive_reopen()
    -> Result<(), String> {
        let fixture = build_fixture(true)?;
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
        let fixture = build_fixture(true)?;
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

        let denied = build_fixture(false)?;
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
}
