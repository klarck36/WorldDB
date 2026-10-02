//! Productive, snapshot-bound Raw, Resolved, and Explain query entry points.

use std::collections::BTreeSet;
use std::fmt;

use crate::archive_projection::ArchiveHistoryReferenceModel;
use crate::assertion_point_index::{
    AssertionPointHistoryIndex, AssertionPointIndexError, AssertionPointQuery,
};
use crate::candidate_scan::{
    AssertionCandidate, AssertionCandidateQuery, AssertionCandidateSecurityContext,
    AssertionHistoryRecord, AssertionPointCandidateFilter, CandidateScanError,
    full_scan_authorized_point_assertion_candidates, indexed_authorized_assertion_candidates,
};
use crate::context::{ContextError, ContextKey};
use crate::history_model::HistorySpaceReferenceModel;
use crate::ids::{AssertionId, MaskId, ReplacementBoundaryId};
use crate::index_generation::{
    FullScanBudget, IndexAccessPlan, IndexAvailability, IndexBuildVersion, IndexFallbackReason,
    IndexFamily, IndexFormatVersion, IndexQueryRequirement, IndexSchemaVersion, plan_index_access,
};
use crate::layers::LayerSchemaSnapshot;
use crate::mask_projection::{
    AssertionMaskContext, AuthorizedAssertionMaskHistory,
    apply_authorized_assertion_masks_with_trace,
};
use crate::masks::{
    ReplacementBoundary, ReplacementBoundaryRetraction, ReplacementBoundaryValidityClosure,
};
use crate::multi_value_resolution::{
    MultiValueReplaceContext, MultiValueResolutionError, MultiValueSlot,
    ReplacementBoundaryHistory, resolve_multi_value_overlay,
    resolve_multi_value_replace_with_trace,
};
use crate::query_context::{BudgetDimension, QueryContext, WorldTimeSelector};
use crate::query_ports::{
    OwnedQueryResult, QueryPortError, bind_authorized_explain, bind_authorized_resolved_view,
    full_scan_owned_authorized_raw_history,
};
use crate::query_search::{
    QuerySearchError, SearchDocument, SearchHit, SearchSpec, full_scan_token_search,
};
use crate::record_refs::RecordRef;
use crate::reference_query::{
    ExplainStage, ExplainStageError, ExplainStageKind, RawHistoryError, RawHistoryRow,
    ReferenceExplain, ReferenceExplainError, ResolvedView, ResolvedViewError,
};
use crate::schema::{PredicateDefinition, ResolutionPolicy};
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicyHistory,
    SecurityPolicyHistoryError, SecurityPolicySnapshot,
};
use crate::single_value_resolution::{
    SingleValueResolutionError, SingleValueSlot, resolve_single_value_replace,
};
use crate::values::Value;

/// Optional typed Assertion point index plus its durable family metadata.
#[derive(Clone, Copy)]
pub struct AssertionPointIndexAccess<'a> {
    availability: IndexAvailability,
    index: Option<&'a AssertionPointHistoryIndex>,
}

impl<'a> AssertionPointIndexAccess<'a> {
    /// Binds the validated generation descriptor to the decoded typed index payload.
    #[must_use]
    pub const fn new(
        availability: IndexAvailability,
        index: Option<&'a AssertionPointHistoryIndex>,
    ) -> Self {
        Self {
            availability,
            index,
        }
    }

    /// Query path when no Assertion point generation is available.
    #[must_use]
    pub const fn missing() -> Self {
        Self::new(IndexAvailability::Missing, None)
    }

    /// Query path when loading or validating the persisted generation failed.
    #[must_use]
    pub const fn corrupt() -> Self {
        Self::new(IndexAvailability::Corrupt, None)
    }
}

/// Complete boundary and lifecycle inputs for a pinned point query.
pub struct ReplacementBoundarySource<'a> {
    boundaries: &'a [ReplacementBoundary],
    closures: &'a [ReplacementBoundaryValidityClosure],
    retractions: &'a [ReplacementBoundaryRetraction],
    archive_visible_boundaries: &'a BTreeSet<ReplacementBoundaryId>,
}

impl<'a> ReplacementBoundarySource<'a> {
    /// Binds retained boundary records and the archive projection for the query revision.
    #[must_use]
    pub const fn new(
        boundaries: &'a [ReplacementBoundary],
        closures: &'a [ReplacementBoundaryValidityClosure],
        retractions: &'a [ReplacementBoundaryRetraction],
        archive_visible_boundaries: &'a BTreeSet<ReplacementBoundaryId>,
    ) -> Self {
        Self {
            boundaries,
            closures,
            retractions,
            archive_visible_boundaries,
        }
    }
}

/// Immutable assertion-side query sources pinned by the host to one data view.
#[derive(Clone, Copy)]
pub struct AssertionQueryStore<'a> {
    history: &'a HistorySpaceReferenceModel<AssertionHistoryRecord>,
    archive: &'a ArchiveHistoryReferenceModel,
    layers: &'a LayerSchemaSnapshot,
    policies: &'a SecurityPolicyHistory,
}

impl<'a> AssertionQueryStore<'a> {
    /// Binds the retained history, archive projection, layer schema, and policy history.
    #[must_use]
    pub const fn new(
        history: &'a HistorySpaceReferenceModel<AssertionHistoryRecord>,
        archive: &'a ArchiveHistoryReferenceModel,
        layers: &'a LayerSchemaSnapshot,
        policies: &'a SecurityPolicyHistory,
    ) -> Self {
        Self {
            history,
            archive,
            layers,
            policies,
        }
    }
}

/// Per-request semantic and index inputs for one Assertion point query.
pub struct AssertionPointRequest<'a> {
    context: &'a QueryContext,
    masks: AuthorizedAssertionMaskHistory<'a>,
    boundaries: ReplacementBoundarySource<'a>,
    point_index: AssertionPointIndexAccess<'a>,
    scan_budget: FullScanBudget,
}

impl<'a> AssertionPointRequest<'a> {
    /// Binds the complete query context and immutable mask/boundary/index snapshots.
    #[must_use]
    pub const fn new(
        context: &'a QueryContext,
        masks: AuthorizedAssertionMaskHistory<'a>,
        boundaries: ReplacementBoundarySource<'a>,
        point_index: AssertionPointIndexAccess<'a>,
        scan_budget: FullScanBudget,
    ) -> Self {
        Self {
            context,
            masks,
            boundaries,
            point_index,
            scan_budget,
        }
    }
}

/// How an owned result was produced. The path never changes semantic ordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryExecutionPath {
    /// Raw HistorySpace enumeration or a caller-approved full-scan fallback.
    FullScan,
    /// Compatible, complete point-index generation supplied the assertion identities.
    Indexed { generation_id: u64 },
    /// Index metadata was absent or incompatible, so a complete scan was selected.
    IndexFallback { reason: IndexFallbackReason },
}

