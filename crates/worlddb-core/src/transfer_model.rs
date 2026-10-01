//! Closed, typed plans for same-database HistorySpace content transfer.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ids::{
    AssertionId, AssertionRetractionId, AssertionValidityClosureId, DatabaseId, DomainId, EventId,
    EventMaskId, EventMaskRetractionId, EventRelationId, EventRetractionId, EventSpanClosureId,
    HistorySpaceId, MaskId, MaskRetractionId, MaskValidityClosureId, ReplacementBoundaryId,
    ReplacementBoundaryRetractionId, ReplacementBoundaryValidityClosureId, Revision,
    TransferLineageId,
};
use crate::record_refs::RecordRef;
use crate::wire_records::encode_record_ref;

/// A closed typed reference to a HistorySpace-owned record that can be copied.
///
/// Project-wide EventRelation and EventRelationRetraction records are
/// deliberately excluded; transfer plans map those through their separate
/// EventRelation identity map.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HistorySpaceContentRef {
    /// An Assertion record.
    Assertion(AssertionId),
    /// A Mask record.
    Mask(MaskId),
    /// A ReplacementBoundary record.
    ReplacementBoundary(ReplacementBoundaryId),
    /// An Event record.
    Event(EventId),
    /// An EventMask record.
    EventMask(EventMaskId),
    /// An AssertionValidityClosure record.
    AssertionValidityClosure(AssertionValidityClosureId),
    /// An AssertionRetraction record.
    AssertionRetraction(AssertionRetractionId),
    /// A MaskValidityClosure record.
    MaskValidityClosure(MaskValidityClosureId),
    /// A MaskRetraction record.
    MaskRetraction(MaskRetractionId),
    /// A ReplacementBoundaryValidityClosure record.
    ReplacementBoundaryValidityClosure(ReplacementBoundaryValidityClosureId),
    /// A ReplacementBoundaryRetraction record.
    ReplacementBoundaryRetraction(ReplacementBoundaryRetractionId),
    /// An EventSpanClosure record.
    EventSpanClosure(EventSpanClosureId),
    /// An EventRetraction record.
    EventRetraction(EventRetractionId),
    /// An EventMaskRetraction record.
    EventMaskRetraction(EventMaskRetractionId),
}

impl HistorySpaceContentRef {
    /// Returns the exact closed `RecordRef` variant for this content identity.
    #[must_use]
    pub const fn record_ref(self) -> RecordRef {
        match self {
            Self::Assertion(id) => RecordRef::Assertion(id),
            Self::Mask(id) => RecordRef::Mask(id),
            Self::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
            Self::Event(id) => RecordRef::Event(id),
            Self::EventMask(id) => RecordRef::EventMask(id),
            Self::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
            Self::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
            Self::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
            Self::MaskRetraction(id) => RecordRef::MaskRetraction(id),
            Self::ReplacementBoundaryValidityClosure(id) => {
                RecordRef::ReplacementBoundaryValidityClosure(id)
            }
            Self::ReplacementBoundaryRetraction(id) => RecordRef::ReplacementBoundaryRetraction(id),
            Self::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
            Self::EventRetraction(id) => RecordRef::EventRetraction(id),
            Self::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        }
    }

    const fn family(self) -> HistorySpaceContentFamily {
        match self {
            Self::Assertion(_) => HistorySpaceContentFamily::Assertion,
            Self::Mask(_) => HistorySpaceContentFamily::Mask,
            Self::ReplacementBoundary(_) => HistorySpaceContentFamily::ReplacementBoundary,
            Self::Event(_) => HistorySpaceContentFamily::Event,
            Self::EventMask(_) => HistorySpaceContentFamily::EventMask,
            Self::AssertionValidityClosure(_) => {
                HistorySpaceContentFamily::AssertionValidityClosure
            }
            Self::AssertionRetraction(_) => HistorySpaceContentFamily::AssertionRetraction,
            Self::MaskValidityClosure(_) => HistorySpaceContentFamily::MaskValidityClosure,
            Self::MaskRetraction(_) => HistorySpaceContentFamily::MaskRetraction,
            Self::ReplacementBoundaryValidityClosure(_) => {
                HistorySpaceContentFamily::ReplacementBoundaryValidityClosure
            }
            Self::ReplacementBoundaryRetraction(_) => {
                HistorySpaceContentFamily::ReplacementBoundaryRetraction
            }
            Self::EventSpanClosure(_) => HistorySpaceContentFamily::EventSpanClosure,
            Self::EventRetraction(_) => HistorySpaceContentFamily::EventRetraction,
            Self::EventMaskRetraction(_) => HistorySpaceContentFamily::EventMaskRetraction,
        }
    }
}

impl TryFrom<RecordRef> for HistorySpaceContentRef {
    type Error = HistorySpaceContentRefError;

