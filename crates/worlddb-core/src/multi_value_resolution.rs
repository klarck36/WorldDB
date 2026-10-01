//! Reference-model resolution for MultiValueOverlay predicate slots.

use std::collections::BTreeSet;
use std::fmt;

use crate::assertions::{Polarity, Subject};
use crate::candidate_scan::AssertionCandidate;
use crate::catalog::HistorySpaceCatalog;
use crate::context::ContextKey;
use crate::context_precedence::{ContextPrecedence, ContextPrecedenceError};
use crate::ids::{
    AssertionId, LayerId, PredicateId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
    ReplacementBoundaryValidityClosureId,
};
use crate::layers::LayerSchemaSnapshot;
use crate::masks::{
    ReplacementBoundary, ReplacementBoundaryRetraction, ReplacementBoundaryValidityClosure,
};
use crate::schema::{PredicateDefinition, ResolutionPolicy, ValueKind};
use crate::temporal::{RecordedAsOf, TemporalError, WorldTime};
use crate::values::{Value, canonical_value_equality};

/// One logical subject/Predicate slot selected for multi-value resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MultiValueSlot {
    subject: Subject,
    predicate_id: PredicateId,
}

impl MultiValueSlot {
    /// Selects one stable subject and Predicate identity.
    #[must_use]
    pub const fn new(subject: Subject, predicate_id: PredicateId) -> Self {
        Self {
            subject,
            predicate_id,
        }
    }

    /// Returns the selected subject.
    #[must_use]
    pub const fn subject(self) -> Subject {
        self.subject
    }

    /// Returns the selected Predicate identity.
    #[must_use]
    pub const fn predicate_id(self) -> PredicateId {
        self.predicate_id
    }
}

/// Pinned query metadata required to evaluate ReplacementBoundary visibility.
pub struct MultiValueReplaceContext<'a> {
    query_context: ContextKey,
    selected_layer_ids: BTreeSet<LayerId>,
    recorded_as_of: RecordedAsOf,
    world_time: WorldTime,
    history_spaces: &'a HistorySpaceCatalog,
    layers: &'a LayerSchemaSnapshot,
}

/// Boundary records and their archive-visible identity set for one query snapshot.
pub struct ReplacementBoundaryHistory<'a> {
    boundaries: &'a [ReplacementBoundary],
    closures: &'a [ReplacementBoundaryValidityClosure],
    retractions: &'a [ReplacementBoundaryRetraction],
    archive_visible_boundaries: &'a BTreeSet<ReplacementBoundaryId>,
}

