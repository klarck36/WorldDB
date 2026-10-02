//! Reference-model evaluation for assertion masks.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::assertions::{Assertion, Polarity, Subject};
use crate::candidate_scan::AssertionCandidate;
use crate::catalog::HistorySpaceCatalog;
use crate::context::ContextKey;
use crate::context_precedence::{ContextPrecedence, ContextPrecedenceError};
use crate::ids::{
    HistorySpaceId, MaskId, MaskRetractionId, MaskValidityClosureId, PredicateId, PrincipalId,
    Revision,
};
use crate::layers::LayerSchemaSnapshot;
use crate::masks::{Mask, MaskRetraction, MaskSelector, MaskValidityClosure, PropositionKey};
use crate::query_context::{QueryContext, WorldTimeSelector};
use crate::record_refs::RecordRef;
use crate::security::{
    AuthorizationDecision, Capability, FieldSelector, PolicyTarget, SecurityPolicySnapshot,
};
use crate::temporal::{RecordedAsOf, TemporalError, WorldTime};
use crate::values::{Value, canonical_value_equality};

/// Pinned historical coordinates and metadata snapshots for one Mask projection.
pub struct AssertionMaskContext<'a> {
    recorded_as_of: RecordedAsOf,
    world_time: WorldTime,
    query_history_space: Option<HistorySpaceId>,
    history_spaces: &'a HistorySpaceCatalog,
    layers: &'a LayerSchemaSnapshot,
}

impl<'a> AssertionMaskContext<'a> {
    /// Binds both time axes and the query's ancestry/layer snapshots.
    #[must_use]
    pub const fn new(
        recorded_as_of: RecordedAsOf,
        world_time: WorldTime,
        history_spaces: &'a HistorySpaceCatalog,
        layers: &'a LayerSchemaSnapshot,
    ) -> Self {
        Self {
            recorded_as_of,
            world_time,
            query_history_space: None,
            history_spaces,
            layers,
        }
    }

    /// Binds the query HistorySpace so a security wrapper can validate even an
    /// empty candidate set without inferring identity from result rows.
    #[must_use]
    pub const fn with_query_history_space(mut self, history_space: HistorySpaceId) -> Self {
        self.query_history_space = Some(history_space);
        self
    }
}

/// Complete Mask and lifecycle history input for one authorized projection.
pub struct AuthorizedAssertionMaskHistory<'a> {
    masks: &'a [Mask],
    validity_closures: &'a [MaskValidityClosure],
    retractions: &'a [MaskRetraction],
    archive_visible_masks: &'a BTreeSet<MaskId>,
}

/// Visible candidates after one authorized mask pass, with masks that removed
/// at least one candidate for an Explain trace.
#[derive(Clone, Debug)]
pub struct AssertionMaskProjection {
    candidates: Vec<AssertionCandidate>,
    applied_mask_ids: Vec<MaskId>,
}

impl AssertionMaskProjection {
    /// Candidates that survived the mask pass, in canonical AssertionId order.
    #[must_use]
    pub fn candidates(&self) -> &[AssertionCandidate] {
        &self.candidates
    }

    /// Masks that actually removed one or more candidates, in canonical ID order.
    #[must_use]
    pub fn applied_mask_ids(&self) -> &[MaskId] {
        &self.applied_mask_ids
    }
}

impl<'a> AuthorizedAssertionMaskHistory<'a> {
    /// Binds Mask lifecycle and archive visibility inputs as one history view.
    #[must_use]
    pub const fn new(
        masks: &'a [Mask],
        validity_closures: &'a [MaskValidityClosure],
        retractions: &'a [MaskRetraction],
        archive_visible_masks: &'a BTreeSet<MaskId>,
    ) -> Self {
        Self {
            masks,
            validity_closures,
            retractions,
            archive_visible_masks,
        }
    }
}

/// Applies active assertion masks to an already security-filtered candidate set.
///
/// The caller supplies archive-visible Mask IDs and a schema-aware comparator for
/// temporal proposition values. Masks outside the query's Perspective/Epistemic
/// partition or HistorySpace ancestry cannot affect the result. A mask removes
/// only candidates with strictly lower precedence; it never creates a positive
/// or negative assertion.
pub fn apply_assertion_masks<F>(
    candidates: &[AssertionCandidate],
    masks: &[Mask],
    validity_closures: &[MaskValidityClosure],
    retractions: &[MaskRetraction],
    archive_visible_masks: &BTreeSet<MaskId>,
    context: AssertionMaskContext<'_>,
    temporal_value_equal: F,
) -> Result<Vec<AssertionCandidate>, MaskProjectionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    Ok(apply_assertion_masks_with_trace(
        candidates,
        masks,
        validity_closures,
        retractions,
        archive_visible_masks,
        context,
        temporal_value_equal,
    )?
    .candidates)
}

