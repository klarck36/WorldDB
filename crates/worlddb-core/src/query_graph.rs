//! Authorized, bounded graph traversal over index-free candidate nodes and edges.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::mem::size_of;

use crate::ids::{HistorySpaceId, LayerId};
use crate::query_context::{QueryContext, QueryContextBinding};
use crate::query_ports::OwnedQueryResult;
use crate::record_refs::RecordRef;
use crate::resource_profile::{MemoryReservation, ResourceClass};
use crate::security::{
    AuthorizationDecision, Capability, PolicyTarget, RelationshipSelector, SecurityPolicyHistory,
    SecurityPolicyHistoryError,
};

/// Closed relationship kinds available to reference graph traversal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GraphRelationshipKind {
    /// Directed event ordering edge.
    EventBefore,
    /// Symmetric event simultaneity relation.
    EventSameTime,
    /// Directed event causality edge.
    EventCauses,
    /// Provenance correction explanation.
    ProvenanceCorrects,
    /// Provenance derivation explanation.
    ProvenanceDerivedFrom,
    /// Provenance story consequence explanation.
    ProvenanceResultedFrom,
}

/// Explicit traversal orientation relative to stored relationship endpoints.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphDirection {
    /// Follow stored from-to endpoints.
    Outgoing,
    /// Follow reverse endpoints.
    Incoming,
    /// Follow either endpoint orientation.
    Both,
}

/// Behavior when a visible edge points to a node already emitted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphCyclePolicy {
    /// Omit an edge that would revisit an emitted node.
    StopAtRepeatedNode,
    /// Emit that visible edge once, then do not expand the repeated node again.
    EmitRepeatedEdgeAndStop,
}

/// Typed roots, relationship selection, direction, and finite traversal bounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphSpec {
    roots: Vec<RecordRef>,
    relationships: Vec<GraphRelationshipKind>,
    direction: GraphDirection,
    max_depth: u16,
    max_nodes: u64,
    max_edges: u64,
    cycle_policy: GraphCyclePolicy,
}

impl GraphSpec {
    /// Creates a canonical request. Roots and relationship kinds must be nonempty and unique.
    pub fn new(
        mut roots: Vec<RecordRef>,
        mut relationships: Vec<GraphRelationshipKind>,
        direction: GraphDirection,
        max_depth: u16,
        max_nodes: u64,
        max_edges: u64,
        cycle_policy: GraphCyclePolicy,
    ) -> Result<Self, GraphError> {
        if roots.is_empty() || relationships.is_empty() || max_nodes == 0 || max_edges == 0 {
            return Err(GraphError::InvalidSpec);
        }
        roots.sort_unstable();
        relationships.sort_unstable();
        if roots.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(left, right)| left == right)
        }) || relationships.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(left, right)| left == right)
        }) {
            return Err(GraphError::InvalidSpec);
        }
        Ok(Self {
            roots,
            relationships,
            direction,
            max_depth,
            max_nodes,
            max_edges,
            cycle_policy,
        })
    }
}

/// Candidate node with the exact resource coordinates needed for read checks.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GraphNode {
    record_ref: RecordRef,
    history_space: HistorySpaceId,
    layer: LayerId,
}

impl GraphNode {
    /// Binds typed node identity to its owner coordinates.
    #[must_use]
    pub const fn new(record_ref: RecordRef, history_space: HistorySpaceId, layer: LayerId) -> Self {
        Self {
            record_ref,
            history_space,
            layer,
        }
    }

    /// Typed visible node identity.
    #[must_use]
    pub const fn record_ref(self) -> RecordRef {
        self.record_ref
    }
}

/// Candidate relationship edge with both typed endpoints.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GraphEdge {
    record_ref: RecordRef,
    from: RecordRef,
    to: RecordRef,
    history_space: HistorySpaceId,
    layer: LayerId,
    relationship: GraphRelationshipKind,
}

