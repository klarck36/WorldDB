//! Authorized, schema-checked, WAL-backed factual-record writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use worlddb_core::{
    AggregateError, AggregateResult, AggregateSpec, ArchiveAction, ArchiveHistoryReferenceModel,
    ArchiveState, ArchiveTargetRecord, ArchiveTargetRef, ArchiveTransition, ArchiveTransitionId,
    Assertion, AssertionCorrectionCommand, AssertionDraft, AssertionHistoryRecord, AssertionId,
    AssertionPointIndexAccess, AssertionPointRequest, AssertionQueryStore, AssertionRetraction,
    AssertionRetractionId, AssertionValidity, AuditAction, AuditCommitContext, AuditObjectClass,
    AuditOutcome, AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity,
    AuthorizationDecision, AuthorizedAssertionMaskHistory, Bytes, CancellationToken, Capability,
    ContextKey, ContextPrecedence, CursorStateStore, CursorToken, DomainId, EntityTypeConstraint,
    EpistemicMode, Event, EventDraft, EventId, EventKindId, EventMask, EventMaskId,
    EventMaskRetraction, EventMaskRetractionId, EventRelation, EventRelationBatch,
    EventRelationHistory, EventRelationId, EventRelationInputKind, EventRelationKind,
    EventRelationRetraction, EventRelationRetractionId, EventRetraction, EventRetractionId,
    EventSpanClosure, EventSpanClosureId, FieldSelector, FullScanBudget, GraphCandidateSet,
    GraphEdge, GraphNode, GraphRelationshipKind, GraphResult, GraphSpec, GroupValueKey,
    HistoricalQueryBinding, HistorySpaceId, HistorySpaceReferenceModel, HistorySpaceView,
    LayerSelection, Lifecycle, Mask, MaskId, MaskRetraction, MaskRetractionId, MaskSelector,
    OperationId, PageExecution, PageOperation, PageRequest, PerspectiveScope,
    PolicyEventRelationKind, PolicyTarget, PredicateId, ProductiveQueryEngine, ProvenanceId,
    ProvenanceRelationship, QueryBudget, QueryBudgetLimits, QueryContext, QueryContextInput,
    QueryEngineError, QueryEngineOutput, QueryHash, Record, RecordRef, RecordedAsOf,
    ReferenceExplain, RelationshipSelector, ReplacementBoundary, ReplacementBoundaryId,
    ReplacementBoundaryRetraction, ReplacementBoundaryRetractionId, ReplacementBoundarySource,
    ResolutionPreview, ResolvedAggregateRow, Revision, SchemaDefinition, SchemaMode,
    SchemaRevision, SearchDocument, SearchHit, SearchMatch, SearchSpec, SearchTextField,
    SearchToken, SecurityContext, SecurityPolicyVersion, SnapshotBinding, SnapshotBindingInput,
    SnapshotLifetimeLimits, SnapshotPinPurpose, SnapshotRef, SnapshotRegistry,
    SnapshotSecurityBinding, Subject, ValidatedLayerSelection, Value, ValueKind, WorldTimeSelector,
    WriteReferenceSnapshot, encode_record, prepare_assertion_correction, prepare_event_correction,
    validate_assertion_batch, validate_event_graph_transaction, validate_value_for_predicate,
    validate_write_references,
};

use crate::{
    DatabaseLayout, HistorySegmentStore, ManifestSegmentKind, ManifestSegmentReference,
    RecoveryManager, SecurityPolicyHistoryStore, WalOperationStatus, WalPrepareLog, WriterLock,
};

use super::{
    FileProjectMetadataManager, FileSchemaManager, SchemaManagementError, cleanup_staged_schema,
    next_audit_sequence,
};

mod meta_history;
pub use meta_history::{SourceDraft, SourceSupersessionDraft};

/// Receipt for one committed immutable factual record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FactPublicationReceipt {
    operation_id: OperationId,
    revision: Revision,
    record_ref: RecordRef,
}

impl FactPublicationReceipt {
    /// Stable idempotency identity supplied for this write.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Shared database revision assigned to the write.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Typed identity of the newly committed record.
    #[must_use]
    pub const fn record_ref(self) -> RecordRef {
        self.record_ref
    }
}

/// Receipt for one atomic Assertion correction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssertionCorrectionReceipt {
    operation_id: OperationId,
    revision: Revision,
    target: AssertionId,
    replacement: AssertionId,
    retraction: AssertionRetractionId,
    corrects: ProvenanceId,
}

impl AssertionCorrectionReceipt {
    /// Idempotency identity of the correction operation.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Shared commit revision of all three correction records.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Original Assertion explicitly retracted by this correction.
    #[must_use]
    pub const fn target(self) -> AssertionId {
        self.target
    }

    /// New immutable replacement Assertion.
    #[must_use]
    pub const fn replacement(self) -> AssertionId {
        self.replacement
    }

    /// Explicit Retraction record for the original Assertion.
    #[must_use]
    pub const fn retraction(self) -> AssertionRetractionId {
        self.retraction
    }

    /// Explanatory `Corrects(replacement, target)` Provenance record.
    #[must_use]
    pub const fn corrects(self) -> ProvenanceId {
        self.corrects
    }
}

/// Receipt for one atomic Event correction. It deliberately has no EventRetraction ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventCorrectionReceipt {
    operation_id: OperationId,
    revision: Revision,
    target: EventId,
    replacement: EventId,
    corrects: ProvenanceId,
}

/// Durable status of one logical factual-record operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactOperationStatus {
    /// No durable prepare for this identity exists.
    NotCommitted,
    /// The operation committed at the returned shared revision.
    Committed(Revision),
    /// A prepare exists but recovery has not established its outcome.
    Indeterminate,
}

impl EventCorrectionReceipt {
    /// Idempotency identity of the correction operation.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }
    /// Shared commit revision of the new Event and Corrects edge.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }
    /// Original Event, which remains active after this correction.
    #[must_use]
    pub const fn target(self) -> EventId {
        self.target
    }
    /// New immutable replacement Event.
    #[must_use]
    pub const fn replacement(self) -> EventId {
        self.replacement
    }
    /// Explanatory `Corrects(replacement, target)` Provenance record.
    #[must_use]
    pub const fn corrects(self) -> ProvenanceId {
        self.corrects
    }
}

/// The identity and creation revision of one Event visible in a factual snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventSnapshotRef {
    event_id: EventId,
    created_revision: Revision,
    history_space_id: HistorySpaceId,
    layer_id: worlddb_core::LayerId,
    event_kind_id: Option<EventKindId>,
}

impl EventSnapshotRef {
    /// Stable identity of the visible Event.
    #[must_use]
    pub const fn event_id(self) -> EventId {
        self.event_id
    }
    /// Revision that created the visible Event.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
    /// Owning HistorySpace of the visible Event.
    #[must_use]
    pub const fn history_space_id(self) -> HistorySpaceId {
        self.history_space_id
    }
    /// Explicit Layer of the visible Event.
    #[must_use]
    pub const fn layer_id(self) -> worlddb_core::LayerId {
        self.layer_id
    }
    /// EventKind only when its field-level read grant is present.
    #[must_use]
    pub const fn event_kind_id(self) -> Option<EventKindId> {
        self.event_kind_id
    }
}

/// Typed input fields for one new ReplacementBoundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplacementBoundaryDraft {
    context: ContextKey,
    subject: Subject,
    predicate_id: PredicateId,
    validity: Option<AssertionValidity>,
}

impl ReplacementBoundaryDraft {
    /// Creates an explicit boundary input without assigning storage identity
    /// or Transaction-Time revision.
    #[must_use]
    pub const fn new(
        context: ContextKey,
        subject: Subject,
        predicate_id: PredicateId,
        validity: Option<AssertionValidity>,
    ) -> Self {
        Self {
            context,
            subject,
            predicate_id,
            validity,
        }
    }

    /// Context in which the replacement set is authoritative.
    #[must_use]
    pub const fn context(self) -> ContextKey {
        self.context
    }

    /// Selected Entity subject.
    #[must_use]
    pub const fn subject(self) -> Subject {
        self.subject
    }

    /// Stable Predicate identity.
    #[must_use]
    pub const fn predicate_id(self) -> PredicateId {
        self.predicate_id
    }

    /// Optional world-time validity interval.
    #[must_use]
    pub const fn validity(self) -> Option<AssertionValidity> {
        self.validity
    }
}

/// Authorized factual records visible at one committed revision.
#[derive(Clone, Debug)]
pub struct FactSnapshot {
    revision: Revision,
    assertions: Vec<Assertion>,
    assertion_retractions: Vec<AssertionRetraction>,
    masks: Vec<Mask>,
    mask_retractions: Vec<MaskRetraction>,
    replacement_boundaries: Vec<ReplacementBoundary>,
    replacement_boundary_retractions: Vec<ReplacementBoundaryRetraction>,
    events: Vec<EventSnapshotRef>,
    event_retractions: Vec<EventRetraction>,
    event_span_closures: Vec<EventSpanClosure>,
    event_masks: Vec<EventMask>,
    event_mask_retractions: Vec<EventMaskRetraction>,
    event_relations: Vec<EventRelation>,
    event_relation_retractions: Vec<EventRelationRetraction>,
    archive_transitions: Vec<ArchiveTransition>,
    sources: Vec<worlddb_core::Source>,
    evidence: Vec<worlddb_core::Evidence>,
    evidence_retractions: Vec<worlddb_core::EvidenceRetraction>,
    provenance: Vec<worlddb_core::ProvenanceEdge>,
    provenance_retractions: Vec<worlddb_core::ProvenanceRetraction>,
    provenance_endpoints: Vec<worlddb_core::ProvenanceEndpointRef>,
    lifecycle_visible: bool,
}

/// Complete, explicit axes for one authorized Assertion resolution preview.
#[derive(Clone, Debug)]
pub struct FactResolutionPreviewRequest {
    /// HistorySpace whose immutable ancestry is queried.
    pub history_space_id: HistorySpaceId,
    /// Selected query layers, validated against the pinned Layer schema.
    pub layer_selection: LayerSelection,
    /// Assertion subject and Predicate slot.
    pub subject: Subject,
    /// Predicate identity for the selected slot.
    pub predicate_id: PredicateId,
    /// Perspective and epistemic partition.
    pub perspective_scope: PerspectiveScope,
    /// Epistemic partition paired with `perspective_scope`.
    pub epistemic_mode: EpistemicMode,
    /// Point or complete all-times selector.
    pub world_time: WorldTimeSelector,
}

/// One user-visible query mode for a selected Assertion slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactQueryOperation {
    /// Enumerate authorized raw Assertion, Mask, and Boundary history.
    History,
    /// Resolve the selected slot at a point or over all times.
    Resolved,
    /// Explain the selected point resolution, including applied Masks and Boundaries.
    Explain,
    /// Count distinct visible resolved contributors.
    AggregateCount,
    /// Test whether any visible resolved contributor exists.
    AggregateExists,
    /// Count resolved contributors grouped by their visible Assertion polarity.
    AggregateByPolarity,
}

/// Complete semantic inputs for an explicit factual query.
#[derive(Clone, Debug)]
pub struct FactQueryRequest {
    /// Recorded-data revision; the query snapshot itself is pinned at the live head.
    pub recorded_as_of: Revision,
    /// Schema interpretation used to validate query inputs and resolve values.
    pub schema_mode: SchemaMode,
    /// Requested operation.
    pub operation: FactQueryOperation,
    /// HistorySpace whose immutable ancestry is queried.
    pub history_space_id: HistorySpaceId,
    /// Selected query layers, validated against the selected schema.
    pub layer_selection: LayerSelection,
    /// Assertion subject and Predicate slot.
    pub subject: Subject,
    /// Predicate identity for the selected slot.
    pub predicate_id: PredicateId,
    /// Perspective and epistemic partition.
    pub perspective_scope: PerspectiveScope,
    /// Epistemic partition paired with `perspective_scope`.
    pub epistemic_mode: EpistemicMode,
    /// Point or complete all-times selector.
    pub world_time: WorldTimeSelector,
    /// Maximum authorized candidates to inspect.
    pub max_candidates: u64,
    /// Maximum deterministic work units.
    pub max_work_units: u64,
    /// Maximum complete result rows.
    pub max_results: u64,
}

/// Shared, fully typed context for the M8-18 search, graph, and aggregate explorer.
#[derive(Clone, Debug)]
pub struct FactExplorerRequest {
    /// Transaction-time read point.
    pub recorded_as_of: Revision,
    /// Schema interpretation used to validate the selected Predicate and Layers.
    pub schema_mode: SchemaMode,
    /// Selected HistorySpace and immutable ancestry.
    pub history_space_id: HistorySpaceId,
    /// Schema-validated query Layers.
    pub layer_selection: LayerSelection,
    /// Subject of the selected Assertion slot.
    pub subject: Subject,
    /// Predicate of the selected Assertion slot.
    pub predicate_id: PredicateId,
    /// Perspective scope.
    pub perspective_scope: PerspectiveScope,
    /// Epistemic partition.
    pub epistemic_mode: EpistemicMode,
    /// Point or all-times selection.
    pub world_time: WorldTimeSelector,
    /// Visible-candidate budget.
    pub max_candidates: u64,
    /// Deterministic-work budget.
    pub max_work_units: u64,
    /// Complete-result budget.
    pub max_results: u64,
}

/// One complete, exact-token search request with bounded page size.
#[derive(Clone, Debug)]
pub struct FactTokenSearchRequest {
    /// Shared context and finite work budgets.
    pub query: FactExplorerRequest,
    /// Exact UTF-8 terms split from the user's input.
    pub terms: Vec<String>,
    /// Whether every term or any term must match.
    pub matching: SearchMatch,
    /// Positive requested page size.
    pub page_size: u32,
    /// Canonical identity derived by the trusted engine host.
    pub query_hash: QueryHash,
}

/// Server-owned continuation session; its data and snapshot lease never cross IPC.
#[derive(Debug)]
pub struct FactTokenSearchSession {
    query_hash: QueryHash,
    context: QueryContext,
    results: QueryEngineOutput<Vec<SearchHit>>,
    coordinates: BTreeMap<RecordRef, (HistorySpaceId, worlddb_core::LayerId)>,
    field: FieldSelector,
    page_size: u32,
    expires_at_ms: u64,
    snapshot_revision: Revision,
    schema_revision: SchemaRevision,
    resolved_layers: Vec<worlddb_core::LayerId>,
}

impl FactTokenSearchSession {
    /// Query identity used only as an in-process session map key.
    #[must_use]
    pub const fn query_hash(&self) -> QueryHash {
        self.query_hash
    }

    /// Fixed cursor expiry shared by every page in this search session.
    #[must_use]
    pub const fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
}

/// One complete page plus the query binding needed to label it in the UI.
#[derive(Clone, Debug)]
pub struct FactTokenSearchPage {
    /// Live database head fixed for this query.
    pub snapshot_revision: Revision,
    /// Schema revision bound to the query.
    pub schema_revision: SchemaRevision,
    /// Concrete Layers selected by the query.
    pub resolved_layers: Vec<worlddb_core::LayerId>,
    /// Visible search hits without snippets.
    pub hits: Vec<SearchHit>,
    /// Opaque, host-session-bound continuation.
    pub next_cursor: Option<Vec<u8>>,
    /// Expiry of the opaque continuation, if present.
    pub cursor_expires_at_ms: Option<u64>,
}

/// Complete bounded graph traversal with the query axes displayed to the caller.
#[derive(Clone, Debug)]
pub struct FactGraphExecution {
    /// Live database head fixed for the graph read.
    pub snapshot_revision: Revision,
    /// Schema revision bound to the query.
    pub schema_revision: SchemaRevision,
    /// Concrete Layers selected by the query.
    pub resolved_layers: Vec<worlddb_core::LayerId>,
    /// Complete authorized traversal; budget failures contain no partial graph.
    pub result: QueryEngineOutput<GraphResult>,
}

struct PreparedExplorerQuery {
    context: QueryContext,
    facts: FactSnapshot,
    metadata: worlddb_core::ProjectMetadataSnapshot,
    history_spaces: worlddb_core::HistorySpaceCatalog,
    snapshot_revision: Revision,
    schema_revision: SchemaRevision,
    resolved_layers: Vec<worlddb_core::LayerId>,
}

fn history_space_view(
    catalog: &worlddb_core::HistorySpaceCatalog,
    selected: HistorySpaceId,
) -> Result<HistorySpaceView, FactManagementError> {
    let mut ancestry = Vec::new();
    let mut current = Some(selected);
    while let Some(history_space_id) = current {
        let definition =
            catalog
                .definition(history_space_id)
                .ok_or(FactManagementError::InvalidCandidate(
                    "the selected HistorySpace is unavailable",
                ))?;
        ancestry.push(definition);
        current = definition.parent_history_space_id();
    }
    ancestry.reverse();
    HistorySpaceView::new(ancestry).map_err(|error| FactManagementError::Query(error.to_string()))
}

fn history_space_contains_record(
    catalog: &worlddb_core::HistorySpaceCatalog,
    selected: HistorySpaceId,
    record_space: HistorySpaceId,
    record_revision: Revision,
    as_of: Revision,
) -> bool {
    let mut current = selected;
    let mut cutoff = as_of;
    loop {
        let Some(definition) = catalog.definition(current) else {
            return false;
        };
        if current == record_space {
            return record_revision <= cutoff;
        }
        let Some(parent) = definition.parent_history_space_id() else {
            return false;
        };
        cutoff = cutoff.min(definition.base_revision());
        current = parent;
    }
}

fn token_search_sort_key(hit: &SearchHit) -> Vec<u8> {
    match hit.result_key() {
        RecordRef::Assertion(assertion_id) => assertion_id.to_bytes().to_vec(),
        _ => Vec::new(),
    }
}

fn provenance_endpoint_record_ref(endpoint: worlddb_core::ProvenanceEndpointRef) -> RecordRef {
    use worlddb_core::ProvenanceEndpointRef as Endpoint;
    match endpoint {
        Endpoint::Assertion(id) => RecordRef::Assertion(id),
        Endpoint::Mask(id) => RecordRef::Mask(id),
        Endpoint::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        Endpoint::Event(id) => RecordRef::Event(id),
        Endpoint::EventMask(id) => RecordRef::EventMask(id),
        Endpoint::Source(id) => RecordRef::Source(id),
        Endpoint::Evidence(id) => RecordRef::Evidence(id),
        Endpoint::Provenance(id) => RecordRef::Provenance(id),
        Endpoint::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        Endpoint::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        Endpoint::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        Endpoint::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        Endpoint::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        Endpoint::ReplacementBoundaryRetraction(id) => RecordRef::ReplacementBoundaryRetraction(id),
        Endpoint::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        Endpoint::EventRetraction(id) => RecordRef::EventRetraction(id),
        Endpoint::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        Endpoint::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        Endpoint::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        Endpoint::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        Endpoint::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        Endpoint::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        Endpoint::ArchiveTransition(id) => RecordRef::ArchiveTransition(id),
    }
}

/// Output variants from a user-visible factual query.
#[derive(Debug)]
pub enum FactQueryResult {
    /// Authorized raw records from the selected slot's HistorySpace ancestry.
    History(Vec<FactQueryHistoryRow>),
    /// Productive resolution result.
    Resolved(QueryEngineOutput<ResolutionPreview>),
    /// Productive, visibility-checked Explain trace.
    Explain(QueryEngineOutput<ReferenceExplain>),
    /// Complete count, existence, or grouped count over resolved contributors.
    Aggregate(QueryEngineOutput<AggregateResult>),
}

/// One authorized owned raw-history record and its recorded coordinates.
#[derive(Clone, Debug)]
pub struct FactQueryHistoryRow {
    /// Immutable record family and identity.
    pub record_ref: RecordRef,
    /// Revision that created this record.
    pub recorded_revision: Revision,
    /// HistorySpace that owns the record.
    pub owner_history_space_id: HistorySpaceId,
    /// Owned immutable record payload, limited to the query's selected slot.
    pub record: FactHistoryRecord,
}

/// Query binding details needed by the renderer to label every visible result.
#[derive(Debug)]
pub struct FactQueryExecution {
    /// Live database head pinned for this query.
    pub snapshot_revision: Revision,
    /// Selected schema revision.
    pub schema_revision: SchemaRevision,
    /// Concrete Layers resolved from the caller's selection.
    pub resolved_layers: Vec<worlddb_core::LayerId>,
    /// Query result in its selected semantic form.
    pub result: FactQueryResult,
}

#[derive(Clone, Debug)]
pub enum FactHistoryRecord {
    /// One immutable Assertion with its complete stored value and context.
    Assertion(Assertion),
    /// One explicit transaction-time Assertion retraction.
    AssertionRetraction(AssertionRetraction),
    /// One immutable Mask and its selector.
    Mask(Mask),
    /// One explicit transaction-time Mask retraction.
    MaskRetraction(MaskRetraction),
    /// One immutable replacement Boundary.
    ReplacementBoundary(ReplacementBoundary),
    /// One explicit transaction-time Boundary retraction.
    ReplacementBoundaryRetraction(ReplacementBoundaryRetraction),
}

impl FactHistoryRecord {
    fn created_revision(&self) -> Revision {
        match self {
            Self::Assertion(value) => value.created_revision(),
            Self::AssertionRetraction(value) => value.created_revision(),
            Self::Mask(value) => value.created_revision(),
            Self::MaskRetraction(value) => value.created_revision(),
            Self::ReplacementBoundary(value) => value.created_revision(),
            Self::ReplacementBoundaryRetraction(value) => value.created_revision(),
        }
    }

    fn record_ref(&self) -> RecordRef {
        match self {
            Self::Assertion(value) => RecordRef::Assertion(value.id()),
            Self::AssertionRetraction(value) => RecordRef::AssertionRetraction(value.id()),
            Self::Mask(value) => RecordRef::Mask(value.id()),
            Self::MaskRetraction(value) => RecordRef::MaskRetraction(value.id()),
            Self::ReplacementBoundary(value) => RecordRef::ReplacementBoundary(value.id()),
            Self::ReplacementBoundaryRetraction(value) => {
                RecordRef::ReplacementBoundaryRetraction(value.id())
            }
        }
    }

    fn context(&self, facts: &FactSnapshot) -> Option<worlddb_core::ContextKey> {
        match self {
            Self::Assertion(value) => Some(value.context()),
            Self::AssertionRetraction(value) => facts
                .assertions()
                .iter()
                .find(|assertion| assertion.id() == value.assertion_id())
                .map(Assertion::context),
            Self::Mask(value) => Some(value.context()),
            Self::MaskRetraction(value) => facts
                .masks()
                .iter()
                .find(|mask| mask.id() == value.mask_id())
                .map(Mask::context),
            Self::ReplacementBoundary(value) => Some(value.context()),
            Self::ReplacementBoundaryRetraction(value) => facts
                .replacement_boundaries()
                .iter()
                .find(|boundary| boundary.id() == value.replacement_boundary_id())
                .map(ReplacementBoundary::context),
        }
    }

    fn matches_slot(
        &self,
        facts: &FactSnapshot,
        request: &FactQueryRequest,
        resolved_layers: &[worlddb_core::LayerId],
        selected_assertions: &BTreeSet<AssertionId>,
    ) -> bool {
        let Some(context) = self.context(facts) else {
            return false;
        };
        if context.perspective_scope() != request.perspective_scope
            || context.epistemic_mode() != request.epistemic_mode
            || !resolved_layers.contains(&context.layer_id())
        {
            return false;
        }
        match self {
            Self::Assertion(value) => {
                value.subject() == request.subject && value.predicate_id() == request.predicate_id
            }
            Self::AssertionRetraction(value) => selected_assertions.contains(&value.assertion_id()),
            Self::Mask(value) => match value.selector() {
                MaskSelector::ExactAssertion(assertion_id) => {
                    selected_assertions.contains(assertion_id)
                }
                MaskSelector::Proposition(proposition) => {
                    proposition.subject() == request.subject
                        && proposition.predicate_id() == request.predicate_id
                }
                MaskSelector::Slot(slot) => {
                    slot.subject() == request.subject && slot.predicate_id() == request.predicate_id
                }
            },
            Self::MaskRetraction(value) => facts.masks().iter().any(|mask| {
                mask.id() == value.mask_id()
                    && FactHistoryRecord::Mask(mask.clone()).matches_slot(
                        facts,
                        request,
                        resolved_layers,
                        selected_assertions,
                    )
            }),
            Self::ReplacementBoundary(value) => {
                value.subject() == request.subject && value.predicate_id() == request.predicate_id
            }
            Self::ReplacementBoundaryRetraction(value) => facts
                .replacement_boundaries()
                .iter()
                .find(|boundary| boundary.id() == value.replacement_boundary_id())
                .is_some_and(|boundary| {
                    FactHistoryRecord::ReplacementBoundary(boundary.clone()).matches_slot(
                        facts,
                        request,
                        resolved_layers,
                        selected_assertions,
                    )
                }),
        }
    }
}

fn fact_history_model(
    facts: &FactSnapshot,
    history_spaces: Vec<worlddb_core::HistorySpaceDefinition>,
    latest_revision: Revision,
    request: &FactQueryRequest,
    resolved_layers: &[worlddb_core::LayerId],
) -> Result<HistorySpaceReferenceModel<FactHistoryRecord>, FactManagementError> {
    let selected_assertions = facts
        .assertions()
        .iter()
        .filter(|assertion| {
            let context = assertion.context();
            context.perspective_scope() == request.perspective_scope
                && context.epistemic_mode() == request.epistemic_mode
                && resolved_layers.contains(&context.layer_id())
                && assertion.subject() == request.subject
                && assertion.predicate_id() == request.predicate_id
        })
        .map(Assertion::id)
        .collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    candidates.extend(
        facts
            .assertions()
            .iter()
            .cloned()
            .map(FactHistoryRecord::Assertion),
    );
    candidates.extend(
        facts
            .assertion_retractions()
            .iter()
            .cloned()
            .map(FactHistoryRecord::AssertionRetraction),
    );
    candidates.extend(facts.masks().iter().cloned().map(FactHistoryRecord::Mask));
    candidates.extend(
        facts
            .mask_retractions()
            .iter()
            .cloned()
            .map(FactHistoryRecord::MaskRetraction),
    );
    candidates.extend(
        facts
            .replacement_boundaries()
            .iter()
            .cloned()
            .map(FactHistoryRecord::ReplacementBoundary),
    );
    candidates.extend(
        facts
            .replacement_boundary_retractions()
            .iter()
            .cloned()
            .map(FactHistoryRecord::ReplacementBoundaryRetraction),
    );

    let mut commits = BTreeMap::<Revision, Vec<(HistorySpaceId, FactHistoryRecord)>>::new();
    for record in candidates {
        if !record.matches_slot(facts, request, resolved_layers, &selected_assertions) {
            continue;
        }
        let Some(owner) = record
            .context(facts)
            .map(|context| context.history_space_id())
        else {
            continue;
        };
        commits
            .entry(record.created_revision())
            .or_default()
            .push((owner, record));
    }
    HistorySpaceReferenceModel::from_published_snapshot(
        history_spaces,
        latest_revision,
        commits.into_iter().collect(),
    )
    .map_err(|error| FactManagementError::Query(error.to_string()))
}

