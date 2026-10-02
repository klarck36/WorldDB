//! Bounded aggregation over owned, resolved, caller-visible result rows.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ids::{HistorySpaceId, LayerId};
use crate::query_context::QueryContext;
use crate::query_ports::OwnedQueryResult;
use crate::record_refs::RecordRef;
use crate::schema::ValueKind;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicyHistory,
    SecurityPolicyHistoryError,
};

/// Opaque canonical typed key returned by the schema-aware field normalizer.
///
/// `canonical_value` must use the exact comparator encoding for the pinned schema;
/// time values must already be normalized to their timeline.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GroupValueKey {
    value_kind: ValueKind,
    canonical_value: Vec<u8>,
}

impl GroupValueKey {
    /// Binds one schema-normalized value to its closed ValueKind tag.
    #[must_use]
    pub const fn new(value_kind: ValueKind, canonical_value: Vec<u8>) -> Self {
        Self {
            value_kind,
            canonical_value,
        }
    }
}

/// One already resolved and caller-visible result, carrying only permitted group fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAggregateRow {
    result_key: RecordRef,
    history_space: HistorySpaceId,
    layer: LayerId,
    group_values: Vec<(FieldSelector, Option<GroupValueKey>)>,
}

impl ResolvedAggregateRow {
    /// Creates a row; duplicate group selectors are rejected and fields are sorted.
    pub fn new(
        result_key: RecordRef,
        history_space: HistorySpaceId,
        layer: LayerId,
        mut group_values: Vec<(FieldSelector, Option<GroupValueKey>)>,
    ) -> Result<Self, AggregateError> {
        group_values.sort_by_key(|(field, _)| *field);
        if group_values.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|((left, _), (right, _))| left == right)
        }) {
            return Err(AggregateError::DuplicateGroupField);
        }
        Ok(Self {
            result_key,
            history_space,
            layer,
            group_values,
        })
    }
}

/// Closed aggregate operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AggregateSpec {
    /// Number of visible resolved result rows.
    Count,
    /// Whether at least one visible resolved result row exists.
    Exists,
    /// Counts by one or more typed, visible fields; absent values form Missing groups.
    GroupedCount { fields: Vec<FieldSelector> },
}

impl AggregateSpec {
    /// Creates a non-empty, unique, canonically ordered group selector.
    pub fn grouped_count(mut fields: Vec<FieldSelector>) -> Result<Self, AggregateError> {
        if fields.is_empty() {
            return Err(AggregateError::EmptyGroupFields);
        }
        fields.sort_unstable();
        if fields.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(left, right)| left == right)
        }) {
            return Err(AggregateError::DuplicateGroupField);
        }
        Ok(Self::GroupedCount { fields })
    }
}

/// Missing is represented distinctly from every typed group value.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AggregateGroupValue {
    /// The resolved result has no value for the selected field.
    Missing,
    /// A present, schema-normalized typed value.
    Value(GroupValueKey),
}

/// One deterministic grouped-count row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupedCountRow {
    values: Vec<(FieldSelector, AggregateGroupValue)>,
    count: u64,
}

impl GroupedCountRow {
    /// Typed key components in canonical selector order.
    #[must_use]
    pub fn values(&self) -> &[(FieldSelector, AggregateGroupValue)] {
        &self.values
    }

    /// Number of visible resolved rows in this group.
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }
}

/// Complete aggregate value. No partial value is returned on budget or cancellation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AggregateResult {
    /// Count of visible resolved results.
    Count(u64),
    /// Existence among visible resolved results.
    Exists(bool),
    /// Canonically ordered grouped counts.
    GroupedCount(Vec<GroupedCountRow>),
}

