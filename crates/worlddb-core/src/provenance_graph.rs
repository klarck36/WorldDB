//! Bounded dependency-graph validation for the three Provenance relations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ids::{ProvenanceId, ProvenanceRetractionId, Revision};
use crate::query_context::QueryContext;
use crate::record_refs::RecordRef;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, ProvenanceRelationship,
    RelationshipSelector, SecurityPolicySnapshot,
};
use crate::source_provenance::{
    ProvenanceEdge, ProvenanceEdgeHistory, ProvenanceEndpointRef, ProvenanceRelation,
    ProvenanceRetraction, SourceEvidenceProvenanceError, project_active_provenance_edges,
};
use crate::temporal::RecordedAsOf;

/// Maximum number of retained-record, node, and edge visits during validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphValidationBudget {
    max_work: usize,
}

impl GraphValidationBudget {
    /// Sets the hard per-validation work limit. A zero budget accepts only an
    /// empty graph whose validation requires no visits.
    #[must_use]
    pub const fn new(max_work: usize) -> Self {
        Self { max_work }
    }

    /// Returns the maximum number of node and edge visits.
    #[must_use]
    pub const fn max_work(self) -> usize {
        self.max_work
    }
}

/// One explicitly projected dependency edge with its originating record.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProvenanceDependencyEdge {
    from: ProvenanceEndpointRef,
    to: ProvenanceEndpointRef,
    provenance_id: ProvenanceId,
    relation: ProvenanceRelation,
}

impl ProvenanceDependencyEdge {
    /// Returns the dependent record.
    #[must_use]
    pub const fn from(self) -> ProvenanceEndpointRef {
        self.from
    }

    /// Returns the dependency/source record.
    #[must_use]
    pub const fn to(self) -> ProvenanceEndpointRef {
        self.to
    }

    /// Returns the source Provenance record.
    #[must_use]
    pub const fn provenance_id(self) -> ProvenanceId {
        self.provenance_id
    }

    /// Returns the Provenance relation that produced this dependency edge.
    #[must_use]
    pub const fn relation(self) -> ProvenanceRelation {
        self.relation
    }
}

/// A validated acyclic graph at one post-transaction boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenanceGraphProjection {
    active_edges: Vec<ProvenanceEdge>,
    dependencies: Vec<ProvenanceDependencyEdge>,
    validation_work: usize,
}

impl ProvenanceGraphProjection {
    /// Returns active explanatory records in stable Provenance-ID order.
    #[must_use]
    pub fn active_edges(&self) -> &[ProvenanceEdge] {
        &self.active_edges
    }

    /// Returns their canonical dependency-direction projection.
    #[must_use]
    pub fn dependencies(&self) -> &[ProvenanceDependencyEdge] {
        &self.dependencies
    }

    /// Returns node/edge visits charged to the validation budget.
    #[must_use]
    pub const fn validation_work(&self) -> usize {
        self.validation_work
    }
}