impl FactSnapshot {
    /// Revision represented by the snapshot.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Immutable Assertion records visible at this revision.
    #[must_use]
    pub fn assertions(&self) -> &[Assertion] {
        &self.assertions
    }

    /// Visible Transaction-Time retractions for the returned Assertions.
    #[must_use]
    pub fn assertion_retractions(&self) -> &[AssertionRetraction] {
        &self.assertion_retractions
    }

    /// Immutable Mask records visible at this revision.
    #[must_use]
    pub fn masks(&self) -> &[Mask] {
        &self.masks
    }

    /// Visible Transaction-Time retractions for the returned Masks.
    #[must_use]
    pub fn mask_retractions(&self) -> &[MaskRetraction] {
        &self.mask_retractions
    }

    /// Immutable ReplacementBoundary records visible at this revision.
    #[must_use]
    pub fn replacement_boundaries(&self) -> &[ReplacementBoundary] {
        &self.replacement_boundaries
    }

    /// Visible Transaction-Time retractions for the returned Boundaries.
    #[must_use]
    pub fn replacement_boundary_retractions(&self) -> &[ReplacementBoundaryRetraction] {
        &self.replacement_boundary_retractions
    }

    /// Visible Event identities and creation revisions.
    #[must_use]
    pub fn events(&self) -> &[EventSnapshotRef] {
        &self.events
    }

    /// Visible Transaction-Time retractions for Events in this snapshot.
    #[must_use]
    pub fn event_retractions(&self) -> &[EventRetraction] {
        &self.event_retractions
    }

    /// Visible span closures for returned Events.
    #[must_use]
    pub fn event_span_closures(&self) -> &[EventSpanClosure] {
        &self.event_span_closures
    }

    /// Immutable EventMasks whose concrete target Event is visible.
    #[must_use]
    pub fn event_masks(&self) -> &[EventMask] {
        &self.event_masks
    }

    /// Visible Transaction-Time retractions for returned EventMasks.
    #[must_use]
    pub fn event_mask_retractions(&self) -> &[EventMaskRetraction] {
        &self.event_mask_retractions
    }

    /// Canonical EventRelations whose endpoint Events are visible.
    #[must_use]
    pub fn event_relations(&self) -> &[EventRelation] {
        &self.event_relations
    }

    /// Visible Transaction-Time retractions for returned EventRelations.
    #[must_use]
    pub fn event_relation_retractions(&self) -> &[EventRelationRetraction] {
        &self.event_relation_retractions
    }

    /// Visible operational archive transitions for the returned factual records.
    #[must_use]
    pub fn archive_transitions(&self) -> &[ArchiveTransition] {
        &self.archive_transitions
    }

    /// Authorized project-wide Source records whose complete fields are readable.
    #[must_use]
    pub fn sources(&self) -> &[worlddb_core::Source] {
        &self.sources
    }

    /// Authorized historical Evidence links with both readable endpoints.
    #[must_use]
    pub fn evidence(&self) -> &[worlddb_core::Evidence] {
        &self.evidence
    }

    /// Lifecycle records for Evidence links returned by this snapshot.
    #[must_use]
    pub fn evidence_retractions(&self) -> &[worlddb_core::EvidenceRetraction] {
        &self.evidence_retractions
    }

    /// Authorized historical Provenance edges with both readable endpoints.
    #[must_use]
    pub fn provenance(&self) -> &[worlddb_core::ProvenanceEdge] {
        &self.provenance
    }

    /// Lifecycle records for Provenance edges returned by this snapshot.
    #[must_use]
    pub fn provenance_retractions(&self) -> &[worlddb_core::ProvenanceRetraction] {
        &self.provenance_retractions
    }

    /// Closed provenance endpoints that pass current endpoint and field authorization.
    #[must_use]
    pub fn provenance_endpoints(&self) -> &[worlddb_core::ProvenanceEndpointRef] {
        &self.provenance_endpoints
    }

    /// Whether lifecycle state was readable at this snapshot.
    #[must_use]
    pub const fn lifecycle_visible(&self) -> bool {
        self.lifecycle_visible
    }
}

/// File-backed factual-record manager.
///
/// Each write validates against one current schema/catalog snapshot and binds
/// the data record, policy-history advance, manifest, WAL receipt, and required
/// audit record to one shared commit point.
pub struct FileFactManager<'a> {
    schema: FileSchemaManager<'a>,
}

impl<'a> FileFactManager<'a> {
    /// Opens and verifies the database for the authenticated principal.
    pub fn open(
        layout: DatabaseLayout,
        writer_lock: &'a WriterLock,
        principal: worlddb_core::PrincipalId,
    ) -> Result<Self, FactManagementError> {
        let schema =
            FileSchemaManager::open(layout, writer_lock, principal).map_err(map_schema_error)?;
        Ok(Self { schema })
    }