    fn try_from(record_ref: RecordRef) -> Result<Self, Self::Error> {
        match record_ref {
            RecordRef::Assertion(id) => Ok(Self::Assertion(id)),
            RecordRef::Mask(id) => Ok(Self::Mask(id)),
            RecordRef::ReplacementBoundary(id) => Ok(Self::ReplacementBoundary(id)),
            RecordRef::Event(id) => Ok(Self::Event(id)),
            RecordRef::EventMask(id) => Ok(Self::EventMask(id)),
            RecordRef::AssertionValidityClosure(id) => Ok(Self::AssertionValidityClosure(id)),
            RecordRef::AssertionRetraction(id) => Ok(Self::AssertionRetraction(id)),
            RecordRef::MaskValidityClosure(id) => Ok(Self::MaskValidityClosure(id)),
            RecordRef::MaskRetraction(id) => Ok(Self::MaskRetraction(id)),
            RecordRef::ReplacementBoundaryValidityClosure(id) => {
                Ok(Self::ReplacementBoundaryValidityClosure(id))
            }
            RecordRef::ReplacementBoundaryRetraction(id) => {
                Ok(Self::ReplacementBoundaryRetraction(id))
            }
            RecordRef::EventSpanClosure(id) => Ok(Self::EventSpanClosure(id)),
            RecordRef::EventRetraction(id) => Ok(Self::EventRetraction(id)),
            RecordRef::EventMaskRetraction(id) => Ok(Self::EventMaskRetraction(id)),
            unsupported => Err(HistorySpaceContentRefError { unsupported }),
        }
    }
}

/// Persistent typed lineage for a record copied between HistorySpaces.
///
/// This is a dedicated transfer record, not a Provenance endpoint. It keeps
/// copy lineage available for source families excluded by the closed
/// `DerivedFrom` matrix.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TransferLineage {
    id: TransferLineageId,
    source_history_space_id: HistorySpaceId,
    target_history_space_id: HistorySpaceId,
    source: HistorySpaceContentRef,
    target: HistorySpaceContentRef,
    created_revision: Revision,
}

impl TransferLineage {
    /// Creates a lineage record for two distinct identities of the same family.
    pub fn new(
        id: TransferLineageId,
        source_history_space_id: HistorySpaceId,
        target_history_space_id: HistorySpaceId,
        source: HistorySpaceContentRef,
        target: HistorySpaceContentRef,
        created_revision: Revision,
    ) -> Result<Self, TransferLineageError> {
        if source_history_space_id == target_history_space_id
            || source == target
            || source.family() != target.family()
        {
            return Err(TransferLineageError::InvalidIdentityPair);
        }
        Ok(Self {
            id,
            source_history_space_id,
            target_history_space_id,
            source,
            target,
            created_revision,
        })
    }

    /// Returns this record's persistent identity.
    #[must_use]
    pub const fn id(self) -> TransferLineageId {
        self.id
    }
    /// Returns the source HistorySpace.
    #[must_use]
    pub const fn source_history_space_id(self) -> HistorySpaceId {
        self.source_history_space_id
    }
    /// Returns the destination HistorySpace.
    #[must_use]
    pub const fn target_history_space_id(self) -> HistorySpaceId {
        self.target_history_space_id
    }
    /// Returns the exact source record identity.
    #[must_use]
    pub const fn source(self) -> HistorySpaceContentRef {
        self.source
    }
    /// Returns the exact copied record identity.
    #[must_use]
    pub const fn target(self) -> HistorySpaceContentRef {
        self.target
    }
    /// Returns the shared transaction revision.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// Invalid source/copy identity relationship for a TransferLineage record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferLineageError {
    /// Source and target must be distinct identities of one record family.
    InvalidIdentityPair,
}

impl fmt::Display for TransferLineageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "TransferLineage requires distinct source and target IDs of the same record family",
        )
    }
}

impl std::error::Error for TransferLineageError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistorySpaceContentFamily {
    Assertion,
    Mask,
    ReplacementBoundary,
    Event,
    EventMask,
    AssertionValidityClosure,
    AssertionRetraction,
    MaskValidityClosure,
    MaskRetraction,
    ReplacementBoundaryValidityClosure,
    ReplacementBoundaryRetraction,
    EventSpanClosure,
    EventRetraction,
    EventMaskRetraction,
}

/// A `RecordRef` family that is not copyable as HistorySpace-owned content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistorySpaceContentRefError {
    unsupported: RecordRef,
}

impl HistorySpaceContentRefError {
    /// Returns the reference rejected by the closed content subset.
    #[must_use]
    pub const fn unsupported(self) -> RecordRef {
        self.unsupported
    }
}

impl fmt::Display for HistorySpaceContentRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} is not HistorySpace-owned transferable content",
            self.unsupported.variant_name()
        )
    }
}

