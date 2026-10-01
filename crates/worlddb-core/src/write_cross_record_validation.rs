//! Atomic write-time security and relationship validation over complete candidate state.

use std::collections::BTreeSet;
use std::fmt;

use crate::catalog::HistorySpaceCatalog;
use crate::event_relations::{
    EventGraphError, EventGraphProjection, EventRelationBatch, EventRelationHistory,
    EventRelationRetraction, validate_event_graph_transaction,
};
use crate::ids::{EventId, HistorySpaceId, PrincipalId, Revision};
use crate::provenance_graph::{
    GraphValidationBudget, ProvenanceGraphError, ProvenanceGraphProjection,
    validate_provenance_graph_transaction,
};
use crate::schema_write_validation::ValidatedAssertionBatch;
use crate::security::SecurityPolicySnapshot;
use crate::source_evidence_projection::{
    SourceEvidenceError, SourceEvidenceHistory, SourceEvidenceProjection,
    validate_source_evidence_post_transaction,
};
use crate::source_provenance::{
    ProvenanceEdge, ProvenanceEdgeHistory, ProvenanceEndpointRef, ProvenanceRetraction,
};
use crate::temporal::RecordedAsOf;
use crate::write_authorization::{
    WriteAuthorizationError, authorize_deprecated_schema_warnings, authorize_validated_write_batch,
};
use crate::write_reference_validation::ValidatedWriteReferenceBatch;

/// All validated relationship projections for one write attempt.
pub struct ValidatedWriteCrossRecordState<'a> {
    references: &'a ValidatedWriteReferenceBatch,
    event_graph: EventGraphProjection,
    provenance_graph: ProvenanceGraphProjection,
    source_evidence: SourceEvidenceProjection<'a>,
}

/// Complete candidate graph and Source/Evidence state for a write attempt.
///
/// Every collection represents the full post-transaction state, except the
/// explicit retraction/addition lists which describe the candidate delta.
pub struct WriteCrossRecordCandidate<'a> {
    /// Assertion drafts and schema warnings produced by post-transaction schema validation.
    pub schema_assertions: &'a ValidatedAssertionBatch,
    /// Complete post-transaction Event inventory.
    pub event_ids: &'a [EventId],
    /// Existing EventRelation history visible at the selected snapshot.
    pub event_relation_history: EventRelationHistory<'a>,
    /// Published base revision from which this write was prepared.
    pub recorded_as_of: RecordedAsOf,
    /// Revision that would be assigned to this commit.
    pub commit_revision: Revision,
    /// Candidate EventRelation retractions.
    pub event_relation_retractions: &'a [EventRelationRetraction],
    /// Candidate EventRelation additions.
    pub event_relation_additions: &'a EventRelationBatch,
    /// Existing Provenance history visible at the selected snapshot.
    pub provenance_history: ProvenanceEdgeHistory<'a>,
    /// Candidate Provenance retractions.
    pub provenance_retractions: &'a [ProvenanceRetraction],
    /// Candidate Provenance additions.
    pub provenance_additions: &'a [ProvenanceEdge],
    /// Complete typed endpoint inventory after this transaction.
    pub provenance_endpoints_after: &'a BTreeSet<ProvenanceEndpointRef>,
    /// Bounded work budget for cycle validation.
    pub graph_budget: GraphValidationBudget,
    /// Complete existing plus staged Source/Evidence history.
    pub source_evidence_history: SourceEvidenceHistory<'a>,
    /// Pinned HistorySpace catalog used by Source/Evidence validation.
    pub history_spaces: &'a HistorySpaceCatalog,
    /// HistorySpace containing candidate Evidence records.
    pub evidence_history_space: HistorySpaceId,
}