/// An owned M3 query result together with its selected execution path.
#[derive(Clone, Debug)]
pub struct QueryEngineOutput<T: 'static> {
    query: OwnedQueryResult<T>,
    path: QueryExecutionPath,
}

impl<T: 'static> QueryEngineOutput<T> {
    /// Result payload and complete query/security binding.
    #[must_use]
    pub const fn query(&self) -> &OwnedQueryResult<T> {
        &self.query
    }

    /// Index or scan path chosen before execution.
    #[must_use]
    pub const fn path(&self) -> QueryExecutionPath {
        self.path
    }
}

/// Raw, resolved, and Explain entry points over the M2 reference oracles and M3 ports.
pub struct ProductiveQueryEngine;

impl ProductiveQueryEngine {
    /// Runs authorized RawHistory enumeration. The current index families do not
    /// provide a complete HistorySpace enumeration, so this operation uses a scan.
    pub fn raw_history<T: Clone + 'static>(
        history: &HistorySpaceReferenceModel<T>,
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
        scan_budget: FullScanBudget,
        record_ref: impl Fn(&T) -> RecordRef,
    ) -> Result<QueryEngineOutput<Vec<RawHistoryRow<T>>>, QueryEngineError> {
        ensure_active(context)?;
        require_full_scan_budget(scan_budget)?;
        let query = full_scan_owned_authorized_raw_history(history, context, policies, record_ref)?;
        enforce_result_limit(context, query.value().len())?;
        ensure_active(context)?;
        Ok(QueryEngineOutput {
            query,
            path: QueryExecutionPath::FullScan,
        })
    }

    /// Executes deterministic TokenSearch through the M3 owned-result port.
    /// The current index families do not contain text postings, so token search
    /// uses the authorized full-scan implementation. FullTextSearch remains an
    /// optional, unsupported operation and is never substituted by this method.
    pub fn token_search(
        documents: &[SearchDocument],
        spec: &SearchSpec,
        schema_text_fields: &[FieldSelector],
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
    ) -> Result<QueryEngineOutput<Vec<SearchHit>>, QueryEngineError> {
        ensure_active(context)?;
        let query = full_scan_token_search(documents, spec, schema_text_fields, context, policies)?;
        ensure_active(context)?;
        Ok(QueryEngineOutput {
            query,
            path: QueryExecutionPath::FullScan,
        })
    }

    /// Resolves one Assertion subject/Predicate point using a compatible index or
    /// the complete authorized M2 scan. Mask and boundary records are security-filtered
    /// before they can affect the owned outcome.
    pub fn resolved_point<F>(
        store: AssertionQueryStore<'_>,
        request: AssertionPointRequest<'_>,
        slot: MultiValueSlot,
        predicate: &PredicateDefinition,
        temporal_value_equal: F,
    ) -> Result<QueryEngineOutput<ResolvedView>, QueryEngineError>
    where
        F: FnMut(&Value, &Value) -> Result<bool, ()>,
    {
        let context = request.context;
        let policies = store.policies;
        let execution = execute_point(store, request, slot, predicate, temporal_value_equal)?;
        let query = bind_authorized_resolved_view(
            context,
            policies,
            &execution.candidate_ids,
            execution.resolved_view,
        )?;
        ensure_active(context)?;
        Ok(QueryEngineOutput {
            query,
            path: execution.path,
        })
    }

    /// Runs the same resolved pipeline and binds a complete, visibility-checked
    /// M2 Explain trace. QueryExplain is required in addition to QueryResolve.
    pub fn explain_point<F>(
        store: AssertionQueryStore<'_>,
        request: AssertionPointRequest<'_>,
        slot: MultiValueSlot,
        predicate: &PredicateDefinition,
        temporal_value_equal: F,
    ) -> Result<QueryEngineOutput<ReferenceExplain>, QueryEngineError>
    where
        F: FnMut(&Value, &Value) -> Result<bool, ()>,
    {
        let context = request.context;
        let policies = store.policies;
        ensure_active(context)?;
        let security = policies.resolve(context)?;
        if security.snapshot().authorize(
            context.security().principal_id(),
            Capability::QueryExplain,
            PolicyTarget::default(),
        ) != AuthorizationDecision::Allow
        {
            return Err(QueryEngineError::ExplainDenied);
        }
        let execution = execute_point(store, request, slot, predicate, temporal_value_equal)?;
        let mut stages = vec![ExplainStage::new(
            ExplainStageKind::CandidateScan,
            Vec::new(),
            execution.candidate_ids.clone(),
            Vec::new(),
        )?];
        let mask_records = execution
            .applied_mask_ids
            .iter()
            .copied()
            .map(RecordRef::Mask)
            .collect::<Vec<_>>();
        stages.push(ExplainStage::new(
            ExplainStageKind::MaskProjection,
            execution.candidate_ids.clone(),
            execution.masked_candidate_ids.clone(),
            mask_records,
        )?);
        let mut final_stage_input = execution.masked_candidate_ids.clone();
        if let Some((boundary_candidate_ids, applied_boundary_ids)) = execution.boundary_projection
        {
            let boundary_records = applied_boundary_ids
                .into_iter()
                .map(RecordRef::ReplacementBoundary)
                .collect::<Vec<_>>();
            stages.push(ExplainStage::new(
                ExplainStageKind::ReplacementBoundary,
                final_stage_input,
                boundary_candidate_ids.clone(),
                boundary_records,
            )?);
            final_stage_input = boundary_candidate_ids;
        }
        stages.push(ExplainStage::new(
            ExplainStageKind::Resolution,
            final_stage_input,
            execution.resolved_view.contributors().to_vec(),
            Vec::new(),
        )?);
        let explain =
            ReferenceExplain::new(context.schema_binding(), stages, execution.resolved_view)?;
        let visible_records = explain
            .stages()
            .iter()
            .flat_map(|stage| stage.applied_records().iter().copied())
            .collect::<Vec<_>>();
        let query = bind_authorized_explain(
            context,
            policies,
            &execution.candidate_ids,
            &visible_records,
            explain,
        )?;
        ensure_active(context)?;
        Ok(QueryEngineOutput {
            query,
            path: execution.path,
        })
    }
}

struct PointExecution {
    candidate_ids: Vec<AssertionId>,
    masked_candidate_ids: Vec<AssertionId>,
    applied_mask_ids: Vec<MaskId>,
    boundary_projection: Option<(Vec<AssertionId>, Vec<ReplacementBoundaryId>)>,
    resolved_view: ResolvedView,
    path: QueryExecutionPath,
}