impl<'a> ReplacementBoundaryHistory<'a> {
    /// Groups retained boundary history with the IDs visible at the query's RecordedAsOf.
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

impl<'a> MultiValueReplaceContext<'a> {
    /// Binds the query partition, layers, transaction/world-time axes, and schema snapshots.
    #[must_use]
    pub const fn new(
        query_context: ContextKey,
        selected_layer_ids: BTreeSet<LayerId>,
        recorded_as_of: RecordedAsOf,
        world_time: WorldTime,
        history_spaces: &'a HistorySpaceCatalog,
        layers: &'a LayerSchemaSnapshot,
    ) -> Self {
        Self {
            query_context,
            selected_layer_ids,
            recorded_as_of,
            world_time,
            history_spaces,
            layers,
        }
    }
}

fn validate_boundary_lifecycle(
    boundaries: &[ReplacementBoundary],
    closures: &[ReplacementBoundaryValidityClosure],
    retractions: &[ReplacementBoundaryRetraction],
) -> Result<(), MultiValueResolutionError> {
    let mut boundary_ids = BTreeSet::new();
    for boundary in boundaries {
        if !boundary_ids.insert(boundary.id()) {
            return Err(MultiValueResolutionError::DuplicateBoundary {
                boundary_id: boundary.id(),
            });
        }
    }
    let mut closure_ids = BTreeSet::<ReplacementBoundaryValidityClosureId>::new();
    for closure in closures {
        if !closure_ids.insert(closure.id()) {
            return Err(MultiValueResolutionError::DuplicateBoundaryClosure {
                closure_id: closure.id(),
            });
        }
        let boundary = boundaries
            .iter()
            .find(|boundary| boundary.id() == closure.replacement_boundary_id())
            .ok_or(MultiValueResolutionError::MissingBoundary {
                boundary_id: closure.replacement_boundary_id(),
            })?;
        if closure.created_revision() <= boundary.created_revision() {
            return Err(MultiValueResolutionError::BoundaryLifecycleRevisionOrder {
                boundary_id: boundary.id(),
            });
        }
        if boundary.validity().is_some_and(|validity| {
            validity.interval().timeline() != closure.close_at_world_time().timeline()
        }) {
            return Err(MultiValueResolutionError::BoundaryClosureTimelineMismatch {
                boundary_id: boundary.id(),
            });
        }
        if boundary.validity().is_some_and(|validity| {
            validity.interval().start().is_some_and(|start| {
                closure.close_at_world_time().nanoseconds() < start.nanoseconds()
            }) || validity
                .interval()
                .end()
                .is_some_and(|end| closure.close_at_world_time().nanoseconds() > end.nanoseconds())
        }) {
            return Err(MultiValueResolutionError::BoundaryClosureOutsideValidity {
                boundary_id: boundary.id(),
                closure_id: closure.id(),
            });
        }
    }
    let mut retraction_ids = BTreeSet::<ReplacementBoundaryRetractionId>::new();
    for retraction in retractions {
        if !retraction_ids.insert(retraction.id()) {
            return Err(MultiValueResolutionError::DuplicateBoundaryRetraction {
                retraction_id: retraction.id(),
            });
        }
        let boundary = boundaries
            .iter()
            .find(|boundary| boundary.id() == retraction.replacement_boundary_id())
            .ok_or(MultiValueResolutionError::MissingBoundary {
                boundary_id: retraction.replacement_boundary_id(),
            })?;
        if retraction.created_revision() <= boundary.created_revision() {
            return Err(MultiValueResolutionError::BoundaryLifecycleRevisionOrder {
                boundary_id: boundary.id(),
            });
        }
    }
    Ok(())
}

/// A distinct value and polarity retained by an overlay, with its contributors.
#[derive(Clone, Debug)]
pub struct MultiValueEntry {
    value: Value,
    polarity: Polarity,
    contributors: Vec<AssertionId>,
}

impl MultiValueEntry {
    /// Returns the canonical representative of this value.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Returns the explicit polarity; a negative assertion is not a retraction.
    #[must_use]
    pub const fn polarity(&self) -> Polarity {
        self.polarity
    }

    /// Returns contributor IDs in stable order.
    #[must_use]
    pub fn contributors(&self) -> &[AssertionId] {
        &self.contributors
    }
}

/// A value asserted both positively and negatively in the visible overlay.
#[derive(Clone, Debug)]
pub struct MultiValueConflict {
    value: Value,
    positive_contributors: Vec<AssertionId>,
    negative_contributors: Vec<AssertionId>,
}

impl MultiValueConflict {
    /// Returns the canonical representative of the contradictory value.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Returns positive contributors in stable order.
    #[must_use]
    pub fn positive_contributors(&self) -> &[AssertionId] {
        &self.positive_contributors
    }

