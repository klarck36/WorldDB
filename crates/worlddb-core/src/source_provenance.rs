//! Immutable source, evidence, and provenance records with closed endpoints.

use std::fmt;

use crate::ids::{
    ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
    EntityRetirementId, EventId, EventMaskId, EventMaskRetractionId, EventRelationRetractionId,
    EventRetractionId, EventSpanClosureId, EvidenceId, EvidenceRetractionId, MaskId,
    MaskRetractionId, MaskValidityClosureId, PerspectiveRetirementId, ProvenanceId,
    ProvenanceRetractionId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
    ReplacementBoundaryValidityClosureId, Revision, SourceId,
};
use crate::values::{Bytes, Symbol, Value};

/// A non-empty source locator preserved byte-for-byte as UTF-8.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceLocator(String);

impl SourceLocator {
    /// Creates a locator; empty strings are not used to encode absence.
    pub fn new(value: impl Into<String>) -> Result<Self, SourceEvidenceProvenanceError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SourceEvidenceProvenanceError::EmptySourceLocator);
        }
        Ok(Self(value))
    }

    /// Returns the exact stored locator text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An opaque non-empty digest attached to a Source.
///
/// M1 records the bytes without selecting a digest algorithm or fixed length;
/// those are not specified for a Source's optional `ContentDigest` field.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceContentDigest(Bytes);

impl SourceContentDigest {
    /// Creates a digest value without interpreting its algorithm or length.
    pub fn new(bytes: Bytes) -> Result<Self, SourceEvidenceProvenanceError> {
        if bytes.is_empty() {
            return Err(SourceEvidenceProvenanceError::EmptySourceContentDigest);
        }
        Ok(Self(bytes))
    }

    /// Returns the exact digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &Bytes {
        &self.0
    }
}

/// One named typed metadata value attached to a Source.
#[derive(Clone, Debug)]
pub struct SourceMetadataEntry {
    key: Symbol,
    value: Value,
}

impl SourceMetadataEntry {
    /// Creates a metadata entry using the validated WorldDB Symbol grammar.
    #[must_use]
    pub const fn new(key: Symbol, value: Value) -> Self {
        Self { key, value }
    }

    /// Returns the metadata key.
    #[must_use]
    pub const fn key(&self) -> &Symbol {
        &self.key
    }

    /// Returns the typed metadata value.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }
}

/// Canonically ordered, duplicate-free Source metadata.
#[derive(Clone, Debug, Default)]
pub struct SourceMetadata(Vec<SourceMetadataEntry>);

impl SourceMetadata {
    /// Sorts by key and rejects duplicate metadata keys.
    pub fn new(
        mut entries: Vec<SourceMetadataEntry>,
    ) -> Result<Self, SourceEvidenceProvenanceError> {
        entries.sort_by(|left, right| left.key.cmp(&right.key));
        for pair in entries.windows(2) {
            if let [left, right] = pair {
                if left.key == right.key {
                    return Err(SourceEvidenceProvenanceError::DuplicateSourceMetadataKey {
                        key: left.key.clone(),
                    });
                }
            }
        }
        Ok(Self(entries))
    }

    /// Returns metadata entries in canonical key order.
    #[must_use]
    pub fn as_slice(&self) -> &[SourceMetadataEntry] {
        &self.0
    }
}

/// One immutable project-wide record describing a source of information.
#[derive(Clone, Debug)]
pub struct Source {
    id: SourceId,
    source_kind: Symbol,
    locator: Option<SourceLocator>,
    content_digest: Option<SourceContentDigest>,
    metadata: SourceMetadata,
    created_revision: Revision,
}

impl Source {
    /// Creates a Source. Its metadata alone makes no claim about any domain record.
    #[must_use]
    pub const fn new(
        id: SourceId,
        source_kind: Symbol,
        locator: Option<SourceLocator>,
        content_digest: Option<SourceContentDigest>,
        metadata: SourceMetadata,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            source_kind,
            locator,
            content_digest,
            metadata,
            created_revision,
        }
    }

    /// Returns the Source identity.
    #[must_use]
    pub const fn id(&self) -> SourceId {
        self.id
    }

    /// Returns the validated source-kind symbol.
    #[must_use]
    pub const fn source_kind(&self) -> &Symbol {
        &self.source_kind
    }

    /// Returns the optional locator.
    #[must_use]
    pub const fn locator(&self) -> Option<&SourceLocator> {
        self.locator.as_ref()
    }

    /// Returns the optional opaque content digest.
    #[must_use]
    pub const fn content_digest(&self) -> Option<&SourceContentDigest> {
        self.content_digest.as_ref()
    }

    /// Returns canonical source metadata.
    #[must_use]
    pub const fn metadata(&self) -> &SourceMetadata {
        &self.metadata
    }

    /// Returns the Transaction-Time revision that created this Source.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// The closed set of record families that may be an Evidence target.