fn apply_assertion_masks_with_trace<F>(
    candidates: &[AssertionCandidate],
    masks: &[Mask],
    validity_closures: &[MaskValidityClosure],
    retractions: &[MaskRetraction],
    archive_visible_masks: &BTreeSet<MaskId>,
    context: AssertionMaskContext<'_>,
    mut temporal_value_equal: F,
) -> Result<AssertionMaskProjection, MaskProjectionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    validate_mask_lifecycle(masks, validity_closures, retractions)?;
    let mut active = Vec::new();
    for mask in masks {
        if mask.created_revision() > context.recorded_as_of.revision()
            || !archive_visible_masks.contains(&mask.id())
            || retractions.iter().any(|item| {
                item.mask_id() == mask.id()
                    && item.created_revision() <= context.recorded_as_of.revision()
            })
        {
            continue;
        }
        if let Some(validity) = mask.validity() {
            if !validity.contains(context.world_time)? {
                continue;
            }
        }
        let mut closed = false;
        for closure in validity_closures.iter().filter(|closure| {
            closure.mask_id() == mask.id()
                && closure.created_revision() <= context.recorded_as_of.revision()
        }) {
            if context
                .world_time
                .checked_cmp(closure.close_at_world_time())?
                .is_ge()
            {
                closed = true;
                break;
            }
        }
        if closed {
            continue;
        }
        active.push(mask);
    }

    // Exact-ID Masks dominate large retained histories because they are cheap
    // to serialize and unambiguous. Group those selectors by target once so a
    // point query does not compare every candidate with every unrelated Mask.
    // Keep original ordinals and merge with general selectors per candidate;
    // this preserves the reference path's first-matching-mask trace behavior.
    let mut exact_by_assertion = BTreeMap::new();
    let mut general_masks = Vec::new();
    for (ordinal, mask) in active.into_iter().enumerate() {
        match mask.selector() {
            MaskSelector::ExactAssertion(assertion_id) => exact_by_assertion
                .entry(*assertion_id)
                .or_insert_with(Vec::new)
                .push((ordinal, mask)),
            MaskSelector::Proposition(_) | MaskSelector::Slot(_) => {
                general_masks.push((ordinal, mask));
            }
        }
    }

    let mut visible = Vec::with_capacity(candidates.len());
    let mut applied_mask_ids = BTreeSet::new();
    'candidate: for candidate in candidates {
        let assertion = candidate.assertion();
        let mut general = general_masks.iter().copied().peekable();
        let mut exact = exact_by_assertion
            .get(&assertion.id())
            .into_iter()
            .flatten()
            .copied()
            .peekable();
        loop {
            let next = match (general.peek(), exact.peek()) {
                (Some(general_mask), Some(exact_mask)) if general_mask.0 <= exact_mask.0 => {
                    general.next()
                }
                (Some(_), Some(_)) => exact.next(),
                (Some(_), None) => general.next(),
                (None, Some(_)) => exact.next(),
                (None, None) => break,
            };
            let Some((_, mask)) = next else {
                break;
            };
            if !candidate
                .selected_layer_ids()
                .contains(&mask.context().layer_id())
            {
                continue;
            }
            if !same_partition(mask.context(), assertion.context()) {
                continue;
            }
            let mask_precedence = match ContextPrecedence::for_context(
                candidate.query_history_space_id,
                mask.context().history_space_id(),
                mask.context().layer_id(),
                context.history_spaces,
                context.layers,
            ) {
                Ok(value) => value,
                Err(ContextPrecedenceError::RecordOutsideQueryAncestry { .. }) => continue,
                Err(error) => return Err(error.into()),
            };
            if mask_precedence <= candidate.precedence() {
                continue;
            }
            if selector_matches(mask.selector(), assertion, &mut temporal_value_equal)? {
                applied_mask_ids.insert(mask.id());
                continue 'candidate;
            }
        }
        visible.push(candidate.clone());
    }
    Ok(AssertionMaskProjection {
        candidates: visible,
        applied_mask_ids: applied_mask_ids.into_iter().collect(),
    })
}

/// Applies only authorized Masks to candidates already filtered by the same
/// security-bearing query context. Unauthorized Masks and their lifecycle
/// records are discarded before redaction/masking evaluation begins.
pub fn apply_authorized_assertion_masks<F>(
    candidates: &[AssertionCandidate],
    history: AuthorizedAssertionMaskHistory<'_>,
    context: AssertionMaskContext<'_>,
    policy: &SecurityPolicySnapshot,
    query_context: &QueryContext,
    temporal_value_equal: F,
) -> Result<Vec<AssertionCandidate>, MaskProjectionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    Ok(apply_authorized_assertion_masks_with_trace(
        candidates,
        history,
        context,
        policy,
        query_context,
        temporal_value_equal,
    )?
    .candidates)
}