    /// Returns negative contributors in stable order.
    #[must_use]
    pub fn negative_contributors(&self) -> &[AssertionId] {
        &self.negative_contributors
    }
}

/// One resolved MultiValueOverlay slot.
#[derive(Clone, Debug)]
pub enum MultiValueOutcome {
    /// At least one value/polarity proposition is known and no contradiction exists.
    Known { values: Vec<MultiValueEntry> },
    /// No candidate remains after historical, security, archive, and mask filtering.
    Unknown,
    /// One or more values have both positive and negative visible assertions.
    /// Non-conflicting values remain available alongside the contradictions.
    Conflict {
        values: Vec<MultiValueEntry>,
        conflicts: Vec<MultiValueConflict>,
    },
}

#[derive(Default)]
struct ValueGroup {
    value: Option<Value>,
    positive: BTreeSet<AssertionId>,
    negative: BTreeSet<AssertionId>,
}

/// Resolves one MultiValueOverlay slot from visible, post-mask candidates.
///
/// Overlay retains values from every visible ContextPrecedence. It neither
/// replaces a parent's values with a child set nor suppresses equal-precedence
/// peers. Equal value/polarity propositions coalesce and retain all contributors;
/// opposite polarities for one value are reported as Conflict. Context filtering,
/// security, archive visibility, lifecycle, and Masking must already have been
/// applied by the caller. Time equality is supplied by the pinned schema.
pub fn resolve_multi_value_overlay<F>(
    candidates: &[AssertionCandidate],
    slot: MultiValueSlot,
    predicate: &PredicateDefinition,
    temporal_value_equal: F,
) -> Result<MultiValueOutcome, MultiValueResolutionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    resolve_multi_value_set(
        candidates,
        slot,
        predicate,
        ResolutionPolicy::MultiValueOverlay,
        temporal_value_equal,
    )
}

/// Resolves one MultiValueReplace slot from active boundaries and visible candidates.
///
/// The highest-precedence active boundary cuts off strictly lower candidates;
/// equal-precedence candidates remain peers. If a boundary is active but no
/// candidate survives above its cutoff, the complete set is `Known { values: [] }`.
/// With no active boundary and no candidates the result remains `Unknown`.
pub fn resolve_multi_value_replace<F>(
    candidates: &[AssertionCandidate],
    boundary_history: ReplacementBoundaryHistory<'_>,
    slot: MultiValueSlot,
    predicate: &PredicateDefinition,
    context: MultiValueReplaceContext<'_>,
    temporal_value_equal: F,
) -> Result<MultiValueOutcome, MultiValueResolutionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    if predicate.predicate_id() != slot.predicate_id {
        return Err(MultiValueResolutionError::SchemaPredicateMismatch {
            expected: slot.predicate_id,
            actual: predicate.predicate_id(),
        });
    }
    if predicate.resolution_policy() != ResolutionPolicy::MultiValueReplace {
        return Err(MultiValueResolutionError::UnsupportedPolicy {
            actual: predicate.resolution_policy(),
        });
    }
    if context.selected_layer_ids.is_empty() {
        return Err(MultiValueResolutionError::EmptyLayerSelection);
    }
    validate_boundary_lifecycle(
        boundary_history.boundaries,
        boundary_history.closures,
        boundary_history.retractions,
    )?;

    let mut active_boundaries = Vec::new();
    for boundary in boundary_history.boundaries {
        if boundary.subject() != slot.subject || boundary.predicate_id() != slot.predicate_id {
            continue;
        }
        if boundary.created_revision() < predicate.created_revision() {
            return Err(MultiValueResolutionError::BoundaryBeforePredicate {
                boundary_id: boundary.id(),
            });
        }
        if boundary.created_revision() > context.recorded_as_of.revision()
            || !boundary_history
                .archive_visible_boundaries
                .contains(&boundary.id())
            || !context
                .selected_layer_ids
                .contains(&boundary.context().layer_id())
            || boundary.context().perspective_scope() != context.query_context.perspective_scope()
            || boundary.context().epistemic_mode() != context.query_context.epistemic_mode()
        {
            continue;
        }
        if boundary_history.retractions.iter().any(|item| {
            item.replacement_boundary_id() == boundary.id()
                && item.created_revision() <= context.recorded_as_of.revision()
        }) {
            continue;
        }
        if let Some(validity) = boundary.validity() {
            if !validity.contains(context.world_time)? {
                continue;
            }
        }
        let mut closed = false;
        for closure in boundary_history.closures.iter().filter(|item| {
            item.replacement_boundary_id() == boundary.id()
                && item.created_revision() <= context.recorded_as_of.revision()
        }) {
            if closure
                .close_at_world_time()
                .checked_cmp(context.world_time)?
                .is_le()
            {
                closed = true;
                break;
            }
        }
        if closed {
            continue;
        }
        let precedence = match ContextPrecedence::for_context(
            context.query_context.history_space_id(),
            boundary.context().history_space_id(),
            boundary.context().layer_id(),
            context.history_spaces,
            context.layers,
        ) {
            Ok(value) => value,
            Err(ContextPrecedenceError::RecordOutsideQueryAncestry { .. }) => continue,
            Err(error) => return Err(error.into()),
        };
        active_boundaries.push((boundary, precedence));
    }
    let cutoff = active_boundaries
        .iter()
        .map(|(_, precedence)| *precedence)
        .max();
    let mut surviving = Vec::new();
    for candidate in candidates.iter().filter(|candidate| {
        candidate.assertion().subject() == slot.subject
            && candidate.assertion().predicate_id() == slot.predicate_id
    }) {
        let assertion_context = candidate.assertion().context();
        if candidate.query_history_space_id != context.query_context.history_space_id()
            || assertion_context.perspective_scope() != context.query_context.perspective_scope()
            || assertion_context.epistemic_mode() != context.query_context.epistemic_mode()
            || candidate.selected_layer_ids() != &context.selected_layer_ids
        {
            return Err(MultiValueResolutionError::CandidateScopeMismatch {
                assertion_id: candidate.assertion().id(),
            });
        }
        if cutoff.is_none_or(|cutoff| candidate.precedence() >= cutoff) {
            surviving.push(candidate.clone());
        }
    }
    let outcome = resolve_multi_value_set(
        &surviving,
        slot,
        predicate,
        ResolutionPolicy::MultiValueReplace,
        temporal_value_equal,
    )?;
    if cutoff.is_some() && matches!(outcome, MultiValueOutcome::Unknown) {
        Ok(MultiValueOutcome::Known { values: Vec::new() })
    } else {
        Ok(outcome)
    }
}