impl GraphEdge {
    /// Creates an edge candidate. Endpoint and relationship authorization is checked at query time.
    #[must_use]
    pub const fn new(
        record_ref: RecordRef,
        from: RecordRef,
        to: RecordRef,
        history_space: HistorySpaceId,
        layer: LayerId,
        relationship: GraphRelationshipKind,
    ) -> Self {
        Self {
            record_ref,
            from,
            to,
            history_space,
            layer,
            relationship,
        }
    }
}

/// Index-free node/edge source. Authorization is applied before traversal or budget accounting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphCandidateSet {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    query_context_binding: Option<QueryContextBinding>,
}

impl GraphCandidateSet {
    /// Creates a candidate collection from retained node and relationship records.
    #[must_use]
    pub const fn new(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> Self {
        Self {
            nodes,
            edges,
            query_context_binding: None,
        }
    }

    /// Binds indexed or scanned graph candidates to every semantic axis of one query.
    #[must_use]
    pub fn new_for_context(
        context: &QueryContext,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    ) -> Self {
        Self {
            nodes,
            edges,
            query_context_binding: Some(context.binding()),
        }
    }

    pub(crate) fn is_bound_to(&self, context: &QueryContext) -> bool {
        self.query_context_binding
            .as_ref()
            .is_some_and(|binding| binding.matches(context))
    }
}

/// One caller-visible edge emitted by traversal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TraversedGraphEdge {
    record_ref: RecordRef,
    from: RecordRef,
    to: RecordRef,
    relationship: GraphRelationshipKind,
}

impl TraversedGraphEdge {
    /// Visible relationship-record identity.
    #[must_use]
    pub const fn record_ref(self) -> RecordRef {
        self.record_ref
    }

    /// Relationship source endpoint.
    #[must_use]
    pub const fn from(self) -> RecordRef {
        self.from
    }

    /// Relationship destination endpoint.
    #[must_use]
    pub const fn to(self) -> RecordRef {
        self.to
    }
}

/// Complete traversal result. Hidden node/edge counts and partial outputs are never exposed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphResult {
    nodes: Vec<RecordRef>,
    edges: Vec<TraversedGraphEdge>,
    max_depth_reached: u16,
}

impl GraphResult {
    /// Canonically ordered visible nodes.
    #[must_use]
    pub fn nodes(&self) -> &[RecordRef] {
        &self.nodes
    }

    /// Canonically ordered visible traversal edges.
    #[must_use]
    pub fn edges(&self) -> &[TraversedGraphEdge] {
        &self.edges
    }

    /// Greatest returned traversal depth; zero for root-only output.
    #[must_use]
    pub const fn max_depth_reached(&self) -> u16 {
        self.max_depth_reached
    }
}