    /// Current committed shared revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.schema.revision()
    }

    /// Reads the durable status of an OperationId from the verified WAL.
    pub fn operation_status(
        &self,
        operation_id: OperationId,
    ) -> Result<FactOperationStatus, FactManagementError> {
        let status = WalPrepareLog::new(&self.schema.layout)
            .operation_status(self.schema.writer_lock, operation_id)
            .map_err(storage_error)?;
        Ok(match status {
            WalOperationStatus::NotCommitted => FactOperationStatus::NotCommitted,
            WalOperationStatus::Committed(receipt) => {
                FactOperationStatus::Committed(receipt.revision())
            }
            WalOperationStatus::Indeterminate => FactOperationStatus::Indeterminate,
        })
    }

    /// Creates one immutable Assertion after schema, context, reference, and
    /// current-policy validation.
    pub fn create_assertion(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        assertion_id: AssertionId,
        draft: AssertionDraft,
        allow_deprecated_schema: bool,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let context = draft.context();
        let target = context_target(context);
        self.authorize(
            Capability::AssertionCreate,
            field_target(context, FieldSelector::AssertionValue(draft.predicate_id())),
        )?;
        self.authorize(Capability::LayerWrite, target)?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(draft.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldWrite, field_target(context, field))?;
        }
        self.authorize(Capability::EntityReference, target)?;
        if matches!(
            context.perspective_scope(),
            worlddb_core::PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, target)?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_validity(
            metadata.schema(),
            Some(draft.validity()),
            allow_deprecated_schema,
        )?;
        let policy = self.policy_snapshot()?;
        let schema_assertions = validate_assertion_batch(
            vec![draft.clone()],
            metadata.schema(),
            policy,
            self.schema.principal,
            allow_deprecated_schema,
            worlddb_core::DecoderLimits::default(),
            |time| {
                metadata
                    .schema()
                    .resolve_time(time, allow_deprecated_schema)
                    .map_err(|_| worlddb_core::TemporalError::Overflow)
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let references = validate_write_references(
            vec![draft.clone()],
            vec![],
            &WriteReferenceSnapshot {
                history_spaces: metadata.history_spaces(),
                layers: metadata.layers(),
                entities: metadata.entities(),
                perspectives: metadata.perspectives(),
                schema: metadata.schema(),
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_validated_write_batch(&references, policy, self.schema.principal)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_deprecated_schema_warnings(
            &references,
            &schema_assertions,
            policy,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;

        self.ensure_identity_available(RecordRef::Assertion(assertion_id))?;
        let revision = self.next_revision()?;
        let assertion = Assertion::new(assertion_id, draft, revision);
        self.publish(
            expected_base,
            operation_id,
            Record::Assertion(assertion),
            RecordRef::Assertion(assertion_id),
            target,
        )
    }

    /// Corrects one visible Assertion as an atomic replacement, explicit
    /// Retraction, and `Corrects(replacement, target)` Provenance edge.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    pub fn correct_assertion(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        target_id: AssertionId,
        expected_target_created_revision: Revision,
        replacement_id: AssertionId,
        retraction_id: AssertionRetractionId,
        corrects_id: ProvenanceId,
        replacement: AssertionDraft,
        retraction_reason: String,
        allow_deprecated_schema: bool,
    ) -> Result<AssertionCorrectionReceipt, FactManagementError> {
        match self.operation_status(operation_id)? {
            FactOperationStatus::Committed(revision) => {
                return self.replay_assertion_correction(
                    expected_base,
                    operation_id,
                    revision,
                    target_id,
                    expected_target_created_revision,
                    replacement_id,
                    retraction_id,
                    corrects_id,
                    replacement,
                    retraction_reason,
                );
            }
            FactOperationStatus::Indeterminate => {
                return Err(FactManagementError::UnknownCommit(operation_id));
            }
            FactOperationStatus::NotCommitted => {}
        }
        self.check_base(expected_base)?;
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Assertion(assertion) if assertion.id() == target_id => {
                    Some(assertion.clone())
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Assertion is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::AssertionRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::AssertionCorrect, target_policy)?;
        self.authorize(Capability::AssertionCreate, target_policy)?;
        self.authorize(Capability::AssertionRetract, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target(
                target.context(),
                RecordRef::Assertion(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;

        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(target.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldRead, field_target(target.context(), field))
                .map_err(|_| {
                    FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
                })?;
        }
        let context = replacement.context();
        self.authorize(Capability::LayerWrite, context_target(context))?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(replacement.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldWrite, field_target(context, field))?;
        }
        self.authorize(Capability::EntityReference, context_target(context))?;
        if matches!(
            context.perspective_scope(),
            PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, context_target(context))?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_validity(
            metadata.schema(),
            Some(replacement.validity()),
            allow_deprecated_schema,
        )?;
        let policy = self.policy_snapshot()?;
        let schema_assertions = validate_assertion_batch(
            vec![replacement.clone()],
            metadata.schema(),
            policy,
            self.schema.principal,
            allow_deprecated_schema,
            worlddb_core::DecoderLimits::default(),
            |time| {
                metadata
                    .schema()
                    .resolve_time(time, allow_deprecated_schema)
                    .map_err(|_| worlddb_core::TemporalError::Overflow)
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let references = validate_write_references(
            vec![replacement.clone()],
            vec![],
            &WriteReferenceSnapshot {
                history_spaces: metadata.history_spaces(),
                layers: metadata.layers(),
                entities: metadata.entities(),
                perspectives: metadata.perspectives(),
                schema: metadata.schema(),
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_validated_write_batch(&references, policy, self.schema.principal)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_deprecated_schema_warnings(
            &references,
            &schema_assertions,
            policy,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;

        for identity in [
            RecordRef::Assertion(replacement_id),
            RecordRef::AssertionRetraction(retraction_id),
            RecordRef::Provenance(corrects_id),
        ] {
            self.ensure_identity_available(identity)?;
        }
        let retractions = existing_records
            .iter()
            .filter_map(|record| match record {
                Record::AssertionRetraction(value) => Some(value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let commit_revision = self.next_revision()?;
        let prepared = prepare_assertion_correction(
            &target,
            &retractions,
            RecordedAsOf::from_published_revision(expected_base),
            AssertionCorrectionCommand::new(
                expected_target_created_revision,
                replacement_id,
                replacement,
                retraction_id,
                retraction_reason,
                corrects_id,
                commit_revision,
            ),
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let receipt = AssertionCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: prepared.replacement().id(),
            retraction: prepared.retraction().id(),
            corrects: prepared.corrects().id(),
        };
        self.publish_batch(
            expected_base,
            operation_id,
            prepared.into_records().into_iter().collect(),
            RecordRef::Assertion(receipt.replacement),
            target_policy,
        )?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    fn replay_assertion_correction(
        &self,
        expected_base: Revision,
        operation_id: OperationId,
        commit_revision: Revision,
        target_id: AssertionId,
        expected_target_created_revision: Revision,
        replacement_id: AssertionId,
        retraction_id: AssertionRetractionId,
        corrects_id: ProvenanceId,
        replacement: AssertionDraft,
        retraction_reason: String,
    ) -> Result<AssertionCorrectionReceipt, FactManagementError> {
        if expected_base
            .next_commit()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?
            != commit_revision
        {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Assertion(assertion) if assertion.id() == target_id => {
                    Some(assertion.clone())
                }
                _ => None,
            })
            .ok_or(FactManagementError::IdempotencyMismatch)?;
        if target.created_revision() != expected_target_created_revision {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let target_policy = context_target(target.context());
        self.authorize(Capability::AssertionRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::AssertionCorrect, target_policy)?;
        self.authorize(Capability::AssertionCreate, target_policy)?;
        self.authorize(Capability::AssertionRetract, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target(
                target.context(),
                RecordRef::Assertion(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(target.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldRead, field_target(target.context(), field))
                .map_err(|_| {
                    FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
                })?;
        }
        let write_context = replacement.context();
        self.authorize(Capability::LayerWrite, context_target(write_context))?;
        for field in [
            FieldSelector::AssertionSubject,
            FieldSelector::AssertionPredicate,
            FieldSelector::AssertionValue(replacement.predicate_id()),
            FieldSelector::AssertionPolarity,
            FieldSelector::AssertionValidity,
            FieldSelector::AssertionPerspective,
            FieldSelector::AssertionEpistemicMode,
        ] {
            self.authorize(Capability::FieldWrite, field_target(write_context, field))?;
        }
        self.authorize(Capability::EntityReference, context_target(write_context))?;
        if matches!(
            write_context.perspective_scope(),
            PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, context_target(write_context))?;
        }

        let retractions = existing_records
            .iter()
            .filter_map(|record| match record {
                Record::AssertionRetraction(value) => Some(value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let prepared = prepare_assertion_correction(
            &target,
            &retractions,
            RecordedAsOf::from_published_revision(expected_base),
            AssertionCorrectionCommand::new(
                expected_target_created_revision,
                replacement_id,
                replacement,
                retraction_id,
                retraction_reason,
                corrects_id,
                commit_revision,
            ),
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let records = prepared.into_records().into_iter().collect::<Vec<_>>();
        self.verify_replayed_records(commit_revision, &records)?;
        Ok(AssertionCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: replacement_id,
            retraction: retraction_id,
            corrects: corrects_id,
        })
    }

    /// Creates one immutable Event after schema, reference, and policy validation.
    pub fn create_event(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        event_id: EventId,
        draft: EventDraft,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let history_space = draft.history_space_id();
        let layer = draft.layer_id();
        let target = context_target_parts(history_space, layer);
        self.authorize(Capability::EventCreate, target)?;
        self.authorize(Capability::LayerWrite, target)?;
        self.authorize(Capability::EntityReference, target)?;
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(history_space, layer, FieldSelector::EventKind),
        )?;
        for participant in draft.participants().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    history_space,
                    layer,
                    FieldSelector::EventParticipant(draft.event_kind_id(), participant.role_id()),
                ),
            )?;
        }
        for attribute in draft.attributes().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    history_space,
                    layer,
                    FieldSelector::EventAttribute(draft.event_kind_id(), attribute.attribute_id()),
                ),
            )?;
        }
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(
                history_space,
                layer,
                FieldSelector::EventTime(draft.event_kind_id()),
            ),
        )?;

        let metadata = self.write_validation_metadata()?;
        if metadata.revision() != expected_base {
            return Err(FactManagementError::Conflict);
        }
        let references = validate_write_references(
            vec![],
            vec![draft.clone()],
            &WriteReferenceSnapshot {
                history_spaces: metadata.history_spaces(),
                layers: metadata.layers(),
                entities: metadata.entities(),
                perspectives: metadata.perspectives(),
                schema: metadata.schema(),
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_validated_write_batch(
            &references,
            self.policy_snapshot()?,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;

        self.ensure_identity_available(RecordRef::Event(event_id))?;
        let revision = self.next_revision()?;
        let event = Event::new(event_id, draft, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let event_target = event_target(&event);
        self.publish(
            expected_base,
            operation_id,
            Record::Event(event),
            RecordRef::Event(event_id),
            event_target,
        )
    }

    /// Corrects one Event by committing a new Event and `Corrects(new, old)`.
    /// The old Event stays active; no EventRetraction is created here.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    pub fn correct_event(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        target_id: EventId,
        expected_target_created_revision: Revision,
        replacement_id: EventId,
        corrects_id: ProvenanceId,
        replacement: EventDraft,
    ) -> Result<EventCorrectionReceipt, FactManagementError> {
        match self.operation_status(operation_id)? {
            FactOperationStatus::Committed(revision) => {
                return self.replay_event_correction(
                    expected_base,
                    operation_id,
                    revision,
                    target_id,
                    expected_target_created_revision,
                    replacement_id,
                    corrects_id,
                    replacement,
                );
            }
            FactOperationStatus::Indeterminate => {
                return Err(FactManagementError::UnknownCommit(operation_id));
            }
            FactOperationStatus::NotCommitted => {}
        }
        self.check_base(expected_base)?;
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Event(event) if event.id() == target_id => Some(event.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Event is unavailable",
            ))?;
        let target_policy = event_target(&target);
        self.authorize(Capability::EventRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(
            Capability::HistorySpaceRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(
            Capability::LayerRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        if target.created_revision() != expected_target_created_revision {
            return Err(FactManagementError::Conflict);
        }
        self.authorize(Capability::EventCorrect, target_policy)?;
        self.authorize(Capability::EventCreate, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(
            Capability::LayerWrite,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;
        self.authorize(
            Capability::FieldRead,
            record_field_target(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                FieldSelector::EventKind,
            ),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        let target_retracted = existing_records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_id)
        });
        if target_retracted {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event is already retracted",
            ));
        }

        let context = (replacement.history_space_id(), replacement.layer_id());
        let write_target = context_target_parts(context.0, context.1);
        self.authorize(Capability::LayerWrite, write_target)?;
        self.authorize(Capability::EntityReference, write_target)?;
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(context.0, context.1, FieldSelector::EventKind),
        )?;
        for participant in replacement.participants().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    context.0,
                    context.1,
                    FieldSelector::EventParticipant(
                        replacement.event_kind_id(),
                        participant.role_id(),
                    ),
                ),
            )?;
        }
        for attribute in replacement.attributes().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    context.0,
                    context.1,
                    FieldSelector::EventAttribute(
                        replacement.event_kind_id(),
                        attribute.attribute_id(),
                    ),
                ),
            )?;
        }
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(
                context.0,
                context.1,
                FieldSelector::EventTime(replacement.event_kind_id()),
            ),
        )?;

        let metadata = self.write_validation_metadata()?;
        if metadata.revision() != expected_base {
            return Err(FactManagementError::Conflict);
        }
        let references = validate_write_references(
            vec![],
            vec![replacement.clone()],
            &WriteReferenceSnapshot {
                history_spaces: metadata.history_spaces(),
                layers: metadata.layers(),
                entities: metadata.entities(),
                perspectives: metadata.perspectives(),
                schema: metadata.schema(),
            },
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::authorize_validated_write_batch(
            &references,
            self.policy_snapshot()?,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;

        self.ensure_identity_available(RecordRef::Event(replacement_id))?;
        self.ensure_identity_available(RecordRef::Provenance(corrects_id))?;
        let commit_revision = self.next_revision()?;
        let (replacement, edge, correction) = prepare_event_correction(
            &target,
            replacement_id,
            replacement,
            corrects_id,
            commit_revision,
            |target_kind, replacement_kind| target_kind == replacement_kind,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let receipt = EventCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: correction.replacement_id(),
            corrects: correction.corrects_id(),
        };
        self.publish_batch(
            expected_base,
            operation_id,
            vec![Record::Event(replacement), Record::Provenance(edge)],
            RecordRef::Event(receipt.replacement),
            target_policy,
        )?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0008")]
    fn replay_event_correction(
        &self,
        expected_base: Revision,
        operation_id: OperationId,
        commit_revision: Revision,
        target_id: EventId,
        expected_target_created_revision: Revision,
        replacement_id: EventId,
        corrects_id: ProvenanceId,
        replacement: EventDraft,
    ) -> Result<EventCorrectionReceipt, FactManagementError> {
        if expected_base
            .next_commit()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?
            != commit_revision
        {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let existing_records = self.records_at_revision(expected_base)?;
        let target = existing_records
            .iter()
            .find_map(|record| match record {
                Record::Event(event) if event.id() == target_id => Some(event.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::IdempotencyMismatch)?;
        if target.created_revision() != expected_target_created_revision {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let target_policy = event_target(&target);
        self.authorize(Capability::EventRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        let owning_context = context_target_parts(target.history_space_id(), target.layer_id());
        self.authorize(Capability::HistorySpaceRead, owning_context)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(Capability::LayerRead, owning_context)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(Capability::EventCorrect, target_policy)?;
        self.authorize(Capability::EventCreate, target_policy)?;
        self.authorize(Capability::ProvenanceCreate, target_policy)?;
        self.authorize(Capability::LayerWrite, owning_context)?;
        self.authorize(
            Capability::RelationshipCreate,
            relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                ProvenanceRelationship::Corrects,
            ),
        )?;
        self.authorize(
            Capability::FieldRead,
            record_field_target(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
                FieldSelector::EventKind,
            ),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        if existing_records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_id)
        }) {
            return Err(FactManagementError::IdempotencyMismatch);
        }
        let write_context =
            context_target_parts(replacement.history_space_id(), replacement.layer_id());
        self.authorize(Capability::LayerWrite, write_context)?;
        self.authorize(Capability::EntityReference, write_context)?;
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(
                replacement.history_space_id(),
                replacement.layer_id(),
                FieldSelector::EventKind,
            ),
        )?;
        for participant in replacement.participants().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    replacement.history_space_id(),
                    replacement.layer_id(),
                    FieldSelector::EventParticipant(
                        replacement.event_kind_id(),
                        participant.role_id(),
                    ),
                ),
            )?;
        }
        for attribute in replacement.attributes().as_slice() {
            self.authorize(
                Capability::FieldWrite,
                field_target_parts(
                    replacement.history_space_id(),
                    replacement.layer_id(),
                    FieldSelector::EventAttribute(
                        replacement.event_kind_id(),
                        attribute.attribute_id(),
                    ),
                ),
            )?;
        }
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(
                replacement.history_space_id(),
                replacement.layer_id(),
                FieldSelector::EventTime(replacement.event_kind_id()),
            ),
        )?;
        let (event, edge, correction) = prepare_event_correction(
            &target,
            replacement_id,
            replacement,
            corrects_id,
            commit_revision,
            |target_kind, replacement_kind| target_kind == replacement_kind,
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.verify_replayed_records(
            commit_revision,
            &[Record::Event(event), Record::Provenance(edge)],
        )?;
        Ok(EventCorrectionReceipt {
            operation_id,
            revision: commit_revision,
            target: target_id,
            replacement: correction.replacement_id(),
            corrects: correction.corrects_id(),
        })
    }

    /// Creates one immutable Mask. Proposition selectors receive the same
    /// schema and Entity-reference checks as their matching Assertion value.
    pub fn create_mask(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        mask_id: MaskId,
        context: ContextKey,
        selector: MaskSelector,
        validity: Option<AssertionValidity>,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let target = context_target(context);
        self.authorize(Capability::MaskCreate, target)?;
        self.authorize(Capability::LayerWrite, target)?;
        self.authorize(
            Capability::FieldWrite,
            field_target(context, FieldSelector::MaskSelector),
        )?;
        self.authorize(
            Capability::FieldWrite,
            field_target(context, FieldSelector::MaskValidity),
        )?;
        self.authorize(Capability::EntityReference, target)?;
        if matches!(
            context.perspective_scope(),
            worlddb_core::PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, target)?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_context(&metadata, context)?;
        validate_validity(metadata.schema(), validity, false)?;
        match &selector {
            MaskSelector::ExactAssertion(assertion_id) => {
                let assertion = self
                    .records_at_revision(self.revision())?
                    .into_iter()
                    .find_map(|record| match record {
                        Record::Assertion(assertion) if assertion.id() == *assertion_id => {
                            Some(assertion)
                        }
                        _ => None,
                    })
                    .ok_or(FactManagementError::InvalidCandidate(
                        "the selected Assertion is unavailable",
                    ))?;
                self.authorize(
                    Capability::AssertionRead,
                    context_target(assertion.context()),
                )?;
            }
            MaskSelector::Proposition(key) => {
                let predicate =
                    validate_subject_and_predicate(&metadata, key.subject(), key.predicate_id())?;
                validate_predicate_value(metadata.schema(), predicate, key.value())?;
                validate_entity_value(&metadata, predicate, key.value())?;
            }
            MaskSelector::Slot(slot) => {
                let predicate =
                    validate_subject_and_predicate(&metadata, slot.subject(), slot.predicate_id())?;
                if slot.perspective_scope() != context.perspective_scope()
                    || slot.epistemic_mode() != context.epistemic_mode()
                {
                    return Err(FactManagementError::InvalidCandidate(
                        "Mask slot and context partitions do not match",
                    ));
                }
                let _ = predicate;
            }
        }
        self.ensure_identity_available(RecordRef::Mask(mask_id))?;
        let revision = self.next_revision()?;
        let mask = Mask::new(mask_id, context, selector, validity, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::Mask(mask),
            RecordRef::Mask(mask_id),
            target,
        )
    }

    /// Creates one immutable MultiValueReplace boundary.
    pub fn create_replacement_boundary(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        boundary_id: ReplacementBoundaryId,
        draft: ReplacementBoundaryDraft,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let context = draft.context();
        let subject = draft.subject();
        let predicate_id = draft.predicate_id();
        let validity = draft.validity();
        let target = context_target(context);
        self.authorize(Capability::ReplacementBoundaryCreate, target)?;
        self.authorize(Capability::LayerWrite, target)?;
        for field in [
            FieldSelector::ReplacementBoundarySubject,
            FieldSelector::ReplacementBoundaryPredicate,
            FieldSelector::ReplacementBoundaryValidity,
        ] {
            self.authorize(Capability::FieldWrite, field_target(context, field))?;
        }
        self.authorize(Capability::EntityReference, target)?;
        if matches!(
            context.perspective_scope(),
            worlddb_core::PerspectiveScope::Perspective(_)
        ) {
            self.authorize(Capability::PerspectiveUse, target)?;
        }

        let metadata = self.write_validation_metadata()?;
        validate_context(&metadata, context)?;
        validate_validity(metadata.schema(), validity, false)?;
        let predicate = validate_subject_and_predicate(&metadata, subject, predicate_id)?;
        if predicate.resolution_policy() != worlddb_core::ResolutionPolicy::MultiValueReplace {
            return Err(FactManagementError::InvalidCandidate(
                "the Predicate does not use MultiValueReplace",
            ));
        }
        self.ensure_identity_available(RecordRef::ReplacementBoundary(boundary_id))?;
        let revision = self.next_revision()?;
        let boundary =
            ReplacementBoundary::new(boundary_id, context, subject, predicate, validity, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::ReplacementBoundary(boundary),
            RecordRef::ReplacementBoundary(boundary_id),
            target,
        )
    }

    /// Appends a separate explicit Transaction-Time EventRetraction.
    pub fn retract_event(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: EventRetractionId,
        target_id: EventId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::Event(value) if value.id() == target_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Event is unavailable",
            ))?;
        let target_policy = event_target(&target);
        self.authorize(Capability::EventRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(
            Capability::HistorySpaceRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(
            Capability::LayerRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::EventRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
            ),
        )?;
        if records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::EventRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction = EventRetraction::new(retraction_id, &target, reason, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::EventRetraction(retraction),
            RecordRef::EventRetraction(retraction_id),
            target_policy,
        )
    }

    /// Closes an Event span that was created without an end coordinate.
    pub fn close_event_span(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        closure_id: EventSpanClosureId,
        target_id: EventId,
        close_at: worlddb_core::WorldTime,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::Event(value) if value.id() == target_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Event is unavailable",
            ))?;
        let target_policy = event_target(&target);
        self.authorize(Capability::EventRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(
            Capability::HistorySpaceRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(
            Capability::LayerRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::EventSpanClose, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::Event(target_id),
            ),
        )?;
        if records.iter().any(|record| {
            matches!(record, Record::EventSpanClosure(value) if value.event_id() == target_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event span is already closed",
            ));
        }
        if records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::EventSpanClosure(closure_id))?;
        let revision = self.next_revision()?;
        let closure = EventSpanClosure::new(closure_id, &target, close_at, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::EventSpanClosure(closure),
            RecordRef::EventSpanClosure(closure_id),
            target_policy,
        )
    }

    /// Creates an EventMask only when its HistorySpace/Layer context strictly
    /// outranks the target Event.
    pub fn create_event_mask(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        mask_id: EventMaskId,
        history_space_id: HistorySpaceId,
        layer_id: worlddb_core::LayerId,
        target_event_id: EventId,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let event = records
            .iter()
            .find_map(|record| match record {
                Record::Event(value) if value.id() == target_event_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Event is unavailable",
            ))?;
        let event_target = event_target(&event);
        self.authorize(Capability::EventRead, event_target)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Event is unavailable")
            })?;
        self.authorize(
            Capability::HistorySpaceRead,
            context_target_parts(event.history_space_id(), event.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        self.authorize(
            Capability::LayerRead,
            context_target_parts(event.history_space_id(), event.layer_id()),
        )
        .map_err(|_| FactManagementError::InvalidCandidate("the selected Event is unavailable"))?;
        if records.iter().any(|record| {
            matches!(record, Record::EventRetraction(value) if value.event_id() == target_event_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event is already retracted",
            ));
        }
        let metadata = self.write_validation_metadata()?;
        if metadata.revision() != expected_base {
            return Err(FactManagementError::Conflict);
        }
        let layer =
            metadata
                .layers()
                .definition(layer_id)
                .ok_or(FactManagementError::InvalidCandidate(
                    "the selected EventMask Layer is unavailable",
                ))?;
        if layer.lifecycle() == Lifecycle::Retired {
            return Err(FactManagementError::InvalidCandidate(
                "EventMask requires an Active Layer",
            ));
        }
        let precedence = ContextPrecedence::compare_event_contexts(
            history_space_id,
            history_space_id,
            layer_id,
            event.history_space_id(),
            event.layer_id(),
            metadata.history_spaces(),
            metadata.layers(),
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        if precedence != std::cmp::Ordering::Greater {
            return Err(FactManagementError::Validation(
                "EventMask requires a strictly more specific HistorySpace/Layer context than its target Event".to_owned(),
            ));
        }
        let (archive_targets, archive_transitions) = archive_history_records(&records);
        let archive_history =
            ArchiveHistoryReferenceModel::new(archive_targets, archive_transitions)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        if archive_history
            .state_at(
                ArchiveTargetRef::Event(target_event_id),
                RecordedAsOf::from_published_revision(expected_base),
            )
            .map_err(|error| FactManagementError::Validation(error.to_string()))?
            == ArchiveState::Archived
        {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Event is archived",
            ));
        }
        let owner = context_target_parts(history_space_id, layer_id);
        self.authorize(Capability::EventMaskCreate, owner)?;
        self.authorize(Capability::LayerWrite, owner)?;
        self.authorize(
            Capability::FieldWrite,
            field_target_parts(history_space_id, layer_id, FieldSelector::EventMaskTarget),
        )?;
        let mask_policy = PolicyTarget::new(
            Some(history_space_id),
            Some(layer_id),
            Some(RecordRef::EventMask(mask_id)),
            None,
            None,
        );
        self.ensure_identity_available(RecordRef::EventMask(mask_id))?;
        let revision = self.next_revision()?;
        let mask = EventMask::new(
            mask_id,
            history_space_id,
            layer_id,
            target_event_id,
            revision,
        );
        self.publish(
            expected_base,
            operation_id,
            Record::EventMask(mask),
            RecordRef::EventMask(mask_id),
            mask_policy,
        )
    }

    /// Appends a separate explicit Transaction-Time EventMaskRetraction.
    pub fn retract_event_mask(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: EventMaskRetractionId,
        target_id: EventMaskId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::EventMask(value) if value.id() == target_id => Some(*value),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected EventMask is unavailable",
            ))?;
        let target_policy = event_mask_target(&target);
        self.authorize(Capability::EventMaskRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected EventMask is unavailable")
            })?;
        self.authorize(
            Capability::HistorySpaceRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| {
            FactManagementError::InvalidCandidate("the selected EventMask is unavailable")
        })?;
        self.authorize(
            Capability::LayerRead,
            context_target_parts(target.history_space_id(), target.layer_id()),
        )
        .map_err(|_| {
            FactManagementError::InvalidCandidate("the selected EventMask is unavailable")
        })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::EventMaskRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target_parts(
                target.history_space_id(),
                target.layer_id(),
                RecordRef::EventMask(target_id),
            ),
        )?;
        if records.iter().any(|record| {
            matches!(record, Record::EventMaskRetraction(value) if value.event_mask_id() == target_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected EventMask is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::EventMaskRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction = EventMaskRetraction::new(retraction_id, &target, reason, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::EventMaskRetraction(retraction),
            RecordRef::EventMaskRetraction(retraction_id),
            target_policy,
        )
    }

    /// Adds a canonical EventRelation after endpoint, authorization, and full
    /// post-transaction graph validation.
    pub fn create_event_relation(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        relation_id: EventRelationId,
        from_event_id: EventId,
        to_event_id: EventId,
        kind: EventRelationInputKind,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let events = records
            .iter()
            .filter_map(|record| match record {
                Record::Event(event) => Some(event.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for event_id in [from_event_id, to_event_id] {
            let event = events.iter().find(|event| event.id() == event_id).ok_or(
                FactManagementError::InvalidCandidate(
                    "both EventRelation endpoints must identify existing Events",
                ),
            )?;
            self.authorize(Capability::EventRead, event_target(event))
                .map_err(|_| {
                    FactManagementError::InvalidCandidate(
                        "both EventRelation endpoints must be readable",
                    )
                })?;
        }
        let revision = self.next_revision()?;
        let relation = EventRelation::new(relation_id, from_event_id, to_event_id, kind, revision)
            .map_err(|error| {
                FactManagementError::EventGraphConflict(format!("{error}. No relation was saved."))
            })?;
        let policy_target = event_relation_target(&relation);
        self.authorize(Capability::RelationshipCreate, policy_target)?;
        self.ensure_identity_available(RecordRef::EventRelation(relation_id))?;
        let additions = EventRelationBatch::new(vec![relation]).map_err(|error| {
            FactManagementError::EventGraphConflict(event_relation_error_message(&error))
        })?;
        validate_event_relation_transaction(&records, expected_base, revision, &[], &additions)?;
        self.publish(
            expected_base,
            operation_id,
            Record::EventRelation(relation),
            RecordRef::EventRelation(relation_id),
            policy_target,
        )
    }

    /// Retracts one active EventRelation without changing its Events.
    pub fn retract_event_relation(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: EventRelationRetractionId,
        target_id: EventRelationId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let relation = records
            .iter()
            .find_map(|record| match record {
                Record::EventRelation(value) if value.id() == target_id => Some(*value),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected EventRelation is unavailable",
            ))?;
        let target_policy = event_relation_target(&relation);
        self.authorize(Capability::RelationshipRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected EventRelation is unavailable")
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::RelationshipRetract, target_policy)?;
        let revision = self.next_revision()?;
        let retraction = EventRelationRetraction::new(retraction_id, &relation, reason, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.ensure_identity_available(RecordRef::EventRelationRetraction(retraction_id))?;
        let empty = EventRelationBatch::new(Vec::new())
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        validate_event_relation_transaction(
            &records,
            expected_base,
            revision,
            &[retraction.clone()],
            &empty,
        )?;
        self.publish(
            expected_base,
            operation_id,
            Record::EventRelationRetraction(retraction),
            RecordRef::EventRelationRetraction(retraction_id),
            target_policy,
        )
    }

    /// Appends an explicit Transaction-Time retraction for one active Assertion.
    pub fn retract_assertion(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: AssertionRetractionId,
        target_id: AssertionId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::Assertion(value) if value.id() == target_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Assertion is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::AssertionRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Assertion is unavailable")
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::AssertionRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target(target.context(), RecordRef::Assertion(target_id)),
        )?;
        if records.iter().any(|record| {
            matches!(
                record,
                Record::AssertionRetraction(value) if value.assertion_id() == target_id
            )
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Assertion is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::AssertionRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction = AssertionRetraction::new(retraction_id, &target, reason, revision)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::AssertionRetraction(retraction),
            RecordRef::AssertionRetraction(retraction_id),
            target_policy,
        )
    }

    /// Appends an explicit Transaction-Time retraction for one active Mask.
    pub fn retract_mask(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: MaskRetractionId,
        target_id: MaskId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::Mask(value) if value.id() == target_id => Some(value.clone()),
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Mask is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::MaskRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Mask is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Mask is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected Mask is unavailable")
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::MaskRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target(target.context(), RecordRef::Mask(target_id)),
        )?;
        if records.iter().any(|record| {
            matches!(record, Record::MaskRetraction(value) if value.mask_id() == target_id)
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Mask is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::MaskRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction =
            worlddb_core::MaskRetraction::new(retraction_id, &target, reason, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::MaskRetraction(retraction),
            RecordRef::MaskRetraction(retraction_id),
            target_policy,
        )
    }

    /// Appends an explicit Transaction-Time retraction for one active Boundary.
    pub fn retract_replacement_boundary(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        retraction_id: ReplacementBoundaryRetractionId,
        target_id: ReplacementBoundaryId,
        reason: String,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        validate_retraction_reason(&reason)?;
        let records = self.records_at_revision(expected_base)?;
        let target = records
            .iter()
            .find_map(|record| match record {
                Record::ReplacementBoundary(value) if value.id() == target_id => {
                    Some(value.clone())
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected ReplacementBoundary is unavailable",
            ))?;
        let target_policy = context_target(target.context());
        self.authorize(Capability::ReplacementBoundaryRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "the selected ReplacementBoundary is unavailable",
                )
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "the selected ReplacementBoundary is unavailable",
                )
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate(
                    "the selected ReplacementBoundary is unavailable",
                )
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        self.authorize(Capability::ReplacementBoundaryRetract, target_policy)?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target(
                target.context(),
                RecordRef::ReplacementBoundary(target_id),
            ),
        )?;
        if records.iter().any(|record| {
            matches!(
                record,
                Record::ReplacementBoundaryRetraction(value)
                    if value.replacement_boundary_id() == target_id
            )
        }) {
            return Err(FactManagementError::InvalidCandidate(
                "the selected ReplacementBoundary is already retracted",
            ));
        }
        self.ensure_identity_available(RecordRef::ReplacementBoundaryRetraction(retraction_id))?;
        let revision = self.next_revision()?;
        let retraction =
            ReplacementBoundaryRetraction::new(retraction_id, &target, reason, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::ReplacementBoundaryRetraction(retraction),
            RecordRef::ReplacementBoundaryRetraction(retraction_id),
            target_policy,
        )
    }

    /// Archives or unarchives one Assertion, Mask, ReplacementBoundary, Event,
    /// or EventMask.
    /// The transition is append-only and is always committed strictly after
    /// the selected target's creation revision.
    pub fn transition_archive(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        transition_id: ArchiveTransitionId,
        target: ArchiveTargetRef,
        action: ArchiveAction,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.check_base(expected_base)?;
        let records = self.records_at_revision(expected_base)?;
        let (target_ref, history_space, layer, target_created_revision, read_capability) = records
            .iter()
            .find_map(|record| match (target, record) {
                (ArchiveTargetRef::Assertion(id), Record::Assertion(value)) if id == value.id() => {
                    Some((
                        RecordRef::Assertion(id),
                        value.context().history_space_id(),
                        value.context().layer_id(),
                        value.created_revision(),
                        Capability::AssertionRead,
                    ))
                }
                (ArchiveTargetRef::Mask(id), Record::Mask(value)) if id == value.id() => Some((
                    RecordRef::Mask(id),
                    value.context().history_space_id(),
                    value.context().layer_id(),
                    value.created_revision(),
                    Capability::MaskRead,
                )),
                (ArchiveTargetRef::ReplacementBoundary(id), Record::ReplacementBoundary(value))
                    if id == value.id() =>
                {
                    Some((
                        RecordRef::ReplacementBoundary(id),
                        value.context().history_space_id(),
                        value.context().layer_id(),
                        value.created_revision(),
                        Capability::ReplacementBoundaryRead,
                    ))
                }
                (ArchiveTargetRef::Event(id), Record::Event(value)) if id == value.id() => Some((
                    RecordRef::Event(id),
                    value.history_space_id(),
                    value.layer_id(),
                    value.created_revision(),
                    Capability::EventRead,
                )),
                (ArchiveTargetRef::EventMask(id), Record::EventMask(value)) if id == value.id() => {
                    Some((
                        RecordRef::EventMask(id),
                        value.history_space_id(),
                        value.layer_id(),
                        value.created_revision(),
                        Capability::EventMaskRead,
                    ))
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected factual record is unavailable",
            ))?;
        let target_policy = context_target_parts(history_space, layer);
        self.authorize(read_capability, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected factual record is unavailable")
            })?;
        self.authorize(Capability::HistorySpaceRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected factual record is unavailable")
            })?;
        self.authorize(Capability::LayerRead, target_policy)
            .map_err(|_| {
                FactManagementError::InvalidCandidate("the selected factual record is unavailable")
            })?;
        self.authorize(Capability::LifecycleRead, target_policy)?;
        let permission = match action {
            ArchiveAction::Archive => Capability::Archive,
            ArchiveAction::Unarchive => Capability::Unarchive,
        };
        self.authorize(permission, record_target(target_ref))?;
        self.authorize(
            Capability::RelationshipCreate,
            lifecycle_relationship_target_parts(history_space, layer, target_ref),
        )?;
        self.ensure_identity_available(RecordRef::ArchiveTransition(transition_id))?;

        let (archive_targets, transitions) = archive_history_records(&records);
        let history =
            ArchiveHistoryReferenceModel::new(archive_targets.clone(), transitions.clone())
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let prior_state = history
            .state_at(target, RecordedAsOf::from_published_revision(expected_base))
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        let revision = self.next_revision()?;
        if revision <= target_created_revision {
            return Err(FactManagementError::InvalidRecordRevision);
        }
        let transition =
            ArchiveTransition::new(transition_id, target, action, prior_state, revision)
                .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        worlddb_core::validate_archive_transition_transaction(
            worlddb_core::ArchiveTransitionValidation {
                base_revision: expected_base,
                commit_revision: revision,
                base: &history,
                target_records_after: history.targets().to_vec(),
                transition_additions: &[transition],
                policy: self.policy_snapshot()?,
                principal: self.schema.principal,
            },
            &[Record::ArchiveTransition(transition)],
        )
        .map_err(|error| FactManagementError::Validation(error.to_string()))?;
        self.publish(
            expected_base,
            operation_id,
            Record::ArchiveTransition(transition),
            RecordRef::ArchiveTransition(transition_id),
            target_policy,
        )
    }

    /// Reads factual record families at one committed shared revision.
    pub fn snapshot_at(&self, revision: Revision) -> Result<FactSnapshot, FactManagementError> {
        if revision > self.revision() {
            return Err(FactManagementError::RevisionNotPublished);
        }
        let records = self.records_at_revision(revision)?;
        let mut assertions = Vec::new();
        let mut masks = Vec::new();
        let mut replacement_boundaries = Vec::new();
        let mut events = Vec::new();
        let mut event_masks = Vec::new();
        let mut event_relations = Vec::new();
        for record in &records {
            match record {
                Record::Assertion(value) => {
                    let target = context_target(value.context());
                    if self.may_read(Capability::AssertionRead, target)?
                        && self.may_read(Capability::HistorySpaceRead, target)?
                        && self.may_read(Capability::LayerRead, target)?
                    {
                        assertions.push(value.clone());
                    }
                }
                Record::Mask(value) => {
                    let target = context_target(value.context());
                    if self.may_read(Capability::MaskRead, target)?
                        && self.may_read(Capability::HistorySpaceRead, target)?
                        && self.may_read(Capability::LayerRead, target)?
                    {
                        masks.push(value.clone());
                    }
                }
                Record::ReplacementBoundary(value) => {
                    let target = context_target(value.context());
                    if self.may_read(Capability::ReplacementBoundaryRead, target)?
                        && self.may_read(Capability::HistorySpaceRead, target)?
                        && self.may_read(Capability::LayerRead, target)?
                    {
                        replacement_boundaries.push(value.clone());
                    }
                }
                Record::Event(value) => {
                    let owner = context_target_parts(value.history_space_id(), value.layer_id());
                    if self.may_read(Capability::EventRead, event_target(value))?
                        && self.may_read(Capability::HistorySpaceRead, owner)?
                        && self.may_read(Capability::LayerRead, owner)?
                    {
                        let event_kind_id = self
                            .may_read(
                                Capability::FieldRead,
                                record_field_target(
                                    value.history_space_id(),
                                    value.layer_id(),
                                    RecordRef::Event(value.id()),
                                    FieldSelector::EventKind,
                                ),
                            )?
                            .then_some(value.event_kind_id());
                        events.push(EventSnapshotRef {
                            event_id: value.id(),
                            created_revision: value.created_revision(),
                            history_space_id: value.history_space_id(),
                            layer_id: value.layer_id(),
                            event_kind_id,
                        });
                    }
                }
                _ => {}
            }
        }
        let visible_event_ids = events
            .iter()
            .map(|event| event.event_id())
            .collect::<BTreeSet<_>>();
        for record in &records {
            match record {
                Record::EventMask(mask)
                    if visible_event_ids.contains(&mask.target_event())
                        && self.may_read(Capability::EventMaskRead, event_mask_target(mask))?
                        && self.may_read(
                            Capability::HistorySpaceRead,
                            context_target_parts(mask.history_space_id(), mask.layer_id()),
                        )?
                        && self.may_read(
                            Capability::LayerRead,
                            context_target_parts(mask.history_space_id(), mask.layer_id()),
                        )?
                        && self.may_read(
                            Capability::FieldRead,
                            record_field_target(
                                mask.history_space_id(),
                                mask.layer_id(),
                                RecordRef::EventMask(mask.id()),
                                FieldSelector::EventMaskTarget,
                            ),
                        )? =>
                {
                    event_masks.push(*mask);
                }
                Record::EventRelation(relation)
                    if visible_event_ids.contains(&relation.from_event())
                        && visible_event_ids.contains(&relation.to_event())
                        && self.may_read(
                            Capability::RelationshipRead,
                            event_relation_target(relation),
                        )? =>
                {
                    event_relations.push(*relation);
                }
                _ => {}
            }
        }
        let visible_refs = assertions
            .iter()
            .map(|value| RecordRef::Assertion(value.id()))
            .chain(masks.iter().map(|value| RecordRef::Mask(value.id())))
            .chain(
                replacement_boundaries
                    .iter()
                    .map(|value| RecordRef::ReplacementBoundary(value.id())),
            )
            .chain(
                events
                    .iter()
                    .map(|value| RecordRef::Event(value.event_id())),
            )
            .chain(
                event_masks
                    .iter()
                    .map(|value| RecordRef::EventMask(value.id())),
            )
            .chain(
                event_relations
                    .iter()
                    .map(|value| RecordRef::EventRelation(value.id())),
            )
            .collect::<BTreeSet<_>>();
        let may_read_lifecycle =
            self.may_read(Capability::LifecycleRead, PolicyTarget::default())?;
        let mut assertion_retractions = Vec::new();
        let mut mask_retractions = Vec::new();
        let mut replacement_boundary_retractions = Vec::new();
        let mut event_retractions = Vec::new();
        let mut event_span_closures = Vec::new();
        let mut event_mask_retractions = Vec::new();
        let mut event_relation_retractions = Vec::new();
        let mut archive_transitions = Vec::new();
        if may_read_lifecycle {
            for record in &records {
                match record {
                    Record::AssertionRetraction(value)
                        if visible_refs.contains(&RecordRef::Assertion(value.assertion_id())) =>
                    {
                        assertion_retractions.push(value.clone());
                    }
                    Record::MaskRetraction(value)
                        if visible_refs.contains(&RecordRef::Mask(value.mask_id())) =>
                    {
                        mask_retractions.push(value.clone());
                    }
                    Record::ReplacementBoundaryRetraction(value)
                        if visible_refs.contains(&RecordRef::ReplacementBoundary(
                            value.replacement_boundary_id(),
                        )) =>
                    {
                        replacement_boundary_retractions.push(value.clone());
                    }
                    Record::EventRetraction(value)
                        if visible_refs.contains(&RecordRef::Event(value.event_id())) =>
                    {
                        event_retractions.push(value.clone());
                    }
                    Record::EventSpanClosure(value)
                        if visible_refs.contains(&RecordRef::Event(value.event_id())) =>
                    {
                        event_span_closures.push(*value);
                    }
                    Record::EventMaskRetraction(value)
                        if visible_refs.contains(&RecordRef::EventMask(value.event_mask_id())) =>
                    {
                        event_mask_retractions.push(value.clone());
                    }
                    Record::EventRelationRetraction(value)
                        if visible_refs
                            .contains(&RecordRef::EventRelation(value.event_relation_id())) =>
                    {
                        event_relation_retractions.push(value.clone());
                    }
                    Record::ArchiveTransition(value)
                        if archive_target_record_ref(value.target())
                            .is_some_and(|target| visible_refs.contains(&target)) =>
                    {
                        archive_transitions.push(*value);
                    }
                    _ => {}
                }
            }
        }
        let meta_history = meta_history::authorized_meta_history(self, &records)?;
        Ok(FactSnapshot {
            revision,
            assertions,
            assertion_retractions,
            masks,
            mask_retractions,
            replacement_boundaries,
            replacement_boundary_retractions,
            events,
            event_retractions,
            event_span_closures,
            event_masks,
            event_mask_retractions,
            event_relations,
            event_relation_retractions,
            archive_transitions,
            sources: meta_history.sources,
            evidence: meta_history.evidence,
            evidence_retractions: meta_history.evidence_retractions,
            provenance: meta_history.provenance,
            provenance_retractions: meta_history.provenance_retractions,
            provenance_endpoints: meta_history.provenance_endpoints,
            lifecycle_visible: may_read_lifecycle,
        })
    }

    /// Executes the productive M8 resolution preview over one authorized,
    /// revision-pinned factual and metadata snapshot.
    pub fn resolution_preview(
        &self,
        request: FactResolutionPreviewRequest,
    ) -> Result<QueryEngineOutput<ResolutionPreview>, FactManagementError> {
        let revision = self.revision();
        let execution = self.query_slot(FactQueryRequest {
            recorded_as_of: revision,
            schema_mode: SchemaMode::Historical,
            operation: FactQueryOperation::Resolved,
            history_space_id: request.history_space_id,
            layer_selection: request.layer_selection,
            subject: request.subject,
            predicate_id: request.predicate_id,
            perspective_scope: request.perspective_scope,
            epistemic_mode: request.epistemic_mode,
            world_time: request.world_time,
            max_candidates: 100_000,
            max_work_units: 500_000,
            max_results: 20_000,
        })?;
        match execution.result {
            FactQueryResult::Resolved(output) => Ok(output),
            FactQueryResult::History(_)
            | FactQueryResult::Explain(_)
            | FactQueryResult::Aggregate(_) => Err(FactManagementError::Query(
                "resolution query returned an incompatible result".to_owned(),
            )),
        }
    }

    /// Executes History, Resolved, or Explain through the productive query engine.
    pub fn query_slot(
        &self,
        request: FactQueryRequest,
    ) -> Result<FactQueryExecution, FactManagementError> {
        let required_capability = match request.operation {
            FactQueryOperation::History => Capability::RawHistoryRead,
            FactQueryOperation::Resolved => Capability::QueryResolve,
            FactQueryOperation::Explain => Capability::QueryExplain,
            FactQueryOperation::AggregateCount
            | FactQueryOperation::AggregateExists
            | FactQueryOperation::AggregateByPolarity => Capability::QueryAggregate,
        };
        self.authorize(required_capability, PolicyTarget::default())?;
        let revision = self.revision();
        if request.recorded_as_of > revision {
            return Err(FactManagementError::RevisionNotPublished);
        }
        if request.operation == FactQueryOperation::Explain
            && !matches!(request.world_time, WorldTimeSelector::At(_))
        {
            return Err(FactManagementError::InvalidCandidate(
                "Explain requires one explicit WorldTime point",
            ));
        }
        let metadata_manager = FileProjectMetadataManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let metadata = metadata_manager
            .snapshot_at(request.schema_mode, request.recorded_as_of)
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let metadata_as_of = metadata_manager
            .snapshot_at(SchemaMode::Historical, request.recorded_as_of)
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let metadata_head = metadata_manager
            .snapshot_at(SchemaMode::Current, revision)
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let facts = self.snapshot_at(request.recorded_as_of)?;
        let records = self.records_at_revision(request.recorded_as_of)?;
        let recorded_as_of = RecordedAsOf::from_published_revision(request.recorded_as_of);
        let schema_binding = HistoricalQueryBinding::bind(
            &self.schema.schema_history,
            recorded_as_of,
            request.schema_mode,
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let schema_revision = schema_binding.schema_revision();
        let predicate = metadata
            .schema()
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::Predicate(predicate)
                    if predicate.predicate_id() == request.predicate_id =>
                {
                    Some(predicate)
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Predicate is unavailable in the pinned schema",
            ))?;
        let timeline_is_active = match request.world_time {
            WorldTimeSelector::AllTimes => true,
            WorldTimeSelector::At(time) => metadata
                .schema()
                .timeline(time.timeline().id())
                .is_some_and(|timeline| timeline.lifecycle() == Lifecycle::Active),
        };
        if !timeline_is_active {
            return Err(FactManagementError::InvalidCandidate(
                "the selected query time requires an Active registered Timeline",
            ));
        }
        if metadata_as_of
            .history_spaces()
            .definition(request.history_space_id)
            .is_none()
        {
            return Err(FactManagementError::InvalidCandidate(
                "the selected HistorySpace is unavailable",
            ));
        }
        let layers =
            ValidatedLayerSelection::resolve(metadata.layers(), request.layer_selection.clone())
                .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let resolved_layers = layers.resolved().as_slice().to_vec();
        let budget_limits = QueryBudgetLimits::new(100_000, 500_000, 20_000)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let budget = QueryBudget::new(
            request.max_candidates,
            request.max_work_units,
            request.max_results,
            budget_limits,
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let snapshot_id = worlddb_core::storage_internal::generate_project_bootstrap_id::<
            worlddb_core::SnapshotId,
        >()
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let context = QueryContext::new(QueryContextInput {
            snapshot: SnapshotRef::new(snapshot_id),
            snapshot_revision: revision,
            recorded_as_of,
            history_space: request.history_space_id,
            layers,
            world_time: request.world_time,
            perspective: request.perspective_scope,
            epistemic_mode: request.epistemic_mode,
            schema_binding,
            security: SecurityContext::new(
                self.schema.principal,
                worlddb_core::AuthorizationMode::Now,
            ),
            budget,
            cancellation: CancellationToken::new(),
        })
        .map_err(|error| FactManagementError::Query(error.to_string()))?;

        if request.operation == FactQueryOperation::History {
            let history = fact_history_model(
                &facts,
                metadata_head.history_spaces().definitions().to_vec(),
                revision,
                &request,
                &resolved_layers,
            )?;
            let output = ProductiveQueryEngine::raw_history(
                &history,
                &context,
                self.schema.policy_history.policy(),
                FullScanBudget::Available,
                FactHistoryRecord::record_ref,
            )
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
            let rows = output
                .query()
                .value()
                .iter()
                .map(|row| FactQueryHistoryRow {
                    record_ref: row.record_ref(),
                    recorded_revision: row.recorded_revision(),
                    owner_history_space_id: row.owner_history_space_id(),
                    record: row.value().clone(),
                })
                .collect();
            return Ok(FactQueryExecution {
                snapshot_revision: revision,
                schema_revision,
                resolved_layers,
                result: FactQueryResult::History(rows),
            });
        }

        let mut assertion_commits =
            BTreeMap::<Revision, Vec<(HistorySpaceId, AssertionHistoryRecord)>>::new();
        for assertion in facts.assertions() {
            assertion_commits
                .entry(assertion.created_revision())
                .or_default()
                .push((
                    assertion.context().history_space_id(),
                    AssertionHistoryRecord::from_assertion(assertion.clone()),
                ));
        }
        for retraction in records.iter().filter_map(|record| match record {
            Record::AssertionRetraction(value)
                if facts
                    .assertions()
                    .iter()
                    .any(|assertion| assertion.id() == value.assertion_id()) =>
            {
                Some(value.clone())
            }
            _ => None,
        }) {
            let history_space_id = facts
                .assertions()
                .iter()
                .find(|assertion| assertion.id() == retraction.assertion_id())
                .map(|assertion| assertion.context().history_space_id())
                .ok_or(FactManagementError::InvalidCandidate(
                    "an Assertion lifecycle target is unavailable",
                ))?;
            assertion_commits
                .entry(retraction.created_revision())
                .or_default()
                .push((
                    history_space_id,
                    AssertionHistoryRecord::Retraction(retraction),
                ));
        }
        let history = HistorySpaceReferenceModel::from_published_snapshot(
            metadata_head.history_spaces().definitions().to_vec(),
            revision,
            assertion_commits.into_iter().collect(),
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;

        let mut archive_targets = Vec::new();
        for assertion in facts.assertions() {
            archive_targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::Assertion(assertion.id()),
                assertion.created_revision(),
            ));
        }
        for mask in facts.masks() {
            archive_targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::Mask(mask.id()),
                mask.created_revision(),
            ));
        }
        for boundary in facts.replacement_boundaries() {
            archive_targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::ReplacementBoundary(boundary.id()),
                boundary.created_revision(),
            ));
        }
        let visible_refs = facts
            .assertions()
            .iter()
            .map(|value| RecordRef::Assertion(value.id()))
            .chain(
                facts
                    .masks()
                    .iter()
                    .map(|value| RecordRef::Mask(value.id())),
            )
            .chain(
                facts
                    .replacement_boundaries()
                    .iter()
                    .map(|value| RecordRef::ReplacementBoundary(value.id())),
            )
            .collect::<BTreeSet<_>>();
        let archive_transitions = records
            .iter()
            .filter_map(|record| match record {
                Record::ArchiveTransition(value)
                    if archive_target_record_ref(value.target())
                        .is_some_and(|target| visible_refs.contains(&target)) =>
                {
                    Some(*value)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let archive = ArchiveHistoryReferenceModel::new(archive_targets, archive_transitions)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let mut archive_visible_masks = BTreeSet::new();
        let mut archive_visible_boundaries = BTreeSet::new();
        for mask in facts.masks() {
            if archive
                .state_at(ArchiveTargetRef::Mask(mask.id()), recorded_as_of)
                .map_err(|error| FactManagementError::Query(error.to_string()))?
                == ArchiveState::Unarchived
            {
                archive_visible_masks.insert(mask.id());
            }
        }
        for boundary in facts.replacement_boundaries() {
            if archive
                .state_at(
                    ArchiveTargetRef::ReplacementBoundary(boundary.id()),
                    recorded_as_of,
                )
                .map_err(|error| FactManagementError::Query(error.to_string()))?
                == ArchiveState::Unarchived
            {
                archive_visible_boundaries.insert(boundary.id());
            }
        }
        let policies = self.schema.policy_history.policy();
        let store = AssertionQueryStore::new(&history, &archive, metadata.layers(), policies);
        let no_mask_closures = [];
        let mask_retractions = records
            .iter()
            .filter_map(|record| match record {
                Record::MaskRetraction(value)
                    if facts
                        .masks()
                        .iter()
                        .any(|mask| mask.id() == value.mask_id()) =>
                {
                    Some(value.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let masks = AuthorizedAssertionMaskHistory::new(
            facts.masks(),
            &no_mask_closures,
            &mask_retractions,
            &archive_visible_masks,
        );
        let no_boundary_closures = [];
        let boundary_retractions = records
            .iter()
            .filter_map(|record| match record {
                Record::ReplacementBoundaryRetraction(value)
                    if facts
                        .replacement_boundaries()
                        .iter()
                        .any(|boundary| boundary.id() == value.replacement_boundary_id()) =>
                {
                    Some(value.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let boundaries = ReplacementBoundarySource::new(
            facts.replacement_boundaries(),
            &no_boundary_closures,
            &boundary_retractions,
            &archive_visible_boundaries,
        );
        let query = AssertionPointRequest::new(
            &context,
            masks,
            boundaries,
            AssertionPointIndexAccess::missing(),
            FullScanBudget::Available,
        );
        let slot = worlddb_core::MultiValueSlot::new(request.subject, request.predicate_id);
        let result = match request.operation {
            FactQueryOperation::History => {
                return Err(FactManagementError::Query(
                    "history query entered the resolution pipeline".to_owned(),
                ));
            }
            FactQueryOperation::Resolved => FactQueryResult::Resolved(
                ProductiveQueryEngine::resolution_preview(
                    store,
                    query,
                    slot,
                    predicate,
                    |left, right| temporal_value_equality(metadata.schema(), left, right),
                )
                .map_err(|error| FactManagementError::Query(error.to_string()))?,
            ),
            FactQueryOperation::Explain => FactQueryResult::Explain(
                ProductiveQueryEngine::explain_point(
                    store,
                    query,
                    slot,
                    predicate,
                    |left, right| temporal_value_equality(metadata.schema(), left, right),
                )
                .map_err(|error| FactManagementError::Query(error.to_string()))?,
            ),
            FactQueryOperation::AggregateCount
            | FactQueryOperation::AggregateExists
            | FactQueryOperation::AggregateByPolarity => {
                let resolved = ProductiveQueryEngine::resolution_preview(
                    store,
                    query,
                    slot,
                    predicate,
                    |left, right| temporal_value_equality(metadata.schema(), left, right),
                )
                .map_err(|error| FactManagementError::Query(error.to_string()))?;
                let spec = match request.operation {
                    FactQueryOperation::AggregateCount => AggregateSpec::Count,
                    FactQueryOperation::AggregateExists => AggregateSpec::Exists,
                    FactQueryOperation::AggregateByPolarity => {
                        AggregateSpec::grouped_count(vec![FieldSelector::AssertionPolarity])
                            .map_err(|error| FactManagementError::Query(error.to_string()))?
                    }
                    FactQueryOperation::History
                    | FactQueryOperation::Resolved
                    | FactQueryOperation::Explain => {
                        return Err(FactManagementError::Query(
                            "aggregate query entered an incompatible operation".to_owned(),
                        ));
                    }
                };
                let output = ProductiveQueryEngine::aggregate_resolution_contributors(
                    resolved.query(),
                    &spec,
                    &context,
                    policies,
                    |assertion_id, fields| {
                        let assertion = facts
                            .assertions()
                            .iter()
                            .find(|item| item.id() == assertion_id)
                            .ok_or(AggregateError::QueryBindingMismatch)?;
                        let group_values = fields
                            .iter()
                            .map(|field| {
                                let key = match field {
                                    FieldSelector::AssertionPolarity => {
                                        let positive = matches!(
                                            assertion.polarity(),
                                            worlddb_core::Polarity::Positive
                                        );
                                        GroupValueKey::new(
                                            ValueKind::Bool,
                                            vec![u8::from(positive)],
                                        )
                                    }
                                    _ => return Err(AggregateError::QueryBindingMismatch),
                                };
                                Ok((*field, Some(key)))
                            })
                            .collect::<Result<Vec<_>, AggregateError>>()?;
                        ResolvedAggregateRow::new(
                            RecordRef::Assertion(assertion_id),
                            assertion.context().history_space_id(),
                            assertion.context().layer_id(),
                            group_values,
                        )
                    },
                )
                .map_err(|error| FactManagementError::Query(error.to_string()))?;
                FactQueryResult::Aggregate(output)
            }
        };
        Ok(FactQueryExecution {
            snapshot_revision: revision,
            schema_revision,
            resolved_layers,
            result,
        })
    }

    fn prepare_explorer_query(
        &self,
        request: &FactExplorerRequest,
        now_ms: u64,
    ) -> Result<PreparedExplorerQuery, FactManagementError> {
        let revision = self.revision();
        if request.recorded_as_of > revision {
            return Err(FactManagementError::RevisionNotPublished);
        }
        let metadata_manager = FileProjectMetadataManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let metadata = metadata_manager
            .snapshot_at(request.schema_mode, request.recorded_as_of)
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let metadata_as_of = metadata_manager
            .snapshot_at(SchemaMode::Historical, request.recorded_as_of)
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        if metadata_as_of
            .history_spaces()
            .definition(request.history_space_id)
            .is_none()
        {
            return Err(FactManagementError::InvalidCandidate(
                "the selected HistorySpace is unavailable",
            ));
        }
        if let WorldTimeSelector::At(time) = request.world_time {
            if !metadata
                .schema()
                .timeline(time.timeline().id())
                .is_some_and(|timeline| timeline.lifecycle() == Lifecycle::Active)
            {
                return Err(FactManagementError::InvalidCandidate(
                    "the selected query time requires an Active registered Timeline",
                ));
            }
        }
        let layers =
            ValidatedLayerSelection::resolve(metadata.layers(), request.layer_selection.clone())
                .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let resolved_layers = layers.resolved().as_slice().to_vec();
        let facts = self.snapshot_at(request.recorded_as_of)?;
        let recorded_as_of = RecordedAsOf::from_published_revision(request.recorded_as_of);
        let schema_binding = HistoricalQueryBinding::bind(
            &self.schema.schema_history,
            recorded_as_of,
            request.schema_mode,
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let schema_revision = schema_binding.schema_revision();
        let history_spaces = metadata_as_of.history_spaces().clone();
        let budget_limits = QueryBudgetLimits::new(100_000, 500_000, 20_000)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let budget = QueryBudget::new(
            request.max_candidates,
            request.max_work_units,
            request.max_results,
            budget_limits,
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let snapshot_id = worlddb_core::storage_internal::generate_project_bootstrap_id::<
            worlddb_core::SnapshotId,
        >()
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let input = QueryContextInput {
            snapshot: SnapshotRef::new(snapshot_id),
            snapshot_revision: revision,
            recorded_as_of,
            history_space: request.history_space_id,
            layers,
            world_time: request.world_time,
            perspective: request.perspective_scope,
            epistemic_mode: request.epistemic_mode,
            schema_binding,
            security: SecurityContext::new(
                self.schema.principal,
                worlddb_core::AuthorizationMode::Now,
            ),
            budget,
            cancellation: CancellationToken::new(),
        };
        let history_space =
            history_space_view(metadata_as_of.history_spaces(), request.history_space_id)?;
        let database_id = self.schema.layout.database_id().ok_or_else(|| {
            FactManagementError::Storage("database identity is unavailable".to_owned())
        })?;
        let policy_version = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let binding = SnapshotBinding::new(SnapshotBindingInput {
            database_id,
            snapshot_id,
            data_revision: revision,
            recorded_as_of,
            schema: input.schema_binding,
            history_space,
            layer_schema: metadata.layers().clone(),
            layer_selection: request.layer_selection.clone(),
            security: SnapshotSecurityBinding::new(
                self.schema.principal,
                worlddb_core::AuthorizationMode::Now,
                policy_version.epoch(),
            ),
            backend_generation: self.schema.manifest.generation(),
        })
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let limits = SnapshotLifetimeLimits::new(30_000, 120_000, 600_000)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let registry = SnapshotRegistry::new(limits);
        let lease = registry
            .pin(binding, SnapshotPinPurpose::Interactive, now_ms)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let context = QueryContext::new_leased(input, lease, now_ms)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        Ok(PreparedExplorerQuery {
            context,
            facts,
            metadata,
            history_spaces,
            snapshot_revision: revision,
            schema_revision,
            resolved_layers,
        })
    }

    /// Starts a complete authorized TokenSearch and returns its first bounded page.
    pub fn start_token_search(
        &self,
        request: FactTokenSearchRequest,
        cursors: &mut CursorStateStore,
        now_ms: u64,
    ) -> Result<(FactTokenSearchSession, FactTokenSearchPage), FactManagementError> {
        self.authorize(Capability::QuerySearch, PolicyTarget::default())?;
        if request.page_size == 0 || request.page_size > 500 {
            return Err(FactManagementError::Query(
                "page size must be between 1 and 500".to_owned(),
            ));
        }
        let prepared = self.prepare_explorer_query(&request.query, now_ms)?;
        let predicate = prepared
            .metadata
            .schema()
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::Predicate(predicate)
                    if predicate.predicate_id() == request.query.predicate_id =>
                {
                    Some(predicate)
                }
                _ => None,
            })
            .ok_or(FactManagementError::InvalidCandidate(
                "the selected Predicate is unavailable in the pinned schema",
            ))?;
        if predicate.value_kind() != ValueKind::String {
            return Err(FactManagementError::InvalidCandidate(
                "exact TokenSearch requires a String-valued Predicate",
            ));
        }
        let field = FieldSelector::AssertionValue(request.query.predicate_id);
        let mut terms = Vec::new();
        terms
            .try_reserve_exact(request.terms.len())
            .map_err(|_| FactManagementError::Query("search request is too large".to_owned()))?;
        for term in request.terms {
            terms.push(
                SearchToken::new(term)
                    .map_err(|error| FactManagementError::Query(error.to_string()))?,
            );
        }
        let spec = SearchSpec::new(vec![field], terms, request.matching)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;

        let policy = self
            .schema
            .policy_history
            .policy()
            .resolve(&prepared.context)
            .map_err(|error| FactManagementError::Query(error.to_string()))?
            .snapshot();
        let principal = prepared.context.security().principal_id();

        let mut commits = BTreeMap::<Revision, Vec<(HistorySpaceId, Assertion)>>::new();
        for assertion in prepared.facts.assertions() {
            let history_space_id = assertion.context().history_space_id();
            if !token_search_record_is_authorized(
                assertion,
                history_space_id,
                field,
                policy,
                principal,
            ) {
                continue;
            }
            commits
                .entry(assertion.created_revision())
                .or_default()
                .push((history_space_id, assertion.clone()));
        }
        let history = HistorySpaceReferenceModel::from_published_snapshot(
            prepared.history_spaces.definitions().to_vec(),
            request.query.recorded_as_of,
            commits.into_iter().collect(),
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let mut documents = Vec::new();
        let mut coordinates = BTreeMap::new();
        for (_, history_space_id, assertion) in history
            .read_at(request.query.history_space_id, request.query.recorded_as_of)
            .map_err(|error| FactManagementError::Query(error.to_string()))?
        {
            let Some((record_ref, layer_id, document)) = token_search_document_if_authorized(
                assertion,
                history_space_id,
                &request.query,
                &prepared.resolved_layers,
                field,
                policy,
                principal,
            )?
            else {
                continue;
            };
            documents.push(document);
            coordinates.insert(record_ref, (history_space_id, layer_id));
        }
        let results = ProductiveQueryEngine::token_search(
            &documents,
            &spec,
            &[field],
            &prepared.context,
            self.schema.policy_history.policy(),
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let expires_at_ms = now_ms
            .checked_add(60_000)
            .ok_or_else(|| FactManagementError::Query("cursor expiry overflow".to_owned()))?;
        let session = FactTokenSearchSession {
            query_hash: request.query_hash,
            context: prepared.context,
            results,
            coordinates,
            field,
            page_size: request.page_size,
            expires_at_ms,
            snapshot_revision: prepared.snapshot_revision,
            schema_revision: prepared.schema_revision,
            resolved_layers: prepared.resolved_layers,
        };
        let page = self.read_token_search_page(&session, None, cursors, now_ms)?;
        Ok((session, page))
    }

    /// Continues one server-owned TokenSearch with current-policy reauthorization.
    pub fn continue_token_search(
        &self,
        session: &FactTokenSearchSession,
        cursor: Vec<u8>,
        cursors: &mut CursorStateStore,
        now_ms: u64,
    ) -> Result<FactTokenSearchPage, FactManagementError> {
        self.read_token_search_page(session, Some(cursor), cursors, now_ms)
    }

    fn read_token_search_page(
        &self,
        session: &FactTokenSearchSession,
        cursor: Option<Vec<u8>>,
        cursors: &mut CursorStateStore,
        now_ms: u64,
    ) -> Result<FactTokenSearchPage, FactManagementError> {
        let page_request = PageRequest::new(session.page_size, cursor)
            .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let execution = PageExecution::new(
            cursors,
            session.query_hash,
            PageOperation::TokenSearch,
            now_ms,
            session.expires_at_ms,
            500,
            worlddb_core::QueryExecutionPath::FullScan,
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let principal = session.context.security().principal_id();
        let field = session.field;
        let page = ProductiveQueryEngine::stream_page(
            page_request,
            execution,
            &session.context,
            self.schema.policy_history.policy(),
            |after| {
                let hits = session
                    .results
                    .query()
                    .value()
                    .iter()
                    .filter(|hit| {
                        let key = token_search_sort_key(hit);
                        after.is_none_or(|after| after < key.as_slice())
                    })
                    .cloned()
                    .map(Ok)
                    .collect::<Vec<Result<SearchHit, QueryEngineError>>>();
                Ok(hits.into_iter())
            },
            token_search_sort_key,
            |hit, policy, _context| {
                let Some((history_space, layer)) = session.coordinates.get(&hit.result_key())
                else {
                    return false;
                };
                let record_target = PolicyTarget::new(
                    Some(*history_space),
                    Some(*layer),
                    Some(hit.result_key()),
                    None,
                    None,
                );
                policy.authorize(principal, Capability::AssertionRead, record_target)
                    == AuthorizationDecision::Allow
                    && policy.authorize(
                        principal,
                        Capability::FieldRead,
                        PolicyTarget::new(
                            Some(*history_space),
                            Some(*layer),
                            Some(hit.result_key()),
                            Some(field),
                            None,
                        ),
                    ) == AuthorizationDecision::Allow
            },
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        let next_cursor = page.next_cursor().map(<[u8]>::to_vec);
        let cursor_expires_at_ms = next_cursor
            .as_deref()
            .and_then(|wire| CursorToken::decode(wire).ok())
            .map(CursorToken::expires_at_ms);
        Ok(FactTokenSearchPage {
            snapshot_revision: session.snapshot_revision,
            schema_revision: session.schema_revision,
            resolved_layers: session.resolved_layers.clone(),
            hits: page.results().to_vec(),
            next_cursor,
            cursor_expires_at_ms,
        })
    }

    /// Executes a complete bounded graph traversal over visible Event and Provenance records.
    pub fn query_graph(
        &self,
        request: FactExplorerRequest,
        spec: GraphSpec,
        now_ms: u64,
    ) -> Result<FactGraphExecution, FactManagementError> {
        self.authorize(Capability::QueryGraphTraverse, PolicyTarget::default())?;
        let prepared = self.prepare_explorer_query(&request, now_ms)?;
        let mut node_map = BTreeMap::<RecordRef, GraphNode>::new();
        let mut assertion_coordinates = BTreeMap::new();
        let mut mask_coordinates = BTreeMap::new();
        let mut boundary_coordinates = BTreeMap::new();

        for assertion in prepared.facts.assertions() {
            let context = assertion.context();
            if !history_space_contains_record(
                &prepared.history_spaces,
                request.history_space_id,
                context.history_space_id(),
                assertion.created_revision(),
                request.recorded_as_of,
            ) || !prepared.resolved_layers.contains(&context.layer_id())
                || context.perspective_scope() != request.perspective_scope
                || context.epistemic_mode() != request.epistemic_mode
                || assertion.subject() != request.subject
                || assertion.predicate_id() != request.predicate_id
            {
                continue;
            }
            if let WorldTimeSelector::At(time) = request.world_time {
                if !assertion
                    .validity()
                    .contains(time)
                    .map_err(|error| FactManagementError::Query(error.to_string()))?
                {
                    continue;
                }
            }
            let record_ref = RecordRef::Assertion(assertion.id());
            let coordinates = (context.history_space_id(), context.layer_id());
            node_map.insert(
                record_ref,
                GraphNode::new(record_ref, coordinates.0, coordinates.1),
            );
            assertion_coordinates.insert(assertion.id(), coordinates);
        }
        for mask in prepared.facts.masks() {
            let context = mask.context();
            if !history_space_contains_record(
                &prepared.history_spaces,
                request.history_space_id,
                context.history_space_id(),
                mask.created_revision(),
                request.recorded_as_of,
            ) || !prepared.resolved_layers.contains(&context.layer_id())
                || context.perspective_scope() != request.perspective_scope
                || context.epistemic_mode() != request.epistemic_mode
            {
                continue;
            }
            if let WorldTimeSelector::At(time) = request.world_time {
                if let Some(validity) = mask.validity() {
                    if !validity
                        .contains(time)
                        .map_err(|error| FactManagementError::Query(error.to_string()))?
                    {
                        continue;
                    }
                }
            }
            let record_ref = RecordRef::Mask(mask.id());
            let coordinates = (context.history_space_id(), context.layer_id());
            node_map.insert(
                record_ref,
                GraphNode::new(record_ref, coordinates.0, coordinates.1),
            );
            mask_coordinates.insert(mask.id(), coordinates);
        }
        for boundary in prepared.facts.replacement_boundaries() {
            let context = boundary.context();
            if !history_space_contains_record(
                &prepared.history_spaces,
                request.history_space_id,
                context.history_space_id(),
                boundary.created_revision(),
                request.recorded_as_of,
            ) || !prepared.resolved_layers.contains(&context.layer_id())
                || context.perspective_scope() != request.perspective_scope
                || context.epistemic_mode() != request.epistemic_mode
            {
                continue;
            }
            if let WorldTimeSelector::At(time) = request.world_time {
                if let Some(validity) = boundary.validity() {
                    if !validity
                        .contains(time)
                        .map_err(|error| FactManagementError::Query(error.to_string()))?
                    {
                        continue;
                    }
                }
            }
            let record_ref = RecordRef::ReplacementBoundary(boundary.id());
            let coordinates = (context.history_space_id(), context.layer_id());
            node_map.insert(
                record_ref,
                GraphNode::new(record_ref, coordinates.0, coordinates.1),
            );
            boundary_coordinates.insert(boundary.id(), coordinates);
        }
        for event in prepared.facts.events() {
            if !history_space_contains_record(
                &prepared.history_spaces,
                request.history_space_id,
                event.history_space_id(),
                event.created_revision(),
                request.recorded_as_of,
            ) || !prepared.resolved_layers.contains(&event.layer_id())
            {
                continue;
            }
            let record_ref = RecordRef::Event(event.event_id());
            let coordinates = (event.history_space_id(), event.layer_id());
            node_map.insert(
                record_ref,
                GraphNode::new(record_ref, coordinates.0, coordinates.1),
            );
        }

        for (assertion_id, coordinates) in &assertion_coordinates {
            for retraction in prepared
                .facts
                .assertion_retractions()
                .iter()
                .filter(|item| {
                    item.assertion_id() == *assertion_id
                        && item.created_revision() <= request.recorded_as_of
                })
            {
                let record_ref = RecordRef::AssertionRetraction(retraction.id());
                node_map.insert(
                    record_ref,
                    GraphNode::new(record_ref, coordinates.0, coordinates.1),
                );
            }
        }
        for (mask_id, coordinates) in &mask_coordinates {
            for retraction in prepared.facts.mask_retractions().iter().filter(|item| {
                item.mask_id() == *mask_id && item.created_revision() <= request.recorded_as_of
            }) {
                let record_ref = RecordRef::MaskRetraction(retraction.id());
                node_map.insert(
                    record_ref,
                    GraphNode::new(record_ref, coordinates.0, coordinates.1),
                );
            }
        }
        for (boundary_id, coordinates) in &boundary_coordinates {
            for retraction in prepared
                .facts
                .replacement_boundary_retractions()
                .iter()
                .filter(|item| {
                    item.replacement_boundary_id() == *boundary_id
                        && item.created_revision() <= request.recorded_as_of
                })
            {
                let record_ref = RecordRef::ReplacementBoundaryRetraction(retraction.id());
                node_map.insert(
                    record_ref,
                    GraphNode::new(record_ref, coordinates.0, coordinates.1),
                );
            }
        }
        for event_mask in prepared.facts.event_masks() {
            if !history_space_contains_record(
                &prepared.history_spaces,
                request.history_space_id,
                event_mask.history_space_id(),
                event_mask.created_revision(),
                request.recorded_as_of,
            ) || !prepared.resolved_layers.contains(&event_mask.layer_id())
            {
                continue;
            }
            let record_ref = RecordRef::EventMask(event_mask.id());
            node_map.insert(
                record_ref,
                GraphNode::new(
                    record_ref,
                    event_mask.history_space_id(),
                    event_mask.layer_id(),
                ),
            );
        }

        for source in prepared.facts.sources() {
            if source.created_revision() > request.recorded_as_of {
                continue;
            }
            node_map.insert(
                RecordRef::Source(source.id()),
                GraphNode::new_project_wide(RecordRef::Source(source.id())),
            );
        }
        for evidence in prepared.facts.evidence() {
            if evidence.created_revision() > request.recorded_as_of {
                continue;
            }
            let record_ref = RecordRef::Evidence(evidence.id());
            node_map.insert(record_ref, GraphNode::new_project_wide(record_ref));
        }
        for edge in prepared.facts.provenance() {
            if edge.created_revision() > request.recorded_as_of {
                continue;
            }
            let record_ref = RecordRef::Provenance(edge.id());
            node_map.insert(record_ref, GraphNode::new_project_wide(record_ref));
        }
        for retraction in prepared.facts.evidence_retractions() {
            if retraction.created_revision() > request.recorded_as_of {
                continue;
            }
            let record_ref = RecordRef::EvidenceRetraction(retraction.id());
            node_map.insert(record_ref, GraphNode::new_project_wide(record_ref));
        }
        for retraction in prepared.facts.provenance_retractions() {
            if retraction.created_revision() > request.recorded_as_of {
                continue;
            }
            let record_ref = RecordRef::ProvenanceRetraction(retraction.id());
            node_map.insert(record_ref, GraphNode::new_project_wide(record_ref));
        }

        let retracted_relations = prepared
            .facts
            .event_relation_retractions()
            .iter()
            .filter(|item| item.created_revision() <= request.recorded_as_of)
            .map(EventRelationRetraction::event_relation_id)
            .collect::<BTreeSet<_>>();
        let mut edges = Vec::new();
        for relation in prepared.facts.event_relations() {
            if relation.created_revision() > request.recorded_as_of
                || retracted_relations.contains(&relation.id())
            {
                continue;
            }
            let from = RecordRef::Event(relation.from_event());
            let to = RecordRef::Event(relation.to_event());
            if !node_map.contains_key(&from) || !node_map.contains_key(&to) {
                continue;
            }
            let relationship = match relation.kind() {
                worlddb_core::EventRelationKind::Before => GraphRelationshipKind::EventBefore,
                worlddb_core::EventRelationKind::SameTime => GraphRelationshipKind::EventSameTime,
                worlddb_core::EventRelationKind::Causes => GraphRelationshipKind::EventCauses,
            };
            edges.push(GraphEdge::new_project_wide(
                RecordRef::EventRelation(relation.id()),
                from,
                to,
                relationship,
            ));
        }
        let retracted_provenance = prepared
            .facts
            .provenance_retractions()
            .iter()
            .filter(|item| item.created_revision() <= request.recorded_as_of)
            .map(worlddb_core::ProvenanceRetraction::provenance_id)
            .collect::<BTreeSet<_>>();
        for provenance in prepared.facts.provenance() {
            if provenance.created_revision() > request.recorded_as_of
                || retracted_provenance.contains(&provenance.id())
            {
                continue;
            }
            let from = provenance_endpoint_record_ref(provenance.from());
            let to = provenance_endpoint_record_ref(provenance.to());
            if !node_map.contains_key(&from) || !node_map.contains_key(&to) {
                continue;
            }
            let relationship = match provenance.relation() {
                worlddb_core::ProvenanceRelation::Corrects => {
                    GraphRelationshipKind::ProvenanceCorrects
                }
                worlddb_core::ProvenanceRelation::DerivedFrom => {
                    GraphRelationshipKind::ProvenanceDerivedFrom
                }
                worlddb_core::ProvenanceRelation::ResultedFrom => {
                    GraphRelationshipKind::ProvenanceResultedFrom
                }
            };
            edges.push(GraphEdge::new_project_wide(
                RecordRef::Provenance(provenance.id()),
                from,
                to,
                relationship,
            ));
        }
        // Relation records are project-wide. Endpoint coordinates are retained above for
        // their own record checks; the query context still binds every semantic axis.
        let candidates = GraphCandidateSet::new_for_context(
            &prepared.context,
            node_map.into_values().collect(),
            edges,
        );
        let result = ProductiveQueryEngine::graph_traversal(
            &candidates,
            &spec,
            &prepared.context,
            self.schema.policy_history.policy(),
        )
        .map_err(|error| FactManagementError::Query(error.to_string()))?;
        Ok(FactGraphExecution {
            snapshot_revision: prepared.snapshot_revision,
            schema_revision: prepared.schema_revision,
            resolved_layers: prepared.resolved_layers,
            result,
        })
    }

    fn check_base(&self, expected_base: Revision) -> Result<(), FactManagementError> {
        if expected_base == self.revision() {
            Ok(())
        } else {
            Err(FactManagementError::Conflict)
        }
    }

    fn next_revision(&self) -> Result<Revision, FactManagementError> {
        self.schema
            .next_revision()
            .map_err(|error| FactManagementError::Storage(error.to_string()))
    }

    fn write_validation_metadata(
        &self,
    ) -> Result<worlddb_core::ProjectMetadataSnapshot, FactManagementError> {
        let manager = FileProjectMetadataManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        manager
            .snapshot_for_write_validation()
            .map_err(|error| FactManagementError::Storage(error.to_string()))
    }

    fn policy_snapshot(
        &self,
    ) -> Result<&worlddb_core::SecurityPolicySnapshot, FactManagementError> {
        self.schema
            .policy_history
            .policy()
            .latest_version()
            .map(|version| version.snapshot())
            .map_err(|error| FactManagementError::Storage(error.to_string()))
    }

    fn authorize(
        &self,
        capability: Capability,
        target: PolicyTarget,
    ) -> Result<(), FactManagementError> {
        if self
            .policy_snapshot()?
            .authorize(self.schema.principal, capability, target)
            == AuthorizationDecision::Allow
        {
            Ok(())
        } else {
            Err(FactManagementError::Unauthorized(capability))
        }
    }

    fn may_read(
        &self,
        capability: Capability,
        target: PolicyTarget,
    ) -> Result<bool, FactManagementError> {
        Ok(self
            .policy_snapshot()?
            .authorize(self.schema.principal, capability, target)
            == AuthorizationDecision::Allow)
    }

    fn ensure_identity_available(&self, identity: RecordRef) -> Result<(), FactManagementError> {
        let duplicate = self
            .records_at_revision(self.revision())?
            .iter()
            .any(|record| fact_record_ref(record) == Some(identity));
        if duplicate {
            Err(FactManagementError::DuplicateIdentity)
        } else {
            Ok(())
        }
    }

    fn records_at_revision(&self, revision: Revision) -> Result<Vec<Record>, FactManagementError> {
        let mut records = Vec::new();
        let store = HistorySegmentStore::new(self.schema.layout.clone());
        for reference in self
            .schema
            .manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
        {
            if reference.through_revision() > self.schema.manifest.revision() {
                return Err(FactManagementError::Storage(
                    "history reference exceeds the manifest revision".to_owned(),
                ));
            }
            let segment = store.read_segment(reference.id()).map_err(storage_error)?;
            if segment.content_digest() != reference.content_digest() {
                return Err(FactManagementError::Storage(
                    "history segment digest differs from its manifest".to_owned(),
                ));
            }
            for decoded in segment.records() {
                let record = decoded.record();
                let created = record_created_revision(record);
                let Some(created) = created else { continue };
                if created > reference.through_revision()
                    || created > self.schema.manifest.revision()
                {
                    return Err(FactManagementError::Storage(
                        "factual record exceeds its committed history segment".to_owned(),
                    ));
                }
                if created <= revision {
                    records.push(record.clone());
                }
            }
        }
        Ok(records)
    }

    fn verify_replayed_records(
        &self,
        revision: Revision,
        expected: &[Record],
    ) -> Result<(), FactManagementError> {
        let records = self.records_at_revision(revision)?;
        let mut actual = records
            .iter()
            .filter(|record| record_created_revision(record) == Some(revision))
            .map(|record| encode_record(record).map_err(storage_error))
            .collect::<Result<Vec<_>, _>>()?;
        let mut expected = expected
            .iter()
            .map(|record| encode_record(record).map_err(storage_error))
            .collect::<Result<Vec<_>, _>>()?;
        actual.sort();
        expected.sort();
        if actual == expected {
            Ok(())
        } else {
            Err(FactManagementError::IdempotencyMismatch)
        }
    }

    fn publish(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        record: Record,
        record_ref: RecordRef,
        policy_target: PolicyTarget,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        self.publish_batch(
            expected_base,
            operation_id,
            vec![record],
            record_ref,
            policy_target,
        )
    }

    fn publish_batch(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        records: Vec<Record>,
        record_ref: RecordRef,
        policy_target: PolicyTarget,
    ) -> Result<FactPublicationReceipt, FactManagementError> {
        if records.is_empty() {
            return Err(FactManagementError::InvalidRecordRevision);
        }
        self.check_base(expected_base)?;
        let wal = WalPrepareLog::new(&self.schema.layout);
        let live_head = wal
            .commit_head(self.schema.writer_lock)
            .map_err(storage_error)?
            .revision();
        if live_head != expected_base {
            return Err(FactManagementError::Conflict);
        }
        if wal
            .operation_status(self.schema.writer_lock, operation_id)
            .map_err(storage_error)?
            != WalOperationStatus::NotCommitted
        {
            return Err(FactManagementError::OperationAlreadyUsed);
        }

        let policy_version = self
            .schema
            .policy_history
            .policy()
            .latest_version()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let target_revision = expected_base
            .next_commit()
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        if records
            .iter()
            .any(|record| record_created_revision(record) != Some(target_revision))
        {
            return Err(FactManagementError::InvalidRecordRevision);
        }
        let sequence =
            next_audit_sequence(&wal, self.schema.writer_lock).map_err(map_schema_error)?;
        let policy_fingerprint = AuditPolicyFingerprint::new(Bytes::new(
            policy_version
                .snapshot()
                .effective_capability_fingerprint(self.schema.principal, policy_target)
                .to_vec(),
        ))
        .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id: worlddb_core::storage_internal::generate_factual_record_audit_record_id(
                )
                .map_err(storage_error)?,
                sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_factual_record_audit_operation_id()
                        .map_err(storage_error)?,
            },
            AuditRecordDetails {
                actor: self.schema.principal,
                action: AuditAction::FactualRecordWrite,
                object_class: AuditObjectClass::FactualRecord,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: target_revision,
                    operation_id,
                },
                security_epoch: policy_version.epoch(),
                policy_fingerprint,
            },
        );

        let history_store = HistorySegmentStore::new(self.schema.layout.clone());
        let history_receipt = history_store
            .stage_segment(self.schema.writer_lock, &records)
            .map_err(storage_error)?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            target_revision,
        );
        let security_store = SecurityPolicyHistoryStore::new(self.schema.layout.clone());
        let retention = self
            .schema
            .policy_history
            .audit_retention_at(expected_base)
            .map_err(|error| FactManagementError::Storage(error.to_string()))?;
        let next_policy_version = SecurityPolicyVersion::new(
            target_revision,
            policy_version.epoch(),
            policy_version.snapshot().clone(),
        );
        let security_receipt = match security_store.stage_version(
            self.schema.writer_lock,
            &next_policy_version,
            None,
            retention,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let cleanup = history_store.remove_staged_reference(history_reference);
                return Err(FactManagementError::Storage(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; staged factual history cleanup failed: {cleanup_error}")
                    }
                }));
            }
        };
        let security_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            security_receipt.id(),
            security_receipt.content_digest(),
            target_revision,
        );
        let mut next_references = self.schema.manifest.segments().to_vec();
        next_references.push(history_reference);
        next_references.push(security_reference);
        let staged = [history_reference, security_reference];
        match wal.commit_audited_manifest_snapshot(
            self.schema.writer_lock,
            operation_id,
            next_references,
            &staged,
            &audit_record,
        ) {
            Ok(receipt) if receipt.revision() == target_revision => {}
            Ok(_) => return Err(FactManagementError::UnknownCommit(operation_id)),
            Err(commit_error) => {
                let recovered = RecoveryManager::new(self.schema.layout.clone())
                    .recover(self.schema.writer_lock)
                    .map_err(|recovery_error| {
                        FactManagementError::UnknownCommitWithDiagnostic(
                            operation_id,
                            format!("{commit_error}; recovery: {recovery_error}"),
                        )
                    })?;
                let status = wal
                    .operation_status(self.schema.writer_lock, operation_id)
                    .map_err(storage_error)?;
                if !recovered.report().is_clean()
                    || !matches!(status, WalOperationStatus::Committed(receipt) if receipt.revision() == target_revision)
                {
                    if status == WalOperationStatus::NotCommitted {
                        cleanup_staged_schema(
                            &history_store,
                            history_reference,
                            &security_store,
                            security_reference,
                        );
                        return Err(FactManagementError::Storage(commit_error.to_string()));
                    }
                    return Err(FactManagementError::UnknownCommitWithDiagnostic(
                        operation_id,
                        commit_error.to_string(),
                    ));
                }
            }
        }

        let recovered = RecoveryManager::new(self.schema.layout.clone())
            .recover(self.schema.writer_lock)
            .map_err(|error| {
                FactManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
            })?;
        if !recovered.report().is_clean() {
            return Err(FactManagementError::UnknownCommit(operation_id));
        }
        let next_schema = FileSchemaManager::open(
            self.schema.layout.clone(),
            self.schema.writer_lock,
            self.schema.principal,
        )
        .map_err(|error| {
            FactManagementError::UnknownCommitWithDiagnostic(operation_id, error.to_string())
        })?;
        if next_schema.revision() != target_revision {
            return Err(FactManagementError::UnknownCommit(operation_id));
        }
        self.schema = next_schema;
        Ok(FactPublicationReceipt {
            operation_id,
            revision: target_revision,
            record_ref,
        })
    }
}

fn token_search_document_if_authorized(
    assertion: &Assertion,
    history_space_id: HistorySpaceId,
    query: &FactExplorerRequest,
    resolved_layers: &[worlddb_core::LayerId],
    field: FieldSelector,
    policy: &worlddb_core::SecurityPolicySnapshot,
    principal: worlddb_core::PrincipalId,
) -> Result<Option<(RecordRef, worlddb_core::LayerId, SearchDocument)>, FactManagementError> {
    let context = assertion.context();
    let record_ref = RecordRef::Assertion(assertion.id());
    if !token_search_record_is_authorized(assertion, history_space_id, field, policy, principal) {
        return Ok(None);
    }

    if !resolved_layers.contains(&context.layer_id())
        || assertion.subject() != query.subject
        || assertion.predicate_id() != query.predicate_id
        || context.perspective_scope() != query.perspective_scope
        || context.epistemic_mode() != query.epistemic_mode
    {
        return Ok(None);
    }
    if let WorldTimeSelector::At(time) = query.world_time {
        if !assertion
            .validity()
            .contains(time)
            .map_err(|error| FactManagementError::Query(error.to_string()))?
        {
            return Ok(None);
        }
    }
    let Value::String(text) = assertion.value() else {
        return Err(FactManagementError::InvalidCandidate(
            "a String-valued Predicate contains an incompatible Assertion",
        ));
    };
    let document = SearchDocument::new(
        record_ref,
        history_space_id,
        context.layer_id(),
        vec![SearchTextField::new(field, text.clone())],
    )
    .map_err(|error| FactManagementError::Query(error.to_string()))?;
    Ok(Some((record_ref, context.layer_id(), document)))
}

fn token_search_record_is_authorized(
    assertion: &Assertion,
    history_space_id: HistorySpaceId,
    field: FieldSelector,
    policy: &worlddb_core::SecurityPolicySnapshot,
    principal: worlddb_core::PrincipalId,
) -> bool {
    let context = assertion.context();
    let record_ref = RecordRef::Assertion(assertion.id());
    let record_target = PolicyTarget::new(
        Some(history_space_id),
        Some(context.layer_id()),
        Some(record_ref),
        None,
        None,
    );
    if policy.authorize(principal, Capability::AssertionRead, record_target)
        != AuthorizationDecision::Allow
    {
        return false;
    }
    let field_target = PolicyTarget::new(
        Some(history_space_id),
        Some(context.layer_id()),
        Some(record_ref),
        Some(field),
        None,
    );
    if policy.authorize(principal, Capability::FieldRead, field_target)
        != AuthorizationDecision::Allow
    {
        return false;
    }
    true
}

fn context_target(context: ContextKey) -> PolicyTarget {
    context_target_parts(context.history_space_id(), context.layer_id())
}

fn context_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
) -> PolicyTarget {
    PolicyTarget::new(Some(history_space), Some(layer), None, None, None)
}

fn event_target(event: &Event) -> PolicyTarget {
    PolicyTarget::new(
        Some(event.history_space_id()),
        Some(event.layer_id()),
        Some(RecordRef::Event(event.id())),
        None,
        None,
    )
}

fn event_mask_target(mask: &EventMask) -> PolicyTarget {
    PolicyTarget::new(
        Some(mask.history_space_id()),
        Some(mask.layer_id()),
        Some(RecordRef::EventMask(mask.id())),
        None,
        None,
    )
}

fn event_relation_target(relation: &EventRelation) -> PolicyTarget {
    PolicyTarget::new(
        None,
        None,
        Some(RecordRef::EventRelation(relation.id())),
        None,
        Some(RelationshipSelector::EventRelation(
            policy_event_relation_kind(relation.kind()),
        )),
    )
}

fn policy_event_relation_kind(kind: EventRelationKind) -> PolicyEventRelationKind {
    match kind {
        EventRelationKind::Before => PolicyEventRelationKind::Before,
        EventRelationKind::SameTime => PolicyEventRelationKind::SameTime,
        EventRelationKind::Causes => PolicyEventRelationKind::Causes,
    }
}

fn validate_event_relation_transaction(
    records: &[Record],
    base_revision: Revision,
    commit_revision: Revision,
    candidate_retractions: &[EventRelationRetraction],
    additions: &EventRelationBatch,
) -> Result<(), FactManagementError> {
    let events = records
        .iter()
        .filter_map(|record| match record {
            Record::Event(event) => Some(event.id()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let relations = records
        .iter()
        .filter_map(|record| match record {
            Record::EventRelation(relation) => Some(*relation),
            _ => None,
        })
        .collect::<Vec<_>>();
    let retractions = records
        .iter()
        .filter_map(|record| match record {
            Record::EventRelationRetraction(retraction) => Some(retraction.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    validate_event_graph_transaction(
        &events,
        EventRelationHistory::new(&relations, &retractions),
        RecordedAsOf::from_published_revision(base_revision),
        commit_revision,
        candidate_retractions,
        additions,
    )
    .map(|_| ())
    .map_err(|error| FactManagementError::EventGraphConflict(event_graph_error_message(&error)))
}

fn event_graph_error_message(error: &worlddb_core::EventGraphError) -> String {
    use worlddb_core::EventGraphError;

    match error {
        EventGraphError::BeforeWithinSameTimeComponent { .. } => {
            "Before/After widerspricht einer ausdrücklich gespeicherten SameTime-Gruppe. Entferne eine der Relationen oder nimm sie zuerst ausdrücklich zurück.".to_owned()
        }
        EventGraphError::BeforeCycle { .. } => {
            "Before/After würde einen Zyklus in der zeitlichen Reihenfolge erzeugen.".to_owned()
        }
        EventGraphError::CausesCycle { .. } => {
            "Causes würde einen Zyklus im Kausalgraphen erzeugen.".to_owned()
        }
        EventGraphError::MissingEventEndpoint { .. } => {
            "Beide Endpunkte der Relation müssen vorhandene Events bezeichnen.".to_owned()
        }
        EventGraphError::DuplicateActiveRelationKey { .. }
        | EventGraphError::Relation(worlddb_core::EventRelationError::DuplicateRelation { .. }) => {
            "Diese kanonische Relation ist bereits aktiv. After wird als umgekehrtes Before behandelt; bei SameTime spielt die Endpunktreihenfolge keine Rolle.".to_owned()
        }
        EventGraphError::Relation(worlddb_core::EventRelationError::SelfRelation { .. }) => {
            "Ein Event kann nicht mit sich selbst verknüpft werden.".to_owned()
        }
        EventGraphError::RetractionTargetNotActive { .. } => {
            "Die ausgewählte Eventrelation fehlt oder wurde bereits zurückgenommen.".to_owned()
        }
        EventGraphError::Relation(worlddb_core::EventRelationError::DuplicateRelationId { .. })
        | EventGraphError::DuplicateRelationId { .. } => {
            "Die Identität dieser Eventrelation ist bereits vergeben.".to_owned()
        }
        EventGraphError::Relation(worlddb_core::EventRelationError::DuplicateRetractionId { .. })
        | EventGraphError::DuplicateRetractionId { .. } => {
            "Die Identität dieser Rücknahme ist bereits vergeben.".to_owned()
        }
        EventGraphError::Relation(worlddb_core::EventRelationError::MissingRetractionTarget { .. }) => {
            "Die Zielrelation der Rücknahme ist nicht vorhanden.".to_owned()
        }
        EventGraphError::Relation(worlddb_core::EventRelationError::LifecycleRevisionNotAfterTarget { .. })
        | EventGraphError::CommitRevisionNotAfterSnapshot { .. }
        | EventGraphError::CandidateRetractionRevisionMismatch { .. }
        | EventGraphError::AdditionRevisionMismatch { .. } => {
            "Die Relation oder Rücknahme liegt nicht nach dem zugrunde liegenden Datenstand.".to_owned()
        }
        EventGraphError::DuplicateEventId => {
            "Der Ereignisbestand enthält eine doppelte Event-Identität.".to_owned()
        }
        EventGraphError::InternalEmptyComponent => {
            "Der Ereignisgraph ist intern inkonsistent; die Relation wurde nicht gespeichert.".to_owned()
        }
    }
}

fn event_relation_error_message(error: &worlddb_core::EventRelationError) -> String {
    match error {
        worlddb_core::EventRelationError::SelfRelation { .. } => {
            "Ein Event kann nicht mit sich selbst verknüpft werden. Es wurde nichts gespeichert.".to_owned()
        }
        worlddb_core::EventRelationError::DuplicateRelation { .. } => {
            "Diese kanonische Relation ist bereits aktiv. After wird als umgekehrtes Before behandelt; bei SameTime spielt die Endpunktreihenfolge keine Rolle. Es wurde nichts gespeichert.".to_owned()
        }
        _ => format!("Die Eventrelation ist ungültig ({error}). Es wurde nichts gespeichert."),
    }
}

fn field_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    field: FieldSelector,
) -> PolicyTarget {
    PolicyTarget::new(Some(history_space), Some(layer), None, Some(field), None)
}

fn record_field_target(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    record: RecordRef,
    field: FieldSelector,
) -> PolicyTarget {
    PolicyTarget::new(
        Some(history_space),
        Some(layer),
        Some(record),
        Some(field),
        None,
    )
}

fn relationship_target(
    context: ContextKey,
    record: RecordRef,
    relationship: ProvenanceRelationship,
) -> PolicyTarget {
    relationship_target_parts(
        context.history_space_id(),
        context.layer_id(),
        record,
        relationship,
    )
}

fn relationship_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    record: RecordRef,
    relationship: ProvenanceRelationship,
) -> PolicyTarget {
    PolicyTarget::new(
        Some(history_space),
        Some(layer),
        Some(record),
        None,
        Some(RelationshipSelector::Provenance(relationship)),
    )
}

fn lifecycle_relationship_target(context: ContextKey, record: RecordRef) -> PolicyTarget {
    lifecycle_relationship_target_parts(context.history_space_id(), context.layer_id(), record)
}

fn lifecycle_relationship_target_parts(
    history_space: HistorySpaceId,
    layer: worlddb_core::LayerId,
    record: RecordRef,
) -> PolicyTarget {
    PolicyTarget::new(
        Some(history_space),
        Some(layer),
        Some(record),
        None,
        Some(RelationshipSelector::LifecycleTarget),
    )
}

fn record_target(record: RecordRef) -> PolicyTarget {
    PolicyTarget::new(None, None, Some(record), None, None)
}

fn validate_retraction_reason(reason: &str) -> Result<(), FactManagementError> {
    if reason.trim().is_empty() {
        Err(FactManagementError::InvalidCandidate(
            "a non-empty retraction reason is required",
        ))
    } else {
        Ok(())
    }
}

fn field_target(context: ContextKey, field: FieldSelector) -> PolicyTarget {
    PolicyTarget::new(
        Some(context.history_space_id()),
        Some(context.layer_id()),
        None,
        Some(field),
        None,
    )
}

fn validate_context(
    metadata: &worlddb_core::ProjectMetadataSnapshot,
    context: ContextKey,
) -> Result<(), FactManagementError> {
    if metadata
        .history_spaces()
        .definition(context.history_space_id())
        .is_none()
    {
        return Err(FactManagementError::InvalidCandidate(
            "the selected HistorySpace is unavailable",
        ));
    }
    let layer = metadata.layers().definition(context.layer_id()).ok_or(
        FactManagementError::InvalidCandidate("the selected Layer is unavailable"),
    )?;
    if layer.lifecycle() == Lifecycle::Retired {
        return Err(FactManagementError::InvalidCandidate(
            "the selected Layer is retired",
        ));
    }
    if let worlddb_core::PerspectiveScope::Perspective(id) = context.perspective_scope() {
        if metadata.perspectives().is_retired(id)
            || metadata.perspectives().latest_definition(id).is_none()
        {
            return Err(FactManagementError::InvalidCandidate(
                "the selected Perspective is unavailable",
            ));
        }
    }
    Ok(())
}

fn validate_subject_and_predicate(
    metadata: &worlddb_core::ProjectMetadataSnapshot,
    subject: Subject,
    predicate_id: PredicateId,
) -> Result<&worlddb_core::PredicateDefinition, FactManagementError> {
    let entity = metadata.entities().entity(subject.entity_id()).ok_or(
        FactManagementError::InvalidCandidate("the selected Entity is unavailable"),
    )?;
    if metadata.entities().is_retired(subject.entity_id()) {
        return Err(FactManagementError::InvalidCandidate(
            "the selected Entity is retired",
        ));
    }
    let schema = metadata.schema();
    let entity_type = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::EntityType(value)
                if value.entity_type_id() == entity.entity_type_id() =>
            {
                Some(value)
            }
            _ => None,
        });
    let Some(entity_type) = entity_type else {
        return Err(FactManagementError::InvalidCandidate(
            "the Entity type is unavailable in the selected schema",
        ));
    };
    if entity_type.lifecycle() == Lifecycle::Retired {
        return Err(FactManagementError::InvalidCandidate(
            "the Entity type is retired",
        ));
    }
    let predicate = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::Predicate(value) if value.predicate_id() == predicate_id => {
                Some(value)
            }
            _ => None,
        });
    let Some(predicate) = predicate else {
        return Err(FactManagementError::InvalidCandidate(
            "the selected Predicate is unavailable",
        ));
    };
    if predicate.lifecycle() != Lifecycle::Active {
        return Err(FactManagementError::InvalidCandidate(
            "new factual records require an Active Predicate",
        ));
    }
    if !entity_type_constraint_matches(predicate.subject_constraint(), entity.entity_type_id()) {
        return Err(FactManagementError::InvalidCandidate(
            "the Entity type does not satisfy the Predicate subject constraint",
        ));
    }
    Ok(predicate)
}

fn entity_type_constraint_matches(
    constraint: EntityTypeConstraint,
    actual: worlddb_core::EntityTypeId,
) -> bool {
    matches!(constraint, EntityTypeConstraint::AnyEntity)
        || matches!(constraint, EntityTypeConstraint::Exact(expected) if expected == actual)
}

fn validate_entity_value(
    metadata: &worlddb_core::ProjectMetadataSnapshot,
    predicate: &worlddb_core::PredicateDefinition,
    value: &Value,
) -> Result<(), FactManagementError> {
    let Value::Entity(entity_id) = value else {
        return Ok(());
    };
    let entity =
        metadata
            .entities()
            .entity(*entity_id)
            .ok_or(FactManagementError::InvalidCandidate(
                "the referenced Entity is unavailable",
            ))?;
    if metadata.entities().is_retired(*entity_id) {
        return Err(FactManagementError::InvalidCandidate(
            "the referenced Entity is retired",
        ));
    }
    let entity_type =
        metadata
            .schema()
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::EntityType(value)
                    if value.entity_type_id() == entity.entity_type_id() =>
                {
                    Some(value)
                }
                _ => None,
            });
    if !entity_type.is_some_and(|value| value.lifecycle() != Lifecycle::Retired) {
        return Err(FactManagementError::InvalidCandidate(
            "the referenced Entity type is unavailable",
        ));
    }
    let constraint = predicate
        .object_constraint()
        .ok_or(FactManagementError::InvalidCandidate(
            "the Predicate has no Entity object constraint",
        ))?;
    if !entity_type_constraint_matches(constraint, entity.entity_type_id()) {
        return Err(FactManagementError::InvalidCandidate(
            "the referenced Entity type does not satisfy the Predicate object constraint",
        ));
    }
    Ok(())
}

fn temporal_value_equality(
    schema: &worlddb_core::SchemaSnapshot,
    left: &Value,
    right: &Value,
) -> Result<bool, ()> {
    Ok(match (left, right) {
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Int(left), Value::Int(right)) => left == right,
        (Value::UInt(left), Value::UInt(right)) => left == right,
        (Value::Decimal(left), Value::Decimal(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Symbol(left), Value::Symbol(right)) => left == right,
        (Value::Entity(left), Value::Entity(right)) => left == right,
        (Value::Duration(left), Value::Duration(right)) => left == right,
        (Value::Bytes(left), Value::Bytes(right)) => left == right,
        (Value::Time(left), Value::Time(right)) => {
            schema.resolve_time(left, false).map_err(|_| ())?
                == schema.resolve_time(right, false).map_err(|_| ())?
        }
        _ => false,
    })
}

fn validate_predicate_value(
    schema: &worlddb_core::SchemaSnapshot,
    predicate: &worlddb_core::PredicateDefinition,
    value: &Value,
) -> Result<(), FactManagementError> {
    if let Value::Time(time) = value {
        schema
            .resolve_time(time, false)
            .map_err(|error| FactManagementError::Validation(error.to_string()))?;
    }
    for constraint in predicate.constraints().rules() {
        if let worlddb_core::ValueConstraint::TimeRange(range) = constraint {
            for endpoint in [range.min(), range.max()].into_iter().flatten() {
                schema
                    .resolve_time(endpoint, false)
                    .map_err(|error| FactManagementError::Validation(error.to_string()))?;
            }
        }
    }
    validate_value_for_predicate(predicate, value, |time| {
        schema
            .resolve_time(time, false)
            .map_err(|_| worlddb_core::TemporalError::Overflow)
    })
    .map_err(|error| FactManagementError::Validation(error.to_string()))
}

fn validate_validity(
    schema: &worlddb_core::SchemaSnapshot,
    validity: Option<AssertionValidity>,
    allow_deprecated: bool,
) -> Result<(), FactManagementError> {
    let Some(validity) = validity else {
        return Ok(());
    };
    let timeline_id = validity.interval().timeline().id();
    let timeline = schema
        .definitions()
        .iter()
        .find_map(|definition| match definition {
            SchemaDefinition::Timeline(value) if value.timeline_id() == timeline_id => Some(value),
            _ => None,
        });
    if timeline.is_some_and(|value| {
        value.lifecycle() == Lifecycle::Active
            || (allow_deprecated && value.lifecycle() == Lifecycle::Deprecated)
    }) {
        Ok(())
    } else {
        Err(FactManagementError::InvalidCandidate(
            "validity requires an Active registered Timeline",
        ))
    }
}

fn record_created_revision(record: &Record) -> Option<Revision> {
    match record {
        Record::Assertion(value) => Some(value.created_revision()),
        Record::AssertionValidityClosure(value) => Some(value.created_revision()),
        Record::AssertionRetraction(value) => Some(value.created_revision()),
        Record::Mask(value) => Some(value.created_revision()),
        Record::MaskValidityClosure(value) => Some(value.created_revision()),
        Record::MaskRetraction(value) => Some(value.created_revision()),
        Record::ReplacementBoundary(value) => Some(value.created_revision()),
        Record::ReplacementBoundaryValidityClosure(value) => Some(value.created_revision()),
        Record::ReplacementBoundaryRetraction(value) => Some(value.created_revision()),
        Record::Event(value) => Some(value.created_revision()),
        Record::EventRetraction(value) => Some(value.created_revision()),
        Record::EventSpanClosure(value) => Some(value.created_revision()),
        Record::EventMask(value) => Some(value.created_revision()),
        Record::EventMaskRetraction(value) => Some(value.created_revision()),
        Record::EventRelation(value) => Some(value.created_revision()),
        Record::EventRelationRetraction(value) => Some(value.created_revision()),
        Record::Source(value) => Some(value.created_revision()),
        Record::Evidence(value) => Some(value.created_revision()),
        Record::EvidenceRetraction(value) => Some(value.created_revision()),
        Record::Provenance(value) => Some(value.created_revision()),
        Record::ProvenanceRetraction(value) => Some(value.created_revision()),
        Record::EntityRetirement(value) => Some(value.created_revision()),
        Record::PerspectiveRetirement(value) => Some(value.created_revision()),
        Record::ArchiveTransition(value) => Some(value.created_revision()),
        _ => None,
    }
}

fn fact_record_ref(record: &Record) -> Option<RecordRef> {
    Some(match record {
        Record::Assertion(value) => RecordRef::Assertion(value.id()),
        Record::AssertionValidityClosure(value) => RecordRef::AssertionValidityClosure(value.id()),
        Record::AssertionRetraction(value) => RecordRef::AssertionRetraction(value.id()),
        Record::Mask(value) => RecordRef::Mask(value.id()),
        Record::MaskValidityClosure(value) => RecordRef::MaskValidityClosure(value.id()),
        Record::MaskRetraction(value) => RecordRef::MaskRetraction(value.id()),
        Record::ReplacementBoundary(value) => RecordRef::ReplacementBoundary(value.id()),
        Record::ReplacementBoundaryValidityClosure(value) => {
            RecordRef::ReplacementBoundaryValidityClosure(value.id())
        }
        Record::ReplacementBoundaryRetraction(value) => {
            RecordRef::ReplacementBoundaryRetraction(value.id())
        }
        Record::Event(value) => RecordRef::Event(value.id()),
        Record::EventRetraction(value) => RecordRef::EventRetraction(value.id()),
        Record::EventSpanClosure(value) => RecordRef::EventSpanClosure(value.id()),
        Record::EventMask(value) => RecordRef::EventMask(value.id()),
        Record::EventMaskRetraction(value) => RecordRef::EventMaskRetraction(value.id()),
        Record::EventRelation(value) => RecordRef::EventRelation(value.id()),
        Record::EventRelationRetraction(value) => RecordRef::EventRelationRetraction(value.id()),
        Record::Source(value) => RecordRef::Source(value.id()),
        Record::Evidence(value) => RecordRef::Evidence(value.id()),
        Record::EvidenceRetraction(value) => RecordRef::EvidenceRetraction(value.id()),
        Record::Provenance(value) => RecordRef::Provenance(value.id()),
        Record::ProvenanceRetraction(value) => RecordRef::ProvenanceRetraction(value.id()),
        Record::EntityRetirement(value) => {
            RecordRef::EntityRetirement(value.entity_retirement_id())
        }
        Record::PerspectiveRetirement(value) => {
            RecordRef::PerspectiveRetirement(value.perspective_retirement_id())
        }
        Record::ArchiveTransition(value) => RecordRef::ArchiveTransition(value.id()),
        _ => return None,
    })
}

fn archive_target_record_ref(target: ArchiveTargetRef) -> Option<RecordRef> {
    Some(match target {
        ArchiveTargetRef::Assertion(id) => RecordRef::Assertion(id),
        ArchiveTargetRef::Mask(id) => RecordRef::Mask(id),
        ArchiveTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ArchiveTargetRef::Event(id) => RecordRef::Event(id),
        ArchiveTargetRef::EventMask(id) => RecordRef::EventMask(id),
        _ => return None,
    })
}

fn archive_history_records(
    records: &[Record],
) -> (Vec<ArchiveTargetRecord>, Vec<ArchiveTransition>) {
    let mut targets = Vec::new();
    let mut transitions = Vec::new();
    for record in records {
        match record {
            Record::Assertion(value) => targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::Assertion(value.id()),
                value.created_revision(),
            )),
            Record::Mask(value) => targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::Mask(value.id()),
                value.created_revision(),
            )),
            Record::ReplacementBoundary(value) => targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::ReplacementBoundary(value.id()),
                value.created_revision(),
            )),
            Record::Event(value) => targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::Event(value.id()),
                value.created_revision(),
            )),
            Record::EventMask(value) => targets.push(ArchiveTargetRecord::new(
                ArchiveTargetRef::EventMask(value.id()),
                value.created_revision(),
            )),
            Record::ArchiveTransition(value)
                if archive_target_record_ref(value.target()).is_some() =>
            {
                transitions.push(*value);
            }
            _ => {}
        }
    }
    (targets, transitions)
}

