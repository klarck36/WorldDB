//! Authorized Source/Evidence and Provenance neighborhood indexes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::catalog::HistorySpaceCatalog;
use crate::ids::{EvidenceId, ProvenanceId, SourceId};
use crate::provenance_graph::{ProvenanceGraphError, project_authorized_provenance_edges};
use crate::query_context::QueryContext;
use crate::security::{PolicyTarget, SecurityPolicySnapshot};
use crate::source_evidence_projection::{
    EvidenceHistoryEntry, SourceEvidenceError, SourceEvidenceHistory, SourceEvidenceProjection,
    SourceEvidenceQuery, full_scan_authorized_source_evidence,
};
use crate::source_provenance::{
    EvidenceRelation, EvidenceTargetRef, ProvenanceEdge, ProvenanceEdgeHistory,
    ProvenanceEndpointRef, ProvenanceRelation, Source,
};

/// Direction used when looking up explicitly stored Provenance endpoints.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProvenanceIndexDirection {
    /// Select edges stored from the queried endpoint.
    Outgoing,
    /// Select edges stored to the queried endpoint.
    Incoming,
    /// Select edges stored on either side of the queried endpoint.
    Both,
}

/// Evidence adjacency built only after source, target, relation, and lifecycle authorization.
///
/// The index is bound to one authorized query projection. Build it again when the query
/// context or policy changes. Its lookups return only the Sources and Evidence already
/// present in that projection, including historically visible retracted edges.
#[derive(Clone, Debug)]
pub struct EvidenceNeighborhoodIndex<'a> {
    projection: SourceEvidenceProjection<'a>,
    source_ids: BTreeSet<SourceId>,
    entry_positions: BTreeMap<EvidenceId, usize>,
    by_source: BTreeMap<SourceId, BTreeSet<EvidenceId>>,
    by_target: HashMap<EvidenceTargetRef, BTreeSet<EvidenceId>>,
    by_relation: HashMap<EvidenceRelation, BTreeSet<EvidenceId>>,
}

impl<'a> EvidenceNeighborhoodIndex<'a> {
    fn from_authorized_projection(
        projection: SourceEvidenceProjection<'a>,
    ) -> Result<Self, SourceProvenanceIndexError> {
        let mut source_ids = BTreeSet::new();
        for source in projection.sources() {
            if !source_ids.insert(source.id()) {
                return Err(SourceProvenanceIndexError::DuplicateVisibleSource(
                    source.id(),
                ));
            }
        }

        let mut index = Self {
            projection,
            source_ids,
            entry_positions: BTreeMap::new(),
            by_source: BTreeMap::new(),
            by_target: HashMap::new(),
            by_relation: HashMap::new(),
        };
        for (position, entry) in index.projection.evidence().iter().enumerate() {
            let evidence = entry.evidence();
            let evidence_id = evidence.id();
            if index
                .entry_positions
                .insert(evidence_id, position)
                .is_some()
            {
                return Err(SourceProvenanceIndexError::DuplicateVisibleEvidence(
                    evidence_id,
                ));
            }
            index
                .by_source
                .entry(entry.source().id())
                .or_default()
                .insert(evidence_id);
            index
                .by_target
                .entry(entry.target_history().target())
                .or_default()
                .insert(evidence_id);
            index
                .by_relation
                .entry(evidence.relation())
                .or_default()
                .insert(evidence_id);
        }
        Ok(index)
    }

    /// Returns only Sources that were authorized and visible in the bound query.
    #[must_use]
    pub fn sources(&self) -> &[&'a Source] {
        self.projection.sources()
    }

    /// Returns visible Source identities in canonical order.
    #[must_use]
    pub const fn source_ids(&self) -> &BTreeSet<SourceId> {
        &self.source_ids
    }

