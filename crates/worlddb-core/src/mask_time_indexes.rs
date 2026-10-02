//! Separate time, Mask selector, ContextPrecedence, and boundary indexes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::assertions::{Assertion, Polarity, Subject};
use crate::catalog::HistorySpaceCatalog;
use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
use crate::context_precedence::{ContextPrecedence, ContextPrecedenceError};
use crate::ids::{
    AssertionId, HistorySpaceId, LayerId, MaskId, PredicateId, ReplacementBoundaryId, TimelineId,
};
use crate::layers::LayerSchemaSnapshot;
use crate::masks::{Mask, MaskSelector, ReplacementBoundary};
use crate::multi_value_resolution::MultiValueSlot;
use crate::temporal::{AssertionValidity, WorldTime};
use crate::values::{Value, canonical_value_equality};

/// Typed owner of an optional or required world-time validity interval.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ValidityTarget {
    /// An immutable assertion interval.
    Assertion(AssertionId),
    /// An optional Mask interval; absence means unbounded validity.
    Mask(MaskId),
    /// An optional ReplacementBoundary interval; absence means unbounded validity.
    ReplacementBoundary(ReplacementBoundaryId),
}

#[derive(Clone, Debug)]
struct ValidityEntry {
    validity: Option<AssertionValidity>,
}

/// A per-Timeline interval index for Assertions, Masks, and ReplacementBoundaries.
///
/// Lookup is a half-open interval-stabbing query: the start is inclusive, the
/// end exclusive, and open ends remain unbounded. Results include optional
/// validities with no interval. Timelines are separate index partitions.
#[derive(Clone, Debug, Default)]
pub struct AssertionValidityIndex {
    entries: BTreeMap<ValidityTarget, ValidityEntry>,
    unbounded: BTreeSet<ValidityTarget>,
    by_timeline_start: BTreeMap<TimelineId, BTreeMap<Option<i128>, Vec<ValidityTarget>>>,
}

impl AssertionValidityIndex {
    /// Builds validity postings from immutable domain records.
    pub fn build(
        assertions: &[Assertion],
        masks: &[Mask],
        boundaries: &[ReplacementBoundary],
    ) -> Result<Self, ValidityIndexError> {
        let mut index = Self::default();
        for assertion in assertions {
            index.insert(
                ValidityTarget::Assertion(assertion.id()),
                Some(assertion.validity()),
            )?;
        }
        for mask in masks {
            index.insert(ValidityTarget::Mask(mask.id()), mask.validity())?;
        }
        for boundary in boundaries {
            index.insert(
                ValidityTarget::ReplacementBoundary(boundary.id()),
                boundary.validity(),
            )?;
        }
        Ok(index)
    }

    /// Returns typed records whose validity contains this WorldTime.
    #[must_use]
    pub fn active_at(&self, time: WorldTime) -> BTreeSet<ValidityTarget> {
        let mut active = self.unbounded.clone();
        let Some(starts) = self.by_timeline_start.get(&time.timeline().id()) else {
            return active;
        };
        for targets in starts
            .range(..=Some(time.nanoseconds()))
            .map(|(_, targets)| targets)
        {
            for target in targets {
                let Some(entry) = self.entries.get(target) else {
                    continue;
                };
                let Some(validity) = entry.validity else {
                    continue;
                };
                if validity
                    .interval()
                    .end()
                    .is_none_or(|end| end.nanoseconds() > time.nanoseconds())
                {
                    active.insert(*target);
                }
            }
        }
        active
    }

