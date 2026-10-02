//! Closed API and wire references for domain, lifecycle, and operational records.

use std::fmt;

use crate::archive::ArchiveTargetRef;
use crate::ids::{
    ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
    AuditOperationId, AuditRecordId, DatabaseId, EntityRetirementId, EntityTypeId,
    EventAttributeId, EventId, EventKindId, EventMaskId, EventMaskRetractionId, EventRelationId,
    EventRelationRetractionId, EventRetractionId, EventRoleId, EventSpanClosureId, EvidenceId,
    EvidenceRetractionId, JobId, LayerId, MaskId, MaskRetractionId, MaskValidityClosureId,
    MigrationId, MigrationRunId, MigrationStepId, OperationId, PerspectiveRetirementId,
    PolicyRuleId, PredicateId, PrincipalId, ProvenanceId, ProvenanceRetractionId,
    ReplacementBoundaryId, ReplacementBoundaryRetractionId, ReplacementBoundaryValidityClosureId,
    RoleAssignmentId, RoleId, SecurityPolicyRecordId, SnapshotId, SourceId, TimelineId,
    TransactionId, TransferLineageId,
};
use crate::source_provenance::{EvidenceTargetRef, ProvenanceEndpointRef};

/// Fixed, append-only wire tags for the 1.0 RecordRef variants.
///
/// Values are reserved by policy/record-ref-wire-tags.tsv. New variants must
/// receive new values; existing values are never renumbered or reused.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u16)]
pub enum RecordRefWireTag {
    /// Assertion.
    Assertion = 1,
    /// Mask.
    Mask = 2,
    /// ReplacementBoundary.
    ReplacementBoundary = 3,
    /// Event.
    Event = 4,
    /// EventMask.
    EventMask = 5,
    /// EventRelation.
    EventRelation = 6,
    /// Source.
    Source = 7,
    /// Evidence.
    Evidence = 8,
    /// Provenance.
    Provenance = 9,
    /// AssertionValidityClosure.
    AssertionValidityClosure = 10,
    /// AssertionRetraction.
    AssertionRetraction = 11,
    /// MaskValidityClosure.
    MaskValidityClosure = 12,
    /// MaskRetraction.
    MaskRetraction = 13,
    /// ReplacementBoundaryValidityClosure.
    ReplacementBoundaryValidityClosure = 14,
    /// ReplacementBoundaryRetraction.
    ReplacementBoundaryRetraction = 15,
    /// EventSpanClosure.
    EventSpanClosure = 16,
    /// EventRetraction.
    EventRetraction = 17,
    /// EventMaskRetraction.
    EventMaskRetraction = 18,
    /// EventRelationRetraction.
    EventRelationRetraction = 19,
    /// EvidenceRetraction.
    EvidenceRetraction = 20,
    /// ProvenanceRetraction.
    ProvenanceRetraction = 21,
    /// EntityRetirement.
    EntityRetirement = 22,
    /// PerspectiveRetirement.
    PerspectiveRetirement = 23,
    /// ArchiveTransition.
    ArchiveTransition = 24,
    /// TransferLineage.
    TransferLineage = 25,
}

impl RecordRefWireTag {
    /// Returns the fixed numeric tag used by the 1.0 RecordRef encoding.
    #[must_use]
    pub const fn value(self) -> u16 {
        self as u16
    }
}

/// An unknown numeric RecordRef wire tag.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnknownRecordRefWireTag {
    tag: u16,
}

impl UnknownRecordRefWireTag {
    /// Returns the unrecognized numeric tag.
    #[must_use]
    pub const fn tag(self) -> u16 {
        self.tag
    }
}

impl fmt::Display for UnknownRecordRefWireTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown RecordRef wire tag {}", self.tag)
    }
}

impl std::error::Error for UnknownRecordRefWireTag {}

impl TryFrom<u16> for RecordRefWireTag {
    type Error = UnknownRecordRefWireTag;

    fn try_from(tag: u16) -> Result<Self, Self::Error> {
        match tag {
            1 => Ok(Self::Assertion),
            2 => Ok(Self::Mask),
            3 => Ok(Self::ReplacementBoundary),
            4 => Ok(Self::Event),
            5 => Ok(Self::EventMask),
            6 => Ok(Self::EventRelation),
            7 => Ok(Self::Source),
            8 => Ok(Self::Evidence),
            9 => Ok(Self::Provenance),
            10 => Ok(Self::AssertionValidityClosure),
            11 => Ok(Self::AssertionRetraction),
            12 => Ok(Self::MaskValidityClosure),
            13 => Ok(Self::MaskRetraction),
            14 => Ok(Self::ReplacementBoundaryValidityClosure),
            15 => Ok(Self::ReplacementBoundaryRetraction),
            16 => Ok(Self::EventSpanClosure),
            17 => Ok(Self::EventRetraction),
            18 => Ok(Self::EventMaskRetraction),
            19 => Ok(Self::EventRelationRetraction),
            20 => Ok(Self::EvidenceRetraction),
            21 => Ok(Self::ProvenanceRetraction),
            22 => Ok(Self::EntityRetirement),
            23 => Ok(Self::PerspectiveRetirement),
            24 => Ok(Self::ArchiveTransition),
            25 => Ok(Self::TransferLineage),
            _ => Err(UnknownRecordRefWireTag { tag }),
        }
    }
}