/// Filters candidates for operation, node and relationship visibility before bounded BFS.
///
/// Only authorized node/edge visits consume the deterministic work budget. The canonical
/// query budget's work units serve as the elapsed-work bound; wall-clock deadlines are not
/// semantic query limits. Budget exhaustion and cancellation discard the whole result.
pub fn full_scan_authorized_graph_traversal(
    candidates: &GraphCandidateSet,
    spec: &GraphSpec,
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
) -> Result<OwnedQueryResult<GraphResult>, GraphError> {
    let security = policies.resolve(context)?;
    let policy = security.snapshot();
    let principal = context.security().principal_id();
    if policy.authorize(
        principal,
        Capability::QueryGraphTraverse,
        PolicyTarget::default(),
    ) != AuthorizationDecision::Allow
    {
        return Err(GraphError::Unauthorized);
    }

    let mut visible_nodes = BTreeMap::<RecordRef, GraphNode>::new();
    let mut scratch_reservations = Vec::<MemoryReservation>::new();
    let mut authorized_node_candidates = 0_u64;
    let mut work = 0_u64;
    for node in &candidates.nodes {
        check_cancelled(context)?;
        let Some(capability) = record_read_capability(node.record_ref) else {
            continue;
        };
        let target = PolicyTarget::new(
            Some(node.history_space),
            Some(node.layer),
            Some(node.record_ref),
            None,
            None,
        );
        if policy.authorize(principal, capability, target) == AuthorizationDecision::Allow {
            if visible_nodes.contains_key(&node.record_ref) {
                return Err(GraphError::DuplicateVisibleNode);
            }
            authorized_node_candidates = authorized_node_candidates
                .checked_add(1)
                .ok_or(GraphError::BudgetExceeded)?;
            if authorized_node_candidates > context.budget().max_candidates().get() {
                return Err(GraphError::BudgetExceeded);
            }
            work = work.checked_add(1).ok_or(GraphError::BudgetExceeded)?;
            if work > context.budget().max_work_units().get() {
                return Err(GraphError::BudgetExceeded);
            }
            let reservation =
                reserve_query_memory(context, size_of::<GraphNode>().saturating_add(160))?;
            scratch_reservations
                .try_reserve(1)
                .map_err(|_| GraphError::ResourceBudgetExceeded)?;
            visible_nodes.insert(node.record_ref, *node);
            scratch_reservations.push(reservation);
        }
    }

    let mut visible_edges = Vec::new();
    for edge in &candidates.edges {
        check_cancelled(context)?;
        if spec
            .relationships
            .binary_search(&edge.relationship)
            .is_err()
            || !visible_nodes.contains_key(&edge.from)
            || !visible_nodes.contains_key(&edge.to)
        {
            continue;
        }
        if edge_is_authorized(policy, principal, *edge) {
            work = work.checked_add(1).ok_or(GraphError::BudgetExceeded)?;
            if work > context.budget().max_work_units().get() {
                return Err(GraphError::BudgetExceeded);
            }
            let reservation =
                reserve_query_memory(context, size_of::<GraphEdge>().saturating_add(320))?;
            scratch_reservations
                .try_reserve(1)
                .map_err(|_| GraphError::ResourceBudgetExceeded)?;
            visible_edges
                .try_reserve(1)
                .map_err(|_| GraphError::ResourceBudgetExceeded)?;
            visible_edges.push(*edge);
            scratch_reservations.push(reservation);
        }
    }
    visible_edges.sort_unstable();
    if visible_edges.windows(2).any(|pair| {
        pair.first()
            .zip(pair.get(1))
            .is_some_and(|(left, right)| left.record_ref == right.record_ref)
    }) {
        return Err(GraphError::DuplicateVisibleEdge);
    }

    let mut adjacency = BTreeMap::<RecordRef, Vec<(RecordRef, usize, bool)>>::new();
    for (index, edge) in visible_edges.iter().enumerate() {
        let symmetric = edge.relationship == GraphRelationshipKind::EventSameTime;
        match (spec.direction, symmetric) {
            (_, true) => {
                adjacency
                    .entry(edge.from)
                    .or_default()
                    .push((edge.to, index, false));
                if edge.from != edge.to {
                    adjacency
                        .entry(edge.to)
                        .or_default()
                        .push((edge.from, index, true));
                }
            }
            (GraphDirection::Outgoing, false) => adjacency
                .entry(edge.from)
                .or_default()
                .push((edge.to, index, false)),
            (GraphDirection::Incoming, false) => adjacency
                .entry(edge.to)
                .or_default()
                .push((edge.from, index, true)),
            (GraphDirection::Both, false) => {
                adjacency
                    .entry(edge.from)
                    .or_default()
                    .push((edge.to, index, false));
                if edge.from != edge.to {
                    adjacency
                        .entry(edge.to)
                        .or_default()
                        .push((edge.from, index, true));
                }
            }
        }
    }
    for neighbors in adjacency.values_mut() {
        neighbors.sort_unstable();
    }

    let budget = context.budget();
    let mut emitted_nodes = BTreeSet::new();
    let mut emitted_edges = BTreeSet::new();
    let mut queue = VecDeque::new();
    for root in &spec.roots {
        if visible_nodes.contains_key(root) && emitted_nodes.insert(*root) {
            if emitted_nodes.len() as u64 > spec.max_nodes {
                return Err(GraphError::BudgetExceeded);
            }
            queue.push_back((*root, 0_u16));
        }
    }
    let mut max_depth_reached = 0_u16;
    let mut traversal_edges = Vec::new();
    while let Some((node, depth)) = queue.pop_front() {
        check_cancelled(context)?;
        if depth >= spec.max_depth {
            continue;
        }
        let Some(neighbors) = adjacency.get(&node) else {
            continue;
        };
        for (neighbor, edge_index, reverse) in neighbors {
            work = work.checked_add(1).ok_or(GraphError::BudgetExceeded)?;
            if work > budget.max_work_units().get() {
                return Err(GraphError::BudgetExceeded);
            }
            let edge = visible_edges
                .get(*edge_index)
                .ok_or(GraphError::InvalidCandidates)?;
            let repeated_node = emitted_nodes.contains(neighbor);
            if repeated_node && spec.cycle_policy == GraphCyclePolicy::StopAtRepeatedNode {
                continue;
            }
            if emitted_edges.insert(edge.record_ref) {
                if emitted_edges.len() as u64 > spec.max_edges {
                    return Err(GraphError::BudgetExceeded);
                }
                traversal_edges.push(TraversedGraphEdge {
                    record_ref: edge.record_ref,
                    from: if *reverse { edge.to } else { edge.from },
                    to: if *reverse { edge.from } else { edge.to },
                    relationship: edge.relationship,
                });
            }
            if !repeated_node {
                let next_depth = depth.checked_add(1).ok_or(GraphError::BudgetExceeded)?;
                if emitted_nodes.len() as u64 >= spec.max_nodes {
                    return Err(GraphError::BudgetExceeded);
                }
                emitted_nodes.insert(*neighbor);
                max_depth_reached = max_depth_reached.max(next_depth);
                queue.push_back((*neighbor, next_depth));
            }
        }
    }
    let result_bytes = emitted_nodes
        .len()
        .saturating_mul(size_of::<RecordRef>())
        .saturating_add(
            traversal_edges
                .capacity()
                .saturating_mul(size_of::<TraversedGraphEdge>()),
        )
        .saturating_add(64);
    let result_reservation = reserve_query_memory(context, result_bytes)?;
    let result = GraphResult {
        nodes: emitted_nodes.into_iter().collect(),
        edges: traversal_edges,
        max_depth_reached,
    };
    drop(scratch_reservations);
    OwnedQueryResult::bind_with_memory_reservations(
        context,
        policies,
        result,
        vec![result_reservation],
    )
    .map_err(GraphError::Security)
}