impl std::error::Error for HistorySpaceContentRefError {}

/// Immutable, validated identity mapping for an explicit HistorySpace transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPlan {
    database_id: DatabaseId,
    source_history_space_id: HistorySpaceId,
    source_recorded_as_of: Revision,
    target_history_space_id: HistorySpaceId,
    expected_target_head: Revision,
    selected_records: BTreeSet<HistorySpaceContentRef>,
    record_id_map: BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef>,
    selected_event_relations: BTreeSet<EventRelationId>,
    event_relation_id_map: BTreeMap<EventRelationId, EventRelationId>,
    external_reference_decisions: Vec<(RecordRef, ExternalReferenceDecision)>,
    lifecycle_policy: TransferLifecyclePolicy,
    archive_policy: TransferArchivePolicy,
    plan_fingerprint: [u8; 32],
}

/// Complete caller-supplied data for one immutable transfer plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPlanSpec {
    /// The source and destination database identity.
    pub database_id: DatabaseId,
    /// The pinned source HistorySpace.
    pub source_history_space_id: HistorySpaceId,
    /// The exact source snapshot revision.
    pub source_recorded_as_of: Revision,
    /// The existing destination HistorySpace.
    pub target_history_space_id: HistorySpaceId,
    /// The destination head observed during planning.
    pub expected_target_head: Revision,
    /// The exact nonempty set of content records to copy.
    pub selected_records: BTreeSet<HistorySpaceContentRef>,
    /// The complete same-family source-to-copy content identity map.
    pub record_id_map: BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef>,
    /// Project-wide EventRelations explicitly selected for transfer.
    pub selected_event_relations: BTreeSet<EventRelationId>,
    /// The separate typed EventRelation identity map.
    pub event_relation_id_map: BTreeMap<EventRelationId, EventRelationId>,
    /// Decisions for references outside the selected content set.
    pub external_reference_decisions: Vec<(RecordRef, ExternalReferenceDecision)>,
    /// Policy for effective Closure and Retraction records.
    pub lifecycle_policy: TransferLifecyclePolicy,
    /// Policy for the source records' operational archive state.
    pub archive_policy: TransferArchivePolicy,
}