/// Aggregates only owned resolved results. Input binding and policy epochs must match
/// the current QueryContext; record and group-field authorization are rechecked before
/// a row contributes to a count or group.
pub fn aggregate_visible_resolved(
    resolved: &OwnedQueryResult<Vec<ResolvedAggregateRow>>,
    spec: &AggregateSpec,
    context: &QueryContext,
    policies: &SecurityPolicyHistory,
) -> Result<OwnedQueryResult<AggregateResult>, AggregateError> {
    if context.cancellation().is_cancelled() {
        return Err(AggregateError::Cancelled);
    }
    if resolved.binding() != context.schema_binding()
        || !resolved.query_context_binding().matches(context)
    {
        return Err(AggregateError::QueryBindingMismatch);
    }
    let security = policies.resolve(context)?;
    if resolved.current_security_epoch() != security.current_epoch()
        || resolved.evaluated_security_epoch() != security.evaluated_epoch()
    {
        return Err(AggregateError::SecurityBindingMismatch);
    }
    let policy = security.snapshot();
    let principal = context.security().principal_id();
    if policy.authorize(
        principal,
        Capability::QueryAggregate,
        PolicyTarget::default(),
    ) != AuthorizationDecision::Allow
    {
        return Err(AggregateError::Unauthorized);
    }

    let requested_fields = match spec {
        AggregateSpec::GroupedCount { fields } => fields.as_slice(),
        AggregateSpec::Count | AggregateSpec::Exists => &[],
    };
    let budget = context.budget();
    let mut candidates = 0_u64;
    let mut work = 0_u64;
    let mut count = 0_u64;
    let mut visible_keys = BTreeSet::new();
    let mut group_kinds = BTreeMap::<FieldSelector, ValueKind>::new();
    let mut groups = BTreeMap::<Vec<(FieldSelector, AggregateGroupValue)>, u64>::new();

    for row in resolved.value() {
        if context.cancellation().is_cancelled() {
            return Err(AggregateError::Cancelled);
        }
        if !record_is_visible(policy, principal, row) {
            continue;
        }
        if !visible_keys.insert(row.result_key) {
            return Err(AggregateError::DuplicateResultKey);
        }
        if requested_fields.iter().any(|field| {
            let target = PolicyTarget::new(
                Some(row.history_space),
                Some(row.layer),
                Some(row.result_key),
                Some(*field),
                None,
            );
            policy.authorize(principal, Capability::FieldRead, target)
                != AuthorizationDecision::Allow
        }) {
            continue;
        }
        candidates = candidates
            .checked_add(1)
            .ok_or(AggregateError::BudgetExceeded)?;
        let row_work = u64::try_from(requested_fields.len())
            .ok()
            .and_then(|field_count| field_count.checked_add(1))
            .ok_or(AggregateError::BudgetExceeded)?;
        work = work
            .checked_add(row_work)
            .ok_or(AggregateError::BudgetExceeded)?;
        if candidates > budget.max_candidates().get() || work > budget.max_work_units().get() {
            return Err(AggregateError::BudgetExceeded);
        }
        count = count.checked_add(1).ok_or(AggregateError::CountOverflow)?;

        if let AggregateSpec::GroupedCount { fields } = spec {
            let key = fields
                .iter()
                .map(|field| {
                    let value = row
                        .group_values
                        .binary_search_by_key(field, |(selector, _)| *selector)
                        .ok()
                        .and_then(|index| row.group_values.get(index))
                        .and_then(|(_, value)| value.clone())
                        .map_or(AggregateGroupValue::Missing, AggregateGroupValue::Value);
                    if let AggregateGroupValue::Value(value) = &value {
                        if group_kinds
                            .insert(*field, value.value_kind)
                            .is_some_and(|previous| previous != value.value_kind)
                        {
                            return Err(AggregateError::IncomparableGroupValues);
                        }
                    }
                    Ok((*field, value))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let group_count = groups.entry(key).or_default();
            *group_count = group_count
                .checked_add(1)
                .ok_or(AggregateError::CountOverflow)?;
        }
    }

    let result = match spec {
        AggregateSpec::Count => AggregateResult::Count(count),
        AggregateSpec::Exists => AggregateResult::Exists(count > 0),
        AggregateSpec::GroupedCount { .. } => {
            if groups.len() as u64 > budget.max_results().get() {
                return Err(AggregateError::BudgetExceeded);
            }
            AggregateResult::GroupedCount(
                groups
                    .into_iter()
                    .map(|(values, count)| GroupedCountRow { values, count })
                    .collect(),
            )
        }
    };
    if context.cancellation().is_cancelled() {
        return Err(AggregateError::Cancelled);
    }
    let owned =
        OwnedQueryResult::bind(context, policies, result).map_err(AggregateError::Security)?;
    if context.cancellation().is_cancelled() {
        return Err(AggregateError::Cancelled);
    }
    Ok(owned)
}

fn record_is_visible(
    policy: &crate::security::SecurityPolicySnapshot,
    principal: crate::ids::PrincipalId,
    row: &ResolvedAggregateRow,
) -> bool {
    let Some(capability) = record_read_capability(row.result_key) else {
        return false;
    };
    policy.authorize(
        principal,
        capability,
        PolicyTarget::new(
            Some(row.history_space),
            Some(row.layer),
            Some(row.result_key),
            None,
            None,
        ),
    ) == AuthorizationDecision::Allow
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

/// Invalid aggregate request, binding, authorization, or terminal query condition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AggregateError {
    /// Grouping requires at least one field.
    EmptyGroupFields,
    /// One field was selected more than once or occurs twice in a row.
    DuplicateGroupField,
    /// Multiple visible resolved rows have the same caller-visible result identity.
    DuplicateResultKey,
    /// One grouped field contains values with different schema ValueKinds.
    IncomparableGroupValues,
    /// The owned resolved rows do not match the query's snapshot/schema binding.
    QueryBindingMismatch,
    /// Current or evaluated security epoch differs from the resolved result binding.
    SecurityBindingMismatch,
    /// The aggregate operation is not authorized.
    Unauthorized,
    /// A visible result group count exceeded unsigned range.
    CountOverflow,
    /// Candidate, work, or grouped-result budget was exhausted; no partial output is returned.
    BudgetExceeded,
    /// The host cancelled the query; no partial output is returned.
    Cancelled,
    /// Policy selection could not be resolved.
    Security(SecurityPolicyHistoryError),
}

impl fmt::Display for AggregateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyGroupFields => "grouped count requires at least one field",
            Self::DuplicateGroupField => "aggregate request contains a duplicate group field",
            Self::DuplicateResultKey => "aggregate contains duplicate visible result keys",
            Self::IncomparableGroupValues => "aggregate group values are incomparable",
            Self::QueryBindingMismatch => "resolved rows belong to another query binding",
            Self::SecurityBindingMismatch => "resolved rows belong to another security epoch",
            Self::Unauthorized => "aggregation is not authorized",
            Self::CountOverflow => "aggregate count exceeds the supported range",
            Self::BudgetExceeded => "aggregate budget exceeded",
            Self::Cancelled => "aggregation was cancelled",
            Self::Security(_) => "aggregate security context is invalid",
        })
    }
}

