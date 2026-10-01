//! Project-wide as-of reference projection for Source and Evidence history.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::catalog::{HistorySpaceCatalog, HistorySpaceError};
use crate::ids::{
    EvidenceId, EvidenceRetractionId, HistorySpaceId, PrincipalId, Revision, SourceId,
};
use crate::query_context::QueryContext;
use crate::record_refs::RecordRef;
use crate::security::{
    AuthorizationDecision, Capability, EvidenceRelationship, FieldSelector, PolicyTarget,
    RelationshipSelector, SecurityPolicySnapshot,
};
use crate::source_provenance::{
    Evidence, EvidenceRelation, EvidenceRetraction, EvidenceTargetRef, Source,
};
use crate::temporal::RecordedAsOf;

/// Transaction-time creation metadata for one internal Evidence target.
///
/// `history_space_id: None` identifies a project-wide target. Scoped targets
/// are visible only through the selected HistorySpace's pinned ancestry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceTargetHistoryEntry {
    target: EvidenceTargetRef,
    created_revision: Revision,
    history_space_id: Option<HistorySpaceId>,
}

impl EvidenceTargetHistoryEntry {
    /// Binds a typed Evidence target to its creation revision and optional origin.
    #[must_use]
    pub const fn new(
        target: EvidenceTargetRef,
        created_revision: Revision,
        history_space_id: Option<HistorySpaceId>,
    ) -> Self {
        Self {
            target,
            created_revision,
            history_space_id,
        }
    }

    /// Returns the closed typed target identity.
    #[must_use]
    pub const fn target(self) -> EvidenceTargetRef {
        self.target
    }

    /// Returns the Transaction-Time creation revision.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }

    /// Returns the target's origin HistorySpace, if it is scoped.
    #[must_use]
    pub const fn history_space_id(self) -> Option<HistorySpaceId> {
        self.history_space_id
    }
}

/// Complete immutable Source, Evidence, retraction, and target inventory for a scan.
#[derive(Clone)]
pub struct SourceEvidenceHistory<'a> {
    sources: &'a [Source],
    evidence: &'a [Evidence],
    retractions: &'a [EvidenceRetraction],
    targets: &'a [EvidenceTargetHistoryEntry],
    authorized_sources: Option<BTreeSet<SourceId>>,
    authorized_evidence: Option<BTreeSet<EvidenceId>>,
    authorized_targets: Option<HashSet<EvidenceTargetRef>>,
}

impl<'a> SourceEvidenceHistory<'a> {
    /// Binds all project-wide meta-records and the closed target inventory.
    #[must_use]
    pub const fn new(
        sources: &'a [Source],
        evidence: &'a [Evidence],
        retractions: &'a [EvidenceRetraction],
        targets: &'a [EvidenceTargetHistoryEntry],
    ) -> Self {
        Self {
            sources,
            evidence,
            retractions,
            targets,
            authorized_sources: None,
            authorized_evidence: None,
            authorized_targets: None,
        }
    }

    fn with_authorization_filters(
        mut self,
        sources: &BTreeSet<SourceId>,
        evidence: &BTreeSet<EvidenceId>,
        targets: &HashSet<EvidenceTargetRef>,
    ) -> Self {
        self.authorized_sources = Some(sources.clone());
        self.authorized_evidence = Some(evidence.clone());
        self.authorized_targets = Some(targets.clone());
        self
    }
}

/// Query coordinates for the internal Source/Evidence history projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceEvidenceQuery {
    history_space_id: HistorySpaceId,
    recorded_as_of: RecordedAsOf,
}

impl SourceEvidenceQuery {
    /// Selects one HistorySpace ancestry view and Transaction-Time revision.
    #[must_use]
    pub const fn new(history_space_id: HistorySpaceId, recorded_as_of: RecordedAsOf) -> Self {
        Self {
            history_space_id,
            recorded_as_of,
        }
    }
}

/// Whether an Evidence edge had been explicitly retracted at the query revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceHistoryStatus {
    /// No visible EvidenceRetraction existed by the query revision.
    Active,
    /// The earliest visible EvidenceRetraction revision.
    RetractedAt(Revision),
}

/// One visible Evidence edge with its source and historical target metadata.
#[derive(Clone, Copy, Debug)]
pub struct EvidenceHistoryEntry<'a> {
    evidence: &'a Evidence,
    source: &'a Source,
    target: EvidenceTargetHistoryEntry,
    status: EvidenceHistoryStatus,
}

impl<'a> EvidenceHistoryEntry<'a> {
    /// Returns the immutable Evidence record, even when the edge is retracted.
    #[must_use]
    pub const fn evidence(self) -> &'a Evidence {
        self.evidence
    }

    /// Returns its project-wide Source record.
    #[must_use]
    pub const fn source(self) -> &'a Source {
        self.source
    }

    /// Returns historical target metadata without projecting or activating the target.
    #[must_use]
    pub const fn target_history(self) -> EvidenceTargetHistoryEntry {
        self.target
    }

    /// Returns whether the Evidence edge had an explicit visible retraction.
    #[must_use]
    pub const fn status(self) -> EvidenceHistoryStatus {
        self.status
    }
}