impl TransferPlan {
    /// Builds a plan whose ID map covers exactly the selected records and
    /// preserves every concrete record family.
    pub fn new(mut spec: TransferPlanSpec) -> Result<Self, TransferPlanError> {
        if spec.source_history_space_id == spec.target_history_space_id {
            return Err(TransferPlanError::SameSourceAndTarget);
        }
        if spec.selected_records.is_empty() {
            return Err(TransferPlanError::EmptySelection);
        }
        for source in &spec.selected_records {
            if !spec.record_id_map.contains_key(source) {
                return Err(TransferPlanError::MissingMapping { source: *source });
            }
        }
        for source in spec.record_id_map.keys() {
            if !spec.selected_records.contains(source) {
                return Err(TransferPlanError::UnexpectedMapping { source: *source });
            }
        }

        let mut targets = BTreeSet::new();
        for (source, target) in &spec.record_id_map {
            if source.family() != target.family() {
                return Err(TransferPlanError::TypeChangingMapping {
                    source: *source,
                    target: *target,
                });
            }
            if source == target {
                return Err(TransferPlanError::ReusedSourceIdentity { source: *source });
            }
            if !targets.insert(*target) {
                return Err(TransferPlanError::DuplicateTargetIdentity { target: *target });
            }
        }

        for source in &spec.selected_event_relations {
            if !spec.event_relation_id_map.contains_key(source) {
                return Err(TransferPlanError::MissingEventRelationMapping { source: *source });
            }
        }
        for source in spec.event_relation_id_map.keys() {
            if !spec.selected_event_relations.contains(source) {
                return Err(TransferPlanError::UnexpectedEventRelationMapping { source: *source });
            }
        }
        let mut event_relation_targets = BTreeSet::new();
        for (source, target) in &spec.event_relation_id_map {
            if source == target {
                return Err(TransferPlanError::ReusedEventRelationIdentity { source: *source });
            }
            if !event_relation_targets.insert(*target) {
                return Err(TransferPlanError::DuplicateEventRelationTarget { target: *target });
            }
        }

        let mut encoded_decisions = spec
            .external_reference_decisions
            .into_iter()
            .map(|(record_ref, decision)| {
                encode_record_ref(record_ref)
                    .map(|encoded| (encoded, record_ref, decision))
                    .map_err(|_| TransferPlanError::RecordRefEncoding)
            })
            .collect::<Result<Vec<_>, _>>()?;
        encoded_decisions.sort_by(|left, right| left.0.cmp(&right.0));
        if encoded_decisions
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left.0 == right.0))
        {
            return Err(TransferPlanError::DuplicateExternalReferenceDecision);
        }
        spec.external_reference_decisions = encoded_decisions
            .into_iter()
            .map(|(_, record_ref, decision)| (record_ref, decision))
            .collect::<Vec<_>>();

        let plan_fingerprint = compute_plan_fingerprint(&spec)?;

        Ok(Self {
            database_id: spec.database_id,
            source_history_space_id: spec.source_history_space_id,
            source_recorded_as_of: spec.source_recorded_as_of,
            target_history_space_id: spec.target_history_space_id,
            expected_target_head: spec.expected_target_head,
            selected_records: spec.selected_records,
            record_id_map: spec.record_id_map,
            selected_event_relations: spec.selected_event_relations,
            event_relation_id_map: spec.event_relation_id_map,
            external_reference_decisions: spec.external_reference_decisions,
            lifecycle_policy: spec.lifecycle_policy,
            archive_policy: spec.archive_policy,
            plan_fingerprint,
        })
    }

    /// Returns the database this same-database transfer is bound to.
    #[must_use]
    pub const fn database_id(&self) -> DatabaseId {
        self.database_id
    }

    /// Returns the pinned source HistorySpace.
    #[must_use]
    pub const fn source_history_space_id(&self) -> HistorySpaceId {
        self.source_history_space_id
    }

    /// Returns the immutable source snapshot revision.
    #[must_use]
    pub const fn source_recorded_as_of(&self) -> Revision {
        self.source_recorded_as_of
    }

    /// Returns the existing destination HistorySpace.
    #[must_use]
    pub const fn target_history_space_id(&self) -> HistorySpaceId {
        self.target_history_space_id
    }

    /// Returns the destination head observed by the plan.
    #[must_use]
    pub const fn expected_target_head(&self) -> Revision {
        self.expected_target_head
    }

    /// Returns the explicit selected record set.
    #[must_use]
    pub const fn selected_records(&self) -> &BTreeSet<HistorySpaceContentRef> {
        &self.selected_records
    }

    /// Returns the explicit same-family source-to-copy ID map.
    #[must_use]
    pub const fn record_id_map(&self) -> &BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef> {
        &self.record_id_map
    }

    /// Returns the explicitly selected project-wide EventRelations.
    #[must_use]
    pub const fn selected_event_relations(&self) -> &BTreeSet<EventRelationId> {
        &self.selected_event_relations
    }

    /// Returns the separate typed EventRelation identity map.
    #[must_use]
    pub const fn event_relation_id_map(&self) -> &BTreeMap<EventRelationId, EventRelationId> {
        &self.event_relation_id_map
    }

    /// Returns the decisions required for references outside the selected set.
    #[must_use]
    pub fn external_reference_decision(
        &self,
        record_ref: RecordRef,
    ) -> Option<ExternalReferenceDecision> {
        self.external_reference_decisions
            .iter()
            .find_map(|(candidate, decision)| (*candidate == record_ref).then_some(*decision))
    }

    /// Returns the explicit policy for copying effective Closure/Retraction records.
    #[must_use]
    pub const fn lifecycle_policy(&self) -> TransferLifecyclePolicy {
        self.lifecycle_policy
    }

    /// Returns the explicit operational archive-state policy.
    #[must_use]
    pub const fn archive_policy(&self) -> TransferArchivePolicy {
        self.archive_policy
    }

    /// Returns the deterministic fingerprint over the complete immutable plan.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> [u8; 32] {
        self.plan_fingerprint
    }
}

/// Explicit choice for a selected record's reference outside the transfer set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalReferenceDecision {
    /// Retain the original identity only after target-visibility validation.
    RetainVisible,
    /// Reject the transfer if this reference occurs in selected content.
    Reject,
}

/// Policy for effective Closure and Retraction records at the source snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferLifecyclePolicy {
    /// Require and copy all effective lifecycle records needed by selected content.
    CopyEffectiveLifecycle,
    /// Omit them only with an explicit preview acknowledgement of the difference.
    OmitWithAcknowledgement,
}

/// Policy for the operational archive state of copied records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferArchivePolicy {
    /// Create ArchiveTransitions for source records currently archived.
    PreserveArchiveState,
    /// Leave every copied record unarchived.
    StartUnarchived,
}

