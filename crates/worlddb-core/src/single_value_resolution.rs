//! Reference-model resolution for SingleValueReplace predicate slots.

use std::collections::BTreeSet;
use std::fmt;

use crate::assertions::{Polarity, Subject};
use crate::candidate_scan::AssertionCandidate;
use crate::ids::{AssertionId, PredicateId};
use crate::schema::{PredicateDefinition, ResolutionPolicy, ValueKind};
use crate::values::{Value, canonical_value_equality};

/// One logical subject/predicate slot selected for single-value resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SingleValueSlot {
    subject: Subject,
    predicate_id: PredicateId,
}

impl SingleValueSlot {
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

/// One resolved result of `SingleValueReplace`.
#[derive(Clone, Debug)]
pub enum SingleValueOutcome {
    /// One proposition remains, possibly represented by duplicate equal assertions.
    Known {
        /// The canonical retained proposition value.
        value: Value,
        /// Its explicit polarity; negative is not retraction.
        polarity: Polarity,
        /// Stable, sorted assertion IDs supporting this known value.
        contributors: Vec<AssertionId>,
    },
    /// No candidate remains after historical, security, archive, and mask filtering.
    Unknown,
    /// Equally preferred candidates disagree in Value or Polarity.
    Conflict {
        /// All equally preferred competing assertion IDs, in canonical order.
        contributors: Vec<AssertionId>,
    },
}

/// Resolves one selected SingleValueReplace slot from visible, post-mask candidates.
///
/// Lower-precedence candidates are replaced by the greatest available
/// `ContextPrecedence`. Equal-precedence peers are never ordered by ID, revision,
/// or input order: agreeing peers produce `Known`, while any disagreement
/// produces `Conflict`. Time values are compared by the caller using the pinned
/// schema snapshot; a missing comparison fails closed.
pub fn resolve_single_value_replace<F>(
    candidates: &[AssertionCandidate],
    slot: SingleValueSlot,
    predicate: &PredicateDefinition,
    mut temporal_value_equal: F,
) -> Result<SingleValueOutcome, SingleValueResolutionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    if predicate.predicate_id() != slot.predicate_id {
        return Err(SingleValueResolutionError::SchemaPredicateMismatch {
            expected: slot.predicate_id,
            actual: predicate.predicate_id(),
        });
    }
    if predicate.resolution_policy() != ResolutionPolicy::SingleValueReplace {
        return Err(SingleValueResolutionError::UnsupportedPolicy {
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
        return Ok(SingleValueOutcome::Unknown);
    }
    let first_candidate = matching
        .first()
        .copied()
        .ok_or(SingleValueResolutionError::EmptyPreferredSet)?;
    let first_context = first_candidate.assertion().context();
    let query_history_space_id = first_candidate.query_history_space_id;
    let selected_layer_ids = first_candidate.selected_layer_ids();
    let mut candidate_ids = BTreeSet::new();
    for candidate in &matching {
        let assertion_id = candidate.assertion().id();
        if !candidate_ids.insert(assertion_id) {
            return Err(SingleValueResolutionError::DuplicateCandidate { assertion_id });
        }
        let context = candidate.assertion().context();
        if candidate.query_history_space_id != query_history_space_id
            || context.perspective_scope() != first_context.perspective_scope()
            || context.epistemic_mode() != first_context.epistemic_mode()
            || candidate.selected_layer_ids() != selected_layer_ids
        {
            return Err(SingleValueResolutionError::CandidateScopeMismatch { assertion_id });
        }
    }
    let preferred = matching
        .iter()
        .map(|candidate| candidate.precedence())
        .max()
        .ok_or(SingleValueResolutionError::EmptyPreferredSet)?;
    matching.retain(|candidate| candidate.precedence() == preferred);
    matching.sort_by_key(|candidate| candidate.assertion().id());

    for candidate in &matching {
        let (_, _, value, _) = candidate.assertion().proposition_components();
        let actual = ValueKind::of(value);
        if actual != predicate.value_kind() {
            return Err(SingleValueResolutionError::SchemaValueKindMismatch {
                assertion_id: candidate.assertion().id(),
                expected: predicate.value_kind(),
                actual,
            });
        }
    }

    let first = matching
        .first()
        .ok_or(SingleValueResolutionError::EmptyPreferredSet)?
        .assertion();
    let (first_subject, first_predicate, first_value, first_polarity) =
        first.proposition_components();
    let mut agrees = true;
    for candidate in matching.iter().skip(1) {
        let (subject, predicate_id, value, polarity) =
            candidate.assertion().proposition_components();
        let equal = if first_subject != subject
            || first_predicate != predicate_id
            || first_polarity != polarity
        {
            false
        } else {
            match canonical_value_equality(first_value, value) {
                Some(equal) => equal,
                None => temporal_value_equal(first_value, value)
                    .map_err(|()| SingleValueResolutionError::TemporalValueComparisonUnavailable)?,
            }
        };
        if !equal {
            agrees = false;
        }
    }
    let contributors = matching
        .iter()
        .map(|candidate| candidate.assertion().id())
        .collect::<Vec<_>>();
    if !agrees {
        return Ok(SingleValueOutcome::Conflict { contributors });
    }
    Ok(SingleValueOutcome::Known {
        value: first_value.clone(),
        polarity: first_polarity,
        contributors,
    })
}

/// Invalid policy binding or unavailable schema-aware proposition comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleValueResolutionError {
    /// The selected schema definition has a different Predicate identity.
    SchemaPredicateMismatch {
        expected: PredicateId,
        actual: PredicateId,
    },
    /// The selected schema does not use `SingleValueReplace`.
    UnsupportedPolicy { actual: ResolutionPolicy },
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
    /// No candidate remained after the selector unexpectedly entered the preferred-set path.
    EmptyPreferredSet,
    /// Temporal Value equality requires schema resolution which the caller could not provide.
    TemporalValueComparisonUnavailable,
}

impl fmt::Display for SingleValueResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaPredicateMismatch { expected, actual } => write!(
                formatter,
                "slot Predicate {expected} does not match schema Predicate {actual}"
            ),
            Self::UnsupportedPolicy { actual } => write!(
                formatter,
                "single-value resolution requires SingleValueReplace, got {actual:?}"
            ),
            Self::DuplicateCandidate { assertion_id } => write!(
                formatter,
                "candidate Assertion {assertion_id} appears more than once"
            ),
            Self::CandidateScopeMismatch { assertion_id } => write!(
                formatter,
                "candidate Assertion {assertion_id} belongs to another query scope"
            ),
            Self::SchemaValueKindMismatch {
                assertion_id,
                expected,
                actual,
            } => write!(
                formatter,
                "assertion {assertion_id} has ValueKind {actual:?}; pinned Predicate schema requires {expected:?}"
            ),
            Self::EmptyPreferredSet => {
                formatter.write_str("preferred candidate set is unexpectedly empty")
            }
            Self::TemporalValueComparisonUnavailable => {
                formatter.write_str("temporal proposition equality requires the pinned schema")
            }
        }
    }
}

