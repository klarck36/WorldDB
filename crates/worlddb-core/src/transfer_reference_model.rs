//! Index-free reference model for explicit HistorySpace content transfer.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

use crate::archive::{ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition};
use crate::catalog::{HistorySpaceDefinition, HistorySpaceError};
use crate::event_relations::{
    EventRelation, EventRelationInputKind, EventRelationKey, EventRelationKind,
    EventRelationRetraction,
};
use crate::history_model::{HistorySpaceModelError, HistorySpaceReferenceModel};
use crate::ids::{
    ArchiveTransitionId, EventId, EventRelationId, EventRelationRetractionId, HistorySpaceId,
    ProvenanceId, Revision, RevisionError, TransferLineageId,
};
use crate::record_refs::RecordRef;
use crate::source_provenance::{ProvenanceEdge, ProvenanceEndpointRef, ProvenanceRelation};
use crate::transfer_model::{
    ExternalReferenceDecision, HistorySpaceContentRef, TransferLifecyclePolicy, TransferLineage,
    TransferPlan,
};

/// One immutable generic content item used by the transfer reference model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferRecord<T> {
    id: HistorySpaceContentRef,
    payload: T,
    references: Vec<RecordRef>,
    archived: bool,
    effective_lifecycle: bool,
}

impl<T> TransferRecord<T> {
    /// Creates one record with typed identity, domain payload, and explicit references.
    #[must_use]
    pub const fn new(id: HistorySpaceContentRef, payload: T, references: Vec<RecordRef>) -> Self {
        Self {
            id,
            payload,
            references,
            archived: false,
            effective_lifecycle: false,
        }
    }

    /// Sets the source record's operational archive state at the pinned snapshot.
    #[must_use]
    pub const fn with_archived(mut self, archived: bool) -> Self {
        self.archived = archived;
        self
    }

    /// Marks a lifecycle record as effective at the source snapshot.
    #[must_use]
    pub const fn with_effective_lifecycle(mut self, effective: bool) -> Self {
        self.effective_lifecycle = effective;
        self
    }

    /// Returns the immutable typed record identity.
    #[must_use]
    pub const fn id(&self) -> HistorySpaceContentRef {
        self.id
    }

    /// Returns the domain payload without imposing a value ordering.
    #[must_use]
    pub const fn payload(&self) -> &T {
        &self.payload
    }

    /// Returns the record's explicit typed references.
    #[must_use]
    pub fn references(&self) -> &[RecordRef] {
        &self.references
    }

    /// Returns the source record's operational archive state.
    #[must_use]
    pub const fn is_archived(&self) -> bool {
        self.archived
    }
}

/// Committed transfer result; IDs and lineage are returned only after publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferReceipt<T> {
    revision: Revision,
    records: Vec<TransferRecord<T>>,
    record_id_map: BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef>,
    lineage: Vec<ProvenanceEdge>,
    transfer_lineage: Vec<TransferLineage>,
    archive_transitions: Vec<ArchiveTransition>,
    event_relations: Vec<EventRelation>,
    event_relation_retractions: Vec<EventRelationRetraction>,
    equal_payload_pairs: Vec<(HistorySpaceContentRef, HistorySpaceContentRef)>,
}

/// Read-only lifecycle and archive differences bound to one immutable transfer plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPreview {
    plan_fingerprint: [u8; 32],
    omitted_effective_lifecycle_records: Vec<(HistorySpaceContentRef, HistorySpaceContentRef)>,
    omitted_event_relation_retractions: Vec<EventRelationId>,
    archive_targets: Vec<ArchiveTargetRef>,
}

impl TransferPreview {
    /// Returns the plan identity this preview was computed from.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> [u8; 32] {
        self.plan_fingerprint
    }

    /// Effective target-local lifecycle records omitted by the chosen policy.
    #[must_use]
    pub fn omitted_effective_lifecycle_records(
        &self,
    ) -> &[(HistorySpaceContentRef, HistorySpaceContentRef)] {
        &self.omitted_effective_lifecycle_records
    }

    /// Selected project-wide relations whose effective retractions are omitted.
    #[must_use]
    pub fn omitted_event_relation_retractions(&self) -> &[EventRelationId] {
        &self.omitted_event_relation_retractions
    }

    /// Target identities that will receive an Archive transition under the plan.
    #[must_use]
    pub fn archive_targets(&self) -> &[ArchiveTargetRef] {
        &self.archive_targets
    }

    /// Acknowledges exactly these previewed differences for a later commit.
    #[must_use]
    pub fn acknowledge_lifecycle_omissions(&self) -> TransferPreviewAcknowledgement {
        TransferPreviewAcknowledgement {
            preview: self.clone(),
        }
    }
}

/// Unforgeable-by-construction acknowledgement of a specific transfer preview.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPreviewAcknowledgement {
    preview: TransferPreview,
}

impl<T> TransferReceipt<T> {
    /// Returns the single shared revision that published this transfer.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Returns the newly copied target-local records.
    #[must_use]
    pub fn records(&self) -> &[TransferRecord<T>] {
        &self.records
    }

    /// Returns the complete typed source-to-copy identity map.
    #[must_use]
    pub const fn record_id_map(&self) -> &BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef> {
        &self.record_id_map
    }

    /// Returns one `DerivedFrom(source, copy)` lineage edge per copied record.
    #[must_use]
    pub fn lineage(&self) -> &[ProvenanceEdge] {
        &self.lineage
    }

    /// Returns dedicated persistent lineage records for excluded source families.
    #[must_use]
    pub fn transfer_lineage(&self) -> &[TransferLineage] {
        &self.transfer_lineage
    }

    /// Returns archive transitions published by the transfer policy.
    #[must_use]
    pub fn archive_transitions(&self) -> &[ArchiveTransition] {
        &self.archive_transitions
    }

    /// Returns the selected project-wide EventRelations copied by this transfer.
    #[must_use]
    pub fn event_relations(&self) -> &[EventRelation] {
        &self.event_relations
    }

    /// Returns copied effective EventRelation retractions.
    #[must_use]
    pub fn event_relation_retractions(&self) -> &[EventRelationRetraction] {
        &self.event_relation_retractions
    }