fn execute_point<F>(
    store: AssertionQueryStore<'_>,
    request: AssertionPointRequest<'_>,
    slot: MultiValueSlot,
    predicate: &PredicateDefinition,
    mut temporal_value_equal: F,
) -> Result<PointExecution, QueryEngineError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    let AssertionQueryStore {
        history,
        archive,
        layers,
        policies,
    } = store;
    let AssertionPointRequest {
        context,
        masks,
        boundaries: boundary_source,
        point_index,
        scan_budget,
    } = request;
    ensure_active(context)?;
    if predicate.predicate_id() != slot.predicate_id() {
        return Err(QueryEngineError::Resolution(
            ResolutionFailure::PredicateMismatch,
        ));
    }
    let WorldTimeSelector::At(world_time) = context.world_time() else {
        return Err(QueryEngineError::AllTimesResolutionUnavailable);
    };
    let query = AssertionCandidateQuery::new(
        context.history_space(),
        context.layers().requested().clone(),
        context.perspective(),
        context.epistemic_mode(),
        context.recorded_as_of(),
        world_time,
    );
    let security = policies.resolve(context)?;
    let access_plan = point_access_plan(context, point_index, scan_budget)?;
    let (candidates, path) = match access_plan {
        IndexAccessPlan::Indexed { generation_id } => {
            let Some(index) = point_index.index else {
                return Err(QueryEngineError::IndexPayloadUnavailable);
            };
            match collect_point_index_ids(index, context, slot) {
                Ok(indexed_ids) => (
                    indexed_authorized_assertion_candidates(
                        history,
                        archive,
                        &query,
                        layers,
                        AssertionCandidateSecurityContext::new(security.snapshot(), context),
                        AssertionPointCandidateFilter::indexed(
                            slot.subject(),
                            slot.predicate_id(),
                            &indexed_ids,
                        ),
                    )?,
                    QueryExecutionPath::Indexed { generation_id },
                ),
                Err(_) => {
                    let fallback = plan_index_access(
                        IndexAvailability::Corrupt,
                        point_requirement(context)?,
                        scan_budget,
                    );
                    let fallback_reason = match fallback {
                        IndexAccessPlan::FullScan { reason } => reason,
                        IndexAccessPlan::BudgetExceeded { dimension, .. } => {
                            return Err(QueryEngineError::FullScanBudgetExceeded(dimension));
                        }
                        IndexAccessPlan::Indexed { .. } => {
                            return Err(QueryEngineError::IndexPayloadUnavailable);
                        }
                    };
                    (
                        full_scan_authorized_point_assertion_candidates(
                            history,
                            archive,
                            &query,
                            layers,
                            AssertionCandidateSecurityContext::new(security.snapshot(), context),
                            AssertionPointCandidateFilter::full_scan(
                                slot.subject(),
                                slot.predicate_id(),
                            ),
                        )?,
                        QueryExecutionPath::IndexFallback {
                            reason: fallback_reason,
                        },
                    )
                }
            }
        }
        IndexAccessPlan::FullScan { reason } => (
            full_scan_authorized_point_assertion_candidates(
                history,
                archive,
                &query,
                layers,
                AssertionCandidateSecurityContext::new(security.snapshot(), context),
                AssertionPointCandidateFilter::full_scan(slot.subject(), slot.predicate_id()),
            )?,
            QueryExecutionPath::IndexFallback { reason },
        ),
        IndexAccessPlan::BudgetExceeded { dimension, .. } => {
            return Err(QueryEngineError::FullScanBudgetExceeded(dimension));
        }
    };
    enforce_candidate_limit(context, candidates.len())?;
    ensure_active(context)?;
    let visible_candidate_ids = candidate_ids(&candidates);
    let mask_context = AssertionMaskContext::new(
        context.recorded_as_of(),
        world_time,
        history.catalog(),
        layers,
    )
    .with_query_history_space(context.history_space());
    let mask_projection = apply_authorized_assertion_masks_with_trace(
        &candidates,
        masks,
        mask_context,
        security.snapshot(),
        context,
        &mut temporal_value_equal,
    )?;
    let masked_candidate_ids = candidate_ids(mask_projection.candidates());
    let applied_mask_ids = mask_projection.applied_mask_ids().to_vec();

    let mut boundary_projection = None;
    let resolved_view = match predicate.resolution_policy() {
        ResolutionPolicy::SingleValueReplace => {
            ResolvedView::from_single(resolve_single_value_replace(
                mask_projection.candidates(),
                SingleValueSlot::new(slot.subject(), slot.predicate_id()),
                predicate,
                &mut temporal_value_equal,
            )?)?
        }
        ResolutionPolicy::MultiValueOverlay => {
            ResolvedView::from_multi(resolve_multi_value_overlay(
                mask_projection.candidates(),
                slot,
                predicate,
                &mut temporal_value_equal,
            )?)?
        }
        ResolutionPolicy::MultiValueReplace => {
            let authorized =
                authorize_boundary_source(boundary_source, security.snapshot(), context);
            let history_view = ReplacementBoundaryHistory::new(
                &authorized.boundaries,
                &authorized.closures,
                &authorized.retractions,
                &authorized.archive_visible,
            );
            let selected_layers = context
                .layers()
                .resolved()
                .as_slice()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let query_layer = *selected_layers
                .iter()
                .next()
                .ok_or(QueryEngineError::EmptyLayerSelection)?;
            let query_context = ContextKey::new(
                context.history_space(),
                query_layer,
                context.perspective(),
                context.epistemic_mode(),
            )?;
            let replace_context = MultiValueReplaceContext::new(
                query_context,
                selected_layers,
                context.recorded_as_of(),
                world_time,
                history.catalog(),
                layers,
            );
            let trace = resolve_multi_value_replace_with_trace(
                mask_projection.candidates(),
                history_view,
                slot,
                predicate,
                replace_context,
                &mut temporal_value_equal,
            )?;
            boundary_projection = Some((
                trace.surviving_assertion_ids().to_vec(),
                trace.applied_boundary_ids().to_vec(),
            ));
            ResolvedView::from_multi(trace.outcome().clone())?
        }
    };
    enforce_result_limit(context, 1)?;
    ensure_active(context)?;
    Ok(PointExecution {
        candidate_ids: visible_candidate_ids,
        masked_candidate_ids,
        applied_mask_ids,
        boundary_projection,
        resolved_view,
        path,
    })
}

fn point_access_plan(
    context: &QueryContext,
    input: AssertionPointIndexAccess<'_>,
    scan_budget: FullScanBudget,
) -> Result<IndexAccessPlan, QueryEngineError> {
    let availability = match input.availability {
        IndexAvailability::Available(metadata) => {
            if input.index.is_none_or(|index| {
                index.indexed_through() != metadata.coverage().through_inclusive()
            }) {
                IndexAvailability::Corrupt
            } else {
                IndexAvailability::Available(metadata)
            }
        }
        other => other,
    };
    Ok(plan_index_access(
        availability,
        point_requirement(context)?,
        scan_budget,
    ))
}

fn point_requirement(context: &QueryContext) -> Result<IndexQueryRequirement, QueryEngineError> {
    let build_version =
        IndexBuildVersion::new(1).map_err(|_| QueryEngineError::IndexConfigurationUnavailable)?;
    Ok(IndexQueryRequirement::new(
        IndexFamily::AssertionPointHistory,
        context.recorded_as_of().revision(),
        IndexSchemaVersion::V1_0,
        IndexFormatVersion::V1_0,
        build_version,
    ))
}