fn resolve_multi_value_set<F>(
    candidates: &[AssertionCandidate],
    slot: MultiValueSlot,
    predicate: &PredicateDefinition,
    expected_policy: ResolutionPolicy,
    mut temporal_value_equal: F,
) -> Result<MultiValueOutcome, MultiValueResolutionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    if predicate.predicate_id() != slot.predicate_id {
        return Err(MultiValueResolutionError::SchemaPredicateMismatch {
            expected: slot.predicate_id,
            actual: predicate.predicate_id(),
        });
    }
    if predicate.resolution_policy() != expected_policy {
        return Err(MultiValueResolutionError::UnsupportedPolicy {
            actual: predicate.resolution_policy(),
        });
    }
    let mut matching = candidates
        .iter()
        .filter(|candidate| {
            candidate.assertion().subject() == slot.subject
                && candidate.assertion().predicate_id() == slot.predicate_id
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return Ok(MultiValueOutcome::Unknown);
    }

    let first = matching
        .first()
        .copied()
        .ok_or(MultiValueResolutionError::EmptyValueSet)?;
    let context = first.assertion().context();
    let query = first.query_history_space_id;
    let selected_layers = first.selected_layer_ids();
    let mut ids = BTreeSet::new();
    for candidate in &matching {
        let assertion = candidate.assertion();
        if !ids.insert(assertion.id()) {
            return Err(MultiValueResolutionError::DuplicateCandidate {
                assertion_id: assertion.id(),
            });
        }
        let candidate_context = assertion.context();
        if candidate.query_history_space_id != query
            || candidate_context.perspective_scope() != context.perspective_scope()
            || candidate_context.epistemic_mode() != context.epistemic_mode()
            || candidate.selected_layer_ids() != selected_layers
        {
            return Err(MultiValueResolutionError::CandidateScopeMismatch {
                assertion_id: assertion.id(),
            });
        }
    }
    matching.sort_by_key(|candidate| candidate.assertion().id());

    let mut groups: Vec<ValueGroup> = Vec::new();
    for candidate in matching {
        let assertion = candidate.assertion();
        let (_, _, value, polarity) = assertion.proposition_components();
        let actual = ValueKind::of(value);
        if actual != predicate.value_kind() {
            return Err(MultiValueResolutionError::SchemaValueKindMismatch {
                assertion_id: assertion.id(),
                expected: predicate.value_kind(),
                actual,
            });
        }
        let mut group_index = None;
        for (index, group) in groups.iter().enumerate() {
            let Some(representative) = group.value.as_ref() else {
                continue;
            };
            let equal = match canonical_value_equality(representative, value) {
                Some(equal) => equal,
                None => temporal_value_equal(representative, value)
                    .map_err(|()| MultiValueResolutionError::TemporalValueComparisonUnavailable)?,
            };
            if equal {
                group_index = Some(index);
                break;
            }
        }
        let group = if let Some(index) = group_index {
            groups
                .get_mut(index)
                .ok_or(MultiValueResolutionError::EmptyValueSet)?
        } else {
            groups.push(ValueGroup {
                value: Some(value.clone()),
                ..ValueGroup::default()
            });
            groups
                .last_mut()
                .ok_or(MultiValueResolutionError::EmptyValueSet)?
        };
        match polarity {
            Polarity::Positive => {
                group.positive.insert(assertion.id());
            }
            Polarity::Negative => {
                group.negative.insert(assertion.id());
            }
        }
    }

    let mut values = Vec::new();
    let mut conflicts = Vec::new();
    for group in groups {
        let value = group
            .value
            .ok_or(MultiValueResolutionError::EmptyValueSet)?;
        if !group.positive.is_empty() && !group.negative.is_empty() {
            conflicts.push(MultiValueConflict {
                value,
                positive_contributors: group.positive.into_iter().collect(),
                negative_contributors: group.negative.into_iter().collect(),
            });
        } else if !group.positive.is_empty() {
            values.push(MultiValueEntry {
                value,
                polarity: Polarity::Positive,
                contributors: group.positive.into_iter().collect(),
            });
        } else if !group.negative.is_empty() {
            values.push(MultiValueEntry {
                value,
                polarity: Polarity::Negative,
                contributors: group.negative.into_iter().collect(),
            });
        }
    }
    if !conflicts.is_empty() {
        Ok(MultiValueOutcome::Conflict { values, conflicts })
    } else if values.is_empty() {
        Ok(MultiValueOutcome::Unknown)
    } else {
        Ok(MultiValueOutcome::Known { values })
    }
}