    fn insert(
        &mut self,
        target: ValidityTarget,
        validity: Option<AssertionValidity>,
    ) -> Result<(), ValidityIndexError> {
        if self
            .entries
            .insert(target, ValidityEntry { validity })
            .is_some()
        {
            return Err(ValidityIndexError::DuplicateTarget(target));
        }
        let Some(validity) = validity else {
            self.unbounded.insert(target);
            return Ok(());
        };
        let interval = validity.interval();
        self.by_timeline_start
            .entry(interval.timeline().id())
            .or_default()
            .entry(interval.start().map(WorldTime::nanoseconds))
            .or_default()
            .push(target);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum IndexPerspective {
    World,
    Perspective(crate::ids::PerspectiveId),
}

impl From<PerspectiveScope> for IndexPerspective {
    fn from(value: PerspectiveScope) -> Self {
        match value {
            PerspectiveScope::World => Self::World,
            PerspectiveScope::Perspective(id) => Self::Perspective(id),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum IndexEpistemicMode {
    WorldState,
    Knows,
    Believes,
    Claims,
}

impl From<EpistemicMode> for IndexEpistemicMode {
    fn from(value: EpistemicMode) -> Self {
        match value {
            EpistemicMode::WorldState => Self::WorldState,
            EpistemicMode::Knows => Self::Knows,
            EpistemicMode::Believes => Self::Believes,
            EpistemicMode::Claims => Self::Claims,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum IndexPolarity {
    Positive,
    Negative,
}

impl From<Polarity> for IndexPolarity {
    fn from(value: Polarity) -> Self {
        match value {
            Polarity::Positive => Self::Positive,
            Polarity::Negative => Self::Negative,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SelectorPartition {
    perspective: IndexPerspective,
    epistemic_mode: IndexEpistemicMode,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SlotSelectorKey {
    subject: Subject,
    predicate_id: PredicateId,
    partition: SelectorPartition,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PropositionSelectorKey {
    subject: Subject,
    predicate_id: PredicateId,
    polarity: IndexPolarity,
}

/// Selector-only Mask postings. Proposition values remain schema-aware comparisons.
#[derive(Clone, Debug, Default)]
pub struct MaskSelectorIndex {
    masks: Vec<Mask>,
    exact_assertion: BTreeMap<AssertionId, Vec<usize>>,
    proposition_prefix: BTreeMap<PropositionSelectorKey, Vec<usize>>,
    slot: BTreeMap<SlotSelectorKey, Vec<usize>>,
    mask_ids: BTreeSet<MaskId>,
}

impl MaskSelectorIndex {
    /// Builds postings for every member of the closed MaskSelector family.
    pub fn build(masks: &[Mask]) -> Result<Self, MaskSelectorIndexError> {
        let mut index = Self::default();
        for mask in masks {
            if !index.mask_ids.insert(mask.id()) {
                return Err(MaskSelectorIndexError::DuplicateMaskId(mask.id()));
            }
            let position = index.masks.len();
            index.masks.push(mask.clone());
            match mask.selector() {
                MaskSelector::ExactAssertion(assertion_id) => index
                    .exact_assertion
                    .entry(*assertion_id)
                    .or_default()
                    .push(position),
                MaskSelector::Proposition(proposition) => index
                    .proposition_prefix
                    .entry(PropositionSelectorKey {
                        subject: proposition.subject(),
                        predicate_id: proposition.predicate_id(),
                        polarity: proposition.polarity().into(),
                    })
                    .or_default()
                    .push(position),
                MaskSelector::Slot(slot) => index
                    .slot
                    .entry(SlotSelectorKey {
                        subject: slot.subject(),
                        predicate_id: slot.predicate_id(),
                        partition: SelectorPartition {
                            perspective: slot.perspective_scope().into(),
                            epistemic_mode: slot.epistemic_mode().into(),
                        },
                    })
                    .or_default()
                    .push(position),
            }
        }
        Ok(index)
    }

    /// Returns only Masks whose selector exactly matches this Assertion.
    ///
    /// Proposition postings first narrow by Subject, Predicate, and Polarity;
    /// values use canonical equality or the caller's pinned temporal comparator.
    pub fn matching_masks<F>(
        &self,
        assertion: &Assertion,
        mut temporal_value_equal: F,
    ) -> Result<Vec<&Mask>, MaskSelectorIndexError>
    where
        F: FnMut(&Value, &Value) -> Result<bool, ()>,
    {
        let (subject, predicate_id, value, polarity) = assertion.proposition_components();
        let partition = SelectorPartition {
            perspective: assertion.context().perspective_scope().into(),
            epistemic_mode: assertion.context().epistemic_mode().into(),
        };
        let mut positions = BTreeSet::new();
        if let Some(found) = self.exact_assertion.get(&assertion.id()) {
            positions.extend(found.iter().copied());
        }
        if let Some(found) = self.proposition_prefix.get(&PropositionSelectorKey {
            subject,
            predicate_id,
            polarity: polarity.into(),
        }) {
            positions.extend(found.iter().copied());
        }
        if let Some(found) = self.slot.get(&SlotSelectorKey {
            subject,
            predicate_id,
            partition,
        }) {
            positions.extend(found.iter().copied());
        }

        let mut matches = Vec::new();
        for position in positions {
            let Some(mask) = self.masks.get(position) else {
                // A malformed internal posting fails closed as a miss.
                continue;
            };
            let matched = match mask.selector() {
                MaskSelector::ExactAssertion(assertion_id) => *assertion_id == assertion.id(),
                MaskSelector::Proposition(key) => {
                    match canonical_value_equality(key.value(), value) {
                        Some(equal) => equal,
                        None => temporal_value_equal(key.value(), value).map_err(|()| {
                            MaskSelectorIndexError::TemporalValueComparisonUnavailable
                        })?,
                    }
                }
                MaskSelector::Slot(slot) => {
                    slot.subject() == subject
                        && slot.predicate_id() == predicate_id
                        && slot.perspective_scope() == assertion.context().perspective_scope()
                        && slot.epistemic_mode() == assertion.context().epistemic_mode()
                }
            };
            if matched {
                matches.push(mask);
            }
        }
        matches.sort_unstable_by_key(|mask| mask.id());
        Ok(matches)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PrecedenceKey {
    query_history_space_id: HistorySpaceId,
    record_history_space_id: HistorySpaceId,
    layer_id: LayerId,
}

/// Snapshot-bound cache of HistorySpace distance and LayerRank coordinates.
#[derive(Clone, Debug)]
pub struct ContextPrecedenceIndex {
    entries: BTreeMap<PrecedenceKey, ContextPrecedence>,
    history_spaces: HistorySpaceCatalog,
    layers: LayerSchemaSnapshot,
}

impl ContextPrecedenceIndex {
    /// Precomputes each query-space/visible-ancestor/layer precedence tuple.
    pub fn build(
        history_spaces: &HistorySpaceCatalog,
        layers: &LayerSchemaSnapshot,
    ) -> Result<Self, ContextPrecedenceError> {
        let mut index = Self {
            entries: BTreeMap::new(),
            history_spaces: history_spaces.clone(),
            layers: layers.clone(),
        };
        for query in history_spaces.definitions() {
            let query_history_space_id = query.history_space_id();
            let mut visible = Some(query_history_space_id);
            while let Some(record_history_space_id) = visible {
                for layer in layers.definitions() {
                    let layer_id = layer.layer_id();
                    let precedence = ContextPrecedence::for_context(
                        query_history_space_id,
                        record_history_space_id,
                        layer_id,
                        history_spaces,
                        layers,
                    )?;
                    index.entries.insert(
                        PrecedenceKey {
                            query_history_space_id,
                            record_history_space_id,
                            layer_id,
                        },
                        precedence,
                    );
                }
                visible = history_spaces
                    .definition(record_history_space_id)
                    .and_then(|definition| definition.parent_history_space_id());
            }
        }
        Ok(index)
    }

    /// Returns the cached precedence or the reference-model error for this tuple.
    pub fn for_context(
        &self,
        query_history_space_id: HistorySpaceId,
        record_history_space_id: HistorySpaceId,
        layer_id: LayerId,
    ) -> Result<ContextPrecedence, ContextPrecedenceError> {
        let key = PrecedenceKey {
            query_history_space_id,
            record_history_space_id,
            layer_id,
        };
        self.entries.get(&key).copied().map_or_else(
            || {
                ContextPrecedence::for_context(
                    query_history_space_id,
                    record_history_space_id,
                    layer_id,
                    &self.history_spaces,
                    &self.layers,
                )
            },
            Ok,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BoundarySlotKey {
    subject: Subject,
    predicate_id: PredicateId,
    partition: SelectorPartition,
}

/// ReplacementBoundary postings by selected MultiValue slot and epistemic partition.
#[derive(Clone, Debug, Default)]
pub struct ReplacementBoundaryIndex {
    boundaries: Vec<ReplacementBoundary>,
    by_slot: BTreeMap<BoundarySlotKey, Vec<usize>>,
    boundary_ids: BTreeSet<ReplacementBoundaryId>,
}

impl ReplacementBoundaryIndex {
    /// Builds broad slot postings; time, layer, archive, lifecycle, and ancestry remain filters.
    pub fn build(
        boundaries: &[ReplacementBoundary],
    ) -> Result<Self, ReplacementBoundaryIndexError> {
        let mut index = Self::default();
        for boundary in boundaries {
            if !index.boundary_ids.insert(boundary.id()) {
                return Err(ReplacementBoundaryIndexError::DuplicateBoundaryId(
                    boundary.id(),
                ));
            }
            let position = index.boundaries.len();
            index.boundaries.push(boundary.clone());
            let context = boundary.context();
            index
                .by_slot
                .entry(BoundarySlotKey {
                    subject: boundary.subject(),
                    predicate_id: boundary.predicate_id(),
                    partition: SelectorPartition {
                        perspective: context.perspective_scope().into(),
                        epistemic_mode: context.epistemic_mode().into(),
                    },
                })
                .or_default()
                .push(position);
        }
        Ok(index)
    }

    /// Returns boundaries matching one slot and query partition.
    #[must_use]
    pub fn for_slot(
        &self,
        slot: MultiValueSlot,
        query_context: ContextKey,
    ) -> Vec<&ReplacementBoundary> {
        let key = BoundarySlotKey {
            subject: slot.subject(),
            predicate_id: slot.predicate_id(),
            partition: SelectorPartition {
                perspective: query_context.perspective_scope().into(),
                epistemic_mode: query_context.epistemic_mode().into(),
            },
        };
        let mut matches = self
            .by_slot
            .get(&key)
            .into_iter()
            .flatten()
            .filter_map(|position| self.boundaries.get(*position))
            .collect::<Vec<_>>();
        matches.sort_unstable_by_key(|boundary| boundary.id());
        matches
    }
}

/// Duplicate owner encountered while building a validity index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidityIndexError {
    /// One typed assertion, Mask, or boundary identity appears more than once.
    DuplicateTarget(ValidityTarget),
}

impl fmt::Display for ValidityIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTarget(target) => {
                write!(formatter, "duplicate validity target {target:?}")
            }
        }
    }
}

impl std::error::Error for ValidityIndexError {}

/// Invalid Mask selector index source or temporal comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaskSelectorIndexError {
    /// One Mask ID appears more than once.
    DuplicateMaskId(MaskId),
    /// Proposition value equality needs a schema-aware temporal comparator.
    TemporalValueComparisonUnavailable,
}

impl fmt::Display for MaskSelectorIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateMaskId(id) => write!(formatter, "duplicate MaskId {id}"),
            Self::TemporalValueComparisonUnavailable => {
                formatter.write_str("temporal proposition value requires schema-aware equality")
            }
        }
    }
}

impl std::error::Error for MaskSelectorIndexError {}

/// Duplicate record identity encountered while building a boundary index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplacementBoundaryIndexError {
    /// One ReplacementBoundary identity appears more than once.
    DuplicateBoundaryId(ReplacementBoundaryId),
}

impl fmt::Display for ReplacementBoundaryIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBoundaryId(id) => write!(formatter, "duplicate boundary ID {id}"),
        }
    }
}

impl std::error::Error for ReplacementBoundaryIndexError {}

#[cfg(test)]
mod tests {
    use super::{
        AssertionValidityIndex, ContextPrecedenceIndex, MaskSelectorIndex,
        ReplacementBoundaryIndex, ValidityTarget,
    };
    use crate::assertions::{Assertion, AssertionDraft, Polarity, Subject};
    use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition};
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::context_precedence::{ContextPrecedence, ContextPrecedenceError};
    use crate::ids::{
        AssertionId, DomainId, EntityId, HistorySpaceId, LayerId, MaskId, MaskRetractionId,
        MaskValidityClosureId, PerspectiveId, PredicateId, ReplacementBoundaryId, Revision,
        RevisionError, SchemaRevision, TimelineId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot};
    use crate::mask_projection::{
        AssertionMaskContext, MaskProjectionError, apply_assertion_masks,
    };
    use crate::masks::{
        Mask, MaskRetraction, MaskSelector, MaskSlotSelector, MaskValidityClosure, PropositionKey,
        ReplacementBoundary, ReplacementBoundaryRetraction, ReplacementBoundaryValidityClosure,
    };
    use crate::multi_value_resolution::{
        MultiValueReplaceContext, MultiValueSlot, ReplacementBoundaryHistory,
        resolve_multi_value_replace,
    };
    use crate::schema::{
        Cardinality, ConstraintSet, EntityTypeConstraint, Lifecycle, PredicateDefinition,
        PredicateDefinitionSpec, ResolutionPolicy, ValueKind,
    };
    use crate::temporal::{AssertionValidity, TimeInterval, Timeline, WorldTime};
    use crate::values::{Symbol, Value, canonical_value_equality};
    use std::collections::BTreeSet;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(crate::ids::IdValidationError),
        Revision(RevisionError),
        Catalog(crate::catalog::HistorySpaceError),
        Context(crate::context::ContextError),
        Layer(crate::layers::LayerSchemaError),
        Temporal(crate::temporal::TemporalError),
        Mask(crate::masks::MaskRecordError),
        Boundary(crate::masks::ReplacementBoundaryError),
        Schema(crate::schema::SchemaDefinitionError),
        Symbol(crate::values::SymbolError),
        Precedence(crate::context_precedence::ContextPrecedenceError),
        Validity(super::ValidityIndexError),
        Selector(super::MaskSelectorIndexError),
        BoundaryIndex(super::ReplacementBoundaryIndexError),
        MaskProjection(MaskProjectionError),
        Resolution(crate::multi_value_resolution::MultiValueResolutionError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "invalid test ID: {error}"),
                Self::Revision(error) => write!(formatter, "invalid test revision: {error}"),
                Self::Catalog(error) => write!(formatter, "invalid test catalog: {error}"),
                Self::Context(error) => write!(formatter, "invalid test context: {error}"),
                Self::Layer(error) => write!(formatter, "invalid test layer: {error}"),
                Self::Temporal(error) => write!(formatter, "invalid test time: {error}"),
                Self::Mask(error) => write!(formatter, "invalid test Mask: {error}"),
                Self::Boundary(error) => write!(formatter, "invalid test boundary: {error}"),
                Self::Schema(error) => write!(formatter, "invalid test schema: {error}"),
                Self::Symbol(error) => write!(formatter, "invalid test symbol: {error}"),
                Self::Precedence(error) => write!(formatter, "invalid test precedence: {error}"),
                Self::Validity(error) => write!(formatter, "invalid validity index: {error}"),
                Self::Selector(error) => write!(formatter, "invalid Mask selector index: {error}"),
                Self::BoundaryIndex(error) => write!(formatter, "invalid boundary index: {error}"),
                Self::MaskProjection(error) => {
                    write!(formatter, "invalid mask projection: {error}")
                }
                Self::Resolution(error) => write!(formatter, "invalid resolution: {error}"),
            }
        }
    }

    impl std::error::Error for TestError {}

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
    test_error_from!(RevisionError, Revision);
    test_error_from!(crate::catalog::HistorySpaceError, Catalog);
    test_error_from!(crate::context::ContextError, Context);
    test_error_from!(crate::layers::LayerSchemaError, Layer);
    test_error_from!(crate::temporal::TemporalError, Temporal);
    test_error_from!(crate::masks::MaskRecordError, Mask);
    test_error_from!(crate::masks::ReplacementBoundaryError, Boundary);
    test_error_from!(crate::schema::SchemaDefinitionError, Schema);
    test_error_from!(crate::values::SymbolError, Symbol);
    test_error_from!(
        crate::context_precedence::ContextPrecedenceError,
        Precedence
    );
    test_error_from!(super::ValidityIndexError, Validity);
    test_error_from!(super::MaskSelectorIndexError, Selector);
    test_error_from!(super::ReplacementBoundaryIndexError, BoundaryIndex);
    test_error_from!(MaskProjectionError, MaskProjection);
    test_error_from!(
        crate::multi_value_resolution::MultiValueResolutionError,
        Resolution
    );

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn context(
        history_space: HistorySpaceId,
        layer: LayerId,
        perspective: PerspectiveScope,
        mode: EpistemicMode,
    ) -> Result<ContextKey, crate::context::ContextError> {
        ContextKey::new(history_space, layer, perspective, mode)
    }

    fn validity(
        timeline: Timeline,
        start: Option<i128>,
        end: Option<i128>,
    ) -> Result<AssertionValidity, crate::temporal::TemporalError> {
        TimeInterval::new(
            timeline,
            start.map(|nanos| WorldTime::from_nanoseconds(timeline, nanos)),
            end.map(|nanos| WorldTime::from_nanoseconds(timeline, nanos)),
        )
        .map(AssertionValidity::new)
    }

    struct AssertionSpec {
        id: AssertionId,
        context: ContextKey,
        subject: Subject,
        predicate_id: PredicateId,
        value: Value,
        polarity: Polarity,
        validity: AssertionValidity,
        revision: Revision,
    }

    fn assertion(spec: AssertionSpec) -> Assertion {
        Assertion::new(
            spec.id,
            AssertionDraft::new(
                spec.context,
                spec.subject,
                spec.predicate_id,
                spec.value,
                spec.polarity,
                spec.validity,
            ),
            spec.revision,
        )
    }

    fn mask(
        id_tail: u8,
        context: ContextKey,
        selector: MaskSelector,
        validity: Option<AssertionValidity>,
        revision: Revision,
    ) -> Result<Mask, TestError> {
        Ok(Mask::new(
            id::<MaskId>(id_tail)?,
            context,
            selector,
            validity,
            revision,
        )?)
    }

    fn predicate_definition(id: PredicateId) -> Result<PredicateDefinition, TestError> {
        Ok(PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: id,
            symbol: Symbol::new("place")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy: ResolutionPolicy::MultiValueReplace,
            constraints: ConstraintSet::unconstrained(),
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: Revision::GENESIS,
        })?)
    }