impl std::error::Error for SingleValueResolutionError {}

#[cfg(test)]
mod tests {
    use super::{
        SingleValueOutcome, SingleValueResolutionError, SingleValueSlot,
        resolve_single_value_replace,
    };
    use crate::assertions::{Assertion, AssertionDraft, Polarity, Subject};
    use crate::candidate_scan::AssertionCandidate;
    use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition};
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::context_precedence::{ContextPrecedence, ContextPrecedenceError};
    use crate::ids::{
        AssertionId, DomainId, EntityId, HistorySpaceId, IdValidationError, LayerId, MaskId,
        PredicateId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
        ReplacementBoundaryValidityClosureId, Revision, RevisionError, SchemaRevision, TimelineId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaError, LayerSchemaSnapshot};
    use crate::mask_projection::{AssertionMaskContext, apply_assertion_masks};
    use crate::masks::{
        Mask, MaskSelector, ReplacementBoundary, ReplacementBoundaryRetraction,
        ReplacementBoundaryValidityClosure,
    };
    use crate::multi_value_resolution::{
        MultiValueOutcome, MultiValueReplaceContext, MultiValueResolutionError, MultiValueSlot,
        ReplacementBoundaryHistory, resolve_multi_value_overlay, resolve_multi_value_replace,
    };
    use crate::schema::{
        Cardinality, ConstraintSet, EntityTypeConstraint, Lifecycle, PredicateDefinition,
        PredicateDefinitionSpec, ResolutionPolicy, SchemaDefinitionError, ValueKind,
    };
    use crate::temporal::{
        AssertionValidity, RecordedAsOf, TemporalError, TimeInterval, Timeline, WorldTime,
    };
    use crate::values::{SymbolError, Value};
    use std::collections::BTreeSet;
    use std::error::Error;
    use std::fmt;

    type TestResult<T = ()> = Result<T, TestError>;
    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(RevisionError),
        Context(crate::context::ContextError),
        History(crate::catalog::HistorySpaceError),
        Layer(LayerSchemaError),
        Precedence(ContextPrecedenceError),
        Mask(crate::masks::MaskRecordError),
        MaskProjection(crate::mask_projection::MaskProjectionError),
        Schema(SchemaDefinitionError),
        Symbol(SymbolError),
        Temporal(TemporalError),
        Resolution(SingleValueResolutionError),
        MultiValue(MultiValueResolutionError),
        Boundary(crate::masks::ReplacementBoundaryError),
    }
    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(e) => write!(f, "{e}"),
                Self::Revision(e) => write!(f, "{e}"),
                Self::Context(e) => write!(f, "{e}"),
                Self::History(e) => write!(f, "{e}"),
                Self::Layer(e) => write!(f, "{e}"),
                Self::Precedence(e) => write!(f, "{e}"),
                Self::Mask(e) => write!(f, "{e}"),
                Self::MaskProjection(e) => write!(f, "{e}"),
                Self::Schema(e) => write!(f, "{e}"),
                Self::Symbol(e) => write!(f, "{e}"),
                Self::Temporal(e) => write!(f, "{e}"),
                Self::Resolution(e) => write!(f, "{e}"),
                Self::MultiValue(e) => write!(f, "{e}"),
                Self::Boundary(e) => write!(f, "{e}"),
            }
        }
    }
    impl Error for TestError {}
    macro_rules! convert {
        ($ty:ty,$var:ident) => {
            impl From<$ty> for TestError {
                fn from(e: $ty) -> Self {
                    Self::$var(e)
                }
            }
        };
    }
    convert!(IdValidationError, Id);
    convert!(RevisionError, Revision);
    convert!(crate::context::ContextError, Context);
    convert!(crate::catalog::HistorySpaceError, History);
    convert!(LayerSchemaError, Layer);
    convert!(ContextPrecedenceError, Precedence);
    convert!(crate::masks::MaskRecordError, Mask);
    convert!(crate::mask_projection::MaskProjectionError, MaskProjection);
    convert!(SchemaDefinitionError, Schema);
    convert!(SymbolError, Symbol);
    convert!(TemporalError, Temporal);
    convert!(SingleValueResolutionError, Resolution);
    convert!(MultiValueResolutionError, MultiValue);
    convert!(crate::masks::ReplacementBoundaryError, Boundary);
    fn id<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut b = [0_u8; 16];
        b[6] = 0x70;
        b[8] = 0x80;
        b[15] = tail;
        T::try_from_bytes(b)
    }
    fn rev(v: u64) -> Result<Revision, RevisionError> {
        Revision::new(v)
    }

    struct Fixture {
        candidate: AssertionCandidate,
        slot: SingleValueSlot,
        schema: PredicateDefinition,
        timeline: Timeline,
        catalog: HistorySpaceCatalog,
        layers: LayerSchemaSnapshot,
        child: HistorySpaceId,
        root: HistorySpaceId,
        layer: LayerId,
    }
    fn fixture() -> TestResult<Fixture> {
        let root = id::<HistorySpaceId>(1)?;
        let child = id::<HistorySpaceId>(2)?;
        let layer = id::<LayerId>(3)?;
        let predicate_id = id::<PredicateId>(4)?;
        let timeline = Timeline::new(id::<TimelineId>(5)?);
        let subject = Subject::new(id::<EntityId>(6)?);
        let catalog = HistorySpaceCatalog::new(vec![
            HistorySpaceDefinition::new(root, None, Revision::GENESIS)?,
            HistorySpaceDefinition::new(child, Some(root), rev(1)?)?,
        ])?;
        let schema_revision = SchemaRevision::from_published_revision(rev(1)?);
        let layers = LayerSchemaSnapshot::new(
            schema_revision,
            vec![LayerDefinition::new(
                layer,
                crate::Symbol::new("base")?,
                None,
                0,
                Lifecycle::Active,
                schema_revision,
            )],
            layer,
        )?;
        let schema = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol: crate::Symbol::new("answer")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints: ConstraintSet::new(vec![])?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: rev(1)?,
        })?;
        let slot = SingleValueSlot::new(subject, predicate_id);
        let context = ContextKey::new(
            root,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let validity = AssertionValidity::new(TimeInterval::new(timeline, None, None)?);
        let assertion = Assertion::new(
            id::<AssertionId>(7)?,
            AssertionDraft::new(
                context,
                subject,
                predicate_id,
                Value::String(String::from("parent")),
                Polarity::Positive,
                validity,
            ),
            rev(2)?,
        );
        let precedence = ContextPrecedence::for_context(child, root, layer, &catalog, &layers)?;
        let candidate = AssertionCandidate {
            assertion,
            source_history_space_id: root,
            query_history_space_id: child,
            selected_layer_ids: [layer].into_iter().collect(),
            precedence,
        };
        Ok(Fixture {
            candidate,
            slot,
            schema,
            timeline,
            catalog,
            layers,
            child,
            root,
            layer,
        })
    }
    fn add_candidate(
        f: &Fixture,
        tail: u8,
        source: HistorySpaceId,
        value: &str,
        polarity: Polarity,
    ) -> TestResult<AssertionCandidate> {
        let assertion = Assertion::new(
            id::<AssertionId>(tail)?,
            AssertionDraft::new(
                ContextKey::new(
                    source,
                    f.layer,
                    PerspectiveScope::World,
                    EpistemicMode::WorldState,
                )?,
                f.slot.subject(),
                f.slot.predicate_id(),
                Value::String(value.to_owned()),
                polarity,
                AssertionValidity::new(TimeInterval::new(f.timeline, None, None)?),
            ),
            rev(3)?,
        );
        let precedence =
            ContextPrecedence::for_context(f.child, source, f.layer, &f.catalog, &f.layers)?;
        Ok(AssertionCandidate {
            assertion,
            source_history_space_id: source,
            query_history_space_id: f.child,
            selected_layer_ids: [f.layer].into_iter().collect(),
            precedence,
        })
    }

    fn add_time_candidate(
        f: &Fixture,
        tail: u8,
        value: crate::Time,
    ) -> TestResult<AssertionCandidate> {
        let assertion = Assertion::new(
            id::<AssertionId>(tail)?,
            AssertionDraft::new(
                ContextKey::new(
                    f.root,
                    f.layer,
                    PerspectiveScope::World,
                    EpistemicMode::WorldState,
                )?,
                f.slot.subject(),
                f.slot.predicate_id(),
                Value::Time(value),
                Polarity::Positive,
                AssertionValidity::new(TimeInterval::new(f.timeline, None, None)?),
            ),
            rev(3)?,
        );
        let precedence =
            ContextPrecedence::for_context(f.child, f.root, f.layer, &f.catalog, &f.layers)?;
        Ok(AssertionCandidate {
            assertion,
            source_history_space_id: f.root,
            query_history_space_id: f.child,
            selected_layer_ids: [f.layer].into_iter().collect(),
            precedence,
        })
    }

    fn multi_overlay_schema(f: &Fixture, value_kind: ValueKind) -> TestResult<PredicateDefinition> {
        Ok(PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: f.slot.predicate_id(),
            symbol: crate::Symbol::new("multi_answer")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy: ResolutionPolicy::MultiValueOverlay,
            constraints: ConstraintSet::new(vec![])?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: rev(1)?,
        })?)
    }

    fn multi_replace_schema(f: &Fixture) -> TestResult<PredicateDefinition> {
        Ok(PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: f.slot.predicate_id(),
            symbol: crate::Symbol::new("replace_answer")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy: ResolutionPolicy::MultiValueReplace,
            constraints: ConstraintSet::new(vec![])?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: rev(1)?,
        })?)
    }

    fn replacement_context(f: &Fixture) -> TestResult<MultiValueReplaceContext<'_>> {
        Ok(MultiValueReplaceContext::new(
            ContextKey::new(
                f.child,
                f.layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            [f.layer].into_iter().collect(),
            RecordedAsOf::from_published_revision(rev(4)?),
            WorldTime::from_nanoseconds(f.timeline, 10),
            &f.catalog,
            &f.layers,
        ))
    }

    fn add_boundary(
        f: &Fixture,
        tail: u8,
        source: HistorySpaceId,
        predicate: &PredicateDefinition,
    ) -> TestResult<ReplacementBoundary> {
        Ok(ReplacementBoundary::new(
            id::<ReplacementBoundaryId>(tail)?,
            ContextKey::new(
                source,
                f.layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            f.slot.subject(),
            predicate,
            None,
            rev(3)?,
        )?)
    }
    #[test]
    fn no_candidates_returns_unknown() -> TestResult {
        let f = fixture()?;
        assert!(matches!(
            resolve_single_value_replace(&[], f.slot, &f.schema, |_, _| Ok(false))?,
            SingleValueOutcome::Unknown
        ));
        Ok(())
    }

    #[test]
    fn masking_every_candidate_resolves_to_unknown_not_a_negative_fact() -> TestResult {
        let f = fixture()?;
        let mask = Mask::new(
            id::<MaskId>(20)?,
            ContextKey::new(
                f.child,
                f.layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            MaskSelector::ExactAssertion(f.candidate.assertion().id()),
            None,
            rev(3)?,
        )?;
        let masked = apply_assertion_masks(
            std::slice::from_ref(&f.candidate),
            &[mask.clone()],
            &[],
            &[],
            &[mask.id()].into_iter().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 10),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        assert!(matches!(
            resolve_single_value_replace(&masked, f.slot, &f.schema, |_, _| Ok(false))?,
            SingleValueOutcome::Unknown
        ));
        Ok(())
    }

    #[test]
    fn duplicate_or_mixed_query_candidates_fail_closed() -> TestResult {
        let f = fixture()?;
        assert!(matches!(
            resolve_single_value_replace(
                &[f.candidate.clone(), f.candidate.clone()],
                f.slot,
                &f.schema,
                |_, _| Ok(false),
            ),
            Err(SingleValueResolutionError::DuplicateCandidate { .. })
        ));
        let mut other_scope = add_candidate(&f, 14, f.root, "peer", Polarity::Positive)?;
        other_scope.query_history_space_id = f.root;
        assert!(matches!(
            resolve_single_value_replace(
                &[f.candidate.clone(), other_scope],
                f.slot,
                &f.schema,
                |_, _| Ok(false),
            ),
            Err(SingleValueResolutionError::CandidateScopeMismatch { .. })
        ));
        Ok(())
    }
    #[test]
    fn local_precedence_replaces_parent_without_revision_ordering() -> TestResult {
        let f = fixture()?;
        let local = add_candidate(&f, 8, f.child, "local", Polarity::Positive)?;
        let result = resolve_single_value_replace(
            &[f.candidate.clone(), local],
            f.slot,
            &f.schema,
            |_, _| Ok(false),
        )?;
        assert!(
            matches!(result,SingleValueOutcome::Known{value:Value::String(value),polarity:Polarity::Positive,contributors} if value=="local"&&contributors==vec![id::<AssertionId>(8)?])
        );
        Ok(())
    }
    #[test]
    fn equal_precedence_disagreement_is_conflict_and_input_order_independent() -> TestResult {
        let f = fixture()?;
        let peer = add_candidate(&f, 9, f.root, "other", Polarity::Positive)?;
        for input in [
            vec![f.candidate.clone(), peer.clone()],
            vec![peer, f.candidate.clone()],
        ] {
            assert!(
                matches!(resolve_single_value_replace(&input,f.slot,&f.schema,|_,_|Ok(false))?,SingleValueOutcome::Conflict{contributors} if contributors==vec![id::<AssertionId>(7)?,id::<AssertionId>(9)?])
            );
        }
        Ok(())
    }

    #[test]
    fn equal_precedence_resolution_is_stable_across_every_three_candidate_permutation() -> TestResult
    {
        let f = fixture()?;
        let second = add_candidate(&f, 9, f.root, "other", Polarity::Positive)?;
        let third = add_candidate(&f, 16, f.root, "third", Polarity::Positive)?;
        let permutations = [
            vec![f.candidate.clone(), second.clone(), third.clone()],
            vec![f.candidate.clone(), third.clone(), second.clone()],
            vec![second.clone(), f.candidate.clone(), third.clone()],
            vec![second.clone(), third.clone(), f.candidate.clone()],
            vec![third.clone(), f.candidate.clone(), second.clone()],
            vec![third, second, f.candidate.clone()],
        ];
        let expected = vec![
            id::<AssertionId>(7)?,
            id::<AssertionId>(9)?,
            id::<AssertionId>(16)?,
        ];

        for permutation in permutations {
            assert!(matches!(
                resolve_single_value_replace(
                    &permutation,
                    f.slot,
                    &f.schema,
                    |_, _| Ok(false),
                )?,
                SingleValueOutcome::Conflict { contributors } if contributors == expected
            ));
        }
        Ok(())
    }
    #[test]
    fn identical_top_precedence_values_keep_all_stable_contributors() -> TestResult {
        let f = fixture()?;
        let peer = add_candidate(&f, 10, f.root, "parent", Polarity::Positive)?;
        assert!(
            matches!(resolve_single_value_replace(&[peer,f.candidate.clone()],f.slot,&f.schema,|_,_|Ok(false))?,SingleValueOutcome::Known{value:Value::String(value),contributors,..} if value=="parent"&&contributors==vec![id::<AssertionId>(7)?,id::<AssertionId>(10)?])
        );
        Ok(())
    }
    #[test]
    fn polarity_disagreement_is_conflict_and_non_single_policy_is_rejected() -> TestResult {
        let f = fixture()?;
        let negative = add_candidate(&f, 11, f.root, "parent", Polarity::Negative)?;
        assert!(matches!(
            resolve_single_value_replace(
                &[f.candidate.clone(), negative],
                f.slot,
                &f.schema,
                |_, _| Ok(false)
            )?,
            SingleValueOutcome::Conflict { .. }
        ));
        let bad = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: f.slot.predicate_id(),
            symbol: crate::Symbol::new("multi")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality: Cardinality::Multi,
            resolution_policy: ResolutionPolicy::MultiValueOverlay,
            constraints: ConstraintSet::new(vec![])?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: rev(1)?,
        })?;
        assert!(matches!(
            resolve_single_value_replace(
                std::slice::from_ref(&f.candidate),
                f.slot,
                &bad,
                |_, _| Ok(false)
            ),
            Err(SingleValueResolutionError::UnsupportedPolicy {
                actual: ResolutionPolicy::MultiValueOverlay
            })
        ));
        Ok(())
    }

    fn single_outcome_contributors(outcome: &SingleValueOutcome) -> BTreeSet<AssertionId> {
        match outcome {
            SingleValueOutcome::Known { contributors, .. }
            | SingleValueOutcome::Conflict { contributors } => {
                contributors.iter().copied().collect()
            }
            SingleValueOutcome::Unknown => BTreeSet::new(),
        }
    }

    fn multi_outcome_contributors(outcome: &MultiValueOutcome) -> BTreeSet<AssertionId> {
        let mut contributors = BTreeSet::new();
        match outcome {
            MultiValueOutcome::Known { values } | MultiValueOutcome::Conflict { values, .. } => {
                for value in values {
                    contributors.extend(value.contributors().iter().copied());
                }
                if let MultiValueOutcome::Conflict { conflicts, .. } = outcome {
                    for conflict in conflicts {
                        contributors.extend(conflict.positive_contributors().iter().copied());
                        contributors.extend(conflict.negative_contributors().iter().copied());
                    }
                }
            }
            MultiValueOutcome::Unknown => {}
        }
        contributors
    }

    fn schema_for_policy(
        fixture: &Fixture,
        policy: ResolutionPolicy,
    ) -> TestResult<PredicateDefinition> {
        match policy {
            ResolutionPolicy::SingleValueReplace => Ok(fixture.schema.clone()),
            ResolutionPolicy::MultiValueOverlay => multi_overlay_schema(fixture, ValueKind::String),
            ResolutionPolicy::MultiValueReplace => multi_replace_schema(fixture),
        }
    }

    #[test]
    fn combined_resolution_security_truth_table_has_no_hidden_contributors() -> TestResult {
        let policies = [
            ResolutionPolicy::SingleValueReplace,
            ResolutionPolicy::MultiValueOverlay,
            ResolutionPolicy::MultiValueReplace,
        ];
        let mut rows = 0_usize;
        for policy in policies {
            for masked in [false, true] {
                for local_precedence in [false, true] {
                    for negative_peer in [false, true] {
                        for boundary_active in [false, true] {
                            for hide_parent in [false, true] {
                                let fixture = fixture()?;
                                let schema = schema_for_policy(&fixture, policy)?;
                                let parent = add_candidate(
                                    &fixture,
                                    40,
                                    fixture.root,
                                    "parent",
                                    Polarity::Positive,
                                )?;
                                let peer = add_candidate(
                                    &fixture,
                                    41,
                                    if local_precedence {
                                        fixture.child
                                    } else {
                                        fixture.root
                                    },
                                    if local_precedence { "local" } else { "parent" },
                                    if negative_peer {
                                        Polarity::Negative
                                    } else {
                                        Polarity::Positive
                                    },
                                )?;
                                let parent_id = parent.assertion().id();
                                let peer_id = peer.assertion().id();
                                let mut candidates = vec![parent.clone(), peer.clone()];

                                // Security projection precedes masks and resolution.
                                if hide_parent {
                                    candidates.retain(|item| item.assertion().id() != parent_id);
                                }
                                if masked {
                                    let mask = Mask::new(
                                        id::<MaskId>(42)?,
                                        ContextKey::new(
                                            fixture.child,
                                            fixture.layer,
                                            PerspectiveScope::World,
                                            EpistemicMode::WorldState,
                                        )?,
                                        MaskSelector::ExactAssertion(parent_id),
                                        None,
                                        rev(3)?,
                                    )?;
                                    candidates = apply_assertion_masks(
                                        &candidates,
                                        &[mask.clone()],
                                        &[],
                                        &[],
                                        &[mask.id()].into_iter().collect(),
                                        AssertionMaskContext::new(
                                            RecordedAsOf::from_published_revision(rev(4)?),
                                            WorldTime::from_nanoseconds(fixture.timeline, 10),
                                            &fixture.catalog,
                                            &fixture.layers,
                                        ),
                                        |left, right| {
                                            Ok(crate::values::canonical_value_equality(left, right)
                                                .is_some_and(|equal| equal))
                                        },
                                    )?;
                                }

                                let mut expected = BTreeSet::new();
                                if !hide_parent && !masked {
                                    expected.insert(parent_id);
                                }
                                expected.insert(peer_id);
                                if policy == ResolutionPolicy::SingleValueReplace
                                    && local_precedence
                                {
                                    expected.retain(|id| *id == peer_id);
                                }
                                if policy == ResolutionPolicy::MultiValueReplace && boundary_active
                                {
                                    if local_precedence {
                                        expected.retain(|id| *id == peer_id);
                                    } else {
                                        expected.clear();
                                    }
                                }

                                let (outcome_contributors, actual_conflict) = match policy {
                                    ResolutionPolicy::SingleValueReplace => {
                                        let outcome = resolve_single_value_replace(
                                            &candidates,
                                            fixture.slot,
                                            &schema,
                                            |left, right| {
                                                Ok(crate::values::canonical_value_equality(
                                                    left, right,
                                                )
                                                .is_some_and(|equal| equal))
                                            },
                                        )?;
                                        assert!(
                                            !local_precedence
                                                || !matches!(
                                                    &outcome,
                                                    SingleValueOutcome::Conflict { .. }
                                                )
                                        );
                                        let is_conflict =
                                            matches!(&outcome, SingleValueOutcome::Conflict { .. });
                                        (single_outcome_contributors(&outcome), is_conflict)
                                    }
                                    ResolutionPolicy::MultiValueOverlay => {
                                        let outcome = resolve_multi_value_overlay(
                                            &candidates,
                                            MultiValueSlot::new(
                                                fixture.slot.subject(),
                                                fixture.slot.predicate_id(),
                                            ),
                                            &schema,
                                            |left, right| {
                                                Ok(crate::values::canonical_value_equality(
                                                    left, right,
                                                )
                                                .is_some_and(|equal| equal))
                                            },
                                        )?;
                                        let is_conflict = matches!(
                                            &outcome,
                                            MultiValueOutcome::Conflict { conflicts, .. }
                                                if !conflicts.is_empty()
                                        );
                                        (multi_outcome_contributors(&outcome), is_conflict)
                                    }
                                    ResolutionPolicy::MultiValueReplace => {
                                        let boundaries = if boundary_active {
                                            vec![add_boundary(
                                                &fixture,
                                                43,
                                                fixture.child,
                                                &schema,
                                            )?]
                                        } else {
                                            Vec::new()
                                        };
                                        let visible_boundaries = boundaries
                                            .iter()
                                            .map(ReplacementBoundary::id)
                                            .collect();
                                        let result = resolve_multi_value_replace(
                                            &candidates,
                                            ReplacementBoundaryHistory::new(
                                                &boundaries,
                                                &[],
                                                &[],
                                                &visible_boundaries,
                                            ),
                                            MultiValueSlot::new(
                                                fixture.slot.subject(),
                                                fixture.slot.predicate_id(),
                                            ),
                                            &schema,
                                            replacement_context(&fixture)?,
                                            |left, right| {
                                                Ok(crate::values::canonical_value_equality(
                                                    left, right,
                                                )
                                                .is_some_and(|equal| equal))
                                            },
                                        )?;
                                        let is_conflict = matches!(
                                            &result,
                                            MultiValueOutcome::Conflict { conflicts, .. }
                                                if !conflicts.is_empty()
                                        );
                                        (multi_outcome_contributors(&result), is_conflict)
                                    }
                                };
                                assert_eq!(outcome_contributors, expected);
                                let expected_conflict = !local_precedence
                                    && !masked
                                    && !hide_parent
                                    && negative_peer
                                    && (policy != ResolutionPolicy::MultiValueReplace
                                        || !boundary_active);
                                assert_eq!(actual_conflict, expected_conflict);
                                rows += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(rows, 96);
        Ok(())
    }

    #[test]
    fn candidate_value_kind_must_match_the_pinned_predicate() -> TestResult {
        let f = fixture()?;
        let schema = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: f.slot.predicate_id(),
            symbol: crate::Symbol::new("integer_answer")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::Int,
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints: ConstraintSet::new(vec![])?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: rev(1)?,
        })?;
        assert!(matches!(
            resolve_single_value_replace(
                std::slice::from_ref(&f.candidate),
                f.slot,
                &schema,
                |_, _| Ok(false),
            ),
            Err(SingleValueResolutionError::SchemaValueKindMismatch {
                expected: ValueKind::Int,
                actual: ValueKind::String,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn temporal_values_use_the_selected_schema_comparator() -> TestResult {
        let f = fixture()?;
        let seconds = add_time_candidate(
            &f,
            12,
            crate::Time::new(f.timeline.id(), 1, crate::Symbol::new("s")?),
        )?;
        let milliseconds = add_time_candidate(
            &f,
            13,
            crate::Time::new(f.timeline.id(), 1_000, crate::Symbol::new("ms")?),
        )?;
        let schema = PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id: f.slot.predicate_id(),
            symbol: crate::Symbol::new("time_value")?,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::Time,
            object_constraint: None,
            cardinality: Cardinality::Single,
            resolution_policy: ResolutionPolicy::SingleValueReplace,
            constraints: ConstraintSet::new(vec![])?,
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision: rev(1)?,
        })?;
        let equal = |left: &Value, right: &Value| {
            Ok(matches!((left, right), (Value::Time(a), Value::Time(b))
                if a.timeline_id() == b.timeline_id()
                    && ((a.unit().as_str() == "s" && b.unit().as_str() == "ms" && a.ticks().checked_mul(1_000) == Some(b.ticks()))
                        || (a.unit().as_str() == "ms" && b.unit().as_str() == "s" && b.ticks().checked_mul(1_000) == Some(a.ticks())))))
        };
        assert!(matches!(
            resolve_single_value_replace(
                &[seconds.clone(), milliseconds.clone()],
                f.slot,
                &schema,
                equal,
            )?,
            SingleValueOutcome::Known { contributors, .. }
                if contributors == vec![id::<AssertionId>(12)?, id::<AssertionId>(13)?]
        ));
        assert!(matches!(
            resolve_single_value_replace(&[seconds, milliseconds], f.slot, &schema, |_, _| Err(()),),
            Err(SingleValueResolutionError::TemporalValueComparisonUnavailable)
        ));
        Ok(())
    }

    #[test]
    fn multi_overlay_unions_parent_and_local_values_without_precedence_replacement() -> TestResult {
        let f = fixture()?;
        let local = add_candidate(&f, 15, f.child, "local", Polarity::Positive)?;
        let schema = multi_overlay_schema(&f, ValueKind::String)?;
        let outcome = resolve_multi_value_overlay(
            &[local, f.candidate.clone()],
            MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id()),
            &schema,
            |_, _| Ok(false),
        )?;
        assert!(matches!(outcome, MultiValueOutcome::Known { values }
            if values.len() == 2
                && values.iter().any(|entry| matches!(entry.value(), Value::String(value) if value == "parent"))
                && values.iter().any(|entry| matches!(entry.value(), Value::String(value) if value == "local"))));
        Ok(())
    }

    #[test]
    fn multi_overlay_coalesces_equal_values_and_keeps_all_contributors_in_id_order() -> TestResult {
        let f = fixture()?;
        let duplicate = add_candidate(&f, 16, f.root, "parent", Polarity::Positive)?;
        let schema = multi_overlay_schema(&f, ValueKind::String)?;
        let outcome = resolve_multi_value_overlay(
            &[duplicate, f.candidate.clone()],
            MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id()),
            &schema,
            |_, _| Ok(false),
        )?;
        let expected = [id::<AssertionId>(7)?, id::<AssertionId>(16)?];
        assert!(matches!(outcome, MultiValueOutcome::Known { values }
            if values.len() == 1
                && values.first().is_some_and(|entry| entry.contributors() == expected)));
        Ok(())
    }

    #[test]
    fn multi_overlay_preserves_known_values_while_reporting_polarity_conflicts() -> TestResult {
        let f = fixture()?;
        let negative = add_candidate(&f, 17, f.child, "parent", Polarity::Negative)?;
        let other = add_candidate(&f, 18, f.root, "other", Polarity::Positive)?;
        let schema = multi_overlay_schema(&f, ValueKind::String)?;
        let outcome = resolve_multi_value_overlay(
            &[other, negative, f.candidate.clone()],
            MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id()),
            &schema,
            |_, _| Ok(false),
        )?;
        let expected_positive = [id::<AssertionId>(7)?];
        let expected_negative = [id::<AssertionId>(17)?];
        assert!(
            matches!(outcome, MultiValueOutcome::Conflict { values, conflicts }
            if values.len() == 1
                && values.first().is_some_and(|entry| matches!(entry.value(), Value::String(value) if value == "other"))
                && conflicts.len() == 1
                && conflicts.first().is_some_and(|entry| matches!(entry.value(), Value::String(value) if value == "parent")
                    && entry.positive_contributors() == expected_positive
                    && entry.negative_contributors() == expected_negative))
        );
        Ok(())
    }

    #[test]
    fn multi_overlay_uses_post_mask_candidates_and_empty_set_is_unknown() -> TestResult {
        let f = fixture()?;
        let mask = Mask::new(
            id::<MaskId>(21)?,
            ContextKey::new(
                f.child,
                f.layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            MaskSelector::ExactAssertion(f.candidate.assertion().id()),
            None,
            rev(3)?,
        )?;
        let masked = apply_assertion_masks(
            std::slice::from_ref(&f.candidate),
            &[mask.clone()],
            &[],
            &[],
            &[mask.id()].into_iter().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 10),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        let schema = multi_overlay_schema(&f, ValueKind::String)?;
        assert!(matches!(
            resolve_multi_value_overlay(
                &masked,
                MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id()),
                &schema,
                |_, _| Ok(false),
            )?,
            MultiValueOutcome::Unknown
        ));
        Ok(())
    }

    #[test]
    fn multi_overlay_is_input_order_independent_and_temporal_comparison_fails_closed() -> TestResult
    {
        let f = fixture()?;
        let first = add_time_candidate(
            &f,
            22,
            crate::Time::new(f.timeline.id(), 1, crate::Symbol::new("s")?),
        )?;
        let second = add_time_candidate(
            &f,
            23,
            crate::Time::new(f.timeline.id(), 1_000, crate::Symbol::new("ms")?),
        )?;
        let schema = multi_overlay_schema(&f, ValueKind::Time)?;
        let slot = MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id());
        assert!(matches!(
            resolve_multi_value_overlay(&[first.clone(), second.clone()], slot, &schema, |_, _| {
                Err(())
            }),
            Err(MultiValueResolutionError::TemporalValueComparisonUnavailable)
        ));
        let equal = |left: &Value, right: &Value| {
            Ok(matches!((left, right), (Value::Time(a), Value::Time(b))
                if a.timeline_id() == b.timeline_id()
                    && ((a.unit().as_str() == "s" && b.unit().as_str() == "ms" && a.ticks().checked_mul(1_000) == Some(b.ticks()))
                        || (a.unit().as_str() == "ms" && b.unit().as_str() == "s" && b.ticks().checked_mul(1_000) == Some(a.ticks())))))
        };
        let expected = [id::<AssertionId>(22)?, id::<AssertionId>(23)?];
        for candidates in [vec![first.clone(), second.clone()], vec![second, first]] {
            assert!(
                matches!(resolve_multi_value_overlay(&candidates, slot, &schema, equal)?,
                MultiValueOutcome::Known { values } if values.len() == 1
                    && values.first().is_some_and(|entry| entry.contributors() == expected))
            );
        }
        Ok(())
    }

    #[test]
    fn multi_replace_distinguishes_unknown_from_explicit_known_empty_set() -> TestResult {
        let f = fixture()?;
        let schema = multi_replace_schema(&f)?;
        let slot = MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id());
        assert!(matches!(
            resolve_multi_value_replace(
                &[],
                ReplacementBoundaryHistory::new(&[], &[], &[], &BTreeSet::new()),
                slot,
                &schema,
                replacement_context(&f)?,
                |_, _| Ok(false),
            )?,
            MultiValueOutcome::Unknown
        ));
        let boundary = add_boundary(&f, 30, f.child, &schema)?;
        assert!(matches!(
            resolve_multi_value_replace(
                &[], ReplacementBoundaryHistory::new(
                    std::slice::from_ref(&boundary), &[], &[], &[boundary.id()].into_iter().collect(),
                ), slot, &schema,
                replacement_context(&f)?, |_, _| Ok(false),
            )?,
            MultiValueOutcome::Known { values } if values.is_empty()
        ));
        Ok(())
    }

    #[test]
    fn multi_replace_boundary_cuts_off_lower_context_but_keeps_equal_precedence_assertions()
    -> TestResult {
        let f = fixture()?;
        let schema = multi_replace_schema(&f)?;
        let slot = MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id());
        let child_boundary = add_boundary(&f, 31, f.child, &schema)?;
        let local = add_candidate(&f, 32, f.child, "local", Polarity::Positive)?;
        let result = resolve_multi_value_replace(
            &[f.candidate.clone(), local],
            ReplacementBoundaryHistory::new(
                std::slice::from_ref(&child_boundary),
                &[],
                &[],
                &[child_boundary.id()].into_iter().collect(),
            ),
            slot,
            &schema,
            replacement_context(&f)?,
            |_, _| Ok(false),
        )?;
        assert!(matches!(result, MultiValueOutcome::Known { values }
            if values.len() == 1 && values.first().is_some_and(|value|
                matches!(value.value(), Value::String(text) if text == "local"))));
        Ok(())
    }

    #[test]
    fn higher_precedence_assertions_survive_an_ancestor_boundary() -> TestResult {
        let f = fixture()?;
        let schema = multi_replace_schema(&f)?;
        let slot = MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id());
        let parent_boundary = add_boundary(&f, 37, f.root, &schema)?;
        let local = add_candidate(&f, 38, f.child, "local", Polarity::Positive)?;
        let result = resolve_multi_value_replace(
            &[f.candidate.clone(), local],
            ReplacementBoundaryHistory::new(
                std::slice::from_ref(&parent_boundary),
                &[],
                &[],
                &[parent_boundary.id()].into_iter().collect(),
            ),
            slot,
            &schema,
            replacement_context(&f)?,
            |_, _| Ok(false),
        )?;
        assert!(matches!(result, MultiValueOutcome::Known { values }
            if values.len() == 2
                && values.iter().any(|value| matches!(value.value(), Value::String(text) if text == "parent"))
                && values.iter().any(|value| matches!(value.value(), Value::String(text) if text == "local"))));
        Ok(())
    }

    #[test]
    fn closed_or_retracted_boundary_no_longer_replaces_parent_values() -> TestResult {
        let f = fixture()?;
        let schema = multi_replace_schema(&f)?;
        let slot = MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id());
        let boundary = add_boundary(&f, 33, f.child, &schema)?;
        let closure = ReplacementBoundaryValidityClosure::new(
            id::<ReplacementBoundaryValidityClosureId>(34)?,
            &boundary,
            WorldTime::from_nanoseconds(f.timeline, 10),
            rev(4)?,
        )?;
        let closed = resolve_multi_value_replace(
            std::slice::from_ref(&f.candidate),
            ReplacementBoundaryHistory::new(
                std::slice::from_ref(&boundary),
                &[closure],
                &[],
                &[boundary.id()].into_iter().collect(),
            ),
            slot,
            &schema,
            replacement_context(&f)?,
            |_, _| Ok(false),
        )?;
        assert!(matches!(closed, MultiValueOutcome::Known { values }
            if values.first().is_some_and(|value| matches!(value.value(), Value::String(text) if text == "parent"))));

        let retraction = ReplacementBoundaryRetraction::new(
            id::<ReplacementBoundaryRetractionId>(35)?,
            &boundary,
            "withdrawn",
            rev(4)?,
        )?;
        let retracted = resolve_multi_value_replace(
            std::slice::from_ref(&f.candidate),
            ReplacementBoundaryHistory::new(
                std::slice::from_ref(&boundary),
                &[],
                &[retraction],
                &[boundary.id()].into_iter().collect(),
            ),
            slot,
            &schema,
            replacement_context(&f)?,
            |_, _| Ok(false),
        )?;
        assert!(matches!(retracted, MultiValueOutcome::Known { values }
            if values.first().is_some_and(|value| matches!(value.value(), Value::String(text) if text == "parent"))));
        Ok(())
    }

    #[test]
    fn future_or_archived_boundary_does_not_create_a_known_empty_set() -> TestResult {
        let f = fixture()?;
        let schema = multi_replace_schema(&f)?;
        let slot = MultiValueSlot::new(f.slot.subject(), f.slot.predicate_id());
        let future = ReplacementBoundary::new(
            id::<ReplacementBoundaryId>(36)?,
            ContextKey::new(
                f.child,
                f.layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            f.slot.subject(),
            &schema,
            None,
            rev(5)?,
        )?;
        let asof_before_boundary = resolve_multi_value_replace(
            &[],
            ReplacementBoundaryHistory::new(
                std::slice::from_ref(&future),
                &[],
                &[],
                &[future.id()].into_iter().collect(),
            ),
            slot,
            &schema,
            replacement_context(&f)?,
            |_, _| Ok(false),
        )?;
        assert!(matches!(asof_before_boundary, MultiValueOutcome::Unknown));
        let archived = resolve_multi_value_replace(
            &[],
            ReplacementBoundaryHistory::new(
                std::slice::from_ref(&future),
                &[],
                &[],
                &BTreeSet::new(),
            ),
            slot,
            &schema,
            replacement_context(&f)?,
            |_, _| Ok(false),
        )?;
        assert!(matches!(archived, MultiValueOutcome::Unknown));
        Ok(())
    }
}
