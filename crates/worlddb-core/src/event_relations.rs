//! Canonical immutable EventRelation records and transaction-time retractions.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::events::Event;
use crate::ids::{EventId, EventRelationId, EventRelationRetractionId, Revision};
use crate::query_context::QueryContext;
use crate::record_refs::RecordRef;
use crate::security::{
    AuthorizationDecision, Capability, PolicyEventRelationKind, PolicyTarget, RelationshipSelector,
    SecurityPolicySnapshot,
};
use crate::temporal::RecordedAsOf;

/// A persisted EventRelation kind.
///
/// `After` is accepted only as [`EventRelationInputKind::After`] and is stored
/// as the inverse `Before` edge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EventRelationKind {
    /// The source Event precedes the destination Event.
    Before,
    /// Both Events belong to one explicit same-time equivalence relation.
    SameTime,
    /// The source Event causes the destination Event.
    Causes,
}

/// An API/import spelling that is normalized before constructing a record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventRelationInputKind {
    /// The source Event precedes the destination Event.
    Before,
    /// Alias for `Before(to_event, from_event)`; never persisted.
    After,
    /// Symmetric same-time input; endpoint order is canonicalized.
    SameTime,
    /// A directed causal relation, independent from temporal order.
    Causes,
}

/// The canonical logical key of a relation, excluding record identity/revision.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventRelationKey {
    kind: EventRelationKind,
    from_event: EventId,
    to_event: EventId,
}

impl EventRelationKey {
    /// Returns the persisted canonical relation kind.
    #[must_use]
    pub const fn kind(self) -> EventRelationKind {
        self.kind
    }

    /// Returns the canonical source endpoint.
    #[must_use]
    pub const fn from_event(self) -> EventId {
        self.from_event
    }

    /// Returns the canonical destination endpoint.
    #[must_use]
    pub const fn to_event(self) -> EventId {
        self.to_event
    }
}

/// One immutable canonical EventRelation record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EventRelation {
    id: EventRelationId,
    key: EventRelationKey,
    created_revision: Revision,
}

impl EventRelation {
    /// Creates a relation after normalizing aliases and rejecting self-edges.
    pub fn new(
        id: EventRelationId,
        from_event: EventId,
        to_event: EventId,
        input_kind: EventRelationInputKind,
        created_revision: Revision,
    ) -> Result<Self, EventRelationError> {
        if from_event == to_event {
            return Err(EventRelationError::SelfRelation {
                event_id: from_event,
            });
        }

        let key = match input_kind {
            EventRelationInputKind::Before => EventRelationKey {
                kind: EventRelationKind::Before,
                from_event,
                to_event,
            },
            EventRelationInputKind::After => EventRelationKey {
                kind: EventRelationKind::Before,
                from_event: to_event,
                to_event: from_event,
            },
            EventRelationInputKind::SameTime => {
                let (from_event, to_event) = if from_event < to_event {
                    (from_event, to_event)
                } else {
                    (to_event, from_event)
                };
                EventRelationKey {
                    kind: EventRelationKind::SameTime,
                    from_event,
                    to_event,
                }
            }
            EventRelationInputKind::Causes => EventRelationKey {
                kind: EventRelationKind::Causes,
                from_event,
                to_event,
            },
        };

        Ok(Self {
            id,
            key,
            created_revision,
        })
    }

    /// Returns the stable EventRelation identity.
    #[must_use]
    pub const fn id(self) -> EventRelationId {
        self.id
    }

    /// Returns the stored canonical relation kind; it can never be `After`.
    #[must_use]
    pub const fn kind(self) -> EventRelationKind {
        self.key.kind()
    }

    /// Returns the canonical source endpoint.
    #[must_use]
    pub const fn from_event(self) -> EventId {
        self.key.from_event()
    }

    /// Returns the canonical destination endpoint.
    #[must_use]
    pub const fn to_event(self) -> EventId {
        self.key.to_event()
    }

    /// Returns the logical key used for normalized equality and duplicate checks.
    #[must_use]
    pub const fn key(self) -> EventRelationKey {
        self.key
    }

    /// Returns the Transaction-Time revision that created this relation.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// A canonical batch of new relations with no normalized duplicate keys.
///
/// This validates duplicates within the proposed batch. Checking conflicts
/// against already active history requires the transaction's retraction and
/// graph snapshot and belongs to the later engine validation path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventRelationBatch(Vec<EventRelation>);

impl EventRelationBatch {
    /// Sorts by logical key and rejects duplicate canonical relations.
    pub fn new(mut relations: Vec<EventRelation>) -> Result<Self, EventRelationError> {
        relations.sort_by_key(|relation| relation.key());
        for pair in relations.windows(2) {
            if let [left, right] = pair {
                if left.key() == right.key() {
                    return Err(EventRelationError::DuplicateRelation { key: left.key() });
                }
            }
        }
        Ok(Self(relations))
    }

    /// Returns relations in canonical key order.
    #[must_use]
    pub fn as_slice(&self) -> &[EventRelation] {
        &self.0
    }
}

/// EventRelation records and their separate transaction-time retractions.
#[derive(Clone, Copy)]
pub struct EventRelationHistory<'a> {
    relations: &'a [EventRelation],
    retractions: &'a [EventRelationRetraction],
}

impl<'a> EventRelationHistory<'a> {
    /// Binds the complete relation/retraction history for an as-of projection.
    #[must_use]
    pub const fn new(
        relations: &'a [EventRelation],
        retractions: &'a [EventRelationRetraction],
    ) -> Self {
        Self {
            relations,
            retractions,
        }
    }