    fn boundary(
        id_tail: u8,
        context: ContextKey,
        subject: Subject,
        predicate: &PredicateDefinition,
        validity: Option<AssertionValidity>,
        revision: Revision,
    ) -> Result<ReplacementBoundary, TestError> {
        Ok(ReplacementBoundary::new(
            id::<ReplacementBoundaryId>(id_tail)?,
            context,
            subject,
            predicate,
            validity,
            revision,
        )?)
    }

    fn layers() -> Result<(LayerSchemaSnapshot, LayerId, LayerId), TestError> {
        let base = id::<LayerId>(1)?;
        let overlay = id::<LayerId>(2)?;
        let revision = SchemaRevision::from_published_revision(Revision::GENESIS);
        let snapshot = LayerSchemaSnapshot::new(
            revision,
            vec![
                LayerDefinition::new(
                    base,
                    Symbol::new("base")?,
                    None,
                    0,
                    Lifecycle::Active,
                    revision,
                ),
                LayerDefinition::new(
                    overlay,
                    Symbol::new("overlay")?,
                    None,
                    10,
                    Lifecycle::Active,
                    revision,
                ),
            ],
            base,
        )?;
        Ok((snapshot, base, overlay))
    }

    fn history_spaces() -> Result<(HistorySpaceCatalog, [HistorySpaceId; 4]), TestError> {
        let root = id::<HistorySpaceId>(20)?;
        let child = id::<HistorySpaceId>(21)?;
        let grandchild = id::<HistorySpaceId>(22)?;
        let sibling = id::<HistorySpaceId>(23)?;
        let catalog = HistorySpaceCatalog::new(vec![
            HistorySpaceDefinition::new(root, None, Revision::GENESIS)?,
            HistorySpaceDefinition::new(child, Some(root), revision(3)?)?,
            HistorySpaceDefinition::new(grandchild, Some(child), revision(5)?)?,
            HistorySpaceDefinition::new(sibling, Some(root), revision(1)?)?,
        ])?;
        Ok((catalog, [root, child, grandchild, sibling]))
    }