/// Applies security-filtered Masks and records the exact Masks that removed
/// candidates, for a visibility-checked Explain result.
pub fn apply_authorized_assertion_masks_with_trace<F>(
    candidates: &[AssertionCandidate],
    history: AuthorizedAssertionMaskHistory<'_>,
    context: AssertionMaskContext<'_>,
    policy: &SecurityPolicySnapshot,
    query_context: &QueryContext,
    temporal_value_equal: F,
) -> Result<AssertionMaskProjection, MaskProjectionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    if query_context.recorded_as_of() != context.recorded_as_of
        || query_context.world_time() != WorldTimeSelector::At(context.world_time)
        || query_context.layers().schema_revision() != context.layers.revision()
        || context.query_history_space != Some(query_context.history_space())
        || candidates
            .iter()
            .any(|candidate| candidate.query_history_space_id != query_context.history_space())
    {
        return Err(MaskProjectionError::QueryContextMismatch);
    }
    let principal = query_context.security().principal_id();
    let visible_masks = history
        .masks
        .iter()
        .filter(|mask| mask_is_authorized(policy, principal, mask))
        .cloned()
        .collect::<Vec<_>>();
    let visible_ids = visible_masks.iter().map(Mask::id).collect::<BTreeSet<_>>();
    let visible_closures = history
        .validity_closures
        .iter()
        .filter(|closure| visible_ids.contains(&closure.mask_id()))
        .copied()
        .collect::<Vec<_>>();
    let visible_retractions = history
        .retractions
        .iter()
        .filter(|retraction| visible_ids.contains(&retraction.mask_id()))
        .cloned()
        .collect::<Vec<_>>();
    apply_assertion_masks_with_trace(
        candidates,
        &visible_masks,
        &visible_closures,
        &visible_retractions,
        history.archive_visible_masks,
        context,
        temporal_value_equal,
    )
}

fn mask_is_authorized(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    mask: &Mask,
) -> bool {
    let context = mask.context();
    let record = RecordRef::Mask(mask.id());
    let base = PolicyTarget::new(
        Some(context.history_space_id()),
        Some(context.layer_id()),
        Some(record),
        None,
        None,
    );
    [
        Capability::HistorySpaceRead,
        Capability::LayerRead,
        Capability::MaskRead,
    ]
    .into_iter()
    .all(|capability| policy.authorize(principal, capability, base) == AuthorizationDecision::Allow)
        && [FieldSelector::MaskSelector, FieldSelector::MaskValidity]
            .into_iter()
            .all(|field| {
                policy.authorize(
                    principal,
                    Capability::FieldRead,
                    PolicyTarget::new(
                        Some(context.history_space_id()),
                        Some(context.layer_id()),
                        Some(record),
                        Some(field),
                        None,
                    ),
                ) == AuthorizationDecision::Allow
            })
}

fn same_partition(left: ContextKey, right: ContextKey) -> bool {
    left.perspective_scope() == right.perspective_scope()
        && left.epistemic_mode() == right.epistemic_mode()
}

fn selector_matches<F>(
    selector: &MaskSelector,
    assertion: &Assertion,
    temporal_value_equal: &mut F,
) -> Result<bool, MaskProjectionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    Ok(match selector {
        MaskSelector::ExactAssertion(assertion_id) => *assertion_id == assertion.id(),
        MaskSelector::Proposition(key) => {
            let (subject, predicate_id, value, polarity) = assertion.proposition_components();
            proposition_matches(
                key,
                subject,
                predicate_id,
                value,
                polarity,
                temporal_value_equal,
            )?
        }
        MaskSelector::Slot(slot) => {
            slot.subject() == assertion.subject()
                && slot.predicate_id() == assertion.predicate_id()
                && slot.perspective_scope() == assertion.context().perspective_scope()
                && slot.epistemic_mode() == assertion.context().epistemic_mode()
        }
    })
}

fn proposition_matches<F>(
    key: &PropositionKey,
    subject: Subject,
    predicate_id: PredicateId,
    value: &Value,
    polarity: Polarity,
    temporal_value_equal: &mut F,
) -> Result<bool, MaskProjectionError>
where
    F: FnMut(&Value, &Value) -> Result<bool, ()>,
{
    if key.subject() != subject || key.predicate_id() != predicate_id || key.polarity() != polarity
    {
        return Ok(false);
    }
    match canonical_value_equality(key.value(), value) {
        Some(equal) => Ok(equal),
        None => temporal_value_equal(key.value(), value)
            .map_err(|()| MaskProjectionError::TemporalValueComparisonUnavailable),
    }
}

