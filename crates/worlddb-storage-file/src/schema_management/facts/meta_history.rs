//! Persistent writes for project-wide Source, Evidence, and Provenance history.

use std::collections::BTreeSet;

use worlddb_core::{
    Capability, Evidence, EvidenceId, EvidenceRelation, EvidenceRetraction, EvidenceRetractionId,
    EvidenceTargetHistoryEntry, EvidenceTargetRef, FieldSelector, GraphValidationBudget,
    HistorySpaceId, PolicyTarget, ProvenanceEdge, ProvenanceEdgeHistory, ProvenanceEndpointRef,
    ProvenanceId, ProvenanceRelation, ProvenanceRetraction, ProvenanceRetractionId, Record,
    RecordRef, RecordedAsOf, RelationshipSelector, Source, SourceContentDigest,
    SourceEvidenceHistory, SourceId, SourceLocator, SourceMetadata, Symbol,
    validate_provenance_graph_transaction, validate_source_evidence_post_transaction,
};

use super::{
    FactManagementError, FactPublicationReceipt, FileFactManager, fact_record_ref,
    record_created_revision,
};

pub(super) struct AuthorizedMetaHistory {
    pub(super) sources: Vec<Source>,
    pub(super) evidence: Vec<Evidence>,
    pub(super) evidence_retractions: Vec<EvidenceRetraction>,
    pub(super) provenance: Vec<ProvenanceEdge>,
    pub(super) provenance_retractions: Vec<ProvenanceRetraction>,
    pub(super) provenance_endpoints: Vec<ProvenanceEndpointRef>,
}

/// Typed immutable Source input for one storage publication.
#[derive(Clone, Debug)]
pub struct SourceDraft {
    /// Stable identity allocated for the new Source.
    pub source_id: SourceId,
    /// Closed ASCII symbol describing the Source kind.
    pub source_kind: Symbol,
    /// Optional typed locator.
    pub locator: Option<SourceLocator>,
    /// Optional opaque content digest.
    pub content_digest: Option<SourceContentDigest>,
    /// Validated typed metadata entries.
    pub metadata: SourceMetadata,
}

/// Typed input for a replacement Source and its atomic `DerivedFrom` lineage.
#[derive(Clone, Debug)]
pub struct SourceSupersessionDraft {
    /// Complete typed replacement Source.
    pub replacement: SourceDraft,
    /// Stable identity allocated for the lineage edge.
    pub provenance_id: ProvenanceId,
    /// Existing immutable Source being superseded.
    pub superseded_source_id: SourceId,
}

pub(super) fn authorized_meta_history(
    manager: &FileFactManager<'_>,
    records: &[Record],
) -> Result<AuthorizedMetaHistory, FactManagementError> {
    let mut snapshot = AuthorizedMetaHistory {
        sources: Vec::new(),
        evidence: Vec::new(),
        evidence_retractions: Vec::new(),
        provenance: Vec::new(),
        provenance_retractions: Vec::new(),
        provenance_endpoints: Vec::new(),
    };
    for record in records {
        let Some(endpoint) = provenance_endpoint_for_record(record) else {
            continue;
        };
        let record_ref = record_ref_from_provenance_endpoint(endpoint);
        if !may_read_endpoint(manager, records, record_ref)? {
            continue;
        }
        snapshot.provenance_endpoints.push(endpoint);
        match record {
            Record::Source(value) => snapshot.sources.push(value.clone()),
            Record::Evidence(value) => snapshot.evidence.push(*value),
            Record::EvidenceRetraction(value) => snapshot.evidence_retractions.push(value.clone()),
            Record::Provenance(value) => snapshot.provenance.push(*value),
            Record::ProvenanceRetraction(value) => {
                snapshot.provenance_retractions.push(value.clone())
            }
            _ => {}
        }
    }
    snapshot.sources.sort_by_key(Source::id);
    snapshot.evidence.sort_by_key(|value| value.id());
    snapshot
        .evidence_retractions
        .sort_by_key(|value| value.id());
    snapshot.provenance.sort_by_key(|value| value.id());
    snapshot
        .provenance_retractions
        .sort_by_key(|value| value.id());
    snapshot.provenance_endpoints.sort();
    Ok(snapshot)
}