/// One persistable, typed reference to a domain-history record.
///
/// This is the closed 1.0 family. Its stable numeric tags are listed in
/// policy/record-ref-wire-tags.tsv. Project catalog, schema, migration,
/// security, transaction, snapshot, job, and audit identities use their own
/// separate closed reference types below.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RecordRef {
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
    /// An EventRelation record.
    EventRelation(EventRelationId),
    /// A Source record.
    Source(SourceId),
    /// An Evidence record.
    Evidence(EvidenceId),
    /// A ProvenanceEdge record.
    Provenance(ProvenanceId),
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
    /// An EventRelationRetraction record.
    EventRelationRetraction(EventRelationRetractionId),
    /// An EvidenceRetraction record.
    EvidenceRetraction(EvidenceRetractionId),
    /// A ProvenanceRetraction record.
    ProvenanceRetraction(ProvenanceRetractionId),
    /// An EntityRetirement record.
    EntityRetirement(EntityRetirementId),
    /// A PerspectiveRetirement record.
    PerspectiveRetirement(PerspectiveRetirementId),
    /// An ArchiveTransition record.
    ArchiveTransition(ArchiveTransitionId),
    /// A persistent HistorySpace-copy lineage record.
    TransferLineage(TransferLineageId),
}

impl RecordRef {
    /// Returns the permanent wire tag assigned to this concrete variant.
    #[must_use]
    pub const fn wire_tag(self) -> RecordRefWireTag {
        match self {
            Self::Assertion(_) => RecordRefWireTag::Assertion,
            Self::Mask(_) => RecordRefWireTag::Mask,
            Self::ReplacementBoundary(_) => RecordRefWireTag::ReplacementBoundary,
            Self::Event(_) => RecordRefWireTag::Event,
            Self::EventMask(_) => RecordRefWireTag::EventMask,
            Self::EventRelation(_) => RecordRefWireTag::EventRelation,
            Self::Source(_) => RecordRefWireTag::Source,
            Self::Evidence(_) => RecordRefWireTag::Evidence,
            Self::Provenance(_) => RecordRefWireTag::Provenance,
            Self::AssertionValidityClosure(_) => RecordRefWireTag::AssertionValidityClosure,
            Self::AssertionRetraction(_) => RecordRefWireTag::AssertionRetraction,
            Self::MaskValidityClosure(_) => RecordRefWireTag::MaskValidityClosure,
            Self::MaskRetraction(_) => RecordRefWireTag::MaskRetraction,
            Self::ReplacementBoundaryValidityClosure(_) => {
                RecordRefWireTag::ReplacementBoundaryValidityClosure
            }
            Self::ReplacementBoundaryRetraction(_) => {
                RecordRefWireTag::ReplacementBoundaryRetraction
            }
            Self::EventSpanClosure(_) => RecordRefWireTag::EventSpanClosure,
            Self::EventRetraction(_) => RecordRefWireTag::EventRetraction,
            Self::EventMaskRetraction(_) => RecordRefWireTag::EventMaskRetraction,
            Self::EventRelationRetraction(_) => RecordRefWireTag::EventRelationRetraction,
            Self::EvidenceRetraction(_) => RecordRefWireTag::EvidenceRetraction,
            Self::ProvenanceRetraction(_) => RecordRefWireTag::ProvenanceRetraction,
            Self::EntityRetirement(_) => RecordRefWireTag::EntityRetirement,
            Self::PerspectiveRetirement(_) => RecordRefWireTag::PerspectiveRetirement,
            Self::ArchiveTransition(_) => RecordRefWireTag::ArchiveTransition,
            Self::TransferLineage(_) => RecordRefWireTag::TransferLineage,
        }
    }