fn map_schema_error(error: SchemaManagementError) -> FactManagementError {
    match error {
        SchemaManagementError::Unauthorized(capability) => {
            FactManagementError::Unauthorized(capability)
        }
        SchemaManagementError::Conflict => FactManagementError::Conflict,
        SchemaManagementError::OperationAlreadyUsed => FactManagementError::OperationAlreadyUsed,
        SchemaManagementError::UnknownCommit(operation_id) => {
            FactManagementError::UnknownCommit(operation_id)
        }
        SchemaManagementError::UnknownCommitWithDiagnostic(operation_id, diagnostic) => {
            FactManagementError::UnknownCommitWithDiagnostic(operation_id, diagnostic)
        }
        other => FactManagementError::Storage(other.to_string()),
    }
}

fn storage_error(error: impl fmt::Display) -> FactManagementError {
    FactManagementError::Storage(error.to_string())
}

/// Factual-record read, validation, authorization, or publication failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FactManagementError {
    /// The current host Principal lacks the named permission.
    Unauthorized(Capability),
    /// The live database head differs from the expected base revision.
    Conflict,
    /// A requested historical revision is not committed.
    RevisionNotPublished,
    /// A typed input or reference violates the selected schema/catalog.
    Validation(String),
    /// A proposed EventRelation conflicts with the complete active Event graph.
    EventGraphConflict(String),
    /// The pinned resolution preview could not be completed safely.
    Query(String),
    /// A selected typed record, context, or schema definition is unavailable.
    InvalidCandidate(&'static str),
    /// The stable typed record identity already exists.
    DuplicateIdentity,
    /// The supplied OperationId has already been used.
    OperationAlreadyUsed,
    /// The supplied OperationId is committed with a different correction payload.
    IdempotencyMismatch,
    /// The record's Transaction-Time revision is not the next commit revision.
    InvalidRecordRevision,
    /// Commit outcome cannot be proved.
    UnknownCommit(OperationId),
    /// Commit outcome is unknown with recovery diagnostics.
    UnknownCommitWithDiagnostic(OperationId, String),
    /// Storage, recovery, audit, or referenced history failed closed.
    Storage(String),
}

