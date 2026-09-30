//! Assertion masks, replacement boundaries, and their lifecycle records.

use std::fmt;

use crate::assertions::{Polarity, Subject};
use crate::context::{ContextError, ContextKey, EpistemicMode, PerspectiveScope};
use crate::ids::{
    AssertionId, MaskId, MaskRetractionId, MaskValidityClosureId, PredicateId,
    ReplacementBoundaryId, ReplacementBoundaryRetractionId, ReplacementBoundaryValidityClosureId,
    Revision,
};
use crate::schema::{PredicateDefinition, ResolutionPolicy};
use crate::temporal::{AssertionValidity, TemporalError, WorldTime};
use crate::values::Value;

/// The Proposition Equality components selected by a mask.
///
/// It deliberately has no structural `Eq` implementation: the value
/// comparison path must resolve temporal values through the selected schema.
#[derive(Clone, Debug)]
pub struct PropositionKey {
    subject: Subject,
    predicate_id: PredicateId,
    value: Value,
    polarity: Polarity,
}

impl PropositionKey {
    /// Creates the complete proposition key without adding context or record identity.
    #[must_use]
    pub const fn new(
        subject: Subject,
        predicate_id: PredicateId,
        value: Value,
        polarity: Polarity,
    ) -> Self {
        Self {
            subject,
            predicate_id,
            value,
            polarity,
        }
    }

    /// Returns the selected subject.
    #[must_use]
    pub const fn subject(&self) -> Subject {
        self.subject
    }

    /// Returns the selected predicate identity.
    #[must_use]
    pub const fn predicate_id(&self) -> PredicateId {
        self.predicate_id
    }

    /// Returns the selected value.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    /// Returns the selected polarity.
    #[must_use]
    pub const fn polarity(&self) -> Polarity {
        self.polarity
    }
}

/// One explicitly validated slot selector for a mask.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MaskSlotSelector {
    subject: Subject,
    predicate_id: PredicateId,
    perspective_scope: PerspectiveScope,
    epistemic_mode: EpistemicMode,
}

impl MaskSlotSelector {
    /// Creates a slot selector only for a valid PerspectiveScope/EpistemicMode pair.
    pub fn new(
        subject: Subject,
        predicate_id: PredicateId,
        perspective_scope: PerspectiveScope,
        epistemic_mode: EpistemicMode,
    ) -> Result<Self, MaskRecordError> {
        if !valid_partition_pair(perspective_scope, epistemic_mode) {
            return Err(MaskRecordError::InvalidSlotPerspectiveModePair {
                perspective_scope,
                epistemic_mode,
            });
        }
        Ok(Self {
            subject,
            predicate_id,
            perspective_scope,
            epistemic_mode,
        })
    }

    /// Returns the subject in the selected slot.
    #[must_use]
    pub const fn subject(self) -> Subject {
        self.subject
    }

    /// Returns the predicate in the selected slot.
    #[must_use]
    pub const fn predicate_id(self) -> PredicateId {
        self.predicate_id
    }

    /// Returns the explicit Perspective partition.
    #[must_use]
    pub const fn perspective_scope(self) -> PerspectiveScope {
        self.perspective_scope
    }

    /// Returns the explicit Epistemic partition.
    #[must_use]
    pub const fn epistemic_mode(self) -> EpistemicMode {
        self.epistemic_mode
    }
}

/// One closed selector describing which assertions a Mask addresses.
#[derive(Clone, Debug)]
pub enum MaskSelector {
    /// Targets one concrete Assertion identity.
    ExactAssertion(AssertionId),
    /// Targets exactly one proposition under schema-aware value equality.
    Proposition(PropositionKey),
    /// Targets a whole subject/predicate slot in one epistemic partition.
    Slot(MaskSlotSelector),
}

/// One immutable assertion-mask record.
#[derive(Clone, Debug)]
pub struct Mask {
    id: MaskId,
    context: ContextKey,
    selector: MaskSelector,
    validity: Option<AssertionValidity>,
    created_revision: Revision,
}