    /// Returns the stable variant name used by the wire-tag ledger.
    #[must_use]
    pub const fn variant_name(self) -> &'static str {
        match self {
            Self::Assertion(_) => "Assertion",
            Self::Mask(_) => "Mask",
            Self::ReplacementBoundary(_) => "ReplacementBoundary",
            Self::Event(_) => "Event",
            Self::EventMask(_) => "EventMask",
            Self::EventRelation(_) => "EventRelation",
            Self::Source(_) => "Source",
            Self::Evidence(_) => "Evidence",
            Self::Provenance(_) => "Provenance",
            Self::AssertionValidityClosure(_) => "AssertionValidityClosure",
            Self::AssertionRetraction(_) => "AssertionRetraction",
            Self::MaskValidityClosure(_) => "MaskValidityClosure",
            Self::MaskRetraction(_) => "MaskRetraction",
            Self::ReplacementBoundaryValidityClosure(_) => "ReplacementBoundaryValidityClosure",
            Self::ReplacementBoundaryRetraction(_) => "ReplacementBoundaryRetraction",
            Self::EventSpanClosure(_) => "EventSpanClosure",
            Self::EventRetraction(_) => "EventRetraction",
            Self::EventMaskRetraction(_) => "EventMaskRetraction",
            Self::EventRelationRetraction(_) => "EventRelationRetraction",
            Self::EvidenceRetraction(_) => "EvidenceRetraction",
            Self::ProvenanceRetraction(_) => "ProvenanceRetraction",
            Self::EntityRetirement(_) => "EntityRetirement",
            Self::PerspectiveRetirement(_) => "PerspectiveRetirement",
            Self::ArchiveTransition(_) => "ArchiveTransition",
            Self::TransferLineage(_) => "TransferLineage",
        }
    }
}

/// A closed target family for domain lifecycle operations.
///
/// These are the base records addressed by the concrete Closure and Retraction
/// records. Lifecycle records do not target other lifecycle records.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LifecycleTargetRef {
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
    /// An EventRelation record.
    EventRelation(EventRelationId),
    /// An Evidence record.
    Evidence(EvidenceId),
    /// A ProvenanceEdge record.
    Provenance(ProvenanceId),
}

/// A closed relation-specific reference for EventRelation provenance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventRelationProvenanceRef {
    /// An EventRelation record.
    EventRelation(EventRelationId),
    /// An EventRelationRetraction lifecycle record.
    EventRelationRetraction(EventRelationRetractionId),
}

/// Closed reference family for project schema and catalog records.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SchemaRecordRef {
    /// A Layer definition.
    Layer(LayerId),
    /// An EntityType definition.
    EntityType(EntityTypeId),
    /// A Predicate definition.
    Predicate(PredicateId),
    /// An EventKind definition.
    EventKind(EventKindId),
    /// An EventRole definition.
    EventRole(EventRoleId),
    /// An EventAttribute definition.
    EventAttribute(EventAttributeId),
    /// A Timeline definition.
    Timeline(TimelineId),
}

/// Closed reference family for migration history.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MigrationRecordRef {
    /// A registered migration definition.
    Migration(MigrationId),
    /// A migration execution.
    Run(MigrationRunId),
    /// A migration execution step.
    Step(MigrationStepId),
}

/// Closed reference family for security policy history and identities.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SecurityRecordRef {
    /// A SecurityPolicyRecord.
    PolicyRecord(SecurityPolicyRecordId),
    /// A Principal identity.
    Principal(PrincipalId),
    /// A Role identity.
    Role(RoleId),
    /// A RoleAssignment identity.
    RoleAssignment(RoleAssignmentId),
    /// A capability PolicyRule identity.
    PolicyRule(PolicyRuleId),
}

/// Closed reference family for transaction metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TransactionRef {
    /// A write transaction.
    Transaction(TransactionId),
    /// An idempotent operation.
    Operation(OperationId),
}

/// Session-local reference to a snapshot.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SnapshotRef(SnapshotId);

impl SnapshotRef {
    /// Creates a reference to the session-local Snapshot identity.
    #[must_use]
    pub const fn new(id: SnapshotId) -> Self {
        Self(id)
    }

    /// Returns the session-local Snapshot identity.
    #[must_use]
    pub const fn id(self) -> SnapshotId {
        self.0
    }
}

/// Closed reference family for resumable jobs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JobRef(JobId);

impl JobRef {
    /// Creates a reference to a Job identity.
    #[must_use]
    pub const fn new(id: JobId) -> Self {
        Self(id)
    }

    /// Returns the Job identity.
    #[must_use]
    pub const fn id(self) -> JobId {
        self.0
    }
}

/// Closed reference family for audit records and audit operations.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditRecordRef {
    /// An AuditRecord.
    Record(AuditRecordId),
    /// An audit subsystem operation.
    Operation(AuditOperationId),
}

/// The target class rejected by a closed RecordRef subset conversion.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RecordRefConversionError {
    /// The variant is not an EvidenceTargetRef member.
    NotEvidenceTarget,
    /// The variant is not a ProvenanceEndpointRef member.
    NotProvenanceEndpoint,
    /// The variant is not a LifecycleTargetRef member.
    NotLifecycleTarget,
    /// ArchiveTransition cannot target itself.
    NotArchiveTarget,
    /// The variant is not an EventRelationProvenanceRef member.
    NotEventRelationProvenance,
}