    /// Returns immutable relation records supplied to this history view.
    #[must_use]
    pub const fn relations(self) -> &'a [EventRelation] {
        self.relations
    }

    /// Returns immutable relation retractions supplied to this history view.
    #[must_use]
    pub const fn retractions(self) -> &'a [EventRelationRetraction] {
        self.retractions
    }
}

/// Returns canonical active relations at one published Transaction-Time point.
///
/// The projection validates relation identity and lifecycle links before
/// filtering. A logical key may be reused after its former relation has been
/// retracted, but two active records with the same normalized key are rejected.
pub fn project_active_event_relations(
    history: EventRelationHistory<'_>,
    recorded_as_of: RecordedAsOf,
) -> Result<Vec<EventRelation>, EventRelationError> {
    let mut relations_by_id = std::collections::BTreeMap::new();
    for relation in history.relations {
        if relations_by_id.insert(relation.id(), *relation).is_some() {
            return Err(EventRelationError::DuplicateRelationId {
                event_relation_id: relation.id(),
            });
        }
    }

    let mut retraction_ids = BTreeSet::new();
    let mut retracted_at_query = BTreeSet::new();
    for retraction in history.retractions {
        if !retraction_ids.insert(retraction.id()) {
            return Err(EventRelationError::DuplicateRetractionId {
                event_relation_retraction_id: retraction.id(),
            });
        }
        let relation = relations_by_id.get(&retraction.event_relation_id()).ok_or(
            EventRelationError::MissingRetractionTarget {
                event_relation_id: retraction.event_relation_id(),
            },
        )?;
        if retraction.created_revision() <= relation.created_revision() {
            return Err(EventRelationError::LifecycleRevisionNotAfterTarget {
                target_revision: relation.created_revision(),
                lifecycle_revision: retraction.created_revision(),
            });
        }
        if retraction.created_revision() <= recorded_as_of.revision() {
            retracted_at_query.insert(retraction.event_relation_id());
        }
    }

    let mut active = history
        .relations
        .iter()
        .filter(|relation| {
            relation.created_revision() <= recorded_as_of.revision()
                && !retracted_at_query.contains(&relation.id())
        })
        .copied()
        .collect::<Vec<_>>();
    active.sort_by_key(|relation| relation.key());
    for pair in active.windows(2) {
        if let [left, right] = pair {
            if left.key() == right.key() {
                return Err(EventRelationError::DuplicateRelation { key: left.key() });
            }
        }
    }
    Ok(active)
}

/// Projects active EventRelation edges only when the relationship and both
/// Event endpoints are visible. Filtering precedes relation/lifecycle
/// validation so hidden graph records cannot leak through errors or shape.
pub fn project_authorized_active_event_relations(
    history: EventRelationHistory<'_>,
    events: &[Event],
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> Result<Vec<EventRelation>, EventRelationError> {
    let principal = context.security().principal_id();
    if policy.authorize(principal, Capability::QueryResolve, PolicyTarget::default())
        != AuthorizationDecision::Allow
    {
        return Ok(Vec::new());
    }
    let visible_event_ids = events
        .iter()
        .filter(|event| crate::event_projection::event_is_authorized(policy, principal, event))
        .map(Event::id)
        .collect::<BTreeSet<_>>();
    let visible_relations = history
        .relations()
        .iter()
        .copied()
        .filter(|relation| {
            visible_event_ids.contains(&relation.from_event())
                && visible_event_ids.contains(&relation.to_event())
                && event_relation_is_authorized(
                    policy,
                    principal,
                    relation,
                    context.history_space(),
                )
        })
        .collect::<Vec<_>>();
    let visible_relation_ids = visible_relations
        .iter()
        .map(|relation| relation.id())
        .collect::<BTreeSet<_>>();
    let visible_retractions = history
        .retractions()
        .iter()
        .filter(|retraction| visible_relation_ids.contains(&retraction.event_relation_id()))
        .cloned()
        .collect::<Vec<_>>();
    project_active_event_relations(
        EventRelationHistory::new(&visible_relations, &visible_retractions),
        context.recorded_as_of(),
    )
}

fn event_relation_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
    relation: &EventRelation,
    query_history_space: crate::ids::HistorySpaceId,
) -> bool {
    let relation_kind = match relation.kind() {
        EventRelationKind::Before => PolicyEventRelationKind::Before,
        EventRelationKind::SameTime => PolicyEventRelationKind::SameTime,
        EventRelationKind::Causes => PolicyEventRelationKind::Causes,
    };
    let target = PolicyTarget::new(
        Some(query_history_space),
        None,
        Some(RecordRef::EventRelation(relation.id())),
        None,
        Some(RelationshipSelector::EventRelation(relation_kind)),
    );
    policy.authorize(principal, Capability::RelationshipRead, target)
        == AuthorizationDecision::Allow
}

/// Canonical result of checking one post-transaction EventRelation graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventGraphProjection {
    active_relations: Vec<EventRelation>,
    same_time_components: Vec<Vec<EventId>>,
}

impl EventGraphProjection {
    /// Returns explicit active EventRelation records in logical-key order.
    #[must_use]
    pub fn active_relations(&self) -> &[EventRelation] {
        &self.active_relations
    }

    /// Returns SameTime equivalence classes without materializing transitive records.
    #[must_use]
    pub fn same_time_components(&self) -> &[Vec<EventId>] {
        &self.same_time_components
    }
}