///
/// Source and Evidence are intentionally absent. EventRelation is governed by
/// its own relation-reference contract; ArchiveTransition remains an eligible
/// lifecycle target under the generic §31.2.1 lifecycle row.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EvidenceTargetRef {
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
    /// An EventRelationRetraction lifecycle record.
    EventRelationRetraction(EventRelationRetractionId),
    /// An EvidenceRetraction record.
    EvidenceRetraction(EvidenceRetractionId),
    /// A ProvenanceRetraction record.
    ProvenanceRetraction(ProvenanceRetractionId),
    /// An EntityRetirement record.
    EntityRetirement(EntityRetirementId),
    /// A PerspectiveRetirement record.
    PerspectiveRetirement(PerspectiveRetirementId),
    /// A ProvenanceEdge record.
    Provenance(ProvenanceId),
    /// An ArchiveTransition lifecycle record.
    ArchiveTransition(ArchiveTransitionId),
}

/// The explanatory relation from a Source to an Evidence target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EvidenceRelation {
    /// The source supports the target record.
    Supports,
    /// The source contradicts the target record.
    Contradicts,
    /// The source documents the target record without asserting support.
    Documents,
}

/// An immutable, explicit link from a Source to a closed Evidence target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Evidence {
    id: EvidenceId,
    source_id: SourceId,
    target: EvidenceTargetRef,
    relation: EvidenceRelation,
    created_revision: Revision,
}

impl Evidence {
    /// Creates an Evidence link; target existence and authorization are checked by the operation context.
    #[must_use]
    pub const fn new(
        id: EvidenceId,
        source_id: SourceId,
        target: EvidenceTargetRef,
        relation: EvidenceRelation,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            source_id,
            target,
            relation,
            created_revision,
        }
    }

    /// Returns the Evidence identity.
    #[must_use]
    pub const fn id(self) -> EvidenceId {
        self.id
    }

    /// Returns the linked Source identity.
    #[must_use]
    pub const fn source_id(self) -> SourceId {
        self.source_id
    }

    /// Returns the typed target.
    #[must_use]
    pub const fn target(self) -> EvidenceTargetRef {
        self.target
    }

    /// Returns the explanatory relation.
    #[must_use]
    pub const fn relation(self) -> EvidenceRelation {
        self.relation
    }

    /// Returns the Transaction-Time creation revision.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// One closed relation class for a ProvenanceEdge.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProvenanceRelation {
    /// Explains a correction; it does not retract or supersede its target.
    Corrects,
    /// Records an information/logical derivation.
    DerivedFrom,
    /// Records a domain/story consequence without creating an automatic cascade.
    ResultedFrom,
}

/// A closed union of generic Provenance endpoints.
///
/// EventRelation itself is deliberately excluded; it has a separate relation
/// provenance contract. ArchiveTransition is included by the generic
/// Lifecycle Record endpoint rule. Schema, migration, security, transaction,
/// snapshot, job, and audit records use their own reference types.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProvenanceEndpointRef {
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
    /// An EventRelationRetraction lifecycle record.
    EventRelationRetraction(EventRelationRetractionId),
    /// An EvidenceRetraction record.
    EvidenceRetraction(EvidenceRetractionId),
    /// A ProvenanceRetraction record.
    ProvenanceRetraction(ProvenanceRetractionId),
    /// An EntityRetirement record.
    EntityRetirement(EntityRetirementId),
    /// A PerspectiveRetirement record.
    PerspectiveRetirement(PerspectiveRetirementId),
    /// An ArchiveTransition lifecycle record.
    ArchiveTransition(ArchiveTransitionId),
}

impl ProvenanceEndpointRef {
    fn family(self) -> ProvenanceEndpointFamily {
        match self {
            Self::Assertion(_) => ProvenanceEndpointFamily::Assertion,
            Self::Mask(_) => ProvenanceEndpointFamily::Mask,
            Self::ReplacementBoundary(_) => ProvenanceEndpointFamily::ReplacementBoundary,
            Self::Event(_) => ProvenanceEndpointFamily::Event,
            Self::EventMask(_) => ProvenanceEndpointFamily::EventMask,
            Self::Source(_) => ProvenanceEndpointFamily::Source,
            Self::Evidence(_) => ProvenanceEndpointFamily::Evidence,
            Self::Provenance(_) => ProvenanceEndpointFamily::Provenance,
            Self::AssertionValidityClosure(_) => ProvenanceEndpointFamily::AssertionValidityClosure,
            Self::AssertionRetraction(_) => ProvenanceEndpointFamily::AssertionRetraction,
            Self::MaskValidityClosure(_) => ProvenanceEndpointFamily::MaskValidityClosure,
            Self::MaskRetraction(_) => ProvenanceEndpointFamily::MaskRetraction,
            Self::ReplacementBoundaryValidityClosure(_) => {
                ProvenanceEndpointFamily::ReplacementBoundaryValidityClosure
            }
            Self::ReplacementBoundaryRetraction(_) => {
                ProvenanceEndpointFamily::ReplacementBoundaryRetraction
            }
            Self::EventSpanClosure(_) => ProvenanceEndpointFamily::EventSpanClosure,
            Self::EventRetraction(_) => ProvenanceEndpointFamily::EventRetraction,
            Self::EventMaskRetraction(_) => ProvenanceEndpointFamily::EventMaskRetraction,
            Self::EventRelationRetraction(_) => ProvenanceEndpointFamily::EventRelationRetraction,
            Self::EvidenceRetraction(_) => ProvenanceEndpointFamily::EvidenceRetraction,
            Self::ProvenanceRetraction(_) => ProvenanceEndpointFamily::ProvenanceRetraction,
            Self::EntityRetirement(_) => ProvenanceEndpointFamily::EntityRetirement,
            Self::PerspectiveRetirement(_) => ProvenanceEndpointFamily::PerspectiveRetirement,
            Self::ArchiveTransition(_) => ProvenanceEndpointFamily::ArchiveTransition,
        }
    }