/// Validates candidate Provenance retractions and additions atomically.
///
/// The base graph is projected at `recorded_as_of`. Candidate retractions are
/// applied first, then all candidate additions are added together and checked
/// against one shared dependency graph. A budget exhaustion returns an error
/// without a partial graph result.
pub fn validate_provenance_graph_transaction(
    history: ProvenanceEdgeHistory<'_>,
    recorded_as_of: RecordedAsOf,
    commit_revision: Revision,
    candidate_retractions: &[ProvenanceRetraction],
    additions: &[ProvenanceEdge],
    budget: GraphValidationBudget,
) -> Result<ProvenanceGraphProjection, ProvenanceGraphError> {
    if commit_revision <= recorded_as_of.revision() {
        return Err(ProvenanceGraphError::CommitRevisionNotAfterSnapshot {
            snapshot_revision: recorded_as_of.revision(),
            commit_revision,
        });
    }

    let input_work = transaction_input_work(
        history.edges().len(),
        history.retractions().len(),
        candidate_retractions.len(),
        additions.len(),
    )
    .filter(|work| *work <= budget.max_work())
    .ok_or(ProvenanceGraphError::GraphValidationBudgetExceeded {
        max_work: budget.max_work(),
        work_used: budget.max_work(),
    })?;
    let graph_budget = GraphValidationBudget::new(budget.max_work() - input_work);

    let mut active = project_active_provenance_edges(history, recorded_as_of)?;
    let mut active_ids = active.iter().map(|edge| edge.id()).collect::<BTreeSet<_>>();
    let mut all_edge_ids = history
        .edges()
        .iter()
        .map(|edge| edge.id())
        .collect::<BTreeSet<_>>();
    let mut all_retraction_ids = history
        .retractions()
        .iter()
        .map(ProvenanceRetraction::id)
        .collect::<BTreeSet<_>>();

    let mut candidate_retractions = candidate_retractions.to_vec();
    candidate_retractions.sort_by_key(ProvenanceRetraction::id);
    for retraction in candidate_retractions {
        if retraction.created_revision() != commit_revision {
            return Err(ProvenanceGraphError::CandidateRetractionRevisionMismatch {
                retraction_id: retraction.id(),
                expected: commit_revision,
                actual: retraction.created_revision(),
            });
        }
        if !all_retraction_ids.insert(retraction.id()) {
            return Err(ProvenanceGraphError::DuplicateRetractionId {
                retraction_id: retraction.id(),
            });
        }
        if !active_ids.remove(&retraction.provenance_id()) {
            return Err(ProvenanceGraphError::RetractionTargetNotActive {
                provenance_id: retraction.provenance_id(),
            });
        }
        active.retain(|edge| edge.id() != retraction.provenance_id());
    }

    let mut additions = additions.to_vec();
    additions.sort_by_key(|edge| edge.id());
    for edge in additions {
        if edge.created_revision() != commit_revision {
            return Err(ProvenanceGraphError::AdditionRevisionMismatch {
                provenance_id: edge.id(),
                expected: commit_revision,
                actual: edge.created_revision(),
            });
        }
        if !all_edge_ids.insert(edge.id()) || !active_ids.insert(edge.id()) {
            return Err(ProvenanceGraphError::DuplicateProvenanceId {
                provenance_id: edge.id(),
            });
        }
        active.push(edge);
    }

    let mut projection = project_dependency_graph(active, graph_budget)?;
    projection.validation_work += input_work;
    Ok(projection)
}

