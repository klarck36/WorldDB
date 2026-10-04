//! Productive, snapshot-bound Raw, Resolved, and Explain query entry points.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::mem::size_of;

use crate::archive::{ArchiveState, ArchiveTargetRef};
use crate::archive_projection::ArchiveHistoryReferenceModel;
use crate::assertion_point_index::{AssertionPointHistoryIndex, AssertionPointIndexError};
use crate::candidate_scan::{
    AssertionCandidate, AssertionCandidateQuery, AssertionCandidateSecurityContext,
    AssertionHistoryRecord, AssertionPointCandidateFilter, CandidateScanError,
    assertion_candidate_is_authorized, full_scan_authorized_point_assertion_candidates,
    indexed_authorized_assertion_candidates,
};
use crate::context::{ContextError, ContextKey};
use crate::context_precedence::ContextPrecedence;
use crate::cursor::{CursorInsertRequest, CursorSecurityContext, CursorStateError};
use crate::history_model::HistorySpaceReferenceModel;
use crate::ids::{AssertionId, MaskId, ReplacementBoundaryId, TimelineId};
use crate::index_generation::{
    FullScanBudget, IndexAccessPlan, IndexAvailability, IndexBuildVersion, IndexFallbackReason,
    IndexFamily, IndexFormatVersion, IndexQueryRequirement, IndexSchemaVersion, plan_index_access,
};
use crate::layers::LayerSchemaSnapshot;
use crate::mask_projection::{
    AssertionMaskContext, AuthorizedAssertionMaskHistory,
    apply_authorized_assertion_masks_with_trace,
    apply_authorized_assertion_masks_with_trace_all_times, mask_is_authorized,
};
use crate::masks::{
    Mask, MaskRetraction, MaskSelector, MaskValidityClosure, ReplacementBoundary,
    ReplacementBoundaryRetraction, ReplacementBoundaryValidityClosure,
};
use crate::multi_value_resolution::{
    MultiValueReplaceContext, MultiValueResolutionError, MultiValueSlot,
    ReplacementBoundaryHistory, resolve_multi_value_overlay,
    resolve_multi_value_replace_with_trace,
};
use crate::query_aggregate::{
    AggregateError, AggregateResult, AggregateSpec, ResolvedAggregateRow,
    aggregate_visible_resolved,
};
use crate::query_context::{BudgetDimension, QueryContext, WorldTimeSelector};
use crate::query_graph::{
    GraphCandidateSet, GraphError, GraphResult, GraphSpec, full_scan_authorized_graph_traversal,
};
use crate::query_page::{PageCursorState, PageExecution, PageRequest, QueryPage};
use crate::query_ports::{
    OwnedQueryResult, QueryPortError, bind_authorized_explain, bind_authorized_resolved_view,
    full_scan_owned_authorized_raw_history,
};
use crate::query_search::{
    QuerySearchError, SearchDocument, SearchHit, SearchSpec, full_scan_token_search,
};
use crate::query_stream::{CandidateStream, QueryItemError};
use crate::record_refs::RecordRef;
use crate::reference_query::{
    ExplainStage, ExplainStageError, ExplainStageKind, RawHistoryError, RawHistoryRow,
    ReferenceExplain, ReferenceExplainError, ResolvedView, ResolvedViewError,
};
use crate::resource_profile::{MemoryReservation, ResourceClass};
use crate::schema::{PredicateDefinition, ResolutionPolicy};
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicyHistory,
    SecurityPolicyHistoryError, SecurityPolicySnapshot,
};
use crate::single_value_resolution::{
    SingleValueResolutionError, SingleValueSlot, resolve_single_value_replace,
};
use crate::temporal::{TimeInterval, Timeline, WorldTime};
use crate::values::Value;

const MAX_PAGE_SORT_KEY_BYTES: usize = 64 * 1024;

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
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
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
    /// Full input enumeration, aggregate reduction, or a caller-approved scan fallback.
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

/// Complete resolution preview bound to the supplied QueryContext.
///
/// Technical failures are returned as `QueryEngineError`; they are never
/// converted into a domain `Unknown` or an incomplete result.
#[derive(Clone, Debug)]
pub enum ResolutionPreview {
    /// One world-time point was selected, including an `Unknown` outcome when
    /// the selected slot has no visible candidates at that point.
    Point {
        world_time: WorldTime,
        resolved_view: ResolvedView,
    },
    /// Complete, ordered half-open time slices for each visible assertion or
    /// ReplacementBoundary timeline used by the selected slot. This variant
    /// always contains at least one slice.
    AllTimes { slices: Vec<ResolutionTimeSlice> },
    /// A complete all-times query found no visible assertion or explicit
    /// ReplacementBoundary time domain from which a timeline result could be
    /// enumerated.
    CompleteEmpty,
}

/// One outcome over a complete half-open world-time interval.
#[derive(Clone, Debug)]
pub struct ResolutionTimeSlice {
    interval: TimeInterval,
    resolved_view: ResolvedView,
}

impl ResolutionTimeSlice {
    /// Time interval covered by the outcome.
    #[must_use]
    pub const fn interval(&self) -> TimeInterval {
        self.interval
    }