    fn allowed_as_derived_from(self) -> bool {
        matches!(
            self,
            Self::Assertion(_)
                | Self::Event(_)
                | Self::Source(_)
                | Self::Evidence(_)
                | Self::Provenance(_)
                | Self::AssertionValidityClosure(_)
                | Self::AssertionRetraction(_)
                | Self::MaskValidityClosure(_)
                | Self::MaskRetraction(_)
                | Self::ReplacementBoundaryValidityClosure(_)
                | Self::ReplacementBoundaryRetraction(_)
                | Self::EventSpanClosure(_)
                | Self::EventRetraction(_)
                | Self::EventMaskRetraction(_)
                | Self::EventRelationRetraction(_)
                | Self::EvidenceRetraction(_)
                | Self::ProvenanceRetraction(_)
                | Self::EntityRetirement(_)
                | Self::PerspectiveRetirement(_)
                | Self::ArchiveTransition(_)
        )
    }

    fn allowed_as_resulted_from(self) -> bool {
        matches!(self, Self::Assertion(_) | Self::Event(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ProvenanceEndpointFamily {
    Assertion,
    Mask,
    ReplacementBoundary,
    Event,
    EventMask,
    Source,
    Evidence,
    Provenance,
    AssertionValidityClosure,
    AssertionRetraction,
    MaskValidityClosure,
    MaskRetraction,
    ReplacementBoundaryValidityClosure,
    ReplacementBoundaryRetraction,
    EventSpanClosure,
    EventRetraction,
    EventMaskRetraction,
    EventRelationRetraction,
    EvidenceRetraction,
    ProvenanceRetraction,
    EntityRetirement,
    PerspectiveRetirement,
    ArchiveTransition,
}

/// Which side of a Provenance relation an endpoint occupies.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProvenanceEndpointSide {
    /// The source/left endpoint.
    From,
    /// The target/right endpoint.
    To,
}

/// One immutable explanatory edge between typed record endpoints.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProvenanceEdge {
    id: ProvenanceId,
    from: ProvenanceEndpointRef,
    to: ProvenanceEndpointRef,
    relation: ProvenanceRelation,
    created_revision: Revision,
}

impl ProvenanceEdge {
    /// Creates an edge after checking the closed relation/endpoint matrix and self-loop rule.
    pub fn new(
        id: ProvenanceId,
        from: ProvenanceEndpointRef,
        to: ProvenanceEndpointRef,
        relation: ProvenanceRelation,
        created_revision: Revision,
    ) -> Result<Self, SourceEvidenceProvenanceError> {
        if from == to {
            return Err(SourceEvidenceProvenanceError::ProvenanceSelfLoop { endpoint: from });
        }
        if relation == ProvenanceRelation::Corrects && from.family() != to.family() {
            return Err(SourceEvidenceProvenanceError::CorrectsEndpointFamilyMismatch { from, to });
        }
        if relation == ProvenanceRelation::DerivedFrom && !from.allowed_as_derived_from() {
            return Err(SourceEvidenceProvenanceError::ForbiddenProvenanceEndpoint {
                relation,
                side: ProvenanceEndpointSide::From,
                endpoint: from,
            });
        }
        if relation == ProvenanceRelation::ResultedFrom && !from.allowed_as_resulted_from() {
            return Err(SourceEvidenceProvenanceError::ForbiddenProvenanceEndpoint {
                relation,
                side: ProvenanceEndpointSide::From,
                endpoint: from,
            });
        }
        if relation == ProvenanceRelation::ResultedFrom && !to.allowed_as_resulted_from() {
            return Err(SourceEvidenceProvenanceError::ForbiddenProvenanceEndpoint {
                relation,
                side: ProvenanceEndpointSide::To,
                endpoint: to,
            });
        }
        Ok(Self {
            id,
            from,
            to,
            relation,
            created_revision,
        })
    }

    /// Returns this edge's stable identity.
    #[must_use]
    pub const fn id(self) -> ProvenanceId {
        self.id
    }

    /// Returns the typed source endpoint.
    #[must_use]
    pub const fn from(self) -> ProvenanceEndpointRef {
        self.from
    }

    /// Returns the typed destination endpoint.
    #[must_use]
    pub const fn to(self) -> ProvenanceEndpointRef {
        self.to
    }

    /// Returns the explanatory relation.
    #[must_use]
    pub const fn relation(self) -> ProvenanceRelation {
        self.relation
    }

    /// Returns the Transaction-Time creation revision.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// One immutable Transaction-Time retraction of an Evidence edge.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EvidenceRetraction {
    id: EvidenceRetractionId,
    evidence_id: EvidenceId,
    reason: String,
    created_revision: Revision,
}

impl EvidenceRetraction {
    pub(crate) const fn from_wire_fields(
        id: EvidenceRetractionId,
        evidence_id: EvidenceId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            evidence_id,
            reason,
            created_revision,
        }
    }

    /// Creates a retraction strictly after its Evidence target.
    pub fn new(
        id: EvidenceRetractionId,
        evidence: &Evidence,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, SourceEvidenceProvenanceError> {
        validate_lifecycle_revision(evidence.created_revision(), created_revision)?;
        Ok(Self {
            id,
            evidence_id: evidence.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Returns the concrete retraction identity.
    #[must_use]
    pub const fn id(&self) -> EvidenceRetractionId {
        self.id
    }

    /// Returns the target Evidence identity.
    #[must_use]
    pub const fn evidence_id(&self) -> EvidenceId {
        self.evidence_id
    }

    /// Returns the required retraction reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time retraction revision.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// One immutable Transaction-Time retraction of a Provenance edge.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProvenanceRetraction {
    id: ProvenanceRetractionId,
    provenance_id: ProvenanceId,
    reason: String,
    created_revision: Revision,
}

impl ProvenanceRetraction {
    pub(crate) const fn from_wire_fields(
        id: ProvenanceRetractionId,
        provenance_id: ProvenanceId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            provenance_id,
            reason,
            created_revision,
        }
    }

    /// Creates a retraction strictly after its Provenance target.
    pub fn new(
        id: ProvenanceRetractionId,
        provenance: &ProvenanceEdge,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, SourceEvidenceProvenanceError> {
        validate_lifecycle_revision(provenance.created_revision(), created_revision)?;
        Ok(Self {
            id,
            provenance_id: provenance.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Returns the concrete retraction identity.
    #[must_use]
    pub const fn id(&self) -> ProvenanceRetractionId {
        self.id
    }

    /// Returns the target Provenance identity.
    #[must_use]
    pub const fn provenance_id(&self) -> ProvenanceId {
        self.provenance_id
    }

    /// Returns the required retraction reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time retraction revision.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

fn validate_lifecycle_revision(
    target_revision: Revision,
    lifecycle_revision: Revision,
) -> Result<(), SourceEvidenceProvenanceError> {
    if lifecycle_revision <= target_revision {
        return Err(
            SourceEvidenceProvenanceError::LifecycleRevisionNotAfterTarget {
                target_revision,
                lifecycle_revision,
            },
        );
    }
    Ok(())
}

/// A rejected source field, forbidden endpoint, or lifecycle value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceEvidenceProvenanceError {
    /// A present locator must contain at least one byte.
    EmptySourceLocator,
    /// A present digest must contain at least one byte.
    EmptySourceContentDigest,
    /// Source metadata keys are unique within one Source.
    DuplicateSourceMetadataKey {
        /// The repeated validated key.
        key: Symbol,
    },
    /// A Provenance edge cannot connect a record to itself.
    ProvenanceSelfLoop {
        /// The identical typed endpoint on both sides.
        endpoint: ProvenanceEndpointRef,
    },
    /// Corrects endpoints must use the same record family and lifecycle variant.
    CorrectsEndpointFamilyMismatch {
        /// The left endpoint.
        from: ProvenanceEndpointRef,
        /// The right endpoint.
        to: ProvenanceEndpointRef,
    },
    /// The selected relation disallows this endpoint on the indicated side.
    ForbiddenProvenanceEndpoint {
        /// The selected relation kind.
        relation: ProvenanceRelation,
        /// The endpoint side that violates the matrix.
        side: ProvenanceEndpointSide,
        /// The offending typed endpoint.
        endpoint: ProvenanceEndpointRef,
    },
    /// A lifecycle record must be later than its target on Transaction Time.
    LifecycleRevisionNotAfterTarget {
        /// The target record creation revision.
        target_revision: Revision,
        /// The proposed lifecycle-record revision.
        lifecycle_revision: Revision,
    },
}

impl fmt::Display for SourceEvidenceProvenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySourceLocator => formatter.write_str("Source locator must not be empty"),
            Self::EmptySourceContentDigest => {
                formatter.write_str("Source content digest must not be empty")
            }
            Self::DuplicateSourceMetadataKey { key } => {
                write!(formatter, "duplicate Source metadata key {key}")
            }
            Self::ProvenanceSelfLoop { endpoint } => {
                write!(
                    formatter,
                    "Provenance relation cannot self-reference {endpoint:?}"
                )
            }
            Self::CorrectsEndpointFamilyMismatch { from, to } => write!(
                formatter,
                "Corrects endpoints must use the same record family: {from:?} -> {to:?}"
            ),
            Self::ForbiddenProvenanceEndpoint {
                relation,
                side,
                endpoint,
            } => write!(
                formatter,
                "{relation:?} does not allow {endpoint:?} as {side:?} endpoint"
            ),
            Self::LifecycleRevisionNotAfterTarget {
                target_revision,
                lifecycle_revision,
            } => write!(
                formatter,
                "lifecycle revision {lifecycle_revision} must be after target revision {target_revision}"
            ),
        }
    }
}

impl std::error::Error for SourceEvidenceProvenanceError {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::{
        Evidence, EvidenceRelation, EvidenceRetraction, EvidenceTargetRef, ProvenanceEdge,
        ProvenanceEndpointRef, ProvenanceEndpointSide, ProvenanceRelation, ProvenanceRetraction,
        Source, SourceContentDigest, SourceEvidenceProvenanceError, SourceLocator, SourceMetadata,
        SourceMetadataEntry,
    };
    use crate::ids::{
        ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
        DomainId, EntityRetirementId, EventId, EventMaskId, EventMaskRetractionId,
        EventRelationRetractionId, EventRetractionId, EventSpanClosureId, EvidenceId,
        EvidenceRetractionId, IdValidationError, MaskId, MaskRetractionId, MaskValidityClosureId,
        PerspectiveRetirementId, ProvenanceId, ProvenanceRetractionId, ReplacementBoundaryId,
        ReplacementBoundaryRetractionId, ReplacementBoundaryValidityClosureId, Revision,
        RevisionError, SourceId,
    };
    use crate::values::{Bytes, Symbol, SymbolError, Value};

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Record(SourceEvidenceProvenanceError),
        Id(IdValidationError),
        Revision(RevisionError),
        Symbol(SymbolError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Record(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl Error for TestError {}

    macro_rules! error_conversion {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    error_conversion!(SourceEvidenceProvenanceError, Record);
    error_conversion!(IdValidationError, Id);
    error_conversion!(RevisionError, Revision);
    error_conversion!(SymbolError, Symbol);

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    fn uuid<T: DomainId>(byte: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn source_metadata() -> Result<SourceMetadata, TestError> {
        Ok(value!(SourceMetadata::new(vec![SourceMetadataEntry::new(
            value!(Symbol::new("author")),
            Value::String(String::from("A. Author")),
        )])))
    }

    fn provenance_id(byte: u8) -> Result<ProvenanceId, IdValidationError> {
        uuid::<ProvenanceId>(byte)
    }

    fn endpoint_samples() -> Result<Vec<ProvenanceEndpointRef>, IdValidationError> {
        endpoint_samples_with_offset(0)
    }

    fn endpoint_samples_with_offset(
        offset: u8,
    ) -> Result<Vec<ProvenanceEndpointRef>, IdValidationError> {
        Ok(vec![
            ProvenanceEndpointRef::Assertion(uuid::<AssertionId>(1 + offset)?),
            ProvenanceEndpointRef::Mask(uuid::<MaskId>(2 + offset)?),
            ProvenanceEndpointRef::ReplacementBoundary(uuid::<ReplacementBoundaryId>(3 + offset)?),
            ProvenanceEndpointRef::Event(uuid::<EventId>(4 + offset)?),
            ProvenanceEndpointRef::EventMask(uuid::<EventMaskId>(5 + offset)?),
            ProvenanceEndpointRef::Source(uuid::<SourceId>(6 + offset)?),
            ProvenanceEndpointRef::Evidence(uuid::<EvidenceId>(7 + offset)?),
            ProvenanceEndpointRef::Provenance(uuid::<ProvenanceId>(8 + offset)?),
            ProvenanceEndpointRef::AssertionValidityClosure(uuid::<AssertionValidityClosureId>(
                9 + offset,
            )?),
            ProvenanceEndpointRef::AssertionRetraction(uuid::<AssertionRetractionId>(10 + offset)?),
            ProvenanceEndpointRef::MaskValidityClosure(uuid::<MaskValidityClosureId>(11 + offset)?),
            ProvenanceEndpointRef::MaskRetraction(uuid::<MaskRetractionId>(12 + offset)?),
            ProvenanceEndpointRef::ReplacementBoundaryValidityClosure(uuid::<
                ReplacementBoundaryValidityClosureId,
            >(13 + offset)?),
            ProvenanceEndpointRef::ReplacementBoundaryRetraction(uuid::<
                ReplacementBoundaryRetractionId,
            >(14 + offset)?),
            ProvenanceEndpointRef::EventSpanClosure(uuid::<EventSpanClosureId>(15 + offset)?),
            ProvenanceEndpointRef::EventRetraction(uuid::<EventRetractionId>(16 + offset)?),
            ProvenanceEndpointRef::EventMaskRetraction(uuid::<EventMaskRetractionId>(17 + offset)?),
            ProvenanceEndpointRef::EventRelationRetraction(uuid::<EventRelationRetractionId>(
                18 + offset,
            )?),
            ProvenanceEndpointRef::EvidenceRetraction(uuid::<EvidenceRetractionId>(19 + offset)?),
            ProvenanceEndpointRef::ProvenanceRetraction(uuid::<ProvenanceRetractionId>(
                20 + offset,
            )?),
            ProvenanceEndpointRef::EntityRetirement(uuid::<EntityRetirementId>(21 + offset)?),
            ProvenanceEndpointRef::PerspectiveRetirement(uuid::<PerspectiveRetirementId>(
                22 + offset,
            )?),
            ProvenanceEndpointRef::ArchiveTransition(uuid::<ArchiveTransitionId>(23 + offset)?),
        ])
    }

    fn evidence_target_samples() -> Result<Vec<EvidenceTargetRef>, IdValidationError> {
        Ok(vec![
            EvidenceTargetRef::Assertion(uuid::<AssertionId>(1)?),
            EvidenceTargetRef::Mask(uuid::<MaskId>(2)?),
            EvidenceTargetRef::ReplacementBoundary(uuid::<ReplacementBoundaryId>(3)?),
            EvidenceTargetRef::Event(uuid::<EventId>(4)?),
            EvidenceTargetRef::EventMask(uuid::<EventMaskId>(5)?),
            EvidenceTargetRef::AssertionValidityClosure(uuid::<AssertionValidityClosureId>(6)?),
            EvidenceTargetRef::AssertionRetraction(uuid::<AssertionRetractionId>(7)?),
            EvidenceTargetRef::MaskValidityClosure(uuid::<MaskValidityClosureId>(8)?),
            EvidenceTargetRef::MaskRetraction(uuid::<MaskRetractionId>(9)?),
            EvidenceTargetRef::ReplacementBoundaryValidityClosure(uuid::<
                ReplacementBoundaryValidityClosureId,
            >(10)?),
            EvidenceTargetRef::ReplacementBoundaryRetraction(uuid::<
                ReplacementBoundaryRetractionId,
            >(11)?),
            EvidenceTargetRef::EventSpanClosure(uuid::<EventSpanClosureId>(12)?),
            EvidenceTargetRef::EventRetraction(uuid::<EventRetractionId>(13)?),
            EvidenceTargetRef::EventMaskRetraction(uuid::<EventMaskRetractionId>(14)?),
            EvidenceTargetRef::EventRelationRetraction(uuid::<EventRelationRetractionId>(15)?),
            EvidenceTargetRef::EvidenceRetraction(uuid::<EvidenceRetractionId>(16)?),
            EvidenceTargetRef::ProvenanceRetraction(uuid::<ProvenanceRetractionId>(17)?),
            EvidenceTargetRef::EntityRetirement(uuid::<EntityRetirementId>(18)?),
            EvidenceTargetRef::PerspectiveRetirement(uuid::<PerspectiveRetirementId>(19)?),
            EvidenceTargetRef::Provenance(uuid::<ProvenanceId>(20)?),
            EvidenceTargetRef::ArchiveTransition(uuid::<ArchiveTransitionId>(21)?),
        ])
    }

    #[test]
    fn source_metadata_is_sorted_unique_and_optional_fields_are_not_empty_sentinels() -> TestResult
    {
        let source = Source::new(
            value!(uuid::<SourceId>(1)),
            value!(Symbol::new("book")),
            Some(value!(SourceLocator::new("https://example.invalid/book"))),
            Some(value!(SourceContentDigest::new(Bytes::new(vec![1, 2, 3])))),
            source_metadata()?,
            value!(revision(2)),
        );
        assert_eq!(source.source_kind().as_str(), "book");
        assert_eq!(
            source.locator().map(SourceLocator::as_str),
            Some("https://example.invalid/book")
        );
        assert_eq!(
            source
                .content_digest()
                .map(SourceContentDigest::as_bytes)
                .map(Bytes::as_slice),
            Some([1, 2, 3].as_slice())
        );

        assert_eq!(
            SourceLocator::new(""),
            Err(SourceEvidenceProvenanceError::EmptySourceLocator)
        );
        assert_eq!(
            SourceContentDigest::new(Bytes::default()),
            Err(SourceEvidenceProvenanceError::EmptySourceContentDigest)
        );

        let zeta = value!(Symbol::new("zeta"));
        let alpha = value!(Symbol::new("alpha"));
        let metadata = value!(SourceMetadata::new(vec![
            SourceMetadataEntry::new(zeta, Value::Bool(true)),
            SourceMetadataEntry::new(alpha.clone(), Value::UInt(crate::UInt::new(1))),
        ]));
        assert_eq!(
            metadata.as_slice().first().map(|entry| entry.key()),
            Some(&alpha)
        );
        assert!(matches!(
            SourceMetadata::new(vec![
                SourceMetadataEntry::new(alpha.clone(), Value::Bool(true)),
                SourceMetadataEntry::new(alpha.clone(), Value::Bool(false)),
            ]),
            Err(SourceEvidenceProvenanceError::DuplicateSourceMetadataKey { key }) if key == alpha
        ));
        Ok(())
    }

    #[test]
    fn evidence_links_sources_to_only_typed_allowed_target_families() -> TestResult {
        for (index, target) in evidence_target_samples()?.into_iter().enumerate() {
            let evidence = Evidence::new(
                value!(uuid::<EvidenceId>(index as u8 + 23)),
                value!(uuid::<SourceId>(24)),
                target,
                EvidenceRelation::Supports,
                value!(revision(3)),
            );
            assert_eq!(evidence.target(), target);
            assert_eq!(evidence.relation(), EvidenceRelation::Supports);
            assert_eq!(evidence.source_id(), value!(uuid::<SourceId>(24)));
        }
        Ok(())
    }

    #[test]
    fn derived_from_rejects_exactly_the_disallowed_from_families() -> TestResult {
        for (index, from) in value!(endpoint_samples()).into_iter().enumerate() {
            let result = ProvenanceEdge::new(
                value!(provenance_id(index as u8 + 60)),
                from,
                ProvenanceEndpointRef::Assertion(value!(uuid::<AssertionId>(123))),
                ProvenanceRelation::DerivedFrom,
                Revision::GENESIS,
            );
            match from {
                ProvenanceEndpointRef::Mask(_)
                | ProvenanceEndpointRef::ReplacementBoundary(_)
                | ProvenanceEndpointRef::EventMask(_) => assert_eq!(
                    result.err(),
                    Some(SourceEvidenceProvenanceError::ForbiddenProvenanceEndpoint {
                        relation: ProvenanceRelation::DerivedFrom,
                        side: ProvenanceEndpointSide::From,
                        endpoint: from,
                    })
                ),
                _ => assert!(
                    result.is_ok(),
                    "DerivedFrom rejected allowed source {from:?}"
                ),
            }
        }
        Ok(())
    }

    #[test]
    fn resulted_from_accepts_only_assertion_or_event_on_each_side() -> TestResult {
        let disallowed = value!(endpoint_samples())
            .into_iter()
            .filter(|endpoint| {
                !matches!(
                    endpoint,
                    ProvenanceEndpointRef::Assertion(_) | ProvenanceEndpointRef::Event(_)
                )
            })
            .collect::<Vec<_>>();
        for (index, endpoint) in disallowed.into_iter().enumerate() {
            assert_eq!(
                ProvenanceEdge::new(
                    value!(provenance_id(index as u8 + 100)),
                    endpoint,
                    ProvenanceEndpointRef::Assertion(value!(uuid::<AssertionId>(120))),
                    ProvenanceRelation::ResultedFrom,
                    Revision::GENESIS,
                )
                .err(),
                Some(SourceEvidenceProvenanceError::ForbiddenProvenanceEndpoint {
                    relation: ProvenanceRelation::ResultedFrom,
                    side: ProvenanceEndpointSide::From,
                    endpoint,
                })
            );
            assert_eq!(
                ProvenanceEdge::new(
                    value!(provenance_id(index as u8 + 130)),
                    ProvenanceEndpointRef::Event(value!(uuid::<EventId>(121))),
                    endpoint,
                    ProvenanceRelation::ResultedFrom,
                    Revision::GENESIS,
                )
                .err(),
                Some(SourceEvidenceProvenanceError::ForbiddenProvenanceEndpoint {
                    relation: ProvenanceRelation::ResultedFrom,
                    side: ProvenanceEndpointSide::To,
                    endpoint,
                })
            );
        }
        let story_from = [
            ProvenanceEndpointRef::Assertion(value!(uuid::<AssertionId>(151))),
            ProvenanceEndpointRef::Event(value!(uuid::<EventId>(152))),
        ];
        let story_to = [
            ProvenanceEndpointRef::Assertion(value!(uuid::<AssertionId>(153))),
            ProvenanceEndpointRef::Event(value!(uuid::<EventId>(154))),
        ];
        for (index, from) in story_from.iter().copied().enumerate() {
            for (target_index, to) in story_to.iter().copied().enumerate() {
                assert!(
                    ProvenanceEdge::new(
                        value!(provenance_id(index as u8 * 2 + target_index as u8 + 150)),
                        from,
                        to,
                        ProvenanceRelation::ResultedFrom,
                        Revision::GENESIS,
                    )
                    .is_ok()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn corrects_requires_same_concrete_family_and_provenance_rejects_self_loops() -> TestResult {
        let from_endpoints = value!(endpoint_samples_with_offset(0));
        let to_endpoints = value!(endpoint_samples_with_offset(32));
        assert_eq!(from_endpoints.len(), 23);
        assert_eq!(to_endpoints.len(), 23);
        for from in from_endpoints {
            for to in to_endpoints.iter().copied() {
                let result = ProvenanceEdge::new(
                    value!(provenance_id(200)),
                    from,
                    to,
                    ProvenanceRelation::Corrects,
                    Revision::GENESIS,
                );
                if std::mem::discriminant(&from) == std::mem::discriminant(&to) {
                    assert!(
                        result.is_ok(),
                        "Corrects rejected compatible pair {from:?} -> {to:?}"
                    );
                } else {
                    assert_eq!(
                        result.err(),
                        Some(
                            SourceEvidenceProvenanceError::CorrectsEndpointFamilyMismatch {
                                from,
                                to,
                            }
                        ),
                        "Corrects accepted incompatible pair {from:?} -> {to:?}"
                    );
                }
            }
        }
        let assertion = ProvenanceEndpointRef::Assertion(value!(uuid::<AssertionId>(1)));
        let event = ProvenanceEndpointRef::Event(value!(uuid::<EventId>(2)));
        assert_eq!(
            ProvenanceEdge::new(
                value!(provenance_id(3)),
                assertion,
                event,
                ProvenanceRelation::Corrects,
                Revision::GENESIS,
            )
            .err(),
            Some(
                SourceEvidenceProvenanceError::CorrectsEndpointFamilyMismatch {
                    from: assertion,
                    to: event,
                }
            )
        );
        assert_eq!(
            ProvenanceEdge::new(
                value!(provenance_id(4)),
                assertion,
                assertion,
                ProvenanceRelation::DerivedFrom,
                Revision::GENESIS,
            )
            .err(),
            Some(SourceEvidenceProvenanceError::ProvenanceSelfLoop {
                endpoint: assertion,
            })
        );
        let closure = ProvenanceEndpointRef::AssertionValidityClosure(value!(uuid::<
            AssertionValidityClosureId,
        >(5)));
        let retraction =
            ProvenanceEndpointRef::AssertionRetraction(value!(uuid::<AssertionRetractionId>(6)));
        assert_eq!(
            ProvenanceEdge::new(
                value!(provenance_id(7)),
                closure,
                retraction,
                ProvenanceRelation::Corrects,
                Revision::GENESIS,
            )
            .err(),
            Some(
                SourceEvidenceProvenanceError::CorrectsEndpointFamilyMismatch {
                    from: closure,
                    to: retraction,
                }
            )
        );
        assert!(
            ProvenanceEdge::new(
                value!(provenance_id(8)),
                ProvenanceEndpointRef::Assertion(value!(uuid::<AssertionId>(9))),
                ProvenanceEndpointRef::Assertion(value!(uuid::<AssertionId>(10))),
                ProvenanceRelation::Corrects,
                Revision::GENESIS,
            )
            .is_ok()
        );
        assert!(
            ProvenanceEdge::new(
                value!(provenance_id(11)),
                ProvenanceEndpointRef::AssertionRetraction(value!(uuid::<AssertionRetractionId>(
                    12
                ))),
                ProvenanceEndpointRef::AssertionRetraction(value!(uuid::<AssertionRetractionId>(
                    13
                ))),
                ProvenanceRelation::Corrects,
                Revision::GENESIS,
            )
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn evidence_and_provenance_retractions_are_distinct_later_records() -> TestResult {
        let evidence = Evidence::new(
            value!(uuid::<EvidenceId>(1)),
            value!(uuid::<SourceId>(2)),
            EvidenceTargetRef::Event(value!(uuid::<EventId>(3))),
            EvidenceRelation::Documents,
            value!(revision(4)),
        );
        let evidence_retraction = value!(EvidenceRetraction::new(
            value!(uuid::<EvidenceRetractionId>(5)),
            &evidence,
            "evidence correction",
            value!(revision(6)),
        ));
        assert_eq!(evidence_retraction.evidence_id(), evidence.id());

        let provenance = value!(ProvenanceEdge::new(
            value!(provenance_id(7)),
            ProvenanceEndpointRef::Event(value!(uuid::<EventId>(8))),
            ProvenanceEndpointRef::Event(value!(uuid::<EventId>(9))),
            ProvenanceRelation::Corrects,
            value!(revision(4)),
        ));
        let provenance_retraction = value!(ProvenanceRetraction::new(
            value!(uuid::<ProvenanceRetractionId>(10)),
            &provenance,
            "provenance correction",
            value!(revision(6)),
        ));
        assert_eq!(provenance_retraction.provenance_id(), provenance.id());
        assert_eq!(provenance_retraction.reason(), "provenance correction");
        assert_eq!(
            EvidenceRetraction::new(
                value!(uuid::<EvidenceRetractionId>(11)),
                &evidence,
                "too early",
                value!(revision(4)),
            )
            .err(),
            Some(
                SourceEvidenceProvenanceError::LifecycleRevisionNotAfterTarget {
                    target_revision: value!(revision(4)),
                    lifecycle_revision: value!(revision(4)),
                }
            )
        );
        Ok(())
    }
}