fn may_read_endpoint(
    manager: &FileFactManager<'_>,
    records: &[Record],
    record_ref: RecordRef,
) -> Result<bool, FactManagementError> {
    match authorize_record_read(manager, records, record_ref) {
        Ok(()) => Ok(true),
        Err(FactManagementError::Unauthorized(_) | FactManagementError::InvalidCandidate(_)) => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

impl FileFactManager<'_> {
    /// Creates an immutable Source with the caller's exact validated fields.
    pub fn create_source(
        &mut self,
        expected_base: worlddb_core::Revision,
        operation_id: worlddb_core::OperationId,
        draft: SourceDraft,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let SourceDraft {
            source_id,
            source_kind,
            locator,
            content_digest,
            metadata,
        } = draft;
        let record_ref = RecordRef::Source(source_id);
        self.ensure_identity_available(record_ref)?;
        authorize_source_write(self, source_id)?;
        let revision = self.next_revision()?;
        let source = Source::new(
            source_id,
            source_kind,
            locator,
            content_digest,
            metadata,
            revision,
        );
        self.publish(
            expected_base,
            operation_id,
            Record::Source(source),
            record_ref,
            source_policy_target(source_id),
        )
    }

    /// Creates a replacement Source and its explicit `DerivedFrom(old, new)`
    /// lineage in one commit. The prior Source remains immutable and visible.
    pub fn supersede_source(
        &mut self,
        expected_base: worlddb_core::Revision,
        operation_id: worlddb_core::OperationId,
        draft: SourceSupersessionDraft,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let SourceSupersessionDraft {
            replacement,
            provenance_id,
            superseded_source_id,
        } = draft;
        let SourceDraft {
            source_id,
            source_kind,
            locator,
            content_digest,
            metadata,
        } = replacement;
        let records = self.records_at_revision(expected_base)?;
        let old_source = records.iter().find_map(|record| match record {
            Record::Source(value) if value.id() == superseded_source_id => Some(value),
            _ => None,
        });
        let old_source = old_source.ok_or(FactManagementError::InvalidCandidate(
            "the selected Source is unavailable",
        ))?;
        authorize_source_read(self, old_source)?;
        self.authorize(
            Capability::SourceSupersede,
            source_policy_target(superseded_source_id),
        )?;
        authorize_source_write(self, source_id)?;
        for target in provenance_policy_targets(
            provenance_id,
            RecordRef::Source(superseded_source_id),
            RecordRef::Source(source_id),
            &records,
            ProvenanceRelation::DerivedFrom,
        ) {
            self.authorize(Capability::ProvenanceCreate, target)?;
            self.authorize(Capability::RelationshipCreate, target)?;
        }
        self.ensure_identity_available(RecordRef::Source(source_id))?;
        self.ensure_identity_available(RecordRef::Provenance(provenance_id))?;

        let revision = self.next_revision()?;
        let source = Source::new(
            source_id,
            source_kind,
            locator,
            content_digest,
            metadata,
            revision,
        );
        let lineage = ProvenanceEdge::new(
            provenance_id,
            ProvenanceEndpointRef::Source(superseded_source_id),
            ProvenanceEndpointRef::Source(source_id),
            ProvenanceRelation::DerivedFrom,
            revision,
        )
        .map_err(|_| {
            FactManagementError::InvalidCandidate("Source lineage is not valid; nothing was saved")
        })?;
        validate_provenance_state(
            &records,
            expected_base,
            revision,
            &[],
            &[lineage],
            &[Record::Source(source.clone())],
        )?;
        self.publish_batch(
            expected_base,
            operation_id,
            vec![Record::Source(source), Record::Provenance(lineage)],
            RecordRef::Source(source_id),
            source_policy_target(source_id),
        )
    }

    /// Creates a Source-to-record Evidence link after endpoint authorization
    /// and complete post-transaction duplicate/lifecycle validation.
    pub fn create_evidence(
        &mut self,
        expected_base: worlddb_core::Revision,
        operation_id: worlddb_core::OperationId,
        evidence_id: EvidenceId,
        source_id: SourceId,
        target: EvidenceTargetRef,
        relation: EvidenceRelation,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let source = records.iter().find_map(|record| match record {
            Record::Source(value) if value.id() == source_id => Some(value),
            _ => None,
        });
        let source = source.ok_or(FactManagementError::InvalidCandidate(
            "the selected Source or target is unavailable",
        ))?;
        let target_ref = record_ref_from_evidence_target(target);
        authorize_source_read(self, source)?;
        authorize_record_read(self, &records, target_ref)?;
        let policy_target = evidence_policy_target(evidence_id, target_ref, &records, relation);
        self.authorize(Capability::EvidenceCreate, policy_target)?;
        self.authorize(Capability::RelationshipCreate, policy_target)?;
        self.ensure_identity_available(RecordRef::Evidence(evidence_id))?;

        let revision = self.next_revision()?;
        let evidence = Evidence::new(evidence_id, source_id, target, relation, revision);
        let mut candidate_records = records.clone();
        candidate_records.push(Record::Evidence(evidence));
        validate_source_evidence_state(self, &candidate_records, &Record::Evidence(evidence))?;
        self.publish(
            expected_base,
            operation_id,
            Record::Evidence(evidence),
            RecordRef::Evidence(evidence_id),
            policy_target,
        )
    }

    /// Adds a typed Provenance relation after endpoint authorization and full
    /// mixed-relation cycle validation.
    pub fn create_provenance(
        &mut self,
        expected_base: worlddb_core::Revision,
        operation_id: worlddb_core::OperationId,
        provenance_id: ProvenanceId,
        from: ProvenanceEndpointRef,
        to: ProvenanceEndpointRef,
        relation: ProvenanceRelation,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let from_ref = record_ref_from_provenance_endpoint(from);
        let to_ref = record_ref_from_provenance_endpoint(to);
        let from_record = records
            .iter()
            .find(|record| fact_record_ref(record) == Some(from_ref));
        let to_record = records
            .iter()
            .find(|record| fact_record_ref(record) == Some(to_ref));
        let (Some(from_record), Some(to_record)) = (from_record, to_record) else {
            return Err(FactManagementError::InvalidCandidate(
                "a selected Provenance endpoint is unavailable",
            ));
        };
        authorize_record_read(self, &records, from_ref)?;
        authorize_record_read(self, &records, to_ref)?;
        let policy_target =
            provenance_policy_target(provenance_id, from_ref, to_ref, &records, relation);
        for target in provenance_policy_targets(provenance_id, from_ref, to_ref, &records, relation)
        {
            self.authorize(Capability::ProvenanceCreate, target)?;
            self.authorize(Capability::RelationshipCreate, target)?;
        }
        self.ensure_identity_available(RecordRef::Provenance(provenance_id))?;

        let revision = self.next_revision()?;
        let edge =
            ProvenanceEdge::new(provenance_id, from, to, relation, revision).map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "the selected relation does not permit these endpoints; nothing was saved",
                )
            })?;
        validate_corrects_pair(&edge, from_record, to_record)?;
        validate_provenance_state(&records, expected_base, revision, &[], &[edge], &[])?;
        self.publish(
            expected_base,
            operation_id,
            Record::Provenance(edge),
            RecordRef::Provenance(provenance_id),
            policy_target,
        )
    }

    /// Retracts one active Evidence edge as a separate later project revision.
    pub fn retract_evidence(
        &mut self,
        expected_base: worlddb_core::Revision,
        operation_id: worlddb_core::OperationId,
        retraction_id: EvidenceRetractionId,
        evidence_id: EvidenceId,
        reason: impl Into<String>,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let evidence = records.iter().find_map(|record| match record {
            Record::Evidence(value) if value.id() == evidence_id => Some(*value),
            _ => None,
        });
        let evidence = evidence.ok_or(FactManagementError::InvalidCandidate(
            "the selected Evidence is unavailable",
        ))?;
        authorize_record_read(self, &records, RecordRef::Evidence(evidence_id))?;
        let target = PolicyTarget::new(
            evidence_history_space(self, &records, evidence.target())?,
            None,
            Some(RecordRef::Evidence(evidence_id)),
            None,
            Some(RelationshipSelector::Evidence(evidence_relationship(
                evidence.relation(),
            ))),
        );
        self.authorize(Capability::EvidenceRetract, target)?;
        self.authorize(Capability::LifecycleRead, target)?;
        if records.iter().any(|record| {
            matches!(record, Record::EvidenceRetraction(value) if value.evidence_id() == evidence_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Evidence is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::EvidenceRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction = EvidenceRetraction::new(retraction_id, &evidence, reason, revision)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("Evidence retraction must follow its target")
            })?;
        let mut candidate_records = records.clone();
        candidate_records.push(Record::EvidenceRetraction(retraction.clone()));
        validate_source_evidence_state(self, &candidate_records, &Record::Evidence(evidence))?;
        self.publish(
            expected_base,
            operation_id,
            Record::EvidenceRetraction(retraction),
            RecordRef::EvidenceRetraction(retraction_id),
            target,
        )
    }

    /// Retracts one active Provenance edge as a separate later commit.
    pub fn retract_provenance(
        &mut self,
        expected_base: worlddb_core::Revision,
        operation_id: worlddb_core::OperationId,
        retraction_id: ProvenanceRetractionId,
        provenance_id: ProvenanceId,
        reason: impl Into<String>,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let edge = records.iter().find_map(|record| match record {
            Record::Provenance(value) if value.id() == provenance_id => Some(*value),
            _ => None,
        });
        let edge = edge.ok_or(FactManagementError::InvalidCandidate(
            "the selected Provenance edge is unavailable",
        ))?;
        authorize_record_read(self, &records, RecordRef::Provenance(provenance_id))?;
        let target = provenance_policy_target(
            provenance_id,
            record_ref_from_provenance_endpoint(edge.from()),
            record_ref_from_provenance_endpoint(edge.to()),
            &records,
            edge.relation(),
        );
        self.authorize(Capability::ProvenanceRetract, target)?;
        self.authorize(Capability::LifecycleRead, target)?;
        self.ensure_identity_available(RecordRef::ProvenanceRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction = ProvenanceRetraction::new(retraction_id, &edge, reason, revision)
            .map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "Provenance retraction must follow its target",
                )
            })?;
        validate_provenance_state(
            &records,
            expected_base,
            revision,
            &[retraction.clone()],
            &[],
            &[],
        )?;
        self.publish(
            expected_base,
            operation_id,
            Record::ProvenanceRetraction(retraction),
            RecordRef::ProvenanceRetraction(retraction_id),
            target,
        )
    }
}