/// Canonical project-wide Sources and visible Evidence history at one query point.
#[derive(Clone, Debug)]
pub struct SourceEvidenceProjection<'a> {
    sources: Vec<&'a Source>,
    evidence: Vec<EvidenceHistoryEntry<'a>>,
}

impl<'a> SourceEvidenceProjection<'a> {
    /// Returns all Sources that existed by `RecordedAsOf`, ordered by SourceId.
    #[must_use]
    pub fn sources(&self) -> &[&'a Source] {
        &self.sources
    }

    /// Returns visible Evidence edges, ordered by EvidenceId.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceHistoryEntry<'a>] {
        &self.evidence
    }
}

/// Projects Source/Evidence metadata at a historical revision without World-Time validity.
///
/// Evidence remains an explanatory link. Its target may itself be historical or
/// retracted; this projection returns target metadata only and never reactivates
/// a target record in its domain projection. Authorization for public endpoints
/// belongs to later query/security layers.
pub fn full_scan_source_evidence<'a>(
    history: SourceEvidenceHistory<'a>,
    history_spaces: &HistorySpaceCatalog,
    query: SourceEvidenceQuery,
) -> Result<SourceEvidenceProjection<'a>, SourceEvidenceError> {
    project_source_evidence_at(
        history,
        history_spaces,
        query.history_space_id,
        query.recorded_as_of.revision(),
    )
}

/// Validates a complete post-transaction Source/Evidence target inventory at
/// an unpublished commit revision. The caller supplies the entire candidate
/// state, including existing records plus every staged addition/retraction.
pub fn validate_source_evidence_post_transaction<'a>(
    history: SourceEvidenceHistory<'a>,
    history_spaces: &HistorySpaceCatalog,
    history_space_id: HistorySpaceId,
    commit_revision: Revision,
) -> Result<SourceEvidenceProjection<'a>, SourceEvidenceError> {
    project_source_evidence_at(history, history_spaces, history_space_id, commit_revision)
}