fn compute_plan_fingerprint(spec: &TransferPlanSpec) -> Result<[u8; 32], TransferPlanError> {
    let mut hasher = blake3::Hasher::new_derive_key("worlddb.history-space-transfer-plan.v1");
    hasher.update(&spec.database_id.to_bytes());
    hasher.update(&spec.source_history_space_id.to_bytes());
    hasher.update(&spec.source_recorded_as_of.value().to_le_bytes());
    hasher.update(&spec.target_history_space_id.to_bytes());
    hasher.update(&spec.expected_target_head.value().to_le_bytes());
    hasher.update(&(spec.selected_records.len() as u64).to_le_bytes());
    for record in &spec.selected_records {
        update_content_ref(&mut hasher, *record)?;
    }
    hasher.update(&(spec.record_id_map.len() as u64).to_le_bytes());
    for (source, target) in &spec.record_id_map {
        update_content_ref(&mut hasher, *source)?;
        update_content_ref(&mut hasher, *target)?;
    }
    hasher.update(&(spec.selected_event_relations.len() as u64).to_le_bytes());
    for relation_id in &spec.selected_event_relations {
        hasher.update(&relation_id.to_bytes());
    }
    hasher.update(&(spec.event_relation_id_map.len() as u64).to_le_bytes());
    for (source, target) in &spec.event_relation_id_map {
        hasher.update(&source.to_bytes());
        hasher.update(&target.to_bytes());
    }
    hasher.update(&(spec.external_reference_decisions.len() as u64).to_le_bytes());
    for (record_ref, decision) in &spec.external_reference_decisions {
        let encoded =
            encode_record_ref(*record_ref).map_err(|_| TransferPlanError::RecordRefEncoding)?;
        hasher.update(&(encoded.len() as u64).to_le_bytes());
        hasher.update(&encoded);
        hasher.update(&[match decision {
            ExternalReferenceDecision::RetainVisible => 0,
            ExternalReferenceDecision::Reject => 1,
        }]);
    }
    hasher.update(&[match spec.lifecycle_policy {
        TransferLifecyclePolicy::CopyEffectiveLifecycle => 0,
        TransferLifecyclePolicy::OmitWithAcknowledgement => 1,
    }]);
    hasher.update(&[match spec.archive_policy {
        TransferArchivePolicy::PreserveArchiveState => 0,
        TransferArchivePolicy::StartUnarchived => 1,
    }]);
    Ok(*hasher.finalize().as_bytes())
}

fn update_content_ref(
    hasher: &mut blake3::Hasher,
    content_ref: HistorySpaceContentRef,
) -> Result<(), TransferPlanError> {
    let encoded = encode_record_ref(content_ref.record_ref())
        .map_err(|_| TransferPlanError::RecordRefEncoding)?;
    hasher.update(&(encoded.len() as u64).to_le_bytes());
    hasher.update(&encoded);
    Ok(())
}

/// Invalid selection or typed identity map in a transfer plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferPlanError {
    /// An explicit transfer must select at least one record.
    EmptySelection,
    /// Source and target HistorySpaces must differ.
    SameSourceAndTarget,
    /// A selected source record has no destination ID.
    MissingMapping {
        /// The selected record missing from the ID map.
        source: HistorySpaceContentRef,
    },
    /// The ID map contains an unselected source record.
    UnexpectedMapping {
        /// The unexpected map key.
        source: HistorySpaceContentRef,
    },
    /// A destination ID changes the concrete record family.
    TypeChangingMapping {
        /// The source record identity.
        source: HistorySpaceContentRef,
        /// The proposed destination identity.
        target: HistorySpaceContentRef,
    },
    /// A source identity cannot also be used as its copy's identity.
    ReusedSourceIdentity {
        /// The source identity reused as its own target.
        source: HistorySpaceContentRef,
    },
    /// Two source records cannot map to one destination identity.
    DuplicateTargetIdentity {
        /// The repeated destination identity.
        target: HistorySpaceContentRef,
    },
    /// A selected EventRelation has no mapped identity.
    MissingEventRelationMapping {
        /// The selected relation missing from the ID map.
        source: EventRelationId,
    },
    /// The EventRelation map contains an unselected relation.
    UnexpectedEventRelationMapping {
        /// The unexpected map key.
        source: EventRelationId,
    },
    /// A source EventRelation identity cannot also be used as its copy's identity.
    ReusedEventRelationIdentity {
        /// The source identity reused as its own target.
        source: EventRelationId,
    },
    /// Two EventRelations cannot map to one destination identity.
    DuplicateEventRelationTarget {
        /// The repeated destination identity.
        target: EventRelationId,
    },
    /// The plan repeats a decision for one external RecordRef.
    DuplicateExternalReferenceDecision,
    /// A typed RecordRef could not be canonically encoded for the plan fingerprint.
    RecordRefEncoding,
}