    /// Returns every visible Evidence entry, including entries retracted by the query point.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceHistoryEntry<'a>] {
        self.projection.evidence()
    }

    /// Returns visible Evidence entries attached to one Source, ordered by EvidenceId.
    #[must_use]
    pub fn for_source(&self, source_id: SourceId) -> Vec<&EvidenceHistoryEntry<'a>> {
        self.entries_for_ids(self.by_source.get(&source_id))
    }

    /// Returns visible Evidence entries attached to one typed target, ordered by EvidenceId.
    #[must_use]
    pub fn for_target(&self, target: EvidenceTargetRef) -> Vec<&EvidenceHistoryEntry<'a>> {
        self.entries_for_ids(self.by_target.get(&target))
    }

    /// Returns visible direct Source/target links; this does not infer transitive relations.
    #[must_use]
    pub fn between(
        &self,
        source_id: SourceId,
        target: EvidenceTargetRef,
    ) -> Vec<&EvidenceHistoryEntry<'a>> {
        let Some(source_ids) = self.by_source.get(&source_id) else {
            return Vec::new();
        };
        let Some(target_ids) = self.by_target.get(&target) else {
            return Vec::new();
        };
        let ids = source_ids.intersection(target_ids).copied().collect();
        self.entries_for_ids(Some(&ids))
    }

    /// Returns visible Evidence entries of one explicit relation class.
    #[must_use]
    pub fn for_relation(&self, relation: EvidenceRelation) -> Vec<&EvidenceHistoryEntry<'a>> {
        self.entries_for_ids(self.by_relation.get(&relation))
    }

    fn entries_for_ids(
        &self,
        ids: Option<&BTreeSet<EvidenceId>>,
    ) -> Vec<&EvidenceHistoryEntry<'a>> {
        ids.into_iter()
            .flatten()
            .filter_map(|id| {
                self.entry_positions
                    .get(id)
                    .and_then(|position| self.projection.evidence().get(*position))
            })
            .collect()
    }
}

/// Builds visible Source/Evidence postings from the authorized reference projection.
pub fn full_scan_authorized_evidence_neighborhood_index<'a>(
    history: SourceEvidenceHistory<'a>,
    history_spaces: &HistorySpaceCatalog,
    query: SourceEvidenceQuery,
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> Result<EvidenceNeighborhoodIndex<'a>, SourceProvenanceIndexError> {
    let projection =
        full_scan_authorized_source_evidence(history, history_spaces, query, policy, context)?;
    EvidenceNeighborhoodIndex::from_authorized_projection(projection)
}

/// Direct Provenance adjacency index over active edges authorized for one query.
///
/// Both endpoints and the relationship record are authorized before any edge enters the
/// index. An edge with a missing, malformed, or hidden endpoint is omitted as a whole.
#[derive(Clone, Debug, Default)]
pub struct ProvenanceAdjacencyIndex {
    edges: BTreeMap<ProvenanceId, ProvenanceEdge>,
    by_from: BTreeMap<ProvenanceEndpointRef, BTreeSet<ProvenanceId>>,
    by_to: BTreeMap<ProvenanceEndpointRef, BTreeSet<ProvenanceId>>,
    by_relation: BTreeMap<ProvenanceRelation, BTreeSet<ProvenanceId>>,
}

impl ProvenanceAdjacencyIndex {
    fn from_authorized_edges(edges: &[ProvenanceEdge]) -> Result<Self, SourceProvenanceIndexError> {
        let mut index = Self::default();
        for edge in edges {
            let id = edge.id();
            if index.edges.insert(id, *edge).is_some() {
                return Err(SourceProvenanceIndexError::DuplicateVisibleProvenance(id));
            }
            index.by_from.entry(edge.from()).or_default().insert(id);
            index.by_to.entry(edge.to()).or_default().insert(id);
            index
                .by_relation
                .entry(edge.relation())
                .or_default()
                .insert(id);
        }
        Ok(index)
    }

    /// Returns all active, authorized direct edges in canonical ProvenanceId order.
    #[must_use]
    pub fn edges(&self) -> Vec<ProvenanceEdge> {
        self.edges.values().copied().collect()
    }

    /// Returns active, authorized direct edges incident to an endpoint and relation.
    #[must_use]
    pub fn for_endpoint(
        &self,
        endpoint: ProvenanceEndpointRef,
        direction: ProvenanceIndexDirection,
        relation: Option<ProvenanceRelation>,
    ) -> Vec<ProvenanceEdge> {
        let mut ids = BTreeSet::new();
        if matches!(
            direction,
            ProvenanceIndexDirection::Outgoing | ProvenanceIndexDirection::Both
        ) {
            ids.extend(self.by_from.get(&endpoint).into_iter().flatten().copied());
        }
        if matches!(
            direction,
            ProvenanceIndexDirection::Incoming | ProvenanceIndexDirection::Both
        ) {
            ids.extend(self.by_to.get(&endpoint).into_iter().flatten().copied());
        }
        if let Some(relation) = relation {
            let Some(relation_ids) = self.by_relation.get(&relation) else {
                return Vec::new();
            };
            ids.retain(|id| relation_ids.contains(id));
        }
        ids.into_iter()
            .filter_map(|id| self.edges.get(&id).copied())
            .collect()
    }