    #[test]
    fn validity_index_matches_half_open_full_scan_for_assertions_masks_and_boundaries() -> TestResult
    {
        let root = id::<HistorySpaceId>(40)?;
        let (_, base, _) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(41)?);
        let subject = Subject::new(id::<EntityId>(42)?);
        let predicate_id = id::<PredicateId>(43)?;
        let first_assertion_id = id::<AssertionId>(44)?;
        let second_assertion_id = id::<AssertionId>(45)?;
        let context = context(
            root,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let assertions = vec![
            assertion(AssertionSpec {
                id: first_assertion_id,
                context,
                subject,
                predicate_id,
                value: Value::String(String::from("a")),
                polarity: Polarity::Positive,
                validity: validity(timeline, Some(0), Some(10))?,
                revision: revision(1)?,
            }),
            assertion(AssertionSpec {
                id: second_assertion_id,
                context,
                subject,
                predicate_id,
                value: Value::String(String::from("b")),
                polarity: Polarity::Positive,
                validity: validity(timeline, Some(10), Some(20))?,
                revision: revision(1)?,
            }),
        ];
        let masks = vec![
            mask(
                46,
                context,
                MaskSelector::ExactAssertion(first_assertion_id),
                Some(validity(timeline, None, Some(10))?),
                revision(1)?,
            )?,
            mask(
                47,
                context,
                MaskSelector::ExactAssertion(second_assertion_id),
                None,
                revision(1)?,
            )?,
            mask(
                48,
                context,
                MaskSelector::ExactAssertion(first_assertion_id),
                Some(validity(timeline, Some(10), Some(10))?),
                revision(1)?,
            )?,
        ];
        let predicate = predicate_definition(predicate_id)?;
        let boundaries = vec![
            boundary(
                49,
                context,
                subject,
                &predicate,
                Some(validity(timeline, Some(0), None)?),
                revision(1)?,
            )?,
            boundary(50, context, subject, &predicate, None, revision(1)?)?,
        ];
        let index = AssertionValidityIndex::build(&assertions, &masks, &boundaries)?;

        for point in [-1_i128, 0, 9, 10, 19, 20] {
            let time = WorldTime::from_nanoseconds(timeline, point);
            let mut expected = BTreeSet::new();
            for assertion in &assertions {
                if assertion.validity().contains(time)? {
                    expected.insert(ValidityTarget::Assertion(assertion.id()));
                }
            }
            for mask in &masks {
                if match mask.validity() {
                    Some(item) => item.contains(time)?,
                    None => true,
                } {
                    expected.insert(ValidityTarget::Mask(mask.id()));
                }
            }
            for boundary in &boundaries {
                if match boundary.validity() {
                    Some(item) => item.contains(time)?,
                    None => true,
                } {
                    expected.insert(ValidityTarget::ReplacementBoundary(boundary.id()));
                }
            }
            assert_eq!(index.active_at(time), expected, "world time {point}");
        }
        Ok(())
    }