/// Returns `None` when equality requires schema-resolved temporal conversion.
fn validate_mask_lifecycle(
    masks: &[Mask],
    closures: &[MaskValidityClosure],
    retractions: &[MaskRetraction],
) -> Result<(), MaskProjectionError> {
    let mut mask_ids = BTreeSet::new();
    for mask in masks {
        if !mask_ids.insert(mask.id()) {
            return Err(MaskProjectionError::DuplicateMask { mask_id: mask.id() });
        }
    }
    let mut closure_ids = BTreeSet::new();
    for closure in closures {
        if !closure_ids.insert(closure.id()) {
            return Err(MaskProjectionError::DuplicateClosure {
                closure_id: closure.id(),
            });
        }
        let mask = masks
            .iter()
            .find(|mask| mask.id() == closure.mask_id())
            .ok_or(MaskProjectionError::UnknownClosureTarget {
                closure_id: closure.id(),
                mask_id: closure.mask_id(),
            })?;
        validate_lifecycle_revision(mask, closure.created_revision(), "MaskValidityClosure")?;
        if let Some(validity) = mask.validity() {
            let interval = validity.interval();
            if interval.timeline().id() != closure.close_at_world_time().timeline().id() {
                return Err(TemporalError::IncomparableTimelines {
                    expected: interval.timeline().id(),
                    actual: closure.close_at_world_time().timeline().id(),
                }
                .into());
            }
            if interval.start().is_some_and(|start| {
                closure.close_at_world_time().nanoseconds() < start.nanoseconds()
            }) || interval
                .end()
                .is_some_and(|end| closure.close_at_world_time().nanoseconds() > end.nanoseconds())
            {
                return Err(MaskProjectionError::ClosureOutsideValidity {
                    mask_id: mask.id(),
                    closure_id: closure.id(),
                });
            }
        }
    }
    let mut retraction_ids = BTreeSet::new();
    for retraction in retractions {
        if !retraction_ids.insert(retraction.id()) {
            return Err(MaskProjectionError::DuplicateRetraction {
                retraction_id: retraction.id(),
            });
        }
        let mask = masks
            .iter()
            .find(|mask| mask.id() == retraction.mask_id())
            .ok_or(MaskProjectionError::UnknownRetractionTarget {
                retraction_id: retraction.id(),
                mask_id: retraction.mask_id(),
            })?;
        validate_lifecycle_revision(mask, retraction.created_revision(), "MaskRetraction")?;
    }
    Ok(())
}

fn validate_lifecycle_revision(
    mask: &Mask,
    lifecycle_revision: Revision,
    record: &'static str,
) -> Result<(), MaskProjectionError> {
    if lifecycle_revision <= mask.created_revision() {
        return Err(MaskProjectionError::LifecycleRevisionNotAfterMask {
            mask_id: mask.id(),
            mask_revision: mask.created_revision(),
            lifecycle_revision,
            record,
        });
    }
    Ok(())
}

/// Invalid mask history or comparison input encountered while projecting candidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaskProjectionError {
    /// Query context disagrees with the time, ancestry, or schema pins.
    QueryContextMismatch,
    /// One Mask ID appears more than once.
    DuplicateMask { mask_id: MaskId },
    /// One validity closure ID appears more than once.
    DuplicateClosure { closure_id: MaskValidityClosureId },
    /// A closure refers to a Mask absent from the supplied history.
    UnknownClosureTarget {
        closure_id: MaskValidityClosureId,
        mask_id: MaskId,
    },
    /// One retraction ID appears more than once.
    DuplicateRetraction { retraction_id: MaskRetractionId },
    /// A retraction refers to a Mask absent from the supplied history.
    UnknownRetractionTarget {
        retraction_id: MaskRetractionId,
        mask_id: MaskId,
    },
    /// A Mask lifecycle record must be created strictly after its Mask.
    LifecycleRevisionNotAfterMask {
        mask_id: MaskId,
        mask_revision: Revision,
        lifecycle_revision: Revision,
        record: &'static str,
    },
    /// Temporal proposition equality was requested without an available schema comparator.
    TemporalValueComparisonUnavailable,
    /// A validity closure falls outside the Mask's declared validity interval.
    ClosureOutsideValidity {
        mask_id: MaskId,
        closure_id: MaskValidityClosureId,
    },
    /// A time comparison crossed incompatible timeline identities.
    Temporal(TemporalError),
    /// Context precedence could not be established from the selected snapshots.
    Precedence(ContextPrecedenceError),
}

impl From<TemporalError> for MaskProjectionError {
    fn from(error: TemporalError) -> Self {
        Self::Temporal(error)
    }
}

impl From<ContextPrecedenceError> for MaskProjectionError {
    fn from(error: ContextPrecedenceError) -> Self {
        Self::Precedence(error)
    }
}