impl Mask {
    /// Creates a Mask, requiring a slot selector to match its own epistemic partition.
    pub fn new(
        id: MaskId,
        context: ContextKey,
        selector: MaskSelector,
        validity: Option<AssertionValidity>,
        created_revision: Revision,
    ) -> Result<Self, MaskRecordError> {
        if let MaskSelector::Slot(slot) = &selector {
            if slot.perspective_scope() != context.perspective_scope()
                || slot.epistemic_mode() != context.epistemic_mode()
            {
                return Err(MaskRecordError::SlotContextMismatch {
                    mask_scope: context.perspective_scope(),
                    mask_mode: context.epistemic_mode(),
                    target_scope: slot.perspective_scope(),
                    target_mode: slot.epistemic_mode(),
                });
            }
        }
        Ok(Self {
            id,
            context,
            selector,
            validity,
            created_revision,
        })
    }

    /// Returns the stable mask identity.
    #[must_use]
    pub const fn id(&self) -> MaskId {
        self.id
    }

    /// Returns the concrete HistorySpace/Layer/Perspective/Epistemic context.
    #[must_use]
    pub const fn context(&self) -> ContextKey {
        self.context
    }

    /// Returns the closed selector.
    #[must_use]
    pub const fn selector(&self) -> &MaskSelector {
        &self.selector
    }

    /// Returns the optional world-time validity interval.
    #[must_use]
    pub const fn validity(&self) -> Option<AssertionValidity> {
        self.validity
    }

    /// Returns the Transaction-Time revision that created this record.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// One immutable record ending a Mask's world-time validity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MaskValidityClosure {
    id: MaskValidityClosureId,
    mask_id: MaskId,
    close_at_world_time: WorldTime,
    created_revision: Revision,
}