    #[test]
    fn mask_selector_postings_match_closed_selector_full_scan() -> TestResult {
        let root = id::<HistorySpaceId>(60)?;
        let (_, layer, _) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(61)?);
        let subject = Subject::new(id::<EntityId>(62)?);
        let other_subject = Subject::new(id::<EntityId>(63)?);
        let predicate_id = id::<PredicateId>(64)?;
        let perspective_id = id::<PerspectiveId>(65)?;
        let context = context(
            root,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let other_partition_context = ContextKey::new(
            root,
            layer,
            PerspectiveScope::Perspective(perspective_id),
            EpistemicMode::Knows,
        )?;
        let assertion = assertion(AssertionSpec {
            id: id::<AssertionId>(66)?,
            context,
            subject,
            predicate_id,
            value: Value::String(String::from("same")),
            polarity: Polarity::Positive,
            validity: validity(timeline, None, None)?,
            revision: revision(1)?,
        });
        let proposition = |value: &str, polarity| {
            MaskSelector::Proposition(PropositionKey::new(
                subject,
                predicate_id,
                Value::String(String::from(value)),
                polarity,
            ))
        };
        let masks = vec![
            mask(
                67,
                context,
                MaskSelector::ExactAssertion(assertion.id()),
                None,
                revision(1)?,
            )?,
            mask(
                68,
                context,
                MaskSelector::ExactAssertion(id::<AssertionId>(69)?),
                None,
                revision(1)?,
            )?,
            mask(
                70,
                context,
                proposition("same", Polarity::Positive),
                None,
                revision(1)?,
            )?,
            mask(
                71,
                context,
                proposition("other", Polarity::Positive),
                None,
                revision(1)?,
            )?,
            mask(
                72,
                context,
                proposition("same", Polarity::Negative),
                None,
                revision(1)?,
            )?,
            mask(
                73,
                context,
                MaskSelector::Slot(MaskSlotSelector::new(
                    subject,
                    predicate_id,
                    PerspectiveScope::World,
                    EpistemicMode::WorldState,
                )?),
                None,
                revision(1)?,
            )?,
            mask(
                74,
                context,
                MaskSelector::Slot(MaskSlotSelector::new(
                    other_subject,
                    predicate_id,
                    PerspectiveScope::World,
                    EpistemicMode::WorldState,
                )?),
                None,
                revision(1)?,
            )?,
            mask(
                75,
                other_partition_context,
                MaskSelector::Slot(MaskSlotSelector::new(
                    subject,
                    predicate_id,
                    PerspectiveScope::Perspective(perspective_id),
                    EpistemicMode::Knows,
                )?),
                None,
                revision(1)?,
            )?,
        ];
        let index = MaskSelectorIndex::build(&masks)?;
        let indexed = index.matching_masks(&assertion, |left, right| {
            crate::values::canonical_value_equality(left, right).ok_or(())
        })?;
        let scan = masks
            .iter()
            .filter(|mask| slow_selector_matches(mask.selector(), &assertion))
            .map(Mask::id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            indexed
                .iter()
                .map(|mask| mask.id())
                .collect::<BTreeSet<_>>(),
            scan
        );
        assert_eq!(
            indexed.iter().map(|mask| mask.id()).collect::<Vec<_>>(),
            vec![id::<MaskId>(67)?, id::<MaskId>(70)?, id::<MaskId>(73)?]
        );
        Ok(())
    }