/// Validates candidate retractions/additions against an existing relation history.
///
/// Retractions and additions must belong to `commit_revision`. The base state
/// is read at `recorded_as_of`, then the whole post-transaction relation set is
/// checked atomically. EventTime is intentionally absent from this interface:
/// temporal proximity cannot create Before, SameTime, or Causes edges.
pub fn validate_event_graph_transaction(
    event_ids: &[EventId],
    history: EventRelationHistory<'_>,
    recorded_as_of: RecordedAsOf,
    commit_revision: Revision,
    candidate_retractions: &[EventRelationRetraction],
    additions: &EventRelationBatch,
) -> Result<EventGraphProjection, EventGraphError> {
    if commit_revision <= recorded_as_of.revision() {
        return Err(EventGraphError::CommitRevisionNotAfterSnapshot {
            snapshot_revision: recorded_as_of.revision(),
            commit_revision,
        });
    }
    let mut active = project_active_event_relations(history, recorded_as_of)?;
    let mut active_ids = active
        .iter()
        .map(|relation| relation.id())
        .collect::<BTreeSet<_>>();
    let mut relation_ids = history
        .relations
        .iter()
        .map(|relation| relation.id())
        .collect::<BTreeSet<_>>();
    let mut retraction_ids = history
        .retractions
        .iter()
        .map(|retraction| retraction.id())
        .collect::<BTreeSet<_>>();

    let mut candidate_retractions = candidate_retractions.to_vec();
    candidate_retractions
        .sort_by_key(|retraction| (retraction.event_relation_id(), retraction.id()));
    for retraction in &candidate_retractions {
        if retraction.created_revision() != commit_revision {
            return Err(EventGraphError::CandidateRetractionRevisionMismatch {
                retraction_id: retraction.id(),
                expected: commit_revision,
                actual: retraction.created_revision(),
            });
        }
        if !retraction_ids.insert(retraction.id()) {
            return Err(EventGraphError::DuplicateRetractionId {
                retraction_id: retraction.id(),
            });
        }
        if !active_ids.remove(&retraction.event_relation_id()) {
            return Err(EventGraphError::RetractionTargetNotActive {
                event_relation_id: retraction.event_relation_id(),
            });
        }
        active.retain(|relation| relation.id() != retraction.event_relation_id());
    }

    for relation in additions.as_slice() {
        if relation.created_revision() != commit_revision {
            return Err(EventGraphError::AdditionRevisionMismatch {
                event_relation_id: relation.id(),
                expected: commit_revision,
                actual: relation.created_revision(),
            });
        }
        if !relation_ids.insert(relation.id()) {
            return Err(EventGraphError::DuplicateRelationId {
                event_relation_id: relation.id(),
            });
        }
        if !active_ids.insert(relation.id()) {
            return Err(EventGraphError::DuplicateRelationId {
                event_relation_id: relation.id(),
            });
        }
        active.push(*relation);
    }

    project_event_graph(event_ids, active)
}

fn project_event_graph(
    event_ids: &[EventId],
    mut active_relations: Vec<EventRelation>,
) -> Result<EventGraphProjection, EventGraphError> {
    let events = event_ids.iter().copied().collect::<BTreeSet<_>>();
    if events.len() != event_ids.len() {
        return Err(EventGraphError::DuplicateEventId);
    }
    active_relations.sort_by_key(|relation| relation.key());
    let mut relation_ids = BTreeSet::new();
    let mut relation_keys = BTreeSet::new();
    for relation in &active_relations {
        if !relation_ids.insert(relation.id()) {
            return Err(EventGraphError::DuplicateRelationId {
                event_relation_id: relation.id(),
            });
        }
        if !relation_keys.insert(relation.key()) {
            return Err(EventGraphError::DuplicateActiveRelationKey {
                key: relation.key(),
            });
        }
        for endpoint in [relation.from_event(), relation.to_event()] {
            if !events.contains(&endpoint) {
                return Err(EventGraphError::MissingEventEndpoint {
                    event_id: endpoint,
                    event_relation_id: relation.id(),
                });
            }
        }
    }

    let mut same_time = BTreeMap::<EventId, BTreeSet<EventId>>::new();
    for event in &events {
        same_time.entry(*event).or_default();
    }
    for relation in active_relations
        .iter()
        .filter(|relation| relation.kind() == EventRelationKind::SameTime)
    {
        same_time
            .entry(relation.from_event())
            .or_default()
            .insert(relation.to_event());
        same_time
            .entry(relation.to_event())
            .or_default()
            .insert(relation.from_event());
    }
    let mut visited = BTreeSet::new();
    let mut components = Vec::new();
    let mut component_by_event = BTreeMap::new();
    for start in events.iter().copied() {
        if !visited.insert(start) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        let mut component = Vec::new();
        while let Some(current) = queue.pop_front() {
            component.push(current);
            if let Some(neighbors) = same_time.get(&current) {
                for neighbor in neighbors {
                    if visited.insert(*neighbor) {
                        queue.push_back(*neighbor);
                    }
                }
            }
        }
        component.sort();
        let representative = component
            .first()
            .copied()
            .ok_or(EventGraphError::InternalEmptyComponent)?;
        for event in &component {
            component_by_event.insert(*event, representative);
        }
        components.push(component);
    }
    components.sort_by_key(|component| component.first().copied());

    let mut before_edges = Vec::new();
    let mut cause_edges = Vec::new();
    for relation in &active_relations {
        match relation.kind() {
            EventRelationKind::SameTime => {}
            EventRelationKind::Before => {
                let from = component_by_event
                    .get(&relation.from_event())
                    .copied()
                    .ok_or(EventGraphError::MissingEventEndpoint {
                        event_id: relation.from_event(),
                        event_relation_id: relation.id(),
                    })?;
                let to = component_by_event
                    .get(&relation.to_event())
                    .copied()
                    .ok_or(EventGraphError::MissingEventEndpoint {
                        event_id: relation.to_event(),
                        event_relation_id: relation.id(),
                    })?;
                if from == to {
                    return Err(EventGraphError::BeforeWithinSameTimeComponent {
                        event_relation_id: relation.id(),
                        component_representative: from,
                    });
                }
                before_edges.push((from, to, relation.id()));
            }
            EventRelationKind::Causes => {
                cause_edges.push((relation.from_event(), relation.to_event(), relation.id()));
            }
        }
    }
    if let Some((blocked, relations)) =
        graph_cycle_residue(&components_by_representative(&components), &before_edges)
    {
        return Err(EventGraphError::BeforeCycle {
            blocked_components: blocked,
            event_relation_ids: relations,
        });
    }
    if let Some((blocked, relations)) = graph_cycle_residue(&events, &cause_edges) {
        return Err(EventGraphError::CausesCycle {
            blocked_events: blocked,
            event_relation_ids: relations,
        });
    }
    Ok(EventGraphProjection {
        active_relations,
        same_time_components: components,
    })
}