fn collect_point_index_ids(
    index: &AssertionPointHistoryIndex,
    context: &QueryContext,
    slot: MultiValueSlot,
) -> Result<BTreeSet<AssertionId>, AssertionPointIndexError> {
    let mut assertion_ids = BTreeSet::new();
    for layer_id in context.layers().resolved().as_slice() {
        let query_context = ContextKey::new(
            context.history_space(),
            *layer_id,
            context.perspective(),
            context.epistemic_mode(),
        )
        .map_err(|_| AssertionPointIndexError::UnknownHistorySpace)?;
        let query = AssertionPointQuery::new(
            query_context,
            slot.subject(),
            slot.predicate_id(),
            context.recorded_as_of().revision(),
        );
        assertion_ids.extend(
            index
                .point(query)?
                .into_iter()
                .map(|hit| hit.assertion().id()),
        );
    }
    Ok(assertion_ids)
}

fn candidate_ids(candidates: &[AssertionCandidate]) -> Vec<AssertionId> {
    candidates
        .iter()
        .map(|candidate| candidate.assertion().id())
        .collect()
}

struct AuthorizedBoundaries {
    boundaries: Vec<ReplacementBoundary>,
    closures: Vec<ReplacementBoundaryValidityClosure>,
    retractions: Vec<ReplacementBoundaryRetraction>,
    archive_visible: BTreeSet<ReplacementBoundaryId>,
}

fn authorize_boundary_source(
    source: ReplacementBoundarySource<'_>,
    policy: &SecurityPolicySnapshot,
    context: &QueryContext,
) -> AuthorizedBoundaries {
    let principal = context.security().principal_id();
    let boundaries = source
        .boundaries
        .iter()
        .filter(|boundary| boundary_is_authorized(policy, principal, boundary))
        .cloned()
        .collect::<Vec<_>>();
    let visible_ids = boundaries
        .iter()
        .map(ReplacementBoundary::id)
        .collect::<BTreeSet<_>>();
    let closures = source
        .closures
        .iter()
        .filter(|closure| visible_ids.contains(&closure.replacement_boundary_id()))
        .copied()
        .collect();
    let retractions = source
        .retractions
        .iter()
        .filter(|retraction| visible_ids.contains(&retraction.replacement_boundary_id()))
        .cloned()
        .collect();
    let archive_visible = source
        .archive_visible_boundaries
        .intersection(&visible_ids)
        .copied()
        .collect();
    AuthorizedBoundaries {
        boundaries,
        closures,
        retractions,
        archive_visible,
    }
}

fn boundary_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal_id: crate::ids::PrincipalId,
    boundary: &ReplacementBoundary,
) -> bool {
    let record = RecordRef::ReplacementBoundary(boundary.id());
    let target = PolicyTarget::new(
        Some(boundary.context().history_space_id()),
        Some(boundary.context().layer_id()),
        Some(record),
        None,
        None,
    );
    [
        Capability::HistorySpaceRead,
        Capability::LayerRead,
        Capability::ReplacementBoundaryRead,
    ]
    .into_iter()
    .all(|capability| {
        policy.authorize(principal_id, capability, target) == AuthorizationDecision::Allow
    }) && [
        FieldSelector::ReplacementBoundarySubject,
        FieldSelector::ReplacementBoundaryPredicate,
        FieldSelector::ReplacementBoundaryValidity,
    ]
    .into_iter()
    .all(|field| {
        policy.authorize(
            principal_id,
            Capability::FieldRead,
            PolicyTarget::new(
                Some(boundary.context().history_space_id()),
                Some(boundary.context().layer_id()),
                Some(record),
                Some(field),
                None,
            ),
        ) == AuthorizationDecision::Allow
    })
}

fn enforce_candidate_limit(context: &QueryContext, count: usize) -> Result<(), QueryEngineError> {
    if u64::try_from(count).unwrap_or(u64::MAX) > context.budget().max_candidates().get() {
        Err(QueryEngineError::BudgetExceeded(
            BudgetDimension::Candidates,
        ))
    } else {
        Ok(())
    }
}

fn enforce_result_limit(context: &QueryContext, count: usize) -> Result<(), QueryEngineError> {
    if u64::try_from(count).unwrap_or(u64::MAX) > context.budget().max_results().get() {
        Err(QueryEngineError::BudgetExceeded(BudgetDimension::Results))
    } else {
        Ok(())
    }
}

fn require_full_scan_budget(budget: FullScanBudget) -> Result<(), QueryEngineError> {
    match budget {
        FullScanBudget::Available => Ok(()),
        FullScanBudget::Exceeded(dimension) => {
            Err(QueryEngineError::FullScanBudgetExceeded(dimension))
        }
    }
}

fn ensure_active(context: &QueryContext) -> Result<(), QueryEngineError> {
    if context.cancellation().is_cancelled() {
        Err(QueryEngineError::Cancelled)
    } else {
        Ok(())
    }
}

/// Safe, ID-free errors from the productive query boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryEngineError {
    Cancelled,
    ExplainDenied,
    AllTimesResolutionUnavailable,
    EmptyLayerSelection,
    IndexPayloadUnavailable,
    IndexConfigurationUnavailable,
    BudgetExceeded(BudgetDimension),
    FullScanBudgetExceeded(BudgetDimension),
    Resolution(ResolutionFailure),
    RawHistory,
    SecurityHistory,
    CandidateHistory,
    Index,
    Context,
    MaskProjection,
    SingleValueResolution,
    MultiValueResolution,
    QueryBinding,
    Search(QuerySearchError),
    Explain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionFailure {
    PredicateMismatch,
}

impl fmt::Display for QueryEngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "query was cancelled",
            Self::ExplainDenied => "query Explain capability is denied",
            Self::AllTimesResolutionUnavailable => {
                "point resolution requires a concrete WorldTime selector"
            }
            Self::EmptyLayerSelection => "query layer selection is empty",
            Self::IndexPayloadUnavailable => "compatible index payload is unavailable",
            Self::IndexConfigurationUnavailable => "index configuration is unavailable",
            Self::BudgetExceeded(_) | Self::FullScanBudgetExceeded(_) => {
                "query budget does not allow a complete result"
            }
            Self::Resolution(_) => "query resolution schema does not match the selected slot",
            Self::RawHistory => "authorized raw history could not be read",
            Self::SecurityHistory => "query authorization history could not be resolved",
            Self::CandidateHistory => "query candidate history is inconsistent",
            Self::Index => "query index could not be used safely",
            Self::Context => "query context is invalid for the selected history",
            Self::MaskProjection => "query mask projection could not be completed",
            Self::SingleValueResolution => "single-value resolution could not be completed",
            Self::MultiValueResolution => "multi-value resolution could not be completed",
            Self::QueryBinding => "query result does not match its pinned context",
            Self::Search(_) => "query search could not be completed",
            Self::Explain => "query Explain trace could not be constructed",
        })
    }
}