    #[test]
    fn mask_projection_composes_selector_validity_precedence_indexes_match_full_scan() -> TestResult
    {
        let (catalog, [root, child, _grandchild, sibling]) = history_spaces()?;
        let (layers, base, overlay) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(112)?);
        let subject = Subject::new(id::<EntityId>(113)?);
        let predicate_id = id::<PredicateId>(114)?;
        let overlay_assertion_id = id::<AssertionId>(117)?;
        let root_base = context(
            root,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let child_base = context(
            child,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let child_overlay = context(
            child,
            overlay,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let sibling_base = context(
            sibling,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let assertion_records = vec![
            assertion(AssertionSpec {
                id: id::<AssertionId>(115)?,
                context: root_base,
                subject,
                predicate_id,
                value: Value::String(String::from("root")),
                polarity: Polarity::Positive,
                validity: validity(timeline, None, None)?,
                revision: revision(1)?,
            }),
            assertion(AssertionSpec {
                id: id::<AssertionId>(116)?,
                context: child_base,
                subject,
                predicate_id,
                value: Value::String(String::from("child-base")),
                polarity: Polarity::Positive,
                validity: validity(timeline, None, None)?,
                revision: revision(3)?,
            }),
            assertion(AssertionSpec {
                id: overlay_assertion_id,
                context: child_overlay,
                subject,
                predicate_id,
                value: Value::String(String::from("child-overlay")),
                polarity: Polarity::Positive,
                validity: validity(timeline, None, None)?,
                revision: revision(4)?,
            }),
        ];
        let selected_layers = [base, overlay].into_iter().collect::<BTreeSet<_>>();
        let candidates = assertion_records
            .iter()
            .map(|assertion| {
                let source = assertion.context().history_space_id();
                Ok(crate::AssertionCandidate {
                    assertion: assertion.clone(),
                    source_history_space_id: source,
                    query_history_space_id: child,
                    selected_layer_ids: selected_layers.clone(),
                    precedence: ContextPrecedence::for_context(
                        child,
                        source,
                        assertion.context().layer_id(),
                        &catalog,
                        &layers,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, ContextPrecedenceError>>()
            .map_err(TestError::Precedence)?;

        let slot_selector = || -> Result<MaskSelector, crate::masks::MaskRecordError> {
            Ok(MaskSelector::Slot(MaskSlotSelector::new(
                subject,
                predicate_id,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?))
        };
        let closing_mask = mask(
            118,
            child_overlay,
            slot_selector()?,
            Some(validity(timeline, Some(0), Some(10))?),
            revision(4)?,
        )?;
        let retracted_mask = mask(120, child_base, slot_selector()?, None, revision(3)?)?;
        let archived_mask_id = id::<MaskId>(122)?;
        let masks = vec![
            closing_mask.clone(),
            mask(
                119,
                child_base,
                slot_selector()?,
                Some(validity(timeline, Some(10), Some(20))?),
                revision(3)?,
            )?,
            retracted_mask.clone(),
            mask(121, child_overlay, slot_selector()?, None, revision(6)?)?,
            mask(122, child_overlay, slot_selector()?, None, revision(4)?)?,
            mask(123, sibling_base, slot_selector()?, None, revision(4)?)?,
            mask(
                124,
                root_base,
                MaskSelector::ExactAssertion(overlay_assertion_id),
                None,
                revision(1)?,
            )?,
        ];
        let closures = vec![MaskValidityClosure::new(
            id::<MaskValidityClosureId>(125)?,
            &closing_mask,
            WorldTime::from_nanoseconds(timeline, 1),
            revision(5)?,
        )?];
        let retractions = vec![MaskRetraction::new(
            id::<MaskRetractionId>(126)?,
            &retracted_mask,
            "retracted before query",
            revision(4)?,
        )?];
        let archive_visible = masks
            .iter()
            .filter(|mask| mask.id() != archived_mask_id)
            .map(Mask::id)
            .collect::<BTreeSet<_>>();
        let selector_index = MaskSelectorIndex::build(&masks)?;
        let validity_index = AssertionValidityIndex::build(&[], &masks, &[])?;
        let precedence_index = ContextPrecedenceIndex::build(&catalog, &layers)?;
        let as_of = crate::temporal::RecordedAsOf::from_published_revision(revision(5)?);

        for point in [0_i128, 2, 10, 20] {
            let world_time = WorldTime::from_nanoseconds(timeline, point);
            let full = apply_assertion_masks(
                &candidates,
                &masks,
                &closures,
                &retractions,
                &archive_visible,
                AssertionMaskContext::new(as_of, world_time, &catalog, &layers)
                    .with_query_history_space(child),
                |left, right| canonical_value_equality(left, right).ok_or(()),
            )?;

            let time_visible = validity_index.active_at(world_time);
            let mut indexed = Vec::new();
            for candidate in &candidates {
                let mut hidden = false;
                for mask in selector_index
                    .matching_masks(candidate.assertion(), |left, right| {
                        canonical_value_equality(left, right).ok_or(())
                    })?
                {
                    if mask.created_revision() > as_of.revision()
                        || !archive_visible.contains(&mask.id())
                        || retractions.iter().any(|retraction| {
                            retraction.mask_id() == mask.id()
                                && retraction.created_revision() <= as_of.revision()
                        })
                        || !time_visible.contains(&ValidityTarget::Mask(mask.id()))
                        || closures.iter().any(|closure| {
                            closure.mask_id() == mask.id()
                                && closure.created_revision() <= as_of.revision()
                                && closure.close_at_world_time().timeline().id()
                                    == world_time.timeline().id()
                                && world_time.nanoseconds()
                                    >= closure.close_at_world_time().nanoseconds()
                        })
                        || !candidate
                            .selected_layer_ids()
                            .contains(&mask.context().layer_id())
                        || mask.context().perspective_scope()
                            != candidate.assertion().context().perspective_scope()
                        || mask.context().epistemic_mode()
                            != candidate.assertion().context().epistemic_mode()
                    {
                        continue;
                    }
                    let mask_precedence = match precedence_index.for_context(
                        child,
                        mask.context().history_space_id(),
                        mask.context().layer_id(),
                    ) {
                        Ok(precedence) => precedence,
                        Err(ContextPrecedenceError::RecordOutsideQueryAncestry { .. }) => {
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    };
                    if mask_precedence > candidate.precedence() {
                        hidden = true;
                        break;
                    }
                }
                if !hidden {
                    indexed.push(candidate.clone());
                }
            }

            let full_ids = full
                .iter()
                .map(|candidate| candidate.assertion().id())
                .collect::<Vec<_>>();
            let indexed_ids = indexed
                .iter()
                .map(|candidate| candidate.assertion().id())
                .collect::<Vec<_>>();
            assert_eq!(indexed_ids, full_ids, "world time {point}");
        }
        Ok(())
    }

    fn slow_selector_matches(selector: &MaskSelector, assertion: &Assertion) -> bool {
        match selector {
            MaskSelector::ExactAssertion(id) => *id == assertion.id(),
            MaskSelector::Proposition(key) => {
                let (subject, predicate_id, value, polarity) = assertion.proposition_components();
                key.subject() == subject
                    && key.predicate_id() == predicate_id
                    && key.polarity() == polarity
                    && crate::values::canonical_value_equality(key.value(), value).unwrap_or(false)
            }
            MaskSelector::Slot(slot) => {
                slot.subject() == assertion.subject()
                    && slot.predicate_id() == assertion.predicate_id()
                    && slot.perspective_scope() == assertion.context().perspective_scope()
                    && slot.epistemic_mode() == assertion.context().epistemic_mode()
            }
        }
    }

    #[test]
    fn cached_context_precedence_matches_slow_ancestry_calculation() -> TestResult {
        let (catalog, spaces) = history_spaces()?;
        let (layers, base, overlay) = layers()?;
        let index = ContextPrecedenceIndex::build(&catalog, &layers)?;
        let layer_ids = [base, overlay];
        for query in spaces {
            for record in spaces {
                for layer in layer_ids {
                    assert_eq!(
                        index.for_context(query, record, layer),
                        ContextPrecedence::for_context(query, record, layer, &catalog, &layers)
                    );
                }
            }
        }
        let unknown_layer = id::<LayerId>(90)?;
        assert_eq!(
            index.for_context(spaces[1], spaces[0], unknown_layer),
            ContextPrecedence::for_context(spaces[1], spaces[0], unknown_layer, &catalog, &layers)
        );
        Ok(())
    }

    #[test]
    fn boundary_slot_index_matches_scan_and_preserves_replace_results() -> TestResult {
        let (catalog, spaces) = history_spaces()?;
        let [root, child, _grandchild, sibling] = spaces;
        let (layers, base, overlay) = layers()?;
        let timeline = Timeline::new(id::<TimelineId>(100)?);
        let subject = Subject::new(id::<EntityId>(101)?);
        let other_subject = Subject::new(id::<EntityId>(102)?);
        let predicate_id = id::<PredicateId>(103)?;
        let predicate = predicate_definition(predicate_id)?;
        let root_base = context(
            root,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let child_base = context(
            child,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let child_overlay = context(
            child,
            overlay,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let sibling_base = context(
            sibling,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let boundaries = vec![
            boundary(
                104,
                root_base,
                subject,
                &predicate,
                Some(validity(timeline, Some(0), Some(20))?),
                revision(1)?,
            )?,
            boundary(
                105,
                child_base,
                subject,
                &predicate,
                Some(validity(timeline, Some(10), Some(20))?),
                revision(3)?,
            )?,
            boundary(
                106,
                child_overlay,
                subject,
                &predicate,
                Some(validity(timeline, Some(30), Some(40))?),
                revision(4)?,
            )?,
            boundary(107, sibling_base, subject, &predicate, None, revision(1)?)?,
            boundary(
                108,
                child_overlay,
                other_subject,
                &predicate,
                None,
                revision(4)?,
            )?,
        ];
        let index = ReplacementBoundaryIndex::build(&boundaries)?;
        let query_context = context(
            child,
            base,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let slot = MultiValueSlot::new(subject, predicate_id);
        let indexed = index
            .for_slot(slot, query_context)
            .iter()
            .map(|boundary| boundary.id())
            .collect::<BTreeSet<_>>();
        let scan = boundaries
            .iter()
            .filter(|boundary| {
                boundary.subject() == subject
                    && boundary.predicate_id() == predicate_id
                    && boundary.context().perspective_scope() == query_context.perspective_scope()
                    && boundary.context().epistemic_mode() == query_context.epistemic_mode()
            })
            .map(ReplacementBoundary::id)
            .collect::<BTreeSet<_>>();
        assert_eq!(indexed, scan);

        let assertion_context = root_base;
        let candidate_assertion = assertion(AssertionSpec {
            id: id::<AssertionId>(109)?,
            context: assertion_context,
            subject,
            predicate_id,
            value: Value::String(String::from("parent")),
            polarity: Polarity::Positive,
            validity: validity(timeline, None, None)?,
            revision: revision(1)?,
        });
        let child_base_assertion = assertion(AssertionSpec {
            id: id::<AssertionId>(110)?,
            context: child_base,
            subject,
            predicate_id,
            value: Value::String(String::from("child-base")),
            polarity: Polarity::Positive,
            validity: validity(timeline, None, None)?,
            revision: revision(3)?,
        });
        let child_overlay_assertion = assertion(AssertionSpec {
            id: id::<AssertionId>(111)?,
            context: child_overlay,
            subject,
            predicate_id,
            value: Value::String(String::from("child-overlay")),
            polarity: Polarity::Positive,
            validity: validity(timeline, None, None)?,
            revision: revision(4)?,
        });
        let layer_ids = [base, overlay].into_iter().collect::<BTreeSet<_>>();
        let candidates = vec![
            crate::AssertionCandidate {
                assertion: candidate_assertion,
                source_history_space_id: root,
                query_history_space_id: child,
                selected_layer_ids: layer_ids.clone(),
                precedence: ContextPrecedence::for_context(child, root, base, &catalog, &layers)?,
            },
            crate::AssertionCandidate {
                assertion: child_base_assertion,
                source_history_space_id: child,
                query_history_space_id: child,
                selected_layer_ids: layer_ids.clone(),
                precedence: ContextPrecedence::for_context(child, child, base, &catalog, &layers)?,
            },
            crate::AssertionCandidate {
                assertion: child_overlay_assertion,
                source_history_space_id: child,
                query_history_space_id: child,
                selected_layer_ids: layer_ids.clone(),
                precedence: ContextPrecedence::for_context(
                    child, child, overlay, &catalog, &layers,
                )?,
            },
        ];
        let archive = boundaries
            .iter()
            .map(ReplacementBoundary::id)
            .collect::<BTreeSet<_>>();
        let empty_closures: Vec<ReplacementBoundaryValidityClosure> = Vec::new();
        let empty_retractions: Vec<ReplacementBoundaryRetraction> = Vec::new();
        let query_time = WorldTime::from_nanoseconds(timeline, 10);
        let as_of = crate::temporal::RecordedAsOf::from_published_revision(revision(5)?);
        let full = resolve_multi_value_replace(
            &candidates,
            ReplacementBoundaryHistory::new(
                &boundaries,
                &empty_closures,
                &empty_retractions,
                &archive,
            ),
            slot,
            &predicate,
            MultiValueReplaceContext::new(
                query_context,
                layer_ids.clone(),
                as_of,
                query_time,
                &catalog,
                &layers,
            ),
            |left, right| crate::values::canonical_value_equality(left, right).ok_or(()),
        )?;

        let validity_index = AssertionValidityIndex::build(&[], &[], &boundaries)?;
        let time_visible = validity_index.active_at(query_time);
        let precedence_index = ContextPrecedenceIndex::build(&catalog, &layers)?;
        let eligible_ids = index
            .for_slot(slot, query_context)
            .into_iter()
            .filter(|boundary| {
                boundary.created_revision() <= as_of.revision()
                    && archive.contains(&boundary.id())
                    && layer_ids.contains(&boundary.context().layer_id())
                    && time_visible.contains(&ValidityTarget::ReplacementBoundary(boundary.id()))
            })
            .filter(|boundary| {
                precedence_index
                    .for_context(
                        query_context.history_space_id(),
                        boundary.context().history_space_id(),
                        boundary.context().layer_id(),
                    )
                    .is_ok()
            })
            .map(|boundary| boundary.id())
            .collect::<BTreeSet<_>>();
        let indexed_boundaries = boundaries
            .iter()
            .filter(|boundary| eligible_ids.contains(&boundary.id()))
            .cloned()
            .collect::<Vec<_>>();
        let indexed_archive = eligible_ids.clone();
        let indexed = resolve_multi_value_replace(
            &candidates,
            ReplacementBoundaryHistory::new(
                &indexed_boundaries,
                &empty_closures,
                &empty_retractions,
                &indexed_archive,
            ),
            slot,
            &predicate,
            MultiValueReplaceContext::new(
                query_context,
                layer_ids,
                as_of,
                query_time,
                &catalog,
                &layers,
            ),
            |left, right| crate::values::canonical_value_equality(left, right).ok_or(()),
        )?;
        assert_eq!(outcome_ids(&full), outcome_ids(&indexed));
        assert_eq!(
            outcome_ids(&full).into_iter().collect::<BTreeSet<_>>(),
            [id::<AssertionId>(110)?, id::<AssertionId>(111)?]
                .into_iter()
                .collect()
        );
        Ok(())
    }

    fn outcome_ids(outcome: &crate::multi_value_resolution::MultiValueOutcome) -> Vec<AssertionId> {
        match outcome {
            crate::multi_value_resolution::MultiValueOutcome::Unknown => Vec::new(),
            crate::multi_value_resolution::MultiValueOutcome::Known { values }
            | crate::multi_value_resolution::MultiValueOutcome::Conflict { values, .. } => values
                .iter()
                .flat_map(|entry| entry.contributors().iter().copied())
                .collect(),
        }
    }
}
