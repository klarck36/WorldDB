//! Durable, audited same-database HistorySpace content transfer.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use worlddb_core::{
    ArchiveState, ArchiveTargetRef, AuditAction, AuditCommitContext, AuditObjectClass,
    AuditOutcome, AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity,
    AuthorizationDecision, Bytes, Capability, DomainId, EventId, EventRelation, EventRelationBatch,
    EventRelationHistory, EventRelationKind, EventRelationRetraction, ExternalReferenceDecision,
    HistorySpaceCatalog, HistorySpaceContentRef, HistorySpaceId, OperationId, PolicyTarget, Record,
    RecordRef, Revision, TransferArchivePolicy, TransferLifecyclePolicy, TransferLineageId,
    TransferPlan, TransferPlanSpec, TransferPreview, TransferPreviewAcknowledgement,
    TransferReceipt, TransferRecord, TransferReferenceModel, TransferReferenceModelError,
    encode_record, history_space_content_ref, history_space_content_references,
    remap_history_space_content_record, validate_event_graph_transaction,
};

use crate::{
    FileSchemaManager, HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference,
    RecoveryManager, SecurityPolicyHistoryStore, WalOperationStatus, WalPrepareLog, WriterLock,
};

use super::{cleanup_staged_schema, next_audit_sequence, storage_error};

const MAX_TRANSFER_RECORDS: usize = 1_024;
const MAX_TRANSFER_SOURCE_RECORDS: usize = 100_000;
const MAX_TRANSFER_SOURCE_BYTES: usize = 256 * 1024 * 1024;

/// One authorized HistorySpace record available for explicit selection.
#[derive(Clone, Debug)]
pub struct TransferSourceItem {
    identity: HistorySpaceContentRef,
    recorded_revision: Revision,
    archived: bool,
    optional_flags: u64,
    record: Record,
}

impl TransferSourceItem {
    /// Closed typed identity used by the host's hidden selection value.
    #[must_use]
    pub const fn identity(&self) -> HistorySpaceContentRef {
        self.identity
    }

    /// Revision that created the source record.
    #[must_use]
    pub const fn recorded_revision(&self) -> Revision {
        self.recorded_revision
    }

    /// Archive state at the requested source snapshot.
    #[must_use]
    pub const fn is_archived(&self) -> bool {
        self.archived
    }

    /// Whether this version can safely rewrite the encoded source record.
    #[must_use]
    pub fn can_transfer(&self) -> bool {
        self.optional_flags == 0 && !is_lifecycle_record(self.identity)
    }

    /// Original immutable typed Record.
    #[must_use]
    pub const fn record(&self) -> &Record {
        &self.record
    }
}

/// Receipt for a committed durable HistorySpace transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPublicationReceipt {
    operation_id: OperationId,
    revision: Revision,
    record_id_map: BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef>,
    event_relation_id_map: BTreeMap<worlddb_core::EventRelationId, worlddb_core::EventRelationId>,
}

impl TransferPublicationReceipt {
    /// Operation identity bound to the required-audit and WAL commit.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Shared revision that contains the entire transfer.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Complete typed source-to-copy identity map.
    #[must_use]
    pub const fn record_id_map(&self) -> &BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef> {
        &self.record_id_map
    }

    /// Complete typed EventRelation identity map.
    #[must_use]
    pub const fn event_relation_id_map(
        &self,
    ) -> &BTreeMap<worlddb_core::EventRelationId, worlddb_core::EventRelationId> {
        &self.event_relation_id_map
    }
}

/// One source-visible project-wide EventRelation eligible for explicit transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferEventRelationItem {
    relation: EventRelation,
    archived: bool,
    retracted: bool,
}

impl TransferEventRelationItem {
    /// Project-wide identity used only as a hidden selection value.
    #[must_use]
    pub const fn id(self) -> worlddb_core::EventRelationId {
        self.relation.id()
    }

    /// Source event endpoint.
    #[must_use]
    pub const fn from_event(self) -> EventId {
        self.relation.from_event()
    }

    /// Destination event endpoint.
    #[must_use]
    pub const fn to_event(self) -> EventId {
        self.relation.to_event()
    }

    /// Relationship kind.
    #[must_use]
    pub const fn kind(self) -> EventRelationKind {
        self.relation.kind()
    }

    /// Whether the source relation is operationally archived at this revision.
    #[must_use]
    pub const fn is_archived(self) -> bool {
        self.archived
    }

    /// Whether a retraction is effective at this revision.
    #[must_use]
    pub const fn is_retracted(self) -> bool {
        self.retracted
    }
}

/// Durable file-backed manager for explicit content transfer between existing HistorySpaces.
pub struct FileHistorySpaceTransferManager<'a> {
    schema: FileSchemaManager<'a>,
    database_id: worlddb_core::DatabaseId,
}

impl<'a> FileHistorySpaceTransferManager<'a> {
    /// Recovers, verifies, loads, and binds one authenticated project manager.
    pub fn open(
        layout: crate::DatabaseLayout,
        writer_lock: &'a WriterLock,
        principal: worlddb_core::PrincipalId,
    ) -> Result<Self, HistorySpaceTransferError> {
        let database_id = layout
            .database_id()
            .ok_or(HistorySpaceTransferError::InvalidHistory)?;
        let schema = FileSchemaManager::open(layout, writer_lock, principal)
            .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?;
        Ok(Self {
            schema,
            database_id,
        })
    }