impl fmt::Display for MaskProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueryContextMismatch => {
                formatter.write_str("query context does not match mask scan")
            }
            Self::DuplicateMask { mask_id } => write!(formatter, "duplicate mask ID: {mask_id}"),
            Self::DuplicateClosure { closure_id } => {
                write!(formatter, "duplicate mask closure ID: {closure_id}")
            }
            Self::UnknownClosureTarget {
                closure_id,
                mask_id,
            } => write!(
                formatter,
                "mask closure {closure_id} targets unknown mask {mask_id}"
            ),
            Self::DuplicateRetraction { retraction_id } => {
                write!(formatter, "duplicate mask retraction ID: {retraction_id}")
            }
            Self::UnknownRetractionTarget {
                retraction_id,
                mask_id,
            } => write!(
                formatter,
                "mask retraction {retraction_id} targets unknown mask {mask_id}"
            ),
            Self::LifecycleRevisionNotAfterMask {
                mask_id,
                mask_revision,
                lifecycle_revision,
                record,
            } => write!(
                formatter,
                "{record} at revision {lifecycle_revision} does not follow mask {mask_id} at revision {mask_revision}"
            ),
            Self::TemporalValueComparisonUnavailable => {
                formatter.write_str("temporal proposition value requires schema-aware equality")
            }
            Self::ClosureOutsideValidity {
                mask_id,
                closure_id,
            } => write!(
                formatter,
                "mask closure {closure_id} falls outside mask {mask_id}'s validity interval"
            ),
            Self::Temporal(error) => write!(formatter, "invalid mask timeline comparison: {error}"),
            Self::Precedence(error) => write!(formatter, "mask precedence failed: {error}"),
        }
    }
}

impl std::error::Error for MaskProjectionError {}

#[cfg(test)]
mod tests {
    use super::{
        AssertionMaskContext, MaskProjectionError, apply_assertion_masks, mask_is_authorized,
    };
    use crate::assertions::{Assertion, AssertionDraft, Polarity, Subject};
    use crate::candidate_scan::AssertionCandidate;
    use crate::catalog::{HistorySpaceCatalog, HistorySpaceDefinition, HistorySpaceError};
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::context_precedence::ContextPrecedence;
    use crate::ids::{
        AssertionId, DomainId, EntityId, HistorySpaceId, IdValidationError, LayerId, MaskId,
        MaskRetractionId, MaskValidityClosureId, PredicateId, Revision, RevisionError,
        SchemaRevision, TimelineId,
    };
    use crate::layers::{LayerDefinition, LayerSchemaSnapshot};
    use crate::masks::{
        Mask, MaskRetraction, MaskSelector, MaskSlotSelector, MaskValidityClosure, PropositionKey,
    };
    use crate::schema::Lifecycle;
    use crate::temporal::{AssertionValidity, RecordedAsOf, TimeInterval, Timeline, WorldTime};
    use crate::values::Value;
    use std::error::Error;
    use std::fmt;