impl fmt::Display for RecordRefConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotEvidenceTarget => "record is not an allowed Evidence target",
            Self::NotProvenanceEndpoint => "record is not an allowed Provenance endpoint",
            Self::NotLifecycleTarget => "record is not an allowed Lifecycle target",
            Self::NotArchiveTarget => "record cannot receive an archive transition",
            Self::NotEventRelationProvenance => {
                "record is not an allowed EventRelation provenance reference"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RecordRefConversionError {}

impl TryFrom<RecordRef> for EvidenceTargetRef {
    type Error = RecordRefConversionError;

    fn try_from(value: RecordRef) -> Result<Self, Self::Error> {
        match value {
            RecordRef::Assertion(id) => Ok(Self::Assertion(id)),
            RecordRef::Mask(id) => Ok(Self::Mask(id)),
            RecordRef::ReplacementBoundary(id) => Ok(Self::ReplacementBoundary(id)),
            RecordRef::Event(id) => Ok(Self::Event(id)),
            RecordRef::EventMask(id) => Ok(Self::EventMask(id)),
            RecordRef::Source(_) | RecordRef::Evidence(_) | RecordRef::EventRelation(_) => {
                Err(RecordRefConversionError::NotEvidenceTarget)
            }
            RecordRef::Provenance(id) => Ok(Self::Provenance(id)),
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
            RecordRef::EventRelationRetraction(id) => Ok(Self::EventRelationRetraction(id)),
            RecordRef::EvidenceRetraction(id) => Ok(Self::EvidenceRetraction(id)),
            RecordRef::ProvenanceRetraction(id) => Ok(Self::ProvenanceRetraction(id)),
            RecordRef::EntityRetirement(id) => Ok(Self::EntityRetirement(id)),
            RecordRef::PerspectiveRetirement(id) => Ok(Self::PerspectiveRetirement(id)),
            RecordRef::ArchiveTransition(id) => Ok(Self::ArchiveTransition(id)),
            RecordRef::TransferLineage(_) => Err(RecordRefConversionError::NotEvidenceTarget),
        }
    }
}

impl TryFrom<RecordRef> for ProvenanceEndpointRef {
    type Error = RecordRefConversionError;

    fn try_from(value: RecordRef) -> Result<Self, Self::Error> {
        match value {
            RecordRef::Assertion(id) => Ok(Self::Assertion(id)),
            RecordRef::Mask(id) => Ok(Self::Mask(id)),
            RecordRef::ReplacementBoundary(id) => Ok(Self::ReplacementBoundary(id)),
            RecordRef::Event(id) => Ok(Self::Event(id)),
            RecordRef::EventMask(id) => Ok(Self::EventMask(id)),
            RecordRef::EventRelation(_) => Err(RecordRefConversionError::NotProvenanceEndpoint),
            RecordRef::Source(id) => Ok(Self::Source(id)),
            RecordRef::Evidence(id) => Ok(Self::Evidence(id)),
            RecordRef::Provenance(id) => Ok(Self::Provenance(id)),
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
            RecordRef::EventRelationRetraction(id) => Ok(Self::EventRelationRetraction(id)),
            RecordRef::EvidenceRetraction(id) => Ok(Self::EvidenceRetraction(id)),
            RecordRef::ProvenanceRetraction(id) => Ok(Self::ProvenanceRetraction(id)),
            RecordRef::EntityRetirement(id) => Ok(Self::EntityRetirement(id)),
            RecordRef::PerspectiveRetirement(id) => Ok(Self::PerspectiveRetirement(id)),
            RecordRef::ArchiveTransition(id) => Ok(Self::ArchiveTransition(id)),
            RecordRef::TransferLineage(_) => Err(RecordRefConversionError::NotProvenanceEndpoint),
        }
    }
}

impl TryFrom<RecordRef> for LifecycleTargetRef {
    type Error = RecordRefConversionError;

    fn try_from(value: RecordRef) -> Result<Self, Self::Error> {
        match value {
            RecordRef::Assertion(id) => Ok(Self::Assertion(id)),
            RecordRef::Mask(id) => Ok(Self::Mask(id)),
            RecordRef::ReplacementBoundary(id) => Ok(Self::ReplacementBoundary(id)),
            RecordRef::Event(id) => Ok(Self::Event(id)),
            RecordRef::EventMask(id) => Ok(Self::EventMask(id)),
            RecordRef::EventRelation(id) => Ok(Self::EventRelation(id)),
            RecordRef::Evidence(id) => Ok(Self::Evidence(id)),
            RecordRef::Provenance(id) => Ok(Self::Provenance(id)),
            RecordRef::Source(_)
            | RecordRef::AssertionValidityClosure(_)
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
            | RecordRef::ArchiveTransition(_)
            | RecordRef::TransferLineage(_) => Err(RecordRefConversionError::NotLifecycleTarget),
        }
    }
}

impl TryFrom<RecordRef> for ArchiveTargetRef {
    type Error = RecordRefConversionError;

    fn try_from(value: RecordRef) -> Result<Self, Self::Error> {
        match value {
            RecordRef::Assertion(id) => Ok(Self::Assertion(id)),
            RecordRef::Mask(id) => Ok(Self::Mask(id)),
            RecordRef::ReplacementBoundary(id) => Ok(Self::ReplacementBoundary(id)),
            RecordRef::Event(id) => Ok(Self::Event(id)),
            RecordRef::EventMask(id) => Ok(Self::EventMask(id)),
            RecordRef::EventRelation(id) => Ok(Self::EventRelation(id)),
            RecordRef::Source(id) => Ok(Self::Source(id)),
            RecordRef::Evidence(id) => Ok(Self::Evidence(id)),
            RecordRef::Provenance(id) => Ok(Self::Provenance(id)),
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
            RecordRef::EventRelationRetraction(id) => Ok(Self::EventRelationRetraction(id)),
            RecordRef::EvidenceRetraction(id) => Ok(Self::EvidenceRetraction(id)),
            RecordRef::ProvenanceRetraction(id) => Ok(Self::ProvenanceRetraction(id)),
            RecordRef::EntityRetirement(id) => Ok(Self::EntityRetirement(id)),
            RecordRef::PerspectiveRetirement(id) => Ok(Self::PerspectiveRetirement(id)),
            RecordRef::TransferLineage(id) => Ok(Self::TransferLineage(id)),
            RecordRef::ArchiveTransition(_) => Err(RecordRefConversionError::NotArchiveTarget),
        }
    }
}

impl TryFrom<RecordRef> for EventRelationProvenanceRef {
    type Error = RecordRefConversionError;

    fn try_from(value: RecordRef) -> Result<Self, Self::Error> {
        match value {
            RecordRef::EventRelation(id) => Ok(Self::EventRelation(id)),
            RecordRef::EventRelationRetraction(id) => Ok(Self::EventRelationRetraction(id)),
            RecordRef::Assertion(_)
            | RecordRef::Mask(_)
            | RecordRef::ReplacementBoundary(_)
            | RecordRef::Event(_)
            | RecordRef::EventMask(_)
            | RecordRef::Source(_)
            | RecordRef::Evidence(_)
            | RecordRef::Provenance(_)
            | RecordRef::AssertionValidityClosure(_)
            | RecordRef::AssertionRetraction(_)
            | RecordRef::MaskValidityClosure(_)
            | RecordRef::MaskRetraction(_)
            | RecordRef::ReplacementBoundaryValidityClosure(_)
            | RecordRef::ReplacementBoundaryRetraction(_)
            | RecordRef::EventSpanClosure(_)
            | RecordRef::EventRetraction(_)
            | RecordRef::EventMaskRetraction(_)
            | RecordRef::EvidenceRetraction(_)
            | RecordRef::ProvenanceRetraction(_)
            | RecordRef::EntityRetirement(_)
            | RecordRef::PerspectiveRetirement(_)
            | RecordRef::ArchiveTransition(_)
            | RecordRef::TransferLineage(_) => {
                Err(RecordRefConversionError::NotEventRelationProvenance)
            }
        }
    }
}

#[doc(hidden)]
pub mod database_reference_sealed {
    /// Seals database-reference implementations to this module.
    pub trait Sealed {}
}

/// Marker for closed reference types accepted by DatabaseBoundRef.
///
/// This trait is sealed so callers cannot introduce string or generic-ID
/// escape hatches into the public reference boundary.
pub trait DatabaseReference: database_reference_sealed::Sealed + Copy {}

macro_rules! impl_database_reference {
    ($($reference:ty),+ $(,)?) => {
        $(
            impl database_reference_sealed::Sealed for $reference {}
            impl DatabaseReference for $reference {}
        )+
    };
}

impl_database_reference!(
    RecordRef,
    EvidenceTargetRef,
    ProvenanceEndpointRef,
    LifecycleTargetRef,
    ArchiveTargetRef,
    EventRelationProvenanceRef,
    SchemaRecordRef,
    MigrationRecordRef,
    SecurityRecordRef,
    TransactionRef,
    SnapshotRef,
    JobRef,
    AuditRecordRef,
);

/// A closed record reference bound to its database at an API boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DatabaseBoundRef<T: DatabaseReference> {
    database_id: DatabaseId,
    reference: T,
}