fn components_by_representative(components: &[Vec<EventId>]) -> BTreeSet<EventId> {
    components
        .iter()
        .filter_map(|component| component.first().copied())
        .collect()
}

fn graph_cycle_residue(
    nodes: &BTreeSet<EventId>,
    edges: &[(EventId, EventId, EventRelationId)],
) -> Option<(Vec<EventId>, Vec<EventRelationId>)> {
    let mut indegree = nodes
        .iter()
        .map(|node| (*node, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<EventId, Vec<(EventId, EventRelationId)>>::new();
    for (from, to, relation_id) in edges {
        outgoing.entry(*from).or_default().push((*to, *relation_id));
        *indegree.entry(*to).or_default() += 1;
    }
    for adjacent in outgoing.values_mut() {
        adjacent.sort();
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect::<BTreeSet<_>>();
    while let Some(node) = ready.pop_first() {
        if let Some(adjacent) = outgoing.get(&node) {
            for (target, _) in adjacent {
                if let Some(degree) = indegree.get_mut(target) {
                    *degree -= 1;
                    if *degree == 0 {
                        ready.insert(*target);
                    }
                }
            }
        }
    }
    let blocked = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree > 0).then_some(*node))
        .collect::<Vec<_>>();
    if blocked.is_empty() {
        return None;
    }
    let blocked_set = blocked.iter().copied().collect::<BTreeSet<_>>();
    let mut relation_ids = edges
        .iter()
        .filter_map(|(from, to, relation_id)| {
            (blocked_set.contains(from) && blocked_set.contains(to)).then_some(*relation_id)
        })
        .collect::<Vec<_>>();
    relation_ids.sort();
    Some((blocked, relation_ids))
}

/// One immutable Transaction-Time retraction of an EventRelation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EventRelationRetraction {
    id: EventRelationRetractionId,
    event_relation_id: EventRelationId,
    reason: String,
    created_revision: Revision,
}

impl EventRelationRetraction {
    /// Creates a retraction strictly after its EventRelation.
    pub fn new(
        id: EventRelationRetractionId,
        event_relation: &EventRelation,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, EventRelationError> {
        if created_revision <= event_relation.created_revision() {
            return Err(EventRelationError::LifecycleRevisionNotAfterTarget {
                target_revision: event_relation.created_revision(),
                lifecycle_revision: created_revision,
            });
        }
        Ok(Self {
            id,
            event_relation_id: event_relation.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target exists and predates this retraction.
    pub(crate) fn from_wire_fields(
        id: EventRelationRetractionId,
        event_relation_id: EventRelationId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            event_relation_id,
            reason,
            created_revision,
        }
    }

    /// Returns the concrete retraction identity.
    #[must_use]
    pub const fn id(&self) -> EventRelationRetractionId {
        self.id
    }

    /// Returns the target EventRelation identity.
    #[must_use]
    pub const fn event_relation_id(&self) -> EventRelationId {
        self.event_relation_id
    }

    /// Returns the required reason for retracting this relation.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time revision at which retraction applies.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// A rejected EventRelation shape or lifecycle value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventRelationError {
    /// An EventRelation cannot refer from an Event to itself.
    SelfRelation {
        /// The repeated Event identity.
        event_id: EventId,
    },
    /// A batch contains the same normalized logical edge more than once.
    DuplicateRelation {
        /// The duplicated canonical logical key.
        key: EventRelationKey,
    },
    /// A stored history repeats one concrete EventRelation identity.
    DuplicateRelationId {
        /// Repeated stable EventRelation identity.
        event_relation_id: EventRelationId,
    },
    /// A stored history repeats one concrete EventRelationRetraction identity.
    DuplicateRetractionId {
        /// Repeated stable retraction identity.
        event_relation_retraction_id: EventRelationRetractionId,
    },
    /// A stored relation retraction references no relation in this history.
    MissingRetractionTarget {
        /// The missing immutable EventRelation identity.
        event_relation_id: EventRelationId,
    },
    /// A lifecycle record must be later than its target on Transaction Time.
    LifecycleRevisionNotAfterTarget {
        /// The target EventRelation revision.
        target_revision: Revision,
        /// The proposed retraction revision.
        lifecycle_revision: Revision,
    },
}

impl fmt::Display for EventRelationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfRelation { event_id } => {
                write!(
                    formatter,
                    "EventRelation cannot target Event {event_id} from itself"
                )
            }
            Self::DuplicateRelation { key } => write!(
                formatter,
                "duplicate EventRelation {:?}({},{})",
                key.kind(),
                key.from_event(),
                key.to_event()
            ),
            Self::DuplicateRelationId { event_relation_id } => {
                write!(
                    formatter,
                    "duplicate EventRelation identity {event_relation_id}"
                )
            }
            Self::DuplicateRetractionId {
                event_relation_retraction_id,
            } => write!(
                formatter,
                "duplicate EventRelationRetraction identity {event_relation_retraction_id}"
            ),
            Self::MissingRetractionTarget { event_relation_id } => write!(
                formatter,
                "EventRelationRetraction references missing EventRelation {event_relation_id}"
            ),
            Self::LifecycleRevisionNotAfterTarget {
                target_revision,
                lifecycle_revision,
            } => write!(
                formatter,
                "EventRelationRetraction revision {lifecycle_revision} must be after target revision {target_revision}"
            ),
        }
    }
}