    /// Returns equal-payload identity pairs without deduplicating them.
    ///
    /// These pairs remain distinct records. Whether the query resolver reports
    /// `Conflict` is a later projection decision, not an identity-map rule.
    #[must_use]
    pub fn equal_payload_pairs(&self) -> &[(HistorySpaceContentRef, HistorySpaceContentRef)] {
        &self.equal_payload_pairs
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScopedTransferRecord<T> {
    history_space_id: HistorySpaceId,
    record: TransferRecord<T>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegisteredEventRelation {
    relation: EventRelation,
    retraction: Option<EventRelationRetraction>,
    archived: bool,
}

/// Slow, index-free same-database model of explicit HistorySpace transfer.
///
/// Every accepted transfer is published as one revision. All validation and
/// lineage construction happens before publication, so failures have no partial
/// target effects. Project-wide references can be registered as fixture state
/// for the reference model; production visibility remains an engine concern.
#[derive(Clone)]
pub struct TransferReferenceModel<T> {
    database_id: crate::ids::DatabaseId,
    history: HistorySpaceReferenceModel<ScopedTransferRecord<T>>,
    history_space_heads: BTreeMap<HistorySpaceId, Revision>,
    known_content_ids: BTreeSet<HistorySpaceContentRef>,
    known_provenance_ids: BTreeSet<ProvenanceId>,
    known_transfer_lineage_ids: BTreeSet<TransferLineageId>,
    known_archive_transition_ids: BTreeSet<ArchiveTransitionId>,
    event_relations: BTreeMap<EventRelationId, RegisteredEventRelation>,
    known_event_relation_ids: BTreeSet<EventRelationId>,
    known_event_relation_retraction_ids: BTreeSet<EventRelationRetractionId>,
    project_wide_references: HashSet<RecordRef>,
    lineage: Vec<ProvenanceEdge>,
    transfer_lineage: Vec<TransferLineage>,
}

impl<T: Clone + Eq> TransferReferenceModel<T> {
    /// Creates a single-database transfer model from an initial HistorySpace forest.
    pub fn new(
        database_id: crate::ids::DatabaseId,
        definitions: Vec<HistorySpaceDefinition>,
    ) -> Result<Self, TransferReferenceModelError> {
        let history = HistorySpaceReferenceModel::new(definitions)?;
        let history_space_heads = history
            .catalog()
            .definitions()
            .iter()
            .map(|definition| (definition.history_space_id(), definition.base_revision()))
            .collect();
        Ok(Self {
            database_id,
            history,
            history_space_heads,
            known_content_ids: BTreeSet::new(),
            known_provenance_ids: BTreeSet::new(),
            known_transfer_lineage_ids: BTreeSet::new(),
            known_archive_transition_ids: BTreeSet::new(),
            event_relations: BTreeMap::new(),
            known_event_relation_ids: BTreeSet::new(),
            known_event_relation_retraction_ids: BTreeSet::new(),
            project_wide_references: HashSet::new(),
            lineage: Vec::new(),
            transfer_lineage: Vec::new(),
        })
    }

    /// Returns the shared latest published revision.
    #[must_use]
    pub const fn latest_published(&self) -> Revision {
        self.history.latest_published()
    }

    /// Returns the model's database identity.
    #[must_use]
    pub const fn database_id(&self) -> crate::ids::DatabaseId {
        self.database_id
    }

    /// Returns the target-local head, independent of later parent/sibling commits.
    pub fn history_space_head(
        &self,
        history_space_id: HistorySpaceId,
    ) -> Result<Revision, TransferReferenceModelError> {
        self.history_space_heads
            .get(&history_space_id)
            .copied()
            .ok_or(TransferReferenceModelError::UnknownHistorySpace)
    }

    /// Adds an existing child with a fixed parent cutoff.
    pub fn add_history_space(
        &mut self,
        definition: HistorySpaceDefinition,
    ) -> Result<(), TransferReferenceModelError> {
        let id = definition.history_space_id();
        let head = definition.base_revision();
        self.history.add_history_space(definition)?;
        self.history_space_heads.insert(id, head);
        Ok(())
    }

    /// Adds immutable fixture references that are project-wide and visible in each space.
    pub fn register_project_wide_reference(&mut self, record_ref: RecordRef) -> bool {
        self.project_wide_references.insert(record_ref)
    }

    /// Registers an already-published project-wide relation as reference-model fixture state.
    pub fn register_event_relation(
        &mut self,
        relation: EventRelation,
        retraction: Option<EventRelationRetraction>,
        archived: bool,
    ) -> Result<(), TransferReferenceModelError> {
        if relation.created_revision() > self.latest_published() {
            return Err(
                TransferReferenceModelError::EventRelationRevisionNotPublished {
                    requested: relation.created_revision(),
                    published: self.latest_published(),
                },
            );
        }
        if self.known_event_relation_ids.contains(&relation.id()) {
            return Err(
                TransferReferenceModelError::EventRelationIdentityCollision {
                    identity: relation.id(),
                },
            );
        }
        if let Some(retraction) = &retraction {
            if retraction.event_relation_id() != relation.id() {
                return Err(
                    TransferReferenceModelError::EventRelationRetractionTargetMismatch {
                        expected: relation.id(),
                        actual: retraction.event_relation_id(),
                    },
                );
            }
            if retraction.created_revision() <= relation.created_revision() {
                return Err(TransferReferenceModelError::EventRelationRetractionNotAfterTarget);
            }
            if retraction.created_revision() > self.latest_published() {
                return Err(
                    TransferReferenceModelError::EventRelationRevisionNotPublished {
                        requested: retraction.created_revision(),
                        published: self.latest_published(),
                    },
                );
            }
            if self
                .known_event_relation_retraction_ids
                .contains(&retraction.id())
            {
                return Err(
                    TransferReferenceModelError::EventRelationRetractionIdentityCollision {
                        identity: retraction.id(),
                    },
                );
            }
            self.known_event_relation_retraction_ids
                .insert(retraction.id());
        }
        self.known_event_relation_ids.insert(relation.id());
        self.event_relations.insert(
            relation.id(),
            RegisteredEventRelation {
                relation,
                retraction,
                archived,
            },
        );
        self.project_wide_references
            .insert(RecordRef::EventRelation(relation.id()));
        Ok(())
    }

    /// Publishes one ordinary space-local batch for model setup or intervening writes.
    pub fn publish(
        &mut self,
        history_space_id: HistorySpaceId,
        records: Vec<TransferRecord<T>>,
    ) -> Result<Revision, TransferReferenceModelError> {
        let mut incoming = BTreeSet::new();
        for record in &records {
            if !incoming.insert(record.id) || self.known_content_ids.contains(&record.id) {
                return Err(TransferReferenceModelError::ContentIdentityCollision {
                    identity: record.id,
                });
            }
        }

        let revision = self.history.publish(
            history_space_id,
            records
                .into_iter()
                .map(|record| ScopedTransferRecord {
                    history_space_id,
                    record,
                })
                .collect(),
        )?;
        self.known_content_ids.extend(incoming);
        self.history_space_heads.insert(history_space_id, revision);
        Ok(revision)
    }

    /// Reads the selected space and fixed parent ancestry at a published revision.
    pub fn read_at(
        &self,
        history_space_id: HistorySpaceId,
        as_of: Revision,
    ) -> Result<Vec<(Revision, TransferRecord<T>)>, TransferReferenceModelError> {
        Ok(self
            .history
            .read_at(history_space_id, as_of)?
            .into_iter()
            .map(|(revision, _, entry)| (revision, entry.record.clone()))
            .collect())
    }

    /// Previews effective lifecycle omissions and archive transitions without publishing.
    pub fn preview_transfer(
        &self,
        plan: &TransferPlan,
    ) -> Result<TransferPreview, TransferReferenceModelError> {
        if plan.database_id() != self.database_id {
            return Err(TransferReferenceModelError::DatabaseMismatch);
        }
        let target_space = plan.target_history_space_id();
        let actual_target_head = self.history_space_head(target_space)?;
        if actual_target_head != plan.expected_target_head() {
            return Err(TransferReferenceModelError::TargetHeadConflict {
                expected: plan.expected_target_head(),
                actual: actual_target_head,
            });
        }

        let visible = self.read_at(plan.source_history_space_id(), plan.source_recorded_as_of())?;
        let visible_records: BTreeMap<_, _> = visible
            .iter()
            .map(|(_, record)| (record.id, record))
            .collect();
        for source in plan.selected_records() {
            if !visible_records.contains_key(source) {
                return Err(TransferReferenceModelError::SourceRecordNotVisible {
                    identity: *source,
                });
            }
        }

        let mut effective_lifecycle_records =
            BTreeMap::<HistorySpaceContentRef, Vec<HistorySpaceContentRef>>::new();
        for (_, lifecycle_record) in &visible {
            if lifecycle_record.effective_lifecycle && is_lifecycle_record(lifecycle_record.id) {
                for reference in &lifecycle_record.references {
                    if let Ok(target) = HistorySpaceContentRef::try_from(*reference) {
                        if plan.selected_records().contains(&target) {
                            effective_lifecycle_records
                                .entry(target)
                                .or_default()
                                .push(lifecycle_record.id);
                        }
                    }
                }
            }
        }
        let mut omitted_effective_lifecycle_records = Vec::new();
        for (target, lifecycle_records) in effective_lifecycle_records {
            for lifecycle_record in lifecycle_records {
                if !plan.selected_records().contains(&lifecycle_record) {
                    if plan.lifecycle_policy() == TransferLifecyclePolicy::CopyEffectiveLifecycle {
                        return Err(
                            TransferReferenceModelError::MissingEffectiveLifecycleRecord {
                                target,
                                lifecycle_record,
                            },
                        );
                    }
                    omitted_effective_lifecycle_records.push((target, lifecycle_record));
                }
            }
        }

        let mut omitted_event_relation_retractions = Vec::new();
        let mut archive_targets = Vec::new();
        for source in plan.selected_records() {
            if visible_records
                .get(source)
                .is_some_and(|record| record.archived)
                && matches!(
                    plan.archive_policy(),
                    crate::transfer_model::TransferArchivePolicy::PreserveArchiveState
                )
            {
                let target = *plan.record_id_map().get(source).ok_or(
                    TransferReferenceModelError::MissingMappedReference {
                        reference: source.record_ref(),
                    },
                )?;
                archive_targets.push(archive_target(target));
            }
        }
        for relation_id in plan.selected_event_relations() {
            let relation = self.event_relations.get(relation_id).ok_or(
                TransferReferenceModelError::EventRelationNotVisible {
                    identity: *relation_id,
                },
            )?;
            if relation.relation.created_revision() > plan.source_recorded_as_of() {
                return Err(TransferReferenceModelError::EventRelationNotVisible {
                    identity: *relation_id,
                });
            }
            if relation.retraction.as_ref().is_some_and(|retraction| {
                retraction.created_revision() <= plan.source_recorded_as_of()
            }) {
                match plan.lifecycle_policy() {
                    TransferLifecyclePolicy::CopyEffectiveLifecycle => {
                        return Err(
                            TransferReferenceModelError::EffectiveEventRelationRetractionNeedsOmissionPolicy {
                                identity: *relation_id,
                            },
                        );
                    }
                    TransferLifecyclePolicy::OmitWithAcknowledgement => {
                        omitted_event_relation_retractions.push(*relation_id);
                    }
                }
            }
            if relation.archived
                && matches!(
                    plan.archive_policy(),
                    crate::transfer_model::TransferArchivePolicy::PreserveArchiveState
                )
            {
                let target = *plan.event_relation_id_map().get(relation_id).ok_or(
                    TransferReferenceModelError::EventRelationNotVisible {
                        identity: *relation_id,
                    },
                )?;
                archive_targets.push(ArchiveTargetRef::EventRelation(target));
            }
        }
        omitted_effective_lifecycle_records.sort_unstable();
        omitted_event_relation_retractions.sort_unstable();
        archive_targets.sort_unstable();

        Ok(TransferPreview {
            plan_fingerprint: plan.plan_fingerprint(),
            omitted_effective_lifecycle_records,
            omitted_event_relation_retractions,
            archive_targets,
        })
    }

    /// Returns all committed transfer-lineage edges in insertion order.
    #[must_use]
    pub fn lineage(&self) -> &[ProvenanceEdge] {
        &self.lineage
    }

    /// Returns all committed TransferLineage records in insertion order.
    #[must_use]
    pub fn transfer_lineage(&self) -> &[TransferLineage] {
        &self.transfer_lineage
    }

    /// Validates and atomically publishes one explicit transfer.
    pub fn commit_transfer(
        &mut self,
        plan: &TransferPlan,
        provenance_ids: Vec<ProvenanceId>,
        event_relation_retraction_ids: Vec<EventRelationRetractionId>,
        archive_transition_ids: Vec<ArchiveTransitionId>,
    ) -> Result<TransferReceipt<T>, TransferReferenceModelError> {
        self.commit_transfer_impl(
            plan,
            provenance_ids,
            Vec::new(),
            event_relation_retraction_ids,
            archive_transition_ids,
            None,
        )
    }

    /// Commits a transfer with dedicated records for families excluded from DerivedFrom.
    pub fn commit_transfer_with_transfer_lineage_ids(
        &mut self,
        plan: &TransferPlan,
        provenance_ids: Vec<ProvenanceId>,
        transfer_lineage_ids: Vec<TransferLineageId>,
        event_relation_retraction_ids: Vec<EventRelationRetractionId>,
        archive_transition_ids: Vec<ArchiveTransitionId>,
    ) -> Result<TransferReceipt<T>, TransferReferenceModelError> {
        self.commit_transfer_impl(
            plan,
            provenance_ids,
            transfer_lineage_ids,
            event_relation_retraction_ids,
            archive_transition_ids,
            None,
        )
    }

    /// Commits an omission-policy transfer using the exact previously reviewed preview.
    pub fn commit_transfer_acknowledged(
        &mut self,
        plan: &TransferPlan,
        provenance_ids: Vec<ProvenanceId>,
        event_relation_retraction_ids: Vec<EventRelationRetractionId>,
        archive_transition_ids: Vec<ArchiveTransitionId>,
        acknowledgement: &TransferPreviewAcknowledgement,
    ) -> Result<TransferReceipt<T>, TransferReferenceModelError> {
        self.commit_transfer_impl(
            plan,
            provenance_ids,
            Vec::new(),
            event_relation_retraction_ids,
            archive_transition_ids,
            Some(acknowledgement),
        )
    }

    /// Acknowledged commit variant that also publishes dedicated transfer lineages.
    pub fn commit_transfer_acknowledged_with_transfer_lineage_ids(
        &mut self,
        plan: &TransferPlan,
        provenance_ids: Vec<ProvenanceId>,
        transfer_lineage_ids: Vec<TransferLineageId>,
        event_relation_retraction_ids: Vec<EventRelationRetractionId>,
        archive_transition_ids: Vec<ArchiveTransitionId>,
        acknowledgement: &TransferPreviewAcknowledgement,
    ) -> Result<TransferReceipt<T>, TransferReferenceModelError> {
        self.commit_transfer_impl(
            plan,
            provenance_ids,
            transfer_lineage_ids,
            event_relation_retraction_ids,
            archive_transition_ids,
            Some(acknowledgement),
        )
    }

    fn commit_transfer_impl(
        &mut self,
        plan: &TransferPlan,
        provenance_ids: Vec<ProvenanceId>,
        transfer_lineage_ids: Vec<TransferLineageId>,
        event_relation_retraction_ids: Vec<EventRelationRetractionId>,
        archive_transition_ids: Vec<ArchiveTransitionId>,
        acknowledgement: Option<&TransferPreviewAcknowledgement>,
    ) -> Result<TransferReceipt<T>, TransferReferenceModelError> {
        if plan.database_id() != self.database_id {
            return Err(TransferReferenceModelError::DatabaseMismatch);
        }
        let preview = self.preview_transfer(plan)?;
        if plan.lifecycle_policy() == TransferLifecyclePolicy::OmitWithAcknowledgement {
            if acknowledgement.map(|value| &value.preview) != Some(&preview) {
                return Err(TransferReferenceModelError::LifecycleOmissionAcknowledgementMismatch);
            }
        } else if acknowledgement.is_some() {
            return Err(TransferReferenceModelError::UnexpectedLifecycleAcknowledgement);
        }
        let source_space = plan.source_history_space_id();
        let target_space = plan.target_history_space_id();
        let actual_target_head = self.history_space_head(target_space)?;
        if actual_target_head != plan.expected_target_head() {
            return Err(TransferReferenceModelError::TargetHeadConflict {
                expected: plan.expected_target_head(),
                actual: actual_target_head,
            });
        }

        let expected_provenance_count = plan
            .selected_records()
            .iter()
            .filter(|source| allows_derived_from(**source))
            .count();
        if provenance_ids.len() != expected_provenance_count {
            return Err(TransferReferenceModelError::LineageIdCount {
                expected: expected_provenance_count,
                actual: provenance_ids.len(),
            });
        }
        let mut new_provenance_ids = BTreeSet::new();
        for provenance_id in &provenance_ids {
            if !new_provenance_ids.insert(*provenance_id) {
                return Err(TransferReferenceModelError::DuplicateLineageIdentity {
                    identity: *provenance_id,
                });
            }
            if self.known_provenance_ids.contains(provenance_id) {
                return Err(TransferReferenceModelError::LineageIdentityCollision {
                    identity: *provenance_id,
                });
            }
        }

        let expected_transfer_lineage_count =
            plan.selected_records().len() - expected_provenance_count;
        if transfer_lineage_ids.len() != expected_transfer_lineage_count {
            return Err(TransferReferenceModelError::TransferLineageIdCount {
                expected: expected_transfer_lineage_count,
                actual: transfer_lineage_ids.len(),
            });
        }
        let mut new_transfer_lineage_ids = BTreeSet::new();
        for lineage_id in &transfer_lineage_ids {
            if !new_transfer_lineage_ids.insert(*lineage_id) {
                return Err(
                    TransferReferenceModelError::DuplicateTransferLineageIdentity {
                        identity: *lineage_id,
                    },
                );
            }
            if self.known_transfer_lineage_ids.contains(lineage_id) {
                return Err(
                    TransferReferenceModelError::TransferLineageIdentityCollision {
                        identity: *lineage_id,
                    },
                );
            }
        }

        let latest = self.history.latest_published();
        let source_visible = self
            .history
            .read_at(source_space, plan.source_recorded_as_of())?;
        let source_refs: HashSet<RecordRef> = source_visible
            .iter()
            .map(|(_, _, scoped)| scoped.record.id.record_ref())
            .chain(self.project_wide_references.iter().copied())
            .collect();
        let mut selected = BTreeMap::new();
        let mut effective_lifecycle_targets =
            BTreeMap::<HistorySpaceContentRef, Vec<HistorySpaceContentRef>>::new();
        for (_, _, scoped) in source_visible {
            if scoped.record.effective_lifecycle && is_lifecycle_record(scoped.record.id) {
                for reference in &scoped.record.references {
                    if let Ok(target) = HistorySpaceContentRef::try_from(*reference) {
                        effective_lifecycle_targets
                            .entry(target)
                            .or_default()
                            .push(scoped.record.id);
                    }
                }
            }
            if plan.selected_records().contains(&scoped.record.id)
                && selected
                    .insert(scoped.record.id, scoped.record.clone())
                    .is_some()
            {
                return Err(TransferReferenceModelError::DuplicateVisibleSourceIdentity);
            }
        }
        for source in plan.selected_records() {
            if !selected.contains_key(source) {
                return Err(TransferReferenceModelError::SourceRecordNotVisible {
                    identity: *source,
                });
            }
        }

        let mut selected_relations = BTreeMap::new();
        for relation_id in plan.selected_event_relations() {
            let registered = self.event_relations.get(relation_id).ok_or(
                TransferReferenceModelError::EventRelationNotVisible {
                    identity: *relation_id,
                },
            )?;
            if registered.relation.created_revision() > plan.source_recorded_as_of() {
                return Err(TransferReferenceModelError::EventRelationNotVisible {
                    identity: *relation_id,
                });
            }
            selected_relations.insert(*relation_id, registered.clone());
        }
        let mut new_relation_ids = BTreeSet::new();
        for target in plan.event_relation_id_map().values() {
            if !new_relation_ids.insert(*target) || self.known_event_relation_ids.contains(target) {
                return Err(
                    TransferReferenceModelError::EventRelationIdentityCollision {
                        identity: *target,
                    },
                );
            }
        }
        for (relation_id, registered) in &selected_relations {
            for endpoint in [
                registered.relation.from_event(),
                registered.relation.to_event(),
            ] {
                if !source_refs.contains(&RecordRef::Event(endpoint)) {
                    return Err(
                        TransferReferenceModelError::EventRelationEndpointNotVisibleAtSource {
                            relation: *relation_id,
                            event: endpoint,
                        },
                    );
                }
            }
        }
        let effective_relation_retractions =
            if plan.lifecycle_policy() == TransferLifecyclePolicy::CopyEffectiveLifecycle {
                selected_relations
                    .values()
                    .filter_map(|registered| {
                        registered.retraction.as_ref().filter(|retraction| {
                            retraction.created_revision() <= plan.source_recorded_as_of()
                        })
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
        if event_relation_retraction_ids.len() != effective_relation_retractions.len() {
            return Err(
                TransferReferenceModelError::EventRelationRetractionIdCount {
                    expected: effective_relation_retractions.len(),
                    actual: event_relation_retraction_ids.len(),
                },
            );
        }
        let mut new_relation_retraction_ids = BTreeSet::new();
        for retraction_id in &event_relation_retraction_ids {
            if !new_relation_retraction_ids.insert(*retraction_id) {
                return Err(
                    TransferReferenceModelError::DuplicateEventRelationRetractionIdentity {
                        identity: *retraction_id,
                    },
                );
            }
            if self
                .known_event_relation_retraction_ids
                .contains(retraction_id)
            {
                return Err(
                    TransferReferenceModelError::EventRelationRetractionIdentityCollision {
                        identity: *retraction_id,
                    },
                );
            }
        }
        if plan.lifecycle_policy() == TransferLifecyclePolicy::CopyEffectiveLifecycle {
            for source in plan.selected_records() {
                if let Some(effective_lifecycle_records) = effective_lifecycle_targets.get(source) {
                    for lifecycle_record in effective_lifecycle_records {
                        if !plan.selected_records().contains(lifecycle_record) {
                            return Err(
                                TransferReferenceModelError::MissingEffectiveLifecycleRecord {
                                    target: *source,
                                    lifecycle_record: *lifecycle_record,
                                },
                            );
                        }
                    }
                }
            }
        }

        let expected_archive_count = match plan.archive_policy() {
            crate::transfer_model::TransferArchivePolicy::PreserveArchiveState => {
                selected.values().filter(|record| record.archived).count()
                    + selected_relations
                        .values()
                        .filter(|relation| relation.archived)
                        .count()
            }
            crate::transfer_model::TransferArchivePolicy::StartUnarchived => 0,
        };
        if expected_archive_count > 0 {
            return Err(
                TransferReferenceModelError::ArchivePreservationRequiresLaterRevision {
                    archived_targets: expected_archive_count,
                },
            );
        }
        if archive_transition_ids.len() != expected_archive_count {
            return Err(TransferReferenceModelError::ArchiveTransitionIdCount {
                expected: expected_archive_count,
                actual: archive_transition_ids.len(),
            });
        }
        let mut new_archive_transition_ids = BTreeSet::new();
        for archive_id in &archive_transition_ids {
            if !new_archive_transition_ids.insert(*archive_id) {
                return Err(
                    TransferReferenceModelError::DuplicateArchiveTransitionIdentity {
                        identity: *archive_id,
                    },
                );
            }
            if self.known_archive_transition_ids.contains(archive_id) {
                return Err(
                    TransferReferenceModelError::ArchiveTransitionIdentityCollision {
                        identity: *archive_id,
                    },
                );
            }
        }

        let mut new_content_ids = BTreeSet::new();
        for target in plan.record_id_map().values() {
            if !new_content_ids.insert(*target) || self.known_content_ids.contains(target) {
                return Err(TransferReferenceModelError::ContentIdentityCollision {
                    identity: *target,
                });
            }
        }

        let target_visible = self.history.read_at(target_space, latest)?;
        let target_refs: HashSet<RecordRef> = target_visible
            .iter()
            .map(|(_, _, scoped)| scoped.record.id.record_ref())
            .chain(self.project_wide_references.iter().copied())
            .collect();
        let mut target_refs = target_refs;
        target_refs.extend(
            plan.record_id_map()
                .values()
                .filter_map(|mapped| match mapped {
                    HistorySpaceContentRef::Event(event_id) => Some(RecordRef::Event(*event_id)),
                    _ => None,
                }),
        );

        let mut staged_records = Vec::with_capacity(plan.selected_records().len());
        for source_id in plan.selected_records() {
            let source = selected.get(source_id).ok_or(
                TransferReferenceModelError::SourceRecordNotVisible {
                    identity: *source_id,
                },
            )?;
            let mut references = Vec::with_capacity(source.references.len());
            for reference in &source.references {
                if let Ok(content_ref) = HistorySpaceContentRef::try_from(*reference) {
                    if plan.selected_records().contains(&content_ref) {
                        let mapped = plan.record_id_map().get(&content_ref).ok_or(
                            TransferReferenceModelError::MissingMappedReference {
                                reference: *reference,
                            },
                        )?;
                        references.push(mapped.record_ref());
                        continue;
                    }
                }
                match plan.external_reference_decision(*reference) {
                    None => {
                        return Err(
                            TransferReferenceModelError::MissingExternalReferenceDecision {
                                reference: *reference,
                            },
                        );
                    }
                    Some(ExternalReferenceDecision::Reject) => {
                        return Err(TransferReferenceModelError::ExternalReferenceRejected {
                            reference: *reference,
                        });
                    }
                    Some(ExternalReferenceDecision::RetainVisible) => {
                        if !target_refs.contains(reference) {
                            return Err(TransferReferenceModelError::ExternalReferenceNotVisible {
                                reference: *reference,
                            });
                        }
                        references.push(*reference);
                    }
                }
            }
            let target_id = *plan.record_id_map().get(source_id).ok_or(
                TransferReferenceModelError::MissingMappedReference {
                    reference: source_id.record_ref(),
                },
            )?;
            staged_records.push(TransferRecord {
                id: target_id,
                payload: source.payload.clone(),
                references,
                archived: matches!(
                    plan.archive_policy(),
                    crate::transfer_model::TransferArchivePolicy::PreserveArchiveState
                ) && source.archived,
                effective_lifecycle: source.effective_lifecycle,
            });
        }

        let next_revision = latest.next_commit()?;
        let mut staged_event_relations = Vec::with_capacity(selected_relations.len());
        let mut staged_relation_keys = BTreeSet::new();
        for (source_relation_id, registered) in &selected_relations {
            let target_relation_id = *plan.event_relation_id_map().get(source_relation_id).ok_or(
                TransferReferenceModelError::EventRelationNotVisible {
                    identity: *source_relation_id,
                },
            )?;
            let from_event =
                map_event_endpoint(registered.relation.from_event(), plan, &target_refs)?;
            let to_event = map_event_endpoint(registered.relation.to_event(), plan, &target_refs)?;
            let input_kind = match registered.relation.kind() {
                EventRelationKind::Before => EventRelationInputKind::Before,
                EventRelationKind::SameTime => EventRelationInputKind::SameTime,
                EventRelationKind::Causes => EventRelationInputKind::Causes,
            };
            let relation = EventRelation::new(
                target_relation_id,
                from_event,
                to_event,
                input_kind,
                next_revision,
            )
            .map_err(|_| TransferReferenceModelError::InvalidEventRelation {
                source: *source_relation_id,
            })?;
            if !staged_relation_keys.insert(relation.key())
                || self.event_relations.values().any(|existing| {
                    let retracted_at_commit = existing
                        .retraction
                        .as_ref()
                        .is_some_and(|retraction| retraction.created_revision() <= latest);
                    !retracted_at_commit && existing.relation.key() == relation.key()
                })
            {
                return Err(TransferReferenceModelError::EventRelationConflict {
                    key: relation.key(),
                });
            }
            let archived = matches!(
                plan.archive_policy(),
                crate::transfer_model::TransferArchivePolicy::PreserveArchiveState
            ) && registered.archived;
            staged_event_relations.push((relation, archived));
        }

        let mut provenance_ids = provenance_ids.iter();
        let mut transfer_lineage_ids = transfer_lineage_ids.iter();
        let mut lineage = Vec::with_capacity(expected_provenance_count);
        let mut staged_transfer_lineage = Vec::with_capacity(expected_transfer_lineage_count);
        for (source, target) in plan.record_id_map() {
            if allows_derived_from(*source) {
                let provenance_id = provenance_ids.next().copied().ok_or(
                    TransferReferenceModelError::LineageIdCount {
                        expected: expected_provenance_count,
                        actual: provenance_ids.len(),
                    },
                )?;
                lineage.push(
                    ProvenanceEdge::new(
                        provenance_id,
                        provenance_endpoint(*source),
                        provenance_endpoint(*target),
                        ProvenanceRelation::DerivedFrom,
                        next_revision,
                    )
                    .map_err(|_| {
                        TransferReferenceModelError::ForbiddenDerivedFromSource { source: *source }
                    })?,
                );
            } else {
                let lineage_id = transfer_lineage_ids.next().copied().ok_or(
                    TransferReferenceModelError::TransferLineageIdCount {
                        expected: expected_transfer_lineage_count,
                        actual: transfer_lineage_ids.len(),
                    },
                )?;
                staged_transfer_lineage.push(
                    TransferLineage::new(
                        lineage_id,
                        source_space,
                        target_space,
                        *source,
                        *target,
                        next_revision,
                    )
                    .map_err(|_| {
                        TransferReferenceModelError::ForbiddenDerivedFromSource { source: *source }
                    })?,
                );
            }
        }

        let mut staged_event_relation_retractions = Vec::new();
        let mut event_relation_retraction_id_iter = event_relation_retraction_ids.iter();
        for (source_relation_id, registered) in &selected_relations {
            if let Some(source_retraction) = registered.retraction.as_ref().filter(|retraction| {
                plan.lifecycle_policy() == TransferLifecyclePolicy::CopyEffectiveLifecycle
                    && retraction.created_revision() <= plan.source_recorded_as_of()
            }) {
                let target_relation_id = *plan
                    .event_relation_id_map()
                    .get(source_relation_id)
                    .ok_or(TransferReferenceModelError::EventRelationNotVisible {
                        identity: *source_relation_id,
                    })?;
                let target_relation = staged_event_relations
                    .iter()
                    .find(|(relation, _)| relation.id() == target_relation_id)
                    .map(|(relation, _)| relation)
                    .ok_or(TransferReferenceModelError::EventRelationNotVisible {
                        identity: *source_relation_id,
                    })?;
                let retraction_id = *event_relation_retraction_id_iter.next().ok_or(
                    TransferReferenceModelError::EventRelationRetractionIdCount {
                        expected: effective_relation_retractions.len(),
                        actual: event_relation_retraction_ids.len(),
                    },
                )?;
                let retraction = EventRelationRetraction::new(
                    retraction_id,
                    target_relation,
                    source_retraction.reason().to_owned(),
                    next_revision,
                )
                .map_err(|_| TransferReferenceModelError::EventRelationRetractionNotAfterTarget)?;
                staged_event_relation_retractions.push(retraction);
            }
        }

        let mut archive_targets = staged_records
            .iter()
            .filter(|record| record.archived)
            .map(|record| archive_target(record.id))
            .collect::<Vec<_>>();
        archive_targets.extend(
            staged_event_relations
                .iter()
                .filter(|(_, archived)| *archived)
                .map(|(relation, _)| ArchiveTargetRef::EventRelation(relation.id())),
        );
        let archive_transitions = archive_targets
            .into_iter()
            .zip(archive_transition_ids.iter())
            .map(|(target, archive_id)| {
                ArchiveTransition::new(
                    *archive_id,
                    target,
                    ArchiveAction::Archive,
                    ArchiveState::Unarchived,
                    next_revision,
                )
                .map_err(|_| TransferReferenceModelError::InvalidArchiveTransition { target })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut equal_payload_pairs = Vec::new();
        for record in &staged_records {
            for (_, _, existing) in &target_visible {
                if record.payload == existing.record.payload {
                    equal_payload_pairs.push((record.id, existing.record.id));
                }
            }
        }
        for (index, left) in staged_records.iter().enumerate() {
            for right in staged_records.iter().skip(index + 1) {
                if left.payload == right.payload {
                    equal_payload_pairs.push((left.id, right.id));
                }
            }
        }

        let revision = self.publish(target_space, staged_records.clone())?;
        debug_assert_eq!(revision, next_revision);
        self.known_provenance_ids.extend(new_provenance_ids);
        self.known_transfer_lineage_ids
            .extend(new_transfer_lineage_ids);
        self.known_archive_transition_ids
            .extend(new_archive_transition_ids);
        self.known_event_relation_retraction_ids
            .extend(new_relation_retraction_ids);
        for (relation, archived) in &staged_event_relations {
            let retraction = staged_event_relation_retractions
                .iter()
                .find(|retraction| retraction.event_relation_id() == relation.id())
                .cloned();
            self.known_event_relation_ids.insert(relation.id());
            self.event_relations.insert(
                relation.id(),
                RegisteredEventRelation {
                    relation: *relation,
                    retraction,
                    archived: *archived,
                },
            );
        }
        self.lineage.extend(lineage.iter().copied());
        self.transfer_lineage
            .extend(staged_transfer_lineage.iter().copied());

        Ok(TransferReceipt {
            revision,
            records: staged_records,
            record_id_map: plan.record_id_map().clone(),
            lineage,
            transfer_lineage: staged_transfer_lineage,
            archive_transitions,
            event_relations: staged_event_relations
                .iter()
                .map(|(relation, _)| *relation)
                .collect(),
            event_relation_retractions: staged_event_relation_retractions,
            equal_payload_pairs,
        })
    }
}

fn archive_target(content_ref: HistorySpaceContentRef) -> ArchiveTargetRef {
    match content_ref {
        HistorySpaceContentRef::Assertion(id) => ArchiveTargetRef::Assertion(id),
        HistorySpaceContentRef::Mask(id) => ArchiveTargetRef::Mask(id),
        HistorySpaceContentRef::ReplacementBoundary(id) => {
            ArchiveTargetRef::ReplacementBoundary(id)
        }
        HistorySpaceContentRef::Event(id) => ArchiveTargetRef::Event(id),
        HistorySpaceContentRef::EventMask(id) => ArchiveTargetRef::EventMask(id),
        HistorySpaceContentRef::AssertionValidityClosure(id) => {
            ArchiveTargetRef::AssertionValidityClosure(id)
        }
        HistorySpaceContentRef::AssertionRetraction(id) => {
            ArchiveTargetRef::AssertionRetraction(id)
        }
        HistorySpaceContentRef::MaskValidityClosure(id) => {
            ArchiveTargetRef::MaskValidityClosure(id)
        }
        HistorySpaceContentRef::MaskRetraction(id) => ArchiveTargetRef::MaskRetraction(id),
        HistorySpaceContentRef::ReplacementBoundaryValidityClosure(id) => {
            ArchiveTargetRef::ReplacementBoundaryValidityClosure(id)
        }
        HistorySpaceContentRef::ReplacementBoundaryRetraction(id) => {
            ArchiveTargetRef::ReplacementBoundaryRetraction(id)
        }
        HistorySpaceContentRef::EventSpanClosure(id) => ArchiveTargetRef::EventSpanClosure(id),
        HistorySpaceContentRef::EventRetraction(id) => ArchiveTargetRef::EventRetraction(id),
        HistorySpaceContentRef::EventMaskRetraction(id) => {
            ArchiveTargetRef::EventMaskRetraction(id)
        }
    }
}

fn provenance_endpoint(content_ref: HistorySpaceContentRef) -> ProvenanceEndpointRef {
    match content_ref {
        HistorySpaceContentRef::Assertion(id) => ProvenanceEndpointRef::Assertion(id),
        HistorySpaceContentRef::Mask(id) => ProvenanceEndpointRef::Mask(id),
        HistorySpaceContentRef::ReplacementBoundary(id) => {
            ProvenanceEndpointRef::ReplacementBoundary(id)
        }
        HistorySpaceContentRef::Event(id) => ProvenanceEndpointRef::Event(id),
        HistorySpaceContentRef::EventMask(id) => ProvenanceEndpointRef::EventMask(id),
        HistorySpaceContentRef::AssertionValidityClosure(id) => {
            ProvenanceEndpointRef::AssertionValidityClosure(id)
        }
        HistorySpaceContentRef::AssertionRetraction(id) => {
            ProvenanceEndpointRef::AssertionRetraction(id)
        }
        HistorySpaceContentRef::MaskValidityClosure(id) => {
            ProvenanceEndpointRef::MaskValidityClosure(id)
        }
        HistorySpaceContentRef::MaskRetraction(id) => ProvenanceEndpointRef::MaskRetraction(id),
        HistorySpaceContentRef::ReplacementBoundaryValidityClosure(id) => {
            ProvenanceEndpointRef::ReplacementBoundaryValidityClosure(id)
        }
        HistorySpaceContentRef::ReplacementBoundaryRetraction(id) => {
            ProvenanceEndpointRef::ReplacementBoundaryRetraction(id)
        }
        HistorySpaceContentRef::EventSpanClosure(id) => ProvenanceEndpointRef::EventSpanClosure(id),
        HistorySpaceContentRef::EventRetraction(id) => ProvenanceEndpointRef::EventRetraction(id),
        HistorySpaceContentRef::EventMaskRetraction(id) => {
            ProvenanceEndpointRef::EventMaskRetraction(id)
        }
    }
}

fn allows_derived_from(content_ref: HistorySpaceContentRef) -> bool {
    !matches!(
        content_ref,
        HistorySpaceContentRef::Mask(_)
            | HistorySpaceContentRef::ReplacementBoundary(_)
            | HistorySpaceContentRef::EventMask(_)
    )
}

fn map_event_endpoint(
    source_event: EventId,
    plan: &TransferPlan,
    target_refs: &HashSet<RecordRef>,
) -> Result<EventId, TransferReferenceModelError> {
    let source_ref = HistorySpaceContentRef::Event(source_event);
    if plan.selected_records().contains(&source_ref) {
        return match plan.record_id_map().get(&source_ref) {
            Some(HistorySpaceContentRef::Event(target_event)) => Ok(*target_event),
            _ => Err(TransferReferenceModelError::MissingMappedReference {
                reference: RecordRef::Event(source_event),
            }),
        };
    }

    match plan.external_reference_decision(RecordRef::Event(source_event)) {
        None => Err(
            TransferReferenceModelError::MissingExternalReferenceDecision {
                reference: RecordRef::Event(source_event),
            },
        ),
        Some(ExternalReferenceDecision::Reject) => {
            Err(TransferReferenceModelError::ExternalReferenceRejected {
                reference: RecordRef::Event(source_event),
            })
        }
        Some(ExternalReferenceDecision::RetainVisible) => {
            if target_refs.contains(&RecordRef::Event(source_event)) {
                Ok(source_event)
            } else {
                Err(TransferReferenceModelError::ExternalReferenceNotVisible {
                    reference: RecordRef::Event(source_event),
                })
            }
        }
    }
}

fn is_lifecycle_record(content_ref: HistorySpaceContentRef) -> bool {
    matches!(
        content_ref,
        HistorySpaceContentRef::AssertionValidityClosure(_)
            | HistorySpaceContentRef::AssertionRetraction(_)
            | HistorySpaceContentRef::MaskValidityClosure(_)
            | HistorySpaceContentRef::MaskRetraction(_)
            | HistorySpaceContentRef::ReplacementBoundaryValidityClosure(_)
            | HistorySpaceContentRef::ReplacementBoundaryRetraction(_)
            | HistorySpaceContentRef::EventSpanClosure(_)
            | HistorySpaceContentRef::EventRetraction(_)
            | HistorySpaceContentRef::EventMaskRetraction(_)
    )
}

/// Failure to validate or atomically publish a reference-model transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferReferenceModelError {
    /// The same-database plan names another database.
    DatabaseMismatch,
    /// The selected HistorySpace does not exist.
    UnknownHistorySpace,
    /// The source snapshot does not include a selected record.
    SourceRecordNotVisible {
        /// The missing selected record.
        identity: HistorySpaceContentRef,
    },
    /// The source snapshot contains duplicate copies of one typed identity.
    DuplicateVisibleSourceIdentity,
    /// A selected base record omits an effective lifecycle record required by policy.
    MissingEffectiveLifecycleRecord {
        /// The selected base record whose current state would be changed.
        target: HistorySpaceContentRef,
        /// The effective lifecycle record that was not selected.
        lifecycle_record: HistorySpaceContentRef,
    },
    /// A fresh destination content identity is already used.
    ContentIdentityCollision {
        /// The colliding typed content identity.
        identity: HistorySpaceContentRef,
    },
    /// The destination changed since the plan was created.
    TargetHeadConflict {
        /// The target head recorded by the transfer plan.
        expected: Revision,
        /// The target head at commit validation.
        actual: Revision,
    },
    /// A selected record contains a selected reference without a map entry.
    MissingMappedReference {
        /// The source reference with no copy identity.
        reference: RecordRef,
    },
    /// A reference outside the selection has no explicit choice.
    MissingExternalReferenceDecision {
        /// The unplanned external reference.
        reference: RecordRef,
    },
    /// The plan explicitly rejected an external reference.
    ExternalReferenceRejected {
        /// The rejected reference.
        reference: RecordRef,
    },
    /// A retained external reference is not visible in the destination snapshot.
    ExternalReferenceNotVisible {
        /// The invisible reference.
        reference: RecordRef,
    },
    /// A relation or retraction fixture is newer than the shared published head.
    EventRelationRevisionNotPublished {
        /// The fixture revision that was requested.
        requested: Revision,
        /// The latest revision published in the model.
        published: Revision,
    },
    /// A selected EventRelation is absent from the source snapshot.
    EventRelationNotVisible {
        /// The selected relation identity.
        identity: EventRelationId,
    },
    /// One relation endpoint is not visible at the pinned source revision.
    EventRelationEndpointNotVisibleAtSource {
        /// The relation with the unavailable endpoint.
        relation: EventRelationId,
        /// The unavailable Event endpoint.
        event: EventId,
    },
    /// A destination EventRelation identity is already in use.
    EventRelationIdentityCollision {
        /// The colliding relation identity.
        identity: EventRelationId,
    },
    /// A selected EventRelation cannot be reconstructed with the mapped endpoints.
    InvalidEventRelation {
        /// The source relation that produced an invalid copy.
        source: EventRelationId,
    },
    /// Two copies or an existing active relation have the same canonical relation key.
    EventRelationConflict {
        /// The duplicated canonical relation key.
        key: EventRelationKey,
    },
    /// An existing EventRelationRetraction targets a different relation.
    EventRelationRetractionTargetMismatch {
        /// The relation that the fixture retraction should target.
        expected: EventRelationId,
        /// The relation that the fixture retraction actually targets.
        actual: EventRelationId,
    },
    /// An existing EventRelationRetraction does not follow its relation.
    EventRelationRetractionNotAfterTarget,
    /// A same-revision transfer cannot copy an effective relation retraction as-is.
    EffectiveEventRelationRetractionNeedsOmissionPolicy {
        /// The selected relation whose effective retraction needs explicit omission.
        identity: EventRelationId,
    },
    /// An existing EventRelationRetraction identity is already in use.
    EventRelationRetractionIdentityCollision {
        /// The colliding EventRelationRetraction identity.
        identity: EventRelationRetractionId,
    },
    /// A copied effective EventRelation needs a fresh retraction identity.
    EventRelationRetractionIdCount {
        /// Number of effective selected relation retractions.
        expected: usize,
        /// Number of supplied identities.
        actual: usize,
    },
    /// A new EventRelationRetraction identity repeats in this transfer.
    DuplicateEventRelationRetractionIdentity {
        /// The repeated EventRelationRetraction identity.
        identity: EventRelationRetractionId,
    },
    /// There is not exactly one generated Provenance ID per copied record.
    LineageIdCount {
        /// Number of records selected for transfer.
        expected: usize,
        /// Number of supplied Provenance identities.
        actual: usize,
    },
    /// There is not one dedicated TransferLineage ID per excluded source family.
    TransferLineageIdCount {
        /// The number of selected records whose source family is excluded from DerivedFrom.
        expected: usize,
        /// The number of supplied TransferLineage identities.
        actual: usize,
    },
    /// The transfer reuses one Provenance identity within its own batch.
    DuplicateLineageIdentity {
        /// The duplicated Provenance identity.
        identity: ProvenanceId,
    },
    /// The transfer repeats a TransferLineage identity within its batch.
    DuplicateTransferLineageIdentity {
        /// The duplicated TransferLineage identity.
        identity: TransferLineageId,
    },
    /// The transfer reuses a TransferLineage identity already in the model.
    TransferLineageIdentityCollision {
        /// The already-used identity.
        identity: TransferLineageId,
    },
    /// The transfer reuses a Provenance identity already present in the model.
    LineageIdentityCollision {
        /// The already-used Provenance identity.
        identity: ProvenanceId,
    },
    /// The current DerivedFrom source matrix rejects the copied source family.
    ForbiddenDerivedFromSource {
        /// The source record rejected by the closed matrix.
        source: HistorySpaceContentRef,
    },
    /// The lifecycle-omission acknowledgement differs from the current plan preview.
    LifecycleOmissionAcknowledgementMismatch,
    /// A lifecycle-omission acknowledgement was supplied for a copy-all-lifecycle plan.
    UnexpectedLifecycleAcknowledgement,
    /// There is not one ArchiveTransition ID for every archived copied record.
    ArchiveTransitionIdCount {
        /// The number of archived copied records under the selected policy.
        expected: usize,
        /// The number of supplied ArchiveTransition identities.
        actual: usize,
    },
    /// A copied target cannot be archived in the same revision that creates it.
    ArchivePreservationRequiresLaterRevision {
        /// Number of archived source targets selected for preservation.
        archived_targets: usize,
    },
    /// The transfer repeats an ArchiveTransition identity within its batch.
    DuplicateArchiveTransitionIdentity {
        /// The duplicated ArchiveTransition identity.
        identity: ArchiveTransitionId,
    },
    /// The transfer reuses an ArchiveTransition identity already known to the model.
    ArchiveTransitionIdentityCollision {
        /// The already-used ArchiveTransition identity.
        identity: ArchiveTransitionId,
    },
    /// ArchiveTransition construction rejected the planned state change.
    InvalidArchiveTransition {
        /// The copied record targeted by the invalid transition.
        target: ArchiveTargetRef,
    },
    /// Revision exhaustion prevents publication.
    RevisionExhausted(RevisionError),
    /// The HistorySpace reference model rejected a read or publication.
    History(HistorySpaceModelError),
}

impl From<HistorySpaceModelError> for TransferReferenceModelError {
    fn from(error: HistorySpaceModelError) -> Self {
        Self::History(error)
    }
}

impl From<RevisionError> for TransferReferenceModelError {
    fn from(error: RevisionError) -> Self {
        Self::RevisionExhausted(error)
    }
}

impl From<HistorySpaceError> for TransferReferenceModelError {
    fn from(error: HistorySpaceError) -> Self {
        Self::History(HistorySpaceModelError::Catalog(error))
    }
}

impl fmt::Display for TransferReferenceModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseMismatch => formatter.write_str("transfer plan names another database"),
            Self::UnknownHistorySpace => formatter.write_str("unknown transfer HistorySpace"),
            Self::SourceRecordNotVisible { identity } => {
                write!(
                    formatter,
                    "selected source record {identity:?} is not visible"
                )
            }
            Self::DuplicateVisibleSourceIdentity => {
                formatter.write_str("source snapshot has a duplicate content identity")
            }
            Self::MissingEffectiveLifecycleRecord {
                target,
                lifecycle_record,
            } => write!(
                formatter,
                "transfer of {target:?} omitted effective lifecycle record {lifecycle_record:?}"
            ),
            Self::ContentIdentityCollision { identity } => {
                write!(
                    formatter,
                    "transfer content identity collision: {identity:?}"
                )
            }
            Self::TargetHeadConflict { expected, actual } => write!(
                formatter,
                "target HistorySpace head changed from {expected} to {actual}"
            ),
            Self::MissingMappedReference { reference } => {
                write!(
                    formatter,
                    "selected reference {reference:?} has no ID mapping"
                )
            }
            Self::MissingExternalReferenceDecision { reference } => write!(
                formatter,
                "external reference {reference:?} has no explicit transfer decision"
            ),
            Self::ExternalReferenceRejected { reference } => {
                write!(formatter, "external reference {reference:?} was rejected")
            }
            Self::ExternalReferenceNotVisible { reference } => write!(
                formatter,
                "external reference {reference:?} is not visible in the target"
            ),
            Self::EventRelationRevisionNotPublished {
                requested,
                published,
            } => write!(
                formatter,
                "EventRelation fixture revision {requested} is not published through {published}"
            ),
            Self::EventRelationNotVisible { identity } => {
                write!(
                    formatter,
                    "EventRelation {identity} is not visible at the source snapshot"
                )
            }
            Self::EventRelationEndpointNotVisibleAtSource { relation, event } => write!(
                formatter,
                "EventRelation {relation} has Event endpoint {event} not visible at the source snapshot"
            ),
            Self::EventRelationIdentityCollision { identity } => {
                write!(formatter, "EventRelation identity collision: {identity}")
            }
            Self::InvalidEventRelation { source } => {
                write!(
                    formatter,
                    "EventRelation {source} cannot be copied with mapped endpoints"
                )
            }
            Self::EventRelationConflict { key } => {
                write!(formatter, "transfer EventRelation conflicts at key {key:?}")
            }
            Self::EventRelationRetractionTargetMismatch { expected, actual } => write!(
                formatter,
                "EventRelationRetraction targets {actual}; expected {expected}"
            ),
            Self::EventRelationRetractionNotAfterTarget => formatter.write_str(
                "EventRelationRetraction must follow its EventRelation on Transaction Time",
            ),
            Self::EffectiveEventRelationRetractionNeedsOmissionPolicy { identity } => write!(
                formatter,
                "effective EventRelationRetraction for {identity} cannot be copied in one transfer revision; use OmitWithAcknowledgement and review its preview",
            ),
            Self::EventRelationRetractionIdentityCollision { identity } => {
                write!(
                    formatter,
                    "EventRelationRetraction identity collision: {identity}"
                )
            }
            Self::EventRelationRetractionIdCount { expected, actual } => write!(
                formatter,
                "transfer needs {expected} EventRelationRetraction IDs but received {actual}"
            ),
            Self::DuplicateEventRelationRetractionIdentity { identity } => {
                write!(
                    formatter,
                    "duplicate transfer EventRelationRetraction ID {identity}"
                )
            }
            Self::LineageIdCount { expected, actual } => write!(
                formatter,
                "transfer needs {expected} lineage IDs but received {actual}"
            ),
            Self::TransferLineageIdCount { expected, actual } => write!(
                formatter,
                "transfer needs {expected} TransferLineage IDs but received {actual}"
            ),
            Self::DuplicateLineageIdentity { identity } => {
                write!(formatter, "duplicate transfer lineage ID {identity}")
            }
            Self::DuplicateTransferLineageIdentity { identity } => {
                write!(
                    formatter,
                    "duplicate transfer TransferLineage ID {identity}"
                )
            }
            Self::TransferLineageIdentityCollision { identity } => {
                write!(
                    formatter,
                    "transfer TransferLineage ID {identity} already exists"
                )
            }
            Self::LineageIdentityCollision { identity } => {
                write!(formatter, "transfer lineage ID {identity} already exists")
            }
            Self::ForbiddenDerivedFromSource { source } => write!(
                formatter,
                "DerivedFrom source matrix rejects transfer source {source:?}"
            ),
            Self::LifecycleOmissionAcknowledgementMismatch => formatter.write_str(
                "lifecycle-omission acknowledgement does not match the current transfer preview",
            ),
            Self::UnexpectedLifecycleAcknowledgement => formatter.write_str(
                "lifecycle-omission acknowledgement supplied for a copy-all-lifecycle transfer",
            ),
            Self::ArchiveTransitionIdCount { expected, actual } => write!(
                formatter,
                "transfer needs {expected} archive-transition IDs but received {actual}"
            ),
            Self::ArchivePreservationRequiresLaterRevision { archived_targets } => write!(
                formatter,
                "cannot preserve archive state for {archived_targets} copied targets in their creation revision; choose StartUnarchived and archive them in a later transaction"
            ),
            Self::DuplicateArchiveTransitionIdentity { identity } => {
                write!(
                    formatter,
                    "duplicate transfer ArchiveTransition ID {identity}"
                )
            }
            Self::ArchiveTransitionIdentityCollision { identity } => {
                write!(
                    formatter,
                    "transfer ArchiveTransition ID {identity} already exists"
                )
            }
            Self::InvalidArchiveTransition { target } => write!(
                formatter,
                "archive policy produced an invalid transition for {target:?}"
            ),
            Self::RevisionExhausted(error) => write!(formatter, "revision exhausted: {error}"),
            Self::History(error) => write!(formatter, "transfer history failed: {error}"),
        }
    }
}

impl std::error::Error for TransferReferenceModelError {}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt;

    use super::{TransferRecord, TransferReferenceModel, TransferReferenceModelError};
    use crate::catalog::{HistorySpaceDefinition, HistorySpaceError};
    use crate::event_relations::{EventRelation, EventRelationInputKind, EventRelationRetraction};
    use crate::ids::{
        ArchiveTransitionId, AssertionId, AssertionRetractionId, DatabaseId, DomainId, EventId,
        EventRelationId, EventRelationRetractionId, HistorySpaceId, IdValidationError,
        ProvenanceId, Revision, RevisionError, SourceId, TransferLineageId,
    };
    use crate::record_refs::RecordRef;
    use crate::transfer_model::{
        ExternalReferenceDecision, HistorySpaceContentRef, TransferLifecyclePolicy, TransferPlan,
        TransferPlanError, TransferPlanSpec,
    };

    #[derive(Debug)]
    struct TestError(String);

    macro_rules! impl_test_error_from {
        ($error:ty) => {
            impl From<$error> for TestError {
                fn from(error: $error) -> Self {
                    Self(error.to_string())
                }
            }
        };
    }

    impl_test_error_from!(IdValidationError);
    impl_test_error_from!(RevisionError);
    impl_test_error_from!(HistorySpaceError);
    impl_test_error_from!(TransferReferenceModelError);
    impl_test_error_from!(TransferPlanError);
    impl_test_error_from!(crate::event_relations::EventRelationError);

    impl From<&str> for TestError {
        fn from(error: &str) -> Self {
            Self(error.to_owned())
        }
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.0)
        }
    }

    impl std::error::Error for TestError {}

    fn id<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn plan(
        database: DatabaseId,
        source_space: HistorySpaceId,
        source_as_of: Revision,
        target_space: HistorySpaceId,
        target_head: Revision,
        source: HistorySpaceContentRef,
        target: HistorySpaceContentRef,
    ) -> Result<TransferPlan, crate::transfer_model::TransferPlanError> {
        TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: source_as_of,
            target_history_space_id: target_space,
            expected_target_head: target_head,
            selected_records: BTreeSet::from([source]),
            record_id_map: BTreeMap::from([(source, target)]),
            selected_event_relations: BTreeSet::new(),
            event_relation_id_map: BTreeMap::new(),
            external_reference_decisions: Vec::new(),
            lifecycle_policy:
                crate::transfer_model::TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: crate::transfer_model::TransferArchivePolicy::PreserveArchiveState,
        })
    }

    #[test]
    fn assertion_transfer_copies_identity_payload_and_derived_from_atomically()
    -> Result<(), TestError> {
        let database = id::<DatabaseId>(1)?;
        let source_space = id::<HistorySpaceId>(2)?;
        let target_space = id::<HistorySpaceId>(3)?;
        let sibling_space = id::<HistorySpaceId>(7)?;
        let source_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(4)?);
        let target_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(5)?);
        let lineage_id = id::<ProvenanceId>(6)?;
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root])?;
        let source_revision = model.publish(
            source_space,
            vec![TransferRecord::new(source_id, "fact".to_owned(), vec![])],
        )?;
        model.add_history_space(HistorySpaceDefinition::new(
            target_space,
            Some(source_space),
            source_revision,
        )?)?;
        model.add_history_space(HistorySpaceDefinition::new(
            sibling_space,
            Some(source_space),
            source_revision,
        )?)?;
        let expected_target_head = model.history_space_head(target_space)?;
        let transfer_plan = plan(
            database,
            source_space,
            source_revision,
            target_space,
            expected_target_head,
            source_id,
            target_id,
        )?;

        let receipt = model.commit_transfer(&transfer_plan, vec![lineage_id], vec![], vec![])?;
        assert_eq!(receipt.revision(), source_revision.next_commit()?);
        assert_eq!(
            receipt.records().first().map(TransferRecord::id),
            Some(target_id)
        );
        assert_eq!(
            receipt
                .records()
                .first()
                .map(|record| record.payload().as_str()),
            Some("fact")
        );
        assert_eq!(receipt.record_id_map().get(&source_id), Some(&target_id));
        assert_eq!(receipt.lineage().len(), 1);
        let source_endpoint = match source_id {
            HistorySpaceContentRef::Assertion(id) => crate::ProvenanceEndpointRef::Assertion(id),
            _ => return Err("test source must be an Assertion".into()),
        };
        assert_eq!(
            receipt.lineage().first().map(|edge| edge.from()),
            Some(source_endpoint)
        );
        assert_eq!(receipt.equal_payload_pairs(), &[(target_id, source_id)]);
        assert_eq!(model.read_at(source_space, receipt.revision())?.len(), 1);
        assert_eq!(model.read_at(target_space, receipt.revision())?.len(), 2);
        assert_eq!(model.read_at(sibling_space, receipt.revision())?.len(), 1);
        Ok(())
    }

    #[test]
    fn excluded_derived_from_family_uses_dedicated_transfer_lineage() -> Result<(), TestError> {
        let database = id::<DatabaseId>(11)?;
        let source_space = id::<HistorySpaceId>(12)?;
        let target_space = id::<HistorySpaceId>(13)?;
        let sibling_space = id::<HistorySpaceId>(14)?;
        let source_id = HistorySpaceContentRef::Mask(crate::MaskId::try_from_bytes({
            let mut bytes = [0_u8; 16];
            bytes[6] = 0x70;
            bytes[8] = 0x80;
            bytes[15] = 15;
            bytes
        })?);
        let target_id = HistorySpaceContentRef::Mask(crate::MaskId::try_from_bytes({
            let mut bytes = [0_u8; 16];
            bytes[6] = 0x70;
            bytes[8] = 0x80;
            bytes[15] = 16;
            bytes
        })?);
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
        let sibling = HistorySpaceDefinition::new(sibling_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, target, sibling])?;
        let source_revision = model.publish(
            source_space,
            vec![TransferRecord::new(source_id, 7_u8, vec![])],
        )?;
        model.publish(
            sibling_space,
            vec![TransferRecord::new(
                HistorySpaceContentRef::Assertion(id::<AssertionId>(17)?),
                99_u8,
                vec![],
            )],
        )?;
        let transfer_plan = plan(
            database,
            source_space,
            source_revision,
            target_space,
            model.history_space_head(target_space)?,
            source_id,
            target_id,
        )?;
        let receipt = model.commit_transfer_with_transfer_lineage_ids(
            &transfer_plan,
            vec![],
            vec![id::<TransferLineageId>(18)?],
            vec![],
            vec![],
        )?;
        assert_eq!(receipt.lineage().len(), 0);
        assert_eq!(receipt.transfer_lineage().len(), 1);
        let Some(lineage) = receipt.transfer_lineage().first() else {
            return Err("TransferLineage should be present".into());
        };
        assert_eq!(lineage.source(), source_id);
        assert_eq!(lineage.target(), target_id);
        let target_before = model.read_at(target_space, model.latest_published())?;
        let sibling_before = model.read_at(sibling_space, model.latest_published())?;
        assert_eq!(
            model.read_at(target_space, receipt.revision())?,
            target_before
        );
        assert_eq!(
            model.read_at(sibling_space, receipt.revision())?,
            sibling_before
        );
        Ok(())
    }

    #[test]
    fn all_three_excluded_source_families_commit_only_transfer_lineage() -> Result<(), TestError> {
        use crate::ids::{EventMaskId, MaskId, ReplacementBoundaryId};

        let cases = [
            (
                HistorySpaceContentRef::Mask(id::<MaskId>(31)?),
                HistorySpaceContentRef::Mask(id::<MaskId>(32)?),
            ),
            (
                HistorySpaceContentRef::ReplacementBoundary(id::<ReplacementBoundaryId>(33)?),
                HistorySpaceContentRef::ReplacementBoundary(id::<ReplacementBoundaryId>(34)?),
            ),
            (
                HistorySpaceContentRef::EventMask(id::<EventMaskId>(35)?),
                HistorySpaceContentRef::EventMask(id::<EventMaskId>(36)?),
            ),
        ];
        for (index, (source_id, target_id)) in cases.into_iter().enumerate() {
            let offset =
                u8::try_from(index * 3).map_err(|_| TestError("case index overflow".into()))?;
            let database = id::<DatabaseId>(40 + offset)?;
            let source_space = id::<HistorySpaceId>(50 + offset)?;
            let target_space = id::<HistorySpaceId>(51 + offset)?;
            let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
            let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
            let mut model = TransferReferenceModel::new(database, vec![root, target])?;
            let source_revision = model.publish(
                source_space,
                vec![TransferRecord::new(source_id, offset, vec![])],
            )?;
            let transfer_plan = plan(
                database,
                source_space,
                source_revision,
                target_space,
                model.history_space_head(target_space)?,
                source_id,
                target_id,
            )?;
            let receipt = model.commit_transfer_with_transfer_lineage_ids(
                &transfer_plan,
                vec![],
                vec![id::<TransferLineageId>(60 + offset)?],
                vec![],
                vec![],
            )?;
            assert!(receipt.lineage().is_empty());
            assert_eq!(receipt.transfer_lineage().len(), 1);
            let Some(lineage) = receipt.transfer_lineage().first() else {
                return Err("TransferLineage should be present".into());
            };
            assert_eq!(lineage.source(), source_id);
            assert_eq!(lineage.target(), target_id);
            assert_eq!(lineage.source_history_space_id(), source_space);
            assert_eq!(lineage.target_history_space_id(), target_space);
            assert_eq!(lineage.created_revision(), receipt.revision());
        }
        Ok(())
    }

    #[test]
    fn external_references_need_an_explicit_visible_retention_choice() -> Result<(), TestError> {
        let database = id::<DatabaseId>(21)?;
        let source_space = id::<HistorySpaceId>(22)?;
        let target_space = id::<HistorySpaceId>(23)?;
        let source_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(24)?);
        let target_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(25)?);
        let external = RecordRef::Source(id::<SourceId>(26)?);
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, target])?;
        assert!(model.register_project_wide_reference(external));
        let source_revision = model.publish(
            source_space,
            vec![TransferRecord::new(source_id, 1_u8, vec![external])],
        )?;
        let transfer_plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: source_revision,
            target_history_space_id: target_space,
            expected_target_head: model.history_space_head(target_space)?,
            selected_records: BTreeSet::from([source_id]),
            record_id_map: BTreeMap::from([(source_id, target_id)]),
            selected_event_relations: BTreeSet::new(),
            event_relation_id_map: BTreeMap::new(),
            external_reference_decisions: vec![(
                external,
                ExternalReferenceDecision::RetainVisible,
            )],
            lifecycle_policy:
                crate::transfer_model::TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })?;
        let receipt = model.commit_transfer(
            &transfer_plan,
            vec![id::<ProvenanceId>(27)?],
            vec![],
            vec![],
        )?;
        assert_eq!(
            receipt.records().first().map(TransferRecord::references),
            Some(&[external][..])
        );
        Ok(())
    }

    #[test]
    fn stale_target_head_fails_before_any_target_write() -> Result<(), TestError> {
        let database = id::<DatabaseId>(31)?;
        let source_space = id::<HistorySpaceId>(32)?;
        let target_space = id::<HistorySpaceId>(33)?;
        let source_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(34)?);
        let target_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(35)?);
        let intervening_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(36)?);
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, target])?;
        let source_revision = model.publish(
            source_space,
            vec![TransferRecord::new(source_id, 5_u8, vec![])],
        )?;
        let stale_plan = plan(
            database,
            source_space,
            source_revision,
            target_space,
            Revision::GENESIS,
            source_id,
            target_id,
        )?;
        model.publish(
            target_space,
            vec![TransferRecord::new(intervening_id, 9_u8, vec![])],
        )?;
        let before = model.read_at(target_space, model.latest_published())?;
        let actual_head = model.history_space_head(target_space)?;

        assert_eq!(
            model.commit_transfer(&stale_plan, vec![id::<ProvenanceId>(37)?], vec![], vec![]),
            Err(TransferReferenceModelError::TargetHeadConflict {
                expected: Revision::GENESIS,
                actual: actual_head,
            })
        );
        assert_eq!(
            model.read_at(target_space, model.latest_published())?,
            before
        );
        Ok(())
    }

    #[test]
    fn archive_policy_rejects_same_revision_preservation_and_allows_unarchived_copy()
    -> Result<(), TestError> {
        let database = id::<DatabaseId>(41)?;
        let source_space = id::<HistorySpaceId>(42)?;
        let preserve_space = id::<HistorySpaceId>(43)?;
        let clear_space = id::<HistorySpaceId>(44)?;
        let source_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(45)?);
        let preserve_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(46)?);
        let clear_id = HistorySpaceContentRef::Assertion(id::<AssertionId>(47)?);
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let preserve = HistorySpaceDefinition::new(preserve_space, None, Revision::GENESIS)?;
        let clear = HistorySpaceDefinition::new(clear_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, preserve, clear])?;
        let source_revision = model.publish(
            source_space,
            vec![TransferRecord::new(source_id, "hidden", vec![]).with_archived(true)],
        )?;

        let preserve_plan = plan(
            database,
            source_space,
            source_revision,
            preserve_space,
            model.history_space_head(preserve_space)?,
            source_id,
            preserve_id,
        )?;
        let preserve_head = model.latest_published();
        assert!(matches!(
            model.commit_transfer(
                &preserve_plan,
                vec![id::<ProvenanceId>(48)?],
                vec![],
                vec![id::<ArchiveTransitionId>(49)?],
            ),
            Err(
                TransferReferenceModelError::ArchivePreservationRequiresLaterRevision {
                    archived_targets: 1,
                }
            )
        ));
        assert_eq!(model.latest_published(), preserve_head);
        assert_eq!(model.history_space_head(preserve_space)?, Revision::GENESIS);

        let clear_plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: source_revision,
            target_history_space_id: clear_space,
            expected_target_head: model.history_space_head(clear_space)?,
            selected_records: BTreeSet::from([source_id]),
            record_id_map: BTreeMap::from([(source_id, clear_id)]),
            selected_event_relations: BTreeSet::new(),
            event_relation_id_map: BTreeMap::new(),
            external_reference_decisions: Vec::new(),
            lifecycle_policy:
                crate::transfer_model::TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })?;
        let cleared =
            model.commit_transfer(&clear_plan, vec![id::<ProvenanceId>(50)?], vec![], vec![])?;
        assert_eq!(
            cleared.records().first().map(TransferRecord::is_archived),
            Some(false)
        );
        assert!(cleared.archive_transitions().is_empty());
        Ok(())
    }

    #[test]
    fn copy_effective_lifecycle_requires_and_rewrites_the_complete_set() -> Result<(), TestError> {
        let database = id::<DatabaseId>(61)?;
        let source_space = id::<HistorySpaceId>(62)?;
        let target_space = id::<HistorySpaceId>(63)?;
        let source_assertion = HistorySpaceContentRef::Assertion(id::<AssertionId>(64)?);
        let source_retraction =
            HistorySpaceContentRef::AssertionRetraction(id::<AssertionRetractionId>(65)?);
        let target_assertion = HistorySpaceContentRef::Assertion(id::<AssertionId>(66)?);
        let target_retraction =
            HistorySpaceContentRef::AssertionRetraction(id::<AssertionRetractionId>(67)?);
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, target])?;
        let source_revision = model.publish(
            source_space,
            vec![
                TransferRecord::new(source_assertion, "claim", vec![]),
                TransferRecord::new(
                    source_retraction,
                    "retracted",
                    vec![source_assertion.record_ref()],
                )
                .with_effective_lifecycle(true),
            ],
        )?;

        let incomplete_plan = plan(
            database,
            source_space,
            source_revision,
            target_space,
            model.history_space_head(target_space)?,
            source_assertion,
            target_assertion,
        )?;
        assert_eq!(
            model.commit_transfer(
                &incomplete_plan,
                vec![id::<ProvenanceId>(68)?],
                vec![],
                vec![]
            ),
            Err(
                TransferReferenceModelError::MissingEffectiveLifecycleRecord {
                    target: source_assertion,
                    lifecycle_record: source_retraction,
                }
            )
        );
        assert!(
            model
                .read_at(target_space, model.latest_published())?
                .is_empty()
        );

        let complete_plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: source_revision,
            target_history_space_id: target_space,
            expected_target_head: model.history_space_head(target_space)?,
            selected_records: BTreeSet::from([source_assertion, source_retraction]),
            record_id_map: BTreeMap::from([
                (source_assertion, target_assertion),
                (source_retraction, target_retraction),
            ]),
            selected_event_relations: BTreeSet::new(),
            event_relation_id_map: BTreeMap::new(),
            external_reference_decisions: Vec::new(),
            lifecycle_policy:
                crate::transfer_model::TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })?;
        let receipt = model.commit_transfer(
            &complete_plan,
            vec![id::<ProvenanceId>(69)?, id::<ProvenanceId>(70)?],
            vec![],
            vec![],
        )?;
        assert_eq!(receipt.records().len(), 2);
        assert_eq!(
            receipt.records().get(1).map(TransferRecord::references),
            Some(&[target_assertion.record_ref()][..])
        );
        Ok(())
    }

    #[test]
    fn lifecycle_omission_requires_acknowledging_the_current_preview() -> Result<(), TestError> {
        let database = id::<DatabaseId>(71)?;
        let source_space = id::<HistorySpaceId>(72)?;
        let target_space = id::<HistorySpaceId>(73)?;
        let source_assertion = HistorySpaceContentRef::Assertion(id::<AssertionId>(74)?);
        let source_retraction =
            HistorySpaceContentRef::AssertionRetraction(id::<AssertionRetractionId>(75)?);
        let target_assertion = HistorySpaceContentRef::Assertion(id::<AssertionId>(76)?);
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, target])?;
        let source_revision = model.publish(
            source_space,
            vec![
                TransferRecord::new(source_assertion, "claim", vec![]),
                TransferRecord::new(
                    source_retraction,
                    "retracted",
                    vec![source_assertion.record_ref()],
                )
                .with_effective_lifecycle(true),
            ],
        )?;
        let transfer_plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: source_revision,
            target_history_space_id: target_space,
            expected_target_head: model.history_space_head(target_space)?,
            selected_records: BTreeSet::from([source_assertion]),
            record_id_map: BTreeMap::from([(source_assertion, target_assertion)]),
            selected_event_relations: BTreeSet::new(),
            event_relation_id_map: BTreeMap::new(),
            external_reference_decisions: Vec::new(),
            lifecycle_policy: TransferLifecyclePolicy::OmitWithAcknowledgement,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })?;

        let preview = model.preview_transfer(&transfer_plan)?;
        assert_eq!(
            preview.omitted_effective_lifecycle_records(),
            &[(source_assertion, source_retraction)]
        );
        assert!(preview.omitted_event_relation_retractions().is_empty());
        assert!(preview.archive_targets().is_empty());
        assert_eq!(
            model.commit_transfer(
                &transfer_plan,
                vec![id::<ProvenanceId>(77)?],
                vec![],
                vec![]
            ),
            Err(TransferReferenceModelError::LifecycleOmissionAcknowledgementMismatch)
        );
        assert!(
            model
                .read_at(target_space, model.latest_published())?
                .is_empty()
        );

        let acknowledgement = preview.acknowledge_lifecycle_omissions();
        let receipt = model.commit_transfer_acknowledged(
            &transfer_plan,
            vec![id::<ProvenanceId>(77)?],
            vec![],
            vec![],
            &acknowledgement,
        )?;
        assert_eq!(receipt.records().len(), 1);
        assert_eq!(
            receipt.records().first().map(TransferRecord::payload),
            Some(&"claim")
        );
        assert_eq!(
            model.read_at(target_space, model.latest_published())?.len(),
            1
        );
        Ok(())
    }

    #[test]
    fn selected_event_relation_gets_fresh_identity_and_mapped_endpoints() -> Result<(), TestError> {
        let database = id::<DatabaseId>(71)?;
        let source_space = id::<HistorySpaceId>(72)?;
        let target_space = id::<HistorySpaceId>(73)?;
        let source_from = HistorySpaceContentRef::Event(id::<EventId>(74)?);
        let source_to = HistorySpaceContentRef::Event(id::<EventId>(75)?);
        let target_from = HistorySpaceContentRef::Event(id::<EventId>(76)?);
        let target_to = HistorySpaceContentRef::Event(id::<EventId>(77)?);
        let source_relation_id = id::<EventRelationId>(78)?;
        let target_relation_id = id::<EventRelationId>(79)?;
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, target])?;
        let source_revision = model.publish(
            source_space,
            vec![
                TransferRecord::new(source_from, 10_u8, vec![]),
                TransferRecord::new(source_to, 20_u8, vec![]),
            ],
        )?;
        let source_relation = EventRelation::new(
            source_relation_id,
            match source_from {
                HistorySpaceContentRef::Event(id) => id,
                _ => return Err("test endpoint must be an Event".into()),
            },
            match source_to {
                HistorySpaceContentRef::Event(id) => id,
                _ => return Err("test endpoint must be an Event".into()),
            },
            EventRelationInputKind::Before,
            source_revision,
        )?;
        model.register_event_relation(source_relation, None, false)?;
        let transfer_plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: source_revision,
            target_history_space_id: target_space,
            expected_target_head: model.history_space_head(target_space)?,
            selected_records: BTreeSet::from([source_from, source_to]),
            record_id_map: BTreeMap::from([(source_from, target_from), (source_to, target_to)]),
            selected_event_relations: BTreeSet::from([source_relation_id]),
            event_relation_id_map: BTreeMap::from([(source_relation_id, target_relation_id)]),
            external_reference_decisions: Vec::new(),
            lifecycle_policy:
                crate::transfer_model::TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })?;

        let receipt = model.commit_transfer(
            &transfer_plan,
            vec![id::<ProvenanceId>(80)?, id::<ProvenanceId>(81)?],
            vec![],
            vec![],
        )?;
        assert_eq!(receipt.event_relations().len(), 1);
        assert_eq!(
            receipt
                .event_relations()
                .first()
                .map(|relation| relation.id()),
            Some(target_relation_id)
        );
        assert_eq!(
            receipt
                .event_relations()
                .first()
                .map(|relation| relation.from_event()),
            match target_from {
                HistorySpaceContentRef::Event(id) => Some(id),
                _ => None,
            }
        );
        assert_eq!(
            receipt
                .event_relations()
                .first()
                .map(|relation| relation.to_event()),
            match target_to {
                HistorySpaceContentRef::Event(id) => Some(id),
                _ => None,
            }
        );
        assert_eq!(
            receipt
                .event_relations()
                .first()
                .map(|relation| relation.kind()),
            Some(crate::EventRelationKind::Before)
        );
        assert!(receipt.event_relation_retractions().is_empty());
        Ok(())
    }

    #[test]
    fn omission_preview_reports_effective_event_relation_retraction() -> Result<(), TestError> {
        let database = id::<DatabaseId>(91)?;
        let source_space = id::<HistorySpaceId>(92)?;
        let target_space = id::<HistorySpaceId>(93)?;
        let source_from = HistorySpaceContentRef::Event(id::<EventId>(94)?);
        let source_to = HistorySpaceContentRef::Event(id::<EventId>(95)?);
        let target_from = HistorySpaceContentRef::Event(id::<EventId>(96)?);
        let target_to = HistorySpaceContentRef::Event(id::<EventId>(97)?);
        let source_relation_id = id::<EventRelationId>(98)?;
        let root = HistorySpaceDefinition::new(source_space, None, Revision::GENESIS)?;
        let target = HistorySpaceDefinition::new(target_space, None, Revision::GENESIS)?;
        let mut model = TransferReferenceModel::new(database, vec![root, target])?;
        let relation_revision = model.publish(
            source_space,
            vec![
                TransferRecord::new(source_from, "first", vec![]),
                TransferRecord::new(source_to, "second", vec![]),
            ],
        )?;
        model.publish(
            target_space,
            vec![TransferRecord::new(
                HistorySpaceContentRef::Assertion(id::<AssertionId>(99)?),
                "advance shared revision",
                vec![],
            )],
        )?;
        let source_relation = EventRelation::new(
            source_relation_id,
            match source_from {
                HistorySpaceContentRef::Event(id) => id,
                _ => return Err("source endpoint must be an Event".into()),
            },
            match source_to {
                HistorySpaceContentRef::Event(id) => id,
                _ => return Err("source endpoint must be an Event".into()),
            },
            EventRelationInputKind::Before,
            relation_revision,
        )?;
        let retraction = EventRelationRetraction::new(
            id::<EventRelationRetractionId>(100)?,
            &source_relation,
            "source relation retracted",
            model.latest_published(),
        )?;
        model.register_event_relation(source_relation, Some(retraction), false)?;
        let target_relation_id = id::<EventRelationId>(101)?;
        let copy_plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: model.latest_published(),
            target_history_space_id: target_space,
            expected_target_head: model.history_space_head(target_space)?,
            selected_records: BTreeSet::from([source_from, source_to]),
            record_id_map: BTreeMap::from([(source_from, target_from), (source_to, target_to)]),
            selected_event_relations: BTreeSet::from([source_relation_id]),
            event_relation_id_map: BTreeMap::from([(source_relation_id, target_relation_id)]),
            external_reference_decisions: Vec::new(),
            lifecycle_policy: TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })?;
        assert_eq!(
            model.preview_transfer(&copy_plan),
            Err(
                TransferReferenceModelError::EffectiveEventRelationRetractionNeedsOmissionPolicy {
                    identity: source_relation_id,
                }
            )
        );
        let transfer_plan = TransferPlan::new(TransferPlanSpec {
            database_id: database,
            source_history_space_id: source_space,
            source_recorded_as_of: model.latest_published(),
            target_history_space_id: target_space,
            expected_target_head: model.history_space_head(target_space)?,
            selected_records: BTreeSet::from([source_from, source_to]),
            record_id_map: BTreeMap::from([(source_from, target_from), (source_to, target_to)]),
            selected_event_relations: BTreeSet::from([source_relation_id]),
            event_relation_id_map: BTreeMap::from([(source_relation_id, target_relation_id)]),
            external_reference_decisions: Vec::new(),
            lifecycle_policy: TransferLifecyclePolicy::OmitWithAcknowledgement,
            archive_policy: crate::transfer_model::TransferArchivePolicy::StartUnarchived,
        })?;

        let preview = model.preview_transfer(&transfer_plan)?;
        assert_eq!(
            preview.omitted_event_relation_retractions(),
            &[source_relation_id]
        );
        let acknowledgement = preview.acknowledge_lifecycle_omissions();
        let receipt = model.commit_transfer_acknowledged(
            &transfer_plan,
            vec![id::<ProvenanceId>(102)?, id::<ProvenanceId>(103)?],
            vec![],
            vec![],
            &acknowledgement,
        )?;
        assert_eq!(receipt.event_relations().len(), 1);
        assert!(receipt.event_relation_retractions().is_empty());
        Ok(())
    }
}