impl<T: DatabaseReference> DatabaseBoundRef<T> {
    /// Binds the validated reference to a database identity.
    #[must_use]
    pub const fn new(database_id: DatabaseId, reference: T) -> Self {
        Self {
            database_id,
            reference,
        }
    }

    /// Returns the database identity.
    #[must_use]
    pub const fn database_id(self) -> DatabaseId {
        self.database_id
    }

    /// Returns the closed inner reference.
    #[must_use]
    pub const fn reference(self) -> T {
        self.reference
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveTargetRef, AuditRecordRef, DatabaseBoundRef, EventRelationProvenanceRef, JobRef,
        LifecycleTargetRef, MigrationRecordRef, RecordRef, RecordRefConversionError,
        RecordRefWireTag, SchemaRecordRef, SecurityRecordRef, SnapshotRef, TransactionRef,
    };
    use crate::ids::{
        ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
        AuditOperationId, AuditRecordId, DatabaseId, DomainId, EntityRetirementId, EntityTypeId,
        EventAttributeId, EventId, EventKindId, EventMaskId, EventMaskRetractionId,
        EventRelationId, EventRelationRetractionId, EventRetractionId, EventRoleId,
        EventSpanClosureId, EvidenceId, EvidenceRetractionId, JobId, LayerId, MaskId,
        MaskRetractionId, MaskValidityClosureId, MigrationId, MigrationRunId, MigrationStepId,
        OperationId, PerspectiveRetirementId, PolicyRuleId, PredicateId, PrincipalId, ProvenanceId,
        ProvenanceRetractionId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
        ReplacementBoundaryValidityClosureId, RoleAssignmentId, RoleId, SecurityPolicyRecordId,
        SnapshotId, SourceId, TimelineId, TransactionId, TransferLineageId,
    };
    use crate::source_provenance::{EvidenceTargetRef, ProvenanceEndpointRef};