    /// Returns exact stored-direction edges between two endpoints.
    #[must_use]
    pub fn between(
        &self,
        from: ProvenanceEndpointRef,
        to: ProvenanceEndpointRef,
        relation: Option<ProvenanceRelation>,
    ) -> Vec<ProvenanceEdge> {
        self.for_endpoint(from, ProvenanceIndexDirection::Outgoing, relation)
            .into_iter()
            .filter(|edge| edge.to() == to)
            .collect()
    }

    /// Returns direct adjacent endpoint identities in canonical order.
    #[must_use]
    pub fn neighbors(
        &self,
        endpoint: ProvenanceEndpointRef,
        direction: ProvenanceIndexDirection,
        relation: Option<ProvenanceRelation>,
    ) -> Vec<ProvenanceEndpointRef> {
        let mut neighbors = BTreeSet::new();
        for edge in self.for_endpoint(endpoint, direction, relation) {
            match direction {
                ProvenanceIndexDirection::Outgoing => {
                    if edge.from() == endpoint {
                        neighbors.insert(edge.to());
                    }
                }
                ProvenanceIndexDirection::Incoming => {
                    if edge.to() == endpoint {
                        neighbors.insert(edge.from());
                    }
                }
                ProvenanceIndexDirection::Both => {
                    if edge.from() == endpoint {
                        neighbors.insert(edge.to());
                    } else if edge.to() == endpoint {
                        neighbors.insert(edge.from());
                    }
                }
            }
        }
        neighbors.into_iter().collect()
    }
}

/// Builds direct adjacency postings after endpoint and relationship authorization.
pub fn full_scan_authorized_provenance_adjacency_index(
    history: ProvenanceEdgeHistory<'_>,
    endpoint_targets: &BTreeMap<ProvenanceEndpointRef, PolicyTarget>,
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> Result<ProvenanceAdjacencyIndex, SourceProvenanceIndexError> {
    let edges = project_authorized_provenance_edges(history, endpoint_targets, policy, context)?;
    ProvenanceAdjacencyIndex::from_authorized_edges(&edges)
}

/// Authorization or duplicate-identity failure while creating a visible adjacency index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceProvenanceIndexError {
    /// The authorized Source projection contains a repeated identity.
    DuplicateVisibleSource(SourceId),
    /// The authorized Evidence projection contains a repeated identity.
    DuplicateVisibleEvidence(EvidenceId),
    /// The authorized Provenance projection contains a repeated identity.
    DuplicateVisibleProvenance(ProvenanceId),
    /// Source/Evidence reference projection failed.
    SourceEvidence(SourceEvidenceError),
    /// Provenance reference projection failed.
    Provenance(ProvenanceGraphError),
}

impl From<SourceEvidenceError> for SourceProvenanceIndexError {
    fn from(error: SourceEvidenceError) -> Self {
        Self::SourceEvidence(error)
    }
}

impl From<ProvenanceGraphError> for SourceProvenanceIndexError {
    fn from(error: ProvenanceGraphError) -> Self {
        Self::Provenance(error)
    }
}

impl fmt::Display for SourceProvenanceIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Source/Evidence adjacency index failed: {self:?}"
        )
    }
}