impl MaskValidityClosure {
    /// Creates a closure after its target Mask, on its validity timeline when one is explicit.
    pub fn new(
        id: MaskValidityClosureId,
        mask: &Mask,
        close_at_world_time: WorldTime,
        created_revision: Revision,
    ) -> Result<Self, MaskRecordError> {
        validate_lifecycle_revision(mask.created_revision(), created_revision)?;
        if let Some(validity) = mask.validity() {
            ensure_same_timeline(validity, close_at_world_time)
                .map_err(MaskRecordError::CloseTime)?;
        }
        Ok(Self {
            id,
            mask_id: mask.id(),
            close_at_world_time,
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks target order and timeline once the Mask record is loaded.
    pub(crate) const fn from_wire_fields(
        id: MaskValidityClosureId,
        mask_id: MaskId,
        close_at_world_time: WorldTime,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            mask_id,
            close_at_world_time,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(self) -> MaskValidityClosureId {
        self.id
    }

    /// Returns the target Mask identity.
    #[must_use]
    pub const fn mask_id(self) -> MaskId {
        self.mask_id
    }

    /// Returns the exclusive world-time end introduced by this record.
    #[must_use]
    pub const fn close_at_world_time(self) -> WorldTime {
        self.close_at_world_time
    }

    /// Returns the Transaction-Time revision that created this record.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// One immutable Transaction-Time retraction of a Mask.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MaskRetraction {
    id: MaskRetractionId,
    mask_id: MaskId,
    reason: String,
    created_revision: Revision,
}

impl MaskRetraction {
    /// Creates a retraction after its target Mask.
    pub fn new(
        id: MaskRetractionId,
        mask: &Mask,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, MaskRecordError> {
        validate_lifecycle_revision(mask.created_revision(), created_revision)?;
        Ok(Self {
            id,
            mask_id: mask.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target exists and predates this retraction.
    pub(crate) fn from_wire_fields(
        id: MaskRetractionId,
        mask_id: MaskId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            mask_id,
            reason,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(&self) -> MaskRetractionId {
        self.id
    }

    /// Returns the target Mask identity.
    #[must_use]
    pub const fn mask_id(&self) -> MaskId {
        self.mask_id
    }

    /// Returns the required retraction reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time revision at which this retraction applies.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// One immutable replacement boundary for a MultiValueReplace predicate slot.
///
/// The boundary declares that the selected slot's complete replacement set is
/// authoritative. If it has no Assertion candidates, it still represents an
/// explicit empty set.
#[derive(Clone, Debug)]
pub struct ReplacementBoundary {
    id: ReplacementBoundaryId,
    context: ContextKey,
    subject: Subject,
    predicate_id: PredicateId,
    validity: Option<AssertionValidity>,
    created_revision: Revision,
}

impl ReplacementBoundary {
    /// Creates a boundary only for a published `MultiValueReplace` predicate.
    pub fn new(
        id: ReplacementBoundaryId,
        context: ContextKey,
        subject: Subject,
        predicate: &PredicateDefinition,
        validity: Option<AssertionValidity>,
        created_revision: Revision,
    ) -> Result<Self, ReplacementBoundaryError> {
        if predicate.resolution_policy() != ResolutionPolicy::MultiValueReplace {
            return Err(ReplacementBoundaryError::PolicyNotMultiValueReplace {
                actual: predicate.resolution_policy(),
            });
        }
        if predicate.created_revision() > created_revision {
            return Err(ReplacementBoundaryError::PredicateNotYetPublished {
                predicate_revision: predicate.created_revision(),
                boundary_revision: created_revision,
            });
        }
        Ok(Self {
            id,
            context,
            subject,
            predicate_id: predicate.predicate_id(),
            validity,
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding. The history validator
    /// resolves the Predicate and checks policy and publication revision.
    pub(crate) const fn from_wire_fields(
        id: ReplacementBoundaryId,
        context: ContextKey,
        subject: Subject,
        predicate_id: PredicateId,
        validity: Option<AssertionValidity>,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            context,
            subject,
            predicate_id,
            validity,
            created_revision,
        }
    }

    /// Returns the stable boundary identity.
    #[must_use]
    pub const fn id(&self) -> ReplacementBoundaryId {
        self.id
    }

    /// Returns its concrete HistorySpace/Layer/Perspective/Epistemic context.
    #[must_use]
    pub const fn context(&self) -> ContextKey {
        self.context
    }

    /// Returns the selected subject.
    #[must_use]
    pub const fn subject(&self) -> Subject {
        self.subject
    }

    /// Returns the predicate identity validated against the supplied schema definition.
    #[must_use]
    pub const fn predicate_id(&self) -> PredicateId {
        self.predicate_id
    }

    /// Returns the optional world-time validity interval.
    #[must_use]
    pub const fn validity(&self) -> Option<AssertionValidity> {
        self.validity
    }

    /// Returns the Transaction-Time revision that created this boundary.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// One immutable record ending a ReplacementBoundary's world-time validity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ReplacementBoundaryValidityClosure {
    id: ReplacementBoundaryValidityClosureId,
    replacement_boundary_id: ReplacementBoundaryId,
    close_at_world_time: WorldTime,
    created_revision: Revision,
}

impl ReplacementBoundaryValidityClosure {
    /// Creates a closure after its target Boundary, on its validity timeline when explicit.
    pub fn new(
        id: ReplacementBoundaryValidityClosureId,
        boundary: &ReplacementBoundary,
        close_at_world_time: WorldTime,
        created_revision: Revision,
    ) -> Result<Self, ReplacementBoundaryError> {
        validate_boundary_lifecycle_revision(boundary.created_revision(), created_revision)?;
        if let Some(validity) = boundary.validity() {
            ensure_same_timeline(validity, close_at_world_time)
                .map_err(ReplacementBoundaryError::CloseTime)?;
        }
        Ok(Self {
            id,
            replacement_boundary_id: boundary.id(),
            close_at_world_time,
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks target order and timeline once the Boundary is loaded.
    pub(crate) const fn from_wire_fields(
        id: ReplacementBoundaryValidityClosureId,
        replacement_boundary_id: ReplacementBoundaryId,
        close_at_world_time: WorldTime,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            replacement_boundary_id,
            close_at_world_time,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(self) -> ReplacementBoundaryValidityClosureId {
        self.id
    }

    /// Returns the target ReplacementBoundary identity.
    #[must_use]
    pub const fn replacement_boundary_id(self) -> ReplacementBoundaryId {
        self.replacement_boundary_id
    }

    /// Returns the exclusive world-time end introduced by this record.
    #[must_use]
    pub const fn close_at_world_time(self) -> WorldTime {
        self.close_at_world_time
    }

    /// Returns the Transaction-Time revision that created this record.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// One immutable Transaction-Time retraction of a ReplacementBoundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ReplacementBoundaryRetraction {
    id: ReplacementBoundaryRetractionId,
    replacement_boundary_id: ReplacementBoundaryId,
    reason: String,
    created_revision: Revision,
}

impl ReplacementBoundaryRetraction {
    /// Creates a retraction after its target Boundary.
    pub fn new(
        id: ReplacementBoundaryRetractionId,
        boundary: &ReplacementBoundary,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, ReplacementBoundaryError> {
        validate_boundary_lifecycle_revision(boundary.created_revision(), created_revision)?;
        Ok(Self {
            id,
            replacement_boundary_id: boundary.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target exists and predates this retraction.
    pub(crate) fn from_wire_fields(
        id: ReplacementBoundaryRetractionId,
        replacement_boundary_id: ReplacementBoundaryId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            replacement_boundary_id,
            reason,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(&self) -> ReplacementBoundaryRetractionId {
        self.id
    }

    /// Returns the target ReplacementBoundary identity.
    #[must_use]
    pub const fn replacement_boundary_id(&self) -> ReplacementBoundaryId {
        self.replacement_boundary_id
    }

    /// Returns the required retraction reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the Transaction-Time revision at which this retraction applies.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }
}

/// A rejected Mask selector, context, or lifecycle value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaskRecordError {
    /// WorldState requires World; other epistemic modes require a Perspective.
    InvalidSlotPerspectiveModePair {
        /// Requested Perspective partition.
        perspective_scope: PerspectiveScope,
        /// Requested Epistemic mode.
        epistemic_mode: EpistemicMode,
    },
    /// A Slot selector targets another Perspective/Epistemic partition.
    SlotContextMismatch {
        /// Scope stored on the Mask record context.
        mask_scope: PerspectiveScope,
        /// Mode stored on the Mask record context.
        mask_mode: EpistemicMode,
        /// Scope selected by the Slot selector.
        target_scope: PerspectiveScope,
        /// Mode selected by the Slot selector.
        target_mode: EpistemicMode,
    },
    /// A lifecycle record must be created after its target Mask.
    LifecycleRevisionNotAfterMask {
        /// The target Mask's creation revision.
        mask_revision: Revision,
        /// The proposed lifecycle record's creation revision.
        lifecycle_revision: Revision,
    },
    /// A closure time belongs to another timeline than the Mask validity.
    CloseTime(TemporalError),
}

impl fmt::Display for MaskRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSlotPerspectiveModePair {
                perspective_scope,
                epistemic_mode,
            } => write!(
                formatter,
                "epistemic mode {epistemic_mode:?} is incompatible with scope {perspective_scope:?}"
            ),
            Self::SlotContextMismatch {
                mask_scope,
                mask_mode,
                target_scope,
                target_mode,
            } => write!(
                formatter,
                "mask context {mask_scope:?}/{mask_mode:?} cannot target slot {target_scope:?}/{target_mode:?}"
            ),
            Self::LifecycleRevisionNotAfterMask {
                mask_revision,
                lifecycle_revision,
            } => write!(
                formatter,
                "mask lifecycle revision {lifecycle_revision} must be after mask revision {mask_revision}"
            ),
            Self::CloseTime(error) => write!(formatter, "invalid mask close time: {error}"),
        }
    }
}

impl std::error::Error for MaskRecordError {}

/// A rejected ReplacementBoundary schema or lifecycle value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReplacementBoundaryError {
    /// The supplied Predicate does not use MultiValueReplace.
    PolicyNotMultiValueReplace {
        /// The actual schema resolution policy.
        actual: ResolutionPolicy,
    },
    /// The supplied Predicate was not yet published at the boundary revision.
    PredicateNotYetPublished {
        /// The Predicate schema revision.
        predicate_revision: Revision,
        /// The proposed boundary creation revision.
        boundary_revision: Revision,
    },
    /// A lifecycle record must be created after its target boundary.
    LifecycleRevisionNotAfterBoundary {
        /// The target boundary creation revision.
        boundary_revision: Revision,
        /// The proposed lifecycle record creation revision.
        lifecycle_revision: Revision,
    },
    /// A closure time belongs to another timeline than the boundary validity.
    CloseTime(TemporalError),
}

impl fmt::Display for ReplacementBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyNotMultiValueReplace { actual } => write!(
                formatter,
                "replacement boundary requires MultiValueReplace, got {actual:?}"
            ),
            Self::PredicateNotYetPublished {
                predicate_revision,
                boundary_revision,
            } => write!(
                formatter,
                "predicate revision {predicate_revision} is after boundary revision {boundary_revision}"
            ),
            Self::LifecycleRevisionNotAfterBoundary {
                boundary_revision,
                lifecycle_revision,
            } => write!(
                formatter,
                "boundary lifecycle revision {lifecycle_revision} must be after boundary revision {boundary_revision}"
            ),
            Self::CloseTime(error) => {
                write!(formatter, "invalid boundary close time: {error}")
            }
        }
    }
}

impl std::error::Error for ReplacementBoundaryError {}

fn valid_partition_pair(scope: PerspectiveScope, mode: EpistemicMode) -> bool {
    matches!(
        (scope, mode),
        (PerspectiveScope::World, EpistemicMode::WorldState)
            | (
                PerspectiveScope::Perspective(_),
                EpistemicMode::Knows | EpistemicMode::Believes | EpistemicMode::Claims
            )
    )
}

fn validate_lifecycle_revision(
    mask_revision: Revision,
    lifecycle_revision: Revision,
) -> Result<(), MaskRecordError> {
    if lifecycle_revision <= mask_revision {
        return Err(MaskRecordError::LifecycleRevisionNotAfterMask {
            mask_revision,
            lifecycle_revision,
        });
    }
    Ok(())
}

fn validate_boundary_lifecycle_revision(
    boundary_revision: Revision,
    lifecycle_revision: Revision,
) -> Result<(), ReplacementBoundaryError> {
    if lifecycle_revision <= boundary_revision {
        return Err(
            ReplacementBoundaryError::LifecycleRevisionNotAfterBoundary {
                boundary_revision,
                lifecycle_revision,
            },
        );
    }
    Ok(())
}

fn ensure_same_timeline(
    validity: AssertionValidity,
    close_at_world_time: WorldTime,
) -> Result<(), TemporalError> {
    let expected = validity.interval().timeline().id();
    let actual = close_at_world_time.timeline().id();
    if expected == actual {
        Ok(())
    } else {
        Err(TemporalError::IncomparableTimelines { expected, actual })
    }
}

impl From<ContextError> for MaskRecordError {
    fn from(error: ContextError) -> Self {
        match error {
            ContextError::InvalidPerspectiveModePair {
                perspective_scope,
                epistemic_mode,
            } => Self::InvalidSlotPerspectiveModePair {
                perspective_scope,
                epistemic_mode,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::{
        Mask, MaskRecordError, MaskRetraction, MaskSelector, MaskSlotSelector, MaskValidityClosure,
        PropositionKey, ReplacementBoundary, ReplacementBoundaryError,
        ReplacementBoundaryRetraction, ReplacementBoundaryValidityClosure,
    };
    use crate::assertions::{Polarity, Subject};
    use crate::context::{ContextError, ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{
        AssertionId, DomainId, EntityId, HistorySpaceId, IdValidationError, LayerId, MaskId,
        MaskRetractionId, MaskValidityClosureId, PerspectiveId, PredicateId, ReplacementBoundaryId,
        ReplacementBoundaryRetractionId, ReplacementBoundaryValidityClosureId, Revision,
        RevisionError, TimelineId,
    };
    use crate::schema::{
        Cardinality, ConstraintSet, EntityTypeConstraint, Lifecycle, PredicateDefinition,
        PredicateDefinitionSpec, ResolutionPolicy, SchemaDefinitionError, ValueKind,
    };
    use crate::temporal::{AssertionValidity, TemporalError, TimeInterval, Timeline, WorldTime};
    use crate::values::{Symbol, SymbolError, Value};

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Boundary(ReplacementBoundaryError),
        Context(ContextError),
        Id(IdValidationError),
        Mask(MaskRecordError),
        Revision(RevisionError),
        Schema(SchemaDefinitionError),
        Symbol(SymbolError),
        Temporal(TemporalError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Boundary(error) => write!(formatter, "{error}"),
                Self::Context(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Mask(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Schema(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
                Self::Temporal(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl Error for TestError {}

    macro_rules! error_conversion {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    error_conversion!(ReplacementBoundaryError, Boundary);
    error_conversion!(ContextError, Context);
    error_conversion!(IdValidationError, Id);
    error_conversion!(MaskRecordError, Mask);
    error_conversion!(RevisionError, Revision);
    error_conversion!(SchemaDefinitionError, Schema);
    error_conversion!(SymbolError, Symbol);
    error_conversion!(TemporalError, Temporal);

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    fn uuid<T: DomainId>(byte: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn context() -> Result<ContextKey, TestError> {
        Ok(value!(ContextKey::new(
            value!(uuid::<HistorySpaceId>(1)),
            value!(uuid::<LayerId>(2)),
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )))
    }

    fn timeline(byte: u8) -> Result<Timeline, TestError> {
        Ok(Timeline::new(value!(uuid::<TimelineId>(byte))))
    }

    fn validity(selected_timeline: Timeline) -> Result<AssertionValidity, TestError> {
        let start = WorldTime::from_nanoseconds(selected_timeline, 10);
        Ok(AssertionValidity::new(value!(TimeInterval::new(
            selected_timeline,
            Some(start),
            None,
        ))))
    }

    fn predicate(
        byte: u8,
        cardinality: Cardinality,
        resolution_policy: ResolutionPolicy,
        created_revision: Revision,
    ) -> Result<PredicateDefinition, TestError> {
        let predicate_id = value!(uuid::<PredicateId>(byte));
        let symbol = value!(Symbol::new(format!("predicate_{byte}")));
        Ok(value!(PredicateDefinition::new(PredicateDefinitionSpec {
            predicate_id,
            symbol,
            subject_constraint: EntityTypeConstraint::AnyEntity,
            value_kind: ValueKind::String,
            object_constraint: None,
            cardinality,
            resolution_policy,
            constraints: ConstraintSet::unconstrained(),
            decimal_metadata: None,
            lifecycle: Lifecycle::Active,
            created_revision,
        })))
    }

    #[test]
    fn mask_selectors_are_closed_and_slot_masks_stay_in_one_partition() -> TestResult {
        let mask_id = value!(uuid::<MaskId>(3));
        let subject = Subject::new(value!(uuid::<EntityId>(4)));
        let predicate_id = value!(uuid::<PredicateId>(5));
        let exact_assertion_id = value!(uuid::<AssertionId>(6));
        let context = context()?;
        let created_revision = value!(revision(3));

        let exact = value!(Mask::new(
            mask_id,
            context,
            MaskSelector::ExactAssertion(exact_assertion_id),
            None,
            created_revision,
        ));
        assert!(matches!(
            exact.selector(),
            MaskSelector::ExactAssertion(id) if *id == exact_assertion_id
        ));

        let proposition = PropositionKey::new(
            subject,
            predicate_id,
            Value::String(String::from("value")),
            Polarity::Negative,
        );
        assert_eq!(proposition.subject(), subject);
        assert_eq!(proposition.predicate_id(), predicate_id);
        assert_eq!(proposition.polarity(), Polarity::Negative);
        assert!(matches!(proposition.value(), Value::String(value) if value == "value"));

        let slot = value!(MaskSlotSelector::new(
            subject,
            predicate_id,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        ));
        let slot_mask = value!(Mask::new(
            mask_id,
            context,
            MaskSelector::Slot(slot),
            None,
            created_revision,
        ));
        assert!(matches!(slot_mask.selector(), MaskSelector::Slot(_)));

        let perspective_id = value!(uuid::<PerspectiveId>(7));
        assert_eq!(
            MaskSlotSelector::new(
                subject,
                predicate_id,
                PerspectiveScope::Perspective(perspective_id),
                EpistemicMode::WorldState,
            ),
            Err(MaskRecordError::InvalidSlotPerspectiveModePair {
                perspective_scope: PerspectiveScope::Perspective(perspective_id),
                epistemic_mode: EpistemicMode::WorldState,
            })
        );

        let other_partition = value!(MaskSlotSelector::new(
            subject,
            predicate_id,
            PerspectiveScope::Perspective(perspective_id),
            EpistemicMode::Believes,
        ));
        assert_eq!(
            Mask::new(
                mask_id,
                context,
                MaskSelector::Slot(other_partition),
                None,
                created_revision,
            )
            .err(),
            Some(MaskRecordError::SlotContextMismatch {
                mask_scope: PerspectiveScope::World,
                mask_mode: EpistemicMode::WorldState,
                target_scope: PerspectiveScope::Perspective(perspective_id),
                target_mode: EpistemicMode::Believes,
            })
        );
        Ok(())
    }

    #[test]
    fn replacement_boundary_requires_multivalue_replace_and_can_be_empty() -> TestResult {
        let context = context()?;
        let subject = Subject::new(value!(uuid::<EntityId>(8)));
        let created_revision = value!(revision(3));
        let boundary_id = value!(uuid::<ReplacementBoundaryId>(9));

        let single = predicate(
            10,
            Cardinality::Single,
            ResolutionPolicy::SingleValueReplace,
            Revision::GENESIS,
        )?;
        assert_eq!(
            ReplacementBoundary::new(
                boundary_id,
                context,
                subject,
                &single,
                None,
                created_revision,
            )
            .err(),
            Some(ReplacementBoundaryError::PolicyNotMultiValueReplace {
                actual: ResolutionPolicy::SingleValueReplace,
            })
        );

        let overlay = predicate(
            11,
            Cardinality::Multi,
            ResolutionPolicy::MultiValueOverlay,
            Revision::GENESIS,
        )?;
        assert_eq!(
            ReplacementBoundary::new(
                boundary_id,
                context,
                subject,
                &overlay,
                None,
                created_revision,
            )
            .err(),
            Some(ReplacementBoundaryError::PolicyNotMultiValueReplace {
                actual: ResolutionPolicy::MultiValueOverlay,
            })
        );

        let replace = predicate(
            12,
            Cardinality::Multi,
            ResolutionPolicy::MultiValueReplace,
            Revision::GENESIS,
        )?;
        let empty_set_boundary = value!(ReplacementBoundary::new(
            boundary_id,
            context,
            subject,
            &replace,
            None,
            created_revision,
        ));
        assert_eq!(empty_set_boundary.predicate_id(), replace.predicate_id());
        assert_eq!(empty_set_boundary.validity(), None);

        let future_schema = predicate(
            13,
            Cardinality::Multi,
            ResolutionPolicy::MultiValueReplace,
            value!(revision(4)),
        )?;
        assert_eq!(
            ReplacementBoundary::new(
                boundary_id,
                context,
                subject,
                &future_schema,
                None,
                created_revision,
            )
            .err(),
            Some(ReplacementBoundaryError::PredicateNotYetPublished {
                predicate_revision: value!(revision(4)),
                boundary_revision: created_revision,
            })
        );
        Ok(())
    }

    #[test]
    fn mask_and_boundary_lifecycle_records_keep_distinct_typed_targets() -> TestResult {
        let context = context()?;
        let subject = Subject::new(value!(uuid::<EntityId>(14)));
        let created_revision = value!(revision(3));
        let lifecycle_revision = value!(revision(4));
        let timeline = timeline(15)?;
        let mask = value!(Mask::new(
            value!(uuid::<MaskId>(16)),
            context,
            MaskSelector::ExactAssertion(value!(uuid::<AssertionId>(17))),
            Some(validity(timeline)?),
            created_revision,
        ));
        let mask_close = value!(MaskValidityClosure::new(
            value!(uuid::<MaskValidityClosureId>(18)),
            &mask,
            WorldTime::from_nanoseconds(timeline, 20),
            lifecycle_revision,
        ));
        let mask_retraction = value!(MaskRetraction::new(
            value!(uuid::<MaskRetractionId>(19)),
            &mask,
            "mask correction",
            lifecycle_revision,
        ));
        assert_eq!(mask_close.mask_id(), mask.id());
        assert_eq!(mask_retraction.mask_id(), mask.id());
        assert_eq!(mask_retraction.reason(), "mask correction");

        let predicate = predicate(
            20,
            Cardinality::Multi,
            ResolutionPolicy::MultiValueReplace,
            Revision::GENESIS,
        )?;
        let boundary = value!(ReplacementBoundary::new(
            value!(uuid::<ReplacementBoundaryId>(21)),
            context,
            subject,
            &predicate,
            Some(validity(timeline)?),
            created_revision,
        ));
        let boundary_close = value!(ReplacementBoundaryValidityClosure::new(
            value!(uuid::<ReplacementBoundaryValidityClosureId>(22)),
            &boundary,
            WorldTime::from_nanoseconds(timeline, 20),
            lifecycle_revision,
        ));
        let boundary_retraction = value!(ReplacementBoundaryRetraction::new(
            value!(uuid::<ReplacementBoundaryRetractionId>(23)),
            &boundary,
            "boundary correction",
            lifecycle_revision,
        ));
        assert_eq!(boundary_close.replacement_boundary_id(), boundary.id());
        assert_eq!(boundary_retraction.replacement_boundary_id(), boundary.id());
        assert_eq!(boundary_retraction.reason(), "boundary correction");

        let earlier_revision = value!(revision(2));
        assert_eq!(
            MaskValidityClosure::new(
                value!(uuid::<MaskValidityClosureId>(24)),
                &mask,
                WorldTime::from_nanoseconds(timeline, 20),
                earlier_revision,
            )
            .err(),
            Some(MaskRecordError::LifecycleRevisionNotAfterMask {
                mask_revision: created_revision,
                lifecycle_revision: earlier_revision,
            })
        );
        assert_eq!(
            ReplacementBoundaryRetraction::new(
                value!(uuid::<ReplacementBoundaryRetractionId>(25)),
                &boundary,
                "too early",
                earlier_revision,
            )
            .err(),
            Some(
                ReplacementBoundaryError::LifecycleRevisionNotAfterBoundary {
                    boundary_revision: created_revision,
                    lifecycle_revision: earlier_revision,
                }
            )
        );
        Ok(())
    }
}