fn reserve_query_memory(
    context: &QueryContext,
    bytes: usize,
) -> Result<MemoryReservation, GraphError> {
    let bytes = u64::try_from(bytes).map_err(|_| GraphError::ResourceBudgetExceeded)?;
    context
        .resource_budget()
        .reserve(ResourceClass::Query, bytes)
        .map_err(|_| GraphError::ResourceBudgetExceeded)
}

fn edge_is_authorized(
    policy: &crate::security::SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
    edge: GraphEdge,
) -> bool {
    let relationship = match edge.relationship {
        GraphRelationshipKind::EventBefore => {
            RelationshipSelector::EventRelation(crate::security::PolicyEventRelationKind::Before)
        }
        GraphRelationshipKind::EventSameTime => {
            RelationshipSelector::EventRelation(crate::security::PolicyEventRelationKind::SameTime)
        }
        GraphRelationshipKind::EventCauses => {
            RelationshipSelector::EventRelation(crate::security::PolicyEventRelationKind::Causes)
        }
        GraphRelationshipKind::ProvenanceCorrects => {
            RelationshipSelector::Provenance(crate::security::ProvenanceRelationship::Corrects)
        }
        GraphRelationshipKind::ProvenanceDerivedFrom => {
            RelationshipSelector::Provenance(crate::security::ProvenanceRelationship::DerivedFrom)
        }
        GraphRelationshipKind::ProvenanceResultedFrom => {
            RelationshipSelector::Provenance(crate::security::ProvenanceRelationship::ResultedFrom)
        }
    };
    let relationship_target = PolicyTarget::new(
        Some(edge.history_space),
        Some(edge.layer),
        Some(edge.record_ref),
        None,
        Some(relationship),
    );
    if policy.authorize(principal, Capability::RelationshipRead, relationship_target)
        != AuthorizationDecision::Allow
    {
        return false;
    }
    if matches!(
        edge.relationship,
        GraphRelationshipKind::ProvenanceCorrects
            | GraphRelationshipKind::ProvenanceDerivedFrom
            | GraphRelationshipKind::ProvenanceResultedFrom
    ) {
        if !matches!(edge.record_ref, RecordRef::Provenance(_)) {
            return false;
        }
        policy.authorize(
            principal,
            Capability::ProvenanceRead,
            PolicyTarget::new(
                Some(edge.history_space),
                Some(edge.layer),
                Some(edge.record_ref),
                None,
                Some(relationship),
            ),
        ) == AuthorizationDecision::Allow
    } else {
        matches!(edge.record_ref, RecordRef::EventRelation(_))
    }
}