    type TestResult<T = ()> = Result<T, TestError>;
    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(RevisionError),
        Context(crate::context::ContextError),
        Mask(crate::masks::MaskRecordError),
        Layer(crate::layers::LayerSchemaError),
        Precedence(crate::context_precedence::ContextPrecedenceError),
        Projection(MaskProjectionError),
        Temporal(crate::temporal::TemporalError),
        History(HistorySpaceError),
        Symbol(crate::values::SymbolError),
        Policy(crate::security::SecurityPolicyError),
        Role(crate::security::RoleDefinitionError),
        Bundle(crate::security::PolicyBundleError),
    }
    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(e) => write!(f, "{e}"),
                Self::Revision(e) => write!(f, "{e}"),
                Self::Context(e) => write!(f, "{e}"),
                Self::Mask(e) => write!(f, "{e}"),
                Self::Layer(e) => write!(f, "{e}"),
                Self::Precedence(e) => write!(f, "{e}"),
                Self::Projection(e) => write!(f, "{e}"),
                Self::Temporal(e) => write!(f, "{e}"),
                Self::History(e) => write!(f, "{e}"),
                Self::Symbol(e) => write!(f, "{e}"),
                Self::Policy(e) => write!(f, "{e}"),
                Self::Role(e) => write!(f, "{e}"),
                Self::Bundle(e) => write!(f, "{e}"),
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
    convert!(crate::masks::MaskRecordError, Mask);
    convert!(crate::layers::LayerSchemaError, Layer);
    convert!(
        crate::context_precedence::ContextPrecedenceError,
        Precedence
    );
    convert!(MaskProjectionError, Projection);
    convert!(crate::temporal::TemporalError, Temporal);
    convert!(HistorySpaceError, History);
    convert!(crate::values::SymbolError, Symbol);
    convert!(crate::security::SecurityPolicyError, Policy);
    convert!(crate::security::RoleDefinitionError, Role);
    convert!(crate::security::PolicyBundleError, Bundle);

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
        catalog: HistorySpaceCatalog,
        layers: LayerSchemaSnapshot,
        root: HistorySpaceId,
        child: HistorySpaceId,
        layer: LayerId,
        timeline: Timeline,
        assertion: Assertion,
        candidate: AssertionCandidate,
    }
    fn fixture() -> TestResult<Fixture> {
        let root = id::<HistorySpaceId>(1)?;
        let child = id::<HistorySpaceId>(2)?;
        let layer = id::<LayerId>(3)?;
        let timeline = Timeline::new(id::<TimelineId>(4)?);
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
        let context = ContextKey::new(
            root,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let validity = AssertionValidity::new(TimeInterval::new(timeline, None, None)?);
        let assertion = Assertion::new(
            id::<AssertionId>(5)?,
            AssertionDraft::new(
                context,
                Subject::new(id::<EntityId>(6)?),
                id::<PredicateId>(7)?,
                Value::String(String::from("x")),
                Polarity::Positive,
                validity,
            ),
            rev(2)?,
        );
        let precedence = ContextPrecedence::for_context(child, root, layer, &catalog, &layers)?;
        let candidate = AssertionCandidate {
            assertion: assertion.clone(),
            source_history_space_id: root,
            query_history_space_id: child,
            selected_layer_ids: [layer].into_iter().collect(),
            precedence,
        };
        Ok(Fixture {
            catalog,
            layers,
            root,
            child,
            layer,
            timeline,
            assertion,
            candidate,
        })
    }
    fn mask(f: &Fixture, idv: u8, hs: HistorySpaceId, selector: MaskSelector) -> TestResult<Mask> {
        Ok(Mask::new(
            id::<MaskId>(idv)?,
            ContextKey::new(
                hs,
                f.layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            selector,
            None,
            rev(3)?,
        )?)
    }
    fn run(
        f: &Fixture,
        masks: &[Mask],
        ordinary: &[MaskId],
    ) -> TestResult<Vec<AssertionCandidate>> {
        Ok(apply_assertion_masks(
            &[f.candidate.clone()],
            masks,
            &[],
            &[],
            &ordinary.iter().copied().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 5),
                &f.catalog,
                &f.layers,
            ),
            |a, b| {
                Ok(
                    matches!((a,b),(Value::Time(x),Value::Time(y)) if x.timeline_id()==y.timeline_id()&&x.ticks()==y.ticks()&&x.unit()==y.unit()),
                )
            },
        )?)
    }

    #[test]
    fn masks_need_selector_and_validity_field_rights_before_projection() -> TestResult {
        use crate::security::{
            Capability, CapabilityGrant, CapabilityRule, FieldSelector, GrantEffect, PolicyBundle,
            PolicyScope, PolicySubject, Principal, RoleAssignment, RoleDefinition,
            SecurityPolicySnapshot,
        };

        let fixture = fixture()?;
        let mask = mask(
            &fixture,
            40,
            fixture.child,
            MaskSelector::ExactAssertion(fixture.assertion.id()),
        )?;
        let principal = id::<crate::ids::PrincipalId>(41)?;
        let role_id = id::<crate::ids::RoleId>(42)?;
        let assignment_id = id::<crate::ids::RoleAssignmentId>(43)?;
        let role = RoleDefinition::new(
            role_id,
            "mask_reader",
            PolicyBundle::from_grants([
                CapabilityGrant::new(Capability::HistorySpaceRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::LayerRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::MaskRead, GrantEffect::Allow),
                CapabilityGrant::new(Capability::FieldRead, GrantEffect::Allow),
            ])?,
        )?;
        let assignment =
            RoleAssignment::new(assignment_id, principal, role_id, PolicyScope::project());
        let allowed = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role.clone()],
            vec![assignment],
            Vec::new(),
        )?;
        assert!(mask_is_authorized(&allowed, principal, &mask));
        let denied_selector = CapabilityRule::new(
            id::<crate::ids::PolicyRuleId>(44)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::FieldRead, GrantEffect::Deny),
            PolicyScope::new(
                Some(fixture.child),
                Some(fixture.layer),
                Some(crate::RecordRef::Mask(mask.id())),
                Some(FieldSelector::MaskSelector),
                None,
            ),
        );
        let denied = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![role],
            vec![assignment],
            vec![denied_selector],
        )?;
        assert!(!mask_is_authorized(&denied, principal, &mask));
        Ok(())
    }
    #[test]
    fn exact_assertion_mask_only_hides_lower_precedence_candidate() -> TestResult {
        let f = fixture()?;
        let exact = mask(
            &f,
            8,
            f.child,
            MaskSelector::ExactAssertion(f.assertion.id()),
        )?;
        assert!(run(&f, &[exact.clone()], &[exact.id()])?.is_empty());
        let same = mask(
            &f,
            9,
            f.root,
            MaskSelector::ExactAssertion(f.assertion.id()),
        )?;
        assert_eq!(run(&f, &[same.clone()], &[same.id()])?.len(), 1);
        Ok(())
    }
    #[test]
    fn proposition_and_slot_selectors_obey_their_distinct_scopes() -> TestResult {
        let f = fixture()?;
        let key = PropositionKey::new(
            f.assertion.subject(),
            f.assertion.predicate_id(),
            Value::String(String::from("x")),
            Polarity::Positive,
        );
        let proposition = mask(&f, 10, f.child, MaskSelector::Proposition(key))?;
        assert!(run(&f, &[proposition.clone()], &[proposition.id()])?.is_empty());

        let different_value = PropositionKey::new(
            f.assertion.subject(),
            f.assertion.predicate_id(),
            Value::String(String::from("different")),
            Polarity::Positive,
        );
        let value_mask = mask(&f, 20, f.child, MaskSelector::Proposition(different_value))?;
        assert_eq!(run(&f, &[value_mask.clone()], &[value_mask.id()])?.len(), 1);

        let different_polarity = PropositionKey::new(
            f.assertion.subject(),
            f.assertion.predicate_id(),
            Value::String(String::from("x")),
            Polarity::Negative,
        );
        let polarity_mask = mask(
            &f,
            21,
            f.child,
            MaskSelector::Proposition(different_polarity),
        )?;
        assert_eq!(
            run(&f, &[polarity_mask.clone()], &[polarity_mask.id()])?.len(),
            1
        );

        let slot = MaskSlotSelector::new(
            f.assertion.subject(),
            f.assertion.predicate_id(),
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let slotmask = mask(&f, 11, f.child, MaskSelector::Slot(slot))?;
        assert!(run(&f, &[slotmask.clone()], &[slotmask.id()])?.is_empty());
        Ok(())
    }

    #[test]
    fn proposition_mask_truth_table_requires_each_of_the_four_equality_components() -> TestResult {
        let f = fixture()?;
        let subject = f.assertion.subject();
        let predicate = f.assertion.predicate_id();
        let value = Value::String(String::from("x"));
        let cases = [
            (
                30,
                PropositionKey::new(subject, predicate, value.clone(), Polarity::Positive),
                true,
            ),
            (
                31,
                PropositionKey::new(
                    Subject::new(id::<EntityId>(32)?),
                    predicate,
                    value.clone(),
                    Polarity::Positive,
                ),
                false,
            ),
            (
                33,
                PropositionKey::new(
                    subject,
                    id::<PredicateId>(34)?,
                    value.clone(),
                    Polarity::Positive,
                ),
                false,
            ),
            (
                35,
                PropositionKey::new(
                    subject,
                    predicate,
                    Value::String(String::from("different")),
                    Polarity::Positive,
                ),
                false,
            ),
            (
                36,
                PropositionKey::new(subject, predicate, value, Polarity::Negative),
                false,
            ),
        ];

        for (mask_id, key, matches) in cases {
            let proposition_mask = mask(&f, mask_id, f.child, MaskSelector::Proposition(key))?;
            let visible = run(
                &f,
                std::slice::from_ref(&proposition_mask),
                &[proposition_mask.id()],
            )?;
            assert_eq!(visible.is_empty(), matches);
        }
        Ok(())
    }
    #[test]
    fn cross_partition_and_equal_precedence_masks_do_not_hide() -> TestResult {
        let f = fixture()?;
        let slot = MaskSlotSelector::new(
            f.assertion.subject(),
            f.assertion.predicate_id(),
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let equal = mask(&f, 12, f.root, MaskSelector::Slot(slot))?;
        let unrelated = Mask::new(
            id::<MaskId>(13)?,
            ContextKey::new(
                f.child,
                f.layer,
                PerspectiveScope::Perspective(id::<crate::ids::PerspectiveId>(14)?),
                EpistemicMode::Knows,
            )?,
            MaskSelector::ExactAssertion(f.assertion.id()),
            None,
            rev(3)?,
        )?;
        assert_eq!(
            run(
                &f,
                &[equal.clone(), unrelated.clone()],
                &[equal.id(), unrelated.id()]
            )?
            .len(),
            1
        );
        Ok(())
    }
    #[test]
    fn temporal_proposition_needs_the_schema_value_comparator() -> TestResult {
        let mut f = fixture()?;
        let temporal = Value::Time(crate::Time::new(
            id::<TimelineId>(4)?,
            1,
            crate::Symbol::new("tick")?,
        ));
        f.assertion = Assertion::new(
            id::<AssertionId>(16)?,
            AssertionDraft::new(
                f.assertion.context(),
                f.assertion.subject(),
                f.assertion.predicate_id(),
                temporal.clone(),
                Polarity::Positive,
                AssertionValidity::new(TimeInterval::new(f.timeline, None, None)?),
            ),
            rev(2)?,
        );
        f.candidate.assertion = f.assertion.clone();
        let key = PropositionKey::new(
            f.assertion.subject(),
            f.assertion.predicate_id(),
            temporal,
            Polarity::Positive,
        );
        let mask = mask(&f, 15, f.child, MaskSelector::Proposition(key))?;
        let result = apply_assertion_masks(
            &[f.candidate.clone()],
            &[mask.clone()],
            &[],
            &[],
            &[mask.id()].into_iter().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 5),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Err(()),
        );
        assert!(matches!(
            result,
            Err(MaskProjectionError::TemporalValueComparisonUnavailable)
        ));
        Ok(())
    }
    #[test]
    fn mask_closure_and_retraction_respect_their_historical_axes() -> TestResult {
        let f = fixture()?;
        let mask = mask(
            &f,
            17,
            f.child,
            MaskSelector::ExactAssertion(f.assertion.id()),
        )?;
        let mask_id = mask.id();
        let closure = MaskValidityClosure::new(
            id::<MaskValidityClosureId>(18)?,
            &mask,
            WorldTime::from_nanoseconds(f.timeline, 8),
            rev(5)?,
        )?;
        let retraction =
            MaskRetraction::new(id::<MaskRetractionId>(19)?, &mask, "retracted", rev(6)?)?;
        let visible = apply_assertion_masks(
            &[f.candidate.clone()],
            &[mask.clone()],
            &[closure],
            &[retraction.clone()],
            &[mask_id].into_iter().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 5),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        assert!(visible.is_empty());
        let later_world_time = apply_assertion_masks(
            &[f.candidate.clone()],
            &[mask.clone()],
            &[closure],
            &[retraction.clone()],
            &[mask_id].into_iter().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(5)?),
                WorldTime::from_nanoseconds(f.timeline, 8),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        assert_eq!(later_world_time.len(), 1);
        let later_transaction = apply_assertion_masks(
            &[f.candidate.clone()],
            &[mask],
            &[closure],
            &[retraction],
            &[mask_id].into_iter().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(6)?),
                WorldTime::from_nanoseconds(f.timeline, 5),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        assert_eq!(later_transaction.len(), 1);
        Ok(())
    }

    #[test]
    fn mask_validity_is_half_open_and_future_masks_are_not_visible() -> TestResult {
        let f = fixture()?;
        let validity = AssertionValidity::new(TimeInterval::new(
            f.timeline,
            Some(WorldTime::from_nanoseconds(f.timeline, 10)),
            Some(WorldTime::from_nanoseconds(f.timeline, 20)),
        )?);
        let scoped = Mask::new(
            id::<MaskId>(22)?,
            ContextKey::new(
                f.child,
                f.layer,
                PerspectiveScope::World,
                EpistemicMode::WorldState,
            )?,
            MaskSelector::ExactAssertion(f.assertion.id()),
            Some(validity),
            rev(3)?,
        )?;
        let archived_visible = [scoped.id()].into_iter().collect();
        let before_start = apply_assertion_masks(
            &[f.candidate.clone()],
            &[scoped.clone()],
            &[],
            &[],
            &archived_visible,
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 9),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        let at_start = apply_assertion_masks(
            &[f.candidate.clone()],
            &[scoped.clone()],
            &[],
            &[],
            &archived_visible,
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 10),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        let at_end = apply_assertion_masks(
            &[f.candidate.clone()],
            std::slice::from_ref(&scoped),
            &[],
            &[],
            &archived_visible,
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(4)?),
                WorldTime::from_nanoseconds(f.timeline, 20),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        assert_eq!(before_start.len(), 1);
        assert!(at_start.is_empty());
        assert_eq!(at_end.len(), 1);

        for point in 9_i128..=21 {
            let result = apply_assertion_masks(
                &[f.candidate.clone()],
                std::slice::from_ref(&scoped),
                &[],
                &[],
                &archived_visible,
                AssertionMaskContext::new(
                    RecordedAsOf::from_published_revision(rev(4)?),
                    WorldTime::from_nanoseconds(f.timeline, point),
                    &f.catalog,
                    &f.layers,
                ),
                |_, _| Ok(false),
            )?;
            assert_eq!(result.is_empty(), (10..20).contains(&point));
        }

        let future = mask(
            &f,
            23,
            f.child,
            MaskSelector::ExactAssertion(f.assertion.id()),
        )?;
        let future_visible = apply_assertion_masks(
            &[f.candidate.clone()],
            &[future.clone()],
            &[],
            &[],
            &[future.id()].into_iter().collect(),
            AssertionMaskContext::new(
                RecordedAsOf::from_published_revision(rev(2)?),
                WorldTime::from_nanoseconds(f.timeline, 5),
                &f.catalog,
                &f.layers,
            ),
            |_, _| Ok(false),
        )?;
        assert_eq!(future_visible.len(), 1);
        Ok(())
    }
}