fn project_source_evidence_at<'a>(
    history: SourceEvidenceHistory<'a>,
    history_spaces: &HistorySpaceCatalog,
    history_space_id: HistorySpaceId,
    revision: Revision,
) -> Result<SourceEvidenceProjection<'a>, SourceEvidenceError> {
    if history_spaces.definition(history_space_id).is_none() {
        return Err(SourceEvidenceError::UnknownQueryHistorySpace { history_space_id });
    }

    let mut sources_by_id = BTreeMap::new();
    for source in history.sources {
        if history
            .authorized_sources
            .as_ref()
            .is_some_and(|ids| !ids.contains(&source.id()))
        {
            continue;
        }
        if sources_by_id.insert(source.id(), source).is_some() {
            return Err(SourceEvidenceError::DuplicateSourceId {
                source_id: source.id(),
            });
        }
    }

    let mut targets_by_ref = HashMap::new();
    for target in history.targets {
        if history
            .authorized_targets
            .as_ref()
            .is_some_and(|targets| !targets.contains(&target.target()))
        {
            continue;
        }
        if targets_by_ref.insert(target.target(), *target).is_some() {
            return Err(SourceEvidenceError::DuplicateTarget {
                target: target.target(),
            });
        }
        if let Some(history_space_id) = target.history_space_id() {
            if history_spaces.definition(history_space_id).is_none() {
                return Err(SourceEvidenceError::UnknownTargetHistorySpace {
                    history_space_id,
                    target: target.target(),
                });
            }
        }
    }

    let mut evidence_by_id = BTreeMap::new();
    for evidence in history.evidence {
        if history
            .authorized_evidence
            .as_ref()
            .is_some_and(|ids| !ids.contains(&evidence.id()))
        {
            continue;
        }
        if evidence_by_id.insert(evidence.id(), evidence).is_some() {
            return Err(SourceEvidenceError::DuplicateEvidenceId {
                evidence_id: evidence.id(),
            });
        }
        let source =
            sources_by_id
                .get(&evidence.source_id())
                .ok_or(SourceEvidenceError::MissingSource {
                    evidence_id: evidence.id(),
                    source_id: evidence.source_id(),
                })?;
        if source.created_revision() > evidence.created_revision() {
            return Err(SourceEvidenceError::EvidenceBeforeSource {
                evidence_id: evidence.id(),
                source_id: evidence.source_id(),
            });
        }
        let target =
            targets_by_ref
                .get(&evidence.target())
                .ok_or(SourceEvidenceError::MissingTarget {
                    evidence_id: evidence.id(),
                    target: evidence.target(),
                })?;
        if target.created_revision() > evidence.created_revision() {
            return Err(SourceEvidenceError::EvidenceBeforeTarget {
                evidence_id: evidence.id(),
                target: evidence.target(),
            });
        }
    }

    let mut retraction_ids = BTreeSet::new();
    let mut retractions_by_evidence = BTreeMap::<EvidenceId, Vec<Revision>>::new();
    for retraction in history.retractions {
        if history
            .authorized_evidence
            .as_ref()
            .is_some_and(|ids| !ids.contains(&retraction.evidence_id()))
        {
            continue;
        }
        if !retraction_ids.insert(retraction.id()) {
            return Err(SourceEvidenceError::DuplicateEvidenceRetractionId {
                retraction_id: retraction.id(),
            });
        }
        let evidence = evidence_by_id.get(&retraction.evidence_id()).ok_or(
            SourceEvidenceError::MissingRetractionTarget {
                evidence_id: retraction.evidence_id(),
                retraction_id: retraction.id(),
            },
        )?;
        if retraction.created_revision() <= evidence.created_revision() {
            return Err(SourceEvidenceError::RetractionRevisionOrder {
                evidence_id: evidence.id(),
                retraction_id: retraction.id(),
            });
        }
        retractions_by_evidence
            .entry(evidence.id())
            .or_default()
            .push(retraction.created_revision());
    }

    let mut sources = history
        .sources
        .iter()
        .filter(|source| {
            source.created_revision() <= revision
                && history
                    .authorized_sources
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&source.id()))
        })
        .collect::<Vec<_>>();
    sources.sort_by_key(|source| source.id());
    let mut evidence_entries = Vec::new();
    let mut active_evidence_keys = HashSet::new();
    for evidence in history.evidence {
        if history
            .authorized_evidence
            .as_ref()
            .is_some_and(|ids| !ids.contains(&evidence.id()))
        {
            continue;
        }
        if evidence.created_revision() > revision {
            continue;
        }
        let source = sources_by_id.get(&evidence.source_id()).copied().ok_or(
            SourceEvidenceError::MissingSource {
                evidence_id: evidence.id(),
                source_id: evidence.source_id(),
            },
        )?;
        let target = targets_by_ref.get(&evidence.target()).copied().ok_or(
            SourceEvidenceError::MissingTarget {
                evidence_id: evidence.id(),
                target: evidence.target(),
            },
        )?;
        let cutoff = match target.history_space_id() {
            None => Some(revision),
            Some(origin) => {
                target_visibility_cutoff(history_space_id, origin, revision, history_spaces)?
            }
        };
        if cutoff.is_none_or(|cutoff| target.created_revision() > cutoff) {
            continue;
        }
        let retracted_at = retractions_by_evidence
            .get(&evidence.id())
            .into_iter()
            .flatten()
            .copied()
            .filter(|retracted_at| *retracted_at <= revision)
            .min();
        if retracted_at.is_none()
            && !active_evidence_keys.insert((
                evidence.source_id(),
                evidence.target(),
                evidence.relation(),
            ))
        {
            return Err(SourceEvidenceError::DuplicateActiveEvidence {
                source_id: evidence.source_id(),
                target: evidence.target(),
                relation: evidence.relation(),
            });
        }
        evidence_entries.push(EvidenceHistoryEntry {
            evidence,
            source,
            target,
            status: retracted_at.map_or(
                EvidenceHistoryStatus::Active,
                EvidenceHistoryStatus::RetractedAt,
            ),
        });
    }
    evidence_entries.sort_by_key(|entry| entry.evidence.id());

    Ok(SourceEvidenceProjection {
        sources,
        evidence: evidence_entries,
    })
}

/// Projects Source/Evidence records after endpoint and field authorization.
/// Hidden sources, targets, edges, and their lifecycle records are removed
/// before the structural reference scan can validate or emit them.
pub fn full_scan_authorized_source_evidence<'a>(
    history: SourceEvidenceHistory<'a>,
    history_spaces: &HistorySpaceCatalog,
    query: SourceEvidenceQuery,
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> Result<SourceEvidenceProjection<'a>, SourceEvidenceError> {
    if context.history_space() != query.history_space_id
        || context.recorded_as_of() != query.recorded_as_of
    {
        return Err(SourceEvidenceError::QueryContextMismatch);
    }
    let principal = context.security().principal_id();
    if policy.authorize(principal, Capability::QueryResolve, PolicyTarget::default())
        != AuthorizationDecision::Allow
    {
        return Ok(SourceEvidenceProjection {
            sources: Vec::new(),
            evidence: Vec::new(),
        });
    }
    let source_ids = history
        .sources
        .iter()
        .filter(|source| source_is_authorized(policy, principal, source))
        .map(|source| source.id())
        .collect::<BTreeSet<_>>();
    let target_refs = history
        .targets
        .iter()
        .filter(|target| target_is_authorized(policy, principal, **target))
        .map(|target| target.target())
        .collect::<HashSet<_>>();
    let evidence_ids = history
        .evidence
        .iter()
        .filter(|evidence| {
            source_ids.contains(&evidence.source_id())
                && target_refs.contains(&evidence.target())
                && history
                    .sources
                    .iter()
                    .find(|source| source.id() == evidence.source_id())
                    .zip(
                        history
                            .targets
                            .iter()
                            .find(|target| target.target() == evidence.target()),
                    )
                    .is_some_and(|(source, target)| {
                        evidence_endpoints_are_authorized(
                            policy,
                            principal,
                            source,
                            *target,
                            evidence,
                            query.history_space_id,
                        )
                    })
        })
        .map(|evidence| evidence.id())
        .collect::<BTreeSet<_>>();
    full_scan_source_evidence(
        history.with_authorization_filters(&source_ids, &evidence_ids, &target_refs),
        history_spaces,
        query,
    )
}