impl std::error::Error for SourceProvenanceIndexError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ProvenanceIndexDirection, full_scan_authorized_evidence_neighborhood_index,
        full_scan_authorized_provenance_adjacency_index,
    };
    use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition};
    use crate::ids::{
        AssertionId, EvidenceId, EvidenceRetractionId, HistorySpaceId, PolicyRuleId, ProvenanceId,
        Revision, SourceId,
    };
    use crate::query_search::tests::{Fixture, fixture_with_denials, id};
    use crate::record_refs::RecordRef;
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, GrantEffect, PolicyScope, PolicySubject,
        PolicyTarget, Principal, SecurityPolicySnapshot,
    };
    use crate::source_evidence_projection::{
        EvidenceHistoryStatus, EvidenceTargetHistoryEntry, SourceEvidenceHistory,
        SourceEvidenceQuery, full_scan_authorized_source_evidence,
    };
    use crate::source_provenance::{
        Evidence, EvidenceRelation, EvidenceRetraction, EvidenceTargetRef, ProvenanceEdge,
        ProvenanceEdgeHistory, ProvenanceEndpointRef, ProvenanceRelation, Source, SourceMetadata,
    };
    use crate::values::Symbol;

    fn revision(value: u64) -> Result<Revision, String> {
        Revision::new(value).map_err(|error| error.to_string())
    }

    fn policy(
        fixture: &Fixture,
        denied_record: Option<RecordRef>,
    ) -> Result<SecurityPolicySnapshot, String> {
        let principal = fixture.context.security().principal_id();
        let capabilities = [
            Capability::QueryResolve,
            Capability::HistorySpaceRead,
            Capability::LayerRead,
            Capability::AssertionRead,
            Capability::SourceRead,
            Capability::EvidenceRead,
            Capability::ProvenanceRead,
            Capability::RelationshipRead,
            Capability::LifecycleRead,
            Capability::FieldRead,
        ];
        let mut rules = capabilities
            .into_iter()
            .enumerate()
            .map(|(index, capability)| {
                CapabilityRule::new(
                    id::<PolicyRuleId>(index as u8 + 30),
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(capability, GrantEffect::Allow),
                    PolicyScope::project(),
                )
            })
            .collect::<Vec<_>>();
        if let Some(record) = denied_record {
            rules.push(CapabilityRule::new(
                id::<PolicyRuleId>(60),
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::AssertionRead, GrantEffect::Deny),
                PolicyScope::new(Some(fixture.history_space), None, Some(record), None, None),
            ));
        }
        SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            Vec::new(),
            Vec::new(),
            rules,
        )
        .map_err(|error| error.to_string())
    }

    fn source(id: u8) -> Result<Source, String> {
        Ok(Source::new(
            self::id::<SourceId>(id),
            Symbol::new("web").map_err(|error| error.to_string())?,
            None,
            None,
            SourceMetadata::default(),
            revision(1)?,
        ))
    }

    fn evidence(id: u8, source: SourceId, target: EvidenceTargetRef) -> Result<Evidence, String> {
        Ok(Evidence::new(
            self::id::<EvidenceId>(id),
            source,
            target,
            EvidenceRelation::Documents,
            revision(1)?,
        ))
    }

    fn provenance_edge(
        id: u8,
        from: ProvenanceEndpointRef,
        to: ProvenanceEndpointRef,
    ) -> Result<ProvenanceEdge, String> {
        ProvenanceEdge::new(
            self::id::<ProvenanceId>(id),
            from,
            to,
            ProvenanceRelation::DerivedFrom,
            revision(1)?,
        )
        .map_err(|error| error.to_string())
    }

    fn history_catalog(root: HistorySpaceId) -> Result<HistorySpaceCatalog, String> {
        HistorySpaceCatalog::new(vec![
            HistorySpaceDefinition::new(root, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        ])
        .map_err(|error| error.to_string())
    }

    fn endpoint_target(fixture: &Fixture, endpoint: ProvenanceEndpointRef) -> PolicyTarget {
        let record = match endpoint {
            ProvenanceEndpointRef::Assertion(id) => RecordRef::Assertion(id),
            _ => unreachable!("test uses Assertion endpoints"),
        };
        PolicyTarget::new(
            Some(fixture.history_space),
            Some(fixture.layer),
            Some(record),
            None,
            None,
        )
    }

    #[test]
    fn evidence_neighborhood_matches_authorized_reference_and_hides_targets() -> Result<(), String>
    {
        let fixture = fixture_with_denials(None, None, 10, 10);
        let source = source(101)?;
        let visible_target = EvidenceTargetRef::Assertion(id::<AssertionId>(102));
        let retracted_target = EvidenceTargetRef::Assertion(id::<AssertionId>(106));
        let hidden_target = EvidenceTargetRef::Assertion(id::<AssertionId>(103));
        let visible_edge = evidence(104, source.id(), visible_target)?;
        let retracted_edge = evidence(107, source.id(), retracted_target)?;
        let hidden_edge = evidence(105, source.id(), hidden_target)?;
        let retraction = EvidenceRetraction::new(
            id::<EvidenceRetractionId>(108),
            &retracted_edge,
            "replaced source link",
            revision(2)?,
        )
        .map_err(|error| error.to_string())?;
        let sources = [source];
        let evidence_records = [visible_edge, retracted_edge, hidden_edge];
        let retractions = [retraction];
        let targets = [
            EvidenceTargetHistoryEntry::new(
                visible_target,
                revision(1)?,
                Some(fixture.history_space),
            ),
            EvidenceTargetHistoryEntry::new(
                retracted_target,
                revision(1)?,
                Some(fixture.history_space),
            ),
            EvidenceTargetHistoryEntry::new(
                hidden_target,
                revision(1)?,
                Some(fixture.history_space),
            ),
        ];
        let history =
            SourceEvidenceHistory::new(&sources, &evidence_records, &retractions, &targets);
        let catalog = history_catalog(fixture.history_space)?;
        let query =
            SourceEvidenceQuery::new(fixture.history_space, fixture.context.recorded_as_of());
        let policy = policy(
            &fixture,
            Some(RecordRef::Assertion(match hidden_target {
                EvidenceTargetRef::Assertion(id) => id,
                _ => unreachable!("test target is an Assertion"),
            })),
        )?;

        let reference = full_scan_authorized_source_evidence(
            history.clone(),
            &catalog,
            query,
            &policy,
            &fixture.context,
        )
        .map_err(|error| error.to_string())?;
        let index = full_scan_authorized_evidence_neighborhood_index(
            history,
            &catalog,
            query,
            &policy,
            &fixture.context,
        )
        .map_err(|error| error.to_string())?;

        let expected = reference
            .evidence()
            .iter()
            .map(|entry| entry.evidence().id())
            .collect::<Vec<_>>();
        let by_source = index
            .for_source(sources[0].id())
            .into_iter()
            .map(|entry| entry.evidence().id())
            .collect::<Vec<_>>();
        assert_eq!(by_source, expected);
        assert_eq!(index.for_target(visible_target).len(), 1);
        assert!(index.for_target(hidden_target).is_empty());
        assert_eq!(index.between(sources[0].id(), visible_target).len(), 1);
        assert_eq!(index.between(sources[0].id(), hidden_target).len(), 0);
        assert_eq!(index.for_relation(EvidenceRelation::Documents).len(), 2);
        assert_eq!(
            index
                .for_target(retracted_target)
                .first()
                .map(|entry| entry.status()),
            Some(EvidenceHistoryStatus::RetractedAt(revision(2)?))
        );
        assert_eq!(index.source_ids().len(), 1);

        let visible_only_records = [visible_edge, retracted_edge];
        let visible_only_history =
            SourceEvidenceHistory::new(&sources, &visible_only_records, &retractions, &targets);
        let visible_only = full_scan_authorized_evidence_neighborhood_index(
            visible_only_history,
            &catalog,
            query,
            &policy,
            &fixture.context,
        )
        .map_err(|error| error.to_string())?;
        let indexed_ids = index
            .evidence()
            .iter()
            .map(|entry| entry.evidence().id())
            .collect::<Vec<_>>();
        let visible_only_ids = visible_only
            .evidence()
            .iter()
            .map(|entry| entry.evidence().id())
            .collect::<Vec<_>>();
        assert_eq!(indexed_ids, visible_only_ids);
        Ok(())
    }

    #[test]
    fn provenance_adjacency_matches_authorized_scan_and_drops_hidden_endpoint_edges()
    -> Result<(), String> {
        let fixture = fixture_with_denials(None, None, 10, 10);
        let root = ProvenanceEndpointRef::Assertion(id::<AssertionId>(111));
        let visible = ProvenanceEndpointRef::Assertion(id::<AssertionId>(112));
        let hidden = ProvenanceEndpointRef::Assertion(id::<AssertionId>(113));
        let visible_edge = provenance_edge(114, root, visible)?;
        let hidden_edge = provenance_edge(115, root, hidden)?;
        let edges = [visible_edge, hidden_edge];
        let history = ProvenanceEdgeHistory::new(&edges, &[]);
        let endpoint_targets = BTreeMap::from([
            (root, endpoint_target(&fixture, root)),
            (visible, endpoint_target(&fixture, visible)),
            (hidden, endpoint_target(&fixture, hidden)),
        ]);
        let policy = policy(&fixture, Some(RecordRef::Assertion(id::<AssertionId>(113))))?;
        let authorized = crate::provenance_graph::project_authorized_provenance_edges(
            history,
            &endpoint_targets,
            &policy,
            &fixture.context,
        )
        .map_err(|error| error.to_string())?;
        let index = full_scan_authorized_provenance_adjacency_index(
            history,
            &endpoint_targets,
            &policy,
            &fixture.context,
        )
        .map_err(|error| error.to_string())?;

        let expected = authorized
            .iter()
            .filter(|edge| edge.from() == root)
            .map(|edge| edge.id())
            .collect::<Vec<_>>();
        let actual = index
            .for_endpoint(
                root,
                ProvenanceIndexDirection::Outgoing,
                Some(ProvenanceRelation::DerivedFrom),
            )
            .iter()
            .map(|edge| edge.id())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert_eq!(
            index.neighbors(
                root,
                ProvenanceIndexDirection::Outgoing,
                Some(ProvenanceRelation::DerivedFrom),
            ),
            vec![visible]
        );
        assert!(!index.edges().iter().any(|edge| edge.to() == hidden));

        let visible_only_history = ProvenanceEdgeHistory::new(&edges[..1], &[]);
        let visible_only = full_scan_authorized_provenance_adjacency_index(
            visible_only_history,
            &endpoint_targets,
            &policy,
            &fixture.context,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(index.edges(), visible_only.edges());
        Ok(())
    }
}