fn authorize_source_write(
    manager: &FileFactManager<'_>,
    source_id: SourceId,
) -> Result<(), FactManagementError> {
    let record = RecordRef::Source(source_id);
    let target = source_policy_target(source_id);
    manager.authorize(Capability::SourceCreate, target)?;
    for field in source_fields() {
        manager.authorize(
            Capability::FieldWrite,
            PolicyTarget::new(None, None, Some(record), Some(field), None),
        )?;
    }
    Ok(())
}

fn authorize_source_read(
    manager: &FileFactManager<'_>,
    source: &Source,
) -> Result<(), FactManagementError> {
    let record = RecordRef::Source(source.id());
    manager.authorize(Capability::SourceRead, source_policy_target(source.id()))?;
    for field in source_fields() {
        manager.authorize(
            Capability::FieldRead,
            PolicyTarget::new(None, None, Some(record), Some(field), None),
        )?;
    }
    Ok(())
}

fn source_fields() -> [FieldSelector; 4] {
    [
        FieldSelector::SourceKind,
        FieldSelector::SourceLocator,
        FieldSelector::SourceContentDigest,
        FieldSelector::SourceMetadata,
    ]
}

fn source_policy_target(source_id: SourceId) -> PolicyTarget {
    PolicyTarget::new(None, None, Some(RecordRef::Source(source_id)), None, None)
}