/// Invalid policy binding or unavailable schema-aware proposition comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MultiValueResolutionError {
    /// The selected schema definition has a different Predicate identity.
    SchemaPredicateMismatch {
        expected: PredicateId,
        actual: PredicateId,
    },
    /// The selected schema does not use `MultiValueOverlay`.
    UnsupportedPolicy { actual: ResolutionPolicy },
    /// A ReplacementBoundary layer selection must contain at least one layer.
    EmptyLayerSelection,
    /// One persistent ReplacementBoundary identity appears more than once.
    DuplicateBoundary { boundary_id: ReplacementBoundaryId },
    /// One boundary validity-closure identity appears more than once.
    DuplicateBoundaryClosure {
        closure_id: ReplacementBoundaryValidityClosureId,
    },
    /// One boundary-retraction identity appears more than once.
    DuplicateBoundaryRetraction {
        retraction_id: ReplacementBoundaryRetractionId,
    },
    /// A boundary lifecycle record refers to no supplied boundary.
    MissingBoundary { boundary_id: ReplacementBoundaryId },
    /// A boundary lifecycle record must be created after its target.
    BoundaryLifecycleRevisionOrder { boundary_id: ReplacementBoundaryId },
    /// A closure must use the validity timeline of its boundary when one is explicit.
    BoundaryClosureTimelineMismatch { boundary_id: ReplacementBoundaryId },
    /// A closure must fall within the boundary's explicit validity interval.
    BoundaryClosureOutsideValidity {
        boundary_id: ReplacementBoundaryId,
        closure_id: ReplacementBoundaryValidityClosureId,
    },
    /// A ReplacementBoundary cannot predate its selected Predicate definition.
    BoundaryBeforePredicate { boundary_id: ReplacementBoundaryId },
    /// Query context or layer schema cannot produce a valid precedence coordinate.
    ContextPrecedence(ContextPrecedenceError),
    /// Boundary validity or closure time cannot be compared with query WorldTime.
    Temporal(TemporalError),
    /// The input repeats one persistent Assertion identity.
    DuplicateCandidate { assertion_id: AssertionId },
    /// Candidates do not share one query, layer selection, or epistemic partition.
    CandidateScopeMismatch { assertion_id: AssertionId },
    /// An assertion value does not match the selected historical Predicate schema.
    SchemaValueKindMismatch {
        assertion_id: AssertionId,
        expected: ValueKind,
        actual: ValueKind,
    },
    /// A group unexpectedly had no retained value.
    EmptyValueSet,
    /// Temporal Value equality requires schema resolution which the caller could not provide.
    TemporalValueComparisonUnavailable,
}