    /// Known, Unknown, or Conflict domain outcome for this interval.
    #[must_use]
    pub const fn resolved_view(&self) -> &ResolvedView {
        &self.resolved_view
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

    /// Traverses a context-bound graph candidate snapshot through the authorized
    /// M3 BFS port. EventTime indexes or other graph candidate sources must be
    /// bound to this same QueryContext before traversal; the reference graph walk
    /// remains the complete fallback and validates record/relationship rights.
    pub fn graph_traversal(
        candidates: &GraphCandidateSet,
        spec: &GraphSpec,
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
    ) -> Result<QueryEngineOutput<GraphResult>, QueryEngineError> {
        ensure_active(context)?;
        if !candidates.is_bound_to(context) {
            return Err(QueryEngineError::Graph(
                GraphError::CandidateContextMismatch,
            ));
        }
        let query = full_scan_authorized_graph_traversal(candidates, spec, context, policies)?;
        ensure_active(context)?;
        Ok(QueryEngineOutput {
            query,
            path: QueryExecutionPath::FullScan,
        })
    }

    /// Aggregates complete, already-resolved result rows bound to the current query.
    /// Record and group-field permissions are rechecked by the M3 aggregate port;
    /// cancellation and budget failures remain terminal and never return partial data.
    pub fn aggregate(
        resolved: &OwnedQueryResult<Vec<ResolvedAggregateRow>>,
        spec: &AggregateSpec,
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
    ) -> Result<QueryEngineOutput<AggregateResult>, QueryEngineError> {
        ensure_active(context)?;
        let query = aggregate_visible_resolved(resolved, spec, context, policies)?;
        ensure_active(context)?;
        Ok(QueryEngineOutput {
            query,
            path: QueryExecutionPath::FullScan,
        })
    }

    /// Aggregates only the distinct Assertion contributors of an owned resolution result.
    ///
    /// The adapter must return one row for every resolved contributor and may attach only
    /// values read from that contributor at the pinned query snapshot. Core rejects missing,
    /// duplicate, or unrelated rows before applying the normal record/field authorization
    /// checks in the aggregate port.
    pub fn aggregate_resolution_contributors<F>(
        resolved: &OwnedQueryResult<ResolutionPreview>,
        spec: &AggregateSpec,
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
        mut row_for_contributor: F,
    ) -> Result<QueryEngineOutput<AggregateResult>, QueryEngineError>
    where
        F: FnMut(AssertionId, &[FieldSelector]) -> Result<ResolvedAggregateRow, AggregateError>,
    {
        ensure_active(context)?;
        if resolved.binding() != context.schema_binding()
            || !resolved.query_context_binding().matches(context)
        {
            return Err(AggregateError::QueryBindingMismatch.into());
        }
        let requested_fields = match spec {
            AggregateSpec::GroupedCount { fields } => fields.as_slice(),
            AggregateSpec::Count | AggregateSpec::Exists => &[],
        };
        let mut contributors = BTreeSet::new();
        match resolved.value() {
            ResolutionPreview::Point { resolved_view, .. } => {
                contributors.extend(resolved_view.contributors().iter().copied());
            }
            ResolutionPreview::AllTimes { slices } => {
                for slice in slices {
                    contributors.extend(slice.resolved_view().contributors().iter().copied());
                }
            }
            ResolutionPreview::CompleteEmpty => {}
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(contributors.len())
            .map_err(|_| AggregateError::ResourceBudgetExceeded)?;
        let mut row_keys = BTreeSet::new();
        for contributor in contributors {
            ensure_active(context)?;
            let row = row_for_contributor(contributor, requested_fields)?;
            if row.result_key() != RecordRef::Assertion(contributor)
                || !row_keys.insert(row.result_key())
            {
                return Err(AggregateError::QueryBindingMismatch.into());
            }
            rows.push(row);
        }
        let resolved_rows = OwnedQueryResult::bind(context, policies, rows)?;
        Self::aggregate(&resolved_rows, spec, context, policies)
    }

    /// Pulls one bounded page from a deterministic source. The source factory must bind its
    /// ordering and filtering to the supplied snapshot and start strictly after the opaque
    /// internal sort key. The trusted adapter supplies the stable sort key and row-rights
    /// check; the engine evaluates that check against both the current and query-selected
    /// policy before a row can affect output, counters, or the continuation boundary.
    pub fn stream_page<T: 'static, I>(
        request: PageRequest,
        execution: PageExecution<'_>,
        context: &QueryContext,
        policies: &SecurityPolicyHistory,
        source_factory: impl FnOnce(Option<&[u8]>) -> Result<I, QueryEngineError>,
        mut sort_key: impl FnMut(&T) -> Vec<u8>,
        mut row_is_authorized: impl FnMut(&T, &SecurityPolicySnapshot, &QueryContext) -> bool,
    ) -> Result<QueryPage<T>, QueryEngineError>
    where
        I: Iterator<Item = Result<T, QueryEngineError>>,
    {
        let original_cursor = request.cursor().map(<[u8]>::to_vec);
        let mut created_cursor = None;
        let result = (|| {
            if request.limit().get() > execution.max_page_size.get() {
                return Err(QueryEngineError::InvalidPageRequest);
            }
            if u64::from(request.limit().get()) > context.budget().max_results().get() {
                return Err(QueryEngineError::BudgetExceeded(BudgetDimension::Results));
            }
            ensure_active(context)?;
            context
                .ensure_snapshot_live(execution.now_ms)
                .map_err(|_| QueryEngineError::SnapshotExpired)?;
            execution.cursors.reap_expired(execution.now_ms);

            let policy_view = policies.resolve(context)?;
            let principal = context.security().principal_id();
            let operation_capability = execution.operation.capability();
            let operation_target = PolicyTarget::default();
            if policy_view.current_snapshot().authorize(
                principal,
                operation_capability,
                operation_target,
            ) != AuthorizationDecision::Allow
                || policy_view.snapshot().authorize(
                    principal,
                    operation_capability,
                    operation_target,
                ) != AuthorizationDecision::Allow
                || (execution.operation.requires_admin_raw()
                    && (policy_view.current_snapshot().authorize(
                        principal,
                        Capability::AdminRawRead,
                        operation_target,
                    ) != AuthorizationDecision::Allow
                        || policy_view.snapshot().authorize(
                            principal,
                            Capability::AdminRawRead,
                            operation_target,
                        ) != AuthorizationDecision::Allow))
            {
                return Err(QueryEngineError::Unauthorized);
            }

            let security = CursorSecurityContext::new(
                principal,
                policy_view.current_epoch(),
                policy_view.evaluated_epoch(),
                operation_target,
            );
            let (cursor_state, after_sort_key) = if let Some(wire) = request.cursor() {
                let state_bytes = execution
                    .cursors
                    .resolve_authorized(
                        wire,
                        execution.now_ms,
                        context.snapshot(),
                        execution.query_hash,
                        security,
                        policy_view,
                    )
                    .map_err(map_cursor_error)?
                    .to_vec();
                let Some(state) = PageCursorState::decode(&state_bytes) else {
                    execution.cursors.discard_authenticated(wire);
                    return Err(QueryEngineError::CursorInvalidated);
                };
                if state.limit != request.limit().get()
                    || state.expires_at_ms <= execution.now_ms
                    || state.after_sort_key.is_empty()
                {
                    execution.cursors.discard_authenticated(wire);
                    return Err(QueryEngineError::CursorInvalidated);
                }
                (state.clone(), Some(state.after_sort_key.clone()))
            } else {
                (
                    PageCursorState {
                        limit: request.limit().get(),
                        expires_at_ms: execution.first_expires_at_ms,
                        candidates_seen: 0,
                        work_units_seen: 0,
                        results_sent: 0,
                        after_sort_key: Vec::new(),
                    },
                    None,
                )
            };

            let source = source_factory(after_sort_key.as_deref())?;
            let mut stream =
                CandidateStream::from_items(source.map(|item| item.map_err(QueryItemError::Item)));
            let page_limit = usize::try_from(request.limit().get())
                .map_err(|_| QueryEngineError::InvalidPageRequest)?;
            let mut results = Vec::new();
            let mut page_reservations = Vec::<MemoryReservation>::new();
            let mut last_visible_key = (!cursor_state.after_sort_key.is_empty())
                .then(|| cursor_state.after_sort_key.clone());
            let mut candidates_seen = cursor_state.candidates_seen;
            let mut work_units_seen = cursor_state.work_units_seen;
            let mut has_more = false;

            loop {
                ensure_active(context)?;
                let Some(item) = stream.next() else {
                    break;
                };
                let item = match item {
                    Ok(item) => item,
                    Err(QueryItemError::Item(error)) => return Err(error),
                    Err(QueryItemError::Cancelled) => return Err(QueryEngineError::Cancelled),
                    Err(QueryItemError::BudgetExceeded) => {
                        return Err(QueryEngineError::BudgetExceeded(BudgetDimension::WorkUnits));
                    }
                    Err(QueryItemError::SkippableDiagnostic) => continue,
                };

                let visible_now = row_is_authorized(&item, policy_view.current_snapshot(), context);
                let visible_evaluated = row_is_authorized(&item, policy_view.snapshot(), context);
                if !visible_now || !visible_evaluated {
                    continue;
                }

                let key = sort_key(&item);
                if key.is_empty()
                    || key.len() > MAX_PAGE_SORT_KEY_BYTES
                    || last_visible_key
                        .as_ref()
                        .is_some_and(|previous| previous.as_slice() >= key.as_slice())
                {
                    return Err(QueryEngineError::InvalidPageOrder);
                }
                last_visible_key = Some(key.clone());

                candidates_seen =
                    candidates_seen
                        .checked_add(1)
                        .ok_or(QueryEngineError::BudgetExceeded(
                            BudgetDimension::Candidates,
                        ))?;
                work_units_seen = work_units_seen
                    .checked_add(1)
                    .ok_or(QueryEngineError::BudgetExceeded(BudgetDimension::WorkUnits))?;
                if candidates_seen > context.budget().max_candidates().get() {
                    return Err(QueryEngineError::BudgetExceeded(
                        BudgetDimension::Candidates,
                    ));
                }
                if work_units_seen > context.budget().max_work_units().get() {
                    return Err(QueryEngineError::BudgetExceeded(BudgetDimension::WorkUnits));
                }

                let pending_visible = u64::try_from(results.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(1);
                if cursor_state.results_sent.saturating_add(pending_visible)
                    > context.budget().max_results().get()
                {
                    return Err(QueryEngineError::BudgetExceeded(BudgetDimension::Results));
                }
                if results.len() == page_limit {
                    has_more = true;
                    break;
                }
                let reservation_bytes =
                    size_of::<T>().saturating_add(key.len()).saturating_add(128);
                let reservation = context
                    .resource_budget()
                    .reserve(
                        ResourceClass::Query,
                        u64::try_from(reservation_bytes)
                            .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?,
                    )
                    .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
                results
                    .try_reserve(1)
                    .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
                page_reservations
                    .try_reserve(1)
                    .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
                results.push((item, key));
                page_reservations.push(reservation);
            }
            ensure_active(context)?;

            let results_sent = cursor_state
                .results_sent
                .checked_add(u64::try_from(results.len()).unwrap_or(u64::MAX))
                .ok_or(QueryEngineError::BudgetExceeded(BudgetDimension::Results))?;
            let next_after_key = results.last().map(|(_, key)| key.clone());
            let mut page_rows = Vec::new();
            page_rows
                .try_reserve_exact(results.len())
                .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
            for (row, _) in results {
                page_rows.push(row);
            }
            let query = OwnedQueryResult::bind_with_memory_reservations(
                context,
                policies,
                page_rows,
                page_reservations,
            )?;

            let next_cursor = if has_more {
                let Some(last_key) = next_after_key else {
                    return Err(QueryEngineError::InvalidPageOrder);
                };
                let next_state = PageCursorState {
                    limit: request.limit().get(),
                    expires_at_ms: cursor_state.expires_at_ms,
                    candidates_seen,
                    work_units_seen,
                    results_sent,
                    after_sort_key: last_key,
                };
                let state_bytes = next_state
                    .encode()
                    .ok_or(QueryEngineError::CursorUnavailable)?;
                let mut insert = CursorInsertRequest::new(
                    context.snapshot(),
                    execution.query_hash,
                    state_bytes,
                    execution.now_ms,
                    next_state.expires_at_ms,
                );
                if let Some(lease) = context
                    .fork_snapshot_lease(execution.now_ms)
                    .map_err(|_| QueryEngineError::SnapshotExpired)?
                {
                    insert = insert.with_snapshot_lease(lease);
                } else {
                    return Err(QueryEngineError::SnapshotPinRequired);
                }
                let next_token = execution
                    .cursors
                    .insert_authorized(insert, security, policy_view)
                    .map_err(map_cursor_error)?
                    .encode();
                created_cursor = Some(next_token.clone());
                if let Some(wire) = request.cursor() {
                    if let Err(error) = execution.cursors.remove_authorized(
                        wire,
                        execution.now_ms,
                        context.snapshot(),
                        execution.query_hash,
                        security,
                        policy_view,
                    ) {
                        execution.cursors.discard_authenticated(&next_token);
                        return Err(map_cursor_error(error));
                    }
                }
                Some(next_token)
            } else {
                if let Some(wire) = request.cursor() {
                    execution
                        .cursors
                        .remove_authorized(
                            wire,
                            execution.now_ms,
                            context.snapshot(),
                            execution.query_hash,
                            security,
                            policy_view,
                        )
                        .map_err(map_cursor_error)?;
                }
                None
            };

            ensure_active(context)?;
            context
                .ensure_snapshot_live(execution.now_ms)
                .map_err(|_| QueryEngineError::SnapshotExpired)?;
            Ok(QueryPage::new(query, next_cursor, execution.path))
        })();

        if result.is_err() {
            if let Some(wire) = created_cursor.as_deref() {
                execution.cursors.discard_authenticated(wire);
            }
            if let Some(wire) = original_cursor.as_deref() {
                execution.cursors.discard_authenticated(wire);
            }
        }
        result
    }

    /// Produces a complete resolution preview for one subject/Predicate slot.
    ///
    /// A point selector returns exactly one Resolved View. `AllTimes` partitions
    /// each visible assertion or ReplacementBoundary timeline at the half-open
    /// validity and closure boundaries, resolves every cell, and returns an
    /// explicit `CompleteEmpty` when there is no visible temporal domain to
    /// enumerate. Event-window filters are intentionally not part of this
    /// operation.
    pub fn resolution_preview<F>(
        store: AssertionQueryStore<'_>,
        request: AssertionPointRequest<'_>,
        slot: MultiValueSlot,
        predicate: &PredicateDefinition,
        mut temporal_value_equal: F,
    ) -> Result<QueryEngineOutput<ResolutionPreview>, QueryEngineError>
    where
        F: FnMut(&Value, &Value) -> Result<bool, ()>,
    {
        let context = request.context;
        let policies = store.policies;
        ensure_active(context)?;
        if predicate.predicate_id() != slot.predicate_id() {
            return Err(QueryEngineError::Resolution(
                ResolutionFailure::PredicateMismatch,
            ));
        }

        let mut visible_assertions = BTreeSet::new();
        let mut memory_reservations = Vec::new();
        let (preview, path) = match context.world_time() {
            WorldTimeSelector::At(world_time) => {
                let execution = execute_point_at(
                    store,
                    request,
                    slot,
                    predicate,
                    world_time,
                    &mut temporal_value_equal,
                )?;
                visible_assertions.extend(execution.candidate_ids.iter().copied());
                (
                    ResolutionPreview::Point {
                        world_time,
                        resolved_view: execution.resolved_view,
                    },
                    execution.path,
                )
            }
            WorldTimeSelector::AllTimes => {
                require_full_scan_budget(request.scan_budget)?;
                let policy = policies.resolve(context)?;
                let plan = collect_resolution_partitions(
                    store,
                    request,
                    slot,
                    policy.snapshot(),
                    &mut memory_reservations,
                )?;
                if plan.partitions.is_empty() {
                    (
                        ResolutionPreview::CompleteEmpty,
                        QueryExecutionPath::FullScan,
                    )
                } else {
                    let mut slices = Vec::new();
                    slices
                        .try_reserve_exact(plan.partitions.len())
                        .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
                    for partition in plan.partitions {
                        ensure_active(context)?;
                        if u64::try_from(slices.len()).unwrap_or(u64::MAX)
                            >= context.budget().max_results().get()
                        {
                            return Err(QueryEngineError::BudgetExceeded(BudgetDimension::Results));
                        }
                        let execution = execute_all_times_point(
                            store,
                            request,
                            slot,
                            predicate,
                            partition.sample,
                            AllTimesPointHistory {
                                assertion_ids: &plan.assertion_ids,
                                policy: policy.snapshot(),
                            },
                            &mut temporal_value_equal,
                        )?;
                        visible_assertions.extend(execution.candidate_ids.iter().copied());
                        reserve_query_memory(
                            context,
                            size_of::<ResolutionTimeSlice>(),
                            &mut memory_reservations,
                        )?;
                        slices.push(ResolutionTimeSlice {
                            interval: partition.interval,
                            resolved_view: execution.resolved_view,
                        });
                    }
                    (
                        ResolutionPreview::AllTimes { slices },
                        QueryExecutionPath::FullScan,
                    )
                }
            }
        };

        if !preview_contributors_are_visible(&preview, &visible_assertions) {
            return Err(QueryEngineError::QueryBinding);
        }
        ensure_active(context)?;
        let query = OwnedQueryResult::bind_with_memory_reservations(
            context,
            policies,
            preview,
            memory_reservations,
        )?;
        ensure_active(context)?;
        Ok(QueryEngineOutput { query, path })
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
    let WorldTimeSelector::At(world_time) = request.context.world_time() else {
        return Err(QueryEngineError::AllTimesResolutionUnavailable);
    };
    execute_point_at(
        store,
        request,
        slot,
        predicate,
        world_time,
        &mut temporal_value_equal,
    )
}

fn execute_point_at<F>(
    store: AssertionQueryStore<'_>,
    request: AssertionPointRequest<'_>,
    slot: MultiValueSlot,
    predicate: &PredicateDefinition,
    world_time: WorldTime,
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
            match indexed_authorized_assertion_candidates(
                archive,
                index,
                &query,
                layers,
                AssertionCandidateSecurityContext::new(security.snapshot(), context),
                AssertionPointCandidateFilter::new(slot.subject(), slot.predicate_id()),
            ) {
                Ok(Ok(candidates)) => (candidates, QueryExecutionPath::Indexed { generation_id }),
                Ok(Err(error)) => return Err(error.into()),
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
                    let candidates = full_scan_authorized_point_assertion_candidates(
                        history,
                        archive,
                        &query,
                        layers,
                        AssertionCandidateSecurityContext::new(security.snapshot(), context),
                        AssertionPointCandidateFilter::new(slot.subject(), slot.predicate_id()),
                    )?;
                    (
                        candidates,
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
                AssertionPointCandidateFilter::new(slot.subject(), slot.predicate_id()),
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
    let mask_projection = match context.world_time() {
        WorldTimeSelector::At(_) => apply_authorized_assertion_masks_with_trace(
            &candidates,
            masks,
            mask_context,
            security.snapshot(),
            context,
            &mut temporal_value_equal,
        )?,
        WorldTimeSelector::AllTimes => apply_authorized_assertion_masks_with_trace_all_times(
            &candidates,
            masks,
            mask_context,
            security.snapshot(),
            context,
            &mut temporal_value_equal,
        )?,
    };
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

#[derive(Clone, Copy)]
struct ResolutionPartition {
    interval: TimeInterval,
    sample: WorldTime,
}

struct ResolutionTimePlan {
    partitions: Vec<ResolutionPartition>,
    assertion_ids: BTreeSet<AssertionId>,
}

struct AllTimesPointHistory<'a> {
    assertion_ids: &'a BTreeSet<AssertionId>,
    policy: &'a SecurityPolicySnapshot,
}

#[derive(Default)]
struct OwnedMaskHistory {
    masks: Vec<Mask>,
    closures: Vec<MaskValidityClosure>,
    retractions: Vec<MaskRetraction>,
    archive_visible: BTreeSet<MaskId>,
}

#[derive(Default)]
struct OwnedBoundaryHistory {
    boundaries: Vec<ReplacementBoundary>,
    closures: Vec<ReplacementBoundaryValidityClosure>,
    retractions: Vec<ReplacementBoundaryRetraction>,
    archive_visible: BTreeSet<ReplacementBoundaryId>,
}

fn execute_all_times_point<F>(
    store: AssertionQueryStore<'_>,
    request: AssertionPointRequest<'_>,
    slot: MultiValueSlot,
    predicate: &PredicateDefinition,
    world_time: WorldTime,
    history: AllTimesPointHistory<'_>,
    temporal_value_equal: F,
) -> Result<PointExecution, QueryEngineError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    let context = request.context;
    let mask_history = masks_for_time(
        store,
        request.masks,
        context,
        slot,
        history.assertion_ids,
        world_time.timeline().id(),
        history.policy,
    );
    let boundary_history = boundaries_for_time(
        store,
        request.boundaries,
        context,
        slot,
        world_time.timeline().id(),
        history.policy,
    );
    let masks = AuthorizedAssertionMaskHistory::new(
        &mask_history.masks,
        &mask_history.closures,
        &mask_history.retractions,
        &mask_history.archive_visible,
    );
    let boundaries = ReplacementBoundarySource::new(
        &boundary_history.boundaries,
        &boundary_history.closures,
        &boundary_history.retractions,
        &boundary_history.archive_visible,
    );
    let request = AssertionPointRequest::new(
        context,
        masks,
        boundaries,
        AssertionPointIndexAccess::missing(),
        request.scan_budget,
    );
    execute_point_at(
        store,
        request,
        slot,
        predicate,
        world_time,
        temporal_value_equal,
    )
}

fn collect_resolution_partitions(
    store: AssertionQueryStore<'_>,
    request: AssertionPointRequest<'_>,
    slot: MultiValueSlot,
    policy: &SecurityPolicySnapshot,
    memory_reservations: &mut Vec<MemoryReservation>,
) -> Result<ResolutionTimePlan, QueryEngineError> {
    let context = request.context;
    let principal = context.security().principal_id();
    let mut timeline_edges = BTreeMap::<TimelineId, BTreeSet<i128>>::new();
    let mut assertion_ids = BTreeSet::new();
    let mut candidates = 0_u64;
    let mut work_units = 0_u64;
    for (stored_revision, owner_history_space_id, record) in store
        .history
        .iter_at(context.history_space(), context.recorded_as_of().revision())
        .map_err(|_| QueryEngineError::CandidateHistory)?
    {
        match record {
            AssertionHistoryRecord::Assertion(assertion) => {
                if !assertion_candidate_is_authorized(
                    policy,
                    principal,
                    owner_history_space_id,
                    assertion,
                ) || assertion.subject() != slot.subject()
                    || assertion.predicate_id() != slot.predicate_id()
                {
                    continue;
                }
                if stored_revision != assertion.created_revision()
                    || assertion.context().history_space_id() != owner_history_space_id
                {
                    return Err(QueryEngineError::CandidateHistory);
                }
                if assertion.context().perspective_scope() != context.perspective()
                    || assertion.context().epistemic_mode() != context.epistemic_mode()
                    || !context
                        .layers()
                        .resolved()
                        .as_slice()
                        .contains(&assertion.context().layer_id())
                {
                    continue;
                }
                ContextPrecedence::for_context(
                    context.history_space(),
                    owner_history_space_id,
                    assertion.context().layer_id(),
                    store.history.catalog(),
                    store.layers,
                )
                .map_err(|_| QueryEngineError::CandidateHistory)?;
                if candidates >= context.budget().max_candidates().get() {
                    return Err(QueryEngineError::BudgetExceeded(
                        BudgetDimension::Candidates,
                    ));
                }
                candidates += 1;
                count_preview_work(context, &mut work_units)?;
                let archive_target = ArchiveTargetRef::Assertion(assertion.id());
                let archive_record = store
                    .archive
                    .target_record(archive_target)
                    .ok_or(QueryEngineError::CandidateHistory)?;
                if archive_record.created_revision() != assertion.created_revision() {
                    return Err(QueryEngineError::CandidateHistory);
                }
                if store
                    .archive
                    .state_at(archive_target, context.recorded_as_of())
                    .map_err(|_| QueryEngineError::CandidateHistory)?
                    != ArchiveState::Unarchived
                {
                    continue;
                }
                assertion_ids.insert(assertion.id());
                add_interval_endpoints(
                    context,
                    &mut timeline_edges,
                    assertion.validity().interval(),
                    memory_reservations,
                )?;
            }
            AssertionHistoryRecord::ValidityClosure(closure)
                if assertion_ids.contains(&closure.assertion_id())
                    && closure.created_revision() <= context.recorded_as_of().revision() =>
            {
                count_preview_work(context, &mut work_units)?;
                add_endpoint_if_timeline_exists(
                    context,
                    &mut timeline_edges,
                    closure.close_at_world_time(),
                    memory_reservations,
                )?;
            }
            AssertionHistoryRecord::ValidityClosure(_) | AssertionHistoryRecord::Retraction(_) => {}
        }
    }

    if !assertion_ids.is_empty() {
        let (source_masks, source_mask_closures, source_mask_retractions, archive_visible_masks) =
            request.masks.records();
        let mut mask_ids = BTreeSet::new();
        for mask in source_masks {
            if mask.created_revision() > context.recorded_as_of().revision()
                || !archive_visible_masks.contains(&mask.id())
                || !mask_is_authorized(policy, principal, mask)
                || !record_context_is_relevant(store, context, mask.context())
                || !mask_selector_is_relevant(mask, slot, context, &assertion_ids)
            {
                continue;
            }
            count_preview_work(context, &mut work_units)?;
            mask_ids.insert(mask.id());
            if let Some(validity) = mask.validity() {
                add_interval_if_timeline_exists(
                    context,
                    &mut timeline_edges,
                    validity.interval(),
                    memory_reservations,
                )?;
            }
        }
        for closure in source_mask_closures {
            if mask_ids.contains(&closure.mask_id())
                && closure.created_revision() <= context.recorded_as_of().revision()
            {
                count_preview_work(context, &mut work_units)?;
                add_endpoint_if_timeline_exists(
                    context,
                    &mut timeline_edges,
                    closure.close_at_world_time(),
                    memory_reservations,
                )?;
            }
        }
        for retraction in source_mask_retractions {
            if mask_ids.contains(&retraction.mask_id())
                && retraction.created_revision() <= context.recorded_as_of().revision()
            {
                count_preview_work(context, &mut work_units)?;
            }
        }
    }

    let boundary_source = request.boundaries;
    let mut boundary_ids = BTreeSet::new();
    for boundary in boundary_source.boundaries {
        if boundary.created_revision() > context.recorded_as_of().revision()
            || !boundary_source
                .archive_visible_boundaries
                .contains(&boundary.id())
            || !boundary_is_authorized(policy, principal, boundary)
            || boundary.subject() != slot.subject()
            || boundary.predicate_id() != slot.predicate_id()
            || !record_context_is_relevant(store, context, boundary.context())
        {
            continue;
        }
        count_preview_work(context, &mut work_units)?;
        boundary_ids.insert(boundary.id());
        if let Some(validity) = boundary.validity() {
            add_interval_endpoints(
                context,
                &mut timeline_edges,
                validity.interval(),
                memory_reservations,
            )?;
        }
    }
    for closure in boundary_source.closures {
        if boundary_ids.contains(&closure.replacement_boundary_id())
            && closure.created_revision() <= context.recorded_as_of().revision()
        {
            count_preview_work(context, &mut work_units)?;
            add_endpoint_if_timeline_exists(
                context,
                &mut timeline_edges,
                closure.close_at_world_time(),
                memory_reservations,
            )?;
        }
    }
    for retraction in boundary_source.retractions {
        if boundary_ids.contains(&retraction.replacement_boundary_id())
            && retraction.created_revision() <= context.recorded_as_of().revision()
        {
            count_preview_work(context, &mut work_units)?;
        }
    }

    if timeline_edges.is_empty() {
        return Ok(ResolutionTimePlan {
            partitions: Vec::new(),
            assertion_ids,
        });
    }

    let partitions = build_resolution_partitions(context, &timeline_edges, memory_reservations)?;
    Ok(ResolutionTimePlan {
        partitions,
        assertion_ids,
    })
}

fn count_preview_work(
    context: &QueryContext,
    work_units: &mut u64,
) -> Result<(), QueryEngineError> {
    if *work_units >= context.budget().max_work_units().get() {
        return Err(QueryEngineError::BudgetExceeded(BudgetDimension::WorkUnits));
    }
    *work_units += 1;
    Ok(())
}

fn reserve_query_memory(
    context: &QueryContext,
    bytes: usize,
    reservations: &mut Vec<MemoryReservation>,
) -> Result<(), QueryEngineError> {
    reservations
        .try_reserve(1)
        .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
    let bytes = u64::try_from(bytes).map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
    reservations.push(
        context
            .resource_budget()
            .reserve(ResourceClass::Query, bytes)
            .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?,
    );
    Ok(())
}

fn add_timepoint(
    context: &QueryContext,
    timeline_edges: &mut BTreeMap<TimelineId, BTreeSet<i128>>,
    point: WorldTime,
    reservations: &mut Vec<MemoryReservation>,
) -> Result<(), QueryEngineError> {
    let timeline_id = point.timeline().id();
    if let std::collections::btree_map::Entry::Vacant(entry) = timeline_edges.entry(timeline_id) {
        reserve_query_memory(
            context,
            size_of::<(TimelineId, BTreeSet<i128>)>(),
            reservations,
        )?;
        entry.insert(BTreeSet::new());
    }
    let Some(edges) = timeline_edges.get_mut(&timeline_id) else {
        return Err(QueryEngineError::ResourceBudgetExceeded);
    };
    if !edges.contains(&point.nanoseconds()) {
        reserve_query_memory(context, size_of::<i128>(), reservations)?;
        edges.insert(point.nanoseconds());
    }
    Ok(())
}

fn add_interval_endpoints(
    context: &QueryContext,
    timeline_edges: &mut BTreeMap<TimelineId, BTreeSet<i128>>,
    interval: TimeInterval,
    reservations: &mut Vec<MemoryReservation>,
) -> Result<(), QueryEngineError> {
    let timeline_id = interval.timeline().id();
    if let std::collections::btree_map::Entry::Vacant(entry) = timeline_edges.entry(timeline_id) {
        reserve_query_memory(
            context,
            size_of::<(TimelineId, BTreeSet<i128>)>(),
            reservations,
        )?;
        entry.insert(BTreeSet::new());
    }
    for endpoint in [interval.start(), interval.end()].into_iter().flatten() {
        add_timepoint(context, timeline_edges, endpoint, reservations)?;
    }
    Ok(())
}

fn add_endpoint_if_timeline_exists(
    context: &QueryContext,
    timeline_edges: &mut BTreeMap<TimelineId, BTreeSet<i128>>,
    point: WorldTime,
    reservations: &mut Vec<MemoryReservation>,
) -> Result<(), QueryEngineError> {
    if timeline_edges.contains_key(&point.timeline().id()) {
        add_timepoint(context, timeline_edges, point, reservations)?;
    }
    Ok(())
}

fn add_interval_if_timeline_exists(
    context: &QueryContext,
    timeline_edges: &mut BTreeMap<TimelineId, BTreeSet<i128>>,
    interval: TimeInterval,
    reservations: &mut Vec<MemoryReservation>,
) -> Result<(), QueryEngineError> {
    if timeline_edges.contains_key(&interval.timeline().id()) {
        for endpoint in [interval.start(), interval.end()].into_iter().flatten() {
            add_timepoint(context, timeline_edges, endpoint, reservations)?;
        }
    }
    Ok(())
}

fn build_resolution_partitions(
    context: &QueryContext,
    timeline_edges: &BTreeMap<TimelineId, BTreeSet<i128>>,
    memory_reservations: &mut Vec<MemoryReservation>,
) -> Result<Vec<ResolutionPartition>, QueryEngineError> {
    let partition_capacity = timeline_edges.values().try_fold(0_usize, |total, edges| {
        let count = match edges.first() {
            Some(first) => edges
                .len()
                .checked_add(if *first > i128::MIN { 1 } else { 0 })?,
            None => 1,
        };
        total.checked_add(count)
    });
    let partition_capacity = partition_capacity.ok_or(QueryEngineError::ResourceBudgetExceeded)?;
    enforce_result_limit(context, partition_capacity)?;
    let partition_bytes = partition_capacity
        .checked_mul(size_of::<ResolutionPartition>())
        .ok_or(QueryEngineError::ResourceBudgetExceeded)?;
    reserve_query_memory(context, partition_bytes, memory_reservations)?;
    let mut partitions = Vec::new();
    partitions
        .try_reserve_exact(partition_capacity)
        .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
    for (timeline_id, edges) in timeline_edges {
        let timeline = Timeline::new(*timeline_id);
        if edges.is_empty() {
            partitions.push(ResolutionPartition {
                interval: TimeInterval::new(timeline, None, None)
                    .map_err(|_| QueryEngineError::Context)?,
                sample: WorldTime::from_nanoseconds(timeline, i128::MIN),
            });
            continue;
        }
        let mut points = Vec::new();
        points
            .try_reserve_exact(edges.len())
            .map_err(|_| QueryEngineError::ResourceBudgetExceeded)?;
        points.extend(edges.iter().copied());
        let Some(first) = points.first().copied() else {
            continue;
        };
        if first > i128::MIN {
            partitions.push(ResolutionPartition {
                interval: TimeInterval::new(
                    timeline,
                    None,
                    Some(WorldTime::from_nanoseconds(timeline, first)),
                )
                .map_err(|_| QueryEngineError::Context)?,
                sample: WorldTime::from_nanoseconds(timeline, i128::MIN),
            });
        }
        for pair in points.windows(2) {
            let (Some(start), Some(end)) = (pair.first().copied(), pair.get(1).copied()) else {
                continue;
            };
            if start < end {
                partitions.push(ResolutionPartition {
                    interval: TimeInterval::new(
                        timeline,
                        Some(WorldTime::from_nanoseconds(timeline, start)),
                        Some(WorldTime::from_nanoseconds(timeline, end)),
                    )
                    .map_err(|_| QueryEngineError::Context)?,
                    sample: WorldTime::from_nanoseconds(timeline, start),
                });
            }
        }
        let last = points.last().copied().ok_or(QueryEngineError::Context)?;
        partitions.push(ResolutionPartition {
            interval: TimeInterval::new(
                timeline,
                Some(WorldTime::from_nanoseconds(timeline, last)),
                None,
            )
            .map_err(|_| QueryEngineError::Context)?,
            sample: WorldTime::from_nanoseconds(timeline, last),
        });
    }
    Ok(partitions)
}

fn record_context_is_relevant(
    store: AssertionQueryStore<'_>,
    query_context: &QueryContext,
    record_context: ContextKey,
) -> bool {
    record_context.perspective_scope() == query_context.perspective()
        && record_context.epistemic_mode() == query_context.epistemic_mode()
        && query_context
            .layers()
            .resolved()
            .as_slice()
            .contains(&record_context.layer_id())
        && ContextPrecedence::for_context(
            query_context.history_space(),
            record_context.history_space_id(),
            record_context.layer_id(),
            store.history.catalog(),
            store.layers,
        )
        .is_ok()
}

fn mask_selector_is_relevant(
    mask: &Mask,
    slot: MultiValueSlot,
    context: &QueryContext,
    assertion_ids: &BTreeSet<AssertionId>,
) -> bool {
    match mask.selector() {
        MaskSelector::ExactAssertion(assertion_id) => assertion_ids.contains(assertion_id),
        MaskSelector::Proposition(proposition) => {
            proposition.subject() == slot.subject()
                && proposition.predicate_id() == slot.predicate_id()
        }
        MaskSelector::Slot(selector) => {
            selector.subject() == slot.subject()
                && selector.predicate_id() == slot.predicate_id()
                && selector.perspective_scope() == context.perspective()
                && selector.epistemic_mode() == context.epistemic_mode()
        }
    }
}

fn masks_for_time(
    store: AssertionQueryStore<'_>,
    history: AuthorizedAssertionMaskHistory<'_>,
    context: &QueryContext,
    slot: MultiValueSlot,
    assertion_ids: &BTreeSet<AssertionId>,
    timeline_id: TimelineId,
    policy: &SecurityPolicySnapshot,
) -> OwnedMaskHistory {
    let principal = context.security().principal_id();
    let (source_masks, source_closures, source_retractions, archive_visible) = history.records();
    let mut owned = OwnedMaskHistory::default();
    let mut mask_ids = BTreeSet::new();
    for mask in source_masks {
        if mask.created_revision() > context.recorded_as_of().revision()
            || !archive_visible.contains(&mask.id())
            || !mask_is_authorized(policy, principal, mask)
            || !record_context_is_relevant(store, context, mask.context())
            || !mask_selector_is_relevant(mask, slot, context, assertion_ids)
            || mask
                .validity()
                .is_some_and(|validity| validity.interval().timeline().id() != timeline_id)
        {
            continue;
        }
        mask_ids.insert(mask.id());
        owned.archive_visible.insert(mask.id());
        owned.masks.push(mask.clone());
    }
    for closure in source_closures {
        if mask_ids.contains(&closure.mask_id())
            && closure.created_revision() <= context.recorded_as_of().revision()
            && closure.close_at_world_time().timeline().id() == timeline_id
        {
            owned.closures.push(*closure);
        }
    }
    for retraction in source_retractions {
        if mask_ids.contains(&retraction.mask_id())
            && retraction.created_revision() <= context.recorded_as_of().revision()
        {
            owned.retractions.push(retraction.clone());
        }
    }
    owned
}

fn boundaries_for_time(
    store: AssertionQueryStore<'_>,
    source: ReplacementBoundarySource<'_>,
    context: &QueryContext,
    slot: MultiValueSlot,
    timeline_id: TimelineId,
    policy: &SecurityPolicySnapshot,
) -> OwnedBoundaryHistory {
    let principal = context.security().principal_id();
    let mut owned = OwnedBoundaryHistory::default();
    let mut boundary_ids = BTreeSet::new();
    for boundary in source.boundaries {
        if boundary.created_revision() > context.recorded_as_of().revision()
            || !source.archive_visible_boundaries.contains(&boundary.id())
            || !boundary_is_authorized(policy, principal, boundary)
            || boundary.subject() != slot.subject()
            || boundary.predicate_id() != slot.predicate_id()
            || !record_context_is_relevant(store, context, boundary.context())
            || boundary
                .validity()
                .is_some_and(|validity| validity.interval().timeline().id() != timeline_id)
        {
            continue;
        }
        boundary_ids.insert(boundary.id());
        owned.archive_visible.insert(boundary.id());
        owned.boundaries.push(boundary.clone());
    }
    for closure in source.closures {
        if boundary_ids.contains(&closure.replacement_boundary_id())
            && closure.created_revision() <= context.recorded_as_of().revision()
            && closure.close_at_world_time().timeline().id() == timeline_id
        {
            owned.closures.push(*closure);
        }
    }
    for retraction in source.retractions {
        if boundary_ids.contains(&retraction.replacement_boundary_id())
            && retraction.created_revision() <= context.recorded_as_of().revision()
        {
            owned.retractions.push(retraction.clone());
        }
    }
    owned
}

fn preview_contributors_are_visible(
    preview: &ResolutionPreview,
    visible_assertions: &BTreeSet<AssertionId>,
) -> bool {
    let all_visible = |view: &ResolvedView| {
        view.contributors()
            .iter()
            .all(|assertion_id| visible_assertions.contains(assertion_id))
    };
    match preview {
        ResolutionPreview::Point { resolved_view, .. } => all_visible(resolved_view),
        ResolutionPreview::AllTimes { slices } => {
            !slices.is_empty() && slices.iter().all(|slice| all_visible(&slice.resolved_view))
        }
        ResolutionPreview::CompleteEmpty => true,
    }
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

fn map_cursor_error(error: CursorStateError) -> QueryEngineError {
    match error {
        CursorStateError::CursorInvalidated => QueryEngineError::CursorInvalidated,
        CursorStateError::ZeroEntryLimit
        | CursorStateError::ZeroByteLimit
        | CursorStateError::ZeroTtlLimit
        | CursorStateError::InvalidExpiry
        | CursorStateError::TtlExceedsLimit
        | CursorStateError::StateTooLarge
        | CursorStateError::CapacityExceeded
        | CursorStateError::EntropyUnavailable
        | CursorStateError::RandomHandleCollision => QueryEngineError::CursorUnavailable,
    }
}

/// Safe, ID-free errors from the productive query boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryEngineError {
    Cancelled,
    Unauthorized,
    SnapshotExpired,
    SnapshotPinRequired,
    InvalidPageRequest,
    InvalidPageOrder,
    CursorInvalidated,
    CursorUnavailable,
    ExplainDenied,
    AllTimesResolutionUnavailable,
    EmptyLayerSelection,
    IndexPayloadUnavailable,
    IndexConfigurationUnavailable,
    BudgetExceeded(BudgetDimension),
    ResourceBudgetExceeded,
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
    Graph(GraphError),
    Aggregate(AggregateError),
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
            Self::Unauthorized => "query authorization is denied",
            Self::SnapshotExpired => "query snapshot has expired",
            Self::SnapshotPinRequired => "paged query requires a leased snapshot",
            Self::InvalidPageRequest => "query page request is invalid",
            Self::InvalidPageOrder => "query result ordering is invalid",
            Self::CursorInvalidated => "query cursor is invalidated",
            Self::CursorUnavailable => "query cursor state is unavailable",
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
            Self::ResourceBudgetExceeded => {
                "query process memory budget does not allow a complete result"
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
            Self::Graph(_) => "query graph traversal could not be completed",
            Self::Aggregate(_) => "query aggregation could not be completed",
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

impl From<RawHistoryError> for QueryEngineError {
    fn from(error: RawHistoryError) -> Self {
        match error {
            RawHistoryError::BudgetExceeded(dimension) => Self::BudgetExceeded(dimension),
            RawHistoryError::History(_)
            | RawHistoryError::SecurityHistory(_)
            | RawHistoryError::DuplicateRecordRef { .. }
            | RawHistoryError::AllocationFailed => Self::RawHistory,
        }
    }
}
error_from!(SecurityPolicyHistoryError, SecurityHistory);
impl From<CandidateScanError> for QueryEngineError {
    fn from(error: CandidateScanError) -> Self {
        match error {
            CandidateScanError::BudgetExceeded(dimension) => Self::BudgetExceeded(dimension),
            CandidateScanError::ResourceBudgetExceeded => Self::ResourceBudgetExceeded,
            _ => Self::CandidateHistory,
        }
    }
}
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

impl From<GraphError> for QueryEngineError {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}

impl From<AggregateError> for QueryEngineError {
    fn from(error: AggregateError) -> Self {
        Self::Aggregate(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PageOperation;
    use crate::archive::{ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition};
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
    use crate::multi_value_resolution::MultiValueOutcome;
    use crate::query_context::{
        AuthorizationMode, CancellationToken, QueryBudget, QueryBudgetLimits, QueryContextInput,
        SecurityContext, ValidatedLayerSelection,
    };
    use crate::record_refs::SnapshotRef;
    use crate::reference_query::{HistoricalQueryBinding, ResolvedOutcome, ResolvedView};
    use crate::schema::{
        Cardinality, ConstraintSet, EntityTypeConstraint, Lifecycle, PredicateDefinitionSpec,
        ResolutionPolicy, ValueKind,
    };
    use crate::schema_history::{SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode};
    use crate::security::{
        CapabilityGrant, CapabilityRule, GrantEffect, PolicyBundle, PolicyScope, PolicySubject,
        Principal, RoleAssignment, RoleDefinition, SecurityPolicyVersion,
    };
    use crate::single_value_resolution::SingleValueOutcome;
    use crate::temporal::{AssertionValidity, RecordedAsOf, TimeInterval, Timeline, WorldTime};
    use crate::values::{Symbol, Value};
    use crate::{
        HistorySpaceView, SnapshotBinding, SnapshotBindingInput, SnapshotLifetimeLimits,
        SnapshotPinPurpose, SnapshotRegistry, SnapshotSecurityBinding,
    };
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
        Snapshot(crate::SnapshotError),
        History(crate::history_model::HistorySpaceModelError),
        Assertion(crate::assertions::AssertionRecordError),
        Archive(crate::archive_projection::ArchiveProjectionError),
        ArchiveTransition(crate::archive::ArchiveTransitionError),
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
                Self::Snapshot(error) => Some(error),
                Self::History(error) => Some(error),
                Self::Assertion(error) => Some(error),
                Self::Archive(error) => Some(error),
                Self::ArchiveTransition(error) => Some(error),
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
    test_error_from!(crate::SnapshotError, Snapshot);
    test_error_from!(crate::history_model::HistorySpaceModelError, History);
    test_error_from!(crate::assertions::AssertionRecordError, Assertion);
    test_error_from!(crate::archive_projection::ArchiveProjectionError, Archive);
    test_error_from!(crate::archive::ArchiveTransitionError, ArchiveTransition);
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
        fixture_with_generated_assertions(0, 0)
    }

    fn fixture_with_generated_assertions(
        generated_assertion_count: usize,
        seed: u64,
    ) -> TestResult<Fixture> {
        if generated_assertion_count > 200 {
            return Err(
                std::io::Error::other("generated assertion count exceeds test ID space").into(),
            );
        }
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
        let (candidate_limit, work_limit, result_limit) = if generated_assertion_count == 0 {
            (100, 1_000, 100)
        } else {
            (10_000, 20_000, 10_000)
        };
        let budget_limits = QueryBudgetLimits::new(candidate_limit, work_limit, result_limit)?;
        let budget = QueryBudget::new(candidate_limit, work_limit, result_limit, budget_limits)?;
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
        let make_generated_assertion = |assertion_id: AssertionId,
                                        assertion_subject: Subject,
                                        assertion_predicate_id: PredicateId,
                                        value: String,
                                        polarity: Polarity,
                                        validity: AssertionValidity|
         -> TestResult<Assertion> {
            let assertion_context = ContextKey::new(
                root,
                base,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?;
            Ok(Assertion::new(
                assertion_id,
                AssertionDraft::new(
                    assertion_context,
                    assertion_subject,
                    assertion_predicate_id,
                    Value::String(value),
                    polarity,
                    validity,
                ),
                revision,
            ))
        };
        let all_time_validity = AssertionValidity::new(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, 0)),
            Some(WorldTime::from_nanoseconds(timeline, 100)),
        )?);
        let mut generated_assertions = Vec::with_capacity(16 + generated_assertion_count);
        if generated_assertion_count > 0 {
            for subject_offset in 0_u8..8 {
                let subject_tail = 40 + subject_offset;
                let assertion_subject = Subject::new(id::<EntityId>(subject_tail)?);
                for side in 0_u8..2 {
                    let assertion_tail = 32 + subject_offset * 2 + side;
                    let value = if subject_offset == 0 {
                        if side == 0 {
                            "mutant-left"
                        } else {
                            "mutant-right"
                        }
                        .to_owned()
                    } else if side == 0 {
                        format!("seed-{subject_tail}-left")
                    } else {
                        format!("seed-{subject_tail}-right")
                    };
                    generated_assertions.push(make_generated_assertion(
                        id::<AssertionId>(assertion_tail)?,
                        assertion_subject,
                        predicate_id,
                        value,
                        Polarity::Positive,
                        all_time_validity,
                    )?);
                }
            }
        }
        let validity_patterns = [
            (Some(0), Some(100)),
            (Some(50), Some(100)),
            (Some(51), Some(100)),
            (Some(0), Some(50)),
            (None, Some(50)),
            (None, None),
        ];
        let mut random_state = seed;
        for offset in 0..generated_assertion_count {
            random_state = random_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let subject_tail = 41
                + u8::try_from(random_state % 7).map_err(|_| {
                    std::io::Error::other("generated subject ID escaped the test ID space")
                })?;
            let assertion_subject = Subject::new(id::<EntityId>(subject_tail)?);
            let predicate_for_record = if random_state & 0x8 == 0 {
                predicate_id
            } else {
                multi_predicate_id
            };
            let value = format!("generated-{:02}", (random_state >> 4) % 16);
            let polarity = if random_state & 0x10 == 0 {
                Polarity::Positive
            } else {
                Polarity::Negative
            };
            let validity_index = usize::try_from((random_state >> 8) % 6)
                .map_err(|_| std::io::Error::other("generated validity index is out of range"))?;
            let Some((start, end)) = validity_patterns.get(validity_index).copied() else {
                return Err(
                    std::io::Error::other("generated validity index is out of range").into(),
                );
            };
            let interval = TimeInterval::new(
                timeline,
                start.map(|value| WorldTime::from_nanoseconds(timeline, value)),
                end.map(|value| WorldTime::from_nanoseconds(timeline, value)),
            )?;
            let assertion_tail = u8::try_from(48 + offset).map_err(|_| {
                std::io::Error::other("generated assertion ID escaped the test ID space")
            })?;
            generated_assertions.push(make_generated_assertion(
                id::<AssertionId>(assertion_tail)?,
                assertion_subject,
                predicate_for_record,
                value,
                polarity,
                AssertionValidity::new(interval),
            )?);
        }
        let mut history = HistorySpaceReferenceModel::new(vec![HistorySpaceDefinition::new(
            root,
            None,
            Revision::GENESIS,
        )?])?;
        let mut root_records = vec![
            AssertionHistoryRecord::from_assertion(hidden.clone()),
            AssertionHistoryRecord::from_assertion(second_visible.clone()),
            AssertionHistoryRecord::from_assertion(visible.clone()),
            AssertionHistoryRecord::from_assertion(multi_assertion.clone()),
        ];
        root_records.extend(
            generated_assertions
                .iter()
                .cloned()
                .map(AssertionHistoryRecord::from_assertion),
        );
        history.publish(root, root_records)?;
        history.add_history_space(HistorySpaceDefinition::new(child, Some(root), revision)?)?;
        assert_eq!(history.publish(child, Vec::new())?, query_revision);
        let archived_assertions = [hidden, second_visible, visible, multi_assertion]
            .into_iter()
            .chain(generated_assertions.iter().cloned())
            .collect::<Vec<_>>();
        let archive = ArchiveHistoryReferenceModel::new(
            archived_assertions
                .iter()
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

    fn single_outcomes_match(left: &SingleValueOutcome, right: &SingleValueOutcome) -> bool {
        match (left, right) {
            (
                SingleValueOutcome::Known {
                    value: Value::String(left_value),
                    polarity: left_polarity,
                    contributors: left_contributors,
                },
                SingleValueOutcome::Known {
                    value: Value::String(right_value),
                    polarity: right_polarity,
                    contributors: right_contributors,
                },
            ) => {
                left_value == right_value
                    && left_polarity == right_polarity
                    && left_contributors == right_contributors
            }
            (SingleValueOutcome::Unknown, SingleValueOutcome::Unknown) => true,
            (
                SingleValueOutcome::Conflict {
                    contributors: left_contributors,
                },
                SingleValueOutcome::Conflict {
                    contributors: right_contributors,
                },
            ) => left_contributors == right_contributors,
            _ => false,
        }
    }

    #[test]
    fn all_times_preview_partitions_validity_and_preserves_unknown_regions() -> TestResult {
        let fixture = fixture()?;
        let context = context_with_world_time(&fixture.context, WorldTimeSelector::AllTimes)?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let preview = ProductiveQueryEngine::resolution_preview(
            query_store(&fixture, &fixture.policies),
            point_request(
                &context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;

        let ResolutionPreview::AllTimes { slices } = preview.query().value() else {
            return Err(
                std::io::Error::other("visible assertion history must produce slices").into(),
            );
        };
        assert_eq!(slices.len(), 3);
        let mut slices = slices.iter();
        let (Some(before), Some(during), Some(after)) =
            (slices.next(), slices.next(), slices.next())
        else {
            return Err(std::io::Error::other("all expected time slices must be present").into());
        };
        assert!(slices.next().is_none());
        assert_eq!(before.interval().start(), None);
        assert_eq!(before.interval().end().map(WorldTime::nanoseconds), Some(0));
        assert!(matches!(
            before.resolved_view().outcome(),
            ResolvedOutcome::Single(SingleValueOutcome::Unknown)
        ));
        assert_eq!(
            during.interval().start().map(WorldTime::nanoseconds),
            Some(0)
        );
        assert_eq!(
            during.interval().end().map(WorldTime::nanoseconds),
            Some(100)
        );
        assert!(matches!(
            during.resolved_view().outcome(),
            ResolvedOutcome::Single(SingleValueOutcome::Known { .. })
        ));
        assert_eq!(
            after.interval().start().map(WorldTime::nanoseconds),
            Some(100)
        );
        assert_eq!(after.interval().end(), None);
        assert!(matches!(
            after.resolved_view().outcome(),
            ResolvedOutcome::Single(SingleValueOutcome::Unknown)
        ));
        Ok(())
    }

    #[test]
    fn all_times_preview_preserves_conflict_outcomes() -> TestResult {
        let fixture = fixture_with_generated_assertions(1, 0x5eed)?;
        let context = context_with_world_time(&fixture.context, WorldTimeSelector::AllTimes)?;
        let slot = MultiValueSlot::new(
            Subject::new(id::<EntityId>(40)?),
            fixture.slot.predicate_id(),
        );
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let preview = ProductiveQueryEngine::resolution_preview(
            query_store(&fixture, &fixture.policies),
            point_request(
                &context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        let ResolutionPreview::AllTimes { slices } = preview.query().value() else {
            return Err(
                std::io::Error::other("seeded conflict history must produce slices").into(),
            );
        };
        let Some(conflict_slice) = slices.get(1) else {
            return Err(std::io::Error::other("conflict interval slice is missing").into());
        };
        assert!(matches!(
            conflict_slice.resolved_view().outcome(),
            ResolvedOutcome::Single(SingleValueOutcome::Conflict { .. })
        ));
        Ok(())
    }

    #[test]
    fn all_times_preview_reports_boundary_defined_empty_set() -> TestResult {
        let fixture = fixture()?;
        let context = context_with_world_time(&fixture.context, WorldTimeSelector::AllTimes)?;
        let slot = MultiValueSlot::new(
            Subject::new(id::<EntityId>(23)?),
            fixture.multi_slot.predicate_id(),
        );
        let timeline = Timeline::new(id::<TimelineId>(5)?);
        let validity = AssertionValidity::new(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, 40)),
            Some(WorldTime::from_nanoseconds(timeline, 60)),
        )?);
        let boundary_context = ContextKey::new(
            fixture.context.history_space(),
            id::<LayerId>(2)?,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let boundary = ReplacementBoundary::new(
            id::<ReplacementBoundaryId>(21)?,
            boundary_context,
            slot.subject(),
            &fixture.multi_predicate,
            Some(validity),
            revision(2)?,
        )?;
        let visible_boundaries = [boundary.id()].into_iter().collect();
        let boundaries = ReplacementBoundarySource::new(
            std::slice::from_ref(&boundary),
            &[],
            &[],
            &visible_boundaries,
        );
        let (masks, _) = empty_mask_and_boundary_sources();
        let preview = ProductiveQueryEngine::resolution_preview(
            query_store(&fixture, &fixture.policies),
            point_request(
                &context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            slot,
            &fixture.multi_predicate,
            |_, _| Ok(false),
        )?;

        let ResolutionPreview::AllTimes { slices } = preview.query().value() else {
            return Err(std::io::Error::other("boundary history must produce time slices").into());
        };
        assert_eq!(slices.len(), 3);
        let Some(empty_set_slice) = slices.get(1) else {
            return Err(std::io::Error::other("boundary interval slice is missing").into());
        };
        assert_eq!(
            empty_set_slice
                .interval()
                .start()
                .map(WorldTime::nanoseconds),
            Some(40)
        );
        assert_eq!(
            empty_set_slice.interval().end().map(WorldTime::nanoseconds),
            Some(60)
        );
        assert!(matches!(
            empty_set_slice.resolved_view().outcome(),
            ResolvedOutcome::Multi(MultiValueOutcome::Known { values }) if values.is_empty()
        ));
        Ok(())
    }

    #[test]
    fn complete_empty_history_is_distinct_from_point_unknown_and_budget_errors() -> TestResult {
        let fixture = fixture()?;
        let unasserted_subject = Subject::new(id::<EntityId>(23)?);
        let slot = MultiValueSlot::new(unasserted_subject, fixture.slot.predicate_id());
        let all_times_context =
            context_with_world_time(&fixture.context, WorldTimeSelector::AllTimes)?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let preview = ProductiveQueryEngine::resolution_preview(
            query_store(&fixture, &fixture.policies),
            point_request(
                &all_times_context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        assert!(matches!(
            preview.query().value(),
            ResolutionPreview::CompleteEmpty
        ));

        let point_context = context_with_world_time(
            &fixture.context,
            WorldTimeSelector::At(WorldTime::from_nanoseconds(
                Timeline::new(id::<TimelineId>(5)?),
                50,
            )),
        )?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let point = ProductiveQueryEngine::resolution_preview(
            query_store(&fixture, &fixture.policies),
            point_request(
                &point_context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;
        assert!(matches!(
            point.query().value(),
            ResolutionPreview::Point { resolved_view, .. }
                if matches!(resolved_view.outcome(), ResolvedOutcome::Single(SingleValueOutcome::Unknown))
        ));

        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let error = ProductiveQueryEngine::resolution_preview(
            query_store(&fixture, &fixture.policies),
            point_request(
                &all_times_context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Exceeded(BudgetDimension::Candidates),
            ),
            slot,
            &fixture.predicate,
            |_, _| Ok(false),
        );
        assert!(matches!(
            error,
            Err(QueryEngineError::FullScanBudgetExceeded(
                BudgetDimension::Candidates
            ))
        ));

        let profile = crate::ProcessResourceProfile::new(
            1,
            1,
            1,
            crate::ResourceClassLimits::new(1, 1, 1, 1, 1),
        )
        .map_err(std::io::Error::other)?;
        let memory_budget = profile.memory_budget();
        let memory_limited_context =
            context_with_process_memory_budget(&all_times_context, memory_budget.clone())?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let memory_error = ProductiveQueryEngine::resolution_preview(
            query_store(&fixture, &fixture.policies),
            point_request(
                &memory_limited_context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        );
        assert!(matches!(
            memory_error,
            Err(QueryEngineError::ResourceBudgetExceeded)
        ));
        assert_eq!(
            memory_budget
                .reserved_bytes()
                .map_err(std::io::Error::other)?,
            0
        );
        Ok(())
    }

    #[test]
    fn m6_13_seeded_scaled_indexed_queries_match_the_full_scan_oracle() -> TestResult {
        let seeds = [0x5eed_u64, 0xcafe_u64, 0xdecafbad_u64];
        for (seed_index, seed) in seeds.into_iter().enumerate() {
            let fixture = fixture_with_generated_assertions(128, seed)?;
            let world_time = match fixture.context.world_time() {
                WorldTimeSelector::At(world_time) => world_time,
                WorldTimeSelector::AllTimes => {
                    return Err(std::io::Error::other(
                        "M6-13 fixture must bind one concrete WorldTime",
                    )
                    .into());
                }
            };
            let record_query = AssertionCandidateQuery::new(
                fixture.context.history_space(),
                fixture.context.layers().requested().clone(),
                fixture.context.perspective(),
                fixture.context.epistemic_mode(),
                fixture.context.recorded_as_of(),
                world_time,
            );
            for subject_tail in 40_u8..48 {
                let slot = MultiValueSlot::new(
                    Subject::new(id::<EntityId>(subject_tail)?),
                    fixture.slot.predicate_id(),
                );
                for (policy_index, policies) in [&fixture.policies, &fixture.hidden_policies]
                    .into_iter()
                    .enumerate()
                {
                    let policy = policies.resolve(&fixture.context)?.snapshot();
                    let oracle_candidates = full_scan_authorized_assertion_candidates(
                        &fixture.history,
                        &fixture.archive,
                        &record_query,
                        &fixture.layers,
                        policy,
                        &fixture.context,
                    )?;
                    let oracle = resolve_single_value_replace(
                        &oracle_candidates,
                        SingleValueSlot::new(slot.subject(), slot.predicate_id()),
                        &fixture.predicate,
                        |_, _| Ok(false),
                    )?;

                    let (masks, boundaries) = empty_mask_and_boundary_sources();
                    let indexed = ProductiveQueryEngine::resolved_point(
                        query_store(&fixture, policies),
                        point_request(
                            &fixture.context,
                            masks,
                            boundaries,
                            available_index(&fixture),
                            FullScanBudget::Available,
                        ),
                        slot,
                        &fixture.predicate,
                        |_, _| Ok(false),
                    )?;
                    assert_eq!(
                        indexed.path(),
                        QueryExecutionPath::Indexed { generation_id: 1 },
                        "seed {seed:#x}, subject {subject_tail} did not exercise the index"
                    );
                    let ResolvedOutcome::Single(indexed_outcome) =
                        indexed.query().value().outcome()
                    else {
                        return Err(std::io::Error::other(
                            "M6-13 single-value query returned a non-single outcome",
                        )
                        .into());
                    };
                    assert!(
                        single_outcomes_match(indexed_outcome, &oracle),
                        "M6-13 differential mismatch for seed {seed:#x}, subject {subject_tail}"
                    );

                    let (masks, boundaries) = empty_mask_and_boundary_sources();
                    let explained = ProductiveQueryEngine::explain_point(
                        query_store(&fixture, policies),
                        point_request(
                            &fixture.context,
                            masks,
                            boundaries,
                            available_index(&fixture),
                            FullScanBudget::Available,
                        ),
                        slot,
                        &fixture.predicate,
                        |_, _| Ok(false),
                    )?;
                    assert_eq!(
                        explained.path(),
                        QueryExecutionPath::Indexed { generation_id: 1 },
                        "seed {seed:#x}, subject {subject_tail} explain did not exercise the index"
                    );
                    assert!(single_outcomes_match(
                        match explained.query().value().resolved_view().outcome() {
                            ResolvedOutcome::Single(outcome) => outcome,
                            ResolvedOutcome::Multi(_) => {
                                return Err(std::io::Error::other(
                                    "M6-13 explain returned a non-single outcome",
                                )
                                .into());
                            }
                        },
                        &oracle,
                    ));

                    if seed_index == 0 && subject_tail == 40 && policy_index == 0 {
                        let (masks, boundaries) = empty_mask_and_boundary_sources();
                        let fallback = ProductiveQueryEngine::resolved_point(
                            query_store(&fixture, policies),
                            point_request(
                                &fixture.context,
                                masks,
                                boundaries,
                                AssertionPointIndexAccess::missing(),
                                FullScanBudget::Available,
                            ),
                            slot,
                            &fixture.predicate,
                            |_, _| Ok(false),
                        )?;
                        assert_eq!(
                            fallback.path(),
                            QueryExecutionPath::IndexFallback {
                                reason: IndexFallbackReason::Missing
                            }
                        );
                        let ResolvedOutcome::Single(fallback_outcome) =
                            fallback.query().value().outcome()
                        else {
                            return Err(std::io::Error::other(
                                "M6-13 fallback returned a non-single outcome",
                            )
                            .into());
                        };
                        assert!(single_outcomes_match(fallback_outcome, &oracle));

                        let mut incomplete_oracle = oracle_candidates.clone();
                        let prior_len = incomplete_oracle.len();
                        let omitted_id = id::<AssertionId>(32)?;
                        incomplete_oracle
                            .retain(|candidate| candidate.assertion().id() != omitted_id);
                        assert_eq!(incomplete_oracle.len() + 1, prior_len);
                        let mutant = resolve_single_value_replace(
                            &incomplete_oracle,
                            SingleValueSlot::new(slot.subject(), slot.predicate_id()),
                            &fixture.predicate,
                            |_, _| Ok(false),
                        )?;
                        assert!(
                            !single_outcomes_match(&oracle, &mutant),
                            "the differential check must reject a representative omitted-index result"
                        );
                    }
                }
            }
        }
        Ok(())
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
    fn hidden_index_entries_do_not_consume_candidate_budget() -> TestResult {
        let fixture = fixture()?;
        let context = context_with_candidate_budget(&fixture.context, 2)?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let indexed = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.hidden_policies),
            point_request(
                &context,
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
            indexed.query().value().contributors(),
            &[fixture.visible_id, fixture.second_visible_id]
        );

        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let fallback = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.hidden_policies),
            point_request(
                &context,
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
            fallback.query().value().contributors(),
            indexed.query().value().contributors()
        );

        let too_small = context_with_candidate_budget(&fixture.context, 1)?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let indexed_error = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.hidden_policies),
            point_request(
                &too_small,
                masks,
                boundaries,
                available_index(&fixture),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        );
        assert!(matches!(
            indexed_error,
            Err(QueryEngineError::BudgetExceeded(
                BudgetDimension::Candidates
            ))
        ));

        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let fallback_error = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.hidden_policies),
            point_request(
                &too_small,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        );
        assert!(matches!(
            fallback_error,
            Err(QueryEngineError::BudgetExceeded(
                BudgetDimension::Candidates
            ))
        ));
        Ok(())
    }

    #[test]
    fn process_memory_admission_fails_closed_and_releases_partial_reservations() -> TestResult {
        let fixture = fixture()?;
        let profile = crate::ProcessResourceProfile::new(
            1,
            1,
            1,
            crate::ResourceClassLimits::new(1, 1, 1, 1, 1),
        )
        .map_err(std::io::Error::other)?;
        let memory_budget = profile.memory_budget();
        let context = context_with_process_memory_budget(&fixture.context, memory_budget.clone())?;
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let result = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &context,
                masks,
                boundaries,
                available_index(&fixture),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        );
        assert!(matches!(
            result,
            Err(QueryEngineError::ResourceBudgetExceeded)
        ));
        assert_eq!(
            memory_budget
                .reserved_bytes()
                .map_err(std::io::Error::other)?,
            0
        );

        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let fallback = ProductiveQueryEngine::resolved_point(
            query_store(&fixture, &fixture.policies),
            point_request(
                &context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        );
        assert!(matches!(
            fallback,
            Err(QueryEngineError::ResourceBudgetExceeded)
        ));
        assert_eq!(
            memory_budget
                .reserved_bytes()
                .map_err(std::io::Error::other)?,
            0
        );
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

    fn page_request(limit: u32, cursor: Option<Vec<u8>>) -> TestResult<PageRequest> {
        PageRequest::new(limit, cursor)
            .map_err(|error| TestError::Io(std::io::Error::other(format!("{error:?}"))))
    }

    fn page_execution(
        cursors: &mut crate::CursorStateStore,
        query_hash: crate::QueryHash,
        now_ms: u64,
    ) -> TestResult<PageExecution<'_>> {
        PageExecution::new(
            cursors,
            query_hash,
            PageOperation::ResolvedView,
            now_ms,
            10_000,
            16,
            QueryExecutionPath::FullScan,
        )
        .map_err(|error| TestError::Io(std::io::Error::other(format!("{error:?}"))))
    }

    fn cursor_store() -> TestResult<crate::CursorStateStore> {
        let limits = crate::CursorStoreLimits::new(8, 4_096, 10_000)
            .map_err(|error| TestError::Io(std::io::Error::other(error)))?;
        crate::CursorStateStore::new(limits)
            .map_err(|error| TestError::Io(std::io::Error::other(error)))
    }

    fn leased_page_context(
        context: &QueryContext,
        layer_schema: &LayerSchemaSnapshot,
    ) -> TestResult<QueryContext> {
        let root = id::<HistorySpaceId>(1)?;
        let child = context.history_space();
        let history_space = HistorySpaceView::new(vec![
            HistorySpaceDefinition::new(root, None, Revision::GENESIS)?,
            HistorySpaceDefinition::new(child, Some(root), revision(1)?)?,
        ])?;
        let binding = SnapshotBinding::new(SnapshotBindingInput {
            database_id: id::<crate::DatabaseId>(60)?,
            snapshot_id: context.snapshot().id(),
            data_revision: context.snapshot_revision(),
            recorded_as_of: context.recorded_as_of(),
            schema: context.schema_binding(),
            history_space,
            layer_schema: layer_schema.clone(),
            layer_selection: context.layers().requested().clone(),
            security: SnapshotSecurityBinding::new(
                context.security().principal_id(),
                context.security().authorization_mode(),
                SecurityEpoch::INITIAL,
            ),
            backend_generation: 60,
        })?;
        let limits = SnapshotLifetimeLimits::new(1_000, 5_000, 10_000)?;
        let registry = SnapshotRegistry::new(limits);
        let lease = registry.pin(binding, SnapshotPinPurpose::Interactive, 0)?;
        Ok(QueryContext::new_leased(
            QueryContextInput {
                snapshot: context.snapshot(),
                snapshot_revision: context.snapshot_revision(),
                recorded_as_of: context.recorded_as_of(),
                history_space: context.history_space(),
                layers: context.layers().clone(),
                world_time: context.world_time(),
                perspective: context.perspective(),
                epistemic_mode: context.epistemic_mode(),
                schema_binding: context.schema_binding(),
                security: context.security(),
                budget: context.budget(),
                cancellation: context.cancellation().clone(),
            },
            lease,
            0,
        )?)
    }

    fn context_with_result_budget(
        context: &QueryContext,
        max_results: u64,
    ) -> TestResult<QueryContext> {
        let limits = QueryBudgetLimits::new(100, 1_000, max_results)?;
        let budget = QueryBudget::new(100, 1_000, max_results, limits)?;
        Ok(QueryContext::new(QueryContextInput {
            snapshot: context.snapshot(),
            snapshot_revision: context.snapshot_revision(),
            recorded_as_of: context.recorded_as_of(),
            history_space: context.history_space(),
            layers: context.layers().clone(),
            world_time: context.world_time(),
            perspective: context.perspective(),
            epistemic_mode: context.epistemic_mode(),
            schema_binding: context.schema_binding(),
            security: context.security(),
            budget,
            cancellation: CancellationToken::new(),
        })?)
    }

    fn context_with_world_time(
        context: &QueryContext,
        world_time: WorldTimeSelector,
    ) -> TestResult<QueryContext> {
        Ok(QueryContext::new(QueryContextInput {
            snapshot: context.snapshot(),
            snapshot_revision: context.snapshot_revision(),
            recorded_as_of: context.recorded_as_of(),
            history_space: context.history_space(),
            layers: context.layers().clone(),
            world_time,
            perspective: context.perspective(),
            epistemic_mode: context.epistemic_mode(),
            schema_binding: context.schema_binding(),
            security: context.security(),
            budget: context.budget(),
            cancellation: context.cancellation().clone(),
        })?)
    }

    fn context_with_candidate_budget(
        context: &QueryContext,
        max_candidates: u64,
    ) -> TestResult<QueryContext> {
        let work = context.budget().max_work_units().get();
        let results = context.budget().max_results().get();
        let limits = QueryBudgetLimits::new(max_candidates, work, results)?;
        let budget = QueryBudget::new(max_candidates, work, results, limits)?;
        Ok(QueryContext::new(QueryContextInput {
            snapshot: context.snapshot(),
            snapshot_revision: context.snapshot_revision(),
            recorded_as_of: context.recorded_as_of(),
            history_space: context.history_space(),
            layers: context.layers().clone(),
            world_time: context.world_time(),
            perspective: context.perspective(),
            epistemic_mode: context.epistemic_mode(),
            schema_binding: context.schema_binding(),
            security: context.security(),
            budget,
            cancellation: CancellationToken::new(),
        })?)
    }

    fn context_with_process_memory_budget(
        context: &QueryContext,
        resource_budget: crate::ProcessMemoryBudget,
    ) -> TestResult<QueryContext> {
        Ok(QueryContext::new_with_resource_budget(
            QueryContextInput {
                snapshot: context.snapshot(),
                snapshot_revision: context.snapshot_revision(),
                recorded_as_of: context.recorded_as_of(),
                history_space: context.history_space(),
                layers: context.layers().clone(),
                world_time: context.world_time(),
                perspective: context.perspective(),
                epistemic_mode: context.epistemic_mode(),
                schema_binding: context.schema_binding(),
                security: context.security(),
                budget: context.budget(),
                cancellation: CancellationToken::new(),
            },
            resource_budget,
        )?)
    }

    #[test]
    fn productive_pages_pull_to_boundary_and_release_the_cursor_on_completion() -> TestResult {
        use std::cell::Cell;

        let fixture = fixture()?;
        let context = leased_page_context(&fixture.context, &fixture.layers)?;
        let query_hash = crate::QueryHash::new([51; 32]);
        let mut cursors = cursor_store()?;
        let pulled = Cell::new(0_usize);
        let source_pulls = &pulled;
        let first = ProductiveQueryEngine::stream_page(
            page_request(2, None)?,
            page_execution(&mut cursors, query_hash, 1_000)?,
            &context,
            &fixture.policies,
            move |after| {
                let after = after.map(<[u8]>::to_vec);
                Ok([1_u8, 2, 3, 4, 5]
                    .into_iter()
                    .filter(move |value| match after.as_ref() {
                        Some(key) => key.as_slice() < [*value].as_slice(),
                        None => true,
                    })
                    .map(move |value| {
                        source_pulls.set(source_pulls.get().saturating_add(1));
                        Ok(value)
                    }))
            },
            |value| vec![*value],
            |value, _, _| *value != 2,
        )?;

        assert_eq!(first.results(), &[1, 3]);
        assert!(first.next_cursor().is_some());
        assert_eq!(
            pulled.get(),
            4,
            "page pull includes only one visible lookahead"
        );
        assert_eq!(cursors.len(), 1);

        let continuation = first.next_cursor().map(<[u8]>::to_vec);
        let second = ProductiveQueryEngine::stream_page(
            page_request(2, continuation)?,
            page_execution(&mut cursors, query_hash, 2_000)?,
            &context,
            &fixture.policies,
            |after| {
                let after = after.map(<[u8]>::to_vec);
                Ok([1_u8, 2, 3, 4, 5]
                    .into_iter()
                    .filter(move |value| match after.as_ref() {
                        Some(key) => key.as_slice() < [*value].as_slice(),
                        None => true,
                    })
                    .map(Ok))
            },
            |value| vec![*value],
            |value, _, _| *value != 2,
        )?;
        assert_eq!(second.results(), &[4, 5]);
        assert!(second.next_cursor().is_none());
        assert_eq!(cursors.len(), 0, "the final page consumes its cursor state");
        assert!(
            first
                .query()
                .query_context_binding()
                .matches(&fixture.context)
        );
        Ok(())
    }

    #[test]
    fn all_times_preview_excludes_archived_assertion_timelines() -> TestResult {
        let fixture = fixture()?;
        let context = context_with_world_time(&fixture.context, WorldTimeSelector::AllTimes)?;
        let transitions = vec![
            ArchiveTransition::new(
                id::<crate::ArchiveTransitionId>(24)?,
                ArchiveTargetRef::Assertion(fixture.visible_id),
                ArchiveAction::Archive,
                ArchiveState::Unarchived,
                revision(2)?,
            )?,
            ArchiveTransition::new(
                id::<crate::ArchiveTransitionId>(25)?,
                ArchiveTargetRef::Assertion(fixture.second_visible_id),
                ArchiveAction::Archive,
                ArchiveState::Unarchived,
                revision(2)?,
            )?,
            ArchiveTransition::new(
                id::<crate::ArchiveTransitionId>(26)?,
                ArchiveTargetRef::Assertion(fixture.hidden_id),
                ArchiveAction::Archive,
                ArchiveState::Unarchived,
                revision(2)?,
            )?,
        ];
        let archive =
            ArchiveHistoryReferenceModel::new(fixture.archive.targets().to_vec(), transitions)?;
        let store = AssertionQueryStore::new(
            &fixture.history,
            &archive,
            &fixture.layers,
            &fixture.policies,
        );
        let (masks, boundaries) = empty_mask_and_boundary_sources();
        let preview = ProductiveQueryEngine::resolution_preview(
            store,
            point_request(
                &context,
                masks,
                boundaries,
                AssertionPointIndexAccess::missing(),
                FullScanBudget::Available,
            ),
            fixture.slot,
            &fixture.predicate,
            |_, _| Ok(false),
        )?;

        assert!(matches!(
            preview.query().value(),
            ResolutionPreview::CompleteEmpty
        ));
        Ok(())
    }

    #[test]
    fn terminal_page_error_releases_the_continuation() -> TestResult {
        let fixture = fixture()?;
        let context = leased_page_context(&fixture.context, &fixture.layers)?;
        let query_hash = crate::QueryHash::new([52; 32]);
        let mut cursors = cursor_store()?;
        let first = ProductiveQueryEngine::stream_page(
            page_request(1, None)?,
            page_execution(&mut cursors, query_hash, 1_000)?,
            &context,
            &fixture.policies,
            |_| Ok([Ok(1_u8), Ok(2)].into_iter()),
            |value| vec![*value],
            |_, _, _| true,
        )?;
        let continuation = first.next_cursor().map(<[u8]>::to_vec);
        assert_eq!(cursors.len(), 1);

        let failed = ProductiveQueryEngine::stream_page(
            page_request(1, continuation)?,
            page_execution(&mut cursors, query_hash, 2_000)?,
            &context,
            &fixture.policies,
            |_| Ok([Err(QueryEngineError::CandidateHistory)].into_iter()),
            |value| vec![*value],
            |_, _, _| true,
        );
        assert!(matches!(failed, Err(QueryEngineError::CandidateHistory)));
        assert_eq!(cursors.len(), 0);
        Ok(())
    }

    #[test]
    fn cancelled_continuation_releases_the_snapshot_cursor() -> TestResult {
        let fixture = fixture()?;
        let context = leased_page_context(&fixture.context, &fixture.layers)?;
        let query_hash = crate::QueryHash::new([54; 32]);
        let mut cursors = cursor_store()?;
        let first = ProductiveQueryEngine::stream_page(
            page_request(1, None)?,
            page_execution(&mut cursors, query_hash, 1_000)?,
            &context,
            &fixture.policies,
            |_| Ok([Ok(1_u8), Ok(2)].into_iter()),
            |value| vec![*value],
            |_, _, _| true,
        )?;
        let continuation = first.next_cursor().map(<[u8]>::to_vec);
        assert_eq!(cursors.len(), 1);
        context.cancellation().cancel();

        let cancelled = ProductiveQueryEngine::stream_page(
            page_request(1, continuation)?,
            page_execution(&mut cursors, query_hash, 2_000)?,
            &context,
            &fixture.policies,
            |_| Ok([Ok(2_u8)].into_iter()),
            |value| vec![*value],
            |_, _, _| true,
        );
        assert!(matches!(cancelled, Err(QueryEngineError::Cancelled)));
        assert_eq!(cursors.len(), 0);
        Ok(())
    }

    #[test]
    fn raw_history_budget_exhaustion_fails_before_returning_a_partial_result() -> TestResult {
        let fixture = fixture()?;
        let context = context_with_result_budget(&fixture.context, 3)?;
        let result = ProductiveQueryEngine::raw_history(
            &fixture.history,
            &context,
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
        );
        assert!(matches!(
            result,
            Err(QueryEngineError::BudgetExceeded(BudgetDimension::Results))
        ));
        Ok(())
    }

    #[test]
    fn result_budget_exhaustion_returns_no_partial_page() -> TestResult {
        let fixture = fixture()?;
        let context = context_with_result_budget(&fixture.context, 1)?;
        let mut cursors = cursor_store()?;
        let result = ProductiveQueryEngine::stream_page(
            page_request(1, None)?,
            page_execution(&mut cursors, crate::QueryHash::new([55; 32]), 1_000)?,
            &context,
            &fixture.policies,
            |_| Ok([Ok(1_u8), Ok(2)].into_iter()),
            |value| vec![*value],
            |_, _, _| true,
        );
        assert!(matches!(
            result,
            Err(QueryEngineError::BudgetExceeded(BudgetDimension::Results))
        ));
        assert_eq!(cursors.len(), 0);
        Ok(())
    }

    #[test]
    fn changed_security_epoch_invalidates_and_releases_a_page_cursor() -> TestResult {
        let fixture = fixture()?;
        let context = leased_page_context(&fixture.context, &fixture.layers)?;
        let query_hash = crate::QueryHash::new([53; 32]);
        let mut cursors = cursor_store()?;
        let first = ProductiveQueryEngine::stream_page(
            page_request(1, None)?,
            page_execution(&mut cursors, query_hash, 1_000)?,
            &context,
            &fixture.policies,
            |_| Ok([Ok(1_u8), Ok(2)].into_iter()),
            |value| vec![*value],
            |_, _, _| true,
        )?;
        let continuation = first.next_cursor().map(<[u8]>::to_vec);
        assert_eq!(cursors.len(), 1);

        let failed = ProductiveQueryEngine::stream_page(
            page_request(1, continuation)?,
            page_execution(&mut cursors, query_hash, 2_000)?,
            &context,
            &fixture.hidden_policies,
            |_| Ok([Ok(2_u8)].into_iter()),
            |value| vec![*value],
            |_, _, _| true,
        );
        assert!(matches!(failed, Err(QueryEngineError::CursorInvalidated)));
        assert_eq!(cursors.len(), 0);
        Ok(())
    }
}