/// Projects only Provenance edges whose relationship and both fully located
/// endpoints are authorized. The caller supplies endpoint coordinates resolved
/// from the same pinned query snapshot; hidden edges and their retractions are
/// removed before duplicate, lifecycle, or graph-shape validation.
pub fn project_authorized_provenance_edges(
    history: ProvenanceEdgeHistory<'_>,
    endpoint_targets: &BTreeMap<ProvenanceEndpointRef, PolicyTarget>,
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> Result<Vec<ProvenanceEdge>, ProvenanceGraphError> {
    let principal = context.security().principal_id();
    if policy.authorize(principal, Capability::QueryResolve, PolicyTarget::default())
        != AuthorizationDecision::Allow
    {
        return Ok(Vec::new());
    }
    let visible_edges = history
        .edges()
        .iter()
        .copied()
        .filter(|edge| {
            authorized_provenance_endpoint(policy, principal, endpoint_targets, edge.from())
                && authorized_provenance_endpoint(policy, principal, endpoint_targets, edge.to())
                && provenance_relationship_is_authorized(
                    policy,
                    principal,
                    edge,
                    context.history_space(),
                )
        })
        .collect::<Vec<_>>();
    let visible_ids = visible_edges
        .iter()
        .map(|edge| edge.id())
        .collect::<BTreeSet<_>>();
    let visible_retractions = history
        .retractions()
        .iter()
        .filter(|retraction| visible_ids.contains(&retraction.provenance_id()))
        .cloned()
        .collect::<Vec<_>>();
    Ok(crate::source_provenance::project_active_provenance_edges(
        ProvenanceEdgeHistory::new(&visible_edges, &visible_retractions),
        context.recorded_as_of(),
    )?)
}

fn authorized_provenance_endpoint(
    policy: &SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
    endpoint_targets: &BTreeMap<ProvenanceEndpointRef, PolicyTarget>,
    endpoint: ProvenanceEndpointRef,
) -> bool {
    let Some(target) = endpoint_targets.get(&endpoint).copied() else {
        return false;
    };
    let Some(record) = provenance_endpoint_record_ref(endpoint) else {
        return false;
    };
    if target.record() != Some(record) {
        return false;
    }
    let capability = match record {
        RecordRef::Assertion(_) => Capability::AssertionRead,
        RecordRef::Mask(_) => Capability::MaskRead,
        RecordRef::ReplacementBoundary(_) => Capability::ReplacementBoundaryRead,
        RecordRef::Event(_) => Capability::EventRead,
        RecordRef::EventMask(_) => Capability::EventMaskRead,
        RecordRef::EventRelation(_) => Capability::RelationshipRead,
        RecordRef::Source(_) => Capability::SourceRead,
        RecordRef::Evidence(_) => Capability::EvidenceRead,
        RecordRef::Provenance(_) => Capability::ProvenanceRead,
        RecordRef::ArchiveTransition(_) => Capability::LifecycleRead,
        _ => Capability::LifecycleRead,
    };
    if (target.history_space().is_some()
        && policy.authorize(principal, Capability::HistorySpaceRead, target)
            != AuthorizationDecision::Allow)
        || (target.layer().is_some()
            && policy.authorize(principal, Capability::LayerRead, target)
                != AuthorizationDecision::Allow)
        || policy.authorize(principal, capability, target) != AuthorizationDecision::Allow
    {
        return false;
    }
    if matches!(record, RecordRef::Source(_)) {
        return [
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
                PolicyTarget::new(
                    target.history_space(),
                    target.layer(),
                    Some(record),
                    Some(field),
                    None,
                ),
            ) == AuthorizationDecision::Allow
        });
    }
    true
}

fn provenance_relationship_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
    edge: &ProvenanceEdge,
    query_history_space: crate::ids::HistorySpaceId,
) -> bool {
    let relationship = match edge.relation() {
        ProvenanceRelation::Corrects => ProvenanceRelationship::Corrects,
        ProvenanceRelation::DerivedFrom => ProvenanceRelationship::DerivedFrom,
        ProvenanceRelation::ResultedFrom => ProvenanceRelationship::ResultedFrom,
    };
    let target = PolicyTarget::new(
        Some(query_history_space),
        None,
        Some(RecordRef::Provenance(edge.id())),
        None,
        Some(RelationshipSelector::Provenance(relationship)),
    );
    [
        Capability::ProvenanceRead,
        Capability::RelationshipRead,
        Capability::LifecycleRead,
    ]
    .into_iter()
    .all(|capability| {
        policy.authorize(principal, capability, target) == AuthorizationDecision::Allow
    })
}

fn provenance_endpoint_record_ref(endpoint: ProvenanceEndpointRef) -> Option<RecordRef> {
    Some(match endpoint {
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
    })
}

fn transaction_input_work(
    historical_edges: usize,
    historical_retractions: usize,
    candidate_retractions: usize,
    additions: usize,
) -> Option<usize> {
    historical_edges
        .checked_mul(4)?
        .checked_add(historical_retractions.checked_mul(3)?)?
        .checked_add(candidate_retractions.checked_mul(2)?)?
        .checked_add(additions.checked_mul(3)?)
}