fn record_read_capability(record_ref: RecordRef) -> Option<Capability> {
    match record_ref {
        RecordRef::Assertion(_)
        | RecordRef::AssertionValidityClosure(_)
        | RecordRef::AssertionRetraction(_) => Some(Capability::AssertionRead),
        RecordRef::Mask(_) | RecordRef::MaskValidityClosure(_) | RecordRef::MaskRetraction(_) => {
            Some(Capability::MaskRead)
        }
        RecordRef::ReplacementBoundary(_)
        | RecordRef::ReplacementBoundaryValidityClosure(_)
        | RecordRef::ReplacementBoundaryRetraction(_) => Some(Capability::ReplacementBoundaryRead),
        RecordRef::Event(_) | RecordRef::EventSpanClosure(_) | RecordRef::EventRetraction(_) => {
            Some(Capability::EventRead)
        }
        RecordRef::EventMask(_) | RecordRef::EventMaskRetraction(_) => {
            Some(Capability::EventMaskRead)
        }
        RecordRef::Source(_) => Some(Capability::SourceRead),
        RecordRef::Evidence(_) | RecordRef::EvidenceRetraction(_) => Some(Capability::EvidenceRead),
        RecordRef::Provenance(_) | RecordRef::ProvenanceRetraction(_) => {
            Some(Capability::ProvenanceRead)
        }
        RecordRef::EntityRetirement(_) => Some(Capability::EntityRead),
        RecordRef::PerspectiveRetirement(_) => Some(Capability::PerspectiveRead),
        RecordRef::EventRelation(_)
        | RecordRef::EventRelationRetraction(_)
        | RecordRef::ArchiveTransition(_)
        | RecordRef::TransferLineage(_) => None,
    }
}

fn check_cancelled(context: &QueryContext) -> Result<(), GraphError> {
    if context.cancellation().is_cancelled() {
        Err(GraphError::Cancelled)
    } else {
        Ok(())
    }
}

/// Invalid graph request, policy, candidate graph, or terminal traversal condition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphError {
    /// Roots/relationships must be nonempty, unique, and node/edge bounds positive.
    InvalidSpec,
    /// Graph traversal permission is denied.
    Unauthorized,
    /// Candidate source repeats a caller-visible typed node identity.
    DuplicateVisibleNode,
    /// Candidate source repeats a visible edge identity.
    DuplicateVisibleEdge,
    /// Candidate rows were not bound to the complete query context.
    CandidateContextMismatch,
    /// An internally selected edge index is invalid.
    InvalidCandidates,
    /// Node, edge, work, or output budget was exhausted; no partial graph is returned.
    BudgetExceeded,
    /// The process query-memory admission budget was exhausted; no partial graph is returned.
    ResourceBudgetExceeded,
    /// The host cancelled traversal; no partial graph is returned.
    Cancelled,
    /// Query policy selection could not be resolved.
    Security(SecurityPolicyHistoryError),
}

impl fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSpec => "graph request is invalid",
            Self::Unauthorized => "graph traversal is not authorized",
            Self::DuplicateVisibleNode => "graph has duplicate visible node identities",
            Self::DuplicateVisibleEdge => "graph has duplicate visible edge identities",
            Self::CandidateContextMismatch => {
                "graph candidates do not match the pinned query context"
            }
            Self::InvalidCandidates => "graph candidate source is invalid",
            Self::BudgetExceeded => "graph traversal budget exceeded",
            Self::ResourceBudgetExceeded => "graph process memory budget exceeded",
            Self::Cancelled => "graph traversal was cancelled",
            Self::Security(_) => "graph security context is invalid",
        })
    }
}