impl fmt::Display for FactManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized(capability) => {
                write!(formatter, "missing required capability {capability:?}")
            }
            Self::Conflict => {
                formatter.write_str("database changed; reload factual data before retrying")
            }
            Self::RevisionNotPublished => {
                formatter.write_str("requested revision is not committed")
            }
            Self::Validation(reason) => {
                write!(formatter, "factual record validation failed: {reason}")
            }
            Self::EventGraphConflict(reason) => formatter.write_str(reason),
            Self::Query(reason) => write!(formatter, "resolution preview failed: {reason}"),
            Self::InvalidCandidate(reason) => formatter.write_str(reason),
            Self::DuplicateIdentity => {
                formatter.write_str("factual record identity already exists")
            }
            Self::OperationAlreadyUsed => formatter.write_str("OperationId is already used"),
            Self::IdempotencyMismatch => {
                formatter.write_str("OperationId is committed with a different payload")
            }
            Self::InvalidRecordRevision => {
                formatter.write_str("factual record does not use the next shared revision")
            }
            Self::UnknownCommit(operation_id) => write!(
                formatter,
                "factual commit outcome is unknown for {operation_id}"
            ),
            Self::UnknownCommitWithDiagnostic(operation_id, reason) => write!(
                formatter,
                "factual commit outcome is unknown for {operation_id}: {reason}"
            ),
            Self::Storage(reason) => {
                write!(formatter, "factual storage operation failed: {reason}")
            }
        }
    }
}