impl fmt::Display for TransferPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => formatter.write_str("transfer selection is empty"),
            Self::SameSourceAndTarget => {
                formatter.write_str("transfer source and target HistorySpaces must differ")
            }
            Self::MissingMapping { source } => {
                write!(
                    formatter,
                    "selected record {source:?} has no mapped identity"
                )
            }
            Self::UnexpectedMapping { source } => {
                write!(formatter, "record {source:?} is mapped but not selected")
            }
            Self::TypeChangingMapping { source, target } => write!(
                formatter,
                "transfer mapping changes record family: {source:?} to {target:?}"
            ),
            Self::ReusedSourceIdentity { source } => write!(
                formatter,
                "transfer mapping reuses source identity {source:?}"
            ),
            Self::DuplicateTargetIdentity { target } => write!(
                formatter,
                "multiple transfer records map to destination identity {target:?}"
            ),
            Self::MissingEventRelationMapping { source } => {
                write!(
                    formatter,
                    "selected EventRelation {source} has no mapped identity"
                )
            }
            Self::UnexpectedEventRelationMapping { source } => {
                write!(
                    formatter,
                    "EventRelation {source} is mapped but not selected"
                )
            }
            Self::ReusedEventRelationIdentity { source } => {
                write!(
                    formatter,
                    "transfer mapping reuses EventRelation identity {source}"
                )
            }
            Self::DuplicateEventRelationTarget { target } => {
                write!(
                    formatter,
                    "multiple EventRelations map to destination identity {target}"
                )
            }
            Self::DuplicateExternalReferenceDecision => {
                formatter.write_str("transfer plan repeats an external reference decision")
            }
            Self::RecordRefEncoding => {
                formatter.write_str("transfer plan could not encode a closed RecordRef")
            }
        }
    }
}