fn evidence_endpoints_are_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    source: &Source,
    target: EvidenceTargetHistoryEntry,
    evidence: &Evidence,
    query_history_space: HistorySpaceId,
) -> bool {
    source_is_authorized(policy, principal, source)
        && target_is_authorized(policy, principal, target)
        && evidence_is_authorized(policy, principal, evidence, query_history_space)
}

fn source_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    source: &Source,
) -> bool {
    let record = RecordRef::Source(source.id());
    let target = PolicyTarget::new(None, None, Some(record), None, None);
    policy.authorize(principal, Capability::SourceRead, target) == AuthorizationDecision::Allow
        && [
            FieldSelector::SourceKind,
            FieldSelector::SourceLocator,
            FieldSelector::SourceContentDigest,
            FieldSelector::SourceMetadata,
        ]
        .into_iter()
        .all(|field| {
            policy.authorize(
                principal,
                Capability::FieldRead,
                PolicyTarget::new(None, None, Some(record), Some(field), None),
            ) == AuthorizationDecision::Allow
        })
}

fn evidence_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    evidence: &Evidence,
    query_history_space: HistorySpaceId,
) -> bool {
    let record = RecordRef::Evidence(evidence.id());
    let relationship = match evidence.relation() {
        EvidenceRelation::Supports => EvidenceRelationship::Supports,
        EvidenceRelation::Contradicts => EvidenceRelationship::Contradicts,
        EvidenceRelation::Documents => EvidenceRelationship::Documents,
    };
    let target = PolicyTarget::new(
        Some(query_history_space),
        None,
        Some(record),
        None,
        Some(RelationshipSelector::Evidence(relationship)),
    );
    [Capability::EvidenceRead, Capability::RelationshipRead]
        .into_iter()
        .all(|capability| {
            policy.authorize(principal, capability, target) == AuthorizationDecision::Allow
        })
        && policy.authorize(
            principal,
            Capability::LifecycleRead,
            PolicyTarget::new(Some(query_history_space), None, Some(record), None, None),
        ) == AuthorizationDecision::Allow
}

fn target_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    target: EvidenceTargetHistoryEntry,
) -> bool {
    let Some(record) = evidence_target_record_ref(target.target()) else {
        return false;
    };
    let capability = match record {
        RecordRef::Assertion(_) => Capability::AssertionRead,
        RecordRef::Mask(_) => Capability::MaskRead,
        RecordRef::ReplacementBoundary(_) => Capability::ReplacementBoundaryRead,
        RecordRef::Event(_) => Capability::EventRead,
        RecordRef::EventMask(_) => Capability::EventMaskRead,
        RecordRef::EventRelation(_) => Capability::RelationshipRead,
        RecordRef::Provenance(_) => Capability::ProvenanceRead,
        RecordRef::ArchiveTransition(_) => Capability::LifecycleRead,
        _ => Capability::LifecycleRead,
    };
    let policy_target =
        PolicyTarget::new(target.history_space_id(), None, Some(record), None, None);
    (policy_target.history_space().is_none()
        || policy.authorize(principal, Capability::HistorySpaceRead, policy_target)
            == AuthorizationDecision::Allow)
        && policy.authorize(principal, capability, policy_target) == AuthorizationDecision::Allow
}

fn evidence_target_record_ref(target: EvidenceTargetRef) -> Option<RecordRef> {
    Some(match target {
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
    })
}

fn target_visibility_cutoff(
    query_history_space_id: HistorySpaceId,
    target_history_space_id: HistorySpaceId,
    query_revision: Revision,
    history_spaces: &HistorySpaceCatalog,
) -> Result<Option<Revision>, SourceEvidenceError> {
    let mut current_id = query_history_space_id;
    let mut cutoff = query_revision;
    loop {
        let current = history_spaces.definition(current_id).ok_or(
            SourceEvidenceError::UnknownQueryHistorySpace {
                history_space_id: current_id,
            },
        )?;
        if current_id == target_history_space_id {
            return Ok(Some(cutoff));
        }
        let Some(parent_id) = current.parent_history_space_id() else {
            return Ok(None);
        };
        cutoff = cutoff.min(current.base_revision());
        current_id = parent_id;
    }
}