    fn uuid<T: DomainId>(tail: u8) -> Result<T, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn record_refs() -> Result<Vec<RecordRef>, crate::ids::IdValidationError> {
        Ok(vec![
            RecordRef::Assertion(uuid::<AssertionId>(1)?),
            RecordRef::Mask(uuid::<MaskId>(2)?),
            RecordRef::ReplacementBoundary(uuid::<ReplacementBoundaryId>(3)?),
            RecordRef::Event(uuid::<EventId>(4)?),
            RecordRef::EventMask(uuid::<EventMaskId>(5)?),
            RecordRef::EventRelation(uuid::<EventRelationId>(6)?),
            RecordRef::Source(uuid::<SourceId>(7)?),
            RecordRef::Evidence(uuid::<EvidenceId>(8)?),
            RecordRef::Provenance(uuid::<ProvenanceId>(9)?),
            RecordRef::AssertionValidityClosure(uuid::<AssertionValidityClosureId>(10)?),
            RecordRef::AssertionRetraction(uuid::<AssertionRetractionId>(11)?),
            RecordRef::MaskValidityClosure(uuid::<MaskValidityClosureId>(12)?),
            RecordRef::MaskRetraction(uuid::<MaskRetractionId>(13)?),
            RecordRef::ReplacementBoundaryValidityClosure(uuid::<
                ReplacementBoundaryValidityClosureId,
            >(14)?),
            RecordRef::ReplacementBoundaryRetraction(uuid::<ReplacementBoundaryRetractionId>(15)?),
            RecordRef::EventSpanClosure(uuid::<EventSpanClosureId>(16)?),
            RecordRef::EventRetraction(uuid::<EventRetractionId>(17)?),
            RecordRef::EventMaskRetraction(uuid::<EventMaskRetractionId>(18)?),
            RecordRef::EventRelationRetraction(uuid::<EventRelationRetractionId>(19)?),
            RecordRef::EvidenceRetraction(uuid::<EvidenceRetractionId>(20)?),
            RecordRef::ProvenanceRetraction(uuid::<ProvenanceRetractionId>(21)?),
            RecordRef::EntityRetirement(uuid::<EntityRetirementId>(22)?),
            RecordRef::PerspectiveRetirement(uuid::<PerspectiveRetirementId>(23)?),
            RecordRef::ArchiveTransition(uuid::<ArchiveTransitionId>(24)?),
            RecordRef::TransferLineage(uuid::<TransferLineageId>(25)?),
        ])
    }

    fn is_evidence_target(reference: RecordRef) -> bool {
        !matches!(
            reference,
            RecordRef::EventRelation(_)
                | RecordRef::Source(_)
                | RecordRef::Evidence(_)
                | RecordRef::TransferLineage(_)
        )
    }

    fn is_provenance_endpoint(reference: RecordRef) -> bool {
        !matches!(
            reference,
            RecordRef::EventRelation(_) | RecordRef::TransferLineage(_)
        )
    }