impl std::error::Error for FactManagementError {}

#[cfg(test)]
mod tests {
    use worlddb_core::{
        ArchiveAction, ArchiveTargetRef, Assertion, AssertionDraft, AssertionValidity, AuditAction,
        AuditCommitContext, AuditObjectClass, AuditPolicyFingerprint, AuditRecord,
        AuditRecordDetails, AuditRecordIdentity, AuditSequence, Bytes, Capability, CapabilityGrant,
        CapabilityRule, Cardinality, ConstraintSet, ContextKey, EntityTypeConstraint,
        EntityTypeDefinition, EntityTypeId, EventAttributeDefinition, EventAttributeId,
        EventAttributeValue, EventDraft, EventKindDefinition, EventKindId, EventParticipant,
        EventRelationInputKind, EventRoleDefinition, EventRoleId, EventTime, EventTimeConstraint,
        EventTimeForm, EvidenceRelation, EvidenceTargetRef, FieldSelector, GrantEffect,
        LayerDefinition, LayerId, LayerSchemaSnapshot, Lifecycle, MaskSelector, OperationId,
        Polarity, PolicyRuleId, PolicyScope, PolicySubject, PredicateDefinition,
        PredicateDefinitionSpec, PredicateId, Principal, ProvenanceEndpointRef, ProvenanceRelation,
        Record, RecordRef, ResolutionPolicy, ResolutionPreview, Revision, RoleCardinality,
        SchemaMode, SchemaRevision, SecurityPolicySnapshot, SecurityPolicyVersion,
        SourceContentDigest, SourceLocator, SourceMetadata, Subject, Symbol, TimeInterval,
        Timeline, TimelineCalendarProfile, TimelineDefinition, TimelineId, Value, ValueKind,
        WorldTime, WorldTimeSelector,
    };

    use crate::{
        DatabaseLayout, FileEntityManager, FileProjectMetadataManager, FileSchemaManager,
        FileSecurityPolicyManager, HistorySegmentStore, ManifestSegmentKind,
        ManifestSegmentReference, ManifestStore, RecoveryManager, SecurityPolicyHistoryStore,
        StorageVerifier, WalPrepareLog, WriterLock,
    };

    use super::super::tests::{TempArea, create_project, id};
    use super::{
        FactExplorerRequest, FactHistoryRecord, FactManagementError, FactQueryOperation,
        FactQueryRequest, FactQueryResult, FactTokenSearchRequest, FileFactManager as Manager,
        SourceDraft, token_search_document_if_authorized,
    };

    struct Fixture {
        _area: TempArea,
        layout: DatabaseLayout,
        lock: WriterLock,
        principal: worlddb_core::PrincipalId,
        history_space_id: worlddb_core::HistorySpaceId,
        layer_id: LayerId,
        predicate_id: PredicateId,
        timeline_id: TimelineId,
        event_kind_id: EventKindId,
        event_role_id: EventRoleId,
        event_attribute_id: EventAttributeId,
        entity_id: worlddb_core::EntityId,
    }

    fn capabilities(
        include_assertion_create: bool,
        include_query_resolve: bool,
        include_assertion_correction: bool,
    ) -> Vec<Capability> {
        let mut values = vec![
            Capability::SchemaRead,
            Capability::SchemaManage,
            Capability::HistorySpaceRead,
            Capability::LayerRead,
            Capability::LayerWrite,
            Capability::EntityRead,
            Capability::EntityCreate,
            Capability::EntityReference,
            Capability::PerspectiveRead,
            Capability::PerspectiveUse,
            Capability::FieldRead,
            Capability::FieldWrite,
            Capability::AssertionRead,
            Capability::MaskCreate,
            Capability::MaskRead,
            Capability::ReplacementBoundaryCreate,
            Capability::ReplacementBoundaryRead,
        ];
        if include_assertion_create {
            values.push(Capability::AssertionCreate);
        }
        if include_query_resolve {
            values.extend([
                Capability::QueryResolve,
                Capability::QueryExplain,
                Capability::QuerySearch,
                Capability::QueryGraphTraverse,
                Capability::QueryAggregate,
                Capability::RawHistoryRead,
            ]);
        }
        if include_assertion_correction {
            values.extend([
                Capability::AssertionCorrect,
                Capability::AssertionRetract,
                Capability::EventRead,
                Capability::EventCreate,
                Capability::EventCorrect,
                Capability::EventRetract,
                Capability::EventSpanClose,
                Capability::EventMaskCreate,
                Capability::EventMaskRead,
                Capability::EventMaskRetract,
                Capability::MaskRetract,
                Capability::ReplacementBoundaryRetract,
                Capability::ProvenanceCreate,
                Capability::LifecycleRead,
                Capability::Archive,
                Capability::Unarchive,
                Capability::RelationshipCreate,
                Capability::RelationshipRead,
                Capability::RelationshipRetract,
                Capability::HistorySpaceCreate,
                Capability::SourceRead,
                Capability::SourceCreate,
                Capability::SourceSupersede,
                Capability::EvidenceRead,
                Capability::EvidenceCreate,
                Capability::EvidenceRetract,
                Capability::ProvenanceRead,
                Capability::ProvenanceRetract,
                Capability::SecurityPolicyManage,
            ]);
        }
        values
    }

    fn build_fixture(
        include_assertion_create: bool,
        include_query_resolve: bool,
    ) -> Result<Fixture, String> {
        build_fixture_with_correction_rights(include_assertion_create, include_query_resolve, false)
    }