/// Invalid Source/Evidence history or target-scope metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceEvidenceError {
    /// Query arguments disagree with the validated security-bearing context.
    QueryContextMismatch,
    /// The query names no registered HistorySpace.
    UnknownQueryHistorySpace { history_space_id: HistorySpaceId },
    /// A target inventory entry names no registered origin HistorySpace.
    UnknownTargetHistorySpace {
        history_space_id: HistorySpaceId,
        target: EvidenceTargetRef,
    },
    /// One SourceId occurs more than once.
    DuplicateSourceId { source_id: SourceId },
    /// One Evidence identity occurs more than once.
    DuplicateEvidenceId { evidence_id: EvidenceId },
    /// One typed Evidence target occurs more than once in the target inventory.
    DuplicateTarget { target: EvidenceTargetRef },
    /// Evidence references no Source in this project-wide history.
    MissingSource {
        evidence_id: EvidenceId,
        source_id: SourceId,
    },
    /// Evidence references no record in the internal target inventory.
    MissingTarget {
        evidence_id: EvidenceId,
        target: EvidenceTargetRef,
    },
    /// Two non-retracted Evidence records duplicate one source/target/relation tuple.
    DuplicateActiveEvidence {
        source_id: SourceId,
        target: EvidenceTargetRef,
        relation: EvidenceRelation,
    },
    /// Evidence predates the Source it references.
    EvidenceBeforeSource {
        evidence_id: EvidenceId,
        source_id: SourceId,
    },
    /// Evidence predates its target record.
    EvidenceBeforeTarget {
        evidence_id: EvidenceId,
        target: EvidenceTargetRef,
    },
    /// One EvidenceRetraction identity occurs more than once.
    DuplicateEvidenceRetractionId { retraction_id: EvidenceRetractionId },
    /// A retraction references no Evidence record.
    MissingRetractionTarget {
        evidence_id: EvidenceId,
        retraction_id: EvidenceRetractionId,
    },
    /// A retraction must be later than its Evidence edge.
    RetractionRevisionOrder {
        evidence_id: EvidenceId,
        retraction_id: EvidenceRetractionId,
    },
    /// HistorySpace ancestry validation failed.
    HistorySpace(HistorySpaceError),
}

impl fmt::Display for SourceEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Source/Evidence projection failed: {self:?}")
    }
}

impl std::error::Error for SourceEvidenceError {}