impl std::error::Error for QueryEngineError {}

macro_rules! error_from {
    ($source:ty, $variant:ident) => {
        impl From<$source> for QueryEngineError {
            fn from(_: $source) -> Self {
                Self::$variant
            }
        }
    };
}

error_from!(RawHistoryError, RawHistory);
error_from!(SecurityPolicyHistoryError, SecurityHistory);
error_from!(CandidateScanError, CandidateHistory);
error_from!(AssertionPointIndexError, Index);
error_from!(ContextError, Context);
error_from!(crate::mask_projection::MaskProjectionError, MaskProjection);
error_from!(SingleValueResolutionError, SingleValueResolution);
error_from!(MultiValueResolutionError, MultiValueResolution);
error_from!(QueryPortError, QueryBinding);
error_from!(ExplainStageError, Explain);
error_from!(ReferenceExplainError, Explain);
error_from!(ResolvedViewError, Explain);

impl From<QuerySearchError> for QueryEngineError {
    fn from(error: QuerySearchError) -> Self {
        Self::Search(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::ArchiveTargetRef;
    use crate::archive_projection::ArchiveTargetRecord;
    use crate::assertions::{Assertion, AssertionDraft, Polarity, Subject};
    use crate::candidate_scan::full_scan_authorized_assertion_candidates;
    use crate::catalog::HistorySpaceDefinition;
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{
        AssertionId, DomainId, EntityId, HistorySpaceId, LayerId, MaskId, PredicateId, PrincipalId,
        ReplacementBoundaryId, Revision, SchemaRevision, SecurityEpoch, SnapshotId, TimelineId,
    };
    use crate::index_generation::{IndexGenerationMetadata, IndexRevisionCoverage};
    use crate::layers::{LayerDefinition, LayerSelection};
    use crate::masks::{Mask, MaskSelector, ReplacementBoundary};
    use crate::query_context::{
        AuthorizationMode, CancellationToken, QueryBudget, QueryBudgetLimits, QueryContextInput,
        SecurityContext, ValidatedLayerSelection,
    };
    use crate::record_refs::SnapshotRef;
    use crate::reference_query::{HistoricalQueryBinding, ResolvedView};
    use crate::schema::{
        Cardinality, ConstraintSet, EntityTypeConstraint, Lifecycle, PredicateDefinitionSpec,
        ResolutionPolicy, ValueKind,
    };
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::security::{
        CapabilityGrant, CapabilityRule, GrantEffect, PolicyBundle, PolicyScope, PolicySubject,
        Principal, RoleAssignment, RoleDefinition, SecurityPolicyVersion,
    };
    use crate::temporal::{AssertionValidity, RecordedAsOf, TimeInterval, Timeline, WorldTime};
    use crate::values::{Symbol, Value};
    use std::error::Error;
    use std::fmt;

    type TestResult<T = ()> = Result<T, TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(crate::ids::IdValidationError),
        Revision(crate::ids::RevisionError),
        Symbol(crate::values::SymbolError),
        Layer(crate::layers::LayerSchemaError),
        Schema(crate::schema::SchemaDefinitionError),
        SchemaHistory(crate::schema_history::SchemaHistoryError),
        Catalog(crate::catalog::HistorySpaceError),
        QueryBudget(crate::query_context::QueryBudgetError),
        QueryContext(crate::query_context::QueryContextError),
        PolicyBundle(crate::security::PolicyBundleError),
        RoleDefinition(crate::security::RoleDefinitionError),
        Security(crate::security::SecurityPolicyError),
        SecurityHistory(crate::security::SecurityPolicyHistoryError),
        History(crate::history_model::HistorySpaceModelError),
        Assertion(crate::assertions::AssertionRecordError),
        Archive(crate::archive_projection::ArchiveProjectionError),
        Index(crate::assertion_point_index::AssertionPointIndexError),
        IndexMetadata(crate::index_generation::IndexMetadataError),
        Temporal(crate::temporal::TemporalError),
        Context(crate::context::ContextError),
        Mask(crate::masks::MaskRecordError),
        Boundary(crate::masks::ReplacementBoundaryError),
        Candidate(crate::candidate_scan::CandidateScanError),
        Resolution(crate::single_value_resolution::SingleValueResolutionError),
        ResolvedView(crate::reference_query::ResolvedViewError),
        QueryEngine(QueryEngineError),
        Io(std::io::Error),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match Error::source(self) {
                Some(source) => write!(formatter, "query engine test setup failed: {source}"),
                None => formatter.write_str("query engine test setup failed"),
            }
        }
    }