    fn build_fixture_with_correction_rights(
        include_assertion_create: bool,
        include_query_resolve: bool,
        include_assertion_correction: bool,
    ) -> Result<Fixture, String> {
        let area = TempArea::create()?;
        let root = area.database();
        let principal = create_project(
            &root,
            &capabilities(
                include_assertion_create,
                include_query_resolve,
                include_assertion_correction,
            ),
        )?;
        let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let (history_space_id, layer_id) = seed_initial_metadata(&layout, &lock, principal)?;

        let entity_type_id = id::<EntityTypeId>(101)?;
        let predicate_id = id::<PredicateId>(102)?;
        let timeline_id = id::<TimelineId>(103)?;
        let event_kind_id = id::<EventKindId>(112)?;
        let event_role_id = id::<EventRoleId>(113)?;
        let event_attribute_id = id::<EventAttributeId>(114)?;
        let mut schema = FileSchemaManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let entity_type = EntityTypeDefinition::new(
            entity_type_id,
            Symbol::new("person").map_err(|error| error.to_string())?,
            None,
            Lifecycle::Active,
            Revision::GENESIS,
        );
        let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol: Symbol::new("name").map_err(|error| error.to_string())?,
            subject_constraint: EntityTypeConstraint::Exact(entity_type_id),
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy: ResolutionPolicy::MultiValueReplace,
            constraints: ConstraintSet::new(vec![]).map_err(|error| error.to_string())?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: Revision::GENESIS,
        })
        .map_err(|error| error.to_string())?;
        let timeline = TimelineDefinition::new(
            timeline_id,
            Symbol::new("world_clock").map_err(|error| error.to_string())?,
            TimelineCalendarProfile::None,
            Lifecycle::Active,
            Revision::GENESIS,
        );
        let event_kind = EventKindDefinition::new(
            event_kind_id,
            Symbol::new("happening").map_err(|error| error.to_string())?,
            vec![EventRoleDefinition::new(
                event_role_id,
                Symbol::new("actor").map_err(|error| error.to_string())?,
                EntityTypeConstraint::Exact(entity_type_id),
                RoleCardinality::new(0, Some(4)).map_err(|error| error.to_string())?,
            )],
            vec![
                EventAttributeDefinition::new(
                    event_attribute_id,
                    Symbol::new("title").map_err(|error| error.to_string())?,
                    ValueKind::String,
                    None,
                    ConstraintSet::new(vec![]).map_err(|error| error.to_string())?,
                    None,
                    false,
                )
                .map_err(|error| error.to_string())?,
            ],
            EventTimeConstraint::new(EventTimeForm::OpenSpanAllowed, None)
                .map_err(|error| error.to_string())?,
            Lifecycle::Active,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
        let schema_receipt = schema
            .publish(
                schema.revision(),
                id::<OperationId>(104)?,
                vec![
                    Record::EntityTypeDefinition(entity_type),
                    Record::PredicateDefinition(predicate),
                    Record::TimelineDefinition(timeline),
                    Record::EventKindDefinition(event_kind),
                ],
            )
            .map_err(|error| error.to_string())?;
        let entity_id = id::<worlddb_core::EntityId>(105)?;
        FileEntityManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?
            .create(
                schema_receipt.revision(),
                id::<OperationId>(106)?,
                entity_id,
                entity_type_id,
                false,
            )
            .map_err(|error| error.to_string())?;

        Ok(Fixture {
            _area: area,
            layout,
            lock,
            principal,
            history_space_id,
            layer_id,
            predicate_id,
            timeline_id,
            event_kind_id,
            event_role_id,
            event_attribute_id,
            entity_id,
        })
    }

    fn seed_initial_metadata(
        layout: &DatabaseLayout,
        lock: &WriterLock,
        principal: worlddb_core::PrincipalId,
    ) -> Result<(worlddb_core::HistorySpaceId, LayerId), String> {
        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| String::from("project manifest is missing"))?;
        let policy_ids = manifest
            .segments()
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
            .map(|reference| reference.id())
            .collect::<Vec<_>>();
        let policy_history = SecurityPolicyHistoryStore::new(layout.clone())
            .load_history(manifest.revision(), &policy_ids)
            .map_err(|error| error.to_string())?;
        let policy_version = policy_history
            .policy()
            .latest_version()
            .map_err(|error| error.to_string())?
            .clone();
        let revision = manifest
            .revision()
            .next_commit()
            .map_err(|error| error.to_string())?;
        let history_space_id = id::<worlddb_core::HistorySpaceId>(107)?;
        let layer_id = id::<LayerId>(108)?;
        let history_space =
            worlddb_core::HistorySpaceDefinition::new(history_space_id, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?;
        let base_layer = LayerDefinition::new(
            layer_id,
            Symbol::new("base").map_err(|error| error.to_string())?,
            None,
            0,
            Lifecycle::Active,
            SchemaRevision::from_published_revision(revision),
        );
        let layers = LayerSchemaSnapshot::new(
            SchemaRevision::from_published_revision(revision),
            vec![base_layer.clone()],
            layer_id,
        )
        .map_err(|error| error.to_string())?;
        let records = vec![
            Record::HistorySpaceDefinition(history_space),
            Record::LayerDefinition(base_layer),
            Record::LayerSchemaSnapshot(layers),
        ];
        let history_store = HistorySegmentStore::new(layout.clone());
        let history_receipt = history_store
            .stage_segment(lock, &records)
            .map_err(|error| error.to_string())?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            revision,
        );
        let policy_store = SecurityPolicyHistoryStore::new(layout.clone());
        let next_version = SecurityPolicyVersion::new(
            revision,
            policy_version.epoch(),
            policy_version.snapshot().clone(),
        );
        let retention = policy_history
            .audit_retention_at(manifest.revision())
            .map_err(|error| error.to_string())?;
        let policy_receipt = policy_store
            .stage_version(lock, &next_version, None, retention)
            .map_err(|error| error.to_string())?;
        let policy_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            policy_receipt.id(),
            policy_receipt.content_digest(),
            revision,
        );
        let operation_id = id::<OperationId>(109)?;
        let audit = AuditRecord::new(
            AuditRecordIdentity {
                record_id: id::<worlddb_core::AuditRecordId>(110)?,
                sequence: AuditSequence::new(2),
                audit_operation_id: id::<worlddb_core::AuditOperationId>(111)?,
            },
            AuditRecordDetails {
                actor: principal,
                action: AuditAction::SchemaManagement,
                object_class: AuditObjectClass::SchemaDefinition,
                outcome: worlddb_core::AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision,
                    operation_id,
                },
                security_epoch: policy_version.epoch(),
                policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(
                    policy_version
                        .snapshot()
                        .effective_capability_fingerprint(
                            principal,
                            worlddb_core::PolicyTarget::default(),
                        )
                        .to_vec(),
                ))
                .map_err(|error| error.to_string())?,
            },
        );
        let mut references = manifest.segments().to_vec();
        references.extend([history_reference, policy_reference]);
        WalPrepareLog::new(layout)
            .commit_audited_manifest_snapshot(
                lock,
                operation_id,
                references,
                &[history_reference, policy_reference],
                &audit,
            )
            .map_err(|error| error.to_string())?;
        let recovered = RecoveryManager::new(layout.clone())
            .recover(lock)
            .map_err(|error| error.to_string())?;
        if !recovered.report().is_clean() {
            return Err("test metadata bootstrap requires recovery".to_owned());
        }
        Ok((history_space_id, layer_id))
    }

    fn make_context(fixture: &Fixture) -> Result<ContextKey, String> {
        ContextKey::new(
            fixture.history_space_id,
            fixture.layer_id,
            worlddb_core::PerspectiveScope::World,
            worlddb_core::EpistemicMode::WorldState,
        )
        .map_err(|error| error.to_string())
    }

    fn event_draft(
        fixture: &Fixture,
        manager: &Manager<'_>,
        nanoseconds: i128,
    ) -> Result<EventDraft, String> {
        let schema_manager =
            FileSchemaManager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
                .map_err(|error| error.to_string())?;
        let schema = schema_manager
            .schema_at(SchemaMode::Current, manager.revision())
            .map_err(|error| error.to_string())?;
        let kind = schema
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                worlddb_core::SchemaDefinition::EventKind(value)
                    if value.event_kind_id() == fixture.event_kind_id =>
                {
                    Some(value)
                }
                _ => None,
            })
            .ok_or_else(|| "test EventKind disappeared".to_owned())?;
        EventDraft::new(
            fixture.history_space_id,
            fixture.layer_id,
            kind,
            vec![],
            vec![],
            EventTime::Instant(WorldTime::from_nanoseconds(
                Timeline::new(fixture.timeline_id),
                nanoseconds,
            )),
        )
        .map_err(|error| error.to_string())
    }

    fn detailed_event_draft(
        fixture: &Fixture,
        manager: &Manager<'_>,
        event_time: EventTime,
    ) -> Result<EventDraft, String> {
        let schema_manager =
            FileSchemaManager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
                .map_err(|error| error.to_string())?;
        let schema = schema_manager
            .schema_at(SchemaMode::Current, manager.revision())
            .map_err(|error| error.to_string())?;
        let kind = schema
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                worlddb_core::SchemaDefinition::EventKind(value)
                    if value.event_kind_id() == fixture.event_kind_id =>
                {
                    Some(value)
                }
                _ => None,
            })
            .ok_or_else(|| "test EventKind disappeared".to_owned())?;
        EventDraft::new(
            fixture.history_space_id,
            fixture.layer_id,
            kind,
            vec![EventParticipant::new(
                fixture.event_role_id,
                fixture.entity_id,
            )],
            vec![EventAttributeValue::new(
                fixture.event_attribute_id,
                Value::String("test event".to_owned()),
            )],
            event_time,
        )
        .map_err(|error| error.to_string())
    }

    fn publish_test_event(
        fixture: &Fixture,
        manager: &mut Manager<'_>,
        event_id: worlddb_core::EventId,
        operation_id: OperationId,
        nanoseconds: i128,
    ) -> Result<super::FactPublicationReceipt, String> {
        let revision = manager.next_revision().map_err(|error| error.to_string())?;
        let event = worlddb_core::Event::new(
            event_id,
            event_draft(fixture, manager, nanoseconds)?,
            revision,
        )
        .map_err(|error| error.to_string())?;
        let target = worlddb_core::PolicyTarget::new(
            Some(fixture.history_space_id),
            Some(fixture.layer_id),
            Some(RecordRef::Event(event_id)),
            None,
            None,
        );
        manager
            .publish(
                manager.revision(),
                operation_id,
                Record::Event(event),
                RecordRef::Event(event_id),
                target,
            )
            .map_err(|error| error.to_string())
    }

    #[test]
    fn assertion_correction_commits_replacement_retraction_and_corrects_together()
    -> Result<(), String> {
        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let context = make_context(&fixture)?;
        let target_id = id::<worlddb_core::AssertionId>(151)?;
        let target = manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(150)?,
                target_id,
                AssertionDraft::new(
                    context,
                    Subject::new(fixture.entity_id),
                    fixture.predicate_id,
                    Value::String("Original".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        let base = target.revision();
        let operation_id = id::<OperationId>(152)?;
        let replacement_id = id::<worlddb_core::AssertionId>(154)?;
        let retraction_id = id::<worlddb_core::AssertionRetractionId>(155)?;
        let corrects_id = id::<worlddb_core::ProvenanceId>(156)?;
        let replacement = AssertionDraft::new(
            context,
            Subject::new(fixture.entity_id),
            fixture.predicate_id,
            Value::String("Replacement".to_owned()),
            Polarity::Negative,
            validity(&fixture)?,
        );
        let retraction_reason = "corrected the original value".to_owned();
        let receipt = manager
            .correct_assertion(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                retraction_id,
                corrects_id,
                replacement.clone(),
                retraction_reason.clone(),
                false,
            )
            .map_err(|error| error.to_string())?;
        let replay = manager
            .correct_assertion(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                retraction_id,
                corrects_id,
                replacement.clone(),
                retraction_reason.clone(),
                false,
            )
            .map_err(|error| error.to_string())?;
        if replay != receipt
            || manager
                .operation_status(operation_id)
                .map_err(|error| error.to_string())?
                != super::FactOperationStatus::Committed(receipt.revision())
        {
            return Err(
                "same OperationId and payload did not return the original correction receipt"
                    .into(),
            );
        }
        let changed_payload = AssertionDraft::new(
            context,
            Subject::new(fixture.entity_id),
            fixture.predicate_id,
            Value::String("Changed retry payload".to_owned()),
            Polarity::Negative,
            validity(&fixture)?,
        );
        if !matches!(
            manager.correct_assertion(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                retraction_id,
                corrects_id,
                changed_payload,
                retraction_reason,
                false,
            ),
            Err(FactManagementError::IdempotencyMismatch)
        ) {
            return Err("a changed payload reused the original correction OperationId".into());
        }
        if receipt.revision() != base.next_commit().map_err(|error| error.to_string())?
            || receipt.target() != target_id
            || manager.revision() != receipt.revision()
        {
            return Err(
                "correction receipt did not bind its target and one shared revision".into(),
            );
        }

        let manifest = ManifestStore::new(fixture.layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "manifest disappeared after correction".to_owned())?;
        let history_store = HistorySegmentStore::new(fixture.layout.clone());
        let correction_segment = manifest
            .segments()
            .iter()
            .find(|reference| {
                reference.kind() == ManifestSegmentKind::History
                    && reference.through_revision() == receipt.revision()
            })
            .ok_or_else(|| "correction history segment is missing".to_owned())?;
        let segment = history_store
            .read_segment(correction_segment.id())
            .map_err(|error| error.to_string())?;
        let records = segment
            .records()
            .iter()
            .map(|decoded| decoded.record())
            .collect::<Vec<_>>();
        if records.len() != 3
            || !records.iter().any(|record| {
                matches!(record, Record::Assertion(value) if value.id() == receipt.replacement())
            })
            || !records.iter().any(|record| {
                matches!(record, Record::AssertionRetraction(value)
                    if value.id() == receipt.retraction() && value.assertion_id() == receipt.target())
            })
            || !records.iter().any(|record| {
                matches!(record, Record::Provenance(value)
                    if value.id() == receipt.corrects()
                        && value.from() == worlddb_core::ProvenanceEndpointRef::Assertion(receipt.replacement())
                        && value.to() == worlddb_core::ProvenanceEndpointRef::Assertion(receipt.target()))
            })
        {
            return Err("correction did not publish exactly its three-record effect".into());
        }
        let audit_records = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?;
        if audit_records
            .iter()
            .filter(|entry| {
                entry.operation_id() == receipt.operation_id()
                    && entry.revision() == receipt.revision()
            })
            .count()
            != 1
        {
            return Err("correction did not commit one matching Required Audit record".into());
        }

        drop(manager);
        let reopened = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let snapshot = reopened
            .snapshot_at(receipt.revision())
            .map_err(|error| error.to_string())?;
        if snapshot.assertions().len() != 2
            || snapshot.assertion_retractions().len() != 1
            || snapshot
                .assertion_retractions()
                .first()
                .is_none_or(|retraction| retraction.assertion_id() != receipt.target())
        {
            return Err(
                "correction records did not survive reopen with their explicit Retraction".into(),
            );
        }
        let report = StorageVerifier::new(fixture.layout.clone())
            .verify(&fixture.lock)
            .map_err(|error| error.to_string())?;
        if !report.is_clean() {
            return Err("corrected database did not verify cleanly".into());
        }
        Ok(())
    }

    #[test]
    fn source_evidence_and_provenance_writes_validate_relation_targets_and_cycles()
    -> Result<(), String> {
        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let source_id = id::<worlddb_core::SourceId>(161)?;
        manager
            .create_source(
                manager.revision(),
                id::<OperationId>(162)?,
                SourceDraft {
                    source_id,
                    source_kind: Symbol::new("book").map_err(|error| error.to_string())?,
                    locator: Some(
                        SourceLocator::new("https://example.invalid/source")
                            .map_err(|error| error.to_string())?,
                    ),
                    content_digest: Some(
                        SourceContentDigest::new(Bytes::new(vec![0x12, 0x34]))
                            .map_err(|error| error.to_string())?,
                    ),
                    metadata: SourceMetadata::default(),
                },
            )
            .map_err(|error| error.to_string())?;

        let context = make_context(&fixture)?;
        let assertion_id = id::<worlddb_core::AssertionId>(163)?;
        manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(164)?,
                assertion_id,
                AssertionDraft::new(
                    context,
                    Subject::new(fixture.entity_id),
                    fixture.predicate_id,
                    Value::String("evidence target".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;

        let evidence_id = id::<worlddb_core::EvidenceId>(165)?;
        manager
            .create_evidence(
                manager.revision(),
                id::<OperationId>(166)?,
                evidence_id,
                source_id,
                EvidenceTargetRef::Assertion(assertion_id),
                EvidenceRelation::Supports,
            )
            .map_err(|error| error.to_string())?;

        let first_edge = id::<worlddb_core::ProvenanceId>(167)?;
        manager
            .create_provenance(
                manager.revision(),
                id::<OperationId>(168)?,
                first_edge,
                ProvenanceEndpointRef::Source(source_id),
                ProvenanceEndpointRef::Assertion(assertion_id),
                ProvenanceRelation::DerivedFrom,
            )
            .map_err(|error| error.to_string())?;
        let before_rejections = manager.revision();
        let forbidden_edge = id::<worlddb_core::ProvenanceId>(169)?;
        assert!(matches!(
            manager.create_provenance(
                before_rejections,
                id::<OperationId>(170)?,
                forbidden_edge,
                ProvenanceEndpointRef::Source(source_id),
                ProvenanceEndpointRef::Assertion(assertion_id),
                ProvenanceRelation::ResultedFrom,
            ),
            Err(FactManagementError::InvalidCandidate(_))
        ));
        let cycle_edge = id::<worlddb_core::ProvenanceId>(171)?;
        assert!(matches!(
            manager.create_provenance(
                before_rejections,
                id::<OperationId>(172)?,
                cycle_edge,
                ProvenanceEndpointRef::Assertion(assertion_id),
                ProvenanceEndpointRef::Source(source_id),
                ProvenanceRelation::DerivedFrom,
            ),
            Err(FactManagementError::InvalidCandidate(_))
        ));
        assert_eq!(manager.revision(), before_rejections);
        let records = manager
            .records_at_revision(before_rejections)
            .map_err(|error| error.to_string())?;
        assert!(
            records
                .iter()
                .any(|record| matches!(record, Record::Source(value) if value.id() == source_id))
        );
        assert!(
            records.iter().any(
                |record| matches!(record, Record::Evidence(value) if value.id() == evidence_id)
            )
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| matches!(record, Record::Provenance(_)))
                .count(),
            1
        );

        let visible = manager
            .snapshot_at(before_rejections)
            .map_err(|error| error.to_string())?;
        assert_eq!(visible.sources().len(), 1);
        assert_eq!(visible.evidence().len(), 1);
        assert_eq!(visible.provenance().len(), 1);
        assert!(
            visible
                .provenance_endpoints()
                .contains(&ProvenanceEndpointRef::Assertion(assertion_id))
        );

        drop(manager);
        let mut security = FileSecurityPolicyManager::open(
            fixture.layout.clone(),
            &fixture.lock,
            fixture.principal,
        )
        .map_err(|error| error.to_string())?;
        security
            .add_capability_rule(
                security.revision(),
                id::<OperationId>(173)?,
                id::<PolicyRuleId>(174)?,
                PolicySubject::Principal(fixture.principal),
                Capability::AssertionRead,
                GrantEffect::Deny,
            )
            .map_err(|error| error.to_string())?;
        let mut denied_manager =
            Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
                .map_err(|error| error.to_string())?;
        let denied_base = denied_manager.revision();
        assert!(matches!(
            denied_manager.create_evidence(
                denied_base,
                id::<OperationId>(175)?,
                id::<worlddb_core::EvidenceId>(176)?,
                source_id,
                EvidenceTargetRef::Assertion(assertion_id),
                EvidenceRelation::Documents,
            ),
            Err(FactManagementError::Unauthorized(Capability::AssertionRead))
        ));
        assert_eq!(denied_manager.revision(), denied_base);
        let redacted = denied_manager
            .snapshot_at(denied_base)
            .map_err(|error| error.to_string())?;
        assert_eq!(redacted.sources().len(), 1);
        assert!(redacted.evidence().is_empty());
        assert!(redacted.provenance().is_empty());
        assert!(
            !redacted
                .provenance_endpoints()
                .contains(&ProvenanceEndpointRef::Assertion(assertion_id))
        );
        Ok(())
    }

    #[test]
    fn event_correction_is_two_records_and_keeps_retraction_separate() -> Result<(), String> {
        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let target_id = id::<worlddb_core::EventId>(158)?;
        let target = publish_test_event(
            &fixture,
            &mut manager,
            target_id,
            id::<OperationId>(157)?,
            10,
        )?;
        let base = target.revision();
        let replacement_id = id::<worlddb_core::EventId>(159)?;
        let corrects_id = id::<worlddb_core::ProvenanceId>(160)?;
        let replacement = event_draft(&fixture, &manager, 20)?;
        let operation_id = id::<OperationId>(161)?;
        let correction = manager
            .correct_event(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                corrects_id,
                replacement.clone(),
            )
            .map_err(|error| error.to_string())?;
        let replay = manager
            .correct_event(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                corrects_id,
                replacement.clone(),
            )
            .map_err(|error| error.to_string())?;
        if replay != correction {
            return Err(
                "same Event correction OperationId did not return its original receipt".into(),
            );
        }
        if !matches!(
            manager.correct_event(
                base,
                operation_id,
                target_id,
                target.revision(),
                replacement_id,
                corrects_id,
                event_draft(&fixture, &manager, 21)?,
            ),
            Err(FactManagementError::IdempotencyMismatch)
        ) {
            return Err(
                "a changed Event payload reused the original correction OperationId".into(),
            );
        }
        if correction.target() != target_id
            || correction.replacement() != replacement_id
            || correction.revision() != base.next_commit().map_err(|error| error.to_string())?
        {
            return Err("Event correction receipt did not bind the exact two-record effect".into());
        }
        let manifest = ManifestStore::new(fixture.layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "manifest disappeared after Event correction".to_owned())?;
        let reference = manifest
            .segments()
            .iter()
            .find(|segment| {
                segment.kind() == ManifestSegmentKind::History
                    && segment.through_revision() == correction.revision()
            })
            .ok_or_else(|| "Event correction segment is missing".to_owned())?;
        let segment = HistorySegmentStore::new(fixture.layout.clone())
            .read_segment(reference.id())
            .map_err(|error| error.to_string())?;
        let records = segment
            .records()
            .iter()
            .map(|decoded| decoded.record())
            .collect::<Vec<_>>();
        if records.len() != 2
            || !records.iter().any(|record| {
                matches!(record, Record::Event(value) if value.id() == replacement_id)
            })
            || !records.iter().any(|record| {
                matches!(record, Record::Provenance(value)
                    if value.id() == corrects_id
                        && value.from() == worlddb_core::ProvenanceEndpointRef::Event(replacement_id)
                        && value.to() == worlddb_core::ProvenanceEndpointRef::Event(target_id))
            })
        {
            return Err("Event correction did not publish only Event and Corrects records".into());
        }
        let snapshot = manager
            .snapshot_at(correction.revision())
            .map_err(|error| error.to_string())?;
        if snapshot.events().len() != 2 || !snapshot.event_retractions().is_empty() {
            return Err("Event correction implicitly retracted its original Event".into());
        }
        let explicit_retraction = manager
            .retract_event(
                correction.revision(),
                id::<OperationId>(162)?,
                id::<worlddb_core::EventRetractionId>(163)?,
                target_id,
                "separate explicit lifecycle decision".to_owned(),
            )
            .map_err(|error| error.to_string())?;
        let after_retraction = manager
            .snapshot_at(explicit_retraction.revision())
            .map_err(|error| error.to_string())?;
        if after_retraction.event_retractions().len() != 1
            || after_retraction
                .event_retractions()
                .first()
                .is_none_or(|retraction| retraction.event_id() != target_id)
        {
            return Err("explicit EventRetraction was not recorded as its own commit".into());
        }
        let archived = manager
            .transition_archive(
                explicit_retraction.revision(),
                id::<OperationId>(164)?,
                id::<worlddb_core::ArchiveTransitionId>(165)?,
                ArchiveTargetRef::Event(replacement_id),
                ArchiveAction::Archive,
            )
            .map_err(|error| error.to_string())?;
        let after_archive = manager
            .snapshot_at(archived.revision())
            .map_err(|error| error.to_string())?;
        if after_archive.archive_transitions().len() != 1
            || after_archive
                .archive_transitions()
                .first()
                .is_none_or(|transition| {
                    transition.target() != ArchiveTargetRef::Event(replacement_id)
                })
        {
            return Err("Event archive did not create a separate target transition".into());
        }
        Ok(())
    }

    #[test]
    fn event_writes_validate_roles_masks_spans_and_relation_graphs() -> Result<(), String> {
        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let instant_a = EventTime::Instant(WorldTime::from_nanoseconds(
            Timeline::new(fixture.timeline_id),
            10,
        ));
        let event_a_id = id::<worlddb_core::EventId>(170)?;
        manager
            .create_event(
                manager.revision(),
                id::<OperationId>(171)?,
                event_a_id,
                detailed_event_draft(&fixture, &manager, instant_a)?,
            )
            .map_err(|error| error.to_string())?;
        let event_b_id = id::<worlddb_core::EventId>(172)?;
        manager
            .create_event(
                manager.revision(),
                id::<OperationId>(173)?,
                event_b_id,
                detailed_event_draft(
                    &fixture,
                    &manager,
                    EventTime::Instant(WorldTime::from_nanoseconds(
                        Timeline::new(fixture.timeline_id),
                        20,
                    )),
                )?,
            )
            .map_err(|error| error.to_string())?;
        let open_span_id = id::<worlddb_core::EventId>(174)?;
        manager
            .create_event(
                manager.revision(),
                id::<OperationId>(175)?,
                open_span_id,
                detailed_event_draft(
                    &fixture,
                    &manager,
                    EventTime::span(
                        WorldTime::from_nanoseconds(Timeline::new(fixture.timeline_id), 30),
                        None,
                    )
                    .map_err(|error| error.to_string())?,
                )?,
            )
            .map_err(|error| error.to_string())?;

        let closed_span = manager
            .close_event_span(
                manager.revision(),
                id::<OperationId>(176)?,
                id::<worlddb_core::EventSpanClosureId>(177)?,
                open_span_id,
                WorldTime::from_nanoseconds(Timeline::new(fixture.timeline_id), 50),
            )
            .map_err(|error| error.to_string())?;
        let before_duplicate_closure = manager.revision();
        if !matches!(
            manager.close_event_span(
                before_duplicate_closure,
                id::<OperationId>(178)?,
                id::<worlddb_core::EventSpanClosureId>(179)?,
                open_span_id,
                WorldTime::from_nanoseconds(Timeline::new(fixture.timeline_id), 60),
            ),
            Err(FactManagementError::InvalidCandidate(_))
        ) || manager.revision() != before_duplicate_closure
        {
            return Err("a closed Event span accepted a second closure".into());
        }

        let before_id = id::<worlddb_core::EventRelationId>(180)?;
        manager
            .create_event_relation(
                closed_span.revision(),
                id::<OperationId>(181)?,
                before_id,
                event_a_id,
                event_b_id,
                EventRelationInputKind::Before,
            )
            .map_err(|error| error.to_string())?;
        let relation_conflict_revision = manager.revision();
        if !matches!(
            manager.create_event_relation(
                relation_conflict_revision,
                id::<OperationId>(182)?,
                id::<worlddb_core::EventRelationId>(183)?,
                event_b_id,
                event_a_id,
                EventRelationInputKind::After,
            ),
            Err(FactManagementError::EventGraphConflict(_))
        ) || manager.revision() != relation_conflict_revision
        {
            return Err("inverse After input duplicated an active Before relation".into());
        }
        if !matches!(
            manager.create_event_relation(
                relation_conflict_revision,
                id::<OperationId>(184)?,
                id::<worlddb_core::EventRelationId>(185)?,
                event_a_id,
                event_b_id,
                EventRelationInputKind::SameTime,
            ),
            Err(FactManagementError::EventGraphConflict(_))
        ) || manager.revision() != relation_conflict_revision
        {
            return Err("SameTime contradicted an active Before path without rejection".into());
        }

        let causes = manager
            .create_event_relation(
                relation_conflict_revision,
                id::<OperationId>(186)?,
                id::<worlddb_core::EventRelationId>(187)?,
                event_a_id,
                event_b_id,
                EventRelationInputKind::Causes,
            )
            .map_err(|error| error.to_string())?;
        let before_causes_cycle = manager.revision();
        if !matches!(
            manager.create_event_relation(
                before_causes_cycle,
                id::<OperationId>(188)?,
                id::<worlddb_core::EventRelationId>(189)?,
                event_b_id,
                event_a_id,
                EventRelationInputKind::Causes,
            ),
            Err(FactManagementError::EventGraphConflict(_))
        ) || manager.revision() != before_causes_cycle
        {
            return Err("a cycle in the separate Causes graph was accepted".into());
        }

        let retract_before = manager
            .retract_event_relation(
                causes.revision(),
                id::<OperationId>(190)?,
                id::<worlddb_core::EventRelationRetractionId>(191)?,
                before_id,
                "replace temporal direction".to_owned(),
            )
            .map_err(|error| error.to_string())?;
        let inverse = manager
            .create_event_relation(
                retract_before.revision(),
                id::<OperationId>(192)?,
                id::<worlddb_core::EventRelationId>(193)?,
                event_a_id,
                event_b_id,
                EventRelationInputKind::After,
            )
            .map_err(|error| error.to_string())?;
        let relation_snapshot = manager
            .snapshot_at(inverse.revision())
            .map_err(|error| error.to_string())?;
        if relation_snapshot.event_relations().len() != 3
            || relation_snapshot.event_relation_retractions().len() != 1
            || !relation_snapshot.event_relations().iter().any(|relation| {
                relation.created_revision() == inverse.revision()
                    && relation.kind() == worlddb_core::EventRelationKind::Before
                    && relation.from_event() == event_b_id
                    && relation.to_event() == event_a_id
            })
        {
            return Err("After was not persisted as the canonical reversed Before relation".into());
        }

        let assertion_id = id::<worlddb_core::AssertionId>(200)?;
        let assertion_context = make_context(&fixture)?;
        let assertion = manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(201)?,
                assertion_id,
                AssertionDraft::new(
                    assertion_context,
                    Subject::new(fixture.entity_id),
                    fixture.predicate_id,
                    Value::String("archived assertion before EventMask".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        let archived_assertion = manager
            .transition_archive(
                assertion.revision(),
                id::<OperationId>(202)?,
                id::<worlddb_core::ArchiveTransitionId>(203)?,
                ArchiveTargetRef::Assertion(assertion_id),
                ArchiveAction::Archive,
            )
            .map_err(|error| error.to_string())?;

        drop(manager);
        let mut metadata = FileProjectMetadataManager::open(
            fixture.layout.clone(),
            &fixture.lock,
            fixture.principal,
        )
        .map_err(|error| error.to_string())?;
        let child_id = id::<worlddb_core::HistorySpaceId>(194)?;
        let child = metadata
            .create_child(
                metadata.revision(),
                id::<OperationId>(195)?,
                child_id,
                fixture.history_space_id,
                archived_assertion.revision(),
            )
            .map_err(|error| error.to_string())?;
        drop(metadata);
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let mask = manager
            .create_event_mask(
                child.revision(),
                id::<OperationId>(196)?,
                id::<worlddb_core::EventMaskId>(197)?,
                child_id,
                fixture.layer_id,
                event_a_id,
            )
            .map_err(|error| error.to_string())?;
        let mask_retraction = manager
            .retract_event_mask(
                mask.revision(),
                id::<OperationId>(198)?,
                id::<worlddb_core::EventMaskRetractionId>(199)?,
                id::<worlddb_core::EventMaskId>(197)?,
                "the event is visible again".to_owned(),
            )
            .map_err(|error| error.to_string())?;
        let snapshot = manager
            .snapshot_at(mask_retraction.revision())
            .map_err(|error| error.to_string())?;
        if snapshot.event_masks().len() != 1
            || snapshot.event_mask_retractions().len() != 1
            || snapshot
                .event_retractions()
                .iter()
                .any(|retraction| retraction.event_id() == event_a_id)
        {
            return Err("EventMask retraction changed the target Event lifecycle".into());
        }
        let report = StorageVerifier::new(fixture.layout.clone())
            .verify(&fixture.lock)
            .map_err(|error| error.to_string())?;
        if !report.is_clean() {
            return Err("event writes did not leave a clean verified database".into());
        }
        Ok(())
    }

    fn validity(fixture: &Fixture) -> Result<AssertionValidity, String> {
        let interval = TimeInterval::new(Timeline::new(fixture.timeline_id), None, None)
            .map_err(|error| error.to_string())?;
        Ok(AssertionValidity::new(interval))
    }

    #[test]
    fn assertion_mask_and_boundary_commit_with_required_audit_and_survive_reopen()
    -> Result<(), String> {
        let fixture = build_fixture(true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let base = manager.revision();
        let context = make_context(&fixture)?;
        let subject = Subject::new(fixture.entity_id);
        let validity = validity(&fixture)?;
        let assertion_id = id::<worlddb_core::AssertionId>(120)?;
        let assertion = manager
            .create_assertion(
                base,
                id::<OperationId>(121)?,
                assertion_id,
                AssertionDraft::new(
                    context,
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                    validity,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        let mask = manager
            .create_mask(
                assertion.revision(),
                id::<OperationId>(122)?,
                id::<worlddb_core::MaskId>(123)?,
                context,
                MaskSelector::Proposition(worlddb_core::PropositionKey::new(
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                )),
                None,
            )
            .map_err(|error| error.to_string())?;
        let boundary = manager
            .create_replacement_boundary(
                mask.revision(),
                id::<OperationId>(124)?,
                id::<worlddb_core::ReplacementBoundaryId>(125)?,
                super::ReplacementBoundaryDraft::new(context, subject, fixture.predicate_id, None),
            )
            .map_err(|error| error.to_string())?;

        let audits = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?;
        let factual = audits
            .iter()
            .filter(|entry| entry.record().action() == AuditAction::FactualRecordWrite)
            .collect::<Vec<_>>();
        if factual.len() != 3
            || factual.iter().any(|entry| {
                entry.record().object_class() != AuditObjectClass::FactualRecord
                    || entry.record().commit_context()
                        != (AuditCommitContext::Committed {
                            revision: entry.revision(),
                            operation_id: entry.operation_id(),
                        })
            })
            || !factual.iter().any(|entry| {
                entry.revision() == assertion.revision()
                    && entry.operation_id() == assertion.operation_id()
            })
            || !factual.iter().any(|entry| {
                entry.revision() == mask.revision() && entry.operation_id() == mask.operation_id()
            })
            || !factual.iter().any(|entry| {
                entry.revision() == boundary.revision()
                    && entry.operation_id() == boundary.operation_id()
            })
        {
            return Err(
                "factual records and Required Audit did not share their commits".to_owned(),
            );
        }

        drop(manager);
        let reopened = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let snapshot = reopened
            .snapshot_at(boundary.revision())
            .map_err(|error| error.to_string())?;
        let assertion_matches = snapshot.assertions().first().is_some_and(|assertion| {
            assertion.id() == assertion_id
                && matches!(assertion.value(), Value::String(value) if value == "Alice")
        });
        if snapshot.assertions().len() != 1
            || snapshot.masks().len() != 1
            || snapshot.replacement_boundaries().len() != 1
            || !assertion_matches
        {
            return Err("factual records did not survive a manager reopen".to_owned());
        }
        let report = StorageVerifier::new(fixture.layout.clone())
            .verify(&fixture.lock)
            .map_err(|error| error.to_string())?;
        if !report.is_clean() {
            return Err("factual-record commits did not verify cleanly".to_owned());
        }
        Ok(())
    }

    #[test]
    fn invalid_and_unauthorized_assertions_leave_revision_and_audit_unchanged() -> Result<(), String>
    {
        let fixture = build_fixture(true, true)?;
        let context = make_context(&fixture)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let base = manager.revision();
        let audit_count = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?
            .len();
        let invalid = manager.create_assertion(
            base,
            id::<OperationId>(130)?,
            id::<worlddb_core::AssertionId>(131)?,
            AssertionDraft::new(
                context,
                Subject::new(fixture.entity_id),
                fixture.predicate_id,
                Value::Bool(true),
                Polarity::Positive,
                validity(&fixture)?,
            ),
            false,
        );
        if !matches!(invalid, Err(FactManagementError::Validation(_))) || manager.revision() != base
        {
            return Err("schema-invalid assertion was not rejected before commit".to_owned());
        }
        let after_invalid = WalPrepareLog::new(&fixture.layout)
            .committed_required_audit_records(&fixture.lock)
            .map_err(|error| error.to_string())?
            .len();
        if after_invalid != audit_count {
            return Err("rejected assertion left an audit or data commit".to_owned());
        }

        let denied = build_fixture(false, true)?;
        let denied_context = make_context(&denied)?;
        let mut denied_manager =
            Manager::open(denied.layout.clone(), &denied.lock, denied.principal)
                .map_err(|error| error.to_string())?;
        let denied_base = denied_manager.revision();
        let denied_audit_count = WalPrepareLog::new(&denied.layout)
            .committed_required_audit_records(&denied.lock)
            .map_err(|error| error.to_string())?
            .len();
        let unauthorized = denied_manager.create_assertion(
            denied_base,
            id::<OperationId>(132)?,
            id::<worlddb_core::AssertionId>(133)?,
            AssertionDraft::new(
                denied_context,
                Subject::new(denied.entity_id),
                denied.predicate_id,
                Value::String("Bob".to_owned()),
                Polarity::Positive,
                validity(&denied)?,
            ),
            false,
        );
        if !matches!(
            unauthorized,
            Err(FactManagementError::Unauthorized(
                Capability::AssertionCreate
            ))
        ) || denied_manager.revision() != denied_base
        {
            return Err("assertion without AssertionCreate was not rejected".to_owned());
        }
        let denied_after = WalPrepareLog::new(&denied.layout)
            .committed_required_audit_records(&denied.lock)
            .map_err(|error| error.to_string())?
            .len();
        if denied_after != denied_audit_count {
            return Err("unauthorized assertion left an audit or data commit".to_owned());
        }
        Ok(())
    }

    #[test]
    fn resolution_preview_uses_persisted_facts_and_requires_query_resolve() -> Result<(), String> {
        let fixture = build_fixture(true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let context = make_context(&fixture)?;
        let subject = Subject::new(fixture.entity_id);
        manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(140)?,
                id::<worlddb_core::AssertionId>(141)?,
                AssertionDraft::new(
                    context,
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;

        let preview = manager
            .resolution_preview(super::FactResolutionPreviewRequest {
                history_space_id: fixture.history_space_id,
                layer_selection: worlddb_core::LayerSelection::BaseOnly,
                subject,
                predicate_id: fixture.predicate_id,
                perspective_scope: worlddb_core::PerspectiveScope::World,
                epistemic_mode: worlddb_core::EpistemicMode::WorldState,
                world_time: worlddb_core::WorldTimeSelector::AllTimes,
            })
            .map_err(|error| error.to_string())?;
        if !matches!(
            preview.query().value(),
            worlddb_core::ResolutionPreview::AllTimes { slices } if !slices.is_empty()
        ) {
            return Err(format!(
                "the all-times preview did not resolve the persisted Assertion: {:?}",
                preview.query().value()
            ));
        }

        let denied = build_fixture(true, false)?;
        let denied_manager = Manager::open(denied.layout.clone(), &denied.lock, denied.principal)
            .map_err(|error| error.to_string())?;
        let denied = denied_manager.resolution_preview(super::FactResolutionPreviewRequest {
            history_space_id: denied.history_space_id,
            layer_selection: worlddb_core::LayerSelection::BaseOnly,
            subject: Subject::new(denied.entity_id),
            predicate_id: denied.predicate_id,
            perspective_scope: worlddb_core::PerspectiveScope::World,
            epistemic_mode: worlddb_core::EpistemicMode::WorldState,
            world_time: worlddb_core::WorldTimeSelector::AllTimes,
        });
        if !matches!(
            denied,
            Err(FactManagementError::Unauthorized(Capability::QueryResolve))
        ) {
            return Err("resolution preview without QueryResolve was not rejected".to_owned());
        }
        Ok(())
    }

    #[test]
    fn query_slot_binds_recorded_revision_and_explains_applied_mask_and_boundary()
    -> Result<(), String> {
        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let context = make_context(&fixture)?;
        let subject = Subject::new(fixture.entity_id);
        let assertion_id = id::<worlddb_core::AssertionId>(143)?;
        let assertion = manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(142)?,
                assertion_id,
                AssertionDraft::new(
                    context,
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        let child_history_space_id = id::<worlddb_core::HistorySpaceId>(148)?;
        let mut metadata = FileProjectMetadataManager::open(
            fixture.layout.clone(),
            &fixture.lock,
            fixture.principal,
        )
        .map_err(|error| error.to_string())?;
        metadata
            .create_child(
                assertion.revision(),
                id::<OperationId>(149)?,
                child_history_space_id,
                fixture.history_space_id,
                assertion.revision(),
            )
            .map_err(|error| error.to_string())?;
        drop(metadata);
        drop(manager);
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let child_context = ContextKey::new(
            child_history_space_id,
            fixture.layer_id,
            worlddb_core::PerspectiveScope::World,
            worlddb_core::EpistemicMode::WorldState,
        )
        .map_err(|error| error.to_string())?;
        let mask_id = id::<worlddb_core::MaskId>(145)?;
        let mask = manager
            .create_mask(
                manager.revision(),
                id::<OperationId>(144)?,
                mask_id,
                child_context,
                MaskSelector::Proposition(worlddb_core::PropositionKey::new(
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                )),
                None,
            )
            .map_err(|error| error.to_string())?;
        let boundary_id = id::<worlddb_core::ReplacementBoundaryId>(147)?;
        let boundary = manager
            .create_replacement_boundary(
                mask.revision(),
                id::<OperationId>(146)?,
                boundary_id,
                super::ReplacementBoundaryDraft::new(
                    child_context,
                    subject,
                    fixture.predicate_id,
                    None,
                ),
            )
            .map_err(|error| error.to_string())?;
        let request = |history_space_id, recorded_as_of, schema_mode, operation, world_time| {
            FactQueryRequest {
                recorded_as_of,
                schema_mode,
                operation,
                history_space_id,
                layer_selection: worlddb_core::LayerSelection::BaseOnly,
                subject,
                predicate_id: fixture.predicate_id,
                perspective_scope: worlddb_core::PerspectiveScope::World,
                epistemic_mode: worlddb_core::EpistemicMode::WorldState,
                world_time,
                max_candidates: 100_000,
                max_work_units: 500_000,
                max_results: 20_000,
            }
        };
        let point = WorldTimeSelector::At(WorldTime::from_nanoseconds(
            Timeline::new(fixture.timeline_id),
            0,
        ));

        let historical = manager
            .query_slot(request(
                fixture.history_space_id,
                assertion.revision(),
                SchemaMode::Historical,
                FactQueryOperation::Resolved,
                point,
            ))
            .map_err(|error| format!("historical Resolved query: {error}"))?;
        let FactQueryResult::Resolved(output) = &historical.result else {
            return Err("historical query did not return a Resolved result".to_owned());
        };
        if historical.snapshot_revision != boundary.revision()
            || !matches!(
                output.query().value(),
                ResolutionPreview::Point { resolved_view, .. }
                    if resolved_view.contributors() == [assertion_id]
            )
        {
            return Err("Resolved did not honor its historical RecordedAsOf binding".to_owned());
        }

        let history = manager
            .query_slot(request(
                child_history_space_id,
                boundary.revision(),
                SchemaMode::Historical,
                FactQueryOperation::History,
                WorldTimeSelector::AllTimes,
            ))
            .map_err(|error| format!("raw History query: {error}"))?;
        if !matches!(&history.result, FactQueryResult::History(rows)
            if rows.len() == 3
                && rows.iter().any(|row| row.record_ref == RecordRef::Assertion(assertion_id))
                && rows.iter().any(|row| matches!(&row.record,
                    FactHistoryRecord::Assertion(value)
                        if matches!(value.value(), Value::String(text) if text == "Alice")))
                && rows.iter().any(|row| row.record_ref == RecordRef::Mask(mask_id))
                && rows.iter().any(|row| row.record_ref == RecordRef::ReplacementBoundary(boundary_id)))
        {
            return Err(
                "History did not enumerate the selected slot's three raw records".to_owned(),
            );
        }

        let explain = manager
            .query_slot(request(
                child_history_space_id,
                boundary.revision(),
                SchemaMode::Current,
                FactQueryOperation::Explain,
                point,
            ))
            .map_err(|error| format!("Explain query: {error}"))?;
        if !matches!(&explain.result, FactQueryResult::Explain(output)
        if output.query().value().stages().iter().any(|stage| {
            stage.kind() == worlddb_core::ExplainStageKind::MaskProjection
                && stage.applied_records().contains(&RecordRef::Mask(mask_id))
        }) && output.query().value().stages().iter().any(|stage| {
            stage.kind() == worlddb_core::ExplainStageKind::ReplacementBoundary
                && stage.applied_records().contains(&RecordRef::ReplacementBoundary(boundary_id))
        })) {
            return Err(format!(
                "Explain did not include the applied Mask and Boundary records: {:?}",
                match &explain.result {
                    FactQueryResult::Explain(output) => output.query().value().stages(),
                    _ => &[],
                }
            ));
        }

        let explicit_schema = SchemaRevision::from_published_revision(assertion.revision());
        let explicit = manager
            .query_slot(request(
                fixture.history_space_id,
                assertion.revision(),
                SchemaMode::Explicit(explicit_schema),
                FactQueryOperation::Resolved,
                point,
            ))
            .map_err(|error| format!("Explicit-schema query: {error}"))?;
        if explicit.schema_revision != explicit_schema {
            return Err("Explicit SchemaMode did not bind the selected schema revision".to_owned());
        }

        let denied = build_fixture(true, false)?;
        let denied_manager = Manager::open(denied.layout.clone(), &denied.lock, denied.principal)
            .map_err(|error| error.to_string())?;
        let denied_request = FactQueryRequest {
            recorded_as_of: denied_manager.revision(),
            schema_mode: SchemaMode::Historical,
            operation: FactQueryOperation::Explain,
            history_space_id: denied.history_space_id,
            layer_selection: worlddb_core::LayerSelection::BaseOnly,
            subject: Subject::new(denied.entity_id),
            predicate_id: denied.predicate_id,
            perspective_scope: worlddb_core::PerspectiveScope::World,
            epistemic_mode: worlddb_core::EpistemicMode::WorldState,
            world_time: WorldTimeSelector::At(WorldTime::from_nanoseconds(
                Timeline::new(denied.timeline_id),
                0,
            )),
            max_candidates: 100_000,
            max_work_units: 500_000,
            max_results: 20_000,
        };
        if !matches!(
            denied_manager.query_slot(denied_request),
            Err(FactManagementError::Unauthorized(Capability::QueryExplain))
        ) {
            return Err("Explain without QueryExplain was not rejected".to_owned());
        }
        Ok(())
    }

    #[test]
    fn token_search_denies_field_before_inspecting_assertion_value() -> Result<(), String> {
        let fixture = build_fixture(true, true)?;
        let context = make_context(&fixture)?;
        let assertion_id = id::<worlddb_core::AssertionId>(187)?;
        let assertion = Assertion::new(
            assertion_id,
            AssertionDraft::new(
                context,
                Subject::new(fixture.entity_id),
                fixture.predicate_id,
                Value::Bool(true),
                Polarity::Positive,
                validity(&fixture)?,
            ),
            Revision::GENESIS,
        );
        let record_ref = RecordRef::Assertion(assertion_id);
        let field = FieldSelector::AssertionValue(fixture.predicate_id);
        let policy = SecurityPolicySnapshot::new(
            vec![Principal::new(fixture.principal)],
            vec![],
            vec![],
            vec![
                worlddb_core::CapabilityRule::new(
                    id::<PolicyRuleId>(188)?,
                    PolicySubject::Principal(fixture.principal),
                    CapabilityGrant::new(Capability::AssertionRead, GrantEffect::Allow),
                    PolicyScope::project(),
                ),
                CapabilityRule::new(
                    id::<PolicyRuleId>(189)?,
                    PolicySubject::Principal(fixture.principal),
                    CapabilityGrant::new(Capability::FieldRead, GrantEffect::Deny),
                    PolicyScope::new(
                        Some(fixture.history_space_id),
                        Some(fixture.layer_id),
                        Some(record_ref),
                        Some(field),
                        None,
                    ),
                ),
            ],
        )
        .map_err(|error| error.to_string())?;
        let query = FactExplorerRequest {
            recorded_as_of: Revision::GENESIS,
            schema_mode: SchemaMode::Current,
            history_space_id: fixture.history_space_id,
            layer_selection: worlddb_core::LayerSelection::BaseOnly,
            subject: Subject::new(fixture.entity_id),
            predicate_id: fixture.predicate_id,
            perspective_scope: worlddb_core::PerspectiveScope::World,
            epistemic_mode: worlddb_core::EpistemicMode::WorldState,
            world_time: WorldTimeSelector::AllTimes,
            max_candidates: 10,
            max_work_units: 10,
            max_results: 10,
        };
        let result = token_search_document_if_authorized(
            &assertion,
            fixture.history_space_id,
            &query,
            &[fixture.layer_id],
            field,
            &policy,
            fixture.principal,
        )
        .map_err(|error| error.to_string())?;
        if result.is_some() {
            return Err(
                "a FieldRead-denied assertion reached TokenSearch document construction".to_owned(),
            );
        }
        Ok(())
    }

    #[test]
    fn storage_token_search_omits_assertion_when_field_read_is_denied() -> Result<(), String> {
        use worlddb_core::{CursorStateStore, CursorStoreLimits, QueryHash, SearchMatch};

        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(196)?,
                id::<worlddb_core::AssertionId>(197)?,
                AssertionDraft::new(
                    make_context(&fixture)?,
                    Subject::new(fixture.entity_id),
                    fixture.predicate_id,
                    Value::String("private token".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        drop(manager);

        let mut security = FileSecurityPolicyManager::open(
            fixture.layout.clone(),
            &fixture.lock,
            fixture.principal,
        )
        .map_err(|error| error.to_string())?;
        security
            .add_capability_rule(
                security.revision(),
                id::<OperationId>(198)?,
                id::<PolicyRuleId>(199)?,
                PolicySubject::Principal(fixture.principal),
                Capability::FieldRead,
                GrantEffect::Deny,
            )
            .map_err(|error| error.to_string())?;
        drop(security);

        let manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let query = FactExplorerRequest {
            recorded_as_of: manager.revision(),
            schema_mode: SchemaMode::Current,
            history_space_id: fixture.history_space_id,
            layer_selection: worlddb_core::LayerSelection::BaseOnly,
            subject: Subject::new(fixture.entity_id),
            predicate_id: fixture.predicate_id,
            perspective_scope: worlddb_core::PerspectiveScope::World,
            epistemic_mode: worlddb_core::EpistemicMode::WorldState,
            world_time: WorldTimeSelector::AllTimes,
            max_candidates: 100,
            max_work_units: 100,
            max_results: 100,
        };
        let mut cursors = CursorStateStore::new(
            CursorStoreLimits::new(4, 16_384, 60_000).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let (_, page) = manager
            .start_token_search(
                FactTokenSearchRequest {
                    query,
                    terms: vec!["private".to_owned()],
                    matching: SearchMatch::AllTerms,
                    page_size: 10,
                    query_hash: QueryHash::new([0xa6; 32]),
                },
                &mut cursors,
                1_000,
            )
            .map_err(|error| error.to_string())?;
        if !page.hits.is_empty() {
            return Err("TokenSearch returned a hit after FieldRead was denied".to_owned());
        }
        Ok(())
    }

    #[test]
    fn query_explorer_pages_search_and_returns_complete_graph_and_aggregates() -> Result<(), String>
    {
        use worlddb_core::{
            AggregateResult, CursorStateStore, CursorStoreLimits, GraphCyclePolicy, GraphDirection,
            GraphRelationshipKind, GraphSpec, QueryHash, SearchMatch,
        };

        let fixture = build_fixture_with_correction_rights(true, true, true)?;
        let mut manager = Manager::open(fixture.layout.clone(), &fixture.lock, fixture.principal)
            .map_err(|error| error.to_string())?;
        let context = make_context(&fixture)?;
        let subject = Subject::new(fixture.entity_id);
        let first_id = id::<worlddb_core::AssertionId>(151)?;
        let first = manager
            .create_assertion(
                manager.revision(),
                id::<OperationId>(150)?,
                first_id,
                AssertionDraft::new(
                    context,
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;
        let second_id = id::<worlddb_core::AssertionId>(153)?;
        manager
            .create_assertion(
                first.revision(),
                id::<OperationId>(152)?,
                second_id,
                AssertionDraft::new(
                    context,
                    subject,
                    fixture.predicate_id,
                    Value::String("Alice".to_owned()),
                    Polarity::Positive,
                    validity(&fixture)?,
                ),
                false,
            )
            .map_err(|error| error.to_string())?;

        let explorer = || FactExplorerRequest {
            recorded_as_of: manager.revision(),
            schema_mode: SchemaMode::Current,
            history_space_id: fixture.history_space_id,
            layer_selection: worlddb_core::LayerSelection::BaseOnly,
            subject,
            predicate_id: fixture.predicate_id,
            perspective_scope: worlddb_core::PerspectiveScope::World,
            epistemic_mode: worlddb_core::EpistemicMode::WorldState,
            world_time: WorldTimeSelector::AllTimes,
            max_candidates: 100_000,
            max_work_units: 500_000,
            max_results: 20_000,
        };

        let mut cursors = CursorStateStore::new(
            CursorStoreLimits::new(8, 65_536, 60_000).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let (session, first_page) = manager
            .start_token_search(
                FactTokenSearchRequest {
                    query: explorer(),
                    terms: vec!["Alice".to_owned()],
                    matching: SearchMatch::AllTerms,
                    page_size: 1,
                    query_hash: QueryHash::new([0x5a; 32]),
                },
                &mut cursors,
                100,
            )
            .map_err(|error| format!("start TokenSearch: {error}"))?;
        let next = first_page
            .next_cursor
            .clone()
            .ok_or_else(|| "TokenSearch did not return a continuation for two hits".to_owned())?;
        let second_page = manager
            .continue_token_search(&session, next, &mut cursors, 101)
            .map_err(|error| format!("continue TokenSearch: {error}"))?;
        if first_page.hits.len() != 1
            || second_page.hits.len() != 1
            || second_page.next_cursor.is_some()
        {
            return Err(
                "TokenSearch pagination did not return two complete single-hit pages".to_owned(),
            );
        }

        let graph = manager
            .query_graph(
                explorer(),
                GraphSpec::new(
                    vec![RecordRef::Assertion(first_id)],
                    vec![GraphRelationshipKind::EventCauses],
                    GraphDirection::Both,
                    0,
                    2,
                    2,
                    GraphCyclePolicy::StopAtRepeatedNode,
                )
                .map_err(|error| error.to_string())?,
                102,
            )
            .map_err(|error| format!("Graph traversal: {error}"))?;
        if graph.result.query().value().nodes() != [RecordRef::Assertion(first_id)]
            || !graph.result.query().value().edges().is_empty()
        {
            return Err(
                "zero-depth Graph traversal did not return only its visible root".to_owned(),
            );
        }

        let query_slot = |operation| FactQueryRequest {
            recorded_as_of: manager.revision(),
            schema_mode: SchemaMode::Current,
            operation,
            history_space_id: fixture.history_space_id,
            layer_selection: worlddb_core::LayerSelection::BaseOnly,
            subject,
            predicate_id: fixture.predicate_id,
            perspective_scope: worlddb_core::PerspectiveScope::World,
            epistemic_mode: worlddb_core::EpistemicMode::WorldState,
            world_time: WorldTimeSelector::AllTimes,
            max_candidates: 100_000,
            max_work_units: 500_000,
            max_results: 20_000,
        };
        let count = manager
            .query_slot(query_slot(FactQueryOperation::AggregateCount))
            .map_err(|error| format!("COUNT: {error}"))?;
        let exists = manager
            .query_slot(query_slot(FactQueryOperation::AggregateExists))
            .map_err(|error| format!("EXISTS: {error}"))?;
        let grouped = manager
            .query_slot(query_slot(FactQueryOperation::AggregateByPolarity))
            .map_err(|error| format!("GroupedCount: {error}"))?;
        if !matches!(count.result, FactQueryResult::Aggregate(output)
            if matches!(output.query().value(), AggregateResult::Count(2)))
            || !matches!(exists.result, FactQueryResult::Aggregate(output)
                if matches!(output.query().value(), AggregateResult::Exists(true)))
            || !matches!(grouped.result, FactQueryResult::Aggregate(output)
                if matches!(output.query().value(), AggregateResult::GroupedCount(groups)
                    if groups.len() == 1 && groups.first().is_some_and(|group| group.count() == 2)))
        {
            return Err(
                "COUNT/EXISTS/GroupedCount did not return complete resolved-contributor results"
                    .to_owned(),
            );
        }
        Ok(())
    }
}