impl std::error::Error for EventRelationError {}

/// A malformed graph transaction or a deterministic Event graph conflict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventGraphError {
    /// Existing relation-history validation failed.
    Relation(EventRelationError),
    /// The candidate transaction must be later than its historical snapshot.
    CommitRevisionNotAfterSnapshot {
        /// Revision selected for the pre-transaction graph.
        snapshot_revision: Revision,
        /// Requested revision of the candidate transaction.
        commit_revision: Revision,
    },
    /// A proposed relation retraction must have the transaction's revision.
    CandidateRetractionRevisionMismatch {
        /// Identity of the candidate retraction.
        retraction_id: EventRelationRetractionId,
        /// Required transaction revision.
        expected: Revision,
        /// Revision stored by this retraction.
        actual: Revision,
    },
    /// A candidate retraction identity is already present in the history/batch.
    DuplicateRetractionId {
        /// Repeated retraction identity.
        retraction_id: EventRelationRetractionId,
    },
    /// Candidate retractions may target only relations active at the base snapshot.
    RetractionTargetNotActive {
        /// Relation that is missing, already inactive, or retracted twice.
        event_relation_id: EventRelationId,
    },
    /// A proposed relation must have the transaction's revision.
    AdditionRevisionMismatch {
        /// Identity of the candidate relation.
        event_relation_id: EventRelationId,
        /// Required transaction revision.
        expected: Revision,
        /// Revision stored by this relation.
        actual: Revision,
    },
    /// The event inventory contains one Event identity more than once.
    DuplicateEventId,
    /// A relation endpoint is absent from the supplied Event inventory.
    MissingEventEndpoint {
        /// Missing Event identity.
        event_id: EventId,
        /// Relation containing the missing endpoint.
        event_relation_id: EventRelationId,
    },
    /// A concrete EventRelation identity is repeated in the post-transaction set.
    DuplicateRelationId {
        /// Repeated immutable relation identity.
        event_relation_id: EventRelationId,
    },
    /// More than one active relation has the same canonical logical key.
    DuplicateActiveRelationKey {
        /// Repeated logical key.
        key: EventRelationKey,
    },
    /// A Before edge orders two Events in one SameTime equivalence component.
    BeforeWithinSameTimeComponent {
        /// Conflicting Before relation.
        event_relation_id: EventRelationId,
        /// Smallest EventId representing the collapsed component.
        component_representative: EventId,
    },
    /// The collapsed Before graph contains a cycle.
    BeforeCycle {
        /// Stable sorted set of components blocked by the cycle.
        blocked_components: Vec<EventId>,
        /// Stable sorted relation identities inside the blocked subgraph.
        event_relation_ids: Vec<EventRelationId>,
    },
    /// The independent Causes graph contains a cycle.
    CausesCycle {
        /// Stable sorted set of Events blocked by the cycle.
        blocked_events: Vec<EventId>,
        /// Stable sorted relation identities inside the blocked subgraph.
        event_relation_ids: Vec<EventRelationId>,
    },
    /// Internal invariant: traversal must always produce a non-empty component.
    InternalEmptyComponent,
}

impl From<EventRelationError> for EventGraphError {
    fn from(value: EventRelationError) -> Self {
        Self::Relation(value)
    }
}

impl fmt::Display for EventGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Event graph validation failed: {self:?}")
    }
}