fn validate_source_evidence_state(
    manager: &FileFactManager<'_>,
    records: &[Record],
    focus: &Record,
) -> Result<(), FactManagementError> {
    let metadata = manager
        .write_validation_metadata()
        .map_err(|_| FactManagementError::InvalidCandidate("Evidence validation failed"))?;
    let sources = records
        .iter()
        .filter_map(|record| match record {
            Record::Source(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let evidence = records
        .iter()
        .filter_map(|record| match record {
            Record::Evidence(value) => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    let retractions = records
        .iter()
        .filter_map(|record| match record {
            Record::EvidenceRetraction(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let targets = records
        .iter()
        .filter_map(|record| evidence_target_history_entry(record, records))
        .collect::<Vec<_>>();
    let root = metadata
        .history_spaces()
        .definitions()
        .iter()
        .find(|definition| definition.parent_history_space_id().is_none())
        .map(|definition| definition.history_space_id())
        .ok_or(FactManagementError::InvalidCandidate(
            "Evidence validation failed",
        ))?;
    let focus_target = match focus {
        Record::Evidence(value) => Some(record_ref_from_evidence_target(value.target())),
        Record::EvidenceRetraction(value) => records.iter().find_map(|record| match record {
            Record::Evidence(evidence) if evidence.id() == value.evidence_id() => {
                Some(record_ref_from_evidence_target(evidence.target()))
            }
            _ => None,
        }),
        _ => None,
    };
    let query_history_space = focus_target
        .and_then(|target| record_history_space_for_ref(target, records))
        .unwrap_or(root);
    validate_source_evidence_post_transaction(
        SourceEvidenceHistory::new(&sources, &evidence, &retractions, &targets),
        metadata.history_spaces(),
        query_history_space,
        records
            .iter()
            .filter_map(record_created_revision)
            .max()
            .unwrap_or(manager.revision()),
    )
    .map_err(|_| {
        FactManagementError::InvalidCandidate(
            "Evidence endpoints or active-tuple rules are invalid; nothing was saved",
        )
    })?;
    Ok(())
}

fn validate_provenance_state(
    records: &[Record],
    base_revision: worlddb_core::Revision,
    commit_revision: worlddb_core::Revision,
    retractions: &[ProvenanceRetraction],
    additions: &[ProvenanceEdge],
    extra_records: &[Record],
) -> Result<(), FactManagementError> {
    let edges = records
        .iter()
        .filter_map(|record| match record {
            Record::Provenance(value) => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    let old_retractions = records
        .iter()
        .filter_map(|record| match record {
            Record::ProvenanceRetraction(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let endpoints = records
        .iter()
        .chain(extra_records)
        .filter_map(provenance_endpoint_for_record)
        .collect::<BTreeSet<_>>();
    for edge in additions {
        if !endpoints.contains(&edge.from()) || !endpoints.contains(&edge.to()) {
            return Err(FactManagementError::InvalidCandidate(
                "a selected Provenance endpoint is unavailable",
            ));
        }
    }
    let history = ProvenanceEdgeHistory::new(&edges, &old_retractions);
    validate_provenance_graph_transaction(
        history,
        RecordedAsOf::from_published_revision(base_revision),
        commit_revision,
        retractions,
        additions,
        GraphValidationBudget::new(250_000),
    )
    .map_err(|_| {
        FactManagementError::InvalidCandidate(
            "Provenance graph is invalid or exceeds its validation budget; nothing was saved",
        )
    })?;
    Ok(())
}

fn validate_corrects_pair(
    edge: &ProvenanceEdge,
    from: &Record,
    to: &Record,
) -> Result<(), FactManagementError> {
    if edge.relation() != ProvenanceRelation::Corrects {
        return Ok(());
    }
    let from_revision = record_created_revision(from).ok_or(
        FactManagementError::InvalidCandidate("Corrects endpoints are unavailable"),
    )?;
    let to_revision = record_created_revision(to).ok_or(FactManagementError::InvalidCandidate(
        "Corrects endpoints are unavailable",
    ))?;
    edge.validate_corrects_transaction_order(from_revision, to_revision)
        .map_err(|_| {
            FactManagementError::InvalidCandidate(
                "Corrects must point from a later record to an earlier record",
            )
        })?;
    match (from, to) {
        (Record::Assertion(newer), Record::Assertion(older)) => {
            let left = newer.context();
            let right = older.context();
            if newer.subject() != older.subject()
                || newer.predicate_id() != older.predicate_id()
                || left.perspective_scope() != right.perspective_scope()
                || left.epistemic_mode() != right.epistemic_mode()
            {
                return Err(FactManagementError::InvalidCandidate(
                    "Corrects Assertions must occupy the same proposition slot",
                ));
            }
        }
        (Record::Event(newer), Record::Event(older))
            if newer.event_kind_id() != older.event_kind_id() =>
        {
            return Err(FactManagementError::InvalidCandidate(
                "Corrects Events must use the same EventKind",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn evidence_target_history_entry(
    record: &Record,
    records: &[Record],
) -> Option<EvidenceTargetHistoryEntry> {
    let endpoint = provenance_endpoint_for_record(record)?;
    let target = EvidenceTargetRef::try_from(record_ref_from_provenance_endpoint(endpoint)).ok()?;
    Some(EvidenceTargetHistoryEntry::new(
        target,
        record_created_revision(record)?,
        record_history_space(record, records),
    ))
}

fn provenance_endpoint_for_record(record: &Record) -> Option<ProvenanceEndpointRef> {
    Some(match record {
        Record::Assertion(value) => ProvenanceEndpointRef::Assertion(value.id()),
        Record::Mask(value) => ProvenanceEndpointRef::Mask(value.id()),
        Record::ReplacementBoundary(value) => {
            ProvenanceEndpointRef::ReplacementBoundary(value.id())
        }
        Record::Event(value) => ProvenanceEndpointRef::Event(value.id()),
        Record::EventMask(value) => ProvenanceEndpointRef::EventMask(value.id()),
        Record::Source(value) => ProvenanceEndpointRef::Source(value.id()),
        Record::Evidence(value) => ProvenanceEndpointRef::Evidence(value.id()),
        Record::Provenance(value) => ProvenanceEndpointRef::Provenance(value.id()),
        Record::AssertionValidityClosure(value) => {
            ProvenanceEndpointRef::AssertionValidityClosure(value.id())
        }
        Record::AssertionRetraction(value) => {
            ProvenanceEndpointRef::AssertionRetraction(value.id())
        }
        Record::MaskValidityClosure(value) => {
            ProvenanceEndpointRef::MaskValidityClosure(value.id())
        }
        Record::MaskRetraction(value) => ProvenanceEndpointRef::MaskRetraction(value.id()),
        Record::ReplacementBoundaryValidityClosure(value) => {
            ProvenanceEndpointRef::ReplacementBoundaryValidityClosure(value.id())
        }
        Record::ReplacementBoundaryRetraction(value) => {
            ProvenanceEndpointRef::ReplacementBoundaryRetraction(value.id())
        }
        Record::EventSpanClosure(value) => ProvenanceEndpointRef::EventSpanClosure(value.id()),
        Record::EventRetraction(value) => ProvenanceEndpointRef::EventRetraction(value.id()),
        Record::EventMaskRetraction(value) => {
            ProvenanceEndpointRef::EventMaskRetraction(value.id())
        }
        Record::EventRelationRetraction(value) => {
            ProvenanceEndpointRef::EventRelationRetraction(value.id())
        }
        Record::EvidenceRetraction(value) => ProvenanceEndpointRef::EvidenceRetraction(value.id()),
        Record::ProvenanceRetraction(value) => {
            ProvenanceEndpointRef::ProvenanceRetraction(value.id())
        }
        Record::EntityRetirement(value) => {
            ProvenanceEndpointRef::EntityRetirement(value.entity_retirement_id())
        }
        Record::PerspectiveRetirement(value) => {
            ProvenanceEndpointRef::PerspectiveRetirement(value.perspective_retirement_id())
        }
        Record::ArchiveTransition(value) => ProvenanceEndpointRef::ArchiveTransition(value.id()),
        _ => return None,
    })
}

fn record_ref_from_provenance_endpoint(endpoint: ProvenanceEndpointRef) -> RecordRef {
    match endpoint {
        ProvenanceEndpointRef::Assertion(id) => RecordRef::Assertion(id),
        ProvenanceEndpointRef::Mask(id) => RecordRef::Mask(id),
        ProvenanceEndpointRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ProvenanceEndpointRef::Event(id) => RecordRef::Event(id),
        ProvenanceEndpointRef::EventMask(id) => RecordRef::EventMask(id),
        ProvenanceEndpointRef::Source(id) => RecordRef::Source(id),
        ProvenanceEndpointRef::Evidence(id) => RecordRef::Evidence(id),
        ProvenanceEndpointRef::Provenance(id) => RecordRef::Provenance(id),
        ProvenanceEndpointRef::AssertionValidityClosure(id) => {
            RecordRef::AssertionValidityClosure(id)
        }
        ProvenanceEndpointRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ProvenanceEndpointRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ProvenanceEndpointRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ProvenanceEndpointRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ProvenanceEndpointRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ProvenanceEndpointRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ProvenanceEndpointRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ProvenanceEndpointRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ProvenanceEndpointRef::EventRelationRetraction(id) => {
            RecordRef::EventRelationRetraction(id)
        }
        ProvenanceEndpointRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ProvenanceEndpointRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ProvenanceEndpointRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ProvenanceEndpointRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ProvenanceEndpointRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn record_ref_from_evidence_target(target: EvidenceTargetRef) -> RecordRef {
    match target {
        EvidenceTargetRef::Assertion(id) => RecordRef::Assertion(id),
        EvidenceTargetRef::Mask(id) => RecordRef::Mask(id),
        EvidenceTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        EvidenceTargetRef::Event(id) => RecordRef::Event(id),
        EvidenceTargetRef::EventMask(id) => RecordRef::EventMask(id),
        EvidenceTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        EvidenceTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        EvidenceTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        EvidenceTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        EvidenceTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        EvidenceTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        EvidenceTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        EvidenceTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        EvidenceTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        EvidenceTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        EvidenceTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        EvidenceTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        EvidenceTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        EvidenceTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        EvidenceTargetRef::Provenance(id) => RecordRef::Provenance(id),
        EvidenceTargetRef::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

fn evidence_policy_target(
    evidence_id: EvidenceId,
    target_ref: RecordRef,
    records: &[Record],
    relation: EvidenceRelation,
) -> PolicyTarget {
    PolicyTarget::new(
        record_history_space_for_ref(target_ref, records),
        None,
        Some(RecordRef::Evidence(evidence_id)),
        None,
        Some(RelationshipSelector::Evidence(evidence_relationship(
            relation,
        ))),
    )
}

fn provenance_policy_target(
    provenance_id: ProvenanceId,
    from_ref: RecordRef,
    to_ref: RecordRef,
    records: &[Record],
    relation: ProvenanceRelation,
) -> PolicyTarget {
    let history_space = record_history_space_for_ref(from_ref, records)
        .or_else(|| record_history_space_for_ref(to_ref, records));
    PolicyTarget::new(
        history_space,
        None,
        Some(RecordRef::Provenance(provenance_id)),
        None,
        Some(RelationshipSelector::Provenance(provenance_relationship(
            relation,
        ))),
    )
}

fn provenance_policy_targets(
    provenance_id: ProvenanceId,
    from_ref: RecordRef,
    to_ref: RecordRef,
    records: &[Record],
    relation: ProvenanceRelation,
) -> Vec<PolicyTarget> {
    let from_scope = record_scope_for_ref(from_ref, records);
    let to_scope = record_scope_for_ref(to_ref, records);
    let mut scopes = BTreeSet::new();
    if let Some(scope) = from_scope {
        scopes.insert(Some(scope.0));
    } else {
        scopes.insert(None);
    }
    if let Some(scope) = to_scope {
        scopes.insert(Some(scope.0));
    } else {
        scopes.insert(None);
    }
    scopes
        .into_iter()
        .map(|history_space| {
            PolicyTarget::new(
                history_space,
                None,
                Some(RecordRef::Provenance(provenance_id)),
                None,
                Some(RelationshipSelector::Provenance(provenance_relationship(
                    relation,
                ))),
            )
        })
        .collect()
}

fn record_scope_for_ref(
    record_ref: RecordRef,
    records: &[Record],
) -> Option<(HistorySpaceId, worlddb_core::LayerId)> {
    records
        .iter()
        .find(|record| fact_record_ref(record) == Some(record_ref))
        .and_then(|record| record_scope(record, records))
}

fn evidence_relationship(value: EvidenceRelation) -> worlddb_core::EvidenceRelationship {
    match value {
        EvidenceRelation::Supports => worlddb_core::EvidenceRelationship::Supports,
        EvidenceRelation::Contradicts => worlddb_core::EvidenceRelationship::Contradicts,
        EvidenceRelation::Documents => worlddb_core::EvidenceRelationship::Documents,
    }
}

fn provenance_relationship(value: ProvenanceRelation) -> worlddb_core::ProvenanceRelationship {
    match value {
        ProvenanceRelation::Corrects => worlddb_core::ProvenanceRelationship::Corrects,
        ProvenanceRelation::DerivedFrom => worlddb_core::ProvenanceRelationship::DerivedFrom,
        ProvenanceRelation::ResultedFrom => worlddb_core::ProvenanceRelationship::ResultedFrom,
    }
}

fn evidence_history_space(
    manager: &FileFactManager<'_>,
    records: &[Record],
    target: EvidenceTargetRef,
) -> Result<Option<HistorySpaceId>, FactManagementError> {
    let record_ref = record_ref_from_evidence_target(target);
    Ok(
        record_history_space_for_ref(record_ref, records).or_else(|| {
            manager
                .write_validation_metadata()
                .ok()
                .and_then(|metadata| {
                    metadata
                        .history_spaces()
                        .definitions()
                        .iter()
                        .find(|definition| definition.parent_history_space_id().is_none())
                        .map(|definition| definition.history_space_id())
                })
        }),
    )
}

fn authorize_record_read(
    manager: &FileFactManager<'_>,
    records: &[Record],
    record_ref: RecordRef,
) -> Result<(), FactManagementError> {
    authorize_record_read_inner(manager, records, record_ref, &mut BTreeSet::new())
}

fn authorize_record_read_inner(
    manager: &FileFactManager<'_>,
    records: &[Record],
    record_ref: RecordRef,
    visited: &mut BTreeSet<RecordRef>,
) -> Result<(), FactManagementError> {
    if !visited.insert(record_ref) || visited.len() > 1_024 {
        return Err(FactManagementError::InvalidCandidate(
            "selected endpoint unavailable",
        ));
    }
    let record = records
        .iter()
        .find(|record| fact_record_ref(record) == Some(record_ref))
        .ok_or(FactManagementError::InvalidCandidate(
            "selected endpoint unavailable",
        ))?;
    if let Some(parent) = lifecycle_parent_ref(record, records) {
        authorize_record_read_inner(manager, records, parent, visited)?;
    }
    match record {
        Record::Evidence(value) => {
            authorize_record_read_inner(
                manager,
                records,
                RecordRef::Source(value.source_id()),
                visited,
            )?;
            authorize_record_read_inner(
                manager,
                records,
                record_ref_from_evidence_target(value.target()),
                visited,
            )?;
        }
        Record::Provenance(value) => {
            authorize_record_read_inner(
                manager,
                records,
                record_ref_from_provenance_endpoint(value.from()),
                visited,
            )?;
            authorize_record_read_inner(
                manager,
                records,
                record_ref_from_provenance_endpoint(value.to()),
                visited,
            )?;
        }
        Record::EventRelation(value) => {
            authorize_record_read_inner(
                manager,
                records,
                RecordRef::Event(value.from_event()),
                visited,
            )?;
            authorize_record_read_inner(
                manager,
                records,
                RecordRef::Event(value.to_event()),
                visited,
            )?;
        }
        _ => {}
    }
    let history_space = record_history_space_for_ref(record_ref, records);
    let layer = record_layer_for_ref(record_ref, records);
    let target = PolicyTarget::new(history_space, layer, Some(record_ref), None, None);
    if let Record::Evidence(evidence) = record {
        authorize_meta_edge(
            manager,
            evidence_policy_target(
                evidence.id(),
                record_ref_from_evidence_target(evidence.target()),
                records,
                evidence.relation(),
            ),
            [
                Capability::EvidenceRead,
                Capability::RelationshipRead,
                Capability::LifecycleRead,
            ],
        )?;
    } else if let Record::Provenance(edge) = record {
        authorize_provenance_edge(manager, records, edge, Capability::ProvenanceRead)?;
        authorize_provenance_edge(manager, records, edge, Capability::RelationshipRead)?;
        authorize_provenance_edge(manager, records, edge, Capability::LifecycleRead)?;
    } else if let Record::EventRelation(relation) = record {
        manager.authorize(
            Capability::RelationshipRead,
            super::event_relation_target(relation),
        )?;
    } else if let Record::Source(source) = record {
        manager.authorize(Capability::SourceRead, source_policy_target(source.id()))?;
        for field in source_fields() {
            manager.authorize(
                Capability::FieldRead,
                PolicyTarget::new(None, None, Some(record_ref), Some(field), None),
            )?;
        }
    } else if let Some(capability) = record_read_capability(record_ref) {
        manager.authorize(capability, target)?;
    } else {
        return Err(FactManagementError::InvalidCandidate(
            "selected endpoint unavailable",
        ));
    }
    if let Some(history_space) = history_space {
        manager.authorize(
            Capability::HistorySpaceRead,
            PolicyTarget::new(Some(history_space), layer, Some(record_ref), None, None),
        )?;
    }
    if let Some(layer) = layer {
        manager.authorize(
            Capability::LayerRead,
            PolicyTarget::new(history_space, Some(layer), Some(record_ref), None, None),
        )?;
    }
    visited.remove(&record_ref);
    Ok(())
}

fn authorize_meta_edge(
    manager: &FileFactManager<'_>,
    target: PolicyTarget,
    capabilities: [Capability; 3],
) -> Result<(), FactManagementError> {
    for capability in capabilities {
        manager.authorize(capability, target)?;
    }
    Ok(())
}

fn authorize_provenance_edge(
    manager: &FileFactManager<'_>,
    records: &[Record],
    edge: &ProvenanceEdge,
    capability: Capability,
) -> Result<(), FactManagementError> {
    for target in provenance_policy_targets(
        edge.id(),
        record_ref_from_provenance_endpoint(edge.from()),
        record_ref_from_provenance_endpoint(edge.to()),
        records,
        edge.relation(),
    ) {
        manager.authorize(capability, target)?;
    }
    Ok(())
}

fn lifecycle_parent_ref(record: &Record, records: &[Record]) -> Option<RecordRef> {
    match record {
        Record::AssertionValidityClosure(value) => Some(RecordRef::Assertion(value.assertion_id())),
        Record::AssertionRetraction(value) => Some(RecordRef::Assertion(value.assertion_id())),
        Record::MaskValidityClosure(value) => Some(RecordRef::Mask(value.mask_id())),
        Record::MaskRetraction(value) => Some(RecordRef::Mask(value.mask_id())),
        Record::ReplacementBoundaryValidityClosure(value) => Some(RecordRef::ReplacementBoundary(
            value.replacement_boundary_id(),
        )),
        Record::ReplacementBoundaryRetraction(value) => Some(RecordRef::ReplacementBoundary(
            value.replacement_boundary_id(),
        )),
        Record::EventSpanClosure(value) => Some(RecordRef::Event(value.event_id())),
        Record::EventRetraction(value) => Some(RecordRef::Event(value.event_id())),
        Record::EventMaskRetraction(value) => Some(RecordRef::EventMask(value.event_mask_id())),
        Record::EventRelationRetraction(value) => {
            Some(RecordRef::EventRelation(value.event_relation_id()))
        }
        Record::EvidenceRetraction(value) => Some(RecordRef::Evidence(value.evidence_id())),
        Record::ProvenanceRetraction(value) => Some(RecordRef::Provenance(value.provenance_id())),
        Record::ArchiveTransition(value) => super::archive_target_record_ref(value.target()),
        _ => {
            let _ = records;
            None
        }
    }
}

fn record_read_capability(record_ref: RecordRef) -> Option<Capability> {
    Some(match record_ref {
        RecordRef::Assertion(_) => Capability::AssertionRead,
        RecordRef::Mask(_) => Capability::MaskRead,
        RecordRef::ReplacementBoundary(_) => Capability::ReplacementBoundaryRead,
        RecordRef::Event(_) => Capability::EventRead,
        RecordRef::EventMask(_) => Capability::EventMaskRead,
        RecordRef::Source(_) => Capability::SourceRead,
        RecordRef::Evidence(_) => Capability::EvidenceRead,
        RecordRef::Provenance(_) => Capability::ProvenanceRead,
        RecordRef::EventRelation(_) => Capability::RelationshipRead,
        RecordRef::AssertionValidityClosure(_)
        | RecordRef::AssertionRetraction(_)
        | RecordRef::MaskValidityClosure(_)
        | RecordRef::MaskRetraction(_)
        | RecordRef::ReplacementBoundaryValidityClosure(_)
        | RecordRef::ReplacementBoundaryRetraction(_)
        | RecordRef::EventSpanClosure(_)
        | RecordRef::EventRetraction(_)
        | RecordRef::EventMaskRetraction(_)
        | RecordRef::EventRelationRetraction(_)
        | RecordRef::EvidenceRetraction(_)
        | RecordRef::ProvenanceRetraction(_)
        | RecordRef::EntityRetirement(_)
        | RecordRef::PerspectiveRetirement(_)
        | RecordRef::ArchiveTransition(_) => Capability::LifecycleRead,
        RecordRef::TransferLineage(_) => return None,
    })
}

fn record_history_space_for_ref(
    record_ref: RecordRef,
    records: &[Record],
) -> Option<HistorySpaceId> {
    records
        .iter()
        .find(|record| fact_record_ref(record) == Some(record_ref))
        .and_then(|record| record_history_space(record, records))
}

fn record_layer_for_ref(
    record_ref: RecordRef,
    records: &[Record],
) -> Option<worlddb_core::LayerId> {
    records
        .iter()
        .find(|record| fact_record_ref(record) == Some(record_ref))
        .and_then(|record| record_scope(record, records))
        .map(|(_, layer)| layer)
}

fn record_history_space(record: &Record, records: &[Record]) -> Option<HistorySpaceId> {
    record_scope(record, records).map(|(history_space, _)| history_space)
}

fn record_scope(
    record: &Record,
    records: &[Record],
) -> Option<(HistorySpaceId, worlddb_core::LayerId)> {
    match record {
        Record::Assertion(value) => Some((value.context().history_space_id(), value.context().layer_id())),
        Record::Mask(value) => Some((value.context().history_space_id(), value.context().layer_id())),
        Record::ReplacementBoundary(value) => Some((value.context().history_space_id(), value.context().layer_id())),
        Record::Event(value) => Some((value.history_space_id(), value.layer_id())),
        Record::EventMask(value) => Some((value.history_space_id(), value.layer_id())),
        Record::AssertionRetraction(value) => find_scope(records, |candidate| matches!(candidate, Record::Assertion(target) if target.id() == value.assertion_id())),
        Record::AssertionValidityClosure(value) => find_scope(records, |candidate| matches!(candidate, Record::Assertion(target) if target.id() == value.assertion_id())),
        Record::MaskRetraction(value) => find_scope(records, |candidate| matches!(candidate, Record::Mask(target) if target.id() == value.mask_id())),
        Record::MaskValidityClosure(value) => find_scope(records, |candidate| matches!(candidate, Record::Mask(target) if target.id() == value.mask_id())),
        Record::ReplacementBoundaryRetraction(value) => find_scope(records, |candidate| matches!(candidate, Record::ReplacementBoundary(target) if target.id() == value.replacement_boundary_id())),
        Record::ReplacementBoundaryValidityClosure(value) => find_scope(records, |candidate| matches!(candidate, Record::ReplacementBoundary(target) if target.id() == value.replacement_boundary_id())),
        Record::EventRetraction(value) => find_scope(records, |candidate| matches!(candidate, Record::Event(target) if target.id() == value.event_id())),
        Record::EventSpanClosure(value) => find_scope(records, |candidate| matches!(candidate, Record::Event(target) if target.id() == value.event_id())),
        Record::EventMaskRetraction(value) => find_scope(records, |candidate| matches!(candidate, Record::EventMask(target) if target.id() == value.event_mask_id())),
        Record::EventRelationRetraction(value) => records.iter().find_map(|candidate| match candidate {
            Record::EventRelation(target) if target.id() == value.event_relation_id() => find_scope(records, |event| matches!(event, Record::Event(event) if event.id() == target.from_event())),
            _ => None,
        }),
        Record::ArchiveTransition(value) => {
            let target = super::archive_target_record_ref(value.target())?;
            records.iter().find(|record| fact_record_ref(record) == Some(target)).and_then(|record| record_scope(record, records))
        }
        _ => None,
    }
}

fn find_scope(
    records: &[Record],
    predicate: impl Fn(&Record) -> bool,
) -> Option<(HistorySpaceId, worlddb_core::LayerId)> {
    records
        .iter()
        .find(|record| predicate(record))
        .and_then(|record| record_scope(record, records))
}