    fn is_lifecycle_target(reference: RecordRef) -> bool {
        matches!(
            reference,
            RecordRef::Assertion(_)
                | RecordRef::Mask(_)
                | RecordRef::ReplacementBoundary(_)
                | RecordRef::Event(_)
                | RecordRef::EventMask(_)
                | RecordRef::EventRelation(_)
                | RecordRef::Evidence(_)
                | RecordRef::Provenance(_)
        )
    }

    fn is_archive_target(reference: RecordRef) -> bool {
        !matches!(reference, RecordRef::ArchiveTransition(_))
    }

    fn is_event_relation_provenance(reference: RecordRef) -> bool {
        matches!(
            reference,
            RecordRef::EventRelation(_) | RecordRef::EventRelationRetraction(_)
        )
    }

    #[test]
    fn every_record_ref_has_one_stable_unique_wire_tag_and_ledger_entry()
    -> Result<(), crate::ids::IdValidationError> {
        let references = record_refs()?;
        assert_eq!(references.len(), 25);
        let mut expected_tag = 1_u16;
        let mut expected_rows = Vec::new();
        for reference in &references {
            let tag = reference.wire_tag();
            assert_eq!(tag.value(), expected_tag);
            assert_eq!(RecordRefWireTag::try_from(expected_tag), Ok(tag));
            expected_rows.push(format!(
                "{}\t{}\t{}",
                expected_tag,
                reference.variant_name(),
                reference_id_type(*reference)
            ));
            expected_tag += 1;
        }
        let ledger_rows = include_str!("../../../policy/record-ref-wire-tags.tsv")
            .lines()
            .skip(1)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(ledger_rows, expected_rows);
        for reference in &references {
            assert_eq!(
                references
                    .iter()
                    .filter(|candidate| candidate.wire_tag() == reference.wire_tag())
                    .count(),
                1
            );
        }
        Ok(())
    }