fn project_dependency_graph(
    mut active_edges: Vec<ProvenanceEdge>,
    budget: GraphValidationBudget,
) -> Result<ProvenanceGraphProjection, ProvenanceGraphError> {
    active_edges.sort_by_key(|edge| edge.id());
    let mut edge_ids = BTreeSet::new();
    let mut logical_keys = BTreeSet::new();
    let mut nodes = BTreeSet::new();
    let mut dependencies = Vec::with_capacity(active_edges.len());
    for edge in &active_edges {
        if !edge_ids.insert(edge.id()) {
            return Err(ProvenanceGraphError::DuplicateProvenanceId {
                provenance_id: edge.id(),
            });
        }
        if !logical_keys.insert((edge.from(), edge.to(), edge.relation())) {
            return Err(ProvenanceGraphError::DuplicateActiveTuple {
                from: edge.from(),
                to: edge.to(),
                relation: edge.relation(),
            });
        }
        let (from, to) = match edge.relation() {
            ProvenanceRelation::Corrects => (edge.from(), edge.to()),
            ProvenanceRelation::DerivedFrom | ProvenanceRelation::ResultedFrom => {
                (edge.to(), edge.from())
            }
        };
        nodes.insert(from);
        nodes.insert(to);
        dependencies.push(ProvenanceDependencyEdge {
            from,
            to,
            provenance_id: edge.id(),
            relation: edge.relation(),
        });
    }
    dependencies.sort();

    let mut work = 0_usize;
    let mut indegree = BTreeMap::new();
    let mut outgoing =
        BTreeMap::<ProvenanceEndpointRef, Vec<(ProvenanceEndpointRef, ProvenanceId)>>::new();
    for node in &nodes {
        consume_work(&mut work, budget)?;
        indegree.insert(*node, 0_usize);
    }
    for edge in &dependencies {
        consume_work(&mut work, budget)?;
        outgoing
            .entry(edge.from())
            .or_default()
            .push((edge.to(), edge.provenance_id()));
        let degree = indegree
            .get_mut(&edge.to())
            .ok_or(ProvenanceGraphError::InternalMissingNode)?;
        *degree = degree
            .checked_add(1)
            .ok_or(ProvenanceGraphError::InternalDegreeOverflow)?;
    }
    for adjacent in outgoing.values_mut() {
        adjacent.sort();
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect::<BTreeSet<_>>();
    while let Some(node) = ready.pop_first() {
        consume_work(&mut work, budget)?;
        if let Some(adjacent) = outgoing.get(&node) {
            for (target, _) in adjacent {
                consume_work(&mut work, budget)?;
                let degree = indegree
                    .get_mut(target)
                    .ok_or(ProvenanceGraphError::InternalMissingNode)?;
                *degree = degree
                    .checked_sub(1)
                    .ok_or(ProvenanceGraphError::InternalDegreeUnderflow)?;
                if *degree == 0 {
                    ready.insert(*target);
                }
            }
        }
    }

    let blocked = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree > 0).then_some(*node))
        .collect::<Vec<_>>();
    if !blocked.is_empty() {
        let blocked_set = blocked.iter().copied().collect::<BTreeSet<_>>();
        let provenance_ids = dependencies
            .iter()
            .filter(|edge| blocked_set.contains(&edge.from()) && blocked_set.contains(&edge.to()))
            .map(|edge| edge.provenance_id())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        return Err(ProvenanceGraphError::Cycle {
            blocked_endpoints: blocked,
            provenance_ids,
        });
    }

    Ok(ProvenanceGraphProjection {
        active_edges,
        dependencies,
        validation_work: work,
    })
}

fn consume_work(
    work: &mut usize,
    budget: GraphValidationBudget,
) -> Result<(), ProvenanceGraphError> {
    if *work >= budget.max_work() {
        return Err(ProvenanceGraphError::GraphValidationBudgetExceeded {
            max_work: budget.max_work(),
            work_used: *work,
        });
    }
    *work += 1;
    Ok(())
}