    impl Error for TestError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match self {
                Self::Id(error) => Some(error),
                Self::Revision(error) => Some(error),
                Self::Symbol(error) => Some(error),
                Self::Layer(error) => Some(error),
                Self::Schema(error) => Some(error),
                Self::SchemaHistory(error) => Some(error),
                Self::Catalog(error) => Some(error),
                Self::QueryBudget(error) => Some(error),
                Self::QueryContext(error) => Some(error),
                Self::PolicyBundle(error) => Some(error),
                Self::RoleDefinition(error) => Some(error),
                Self::Security(error) => Some(error),
                Self::SecurityHistory(error) => Some(error),
                Self::History(error) => Some(error),
                Self::Assertion(error) => Some(error),
                Self::Archive(error) => Some(error),
                Self::Index(error) => Some(error),
                Self::IndexMetadata(error) => Some(error),
                Self::Temporal(error) => Some(error),
                Self::Context(error) => Some(error),
                Self::Mask(error) => Some(error),
                Self::Boundary(error) => Some(error),
                Self::Candidate(error) => Some(error),
                Self::Resolution(error) => Some(error),
                Self::ResolvedView(error) => Some(error),
                Self::QueryEngine(error) => Some(error),
                Self::Io(error) => Some(error),
            }
        }
    }

    macro_rules! test_error_from {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    test_error_from!(crate::ids::IdValidationError, Id);
    test_error_from!(crate::ids::RevisionError, Revision);
    test_error_from!(crate::values::SymbolError, Symbol);
    test_error_from!(crate::layers::LayerSchemaError, Layer);
    test_error_from!(crate::schema::SchemaDefinitionError, Schema);
    test_error_from!(crate::schema_history::SchemaHistoryError, SchemaHistory);
    test_error_from!(crate::catalog::HistorySpaceError, Catalog);
    test_error_from!(crate::query_context::QueryBudgetError, QueryBudget);
    test_error_from!(crate::query_context::QueryContextError, QueryContext);
    test_error_from!(crate::security::PolicyBundleError, PolicyBundle);
    test_error_from!(crate::security::RoleDefinitionError, RoleDefinition);
    test_error_from!(crate::security::SecurityPolicyError, Security);
    test_error_from!(crate::security::SecurityPolicyHistoryError, SecurityHistory);
    test_error_from!(crate::history_model::HistorySpaceModelError, History);
    test_error_from!(crate::assertions::AssertionRecordError, Assertion);
    test_error_from!(crate::archive_projection::ArchiveProjectionError, Archive);
    test_error_from!(
        crate::assertion_point_index::AssertionPointIndexError,
        Index
    );
    test_error_from!(crate::index_generation::IndexMetadataError, IndexMetadata);
    test_error_from!(crate::temporal::TemporalError, Temporal);
    test_error_from!(crate::context::ContextError, Context);
    test_error_from!(crate::masks::MaskRecordError, Mask);
    test_error_from!(crate::masks::ReplacementBoundaryError, Boundary);
    test_error_from!(crate::candidate_scan::CandidateScanError, Candidate);
    test_error_from!(
        crate::single_value_resolution::SingleValueResolutionError,
        Resolution
    );
    test_error_from!(crate::reference_query::ResolvedViewError, ResolvedView);
    test_error_from!(QueryEngineError, QueryEngine);
    test_error_from!(std::io::Error, Io);

    struct Fixture {
        history: HistorySpaceReferenceModel<AssertionHistoryRecord>,
        archive: ArchiveHistoryReferenceModel,
        layers: LayerSchemaSnapshot,
        policies: SecurityPolicyHistory,
        hidden_policies: SecurityPolicyHistory,
        context: QueryContext,
        predicate: PredicateDefinition,
        slot: MultiValueSlot,
        multi_predicate: PredicateDefinition,
        multi_slot: MultiValueSlot,
        index: AssertionPointHistoryIndex,
        metadata: crate::IndexGenerationMetadata,
        visible_id: AssertionId,
        second_visible_id: AssertionId,
        hidden_id: AssertionId,
    }

    fn id<T: DomainId>(tail: u8) -> TestResult<T> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        Ok(T::try_from_bytes(bytes)?)
    }

    fn revision(value: u64) -> TestResult<Revision> {
        Ok(Revision::new(value)?)
    }

    fn fixture() -> TestResult<Fixture> {
        let query_revision = revision(2)?;
        let revision = revision(1)?;
        let root = id::<HistorySpaceId>(1)?;
        let child = id::<HistorySpaceId>(14)?;
        let base = id::<LayerId>(2)?;
        let predicate_id = id::<PredicateId>(3)?;
        let multi_predicate_id = id::<PredicateId>(15)?;
        let principal_id = id::<PrincipalId>(4)?;
        let timeline = Timeline::new(id::<TimelineId>(5)?);
        let subject = Subject::new(id::<EntityId>(6)?);
        let schema_revision = SchemaRevision::from_published_revision(revision);
        let layers_v1 = LayerSchemaSnapshot::new(
            schema_revision,
            vec![LayerDefinition::new(
                base,
                Symbol::new("base")?,
                None,
                0,
                Lifecycle::Active,
                schema_revision,
            )],
            base,
        )?;
        let predicate = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol: Symbol::new("answer")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints: ConstraintSet::unconstrained(),
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: revision,
        })?;
        let multi_predicate = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: multi_predicate_id,
            symbol: Symbol::new("labels")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy: ResolutionPolicy::MultiValueReplace,
            constraints: ConstraintSet::unconstrained(),
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: revision,
        })?;
        let mut schema_history = SchemaHistoryReferenceModel::new();
        schema_history.publish(
            revision,
            vec![
                SchemaDefinition::LayerSnapshot(layers_v1.clone()),
                SchemaDefinition::Predicate(predicate.clone()),
                SchemaDefinition::Predicate(multi_predicate.clone()),
            ],
        )?;
        let layers = LayerSchemaSnapshot::new(
            SchemaRevision::from_published_revision(query_revision),
            layers_v1.definitions().to_vec(),
            base,
        )?;
        schema_history.publish(
            query_revision,
            vec![SchemaDefinition::LayerSnapshot(layers.clone())],
        )?;
        let recorded_as_of = RecordedAsOf::from_published_revision(query_revision);
        let schema_binding =
            HistoricalQueryBinding::bind(&schema_history, recorded_as_of, SchemaMode::Historical)?;
        let validated_layers = ValidatedLayerSelection::resolve(&layers, LayerSelection::BaseOnly)?;
        let budget_limits = QueryBudgetLimits::new(100, 1_000, 100)?;
        let budget = QueryBudget::new(100, 1_000, 100, budget_limits)?;
        let context = QueryContext::new(QueryContextInput {
            snapshot: SnapshotRef::new(id::<SnapshotId>(7)?),
            snapshot_revision: query_revision,
            recorded_as_of,
            history_space: child,
            layers: validated_layers,
            world_time: WorldTimeSelector::At(WorldTime::from_nanoseconds(timeline, 50)),
            perspective: PerspectiveScope::World,
            epistemic_mode: EpistemicMode::WorldState,
            schema_binding,
            security: SecurityContext::new(principal_id, AuthorizationMode::Now),
            budget,
            cancellation: CancellationToken::new(),
        })?;
        let role_id = id::<crate::RoleId>(8)?;
        let assignment_id = id::<crate::RoleAssignmentId>(9)?;
        let role = RoleDefinition::new(
            role_id,
            "query_reader",
            PolicyBundle::from_grants(
                [
                    Capability::HistorySpaceRead,
                    Capability::LayerRead,
                    Capability::AssertionRead,
                    Capability::FieldRead,
                    Capability::MaskRead,
                    Capability::ReplacementBoundaryRead,
                    Capability::QueryResolve,
                    Capability::QueryExplain,
                    Capability::RawHistoryRead,
                ]
                .into_iter()
                .map(|capability| CapabilityGrant::new(capability, GrantEffect::Allow)),
            )?,
        )?;
        let assignment =
            RoleAssignment::new(assignment_id, principal_id, role_id, PolicyScope::project());
        let broad_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id)],
            vec![role.clone()],
            vec![assignment],
            Vec::new(),
        )?;
        let visible_id = id::<AssertionId>(10)?;
        let second_visible_id = id::<AssertionId>(11)?;
        let hidden_id = id::<AssertionId>(12)?;
        let deny = CapabilityRule::new(
            id::<crate::PolicyRuleId>(13)?,
            PolicySubject::Principal(principal_id),
            CapabilityGrant::new(Capability::FieldRead, GrantEffect::Deny),
            PolicyScope::new(
                Some(root),
                Some(base),
                Some(RecordRef::Assertion(hidden_id)),
                Some(FieldSelector::AssertionValue(predicate_id)),
                None,
            ),
        );
        let hidden_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal_id)],
            vec![role],
            vec![assignment],
            vec![deny],
        )?;
        let policies = SecurityPolicyHistory::new(
            query_revision,
            vec![
                SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    broad_policy.clone(),
                ),
                SecurityPolicyVersion::new(revision, SecurityEpoch::INITIAL, broad_policy.clone()),
                SecurityPolicyVersion::new(
                    query_revision,
                    SecurityEpoch::INITIAL,
                    broad_policy.clone(),
                ),
            ],
        )?;
        let hidden_policies = SecurityPolicyHistory::new(
            query_revision,
            vec![
                SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    broad_policy.clone(),
                ),
                SecurityPolicyVersion::new(revision, SecurityEpoch::INITIAL, broad_policy.clone()),
                SecurityPolicyVersion::new(query_revision, SecurityEpoch::new(1), hidden_policy),
            ],
        )?;
        let validity = AssertionValidity::new(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, 0)),
            Some(WorldTime::from_nanoseconds(timeline, 100)),
        )?);
        let make_assertion = |id, assertion_predicate_id, value: &str| {
            let context = ContextKey::new(
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?;
            Ok::<_, TestError>(Assertion::new(
                id,
                AssertionDraft::new(
                    context,
                    subject,
                    assertion_predicate_id,
                    Value::String(value.to_owned()),
                    Polarity::Positive,
                    validity,
                ),
                revision,
            ))
        };
        let visible = make_assertion(visible_id, predicate_id, "same")?;
        let second_visible = make_assertion(second_visible_id, predicate_id, "same")?;
        let hidden = make_assertion(hidden_id, predicate_id, "same")?;
        let multi_assertion_id = id::<AssertionId>(16)?;
        let multi_assertion = make_assertion(multi_assertion_id, multi_predicate_id, "old")?;
        let mut history = HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?])?;
        history.publish(
            root,
            vec![
                AssertionHistoryRecord::from_assertion(hidden.clone()),
                AssertionHistoryRecord::from_assertion(second_visible.clone()),
                AssertionHistoryRecord::from_assertion(visible.clone()),
                AssertionHistoryRecord::from_assertion(multi_assertion.clone()),
            ],
        )?;
        history.add_history_space(HistorySpaceDefinition::new(child, Some(root), revision)?)?;
        assert_eq!(history.publish(child, Vec::new())?, query_revision);
        let archive = ArchiveHistoryReferenceModel::new(
            [&hidden, &second_visible, &visible, &multi_assertion]
                .into_iter()
                .map(|assertion| {
                    ArchiveTargetRecord::new(ArchiveTargetRef::Assertion(assertion.id()), revision)
                })
                .collect(),
            Vec::new(),
        )?;
        let index = AssertionPointHistoryIndex::build(&history)?;
        let metadata = IndexGenerationMetadata::new(
            IndexFamily::AssertionPointHistory,
            1,
            IndexSchemaVersion::V1_0,
            IndexFormatVersion::V1_0,
            IndexBuildVersion::new(1)?,
            IndexRevisionCoverage::new(Revision::GENESIS, query_revision)?,
        )?;
        Ok(Fixture {
            history,
            archive,
            layers,
            policies,
            hidden_policies,
            context,
            predicate,
            slot: MultiValueSlot::new(subject, predicate_id),
            multi_predicate,
            multi_slot: MultiValueSlot::new(subject, multi_predicate_id),
            index,
            metadata,
            visible_id,
            second_visible_id,
            hidden_id,
        })
    }

    fn empty_mask_and_boundary_sources() -> (
        AuthorizedAssertionMaskHistory<'static>,
        ReplacementBoundarySource<'static>,
    ) {
        static EMPTY_MASK_IDS: std::sync::OnceLock<BTreeSet<MaskId>> = std::sync::OnceLock::new();
        static EMPTY_BOUNDARY_IDS: std::sync::OnceLock<BTreeSet<ReplacementBoundaryId>> =
            std::sync::OnceLock::new();
        let mask_ids = EMPTY_MASK_IDS.get_or_init(BTreeSet::new);
        let boundary_ids = EMPTY_BOUNDARY_IDS.get_or_init(BTreeSet::new);
        (
            AuthorizedAssertionMaskHistory::new(&[], &[], &[], mask_ids),
            ReplacementBoundarySource::new(&[], &[], &[], boundary_ids),
        )
    }

    fn available_index(fixture: &Fixture) -> AssertionPointIndexAccess<'_> {
        AssertionPointIndexAccess::new(
            IndexAvailability::Available(fixture.metadata),
            Some(&fixture.index),
        )
    }

    fn query_store<'a>(
        fixture: &'a Fixture,
        policies: &'a SecurityPolicyHistory,
    ) -> AssertionQueryStore<'a> {
        AssertionQueryStore::new(
            &fixture.history,
            &fixture.archive,
            &fixture.layers,
            policies,
        )
    }

    fn point_request<'a>(
        context: &'a QueryContext,
        masks: AuthorizedAssertionMaskHistory<'a>,
        boundaries: ReplacementBoundarySource<'a>,
        point_index: AssertionPointIndexAccess<'a>,
        scan_budget: FullScanBudget,
    ) -> AssertionPointRequest<'a> {
        AssertionPointRequest::new(context, masks, boundaries, point_index, scan_budget)
    }

    #[test]
    fn indexed_fallback_raw_and_explain_paths_match_the_reference_outcome() -> TestResult {
        let fixture = fixture()?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let indexed = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &fixture.context,
                masks,
                boundaries,
                available_index(&fixture),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        assert_eq!(
            indexed.path(),
            QueryExecutionPath::Indexed { generation_id: 1 }
        );

        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let fallback = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &fixture.context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        assert_eq!(
            fallback.path(),
            QueryExecutionPath::IndexFallback {
                reason: IndexFallbackReason::Missing
            }
        );
        assert_eq!(
            indexed.query().value().contributors(),
            &[
                fixture.visible_id,
                fixture.second_visible_id,
                fixture.hidden_id,
            ]
        );
        assert_eq!(
            indexed.query().value().contributors(),
            fallback.query().value().contributors()
        );

        let record_query = AssertionCandidateQuery::new(
            fixture.context.history_space(),
            fixture.context.layers().requested().clone(),
            fixture.context.perspective(),
            fixture.context.epistemic_mode(),
            fixture.context.recorded_as_of(),
            match fixture.context.world_time() {
                WorldTimeSelector::At(value) => value,
                WorldTimeSelector::AllTimes => unreachable!(),
            },
        );
        let policy = fixture.policies.resolve(&fixture.context)?.snapshot();
        let oracle_candidates = full_scan_authorized_assertion_candidates(
            &fixture.history,
            &fixture.archive,
            &record_query,
            &fixture.layers,
            policy,
            &fixture.context,
        )?;
        let oracle_outcome = resolve_single_value_replace(
            &oracle_candidates,
            SingleValueSlot::new(fixture.slot.subject(), fixture.slot.predicate_id()),
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        let oracle = ResolvedView::from_single(oracle_outcome)?;
        assert_eq!(
            indexed.query().value().contributors(),
            oracle.contributors()
        );

        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let explain = ProductiveQueryEngine::explain_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &fixture.context,
                masks,
                boundaries,
                available_index(&fixture),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        assert_eq!(
            explain.query().value().resolved_view().contributors(),
            oracle.contributors()
        );
        assert_eq!(explain.query().value().stages().len(), 3);

        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let hidden = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.hidden_policies),
            point_request(
                &fixture.context,
                masks,
                boundaries,
                available_index(&fixture),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        assert_eq!(
            hidden.query().value().contributors(),
            &[fixture.visible_id, fixture.second_visible_id]
        );
        assert!(
            !hidden
                .query()
                .value()
                .contributors()
                .contains(&fixture.hidden_id)
        );

        let raw = ProductiveQueryEngine::raw_history(
            &fixture.history,
            &fixture.context,
            &fixture.policies,
            FullScanBudget::Available,
            |record| match record {
                AssertionHistoryRecord::Assertion(assertion) => {
                    RecordRef::Assertion(assertion.id())
                }
                AssertionHistoryRecord::ValidityClosure(closure) => {
                    RecordRef::AssertionValidityClosure(closure.id())
                }
                AssertionHistoryRecord::Retraction(retraction) => {
                    RecordRef::AssertionRetraction(retraction.id())
                }
            },
        )?;
        assert_eq!(raw.path(), QueryExecutionPath::FullScan);
        assert_eq!(raw.query().value().len(), 4);
        assert_eq!(
            raw.query()
                .value()
                .iter()
                .map(RawHistoryRow::record_ref)
                .collect::<Vec<_>>(),
            vec![
                RecordRef::Assertion(fixture.visible_id),
                RecordRef::Assertion(fixture.second_visible_id),
                RecordRef::Assertion(fixture.hidden_id),
                RecordRef::Assertion(id::<AssertionId>(16)?),
            ]
        );

        let principal = fixture.context.security().principal_id();
        let raw_only_rules = [Capability::HistorySpaceRead, Capability::RawHistoryRead]
            .into_iter()
            .enumerate()
            .map(|(index, capability)| {
                Ok(CapabilityRule::new(
                    id::<crate::PolicyRuleId>(30 + index as u8)?,
                    PolicySubject::Principal(principal),
                    CapabilityGrant::new(capability, GrantEffect::Allow),
                    PolicyScope::project(),
                ))
            })
            .collect::<TestResult<Vec<_>>>()?;
        let raw_only_policy = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            Vec::new(),
            Vec::new(),
            raw_only_rules,
        )?;
        let raw_only_history = SecurityPolicyHistory::new(
            fixture.context.recorded_as_of().revision(),
            vec![
                SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    raw_only_policy.clone(),
                ),
                SecurityPolicyVersion::new(
                    revision(1)?,
                    SecurityEpoch::INITIAL,
                    raw_only_policy.clone(),
                ),
                SecurityPolicyVersion::new(
                    fixture.context.recorded_as_of().revision(),
                    SecurityEpoch::INITIAL,
                    raw_only_policy,
                ),
            ],
        )?;
        let denied_by_record_class = ProductiveQueryEngine::raw_history(
            &fixture.history,
            &fixture.context,
            &raw_only_history,
            FullScanBudget::Available,
            |record| match record {
                AssertionHistoryRecord::Assertion(assertion) => {
                    RecordRef::Assertion(assertion.id())
                }
                AssertionHistoryRecord::ValidityClosure(closure) => {
                    RecordRef::AssertionValidityClosure(closure.id())
                }
                AssertionHistoryRecord::Retraction(retraction) => {
                    RecordRef::AssertionRetraction(retraction.id())
                }
            },
        )?;
        assert!(denied_by_record_class.query().value().is_empty());
        Ok(())
    }

    #[test]
    fn incompatible_indexes_fall_back_or_fail_before_returning_partial_results() -> TestResult {
        let fixture = fixture()?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let error = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &fixture.context,
                masks,
                boundaries,
                AssertionPointIndexAccess::corrupt(),
                FullScanBudget::Exceeded(BudgetDimension::WorkUnits),
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        );
        assert!(matches!(
            error,
            Err(QueryEngineError::FullScanBudgetExceeded(
                BudgetDimension::WorkUnits
            ))
        ));
        Ok(())
    }

    #[test]
    fn explain_trace_lists_only_the_authorized_mask_that_removed_a_candidate() -> TestResult {
        let fixture = fixture()?;
        let mask = Mask::new(
            id::<MaskId>(20)?,
            ContextKey::new(
                fixture.context.history_space(),
                id::<LayerId>(2)?,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            MaskSelector::ExactAssertion(fixture.visible_id),
            None,
            revision(2)?,
        )?;
        let archive_visible_masks = [mask.id()].into_iter().collect();
        let boundaries = empty_mask_and_boundary_sources().1;
        let explain = ProductiveQueryEngine::explain_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &fixture.context,
                AuthorizedAssertionMaskHistory::new(
                    std::slice::from_ref(&mask),
                    &[],
                    &[],
                    &archive_visible_masks,
                ),
                boundaries,
                available_index(&fixture),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        assert_eq!(
            explain.query().value().resolved_view().contributors(),
            &[fixture.second_visible_id, fixture.hidden_id]
        );
        let Some(mask_stage) = explain.query().value().stages().get(1) else {
            return Err(std::io::Error::other("Explain is missing its mask stage").into());
        };
        assert_eq!(mask_stage.applied_records(), &[RecordRef::Mask(mask.id())]);
        Ok(())
    }

    #[test]
    fn explain_trace_reports_the_boundary_that_cut_off_inherited_candidates() -> TestResult {
        let fixture = fixture()?;
        let layer = id::<LayerId>(2)?;
        let context = ContextKey::new(
            fixture.context.history_space(),
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let boundary = ReplacementBoundary::new(
            id::<ReplacementBoundaryId>(21)?,
            context,
            fixture.multi_slot.subject(),
            &fixture.multi_predicate,
            None,
            revision(2)?,
        )?;
        let archive_visible_boundaries = [boundary.id()].into_iter().collect();
        let boundaries = ReplacementBoundarySource::new(
            std::slice::from_ref(&boundary),
            &[],
            &[],
            &archive_visible_boundaries,
        );
        let (masks, _) = empty_mask_and_boundary_sources();
        let explain = ProductiveQueryEngine::explain_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &fixture.context,
                masks,
                boundaries,
                available_index(&fixture),
                FullScanBudget::Available,
            ),
            fixture.multi_slot,
            &fixture.multi_predicate,
            |_, _| Ok(false),
        )?;
        assert!(
            explain
                .query()
                .value()
                .resolved_view()
                .contributors()
                .is_empty()
        );
        let stages = explain.query().value().stages();
        assert_eq!(stages.len(), 4);
        let Some(boundary_stage) = stages.get(2) else {
            return Err(
                std::io::Error::other("Explain is missing its ReplacementBoundary stage").into(),
            );
        };
        assert_eq!(
            boundary_stage.applied_records(),
            &[RecordRef::ReplacementBoundary(boundary.id())]
        );
        assert!(boundary_stage.output_assertions().is_empty());
        Ok(())
    }
}