impl std::error::Error for AggregateError {}

impl From<SecurityPolicyHistoryError> for AggregateError {
    fn from(value: SecurityPolicyHistoryError) -> Self {
        Self::Security(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AggregateError, AggregateGroupValue, AggregateResult, AggregateSpec, GroupValueKey,
        ResolvedAggregateRow, aggregate_visible_resolved,
    };
    use crate::non_interference::{
        CursorObservation, PairedWorld, PublicFailure, PublicObservation,
    };
    use crate::query_engine::{ProductiveQueryEngine, QueryEngineError, QueryExecutionPath};
    use crate::query_ports::OwnedQueryResult;
    use crate::query_search::tests::{fixture_with_candidate_limit, id};
    use crate::record_refs::RecordRef;
    use crate::schema::ValueKind;

    fn rows(
        fixture: &crate::query_search::tests::Fixture,
        include_hidden: bool,
    ) -> Result<Vec<ResolvedAggregateRow>, AggregateError> {
        let visible = ResolvedAggregateRow::new(
            RecordRef::Assertion(id::<crate::ids::AssertionId>(69)),
            fixture.history_space,
            fixture.layer,
            vec![(
                fixture.selector,
                Some(GroupValueKey::new(ValueKind::String, b"north".to_vec())),
            )],
        )?;
        let missing = ResolvedAggregateRow::new(
            RecordRef::Assertion(id::<crate::ids::AssertionId>(70)),
            fixture.history_space,
            fixture.layer,
            vec![(fixture.selector, None)],
        )?;
        let mut rows = vec![visible, missing];
        if include_hidden {
            rows.push(ResolvedAggregateRow::new(
                RecordRef::Assertion(id::<crate::ids::AssertionId>(71)),
                fixture.history_space,
                fixture.layer,
                vec![(
                    fixture.selector,
                    Some(GroupValueKey::new(ValueKind::String, b"north".to_vec())),
                )],
            )?);
        }
        Ok(rows)
    }

    fn bind_rows(
        fixture: &crate::query_search::tests::Fixture,
        include_hidden: bool,
    ) -> Result<OwnedQueryResult<Vec<ResolvedAggregateRow>>, AggregateError> {
        OwnedQueryResult::bind(
            &fixture.context,
            &fixture.policies,
            rows(fixture, include_hidden)?,
        )
        .map_err(AggregateError::Security)
    }

    #[test]
    fn count_exists_and_grouped_count_consume_only_visible_resolved_rows()
    -> Result<(), AggregateError> {
        let hidden = RecordRef::Assertion(id::<crate::ids::AssertionId>(71));
        let fixture = fixture_with_candidate_limit(Some(hidden), 10, 10);
        let resolved = bind_rows(&fixture, true)?;
        let visible_only = bind_rows(&fixture, false)?;

        let count = aggregate_visible_resolved(
            &resolved,
            &AggregateSpec::Count,
            &fixture.context,
            &fixture.policies,
        )?;
        let paired = PairedWorld::new((&resolved, &visible_only), false, true);
        paired
            .compare(|(all_rows, visible_rows), include_hidden| {
                let input = if *include_hidden {
                    *all_rows
                } else {
                    *visible_rows
                };
                match aggregate_visible_resolved(
                    input,
                    &AggregateSpec::Count,
                    &fixture.context,
                    &fixture.policies,
                ) {
                    Ok(result) => PublicObservation::success(
                        result.value().clone(),
                        vec!["count".to_owned()],
                        CursorObservation::Absent,
                    ),
                    Err(error) => PublicObservation::failure(
                        PublicFailure::new(error.to_string(), vec!["code".to_owned()]),
                        vec!["error".to_owned()],
                        CursorObservation::Absent,
                    ),
                }
            })
            .map_err(|_| AggregateError::QueryBindingMismatch)?;
        assert_eq!(count.value(), &AggregateResult::Count(2));
        let exists = aggregate_visible_resolved(
            &resolved,
            &AggregateSpec::Exists,
            &fixture.context,
            &fixture.policies,
        )?;
        let visible_exists = aggregate_visible_resolved(
            &visible_only,
            &AggregateSpec::Exists,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(exists.value(), visible_exists.value());
        assert_eq!(exists.value(), &AggregateResult::Exists(true));

        let grouped = aggregate_visible_resolved(
            &resolved,
            &AggregateSpec::grouped_count(vec![fixture.selector])?,
            &fixture.context,
            &fixture.policies,
        )?;
        let visible_grouped = aggregate_visible_resolved(
            &visible_only,
            &AggregateSpec::grouped_count(vec![fixture.selector])?,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(grouped.value(), visible_grouped.value());
        let AggregateResult::GroupedCount(groups) = grouped.value() else {
            return Err(AggregateError::QueryBindingMismatch);
        };
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|group| {
            group.count() == 1
                && group.values() == [(fixture.selector, AggregateGroupValue::Missing)]
        }));
        assert!(groups.iter().any(|group| {
            group.count() == 1
                && group.values()
                    == [(
                        fixture.selector,
                        AggregateGroupValue::Value(GroupValueKey::new(
                            ValueKind::String,
                            b"north".to_vec(),
                        )),
                    )]
        }));
        Ok(())
    }

    #[test]
    fn grouped_count_budget_exhaustion_returns_no_partial_aggregate() -> Result<(), AggregateError>
    {
        let fixture = fixture_with_candidate_limit(None, 1, 10);
        let resolved = bind_rows(&fixture, true)?;
        assert!(matches!(
            aggregate_visible_resolved(
                &resolved,
                &AggregateSpec::grouped_count(vec![fixture.selector])?,
                &fixture.context,
                &fixture.policies,
            ),
            Err(AggregateError::BudgetExceeded)
        ));
        Ok(())
    }

    #[test]
    fn productive_engine_exposes_count_exists_and_grouped_count() -> Result<(), QueryEngineError> {
        let hidden = RecordRef::Assertion(id::<crate::ids::AssertionId>(71));
        let fixture = fixture_with_candidate_limit(Some(hidden), 10, 10);
        let resolved = bind_rows(&fixture, true).map_err(QueryEngineError::Aggregate)?;

        let count = ProductiveQueryEngine::aggregate(
            &resolved,
            &AggregateSpec::Count,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(count.path(), QueryExecutionPath::FullScan);
        assert_eq!(count.query().value(), &AggregateResult::Count(2));

        let exists = ProductiveQueryEngine::aggregate(
            &resolved,
            &AggregateSpec::Exists,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(exists.query().value(), &AggregateResult::Exists(true));

        let grouped = ProductiveQueryEngine::aggregate(
            &resolved,
            &AggregateSpec::grouped_count(vec![fixture.selector])
                .map_err(QueryEngineError::Aggregate)?,
            &fixture.context,
            &fixture.policies,
        )?;
        let AggregateResult::GroupedCount(groups) = grouped.query().value() else {
            return Err(QueryEngineError::Aggregate(
                AggregateError::QueryBindingMismatch,
            ));
        };
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.count() == 1));

        let empty = OwnedQueryResult::bind(&fixture.context, &fixture.policies, Vec::new())
            .map_err(AggregateError::Security)
            .map_err(QueryEngineError::Aggregate)?;
        let empty_exists = ProductiveQueryEngine::aggregate(
            &empty,
            &AggregateSpec::Exists,
            &fixture.context,
            &fixture.policies,
        )?;
        assert_eq!(
            empty_exists.query().value(),
            &AggregateResult::Exists(false)
        );
        Ok(())
    }

    #[test]
    fn productive_aggregate_keeps_budget_and_empty_cancellation_terminal()
    -> Result<(), QueryEngineError> {
        let limited = fixture_with_candidate_limit(None, 10, 1);
        let resolved = bind_rows(&limited, false).map_err(QueryEngineError::Aggregate)?;
        assert!(matches!(
            ProductiveQueryEngine::aggregate(
                &resolved,
                &AggregateSpec::Count,
                &limited.context,
                &limited.policies,
            ),
            Err(QueryEngineError::Aggregate(AggregateError::BudgetExceeded))
        ));

        let cancelled = fixture_with_candidate_limit(None, 10, 10);
        let empty = OwnedQueryResult::bind(&cancelled.context, &cancelled.policies, Vec::new())
            .map_err(AggregateError::Security)
            .map_err(QueryEngineError::Aggregate)?;
        cancelled.context.cancellation().cancel();
        assert!(matches!(
            ProductiveQueryEngine::aggregate(
                &empty,
                &AggregateSpec::Count,
                &cancelled.context,
                &cancelled.policies,
            ),
            Err(QueryEngineError::Cancelled)
        ));
        assert!(matches!(
            aggregate_visible_resolved(
                &empty,
                &AggregateSpec::Count,
                &cancelled.context,
                &cancelled.policies,
            ),
            Err(AggregateError::Cancelled)
        ));
        Ok(())
    }
}