impl<'a> ValidatedWriteCrossRecordState<'a> {
    /// Returns the reference-validated assertion/event drafts.
    #[must_use]
    pub const fn references(&self) -> &'a ValidatedWriteReferenceBatch {
        self.references
    }

    /// Returns the canonical post-transaction EventRelation graph.
    #[must_use]
    pub const fn event_graph(&self) -> &EventGraphProjection {
        &self.event_graph
    }

    /// Returns the canonical post-transaction Provenance dependency graph.
    #[must_use]
    pub const fn provenance_graph(&self) -> &ProvenanceGraphProjection {
        &self.provenance_graph
    }

    /// Returns the structurally valid post-transaction Source/Evidence view.
    #[must_use]
    pub const fn source_evidence(&self) -> &SourceEvidenceProjection<'a> {
        &self.source_evidence
    }
}

/// Validates permissions and relationship graphs before OCC or publication.
///
/// Inputs describe one candidate commit. Event IDs and provenance endpoints
/// must be the complete post-transaction inventories, and SourceEvidenceHistory
/// must include every retained record plus every staged change. The currently
/// authenticated Principal and current policy are re-evaluated on each call.
/// Any rejection returns no validated cross-record result.
pub fn validate_write_cross_record_state<'a>(
    references: &'a ValidatedWriteReferenceBatch,
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    candidate: WriteCrossRecordCandidate<'a>,
) -> Result<ValidatedWriteCrossRecordState<'a>, WriteCrossRecordValidationError> {
    authorize_validated_write_batch(references, policy, principal)
        .map_err(WriteCrossRecordValidationError::Authorization)?;
    authorize_deprecated_schema_warnings(
        references,
        candidate.schema_assertions,
        policy,
        principal,
    )
    .map_err(WriteCrossRecordValidationError::Authorization)?;

    for edge in candidate.provenance_additions {
        for endpoint in [edge.from(), edge.to()] {
            if !candidate.provenance_endpoints_after.contains(&endpoint) {
                return Err(WriteCrossRecordValidationError::MissingProvenanceEndpoint(
                    endpoint,
                ));
            }
        }
    }

    let event_graph = validate_event_graph_transaction(
        candidate.event_ids,
        candidate.event_relation_history,
        candidate.recorded_as_of,
        candidate.commit_revision,
        candidate.event_relation_retractions,
        candidate.event_relation_additions,
    )
    .map_err(WriteCrossRecordValidationError::EventGraph)?;
    let provenance_graph = validate_provenance_graph_transaction(
        candidate.provenance_history,
        candidate.recorded_as_of,
        candidate.commit_revision,
        candidate.provenance_retractions,
        candidate.provenance_additions,
        candidate.graph_budget,
    )
    .map_err(WriteCrossRecordValidationError::ProvenanceGraph)?;
    let source_evidence = validate_source_evidence_post_transaction(
        candidate.source_evidence_history,
        candidate.history_spaces,
        candidate.evidence_history_space,
        candidate.commit_revision,
    )
    .map_err(WriteCrossRecordValidationError::SourceEvidence)?;

    Ok(ValidatedWriteCrossRecordState {
        references,
        event_graph,
        provenance_graph,
        source_evidence,
    })
}

/// Why the complete write candidate could not be accepted.
#[derive(Debug)]
pub enum WriteCrossRecordValidationError {
    /// One operation, layer, field, Entity reference, or Perspective right was denied.
    Authorization(WriteAuthorizationError),
    /// A Provenance endpoint is absent from the complete post-transaction inventory.
    MissingProvenanceEndpoint(ProvenanceEndpointRef),
    /// EventRelation endpoint, lifecycle, uniqueness, or graph validation failed.
    EventGraph(EventGraphError),
    /// Provenance lifecycle, uniqueness, cycle, or budget validation failed.
    ProvenanceGraph(ProvenanceGraphError),
    /// Source, Evidence, target, retraction, or active-tuple validation failed.
    SourceEvidence(SourceEvidenceError),
}

impl fmt::Display for WriteCrossRecordValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authorization(error) => write!(formatter, "{error}"),
            Self::MissingProvenanceEndpoint(endpoint) => {
                write!(formatter, "Provenance endpoint {endpoint:?} is absent")
            }
            Self::EventGraph(error) => write!(formatter, "{error}"),
            Self::ProvenanceGraph(error) => write!(formatter, "{error}"),
            Self::SourceEvidence(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for WriteCrossRecordValidationError {}