impl std::error::Error for GraphError {}

impl From<SecurityPolicyHistoryError> for GraphError {
    fn from(value: SecurityPolicyHistoryError) -> Self {
        Self::Security(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GraphCandidateSet, GraphCyclePolicy, GraphDirection, GraphEdge, GraphError, GraphNode,
        GraphRelationshipKind, GraphSpec, full_scan_authorized_graph_traversal,
    };
    use crate::non_interference::{
        CursorObservation, PairedWorld, PublicFailure, PublicObservation,
    };
    use crate::query_context::{QueryContext, QueryContextInput, WorldTimeSelector};
    use crate::query_engine::{ProductiveQueryEngine, QueryEngineError, QueryExecutionPath};
    use crate::query_search::tests::{fixture_with_candidate_limit, fixture_with_denials, id};
    use crate::record_refs::RecordRef;
    use crate::temporal::{Timeline, WorldTime};

    #[test]
    fn productive_graph_entrypoint_requires_context_bound_candidates()
    -> Result<(), QueryEngineError> {
        let fixture = fixture_with_candidate_limit(None, 10, 10);
        let root = RecordRef::Assertion(id::<crate::ids::AssertionId>(16));
        let target = RecordRef::Assertion(id::<crate::ids::AssertionId>(17));
        let node_rows = vec![
            GraphNode::new(root, fixture.history_space, fixture.layer),
            GraphNode::new(target, fixture.history_space, fixture.layer),
        ];
        let edge_rows = vec![GraphEdge::new(
            RecordRef::Provenance(id::<crate::ids::ProvenanceId>(50)),
            root,
            target,
            fixture.history_space,
            fixture.layer,
            GraphRelationshipKind::ProvenanceDerivedFrom,
        )];
        let spec = GraphSpec::new(
            vec![root],
            vec![GraphRelationshipKind::ProvenanceDerivedFrom],
            GraphDirection::Outgoing,
            1,
            2,
            1,
            GraphCyclePolicy::StopAtRepeatedNode,
        )
        .map_err(QueryEngineError::Graph)?;
        let unbound = GraphCandidateSet::new(node_rows.clone(), edge_rows.clone());
        assert!(matches!(
            ProductiveQueryEngine::graph_traversal(
                &unbound,
                &spec,
                &fixture.context,
                &fixture.policies,
            ),
            Err(QueryEngineError::Graph(
                GraphError::CandidateContextMismatch
            ))
        ));

        let candidates = GraphCandidateSet::new_for_context(&fixture.context, node_rows, edge_rows);
        let mismatched_time_context = QueryContext::new(QueryContextInput {
            snapshot: fixture.context.snapshot(),
            snapshot_revision: fixture.context.snapshot_revision(),
            recorded_as_of: fixture.context.recorded_as_of(),
            history_space: fixture.context.history_space(),
            layers: fixture.context.layers().clone(),
            world_time: WorldTimeSelector::At(WorldTime::from_nanoseconds(
                Timeline::new(id::<crate::ids::TimelineId>(88)),
                123,
            )),
            perspective: fixture.context.perspective(),
            epistemic_mode: fixture.context.epistemic_mode(),
            schema_binding: fixture.context.schema_binding(),
            security: fixture.context.security(),
            budget: fixture.context.budget(),
            cancellation: fixture.context.cancellation().clone(),
        })
        .unwrap_or_else(|_| unreachable!("only the world-time selector changes"));
        assert!(matches!(
            ProductiveQueryEngine::graph_traversal(
                &candidates,
                &spec,
                &mismatched_time_context,
                &fixture.policies,
            ),
            Err(QueryEngineError::Graph(
                GraphError::CandidateContextMismatch
            ))
        ));
        let output = ProductiveQueryEngine::graph_traversal(
            &candidates,
            &spec,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(output.path(), QueryExecutionPath::FullScan);
        assert_eq!(output.query().value().nodes(), &[root, target]);
        assert_eq!(output.query().value().edges().len(), 1);
        assert!(
            output
                .query()
                .query_context_binding()
                .matches(&fixture.context)
        );
        Ok(())
    }

    #[test]
    fn hidden_nodes_and_edges_are_filtered_before_budgets_and_traversal() -> Result<(), GraphError>
    {
        let hidden = RecordRef::Assertion(id::<crate::ids::AssertionId>(18));
        let denied_edge = RecordRef::Provenance(id::<crate::ids::ProvenanceId>(52));
        let fixture = fixture_with_denials(Some(hidden), Some(denied_edge), 10, 10);
        let root = RecordRef::Assertion(id::<crate::ids::AssertionId>(16));
        let visible = RecordRef::Assertion(id::<crate::ids::AssertionId>(17));
        let denied_target = RecordRef::Assertion(id::<crate::ids::AssertionId>(19));
        let candidates = GraphCandidateSet::new(
            vec![
                GraphNode::new(root, fixture.history_space, fixture.layer),
                GraphNode::new(visible, fixture.history_space, fixture.layer),
                GraphNode::new(denied_target, fixture.history_space, fixture.layer),
                GraphNode::new(hidden, fixture.history_space, fixture.layer),
            ],
            vec![
                GraphEdge::new(
                    RecordRef::Provenance(id::<crate::ids::ProvenanceId>(50)),
                    root,
                    visible,
                    fixture.history_space,
                    fixture.layer,
                    GraphRelationshipKind::ProvenanceDerivedFrom,
                ),
                GraphEdge::new(
                    denied_edge,
                    root,
                    denied_target,
                    fixture.history_space,
                    fixture.layer,
                    GraphRelationshipKind::ProvenanceDerivedFrom,
                ),
                GraphEdge::new(
                    RecordRef::Provenance(id::<crate::ids::ProvenanceId>(53)),
                    root,
                    hidden,
                    fixture.history_space,
                    fixture.layer,
                    GraphRelationshipKind::ProvenanceDerivedFrom,
                ),
            ],
        );
        let spec = GraphSpec::new(
            vec![root],
            vec![GraphRelationshipKind::ProvenanceDerivedFrom],
            GraphDirection::Outgoing,
            1,
            2,
            1,
            GraphCyclePolicy::StopAtRepeatedNode,
        )?;
        let visible_only = GraphCandidateSet::new(
            vec![
                GraphNode::new(root, fixture.history_space, fixture.layer),
                GraphNode::new(visible, fixture.history_space, fixture.layer),
            ],
            vec![GraphEdge::new(
                RecordRef::Provenance(id::<crate::ids::ProvenanceId>(50)),
                root,
                visible,
                fixture.history_space,
                fixture.layer,
                GraphRelationshipKind::ProvenanceDerivedFrom,
            )],
        );
        let paired = PairedWorld::new((&candidates, &visible_only), false, true);
        paired
            .compare(|(all_candidates, visible_candidates), include_hidden| {
                let candidates = if *include_hidden {
                    *all_candidates
                } else {
                    *visible_candidates
                };
                match full_scan_authorized_graph_traversal(
                    candidates,
                    &spec,
                    &fixture.context,
                    &fixture.policies,
                ) {
                    Ok(result) => PublicObservation::success(
                        result.value().clone(),
                        vec!["nodes".to_owned(), "edges".to_owned()],
                        CursorObservation::Absent,
                    ),
                    Err(error) => PublicObservation::failure(
                        PublicFailure::new(error.to_string(), vec!["code".to_owned()]),
                        vec!["error".to_owned()],
                        CursorObservation::Absent,
                    ),
                }
            })
            .map_err(|_| GraphError::InvalidCandidates)?;
        let result = full_scan_authorized_graph_traversal(
            &candidates,
            &spec,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(result.value().nodes().len(), 2);
        assert_eq!(result.value().edges().len(), 1);
        assert!(!result.value().nodes().contains(&hidden));
        assert!(!result.value().nodes().contains(&denied_target));
        assert_eq!(
            result.value().edges().first().map(|edge| edge.to()),
            Some(visible)
        );
        Ok(())
    }

    #[test]
    fn zero_depth_returns_only_visible_roots_and_bounds_fail_without_partial_results()
    -> Result<(), GraphError> {
        let fixture = fixture_with_candidate_limit(None, 10, 10);
        let root = RecordRef::Assertion(id::<crate::ids::AssertionId>(60));
        let next = RecordRef::Assertion(id::<crate::ids::AssertionId>(61));
        let edge = GraphEdge::new(
            RecordRef::Provenance(id::<crate::ids::ProvenanceId>(62)),
            root,
            next,
            fixture.history_space,
            fixture.layer,
            GraphRelationshipKind::ProvenanceCorrects,
        );
        let candidates = GraphCandidateSet::new(
            vec![
                GraphNode::new(root, fixture.history_space, fixture.layer),
                GraphNode::new(next, fixture.history_space, fixture.layer),
            ],
            vec![edge],
        );
        let roots_only = GraphSpec::new(
            vec![root],
            vec![GraphRelationshipKind::ProvenanceCorrects],
            GraphDirection::Outgoing,
            0,
            1,
            1,
            GraphCyclePolicy::StopAtRepeatedNode,
        )?;
        let result = full_scan_authorized_graph_traversal(
            &candidates,
            &roots_only,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(result.value().nodes(), &[root]);
        assert!(result.value().edges().is_empty());

        let too_many_nodes = GraphSpec::new(
            vec![root],
            vec![GraphRelationshipKind::ProvenanceCorrects],
            GraphDirection::Outgoing,
            1,
            1,
            1,
            GraphCyclePolicy::StopAtRepeatedNode,
        )?;
        assert!(matches!(
            full_scan_authorized_graph_traversal(
                &candidates,
                &too_many_nodes,
                &fixture.context,
                &fixture.policies,
            ),
            Err(GraphError::BudgetExceeded)
        ));
        Ok(())
    }

    #[test]
    fn cycle_policy_controls_repeated_edge_emission_without_reexpanding_nodes()
    -> Result<(), GraphError> {
        let fixture = fixture_with_candidate_limit(None, 10, 10);
        let first = RecordRef::Assertion(id::<crate::ids::AssertionId>(72));
        let second = RecordRef::Assertion(id::<crate::ids::AssertionId>(73));
        let candidates = GraphCandidateSet::new(
            vec![
                GraphNode::new(first, fixture.history_space, fixture.layer),
                GraphNode::new(second, fixture.history_space, fixture.layer),
            ],
            vec![
                GraphEdge::new(
                    RecordRef::Provenance(id::<crate::ids::ProvenanceId>(74)),
                    first,
                    second,
                    fixture.history_space,
                    fixture.layer,
                    GraphRelationshipKind::ProvenanceCorrects,
                ),
                GraphEdge::new(
                    RecordRef::Provenance(id::<crate::ids::ProvenanceId>(75)),
                    second,
                    first,
                    fixture.history_space,
                    fixture.layer,
                    GraphRelationshipKind::ProvenanceCorrects,
                ),
            ],
        );
        let spec = |cycle_policy| {
            GraphSpec::new(
                vec![first],
                vec![GraphRelationshipKind::ProvenanceCorrects],
                GraphDirection::Outgoing,
                4,
                2,
                2,
                cycle_policy,
            )
        };
        let stop = full_scan_authorized_graph_traversal(
            &candidates,
            &spec(GraphCyclePolicy::StopAtRepeatedNode)?,
            &fixture.context,
            &fixture.policies,
        )?;
        let emit = full_scan_authorized_graph_traversal(
            &candidates,
            &spec(GraphCyclePolicy::EmitRepeatedEdgeAndStop)?,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(stop.value().nodes().len(), 2);
        assert_eq!(stop.value().edges().len(), 1);
        assert_eq!(emit.value().nodes().len(), 2);
        assert_eq!(emit.value().edges().len(), 2);
        Ok(())
    }
}