/// An invalid, cyclic, or over-budget Provenance graph transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProvenanceGraphError {
    /// An existing Provenance history is malformed.
    History(SourceEvidenceProvenanceError),
    /// The selected commit must follow the base graph snapshot.
    CommitRevisionNotAfterSnapshot {
        /// Base graph revision.
        snapshot_revision: Revision,
        /// Candidate commit revision.
        commit_revision: Revision,
    },
    /// A candidate retraction does not belong to this transaction revision.
    CandidateRetractionRevisionMismatch {
        /// Candidate retraction identity.
        retraction_id: ProvenanceRetractionId,
        /// Selected commit revision.
        expected: Revision,
        /// Retraction's declared revision.
        actual: Revision,
    },
    /// A candidate addition does not belong to this transaction revision.
    AdditionRevisionMismatch {
        /// Candidate edge identity.
        provenance_id: ProvenanceId,
        /// Selected commit revision.
        expected: Revision,
        /// Edge's declared revision.
        actual: Revision,
    },
    /// A candidate retraction identity or target is invalid for the base state.
    DuplicateRetractionId {
        /// Repeated retraction identity.
        retraction_id: ProvenanceRetractionId,
    },
    /// Retraction target is not active at the selected base revision.
    RetractionTargetNotActive {
        /// Target Provenance identity.
        provenance_id: ProvenanceId,
    },
    /// A candidate edge reuses any historical Provenance identity.
    DuplicateProvenanceId {
        /// Repeated edge identity.
        provenance_id: ProvenanceId,
    },
    /// The post-transaction graph contains a duplicate active logical tuple.
    DuplicateActiveTuple {
        /// Shared source endpoint.
        from: ProvenanceEndpointRef,
        /// Shared destination endpoint.
        to: ProvenanceEndpointRef,
        /// Shared relation kind.
        relation: ProvenanceRelation,
    },
    /// The complete graph could not be decided within the configured budget.
    GraphValidationBudgetExceeded {
        /// Hard visit bound.
        max_work: usize,
        /// Work charged before stopping.
        work_used: usize,
    },
    /// The active dependency graph contains a directed cycle.
    Cycle {
        /// Stable sorted Kahn-residue endpoints.
        blocked_endpoints: Vec<ProvenanceEndpointRef>,
        /// Stable sorted Provenance edges in that blocked residue.
        provenance_ids: Vec<ProvenanceId>,
    },
    /// Internal invariant: an edge endpoint was not registered as a node.
    InternalMissingNode,
    /// Internal invariant: indegree arithmetic overflowed.
    InternalDegreeOverflow,
    /// Internal invariant: indegree arithmetic underflowed.
    InternalDegreeUnderflow,
}

impl From<SourceEvidenceProvenanceError> for ProvenanceGraphError {
    fn from(error: SourceEvidenceProvenanceError) -> Self {
        Self::History(error)
    }
}

impl fmt::Display for ProvenanceGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Provenance graph validation failed: {self:?}")
    }
}

impl std::error::Error for ProvenanceGraphError {}

#[cfg(test)]
mod tests {
    use super::{
        GraphValidationBudget, ProvenanceGraphError, authorized_provenance_endpoint,
        validate_provenance_graph_transaction,
    };
    use crate::ids::{
        AssertionId, DomainId, EvidenceId, HistorySpaceId, LayerId, ProvenanceId,
        ProvenanceRetractionId, Revision, SourceId,
    };
    use crate::source_provenance::{
        ProvenanceEdge, ProvenanceEdgeHistory, ProvenanceEndpointRef, ProvenanceRelation,
        ProvenanceRetraction,
    };
    use crate::temporal::RecordedAsOf;