impl std::error::Error for TransferPlanError {}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt;

    use super::{
        HistorySpaceContentRef, TransferArchivePolicy, TransferLifecyclePolicy, TransferLineage,
        TransferLineageError, TransferPlan, TransferPlanError, TransferPlanSpec,
    };
    use crate::ids::{
        ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
        DatabaseId, DomainId, EntityRetirementId, EventId, EventMaskId, EventMaskRetractionId,
        EventRelationId, EventRelationRetractionId, EventRetractionId, EventSpanClosureId,
        EvidenceId, EvidenceRetractionId, HistorySpaceId, IdValidationError, MaskId,
        MaskRetractionId, MaskValidityClosureId, PerspectiveRetirementId, ProvenanceId,
        ProvenanceRetractionId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
        ReplacementBoundaryValidityClosureId, Revision, RevisionError, SourceId, TransferLineageId,
    };
    use crate::record_refs::RecordRef;

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
    impl_test_error_from!(super::HistorySpaceContentRefError);
    impl_test_error_from!(TransferPlanError);

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

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn make_plan(
        database_id: DatabaseId,
        source_space: HistorySpaceId,
        source_as_of: Revision,
        target_space: HistorySpaceId,
        target_head: Revision,
        selected: BTreeSet<HistorySpaceContentRef>,
        mapping: BTreeMap<HistorySpaceContentRef, HistorySpaceContentRef>,
    ) -> Result<TransferPlan, TransferPlanError> {
        TransferPlan::new(TransferPlanSpec {
            database_id,
            source_history_space_id: source_space,
            source_recorded_as_of: source_as_of,
            target_history_space_id: target_space,
            expected_target_head: target_head,
            selected_records: selected,
            record_id_map: mapping,
            selected_event_relations: BTreeSet::new(),
            event_relation_id_map: BTreeMap::new(),
            external_reference_decisions: Vec::new(),
            lifecycle_policy: TransferLifecyclePolicy::CopyEffectiveLifecycle,
            archive_policy: TransferArchivePolicy::PreserveArchiveState,
        })
    }

    #[test]
    fn content_reference_conversion_matches_the_closed_transfer_subset() -> Result<(), TestError> {
        let eligible = [
            RecordRef::Assertion(id::<AssertionId>(1)?),
            RecordRef::Mask(id::<MaskId>(2)?),
            RecordRef::ReplacementBoundary(id::<ReplacementBoundaryId>(3)?),
            RecordRef::Event(id::<EventId>(4)?),
            RecordRef::EventMask(id::<EventMaskId>(5)?),
            RecordRef::AssertionValidityClosure(id::<AssertionValidityClosureId>(6)?),
            RecordRef::AssertionRetraction(id::<AssertionRetractionId>(7)?),
            RecordRef::MaskValidityClosure(id::<MaskValidityClosureId>(8)?),
            RecordRef::MaskRetraction(id::<MaskRetractionId>(9)?),
            RecordRef::ReplacementBoundaryValidityClosure(id::<
                ReplacementBoundaryValidityClosureId,
            >(10)?),
            RecordRef::ReplacementBoundaryRetraction(id::<ReplacementBoundaryRetractionId>(11)?),
            RecordRef::EventSpanClosure(id::<EventSpanClosureId>(12)?),
            RecordRef::EventRetraction(id::<EventRetractionId>(13)?),
            RecordRef::EventMaskRetraction(id::<EventMaskRetractionId>(14)?),
        ];
        for record_ref in eligible {
            assert_eq!(
                HistorySpaceContentRef::try_from(record_ref)?.record_ref(),
                record_ref
            );
        }

        let excluded = [
            RecordRef::EventRelation(id::<EventRelationId>(15)?),
            RecordRef::EventRelationRetraction(id::<EventRelationRetractionId>(16)?),
            RecordRef::Source(id::<SourceId>(17)?),
            RecordRef::Evidence(id::<EvidenceId>(18)?),
            RecordRef::Provenance(id::<ProvenanceId>(19)?),
            RecordRef::EvidenceRetraction(id::<EvidenceRetractionId>(21)?),
            RecordRef::ProvenanceRetraction(id::<ProvenanceRetractionId>(22)?),
            RecordRef::EntityRetirement(id::<EntityRetirementId>(23)?),
            RecordRef::PerspectiveRetirement(id::<PerspectiveRetirementId>(24)?),
            RecordRef::ArchiveTransition(id::<ArchiveTransitionId>(20)?),
        ];
        for record_ref in excluded {
            assert_eq!(
                HistorySpaceContentRef::try_from(record_ref)
                    .err()
                    .map(|error| error.unsupported()),
                Some(record_ref)
            );
        }
        Ok(())
    }

    #[test]
    fn plan_requires_exact_same_family_fresh_identity_map() -> Result<(), TestError> {
        let source_space = id::<HistorySpaceId>(31)?;
        let target_space = id::<HistorySpaceId>(32)?;
        let source_a = HistorySpaceContentRef::Assertion(id::<AssertionId>(33)?);
        let source_b = HistorySpaceContentRef::Assertion(id::<AssertionId>(34)?);
        let target_a = HistorySpaceContentRef::Assertion(id::<AssertionId>(35)?);
        let target_b = HistorySpaceContentRef::Assertion(id::<AssertionId>(36)?);
        let selected = BTreeSet::from([source_a, source_b]);
        let mapping = BTreeMap::from([(source_a, target_a), (source_b, target_b)]);

        let database_id = id::<DatabaseId>(30)?;
        let plan = make_plan(
            database_id,
            source_space,
            revision(4)?,
            target_space,
            revision(5)?,
            selected.clone(),
            mapping.clone(),
        )?;
        assert_eq!(plan.selected_records(), &selected);
        assert_eq!(plan.record_id_map(), &mapping);

        assert_eq!(
            make_plan(
                database_id,
                source_space,
                revision(4)?,
                target_space,
                revision(5)?,
                selected.clone(),
                BTreeMap::from([(source_a, target_a)]),
            ),
            Err(TransferPlanError::MissingMapping { source: source_b })
        );
        assert_eq!(
            make_plan(
                database_id,
                source_space,
                revision(4)?,
                target_space,
                revision(5)?,
                selected.clone(),
                BTreeMap::from([(source_a, target_a), (source_b, target_a)]),
            ),
            Err(TransferPlanError::DuplicateTargetIdentity { target: target_a })
        );
        let target_mask = HistorySpaceContentRef::Mask(id::<MaskId>(37)?);
        assert_eq!(
            make_plan(
                database_id,
                source_space,
                revision(4)?,
                target_space,
                revision(5)?,
                BTreeSet::from([source_a]),
                BTreeMap::from([(source_a, target_mask)]),
            ),
            Err(TransferPlanError::TypeChangingMapping {
                source: source_a,
                target: target_mask,
            })
        );
        Ok(())
    }

    #[test]
    fn transfer_lineage_requires_distinct_same_family_ids_and_spaces() -> Result<(), TestError> {
        let source_space = id::<HistorySpaceId>(80)?;
        let target_space = id::<HistorySpaceId>(81)?;
        let source = HistorySpaceContentRef::Mask(id::<MaskId>(82)?);
        let target = HistorySpaceContentRef::Mask(id::<MaskId>(83)?);
        let lineage_id = id::<TransferLineageId>(84)?;
        assert!(
            TransferLineage::new(
                lineage_id,
                source_space,
                target_space,
                source,
                target,
                Revision::FIRST_COMMIT,
            )
            .is_ok()
        );
        assert_eq!(
            TransferLineage::new(
                lineage_id,
                source_space,
                target_space,
                source,
                source,
                Revision::FIRST_COMMIT,
            ),
            Err(TransferLineageError::InvalidIdentityPair),
        );
        assert_eq!(
            TransferLineage::new(
                lineage_id,
                source_space,
                target_space,
                source,
                HistorySpaceContentRef::Event(id::<EventId>(85)?),
                Revision::FIRST_COMMIT,
            ),
            Err(TransferLineageError::InvalidIdentityPair),
        );
        assert_eq!(
            TransferLineage::new(
                lineage_id,
                source_space,
                source_space,
                source,
                target,
                Revision::FIRST_COMMIT,
            ),
            Err(TransferLineageError::InvalidIdentityPair),
        );
        Ok(())
    }

    #[test]
    fn plan_rejects_empty_same_space_unselected_and_reused_mappings() -> Result<(), TestError> {
        let source_space = id::<HistorySpaceId>(41)?;
        let other_space = id::<HistorySpaceId>(42)?;
        let source = HistorySpaceContentRef::Event(id::<EventId>(43)?);
        let copy = HistorySpaceContentRef::Event(id::<EventId>(44)?);
        let extra = HistorySpaceContentRef::Event(id::<EventId>(45)?);
        let as_of = revision(8)?;

        assert_eq!(
            make_plan(
                id::<DatabaseId>(40)?,
                source_space,
                as_of,
                other_space,
                as_of,
                BTreeSet::new(),
                BTreeMap::new(),
            ),
            Err(TransferPlanError::EmptySelection)
        );
        assert_eq!(
            make_plan(
                id::<DatabaseId>(40)?,
                source_space,
                as_of,
                source_space,
                as_of,
                BTreeSet::from([source]),
                BTreeMap::from([(source, copy)]),
            ),
            Err(TransferPlanError::SameSourceAndTarget)
        );
        assert_eq!(
            make_plan(
                id::<DatabaseId>(40)?,
                source_space,
                as_of,
                other_space,
                as_of,
                BTreeSet::from([source]),
                BTreeMap::from([(source, source)]),
            ),
            Err(TransferPlanError::ReusedSourceIdentity { source })
        );
        assert_eq!(
            make_plan(
                id::<DatabaseId>(40)?,
                source_space,
                as_of,
                other_space,
                as_of,
                BTreeSet::from([source]),
                BTreeMap::from([(source, copy), (extra, extra)]),
            ),
            Err(TransferPlanError::UnexpectedMapping { source: extra })
        );
        Ok(())
    }

    #[test]
    fn full_plan_binds_event_relation_maps_policies_and_sorted_external_decisions()
    -> Result<(), TestError> {
        use super::ExternalReferenceDecision;

        let database_id = id::<DatabaseId>(50)?;
        let source_space = id::<HistorySpaceId>(51)?;
        let target_space = id::<HistorySpaceId>(52)?;
        let source_event = HistorySpaceContentRef::Event(id::<EventId>(53)?);
        let target_event = HistorySpaceContentRef::Event(id::<EventId>(54)?);
        let source_relation = id::<EventRelationId>(55)?;
        let target_relation = id::<EventRelationId>(56)?;
        let retained_source = RecordRef::Source(id::<SourceId>(57)?);
        let external_assertion = RecordRef::Assertion(id::<AssertionId>(58)?);
        let source_as_of = revision(9)?;
        let target_head = revision(10)?;

        let build = |decisions| {
            TransferPlan::new(TransferPlanSpec {
                database_id,
                source_history_space_id: source_space,
                source_recorded_as_of: source_as_of,
                target_history_space_id: target_space,
                expected_target_head: target_head,
                selected_records: BTreeSet::from([source_event]),
                record_id_map: BTreeMap::from([(source_event, target_event)]),
                selected_event_relations: BTreeSet::from([source_relation]),
                event_relation_id_map: BTreeMap::from([(source_relation, target_relation)]),
                external_reference_decisions: decisions,
                lifecycle_policy: TransferLifecyclePolicy::OmitWithAcknowledgement,
                archive_policy: TransferArchivePolicy::StartUnarchived,
            })
        };

        let ordered = build(vec![
            (retained_source, ExternalReferenceDecision::RetainVisible),
            (external_assertion, ExternalReferenceDecision::Reject),
        ])?;
        let reversed = build(vec![
            (external_assertion, ExternalReferenceDecision::Reject),
            (retained_source, ExternalReferenceDecision::RetainVisible),
        ])?;
        assert_eq!(ordered.plan_fingerprint(), reversed.plan_fingerprint());
        assert_eq!(
            ordered.external_reference_decision(retained_source),
            Some(ExternalReferenceDecision::RetainVisible)
        );
        assert_eq!(
            ordered.external_reference_decision(external_assertion),
            Some(ExternalReferenceDecision::Reject)
        );
        assert_eq!(
            ordered.event_relation_id_map(),
            &BTreeMap::from([(source_relation, target_relation)])
        );
        assert_eq!(
            ordered.lifecycle_policy(),
            TransferLifecyclePolicy::OmitWithAcknowledgement
        );
        assert_eq!(
            ordered.archive_policy(),
            TransferArchivePolicy::StartUnarchived
        );
        assert_ne!(ordered.plan_fingerprint(), [0; 32]);
        Ok(())
    }
}