impl fmt::Display for MultiValueResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaPredicateMismatch { expected, actual } => write!(
                f,
                "slot Predicate {expected} does not match schema Predicate {actual}"
            ),
            Self::UnsupportedPolicy { actual } => write!(
                f,
                "multi-value overlay requires MultiValueOverlay, got {actual:?}"
            ),
            Self::EmptyLayerSelection => {
                f.write_str("multi-value replacement requires a nonempty layer selection")
            }
            Self::DuplicateBoundary { boundary_id } => write!(
                f,
                "ReplacementBoundary {boundary_id} appears more than once"
            ),
            Self::DuplicateBoundaryClosure { closure_id } => write!(
                f,
                "ReplacementBoundary closure {closure_id} appears more than once"
            ),
            Self::DuplicateBoundaryRetraction { retraction_id } => write!(
                f,
                "ReplacementBoundary retraction {retraction_id} appears more than once"
            ),
            Self::MissingBoundary { boundary_id } => write!(
                f,
                "lifecycle record targets missing ReplacementBoundary {boundary_id}"
            ),
            Self::BoundaryLifecycleRevisionOrder { boundary_id } => write!(
                f,
                "lifecycle record for ReplacementBoundary {boundary_id} does not follow its creation revision"
            ),
            Self::BoundaryClosureTimelineMismatch { boundary_id } => write!(
                f,
                "closure timeline differs from ReplacementBoundary {boundary_id} validity timeline"
            ),
            Self::BoundaryClosureOutsideValidity {
                boundary_id,
                closure_id,
            } => write!(
                f,
                "closure {closure_id} falls outside ReplacementBoundary {boundary_id} validity"
            ),
            Self::BoundaryBeforePredicate { boundary_id } => write!(
                f,
                "ReplacementBoundary {boundary_id} predates its Predicate definition"
            ),
            Self::ContextPrecedence(error) => {
                write!(f, "invalid ReplacementBoundary query context: {error}")
            }
            Self::Temporal(error) => {
                write!(f, "invalid ReplacementBoundary time comparison: {error}")
            }
            Self::DuplicateCandidate { assertion_id } => write!(
                f,
                "candidate Assertion {assertion_id} appears more than once"
            ),
            Self::CandidateScopeMismatch { assertion_id } => write!(
                f,
                "candidate Assertion {assertion_id} belongs to another query scope"
            ),
            Self::SchemaValueKindMismatch {
                assertion_id,
                expected,
                actual,
            } => write!(
                f,
                "assertion {assertion_id} has ValueKind {actual:?}; pinned Predicate schema requires {expected:?}"
            ),
            Self::EmptyValueSet => {
                f.write_str("resolved multi-value set unexpectedly has no retained value")
            }
            Self::TemporalValueComparisonUnavailable => {
                f.write_str("temporal proposition equality requires the pinned schema")
            }
        }
    }
}

impl std::error::Error for MultiValueResolutionError {}

impl From<ContextPrecedenceError> for MultiValueResolutionError {
    fn from(error: ContextPrecedenceError) -> Self {
        Self::ContextPrecedence(error)
    }
}

impl From<TemporalError> for MultiValueResolutionError {
    fn from(error: TemporalError) -> Self {
        Self::Temporal(error)
    }
}