    /// Current shared database revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.schema.revision()
    }

    /// Lists records visible from the source branch at one pinned revision.
    pub fn visible_content(
        &self,
        source_history_space_id: HistorySpaceId,
        target_history_space_id: HistorySpaceId,
        source_recorded_as_of: Revision,
    ) -> Result<Vec<TransferSourceItem>, HistorySpaceTransferError> {
        self.authorize_transfer(source_history_space_id, target_history_space_id)?;
        let snapshot = self.load_transfer_snapshot(source_recorded_as_of)?;
        let visible = snapshot
            .model
            .read_at(source_history_space_id, source_recorded_as_of)
            .map_err(HistorySpaceTransferError::Model)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(visible.len())
            .map_err(|_| HistorySpaceTransferError::ResourceLimit)?;
        for (recorded_revision, item) in visible {
            let decoded = worlddb_core::decode_record(item.payload())
                .map_err(|_| HistorySpaceTransferError::InvalidHistory)?;
            let optional_flags = snapshot
                .optional_flags
                .get(&item.id().record_ref())
                .copied()
                .ok_or(HistorySpaceTransferError::InvalidHistory)?;
            result.push(TransferSourceItem {
                identity: item.id(),
                recorded_revision,
                archived: item.is_archived(),
                optional_flags,
                record: decoded.into_record(),
            });
        }
        Ok(result)
    }

    /// Current target-local head used to bind a preview to the target branch.
    pub fn history_space_head(
        &self,
        history_space_id: HistorySpaceId,
    ) -> Result<Revision, HistorySpaceTransferError> {
        self.authorize_history_space_read(history_space_id)?;
        self.load_transfer_snapshot(self.revision())?
            .model
            .history_space_head(history_space_id)
            .map_err(HistorySpaceTransferError::Model)
    }

    /// Lists source-visible project-wide relations whose event endpoints are visible there.
    pub fn visible_event_relations(
        &self,
        source_history_space_id: HistorySpaceId,
        source_recorded_as_of: Revision,
    ) -> Result<Vec<TransferEventRelationItem>, HistorySpaceTransferError> {
        self.authorize_history_space_read(source_history_space_id)?;
        let snapshot = self.load_transfer_snapshot(source_recorded_as_of)?;
        let visible = snapshot
            .model
            .read_at(source_history_space_id, source_recorded_as_of)
            .map_err(HistorySpaceTransferError::Model)?;
        let visible_events = visible
            .into_iter()
            .filter_map(|(_, record)| match record.id() {
                HistorySpaceContentRef::Event(id) => Some(id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        Ok(snapshot
            .event_relations
            .into_iter()
            .filter_map(|(relation, retraction, archived)| {
                (relation.created_revision() <= source_recorded_as_of
                    && visible_events.contains(&relation.from_event())
                    && visible_events.contains(&relation.to_event()))
                .then_some(TransferEventRelationItem {
                    relation,
                    archived,
                    retracted: retraction
                        .is_some_and(|value| value.created_revision() <= source_recorded_as_of),
                })
            })
            .collect())
    }

    /// Builds a bounded typed plan and generates every destination identity in the host.
    pub fn build_plan(
        &self,
        source_history_space_id: HistorySpaceId,
        target_history_space_id: HistorySpaceId,
        source_recorded_as_of: Revision,
        selected_records: BTreeSet<HistorySpaceContentRef>,
        selected_event_relations: BTreeSet<worlddb_core::EventRelationId>,
        external_reference_decision: ExternalReferenceDecision,
    ) -> Result<TransferPlan, HistorySpaceTransferError> {
        self.authorize_transfer(source_history_space_id, target_history_space_id)?;
        if selected_records.is_empty() || selected_records.len() > MAX_TRANSFER_RECORDS {
            return Err(HistorySpaceTransferError::InvalidCandidate(
                "select between 1 and 1024 content records",
            ));
        }
        if selected_event_relations.len() > MAX_TRANSFER_RECORDS {
            return Err(HistorySpaceTransferError::ResourceLimit);
        }
        let snapshot = self.load_transfer_snapshot(source_recorded_as_of)?;
        let source_visible = snapshot
            .model
            .read_at(source_history_space_id, source_recorded_as_of)
            .map_err(HistorySpaceTransferError::Model)?;
        let visible_by_id = source_visible
            .iter()
            .map(|(_, record)| (record.id(), record))
            .collect::<BTreeMap<_, _>>();
        let mut record_id_map = BTreeMap::new();
        let mut external_references = BTreeSet::new();
        for source in &selected_records {
            let item = visible_by_id
                .get(source)
                .ok_or(HistorySpaceTransferError::SourceChanged)?;
            if is_lifecycle_record(*source) {
                return Err(HistorySpaceTransferError::LifecycleCopyNotSupported);
            }
            let optional_flags = snapshot
                .optional_flags
                .get(&source.record_ref())
                .copied()
                .ok_or(HistorySpaceTransferError::InvalidHistory)?;
            if optional_flags != 0 {
                return Err(HistorySpaceTransferError::UnsupportedRecordEncoding);
            }
            let decoded = worlddb_core::decode_record(item.payload())
                .map_err(|_| HistorySpaceTransferError::InvalidHistory)?;
            for reference in history_space_content_references(decoded.record()) {
                match HistorySpaceContentRef::try_from(reference) {
                    Ok(internal) if selected_records.contains(&internal) => {}
                    _ => {
                        external_references.insert(reference);
                    }
                }
            }
            record_id_map.insert(*source, fresh_content_identity(*source)?);
        }
        let visible_relations = self
            .visible_event_relations(source_history_space_id, source_recorded_as_of)?
            .into_iter()
            .map(|item| item.id())
            .collect::<BTreeSet<_>>();
        if !selected_event_relations.is_subset(&visible_relations) {
            return Err(HistorySpaceTransferError::SourceChanged);
        }
        let mut event_relation_id_map = BTreeMap::new();
        for source in &selected_event_relations {
            event_relation_id_map.insert(
                *source,
                worlddb_core::storage_internal::generate_project_bootstrap_id()
                    .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?,
            );
        }
        let expected_target_head = snapshot
            .model
            .history_space_head(target_history_space_id)
            .map_err(HistorySpaceTransferError::Model)?;
        let external_reference_decisions = external_references
            .into_iter()
            .map(|reference| (reference, external_reference_decision))
            .collect();
        TransferPlan::new(TransferPlanSpec {
            database_id: self.database_id,
            source_history_space_id,
            source_recorded_as_of,
            target_history_space_id,
            expected_target_head,
            selected_records,
            record_id_map,
            selected_event_relations,
            event_relation_id_map,
            external_reference_decisions,
            lifecycle_policy: TransferLifecyclePolicy::OmitWithAcknowledgement,
            archive_policy: TransferArchivePolicy::StartUnarchived,
        })
        .map_err(|_| HistorySpaceTransferError::InvalidPlan)
    }

    /// Validates a complete plan against the pinned source and current target view.
    pub fn preview(
        &self,
        plan: &TransferPlan,
    ) -> Result<TransferPreview, HistorySpaceTransferError> {
        self.authorize_transfer(
            plan.source_history_space_id(),
            plan.target_history_space_id(),
        )?;
        self.load_transfer_snapshot(plan.source_recorded_as_of())?
            .model
            .preview_transfer(plan)
            .map_err(HistorySpaceTransferError::Model)
    }

    /// Revalidates, stages, audits, and publishes every transfer record in one WAL transaction.
    pub fn commit(
        &mut self,
        plan: &TransferPlan,
        operation_id: OperationId,
        acknowledge_lifecycle_omissions: bool,
    ) -> Result<TransferPublicationReceipt, HistorySpaceTransferError> {
        self.authorize_transfer(
            plan.source_history_space_id(),
            plan.target_history_space_id(),
        )?;
        if self.schema.revision() == Revision::GENESIS {
            return Err(HistorySpaceTransferError::InvalidHistory);
        }
        let mut snapshot = self.load_transfer_snapshot(plan.source_recorded_as_of())?;
        let preview = snapshot
            .model
            .preview_transfer(plan)
            .map_err(HistorySpaceTransferError::Model)?;
        if plan.selected_records().iter().any(|identity| {
            snapshot
                .optional_flags
                .get(&identity.record_ref())
                .is_none_or(|flags| *flags != 0)
        }) || plan.selected_event_relations().iter().any(|identity| {
            snapshot
                .optional_flags
                .get(&RecordRef::EventRelation(*identity))
                .is_none_or(|flags| *flags != 0)
        }) {
            return Err(HistorySpaceTransferError::UnsupportedRecordEncoding);
        }
        if plan
            .selected_records()
            .iter()
            .any(|identity| is_lifecycle_record(*identity))
        {
            return Err(HistorySpaceTransferError::LifecycleCopyNotSupported);
        }
        let provenance_ids = generate_ids::<worlddb_core::ProvenanceId>(
            plan.selected_records()
                .iter()
                .filter(|identity| supports_derived_from(**identity))
                .count(),
        )?;
        let transfer_lineage_ids = generate_ids::<TransferLineageId>(
            plan.selected_records()
                .iter()
                .filter(|identity| !supports_derived_from(**identity))
                .count(),
        )?;
        let receipt = match plan.lifecycle_policy() {
            TransferLifecyclePolicy::CopyEffectiveLifecycle => snapshot
                .model
                .commit_transfer_with_transfer_lineage_ids(
                    plan,
                    provenance_ids,
                    transfer_lineage_ids,
                    Vec::new(),
                    Vec::new(),
                )
                .map_err(HistorySpaceTransferError::Model)?,
            TransferLifecyclePolicy::OmitWithAcknowledgement => {
                if !acknowledge_lifecycle_omissions {
                    return Err(HistorySpaceTransferError::AcknowledgementRequired);
                }
                let acknowledgement: TransferPreviewAcknowledgement =
                    preview.acknowledge_lifecycle_omissions();
                snapshot
                    .model
                    .commit_transfer_acknowledged_with_transfer_lineage_ids(
                        plan,
                        provenance_ids,
                        transfer_lineage_ids,
                        Vec::new(),
                        Vec::new(),
                        &acknowledgement,
                    )
                    .map_err(HistorySpaceTransferError::Model)?
            }
        };

        let revision = receipt.revision();
        let mut records = Vec::new();
        let expected_record_count = plan
            .record_id_map()
            .len()
            .saturating_add(receipt.lineage().len())
            .saturating_add(receipt.transfer_lineage().len())
            .saturating_add(receipt.event_relations().len())
            .saturating_add(receipt.event_relation_retractions().len())
            .saturating_add(receipt.archive_transitions().len());
        if expected_record_count == 0 || expected_record_count > MAX_TRANSFER_RECORDS {
            return Err(HistorySpaceTransferError::ResourceLimit);
        }
        records
            .try_reserve_exact(expected_record_count)
            .map_err(|_| HistorySpaceTransferError::ResourceLimit)?;
        for (source, target) in plan.record_id_map() {
            let original = snapshot
                .content_records
                .get(source)
                .ok_or(HistorySpaceTransferError::SourceChanged)?;
            records.push(
                remap_history_space_content_record(
                    original,
                    plan.target_history_space_id(),
                    *target,
                    plan.record_id_map(),
                    revision,
                )
                .map_err(|_| HistorySpaceTransferError::InvalidTransferRecord)?,
            );
        }
        records.extend(receipt.lineage().iter().cloned().map(Record::Provenance));
        records.extend(
            receipt
                .transfer_lineage()
                .iter()
                .copied()
                .map(Record::TransferLineage),
        );
        records.extend(
            receipt
                .event_relations()
                .iter()
                .copied()
                .map(Record::EventRelation),
        );
        records.extend(
            receipt
                .event_relation_retractions()
                .iter()
                .cloned()
                .map(Record::EventRelationRetraction),
        );
        records.extend(
            receipt
                .archive_transitions()
                .iter()
                .copied()
                .map(Record::ArchiveTransition),
        );
        self.validate_event_graph(&snapshot.records, &receipt, revision)?;
        let record_bytes = records
            .iter()
            .map(|record| encode_record(record).map(|bytes| bytes.len()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| HistorySpaceTransferError::InvalidTransferRecord)?;
        let total_bytes = record_bytes
            .iter()
            .try_fold(0usize, |total, bytes| total.checked_add(*bytes));
        if total_bytes
            .is_none_or(|total| total > worlddb_core::DecoderLimits::DEFAULT.max_batch_bytes)
        {
            return Err(HistorySpaceTransferError::ResourceLimit);
        }
        self.publish(
            operation_id,
            plan,
            records,
            receipt.record_id_map().clone(),
            plan.event_relation_id_map().clone(),
        )
    }

    fn authorize_transfer(
        &self,
        source_history_space_id: HistorySpaceId,
        target_history_space_id: HistorySpaceId,
    ) -> Result<(), HistorySpaceTransferError> {
        if source_history_space_id == target_history_space_id {
            return Err(HistorySpaceTransferError::InvalidCandidate(
                "source and target branches must differ",
            ));
        }
        for history_space_id in [source_history_space_id, target_history_space_id] {
            self.authorize_history_space_read(history_space_id)?;
            if self.authorize_one(history_space_id, Capability::HistorySpaceTransfer)?
                != AuthorizationDecision::Allow
            {
                return Err(HistorySpaceTransferError::Unauthorized(
                    Capability::HistorySpaceTransfer,
                ));
            }
        }
        Ok(())
    }

    fn authorize_history_space_read(
        &self,
        history_space_id: HistorySpaceId,
    ) -> Result<(), HistorySpaceTransferError> {
        if self.authorize_one(history_space_id, Capability::HistorySpaceRead)?
            == AuthorizationDecision::Allow
        {
            Ok(())
        } else {
            Err(HistorySpaceTransferError::Unauthorized(
                Capability::HistorySpaceRead,
            ))
        }
    }

    fn authorize_one(
        &self,
        history_space_id: HistorySpaceId,
        capability: Capability,
    ) -> Result<AuthorizationDecision, HistorySpaceTransferError> {
        let policy = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?;
        let target = PolicyTarget::new(Some(history_space_id), None, None, None, None);
        Ok(policy
            .snapshot()
            .authorize(self.schema.principal, capability, target))
    }

    fn load_transfer_snapshot(
        &self,
        source_recorded_as_of: Revision,
    ) -> Result<TransferSnapshot, HistorySpaceTransferError> {
        if source_recorded_as_of > self.revision() {
            return Err(HistorySpaceTransferError::RevisionNotPublished);
        }
        let store = HistorySegmentStore::new(self.schema.layout.clone());
        let mut all_records = Vec::<(Option<Revision>, u64, Record)>::new();
        let mut record_by_ref = BTreeMap::<RecordRef, Record>::new();
        let mut definitions = Vec::new();
        let mut total_bytes = 0usize;
        for reference in self
            .schema
            .manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
        {
            let segment = store.read_segment(reference.id()).map_err(storage_error)?;
            if segment.content_digest() != reference.content_digest() {
                return Err(HistorySpaceTransferError::InvalidHistory);
            }
            for decoded in segment.records() {
                let frame_bytes = worlddb_core::encode_decoded_record(decoded)
                    .map_err(|_| HistorySpaceTransferError::InvalidHistory)?;
                total_bytes = total_bytes
                    .checked_add(frame_bytes.len())
                    .ok_or(HistorySpaceTransferError::ResourceLimit)?;
                if all_records.len() >= MAX_TRANSFER_SOURCE_RECORDS
                    || total_bytes > MAX_TRANSFER_SOURCE_BYTES
                {
                    return Err(HistorySpaceTransferError::ResourceLimit);
                }
                let record = decoded.record().clone();
                if let Record::HistorySpaceDefinition(definition) = record {
                    definitions.push(definition);
                    continue;
                }
                let revision = record_revision(&record);
                if revision.is_some_and(|revision| {
                    revision > reference.through_revision() || revision > self.revision()
                }) {
                    return Err(HistorySpaceTransferError::InvalidHistory);
                }
                if let Some(record_ref) = record_ref(&record) {
                    if record_by_ref.insert(record_ref, record.clone()).is_some() {
                        return Err(HistorySpaceTransferError::InvalidHistory);
                    }
                }
                all_records.push((revision, decoded.optional_flags(), record));
            }
        }
        let catalog = HistorySpaceCatalog::new(definitions)
            .map_err(|_| HistorySpaceTransferError::InvalidHistory)?;
        let archive_states = archive_states_at(&all_records, source_recorded_as_of)?;
        let mut content_records = BTreeMap::new();
        let mut optional_flags_by_record_ref = BTreeMap::new();
        let mut grouped =
            BTreeMap::<Revision, Vec<(HistorySpaceId, TransferRecord<Vec<u8>>)>>::new();
        let mut project_wide_refs = BTreeSet::new();
        let mut relations_by_id = BTreeMap::<worlddb_core::EventRelationId, EventRelation>::new();
        let mut relation_retractions =
            BTreeMap::<worlddb_core::EventRelationId, EventRelationRetraction>::new();
        for (revision, optional_flags, record) in &all_records {
            let Some(record_ref) = record_ref(record) else {
                continue;
            };
            optional_flags_by_record_ref.insert(record_ref, *optional_flags);
            if let Some(identity) = history_space_content_ref(record) {
                let revision = revision.ok_or(HistorySpaceTransferError::InvalidHistory)?;
                let owner = content_owner(record, &record_by_ref)
                    .ok_or(HistorySpaceTransferError::InvalidHistory)?;
                if catalog.definition(owner).is_none() {
                    return Err(HistorySpaceTransferError::InvalidHistory);
                }
                if content_records.insert(identity, record.clone()).is_some() {
                    return Err(HistorySpaceTransferError::InvalidHistory);
                }
                let archived = archive_states.get(&record_ref) == Some(&ArchiveState::Archived);
                let effective_lifecycle = is_lifecycle_record(identity);
                let payload =
                    encode_record(record).map_err(|_| HistorySpaceTransferError::InvalidHistory)?;
                let transfer_record = TransferRecord::new(
                    identity,
                    payload,
                    history_space_content_references(record),
                )
                .with_archived(archived)
                .with_effective_lifecycle(effective_lifecycle);
                grouped
                    .entry(revision)
                    .or_default()
                    .push((owner, transfer_record));
            } else {
                project_wide_refs.insert(record_ref);
            }
            match record {
                Record::EventRelation(relation) => {
                    if relations_by_id.insert(relation.id(), *relation).is_some() {
                        return Err(HistorySpaceTransferError::InvalidHistory);
                    }
                }
                Record::EventRelationRetraction(retraction) => {
                    if relation_retractions
                        .insert(retraction.event_relation_id(), retraction.clone())
                        .is_some()
                    {
                        return Err(HistorySpaceTransferError::InvalidHistory);
                    }
                }
                _ => {}
            }
        }
        let relation_data = relations_by_id
            .values()
            .map(|relation| {
                let archived = archive_states.get(&RecordRef::EventRelation(relation.id()))
                    == Some(&ArchiveState::Archived);
                (
                    *relation,
                    relation_retractions.get(&relation.id()).cloned(),
                    archived,
                )
            })
            .collect::<Vec<_>>();
        let groups = grouped.into_iter().collect::<Vec<_>>();
        let event_relations = relation_data
            .iter()
            .map(|(relation, retraction, archived)| (*relation, retraction.clone(), *archived))
            .collect();
        let model = TransferReferenceModel::from_published_history(
            self.database_id,
            catalog.definitions().to_vec(),
            self.revision(),
            groups,
            project_wide_refs.into_iter().collect(),
            relation_data,
        )
        .map_err(HistorySpaceTransferError::Model)?;
        Ok(TransferSnapshot {
            model,
            records: all_records
                .into_iter()
                .filter_map(|(revision, _, record)| revision.map(|revision| (revision, record)))
                .collect(),
            content_records,
            optional_flags: optional_flags_by_record_ref,
            event_relations,
        })
    }

    fn validate_event_graph(
        &self,
        snapshot_records: &[(Revision, Record)],
        receipt: &TransferReceipt<Vec<u8>>,
        revision: Revision,
    ) -> Result<(), HistorySpaceTransferError> {
        let event_ids = snapshot_records
            .iter()
            .filter_map(|(_, record)| match record {
                Record::Event(event) => Some(event.id()),
                _ => None,
            })
            .chain(
                receipt
                    .records()
                    .iter()
                    .filter_map(|record| match record.id() {
                        HistorySpaceContentRef::Event(id) => Some(id),
                        _ => None,
                    }),
            )
            .collect::<Vec<EventId>>();
        let relations = snapshot_records
            .iter()
            .filter_map(|(_, record)| match record {
                Record::EventRelation(relation) => Some(*relation),
                _ => None,
            })
            .collect::<Vec<_>>();
        let retractions = snapshot_records
            .iter()
            .filter_map(|(_, record)| match record {
                Record::EventRelationRetraction(retraction) => Some(retraction.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let additions = EventRelationBatch::new(receipt.event_relations().to_vec())
            .map_err(|_| HistorySpaceTransferError::InvalidEventGraph)?;
        validate_event_graph_transaction(
            &event_ids,
            EventRelationHistory::new(&relations, &retractions),
            worlddb_core::RecordedAsOf::from_published_revision(self.revision()),
            revision,
            receipt.event_relation_retractions(),
            &additions,
        )
        .map_err(|_| HistorySpaceTransferError::InvalidEventGraph)?;
        Ok(())
    }

    fn publish(
        &mut self,
        operation_id: OperationId,
        plan: &TransferPlan,
        records: Vec<Record>,
        record_id_map: BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef>,
        event_relation_id_map: BTreeMap<
            worlddb_core::EventRelationId,
            worlddb_core::EventRelationId,
        >,
    ) -> Result<TransferPublicationReceipt, HistorySpaceTransferError> {
        let expected_base = self.revision();
        let wal = WalPrepareLog::new(&self.schema.layout);
        if wal
            .commit_head(self.schema.writer_lock)
            .map_err(storage_error)?
            .revision()
            != expected_base
        {
            return Err(HistorySpaceTransferError::Conflict);
        }
        if wal
            .operation_status(self.schema.writer_lock, operation_id)
            .map_err(storage_error)?
            != WalOperationStatus::NotCommitted
        {
            return Err(HistorySpaceTransferError::OperationAlreadyUsed);
        }
        let policy_version = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?;
        self.authorize_transfer(
            plan.source_history_space_id(),
            plan.target_history_space_id(),
        )?;
        let target_revision = expected_base
            .next_commit()
            .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?;
        let sequence = next_audit_sequence(&wal, self.schema.writer_lock)
            .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?;
        let mut fingerprint_input = Vec::with_capacity(64);
        for history_space_id in [
            plan.source_history_space_id(),
            plan.target_history_space_id(),
        ] {
            fingerprint_input.extend_from_slice(
                &policy_version.snapshot().effective_capability_fingerprint(
                    self.schema.principal,
                    PolicyTarget::new(Some(history_space_id), None, None, None, None),
                ),
            );
        }
        let fingerprint = blake3::hash(&fingerprint_input);
        let policy_fingerprint =
            AuditPolicyFingerprint::new(Bytes::new(fingerprint.as_bytes().to_vec()))
                .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?;
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id:
                    worlddb_core::storage_internal::generate_schema_management_audit_record_id()
                        .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?,
                sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_schema_management_audit_operation_id()
                        .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?,
            },
            AuditRecordDetails {
                actor: self.schema.principal,
                action: AuditAction::HistorySpaceTransfer,
                object_class: AuditObjectClass::Database,
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
            .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?;
        let next_policy_version = worlddb_core::SecurityPolicyVersion::new(
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
                return Err(HistorySpaceTransferError::Storage(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; staged transfer cleanup failed: {cleanup_error}")
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
            Ok(_) => return Err(HistorySpaceTransferError::UnknownCommit(operation_id)),
            Err(commit_error) => {
                let recovery = RecoveryManager::new(self.schema.layout.clone())
                    .recover(self.schema.writer_lock)
                    .map_err(|error| {
                        HistorySpaceTransferError::UnknownCommitDiagnostic(
                            operation_id,
                            format!("{commit_error}; recovery: {error}"),
                        )
                    })?;
                let status = wal
                    .operation_status(self.schema.writer_lock, operation_id)
                    .map_err(storage_error)?;
                if !recovery.report().is_clean()
                    || !matches!(status, WalOperationStatus::Committed(receipt) if receipt.revision() == target_revision)
                {
                    if status == WalOperationStatus::NotCommitted {
                        cleanup_staged_schema(
                            &history_store,
                            history_reference,
                            &security_store,
                            security_reference,
                        );
                        return Err(HistorySpaceTransferError::Storage(commit_error.to_string()));
                    }
                    return Err(HistorySpaceTransferError::UnknownCommitDiagnostic(
                        operation_id,
                        commit_error.to_string(),
                    ));
                }
            }
        }
        let recovery = RecoveryManager::new(self.schema.layout.clone())
            .recover(self.schema.writer_lock)
            .map_err(|error| {
                HistorySpaceTransferError::UnknownCommitDiagnostic(operation_id, error.to_string())
            })?;
        if !recovery.report().is_clean() {
            return Err(HistorySpaceTransferError::UnknownCommit(operation_id));
        }
        let reopened = FileSchemaManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| {
            HistorySpaceTransferError::UnknownCommitDiagnostic(operation_id, error.to_string())
        })?;
        if reopened.revision() != target_revision {
            return Err(HistorySpaceTransferError::UnknownCommit(operation_id));
        }
        self.schema = reopened;
        Ok(TransferPublicationReceipt {
            operation_id,
            revision: target_revision,
            record_id_map,
            event_relation_id_map,
        })
    }
}

struct TransferSnapshot {
    model: TransferReferenceModel<Vec<u8>>,
    records: Vec<(Revision, Record)>,
    content_records: BTreeMap<HistorySpaceContentRef, Record>,
    optional_flags: BTreeMap<RecordRef, u64>,
    event_relations: Vec<(EventRelation, Option<EventRelationRetraction>, bool)>,
}

/// Failure to authorize, validate, persist, or recover a transfer.
#[derive(Debug)]
pub enum HistorySpaceTransferError {
    /// The current principal lacks one of the required permissions.
    Unauthorized(Capability),
    /// The requested source or target is structurally invalid.
    InvalidCandidate(&'static str),
    /// One existing history invariant or typed record is invalid.
    InvalidHistory,
    /// The requested source revision has not been published.
    RevisionNotPublished,
    /// The current target head differs from the preview's pinned head.
    Conflict,
    /// The source content changed or is no longer resolvable as planned.
    SourceChanged,
    /// The closed transfer payload family or rewrite shape is invalid.
    InvalidTransferRecord,
    /// The existing or resulting EventRelation graph is invalid.
    InvalidEventGraph,
    /// A plan needs explicit acknowledgement of the lifecycle difference preview.
    AcknowledgementRequired,
    /// A selected source frame has optional wire flags this version cannot safely rewrite.
    UnsupportedRecordEncoding,
    /// Lifecycle records require a later commit than the copied target, so they cannot share the transfer revision.
    LifecycleCopyNotSupported,
    /// The closed typed transfer plan rejected generated or selected identities.
    InvalidPlan,
    /// The transfer exceeds a fixed record, byte, or allocation budget.
    ResourceLimit,
    /// The operation ID already belongs to an existing or unresolved WAL transaction.
    OperationAlreadyUsed,
    /// A committed revision was published but cannot be reconciled by the operation ID.
    UnknownCommit(OperationId),
    /// A committed revision's outcome is still indeterminate, with a diagnostic.
    UnknownCommitDiagnostic(OperationId, String),
    /// The closed typed transfer reference model rejected the candidate plan.
    Model(TransferReferenceModelError),
    /// Storage or required-audit publication failed.
    Storage(String),
}

impl fmt::Display for HistorySpaceTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized(capability) => {
                write!(formatter, "transfer permission denied: {capability:?}")
            }
            Self::InvalidCandidate(message) => formatter.write_str(message),
            Self::InvalidHistory => formatter.write_str("HistorySpace transfer history is invalid"),
            Self::RevisionNotPublished => formatter.write_str("source revision is not published"),
            Self::Conflict => {
                formatter.write_str("target branch changed; refresh the transfer preview")
            }
            Self::SourceChanged => {
                formatter.write_str("a selected source record is no longer available")
            }
            Self::InvalidTransferRecord => {
                formatter.write_str("a selected record cannot be copied safely")
            }
            Self::InvalidEventGraph => {
                formatter.write_str("the resulting event relationship graph is invalid")
            }
            Self::AcknowledgementRequired => formatter
                .write_str("review and acknowledge the lifecycle differences before transfer"),
            Self::UnsupportedRecordEncoding => formatter.write_str(
                "a selected record uses an optional encoding this version cannot preserve",
            ),
            Self::LifecycleCopyNotSupported => formatter.write_str(
                "lifecycle records need a later published revision than their copied target",
            ),
            Self::InvalidPlan => {
                formatter.write_str("the selected records do not form a valid transfer plan")
            }
            Self::ResourceLimit => {
                formatter.write_str("transfer exceeds the supported resource limits")
            }
            Self::OperationAlreadyUsed => {
                formatter.write_str("transfer operation identity has already been used")
            }
            Self::UnknownCommit(operation_id) => write!(
                formatter,
                "transfer commit outcome is unknown for {operation_id}"
            ),
            Self::UnknownCommitDiagnostic(operation_id, diagnostic) => write!(
                formatter,
                "transfer commit outcome is unknown for {operation_id}: {diagnostic}"
            ),
            Self::Model(error) => write!(formatter, "transfer validation failed: {error}"),
            Self::Storage(error) => write!(formatter, "transfer storage operation failed: {error}"),
        }
    }
}

impl std::error::Error for HistorySpaceTransferError {}

impl From<super::SchemaManagementError> for HistorySpaceTransferError {
    fn from(error: super::SchemaManagementError) -> Self {
        Self::Storage(error.to_string())
    }
}

fn generate_ids<T: DomainId>(count: usize) -> Result<Vec<T>, HistorySpaceTransferError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| HistorySpaceTransferError::ResourceLimit)?;
    for _ in 0..count {
        result.push(
            worlddb_core::storage_internal::generate_project_bootstrap_id::<T>()
                .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))?,
        );
    }
    Ok(result)
}

fn generate_id<T: DomainId>() -> Result<T, HistorySpaceTransferError> {
    worlddb_core::storage_internal::generate_project_bootstrap_id::<T>()
        .map_err(|error| HistorySpaceTransferError::Storage(error.to_string()))
}

fn fresh_content_identity(
    source: HistorySpaceContentRef,
) -> Result<HistorySpaceContentRef, HistorySpaceTransferError> {
    use HistorySpaceContentRef as Content;

    Ok(match source {
        Content::Assertion(_) => Content::Assertion(generate_id::<worlddb_core::AssertionId>()?),
        Content::Mask(_) => Content::Mask(generate_id::<worlddb_core::MaskId>()?),
        Content::ReplacementBoundary(_) => {
            Content::ReplacementBoundary(generate_id::<worlddb_core::ReplacementBoundaryId>()?)
        }
        Content::Event(_) => Content::Event(generate_id::<worlddb_core::EventId>()?),
        Content::EventMask(_) => Content::EventMask(generate_id::<worlddb_core::EventMaskId>()?),
        Content::AssertionValidityClosure(_) => Content::AssertionValidityClosure(generate_id::<
            worlddb_core::AssertionValidityClosureId,
        >()?),
        Content::AssertionRetraction(_) => {
            Content::AssertionRetraction(generate_id::<worlddb_core::AssertionRetractionId>()?)
        }
        Content::MaskValidityClosure(_) => {
            Content::MaskValidityClosure(generate_id::<worlddb_core::MaskValidityClosureId>()?)
        }
        Content::MaskRetraction(_) => {
            Content::MaskRetraction(generate_id::<worlddb_core::MaskRetractionId>()?)
        }
        Content::ReplacementBoundaryValidityClosure(_) => {
            Content::ReplacementBoundaryValidityClosure(generate_id::<
                worlddb_core::ReplacementBoundaryValidityClosureId,
            >()?)
        }
        Content::ReplacementBoundaryRetraction(_) => {
            Content::ReplacementBoundaryRetraction(generate_id::<
                worlddb_core::ReplacementBoundaryRetractionId,
            >()?)
        }
        Content::EventSpanClosure(_) => {
            Content::EventSpanClosure(generate_id::<worlddb_core::EventSpanClosureId>()?)
        }
        Content::EventRetraction(_) => {
            Content::EventRetraction(generate_id::<worlddb_core::EventRetractionId>()?)
        }
        Content::EventMaskRetraction(_) => {
            Content::EventMaskRetraction(generate_id::<worlddb_core::EventMaskRetractionId>()?)
        }
    })
}

fn supports_derived_from(identity: HistorySpaceContentRef) -> bool {
    !matches!(
        identity,
        HistorySpaceContentRef::Mask(_)
            | HistorySpaceContentRef::ReplacementBoundary(_)
            | HistorySpaceContentRef::EventMask(_)
    )
}

fn is_lifecycle_record(identity: HistorySpaceContentRef) -> bool {
    matches!(
        identity,
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

fn direct_owner(record: &Record) -> Option<HistorySpaceId> {
    match record {
        Record::Assertion(value) => Some(value.context().history_space_id()),
        Record::Mask(value) => Some(value.context().history_space_id()),
        Record::ReplacementBoundary(value) => Some(value.context().history_space_id()),
        Record::Event(value) => Some(value.history_space_id()),
        Record::EventMask(value) => Some(value.history_space_id()),
        _ => None,
    }
}

fn content_owner(
    record: &Record,
    records_by_ref: &BTreeMap<RecordRef, Record>,
) -> Option<HistorySpaceId> {
    if let Some(owner) = direct_owner(record) {
        return Some(owner);
    }
    let target = *history_space_content_references(record).first()?;
    direct_owner(records_by_ref.get(&target)?)
}

fn archive_states_at(
    records: &[(Option<Revision>, u64, Record)],
    as_of: Revision,
) -> Result<HashMap<RecordRef, ArchiveState>, HistorySpaceTransferError> {
    let mut transitions = records
        .iter()
        .filter_map(|(revision, _, record)| match (revision, record) {
            (Some(revision), Record::ArchiveTransition(transition)) if *revision <= as_of => {
                Some((*revision, *transition))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    transitions.sort_by_key(|(revision, transition)| (*revision, transition.id()));
    let mut states = HashMap::<RecordRef, ArchiveState>::new();
    for (_, transition) in transitions {
        let target = archive_target_record_ref(transition.target());
        let prior = states
            .get(&target)
            .copied()
            .unwrap_or(ArchiveState::Unarchived);
        let next = prior
            .transition(transition.action())
            .map_err(|_| HistorySpaceTransferError::InvalidHistory)?;
        states.insert(target, next);
    }
    Ok(states)
}

fn archive_target_record_ref(target: ArchiveTargetRef) -> RecordRef {
    match target {
        ArchiveTargetRef::Assertion(id) => RecordRef::Assertion(id),
        ArchiveTargetRef::Mask(id) => RecordRef::Mask(id),
        ArchiveTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ArchiveTargetRef::Event(id) => RecordRef::Event(id),
        ArchiveTargetRef::EventMask(id) => RecordRef::EventMask(id),
        ArchiveTargetRef::EventRelation(id) => RecordRef::EventRelation(id),
        ArchiveTargetRef::Source(id) => RecordRef::Source(id),
        ArchiveTargetRef::Evidence(id) => RecordRef::Evidence(id),
        ArchiveTargetRef::Provenance(id) => RecordRef::Provenance(id),
        ArchiveTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        ArchiveTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ArchiveTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ArchiveTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ArchiveTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ArchiveTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ArchiveTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ArchiveTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ArchiveTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ArchiveTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        ArchiveTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ArchiveTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ArchiveTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ArchiveTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ArchiveTargetRef::TransferLineage(id) => RecordRef::TransferLineage(id),
    }
}

fn record_ref(record: &Record) -> Option<RecordRef> {
    Some(match record {
        Record::Assertion(value) => RecordRef::Assertion(value.id()),
        Record::AssertionValidityClosure(value) => RecordRef::AssertionValidityClosure(value.id()),
        Record::AssertionRetraction(value) => RecordRef::AssertionRetraction(value.id()),
        Record::Mask(value) => RecordRef::Mask(value.id()),
        Record::MaskValidityClosure(value) => RecordRef::MaskValidityClosure(value.id()),
        Record::MaskRetraction(value) => RecordRef::MaskRetraction(value.id()),
        Record::ReplacementBoundary(value) => RecordRef::ReplacementBoundary(value.id()),
        Record::ReplacementBoundaryValidityClosure(value) => {
            RecordRef::ReplacementBoundaryValidityClosure(value.id())
        }
        Record::ReplacementBoundaryRetraction(value) => {
            RecordRef::ReplacementBoundaryRetraction(value.id())
        }
        Record::ArchiveTransition(value) => RecordRef::ArchiveTransition(value.id()),
        Record::Event(value) => RecordRef::Event(value.id()),
        Record::EventMask(value) => RecordRef::EventMask(value.id()),
        Record::EventSpanClosure(value) => RecordRef::EventSpanClosure(value.id()),
        Record::EventRetraction(value) => RecordRef::EventRetraction(value.id()),
        Record::EventMaskRetraction(value) => RecordRef::EventMaskRetraction(value.id()),
        Record::EventRelation(value) => RecordRef::EventRelation(value.id()),
        Record::EventRelationRetraction(value) => RecordRef::EventRelationRetraction(value.id()),
        Record::Source(value) => RecordRef::Source(value.id()),
        Record::Evidence(value) => RecordRef::Evidence(value.id()),
        Record::EvidenceRetraction(value) => RecordRef::EvidenceRetraction(value.id()),
        Record::Provenance(value) => RecordRef::Provenance(value.id()),
        Record::ProvenanceRetraction(value) => RecordRef::ProvenanceRetraction(value.id()),
        Record::EntityRetirement(value) => {
            RecordRef::EntityRetirement(value.entity_retirement_id())
        }
        Record::PerspectiveRetirement(value) => {
            RecordRef::PerspectiveRetirement(value.perspective_retirement_id())
        }
        Record::TransferLineage(value) => RecordRef::TransferLineage(value.id()),
        _ => return None,
    })
}

fn record_revision(record: &Record) -> Option<Revision> {
    Some(match record {
        Record::HistorySpaceDefinition(_) => return None,
        Record::Entity(value) => value.created_revision(),
        Record::EntityRetirement(value) => value.created_revision(),
        Record::PerspectiveDefinitionRevision(value) => value.recorded_revision(),
        Record::PerspectiveRetirement(value) => value.created_revision(),
        Record::LayerDefinition(value) => value.created_revision().revision(),
        Record::LayerSchemaSnapshot(value) => value.revision().revision(),
        Record::EntityTypeDefinition(value) => value.created_revision(),
        Record::PredicateDefinition(value) => value.created_revision(),
        Record::EventKindDefinition(value) => value.created_revision(),
        Record::TimelineDefinition(value) => value.created_revision(),
        Record::TimeUnitDefinition(value) => value.created_revision(),
        Record::MigrationPlan(_)
        | Record::MigrationRun(_)
        | Record::MigrationStepCommitIdentity(_) => {
            return None;
        }
        Record::Assertion(value) => value.created_revision(),
        Record::AssertionValidityClosure(value) => value.created_revision(),
        Record::AssertionRetraction(value) => value.created_revision(),
        Record::Mask(value) => value.created_revision(),
        Record::MaskValidityClosure(value) => value.created_revision(),
        Record::MaskRetraction(value) => value.created_revision(),
        Record::ReplacementBoundary(value) => value.created_revision(),
        Record::ReplacementBoundaryValidityClosure(value) => value.created_revision(),
        Record::ReplacementBoundaryRetraction(value) => value.created_revision(),
        Record::ArchiveTransition(value) => value.created_revision(),
        Record::Event(value) => value.created_revision(),
        Record::EventMask(value) => value.created_revision(),
        Record::EventSpanClosure(value) => value.created_revision(),
        Record::EventRetraction(value) => value.created_revision(),
        Record::EventMaskRetraction(value) => value.created_revision(),
        Record::EventRelation(value) => value.created_revision(),
        Record::EventRelationRetraction(value) => value.created_revision(),
        Record::Source(value) => value.created_revision(),
        Record::Evidence(value) => value.created_revision(),
        Record::Provenance(value) => value.created_revision(),
        Record::EvidenceRetraction(value) => value.created_revision(),
        Record::ProvenanceRetraction(value) => value.created_revision(),
        Record::TransferLineage(value) => value.created_revision(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use worlddb_core::{
        Assertion, AssertionDraft, AssertionValidity, AuditAction, AuditCommitContext,
        AuditObjectClass, AuditOutcome, AuditPolicyFingerprint, AuditRecord, AuditRecordDetails,
        AuditRecordIdentity, AuditSequence, Capability, ContextKey, EpistemicMode,
        HistorySpaceContentRef, HistorySpaceDefinition, HistorySpaceId, LayerId, PerspectiveScope,
        Polarity, PolicyTarget, Record, Revision, SecurityPolicyVersion, Subject, TimeInterval,
        Timeline, Value,
    };

    use crate::{
        DatabaseLayout, HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference,
        ManifestStore, RecoveryManager, SecurityPolicyHistoryStore, StorageVerifier, WalPrepareLog,
    };

    use super::{FileHistorySpaceTransferManager, HistorySpaceTransferError};
    use crate::schema_management::tests::{TempArea, create_project, id};

    fn publish_seed_history(
        root: &std::path::Path,
        principal: worlddb_core::PrincipalId,
        records: &[Record],
        operation_tail: u8,
    ) -> Result<Revision, String> {
        let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "seed project manifest is missing".to_owned())?;
        let base = manifest.revision();
        let revision = base.next_commit().map_err(|error| error.to_string())?;
        let history_store = HistorySegmentStore::new(layout.clone());
        let history_receipt = history_store
            .stage_segment(&lock, records)
            .map_err(|error| error.to_string())?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            revision,
        );
        let policy_store = SecurityPolicyHistoryStore::new(layout.clone());
        let security_ids = manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
            .map(|reference| reference.id())
            .collect::<Vec<_>>();
        let policy_history = policy_store
            .load_history(base, &security_ids)
            .map_err(|error| error.to_string())?;
        let policy_version = policy_history
            .policy()
            .latest_version()
            .map_err(|error| error.to_string())?;
        let retention = policy_history
            .audit_retention_at(base)
            .map_err(|error| error.to_string())?;
        let next_policy = SecurityPolicyVersion::new(
            revision,
            policy_version.epoch(),
            policy_version.snapshot().clone(),
        );
        let security_receipt = policy_store
            .stage_version(&lock, &next_policy, None, retention)
            .map_err(|error| error.to_string())?;
        let security_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            security_receipt.id(),
            security_receipt.content_digest(),
            revision,
        );
        let sequence = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?
            .iter()
            .map(|entry| entry.record().sequence())
            .max()
            .unwrap_or(AuditSequence::new(0))
            .next()
            .map_err(|error| error.to_string())?;
        let fingerprint = policy_version
            .snapshot()
            .effective_capability_fingerprint(principal, PolicyTarget::default());
        let audit = AuditRecord::new(
            AuditRecordIdentity {
                record_id: id::<worlddb_core::AuditRecordId>(operation_tail.wrapping_add(1))?,
                sequence,
                audit_operation_id: id::<worlddb_core::AuditOperationId>(
                    operation_tail.wrapping_add(2),
                )?,
            },
            AuditRecordDetails {
                actor: principal,
                action: AuditAction::HistorySpaceTransfer,
                object_class: AuditObjectClass::Database,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision,
                    operation_id: id::<worlddb_core::OperationId>(operation_tail)?,
                },
                security_epoch: policy_version.epoch(),
                policy_fingerprint: AuditPolicyFingerprint::new(worlddb_core::Bytes::new(
                    fingerprint.to_vec(),
                ))
                .map_err(|error| error.to_string())?,
            },
        );
        let mut references = manifest.segments().to_vec();
        references.push(history_reference);
        references.push(security_reference);
        WalPrepareLog::new(&layout)
            .commit_audited_manifest_snapshot(
                &lock,
                id::<worlddb_core::OperationId>(operation_tail)?,
                references,
                &[history_reference, security_reference],
                &audit,
            )
            .map_err(|error| error.to_string())?;
        let recovery = RecoveryManager::new(layout.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        if !recovery.report().is_clean() {
            return Err("seed history did not recover cleanly".to_owned());
        }
        Ok(revision)
    }

    #[test]
    fn durable_transfer_rewrites_identity_owner_and_publishes_required_audit() -> Result<(), String>
    {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(
            &root,
            &[
                Capability::SchemaRead,
                Capability::HistorySpaceRead,
                Capability::HistorySpaceTransfer,
            ],
        )?;
        let source = id::<HistorySpaceId>(70)?;
        let target = id::<HistorySpaceId>(71)?;
        let layer = id::<LayerId>(72)?;
        let assertion_id = id::<worlddb_core::AssertionId>(73)?;
        let timeline = id::<worlddb_core::TimelineId>(74)?;
        let predicate = id::<worlddb_core::PredicateId>(75)?;
        let subject = id::<worlddb_core::EntityId>(76)?;
        let root_definition = HistorySpaceDefinition::new(source, None, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        let child_definition = HistorySpaceDefinition::new(target, Some(source), Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        let context = ContextKey::new(
            source,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )
        .map_err(|error| error.to_string())?;
        let validity = AssertionValidity::new(
            TimeInterval::new(Timeline::new(timeline), None, None)
                .map_err(|error| error.to_string())?,
        );
        let assertion = Assertion::new(
            assertion_id,
            AssertionDraft::new(
                context,
                Subject::new(subject),
                predicate,
                Value::String("transfer fixture".to_owned()),
                Polarity::Positive,
                validity,
            ),
            Revision::try_from(2_u64).map_err(|error| error.to_string())?,
        );
        let assertion_revision = publish_seed_history(
            &root,
            principal,
            &[
                Record::HistorySpaceDefinition(root_definition),
                Record::HistorySpaceDefinition(child_definition),
                Record::Assertion(assertion.clone()),
            ],
            80,
        )?;
        let retraction_id = id::<worlddb_core::AssertionRetractionId>(77)?;
        let source_revision = publish_seed_history(
            &root,
            principal,
            &[Record::AssertionRetraction(
                worlddb_core::AssertionRetraction::new(
                    retraction_id,
                    &assertion,
                    "fixture lifecycle".to_owned(),
                    assertion_revision
                        .next_commit()
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?,
            )],
            83,
        )?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut manager = FileHistorySpaceTransferManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let visible = manager
            .visible_content(source, target, source_revision)
            .map_err(|error| error.to_string())?;
        if visible.len() != 2
            || !visible.iter().any(|item| {
                item.identity() == HistorySpaceContentRef::Assertion(assertion_id)
                    && item.can_transfer()
            })
            || visible.iter().any(|item| {
                item.identity() == HistorySpaceContentRef::AssertionRetraction(retraction_id)
                    && item.can_transfer()
            })
        {
            return Err("source fixture record was not selectable".to_owned());
        }
        let plan = manager
            .build_plan(
                source,
                target,
                source_revision,
                BTreeSet::from([HistorySpaceContentRef::Assertion(assertion_id)]),
                BTreeSet::new(),
                worlddb_core::ExternalReferenceDecision::Reject,
            )
            .map_err(|error| error.to_string())?;
        let preview = manager.preview(&plan).map_err(|error| error.to_string())?;
        if preview.omitted_effective_lifecycle_records().len() != 1 {
            return Err(
                "transfer preview did not report the effective lifecycle omission".to_owned(),
            );
        }
        if !matches!(
            manager.commit(&plan, id::<worlddb_core::OperationId>(85)?, false),
            Err(HistorySpaceTransferError::AcknowledgementRequired)
        ) || manager.revision() != source_revision
        {
            return Err(
                "transfer published without acknowledging its lifecycle difference".to_owned(),
            );
        }
        let receipt = manager
            .commit(&plan, id::<worlddb_core::OperationId>(86)?, true)
            .map_err(|error| error.to_string())?;
        let copied_id = *receipt
            .record_id_map()
            .get(&HistorySpaceContentRef::Assertion(assertion_id))
            .ok_or_else(|| "transfer receipt omitted the assertion mapping".to_owned())?;
        if copied_id == HistorySpaceContentRef::Assertion(assertion_id)
            || receipt.revision() <= source_revision
        {
            return Err("transfer reused its source identity or revision".to_owned());
        }
        let copied = manager
            .visible_content(target, source, receipt.revision())
            .map_err(|error| error.to_string())?;
        if copied.len() != 1
            || copied
                .first()
                .is_none_or(|item| item.identity() != copied_id)
        {
            return Err("target HistorySpace does not contain the remapped copy".to_owned());
        }
        let source_after = manager
            .visible_content(source, target, receipt.revision())
            .map_err(|error| error.to_string())?;
        if source_after.len() != 2
            || !source_after
                .iter()
                .any(|item| item.identity() == HistorySpaceContentRef::Assertion(assertion_id))
            || !source_after.iter().any(|item| {
                item.identity() == HistorySpaceContentRef::AssertionRetraction(retraction_id)
            })
        {
            return Err("transfer changed source branch history".to_owned());
        }
        let audit = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|entry| entry.operation_id() == receipt.operation_id())
            .ok_or_else(|| "transfer Required Audit record is missing".to_owned())?;
        if audit.record().action() != AuditAction::HistorySpaceTransfer
            || audit.record().object_class() != AuditObjectClass::Database
            || audit.revision() != receipt.revision()
        {
            return Err("transfer audit does not match its shared publication".to_owned());
        }
        let verified = StorageVerifier::new(layout)
            .verify(&lock)
            .map_err(|error| error.to_string())?;
        if !verified.is_clean() {
            return Err("durable transfer failed storage verification".to_owned());
        }
        Ok(())
    }
}