    fn uuid<T: DomainId>(byte: u8) -> Result<T, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, crate::ids::RevisionError> {
        Revision::new(value)
    }

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    #[derive(Debug)]
    enum TestError {
        Id(crate::ids::IdValidationError),
        Revision(crate::ids::RevisionError),
        Provenance(crate::source_provenance::SourceEvidenceProvenanceError),
        Graph(ProvenanceGraphError),
        Policy(crate::security::SecurityPolicyError),
        Role(crate::security::RoleDefinitionError),
        Bundle(crate::security::PolicyBundleError),
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Provenance(error) => write!(formatter, "{error}"),
                Self::Graph(error) => write!(formatter, "{error}"),
                Self::Policy(error) => write!(formatter, "{error}"),
                Self::Role(error) => write!(formatter, "{error}"),
                Self::Bundle(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl std::error::Error for TestError {}

    impl From<crate::ids::IdValidationError> for TestError {
        fn from(error: crate::ids::IdValidationError) -> Self {
            Self::Id(error)
        }
    }

    impl From<crate::ids::RevisionError> for TestError {
        fn from(error: crate::ids::RevisionError) -> Self {
            Self::Revision(error)
        }
    }

    impl From<crate::source_provenance::SourceEvidenceProvenanceError> for TestError {
        fn from(error: crate::source_provenance::SourceEvidenceProvenanceError) -> Self {
            Self::Provenance(error)
        }
    }

    impl From<ProvenanceGraphError> for TestError {
        fn from(error: ProvenanceGraphError) -> Self {
            Self::Graph(error)
        }
    }

    impl From<crate::security::SecurityPolicyError> for TestError {
        fn from(error: crate::security::SecurityPolicyError) -> Self {
            Self::Policy(error)
        }
    }

    impl From<crate::security::RoleDefinitionError> for TestError {
        fn from(error: crate::security::RoleDefinitionError) -> Self {
            Self::Role(error)
        }
    }

    impl From<crate::security::PolicyBundleError> for TestError {
        fn from(error: crate::security::PolicyBundleError) -> Self {
            Self::Bundle(error)
        }
    }

    type TestResult = Result<(), TestError>;

    fn edge(
        id: u8,
        from: ProvenanceEndpointRef,
        to: ProvenanceEndpointRef,
        relation: ProvenanceRelation,
        created: u64,
    ) -> Result<ProvenanceEdge, TestError> {
        Ok(ProvenanceEdge::new(
            value!(uuid::<ProvenanceId>(id)),
            from,
            to,
            relation,
            value!(revision(created)),
        )?)
    }

    #[test]
    fn provenance_endpoint_requires_record_visibility_and_its_resource_scope() -> TestResult {
        use std::collections::BTreeMap;

        use crate::security::{
            Capability, CapabilityGrant, CapabilityRule, GrantEffect, PolicyBundle, PolicyScope,
            PolicySubject, Principal, RoleAssignment, RoleDefinition, SecurityPolicySnapshot,
        };

        let endpoint = ProvenanceEndpointRef::Assertion(uuid::<AssertionId>(91)?);
        let record = crate::RecordRef::Assertion(uuid::<AssertionId>(91)?);
        let principal = uuid::<crate::ids::PrincipalId>(92)?;
        let role_id = uuid::<crate::ids::RoleId>(93)?;
        let assignment_id = uuid::<crate::ids::RoleAssignmentId>(94)?;
        let history_space = uuid::<HistorySpaceId>(95)?;
        let layer = uuid::<LayerId>(96)?;
        let target =
            crate::PolicyTarget::new(Some(history_space), Some(layer), Some(record), None, None);
        let endpoints = BTreeMap::from([(endpoint, target)]);
        let role = RoleDefinition::new(
            role_id,
            "endpoint_reader",
            PolicyBundle::from_grants([
                CapabilityGrant::new(Capability::HistorySpaceRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::LayerRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::AssertionRead, GrantEffect::Allow),
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
        assert!(authorized_provenance_endpoint(
            &allow, principal, &endpoints, endpoint,
        ));

        let deny = CapabilityRule::new(
            uuid::<crate::ids::PolicyRuleId>(97)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::AssertionRead, GrantEffect::Deny),
            PolicyScope::new(Some(history_space), Some(layer), Some(record), None, None),
        );
        let denied = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role],
            vec![assignment],
            vec![deny],
        )?;
        assert!(!authorized_provenance_endpoint(
            &denied, principal, &endpoints, endpoint,
        ));
        Ok(())
    }

    fn endpoint(byte: u8) -> Result<ProvenanceEndpointRef, TestError> {
        Ok(ProvenanceEndpointRef::Assertion(value!(
            uuid::<AssertionId>(byte)
        )))
    }

    #[test]
    fn all_three_relations_project_to_the_contract_dependency_direction() -> TestResult {
        let a = endpoint(1)?;
        let b = endpoint(2)?;
        let c = endpoint(3)?;
        let edges = [
            edge(4, a, b, ProvenanceRelation::Corrects, 3)?,
            edge(5, b, c, ProvenanceRelation::DerivedFrom, 3)?,
            edge(6, a, c, ProvenanceRelation::ResultedFrom, 3)?,
        ];
        let projection = validate_provenance_graph_transaction(
            ProvenanceEdgeHistory::new(&[], &[]),
            RecordedAsOf::from_published_revision(value!(revision(2))),
            value!(revision(3)),
            &[],
            &edges,
            GraphValidationBudget::new(100),
        )?;
        let actual = projection
            .dependencies()
            .iter()
            .map(|edge| (edge.from(), edge.to(), edge.relation()))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            actual,
            [
                (a, b, ProvenanceRelation::Corrects),
                (c, b, ProvenanceRelation::DerivedFrom),
                (c, a, ProvenanceRelation::ResultedFrom),
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(projection.validation_work(), 21);
        Ok(())
    }

    #[test]
    fn mixed_relation_cycle_is_rejected_stably() -> TestResult {
        let a = endpoint(10)?;
        let b = endpoint(11)?;
        let c = endpoint(12)?;
        let edges = [
            edge(13, b, a, ProvenanceRelation::DerivedFrom, 3)?,
            edge(14, b, c, ProvenanceRelation::Corrects, 3)?,
            edge(15, a, c, ProvenanceRelation::ResultedFrom, 3)?,
        ];
        let result = validate_provenance_graph_transaction(
            ProvenanceEdgeHistory::new(&[], &[]),
            RecordedAsOf::from_published_revision(value!(revision(2))),
            value!(revision(3)),
            &[],
            &edges,
            GraphValidationBudget::new(100),
        );
        let reversed_edges = edges.iter().rev().cloned().collect::<Vec<_>>();
        let reversed_result = validate_provenance_graph_transaction(
            ProvenanceEdgeHistory::new(&[], &[]),
            RecordedAsOf::from_published_revision(value!(revision(2))),
            value!(revision(3)),
            &[],
            &reversed_edges,
            GraphValidationBudget::new(100),
        );
        assert_eq!(result.as_ref().err(), reversed_result.as_ref().err());
        assert!(matches!(&result, Err(ProvenanceGraphError::Cycle { .. })));
        if let Err(ProvenanceGraphError::Cycle {
            blocked_endpoints,
            provenance_ids,
        }) = result
        {
            assert_eq!(blocked_endpoints, vec![a, b, c]);
            assert_eq!(
                provenance_ids,
                vec![
                    value!(uuid::<ProvenanceId>(13)),
                    value!(uuid::<ProvenanceId>(14)),
                    value!(uuid::<ProvenanceId>(15))
                ]
            );
        }
        Ok(())
    }

    #[test]
    fn each_relation_rejects_its_own_cycle() -> TestResult {
        let a = endpoint(16)?;
        let b = endpoint(17)?;
        for (first_relation, second_relation) in [
            (ProvenanceRelation::Corrects, ProvenanceRelation::Corrects),
            (
                ProvenanceRelation::DerivedFrom,
                ProvenanceRelation::DerivedFrom,
            ),
            (
                ProvenanceRelation::ResultedFrom,
                ProvenanceRelation::ResultedFrom,
            ),
        ] {
            let (first_from, first_to) = if first_relation == ProvenanceRelation::Corrects {
                (a, b)
            } else {
                (b, a)
            };
            let (second_from, second_to) = if second_relation == ProvenanceRelation::Corrects {
                (b, a)
            } else {
                (a, b)
            };
            let edges = [
                edge(18, first_from, first_to, first_relation, 3)?,
                edge(19, second_from, second_to, second_relation, 3)?,
            ];
            let result = validate_provenance_graph_transaction(
                ProvenanceEdgeHistory::new(&[], &[]),
                RecordedAsOf::from_published_revision(value!(revision(2))),
                value!(revision(3)),
                &[],
                &edges,
                GraphValidationBudget::new(100),
            );
            assert!(matches!(result, Err(ProvenanceGraphError::Cycle { .. })));
        }
        Ok(())
    }

    #[test]
    fn retractions_apply_before_atomic_candidate_cycle_validation() -> TestResult {
        let a = endpoint(20)?;
        let b = endpoint(21)?;
        let original = edge(22, a, b, ProvenanceRelation::Corrects, 2)?;
        let retraction = ProvenanceRetraction::new(
            value!(uuid::<ProvenanceRetractionId>(23)),
            &original,
            "replace relation",
            value!(revision(4)),
        )?;
        let replacement = edge(24, b, a, ProvenanceRelation::Corrects, 4)?;
        let history_edges = [original];
        let history_retractions = [];
        let projection = validate_provenance_graph_transaction(
            ProvenanceEdgeHistory::new(&history_edges, &history_retractions),
            RecordedAsOf::from_published_revision(value!(revision(3))),
            value!(revision(4)),
            &[retraction],
            &[replacement],
            GraphValidationBudget::new(100),
        )?;
        assert_eq!(projection.active_edges().len(), 1);
        assert_eq!(
            projection.active_edges().first().map(|edge| edge.id()),
            Some(replacement.id())
        );
        Ok(())
    }

    #[test]
    fn exhausted_budget_fails_closed_without_partial_projection() -> TestResult {
        let relation = edge(
            30,
            ProvenanceEndpointRef::Source(value!(uuid::<SourceId>(31))),
            ProvenanceEndpointRef::Evidence(value!(uuid::<EvidenceId>(32))),
            ProvenanceRelation::DerivedFrom,
            3,
        )?;
        assert_eq!(
            validate_provenance_graph_transaction(
                ProvenanceEdgeHistory::new(&[], &[]),
                RecordedAsOf::from_published_revision(value!(revision(2))),
                value!(revision(3)),
                &[],
                &[relation],
                GraphValidationBudget::new(0),
            )
            .err(),
            Some(ProvenanceGraphError::GraphValidationBudgetExceeded {
                max_work: 0,
                work_used: 0,
            })
        );
        Ok(())
    }

    #[test]
    fn post_transaction_batch_rejects_duplicate_active_logical_tuples() -> TestResult {
        let from = ProvenanceEndpointRef::Source(value!(uuid::<SourceId>(40)));
        let to = ProvenanceEndpointRef::Evidence(value!(uuid::<EvidenceId>(41)));
        let existing = edge(42, from, to, ProvenanceRelation::DerivedFrom, 2)?;
        let addition = edge(43, from, to, ProvenanceRelation::DerivedFrom, 4)?;
        let history_edges = [existing];
        let history_retractions = [];
        assert_eq!(
            validate_provenance_graph_transaction(
                ProvenanceEdgeHistory::new(&history_edges, &history_retractions),
                RecordedAsOf::from_published_revision(value!(revision(3))),
                value!(revision(4)),
                &[],
                &[addition],
                GraphValidationBudget::new(100),
            )
            .err(),
            Some(ProvenanceGraphError::DuplicateActiveTuple {
                from,
                to,
                relation: ProvenanceRelation::DerivedFrom,
            })
        );
        Ok(())
    }
}