impl std::error::Error for EventGraphError {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::{
        EventGraphError, EventRelation, EventRelationBatch, EventRelationError,
        EventRelationHistory, EventRelationInputKind, EventRelationKind, EventRelationRetraction,
        event_relation_is_authorized, project_active_event_relations,
        validate_event_graph_transaction,
    };
    use crate::ids::{
        DomainId, EventId, EventRelationId, EventRelationRetractionId, HistorySpaceId,
        IdValidationError, Revision, RevisionError,
    };
    use crate::temporal::RecordedAsOf;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Relation(EventRelationError),
        Graph(EventGraphError),
        Id(IdValidationError),
        Revision(RevisionError),
        Policy(crate::security::SecurityPolicyError),
        Role(crate::security::RoleDefinitionError),
        Bundle(crate::security::PolicyBundleError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Relation(error) => write!(formatter, "{error}"),
                Self::Graph(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
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

    error_conversion!(EventRelationError, Relation);
    error_conversion!(EventGraphError, Graph);
    error_conversion!(IdValidationError, Id);
    error_conversion!(RevisionError, Revision);
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

    fn relation(
        id: u8,
        from: u8,
        to: u8,
        kind: EventRelationInputKind,
    ) -> Result<EventRelation, TestError> {
        Ok(value!(EventRelation::new(
            value!(uuid::<EventRelationId>(id)),
            value!(uuid::<EventId>(from)),
            value!(uuid::<EventId>(to)),
            kind,
            value!(revision(3)),
        )))
    }

    fn relation_at(
        id: u8,
        from: u8,
        to: u8,
        kind: EventRelationInputKind,
        created_revision: u64,
    ) -> Result<EventRelation, TestError> {
        Ok(value!(EventRelation::new(
            value!(uuid::<EventRelationId>(id)),
            value!(uuid::<EventId>(from)),
            value!(uuid::<EventId>(to)),
            kind,
            value!(revision(created_revision)),
        )))
    }

    #[test]
    fn event_relation_requires_a_visible_relationship_grant() -> TestResult {
        use crate::security::{
            Capability, CapabilityGrant, CapabilityRule, GrantEffect, PolicyBundle,
            PolicyEventRelationKind, PolicyScope, PolicySubject, Principal, RelationshipSelector,
            RoleAssignment, RoleDefinition, SecurityPolicySnapshot,
        };

        let relation = relation(110, 111, 112, EventRelationInputKind::Before)?;
        let principal = uuid::<crate::ids::PrincipalId>(113)?;
        let role_id = uuid::<crate::ids::RoleId>(114)?;
        let assignment_id = uuid::<crate::ids::RoleAssignmentId>(115)?;
        let role = RoleDefinition::new(
            role_id,
            "relation_reader",
            PolicyBundle::from_grants([CapabilityGrant::new(
                Capability::RelationshipRead,
                GrantEffect::Allow,
            )])?,
        )?;
        let assignment =
            RoleAssignment::new(assignment_id, principal, role_id, PolicyScope::project());
        let policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role.clone()],
            vec![assignment],
            Vec::new(),
        )?;
        let history_space = uuid::<HistorySpaceId>(116)?;
        assert!(event_relation_is_authorized(
            &policy,
            principal,
            &relation,
            history_space,
        ));
        let deny = CapabilityRule::new(
            uuid::<crate::ids::PolicyRuleId>(117)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::RelationshipRead, GrantEffect::Deny),
            PolicyScope::new(
                Some(history_space),
                None,
                Some(crate::RecordRef::EventRelation(relation.id())),
                None,
                Some(RelationshipSelector::EventRelation(
                    PolicyEventRelationKind::Before,
                )),
            ),
        );
        let denied = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role],
            vec![assignment],
            vec![deny],
        )?;
        assert!(!event_relation_is_authorized(
            &denied,
            principal,
            &relation,
            history_space,
        ));
        Ok(())
    }

    fn batch(relations: Vec<EventRelation>) -> Result<EventRelationBatch, EventRelationError> {
        EventRelationBatch::new(relations)
    }

    #[test]
    fn after_is_stored_as_the_inverse_before_edge() -> TestResult {
        let after = relation(1, 8, 9, EventRelationInputKind::After)?;
        let before = relation(1, 9, 8, EventRelationInputKind::Before)?;
        assert_eq!(after, before);
        assert_eq!(after.kind(), EventRelationKind::Before);
        assert_eq!(after.from_event(), value!(uuid::<EventId>(9)));
        assert_eq!(after.to_event(), value!(uuid::<EventId>(8)));
        Ok(())
    }

    #[test]
    fn same_time_is_symmetric_and_canonically_ordered() -> TestResult {
        let forward = relation(2, 8, 9, EventRelationInputKind::SameTime)?;
        let reverse = relation(2, 9, 8, EventRelationInputKind::SameTime)?;
        assert_eq!(forward, reverse);
        assert_eq!(forward.kind(), EventRelationKind::SameTime);
        assert!(forward.from_event() < forward.to_event());
        Ok(())
    }

    #[test]
    fn self_relations_are_rejected_for_every_input_kind() -> TestResult {
        let event_id = value!(uuid::<EventId>(10));
        for kind in [
            EventRelationInputKind::Before,
            EventRelationInputKind::After,
            EventRelationInputKind::SameTime,
            EventRelationInputKind::Causes,
        ] {
            assert_eq!(
                EventRelation::new(
                    value!(uuid::<EventRelationId>(3)),
                    event_id,
                    event_id,
                    kind,
                    Revision::GENESIS,
                )
                .err(),
                Some(EventRelationError::SelfRelation { event_id })
            );
        }
        Ok(())
    }

    #[test]
    fn candidate_batch_rejects_normalized_before_after_and_sametime_duplicates() -> TestResult {
        let before = relation(4, 8, 9, EventRelationInputKind::Before)?;
        let after = relation(5, 9, 8, EventRelationInputKind::After)?;
        assert_eq!(
            EventRelationBatch::new(vec![before, after]).err(),
            Some(EventRelationError::DuplicateRelation { key: before.key() })
        );

        let same_time = relation(6, 8, 9, EventRelationInputKind::SameTime)?;
        let inverse_same_time = relation(7, 9, 8, EventRelationInputKind::SameTime)?;
        assert_eq!(
            EventRelationBatch::new(vec![same_time, inverse_same_time]).err(),
            Some(EventRelationError::DuplicateRelation {
                key: same_time.key(),
            })
        );
        Ok(())
    }

    #[test]
    fn candidate_batch_is_ordered_by_canonical_relation_key() -> TestResult {
        let causes = relation(8, 9, 10, EventRelationInputKind::Causes)?;
        let before = relation(9, 8, 10, EventRelationInputKind::Before)?;
        let batch = value!(EventRelationBatch::new(vec![causes, before]));
        assert_eq!(batch.as_slice(), &[before, causes]);
        Ok(())
    }

    #[test]
    fn relation_batch_is_canonical_for_every_three_edge_input_permutation() -> TestResult {
        let before = relation(20, 1, 2, EventRelationInputKind::Before)?;
        let causes = relation(21, 2, 3, EventRelationInputKind::Causes)?;
        let same_time = relation(22, 1, 3, EventRelationInputKind::SameTime)?;
        let expected = value!(EventRelationBatch::new(vec![before, causes, same_time,]))
            .as_slice()
            .iter()
            .map(|relation| relation.key())
            .collect::<Vec<_>>();
        let permutations = [
            vec![before, causes, same_time],
            vec![before, same_time, causes],
            vec![causes, before, same_time],
            vec![causes, same_time, before],
            vec![same_time, before, causes],
            vec![same_time, causes, before],
        ];

        for permutation in permutations {
            let actual = value!(EventRelationBatch::new(permutation))
                .as_slice()
                .iter()
                .map(|relation| relation.key())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
        Ok(())
    }

    #[test]
    fn relation_retraction_is_a_separate_later_lifecycle_record() -> TestResult {
        let relation = relation(11, 8, 9, EventRelationInputKind::Causes)?;
        let retraction = value!(EventRelationRetraction::new(
            value!(uuid::<EventRelationRetractionId>(12)),
            &relation,
            "relation correction",
            value!(revision(4)),
        ));
        assert_eq!(retraction.event_relation_id(), relation.id());
        assert_eq!(retraction.reason(), "relation correction");
        assert_eq!(
            EventRelationRetraction::new(
                value!(uuid::<EventRelationRetractionId>(13)),
                &relation,
                "too early",
                value!(revision(3)),
            )
            .err(),
            Some(EventRelationError::LifecycleRevisionNotAfterTarget {
                target_revision: value!(revision(3)),
                lifecycle_revision: value!(revision(3)),
            })
        );
        Ok(())
    }

    #[test]
    fn relation_projection_filters_transaction_time_and_returns_key_order() -> TestResult {
        let before = relation_at(14, 9, 8, EventRelationInputKind::After, 3)?;
        let causes = relation_at(15, 7, 6, EventRelationInputKind::Causes, 3)?;
        let retraction = value!(EventRelationRetraction::new(
            value!(uuid::<EventRelationRetractionId>(16)),
            &before,
            "relation withdrawn",
            value!(revision(5)),
        ));
        let relations = [causes, before];
        let retractions = [retraction];

        let before_creation = project_active_event_relations(
            EventRelationHistory::new(&relations, &retractions),
            RecordedAsOf::from_published_revision(value!(revision(2))),
        )?;
        assert!(before_creation.is_empty());

        let before_retraction = project_active_event_relations(
            EventRelationHistory::new(&relations, &retractions),
            RecordedAsOf::from_published_revision(value!(revision(4))),
        )?;
        assert_eq!(before_retraction.len(), 2);
        assert!(
            before_retraction
                .windows(2)
                .all(|pair| matches!(pair, [left, right] if left.key() < right.key()))
        );

        let after_retraction = project_active_event_relations(
            EventRelationHistory::new(&relations, &retractions),
            RecordedAsOf::from_published_revision(value!(revision(5))),
        )?;
        assert_eq!(after_retraction, vec![causes]);
        Ok(())
    }

    #[test]
    fn relation_projection_allows_key_reuse_only_after_prior_retraction() -> TestResult {
        let first = relation_at(17, 8, 9, EventRelationInputKind::Before, 2)?;
        let retraction = value!(EventRelationRetraction::new(
            value!(uuid::<EventRelationRetractionId>(18)),
            &first,
            "replaced relation",
            value!(revision(3)),
        ));
        let replacement = relation_at(19, 9, 8, EventRelationInputKind::After, 4)?;
        assert_eq!(first.key(), replacement.key());
        let relations = [replacement, first];
        let retractions = [retraction];
        let current = project_active_event_relations(
            EventRelationHistory::new(&relations, &retractions),
            RecordedAsOf::from_published_revision(value!(revision(4))),
        )?;
        assert_eq!(current, vec![replacement]);

        let overlapping = [first, replacement];
        assert_eq!(
            project_active_event_relations(
                EventRelationHistory::new(&overlapping, &[]),
                RecordedAsOf::from_published_revision(value!(revision(4))),
            )
            .err(),
            Some(EventRelationError::DuplicateRelation { key: first.key() })
        );
        Ok(())
    }

    #[test]
    fn relation_projection_rejects_duplicate_identity_and_missing_retraction_target() -> TestResult
    {
        let relation = relation_at(20, 3, 4, EventRelationInputKind::Before, 2)?;
        assert_eq!(
            project_active_event_relations(
                EventRelationHistory::new(&[relation, relation], &[]),
                RecordedAsOf::from_published_revision(value!(revision(3))),
            )
            .err(),
            Some(EventRelationError::DuplicateRelationId {
                event_relation_id: relation.id(),
            })
        );
        let missing = EventRelationRetraction::from_wire_fields(
            value!(uuid::<EventRelationRetractionId>(21)),
            value!(uuid::<EventRelationId>(22)),
            "missing target".to_owned(),
            value!(revision(3)),
        );
        assert_eq!(
            project_active_event_relations(
                EventRelationHistory::new(&[relation], &[missing]),
                RecordedAsOf::from_published_revision(value!(revision(3))),
            )
            .err(),
            Some(EventRelationError::MissingRetractionTarget {
                event_relation_id: value!(uuid::<EventRelationId>(22)),
            })
        );
        Ok(())
    }

    #[test]
    fn graph_projection_builds_same_time_components_without_transitive_records() -> TestResult {
        let events = [
            value!(uuid::<EventId>(1)),
            value!(uuid::<EventId>(2)),
            value!(uuid::<EventId>(3)),
            value!(uuid::<EventId>(4)),
        ];
        let explicit = value!(batch(vec![
            relation_at(30, 1, 2, EventRelationInputKind::SameTime, 3)?,
            relation_at(31, 2, 3, EventRelationInputKind::SameTime, 3)?,
        ]));
        let projection = validate_event_graph_transaction(
            &events,
            EventRelationHistory::new(&[], &[]),
            RecordedAsOf::from_published_revision(value!(revision(2))),
            value!(revision(3)),
            &[],
            &explicit,
        )?;
        assert_eq!(projection.active_relations().len(), 2);
        assert_eq!(
            projection.same_time_components(),
            &[
                vec![
                    value!(uuid::<EventId>(1)),
                    value!(uuid::<EventId>(2)),
                    value!(uuid::<EventId>(3))
                ],
                vec![value!(uuid::<EventId>(4))],
            ]
        );
        Ok(())
    }

    #[test]
    fn before_inside_a_same_time_component_is_a_conflict() -> TestResult {
        let events = [value!(uuid::<EventId>(1)), value!(uuid::<EventId>(2))];
        let same_time = relation_at(32, 1, 2, EventRelationInputKind::SameTime, 3)?;
        let before = relation_at(33, 2, 1, EventRelationInputKind::Before, 3)?;
        let relations = value!(batch(vec![same_time, before]));
        assert_eq!(
            validate_event_graph_transaction(
                &events,
                EventRelationHistory::new(&[], &[]),
                RecordedAsOf::from_published_revision(value!(revision(2))),
                value!(revision(3)),
                &[],
                &relations,
            )
            .err(),
            Some(EventGraphError::BeforeWithinSameTimeComponent {
                event_relation_id: before.id(),
                component_representative: value!(uuid::<EventId>(1)),
            })
        );
        Ok(())
    }

    #[test]
    fn before_cycle_after_same_time_collapse_is_stable_under_input_order() -> TestResult {
        let events = (1..=4)
            .map(uuid::<EventId>)
            .collect::<Result<Vec<_>, _>>()?;
        let edge_one = relation_at(34, 2, 3, EventRelationInputKind::Before, 3)?;
        let edge_two = relation_at(35, 3, 4, EventRelationInputKind::Before, 3)?;
        let same_left = relation_at(36, 1, 2, EventRelationInputKind::SameTime, 3)?;
        let same_right = relation_at(37, 1, 4, EventRelationInputKind::SameTime, 3)?;
        let relations = value!(batch(vec![edge_one, edge_two, same_left, same_right]));
        let as_of = RecordedAsOf::from_published_revision(value!(revision(2)));
        let commit = value!(revision(3));
        let first = validate_event_graph_transaction(
            &events,
            EventRelationHistory::new(&[], &[]),
            as_of,
            commit,
            &[],
            &relations,
        )
        .err();
        let reversed_events = events.iter().rev().copied().collect::<Vec<_>>();
        let reversed_relations = value!(batch(vec![same_right, same_left, edge_two, edge_one]));
        let second = validate_event_graph_transaction(
            &reversed_events,
            EventRelationHistory::new(&[], &[]),
            as_of,
            commit,
            &[],
            &reversed_relations,
        )
        .err();
        assert_eq!(first, second);
        assert!(matches!(first, Some(EventGraphError::BeforeCycle { .. })));
        Ok(())
    }

    #[test]
    fn causes_cycles_are_checked_separately_and_do_not_create_before_edges() -> TestResult {
        let events = [value!(uuid::<EventId>(1)), value!(uuid::<EventId>(2))];
        let causes = value!(batch(vec![relation_at(
            38,
            1,
            2,
            EventRelationInputKind::Causes,
            3,
        )?]));
        let valid = validate_event_graph_transaction(
            &events,
            EventRelationHistory::new(&[], &[]),
            RecordedAsOf::from_published_revision(value!(revision(2))),
            value!(revision(3)),
            &[],
            &causes,
        )?;
        assert!(
            valid
                .active_relations()
                .iter()
                .all(|relation| relation.kind() == EventRelationKind::Causes)
        );

        let cycle = value!(batch(vec![
            relation_at(39, 1, 2, EventRelationInputKind::Causes, 3)?,
            relation_at(40, 2, 1, EventRelationInputKind::Causes, 3)?,
        ]));
        assert!(matches!(
            validate_event_graph_transaction(
                &events,
                EventRelationHistory::new(&[], &[]),
                RecordedAsOf::from_published_revision(value!(revision(2))),
                value!(revision(3)),
                &[],
                &cycle,
            ),
            Err(EventGraphError::CausesCycle { .. })
        ));
        Ok(())
    }

    #[test]
    fn post_transaction_graph_applies_retractions_before_cycle_validation() -> TestResult {
        let old = relation_at(41, 1, 2, EventRelationInputKind::Before, 2)?;
        let replacement = relation_at(42, 2, 1, EventRelationInputKind::Before, 4)?;
        let retraction = value!(EventRelationRetraction::new(
            value!(uuid::<EventRelationRetractionId>(43)),
            &old,
            "replace direction",
            value!(revision(4)),
        ));
        let history = [old];
        let addition = value!(batch(vec![replacement]));
        let events = [value!(uuid::<EventId>(1)), value!(uuid::<EventId>(2))];
        let projection = validate_event_graph_transaction(
            &events,
            EventRelationHistory::new(&history, &[]),
            RecordedAsOf::from_published_revision(value!(revision(3))),
            value!(revision(4)),
            std::slice::from_ref(&retraction),
            &addition,
        )?;
        assert_eq!(projection.active_relations(), &[replacement]);

        let cycle_addition = value!(batch(vec![replacement]));
        assert!(matches!(
            validate_event_graph_transaction(
                &events,
                EventRelationHistory::new(&history, &[]),
                RecordedAsOf::from_published_revision(value!(revision(3))),
                value!(revision(4)),
                &[],
                &cycle_addition,
            ),
            Err(EventGraphError::BeforeCycle { .. })
        ));
        Ok(())
    }

    #[test]
    fn event_times_are_not_graph_inputs_and_cannot_infer_relations() -> TestResult {
        let events = [value!(uuid::<EventId>(1)), value!(uuid::<EventId>(2))];
        let empty = value!(batch(Vec::new()));
        let projection = validate_event_graph_transaction(
            &events,
            EventRelationHistory::new(&[], &[]),
            RecordedAsOf::from_published_revision(value!(revision(2))),
            value!(revision(3)),
            &[],
            &empty,
        )?;
        assert!(projection.active_relations().is_empty());
        assert_eq!(projection.same_time_components().len(), 2);
        Ok(())
    }
}