impl From<HistorySpaceError> for SourceEvidenceError {
    fn from(value: HistorySpaceError) -> Self {
        Self::HistorySpace(value)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::{
        EvidenceHistoryStatus, EvidenceTargetHistoryEntry, SourceEvidenceError,
        SourceEvidenceHistory, SourceEvidenceQuery, full_scan_source_evidence,
        validate_source_evidence_post_transaction,
    };
    use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition};
    use crate::ids::{
        AssertionId, AssertionRetractionId, DomainId, EvidenceId, EvidenceRetractionId,
        HistorySpaceId, IdValidationError, Revision, RevisionError, SourceId,
    };
    use crate::source_provenance::{
        Evidence, EvidenceRelation, EvidenceRetraction, EvidenceTargetRef, Source,
        SourceEvidenceProvenanceError, SourceMetadata,
    };
    use crate::temporal::RecordedAsOf;
    use crate::values::{Symbol, SymbolError};

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Projection(SourceEvidenceError),
        Record(SourceEvidenceProvenanceError),
        Catalog(crate::catalog::HistorySpaceError),
        Id(IdValidationError),
        Revision(RevisionError),
        Symbol(SymbolError),
        Policy(crate::security::SecurityPolicyError),
        Role(crate::security::RoleDefinitionError),
        Bundle(crate::security::PolicyBundleError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Projection(error) => write!(formatter, "{error}"),
                Self::Record(error) => write!(formatter, "{error}"),
                Self::Catalog(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
                Self::Policy(error) => write!(formatter, "{error}"),
                Self::Role(error) => write!(formatter, "{error}"),
                Self::Bundle(error) => write!(formatter, "{error}"),
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

    error_conversion!(SourceEvidenceError, Projection);
    error_conversion!(SourceEvidenceProvenanceError, Record);
    error_conversion!(crate::catalog::HistorySpaceError, Catalog);
    error_conversion!(IdValidationError, Id);
    error_conversion!(RevisionError, Revision);
    error_conversion!(SymbolError, Symbol);
    error_conversion!(crate::security::SecurityPolicyError, Policy);
    error_conversion!(crate::security::RoleDefinitionError, Role);
    error_conversion!(crate::security::PolicyBundleError, Bundle);

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

    fn catalog(
        root: HistorySpaceId,
        child: HistorySpaceId,
    ) -> Result<HistorySpaceCatalog, TestError> {
        Ok(value!(HistorySpaceCatalog::new(vec![
            HistorySpaceDefinition::new(root, None, Revision::GENESIS)?,
            HistorySpaceDefinition::new(child, Some(root), value!(revision(2)))?,
        ])))
    }

    fn source(id: u8, created: u64) -> Result<Source, TestError> {
        Ok(Source::new(
            value!(uuid::<SourceId>(id)),
            value!(Symbol::new("web")),
            None,
            None,
            SourceMetadata::default(),
            value!(revision(created)),
        ))
    }

    fn evidence(
        id: u8,
        source: SourceId,
        target: EvidenceTargetRef,
        created: u64,
    ) -> Result<Evidence, TestError> {
        Ok(Evidence::new(
            value!(uuid::<EvidenceId>(id)),
            source,
            target,
            EvidenceRelation::Documents,
            value!(revision(created)),
        ))
    }

    #[test]
    fn source_is_hidden_when_any_emitted_field_is_not_authorized() -> TestResult {
        use crate::security::{
            Capability, CapabilityGrant, CapabilityRule, FieldSelector, GrantEffect, PolicyBundle,
            PolicyScope, PolicySubject, Principal, RoleAssignment, RoleDefinition,
            SecurityPolicySnapshot,
        };

        let principal = value!(uuid::<crate::ids::PrincipalId>(80));
        let role_id = value!(uuid::<crate::ids::RoleId>(81));
        let assignment_id = value!(uuid::<crate::ids::RoleAssignmentId>(82));
        let source = source(83, 1)?;
        let role = RoleDefinition::new(
            role_id,
            "source_reader",
            PolicyBundle::from_grants([
                CapabilityGrant::new(Capability::HistorySpaceRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::SourceRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::AssertionRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::EvidenceRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::RelationshipRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::LifecycleRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::FieldRead, GrantEffect::Allow),
            ])?,
        )?;
        let assignment =
            RoleAssignment::new(assignment_id, principal, role_id, PolicyScope::project());
        let allow = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role.clone()],
            vec![assignment],
            Vec::new(),
        )?;
        assert!(super::source_is_authorized(&allow, principal, &source));
        let target_assertion_id = value!(uuid::<AssertionId>(85));
        let target_ref = EvidenceTargetRef::Assertion(target_assertion_id);
        let target_space = value!(uuid::<HistorySpaceId>(86));
        let target =
            EvidenceTargetHistoryEntry::new(target_ref, value!(revision(1)), Some(target_space));
        let evidence = evidence(87, source.id(), target_ref, 2)?;
        assert!(super::evidence_endpoints_are_authorized(
            &allow,
            principal,
            &source,
            target,
            &evidence,
            target_space,
        ));

        let deny_locator = CapabilityRule::new(
            value!(uuid::<crate::ids::PolicyRuleId>(84)),
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::FieldRead, GrantEffect::Deny),
            PolicyScope::new(
                None,
                None,
                Some(crate::RecordRef::Source(source.id())),
                Some(FieldSelector::SourceLocator),
                None,
            ),
        );
        let denied = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role.clone()],
            vec![assignment],
            vec![deny_locator],
        )?;
        assert!(!super::source_is_authorized(&denied, principal, &source));
        assert!(!super::evidence_endpoints_are_authorized(
            &denied,
            principal,
            &source,
            target,
            &evidence,
            target_space,
        ));

        let deny_target = CapabilityRule::new(
            value!(uuid::<crate::ids::PolicyRuleId>(88)),
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::AssertionRead, GrantEffect::Deny),
            PolicyScope::new(
                Some(target_space),
                None,
                Some(crate::RecordRef::Assertion(target_assertion_id)),
                None,
                None,
            ),
        );
        let target_hidden = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role.clone()],
            vec![assignment],
            vec![deny_target],
        )?;
        assert!(!super::evidence_endpoints_are_authorized(
            &target_hidden,
            principal,
            &source,
            target,
            &evidence,
            target_space,
        ));
        Ok(())
    }

    #[test]
    fn as_of_filters_project_wide_sources_evidence_and_retractions() -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(1));
        let child = value!(uuid::<HistorySpaceId>(2));
        let catalog = catalog(root, child)?;
        let source = source(3, 2)?;
        let target = EvidenceTargetRef::Assertion(value!(uuid::<AssertionId>(4)));
        let evidence = evidence(5, source.id(), target, 3)?;
        let retraction = value!(EvidenceRetraction::new(
            value!(uuid::<EvidenceRetractionId>(6)),
            &evidence,
            "evidence superseded",
            value!(revision(5)),
        ));
        let sources = [source];
        let evidence_records = [evidence];
        let retractions = [retraction];
        let targets = [EvidenceTargetHistoryEntry::new(
            target,
            value!(revision(2)),
            None,
        )];
        let history =
            SourceEvidenceHistory::new(&sources, &evidence_records, &retractions, &targets);
        let before_evidence = full_scan_source_evidence(
            history.clone(),
            &catalog,
            SourceEvidenceQuery::new(
                root,
                RecordedAsOf::from_published_revision(value!(revision(2))),
            ),
        )?;
        assert_eq!(before_evidence.sources().len(), 1);
        assert!(before_evidence.evidence().is_empty());

        let before = full_scan_source_evidence(
            history.clone(),
            &catalog,
            SourceEvidenceQuery::new(
                root,
                RecordedAsOf::from_published_revision(value!(revision(4))),
            ),
        )?;
        assert_eq!(before.sources().len(), 1);
        assert_eq!(before.evidence().len(), 1);
        assert_eq!(
            before.evidence().first().map(|entry| entry.status()),
            Some(EvidenceHistoryStatus::Active)
        );

        let after = full_scan_source_evidence(
            history,
            &catalog,
            SourceEvidenceQuery::new(
                root,
                RecordedAsOf::from_published_revision(value!(revision(5))),
            ),
        )?;
        assert_eq!(after.evidence().len(), 1);
        assert_eq!(
            after.evidence().first().map(|entry| entry.status()),
            Some(EvidenceHistoryStatus::RetractedAt(value!(revision(5))))
        );
        Ok(())
    }

    #[test]
    fn child_cutoff_hides_future_scoped_targets_but_keeps_visible_parent_evidence() -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(10));
        let child = value!(uuid::<HistorySpaceId>(11));
        let catalog = catalog(root, child)?;
        let source = source(12, 2)?;
        let visible_target = EvidenceTargetRef::Assertion(value!(uuid::<AssertionId>(13)));
        let hidden_target = EvidenceTargetRef::Assertion(value!(uuid::<AssertionId>(14)));
        let visible_evidence = evidence(15, source.id(), visible_target, 4)?;
        let hidden_evidence = evidence(16, source.id(), hidden_target, 4)?;
        let sources = [source];
        let evidence_records = [hidden_evidence, visible_evidence];
        let targets = [
            EvidenceTargetHistoryEntry::new(visible_target, value!(revision(2)), Some(root)),
            EvidenceTargetHistoryEntry::new(hidden_target, value!(revision(3)), Some(root)),
        ];
        let projection = full_scan_source_evidence(
            SourceEvidenceHistory::new(&sources, &evidence_records, &[], &targets),
            &catalog,
            SourceEvidenceQuery::new(
                child,
                RecordedAsOf::from_published_revision(value!(revision(5))),
            ),
        )?;
        assert_eq!(projection.evidence().len(), 1);
        assert_eq!(
            projection
                .evidence()
                .first()
                .map(|entry| entry.evidence().target()),
            Some(visible_target)
        );
        let root_projection = full_scan_source_evidence(
            SourceEvidenceHistory::new(&sources, &evidence_records, &[], &targets),
            &catalog,
            SourceEvidenceQuery::new(
                root,
                RecordedAsOf::from_published_revision(value!(revision(5))),
            ),
        )?;
        assert_eq!(root_projection.evidence().len(), 2);
        Ok(())
    }

    #[test]
    fn retracted_domain_target_remains_referenced_without_becoming_an_active_domain_record()
    -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(20));
        let child = value!(uuid::<HistorySpaceId>(21));
        let catalog = catalog(root, child)?;
        let source = source(22, 2)?;
        let historical_assertion = EvidenceTargetRef::Assertion(value!(uuid::<AssertionId>(23)));
        let historical_evidence = evidence(24, source.id(), historical_assertion, 4)?;
        let assertion_retraction =
            EvidenceTargetRef::AssertionRetraction(value!(uuid::<AssertionRetractionId>(25)));
        let retraction_evidence = evidence(26, source.id(), assertion_retraction, 6)?;
        let sources = [source];
        let evidence_records = [historical_evidence, retraction_evidence];
        let targets = [
            EvidenceTargetHistoryEntry::new(historical_assertion, value!(revision(3)), Some(root)),
            EvidenceTargetHistoryEntry::new(assertion_retraction, value!(revision(5)), Some(root)),
        ];
        let projection = full_scan_source_evidence(
            SourceEvidenceHistory::new(&sources, &evidence_records, &[], &targets),
            &catalog,
            SourceEvidenceQuery::new(
                root,
                RecordedAsOf::from_published_revision(value!(revision(6))),
            ),
        )?;
        assert_eq!(projection.evidence().len(), 2);
        assert_eq!(
            projection
                .evidence()
                .first()
                .map(|entry| entry.target_history().target()),
            Some(historical_assertion)
        );
        assert_eq!(
            projection
                .evidence()
                .get(1)
                .map(|entry| entry.target_history().target()),
            Some(assertion_retraction)
        );
        assert!(
            projection
                .evidence()
                .iter()
                .all(|entry| entry.status() == EvidenceHistoryStatus::Active)
        );
        Ok(())
    }

    #[test]
    fn malformed_sources_targets_and_retractions_fail_closed() -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(30));
        let child = value!(uuid::<HistorySpaceId>(31));
        let catalog = catalog(root, child)?;
        let too_early_source = source(32, 4)?;
        let source_id = too_early_source.id();
        let target = EvidenceTargetRef::Assertion(value!(uuid::<AssertionId>(33)));
        let too_early = evidence(34, source_id, target, 3)?;
        let sources = [too_early_source];
        let evidence_records = [too_early];
        let targets = [EvidenceTargetHistoryEntry::new(
            target,
            value!(revision(2)),
            None,
        )];
        assert_eq!(
            full_scan_source_evidence(
                SourceEvidenceHistory::new(&sources, &evidence_records, &[], &targets),
                &catalog,
                SourceEvidenceQuery::new(
                    root,
                    RecordedAsOf::from_published_revision(value!(revision(5))),
                ),
            )
            .err(),
            Some(SourceEvidenceError::EvidenceBeforeSource {
                evidence_id: too_early.id(),
                source_id,
            })
        );

        let valid_source = source(35, 2)?;
        let valid_target = EvidenceTargetRef::Assertion(value!(uuid::<AssertionId>(36)));
        let missing_target_evidence = evidence(37, valid_source.id(), valid_target, 3)?;
        let valid_sources = [valid_source];
        let missing_target_records = [missing_target_evidence];
        assert_eq!(
            full_scan_source_evidence(
                SourceEvidenceHistory::new(&valid_sources, &missing_target_records, &[], &[],),
                &catalog,
                SourceEvidenceQuery::new(
                    root,
                    RecordedAsOf::from_published_revision(value!(revision(5))),
                ),
            )
            .err(),
            Some(SourceEvidenceError::MissingTarget {
                evidence_id: missing_target_evidence.id(),
                target: valid_target,
            })
        );

        let invalid_retraction = EvidenceRetraction::from_wire_fields(
            value!(uuid::<EvidenceRetractionId>(38)),
            missing_target_evidence.id(),
            "invalid early retraction".to_owned(),
            value!(revision(3)),
        );
        let valid_target_history = [EvidenceTargetHistoryEntry::new(
            valid_target,
            value!(revision(2)),
            None,
        )];
        assert_eq!(
            full_scan_source_evidence(
                SourceEvidenceHistory::new(
                    &valid_sources,
                    &missing_target_records,
                    &[invalid_retraction],
                    &valid_target_history,
                ),
                &catalog,
                SourceEvidenceQuery::new(
                    root,
                    RecordedAsOf::from_published_revision(value!(revision(5))),
                ),
            )
            .err(),
            Some(SourceEvidenceError::RetractionRevisionOrder {
                evidence_id: missing_target_evidence.id(),
                retraction_id: value!(uuid::<EvidenceRetractionId>(38)),
            })
        );
        Ok(())
    }

    #[test]
    fn active_duplicate_evidence_tuples_conflict_but_retraction_frees_the_tuple() -> TestResult {
        let root = value!(uuid::<HistorySpaceId>(40));
        let child = value!(uuid::<HistorySpaceId>(41));
        let catalog = catalog(root, child)?;
        let source = source(42, 2)?;
        let target = EvidenceTargetRef::Assertion(value!(uuid::<AssertionId>(43)));
        let first = evidence(44, source.id(), target, 3)?;
        let second = evidence(45, source.id(), target, 3)?;
        let source_id = source.id();
        let sources = [source];
        let records = [first, second];
        let targets = [EvidenceTargetHistoryEntry::new(
            target,
            value!(revision(2)),
            None,
        )];
        let query = SourceEvidenceQuery::new(
            root,
            RecordedAsOf::from_published_revision(value!(revision(3))),
        );
        assert_eq!(
            full_scan_source_evidence(
                SourceEvidenceHistory::new(&sources, &records, &[], &targets),
                &catalog,
                query,
            )
            .err(),
            Some(SourceEvidenceError::DuplicateActiveEvidence {
                source_id,
                target,
                relation: EvidenceRelation::Documents,
            })
        );
        assert_eq!(
            validate_source_evidence_post_transaction(
                SourceEvidenceHistory::new(&sources, &records, &[], &targets),
                &catalog,
                root,
                value!(revision(3)),
            )
            .err(),
            Some(SourceEvidenceError::DuplicateActiveEvidence {
                source_id,
                target,
                relation: EvidenceRelation::Documents,
            })
        );

        let retraction = value!(EvidenceRetraction::new(
            value!(uuid::<EvidenceRetractionId>(46)),
            &first,
            "duplicate evidence retracted",
            value!(revision(4)),
        ));
        let retractions = [retraction];
        let projection = full_scan_source_evidence(
            SourceEvidenceHistory::new(&sources, &records, &retractions, &targets),
            &catalog,
            SourceEvidenceQuery::new(
                root,
                RecordedAsOf::from_published_revision(value!(revision(4))),
            ),
        )?;
        assert_eq!(projection.evidence().len(), 2);
        assert_eq!(
            projection.evidence().first().map(|entry| entry.status()),
            Some(EvidenceHistoryStatus::RetractedAt(value!(revision(4))))
        );
        assert_eq!(
            projection.evidence().get(1).map(|entry| entry.status()),
            Some(EvidenceHistoryStatus::Active)
        );
        Ok(())
    }
}