    fn reference_id_type(reference: RecordRef) -> &'static str {
        match reference {
            RecordRef::Assertion(_) => "AssertionId",
            RecordRef::Mask(_) => "MaskId",
            RecordRef::ReplacementBoundary(_) => "ReplacementBoundaryId",
            RecordRef::Event(_) => "EventId",
            RecordRef::EventMask(_) => "EventMaskId",
            RecordRef::EventRelation(_) => "EventRelationId",
            RecordRef::Source(_) => "SourceId",
            RecordRef::Evidence(_) => "EvidenceId",
            RecordRef::Provenance(_) => "ProvenanceId",
            RecordRef::AssertionValidityClosure(_) => "AssertionValidityClosureId",
            RecordRef::AssertionRetraction(_) => "AssertionRetractionId",
            RecordRef::MaskValidityClosure(_) => "MaskValidityClosureId",
            RecordRef::MaskRetraction(_) => "MaskRetractionId",
            RecordRef::ReplacementBoundaryValidityClosure(_) => {
                "ReplacementBoundaryValidityClosureId"
            }
            RecordRef::ReplacementBoundaryRetraction(_) => "ReplacementBoundaryRetractionId",
            RecordRef::EventSpanClosure(_) => "EventSpanClosureId",
            RecordRef::EventRetraction(_) => "EventRetractionId",
            RecordRef::EventMaskRetraction(_) => "EventMaskRetractionId",
            RecordRef::EventRelationRetraction(_) => "EventRelationRetractionId",
            RecordRef::EvidenceRetraction(_) => "EvidenceRetractionId",
            RecordRef::ProvenanceRetraction(_) => "ProvenanceRetractionId",
            RecordRef::EntityRetirement(_) => "EntityRetirementId",
            RecordRef::PerspectiveRetirement(_) => "PerspectiveRetirementId",
            RecordRef::ArchiveTransition(_) => "ArchiveTransitionId",
            RecordRef::TransferLineage(_) => "TransferLineageId",
        }
    }

    #[test]
    fn unknown_record_ref_wire_tags_fail_closed() {
        for tag in [0, 26, u16::MAX] {
            let result = RecordRefWireTag::try_from(tag);
            assert_eq!(result, Err(super::UnknownRecordRefWireTag { tag }));
        }
    }

    #[test]
    fn all_record_ref_subset_conversions_match_the_closed_memberships()
    -> Result<(), crate::ids::IdValidationError> {
        let references = record_refs()?;
        for reference in &references {
            let evidence = EvidenceTargetRef::try_from(*reference);
            assert_eq!(evidence.is_ok(), is_evidence_target(*reference));
            if !is_evidence_target(*reference) {
                assert_eq!(evidence, Err(RecordRefConversionError::NotEvidenceTarget));
            }

            let provenance = ProvenanceEndpointRef::try_from(*reference);
            assert_eq!(provenance.is_ok(), is_provenance_endpoint(*reference));
            if !is_provenance_endpoint(*reference) {
                assert_eq!(
                    provenance,
                    Err(RecordRefConversionError::NotProvenanceEndpoint)
                );
            }

            let lifecycle = LifecycleTargetRef::try_from(*reference);
            assert_eq!(lifecycle.is_ok(), is_lifecycle_target(*reference));
            if !is_lifecycle_target(*reference) {
                assert_eq!(lifecycle, Err(RecordRefConversionError::NotLifecycleTarget));
            }

            let archive = ArchiveTargetRef::try_from(*reference);
            assert_eq!(archive.is_ok(), is_archive_target(*reference));
            if !is_archive_target(*reference) {
                assert_eq!(archive, Err(RecordRefConversionError::NotArchiveTarget));
            }

            let event_relation = EventRelationProvenanceRef::try_from(*reference);
            assert_eq!(
                event_relation.is_ok(),
                is_event_relation_provenance(*reference)
            );
            if !is_event_relation_provenance(*reference) {
                assert_eq!(
                    event_relation,
                    Err(RecordRefConversionError::NotEventRelationProvenance)
                );
            }
        }
        assert_eq!(
            references
                .iter()
                .filter(|reference| is_evidence_target(**reference))
                .count(),
            21
        );
        assert_eq!(
            references
                .iter()
                .filter(|reference| is_provenance_endpoint(**reference))
                .count(),
            23
        );
        assert_eq!(
            references
                .iter()
                .filter(|reference| is_lifecycle_target(**reference))
                .count(),
            8
        );
        assert_eq!(
            references
                .iter()
                .filter(|reference| is_archive_target(**reference))
                .count(),
            24
        );
        assert_eq!(
            references
                .iter()
                .filter(|reference| is_event_relation_provenance(**reference))
                .count(),
            2
        );
        Ok(())
    }

    #[test]
    fn database_bound_ref_preserves_both_the_database_and_closed_reference()
    -> Result<(), crate::ids::IdValidationError> {
        let database = uuid::<DatabaseId>(31)?;
        let reference = RecordRef::Assertion(uuid::<AssertionId>(32)?);
        let bound = DatabaseBoundRef::new(database, reference);
        assert_eq!(bound.database_id(), database);
        assert_eq!(bound.reference(), reference);

        let snapshot_id = uuid::<SnapshotId>(33)?;
        let session_snapshot = SnapshotRef::new(snapshot_id);
        assert_eq!(session_snapshot.id(), snapshot_id);
        let job_id = uuid::<JobId>(34)?;
        let job = JobRef::new(job_id);
        assert_eq!(job.id(), job_id);
        Ok(())
    }

    #[test]
    fn separate_operational_reference_families_are_typed_and_closed()
    -> Result<(), crate::ids::IdValidationError> {
        let _schema = SchemaRecordRef::Layer(uuid::<LayerId>(41)?);
        let _schema = SchemaRecordRef::EntityType(uuid::<EntityTypeId>(42)?);
        let _schema = SchemaRecordRef::Predicate(uuid::<PredicateId>(43)?);
        let _schema = SchemaRecordRef::EventKind(uuid::<EventKindId>(44)?);
        let _schema = SchemaRecordRef::EventRole(uuid::<EventRoleId>(45)?);
        let _schema = SchemaRecordRef::EventAttribute(uuid::<EventAttributeId>(46)?);
        let _schema = SchemaRecordRef::Timeline(uuid::<TimelineId>(47)?);
        let _migration = MigrationRecordRef::Migration(uuid::<MigrationId>(48)?);
        let _migration = MigrationRecordRef::Run(uuid::<MigrationRunId>(49)?);
        let _migration = MigrationRecordRef::Step(uuid::<MigrationStepId>(50)?);
        let _security = SecurityRecordRef::PolicyRecord(uuid::<SecurityPolicyRecordId>(51)?);
        let _security = SecurityRecordRef::Principal(uuid::<PrincipalId>(52)?);
        let _security = SecurityRecordRef::Role(uuid::<RoleId>(53)?);
        let _security = SecurityRecordRef::RoleAssignment(uuid::<RoleAssignmentId>(54)?);
        let _security = SecurityRecordRef::PolicyRule(uuid::<PolicyRuleId>(55)?);
        let _transaction = TransactionRef::Transaction(uuid::<TransactionId>(56)?);
        let _transaction = TransactionRef::Operation(uuid::<OperationId>(57)?);
        let _audit = AuditRecordRef::Record(uuid::<AuditRecordId>(58)?);
        let _audit = AuditRecordRef::Operation(uuid::<AuditOperationId>(59)?);
        Ok(())
    }
}
