//! Immutable assertion and assertion-lifecycle record values.

use std::fmt;

use crate::context::ContextKey;
use crate::ids::{
    AssertionId, AssertionRetractionId, AssertionValidityClosureId, EntityId, PredicateId, Revision,
};
use crate::temporal::{AssertionValidity, TemporalError, WorldTime};
use crate::values::Value;

/// An entity used as the subject of an assertion.
///
/// The wrapper keeps subject identity distinct from an entity-valued predicate
/// value while preserving the schema contract that assertion subjects are
/// entities. Existence, type-constraint, and authorization checks belong to
/// the operation's pinned schema and security context.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Subject(EntityId);

impl Subject {
    /// Binds a typed entity identity as an assertion subject.
    #[must_use]
    pub const fn new(entity_id: EntityId) -> Self {
        Self(entity_id)
    }

    /// Returns the referenced entity identity.
    #[must_use]
    pub const fn entity_id(self) -> EntityId {
        self.0
    }
}

/// Whether an assertion states a proposition or its explicit negation.
///
/// Retraction is not a negative polarity: it is a separate lifecycle record
/// effective on Transaction Time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Polarity {
    /// The proposition is asserted positively.
    Positive,
    /// The proposition is explicitly asserted negatively.
    Negative,
}

/// Immutable input fields for constructing an [`Assertion`].
///
/// The draft has no persistent identity or Transaction-Time revision. Those
/// are assigned when the operation creates the final assertion record.
#[derive(Clone, Debug)]
pub struct AssertionDraft {
    context: ContextKey,
    subject: Subject,
    predicate_id: PredicateId,
    value: Value,
    polarity: Polarity,
    validity: AssertionValidity,
}

impl AssertionDraft {
    /// Creates a complete typed assertion payload.
    #[must_use]
    pub const fn new(
        context: ContextKey,
        subject: Subject,
        predicate_id: PredicateId,
        value: Value,
        polarity: Polarity,
        validity: AssertionValidity,
    ) -> Self {
        Self {
            context,
            subject,
            predicate_id,
            value,
            polarity,
            validity,
        }
    }
}

/// One immutable assertion history record.
///
/// There are no mutation methods. A correction creates another `Assertion`
/// and a separate `AssertionRetraction`; ending world-time validity creates a
/// separate `AssertionValidityClosure`. Schema validation of the value,
/// subject existence, and references is performed at the later operation
/// boundary.
#[derive(Clone, Debug)]
pub struct Assertion {
    id: AssertionId,
    context: ContextKey,
    subject: Subject,
    predicate_id: PredicateId,
    value: Value,
    polarity: Polarity,
    validity: AssertionValidity,
    created_revision: Revision,
}

impl Assertion {
    /// Creates one immutable assertion record.
    #[must_use]
    pub fn new(id: AssertionId, draft: AssertionDraft, created_revision: Revision) -> Self {
        Self {
            id,
            context: draft.context,
            subject: draft.subject,
            predicate_id: draft.predicate_id,
            value: draft.value,
            polarity: draft.polarity,
            validity: draft.validity,
            created_revision,
        }
    }

    /// Returns the stable assertion identity.
    #[must_use]
    pub const fn id(&self) -> AssertionId {
        self.id
    }

    /// Returns the explicit HistorySpace/Layer/Perspective/Epistemic context.
    #[must_use]
    pub const fn context(&self) -> ContextKey {
        self.context
    }

    /// Returns the subject entity.
    #[must_use]
    pub const fn subject(&self) -> Subject {
        self.subject
    }

    /// Returns the stable predicate identity.
    #[must_use]
    pub const fn predicate_id(&self) -> PredicateId {
        self.predicate_id
    }

    /// Returns the typed assertion value.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    /// Returns the explicit proposition polarity.
    #[must_use]
    pub const fn polarity(&self) -> Polarity {
        self.polarity
    }

    /// Returns the immutable world-time validity interval.
    #[must_use]
    pub const fn validity(&self) -> AssertionValidity {
        self.validity
    }

    /// Returns the Transaction-Time revision that created this record.
    #[must_use]
    pub const fn created_revision(&self) -> Revision {
        self.created_revision
    }

    /// Projects exactly the Proposition Equality components.
    ///
    /// Context, assertion identity, validity, and creation revision are not
    /// part of Proposition Equality. `Value` comparison is deliberately left
    /// to the schema-aware comparison path because time values require unit
    /// resolution on their registered timeline.
    #[must_use]
    pub const fn proposition_components(&self) -> (Subject, PredicateId, &Value, Polarity) {
        (self.subject, self.predicate_id, &self.value, self.polarity)
    }
}

/// One immutable record that ends an assertion's world-time validity.
///
/// The target assertion remains untouched. The closure is effective only as
/// interpreted by a later historical view; its own creation revision remains
/// Transaction Time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AssertionValidityClosure {
    id: AssertionValidityClosureId,
    assertion_id: AssertionId,
    close_at_world_time: WorldTime,
    created_revision: Revision,
}

impl AssertionValidityClosure {
    /// Creates a closure for an existing assertion at a later Transaction-Time revision.
    pub fn new(
        id: AssertionValidityClosureId,
        assertion: &Assertion,
        close_at_world_time: WorldTime,
        created_revision: Revision,
    ) -> Result<Self, AssertionRecordError> {
        validate_lifecycle_revision(assertion, created_revision)?;
        let expected_timeline = assertion.validity().interval().timeline().id();
        let actual_timeline = close_at_world_time.timeline().id();
        if expected_timeline != actual_timeline {
            return Err(AssertionRecordError::CloseTime(
                TemporalError::IncomparableTimelines {
                    expected: expected_timeline,
                    actual: actual_timeline,
                },
            ));
        }

        Ok(Self {
            id,
            assertion_id: assertion.id(),
            close_at_world_time,
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks the target revision and timeline once the target record is loaded.
    pub(crate) const fn from_wire_fields(
        id: AssertionValidityClosureId,
        assertion_id: AssertionId,
        close_at_world_time: WorldTime,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            assertion_id,
            close_at_world_time,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(self) -> AssertionValidityClosureId {
        self.id
    }

    /// Returns the assertion whose world-time validity is closed.
    #[must_use]
    pub const fn assertion_id(self) -> AssertionId {
        self.assertion_id
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

/// One immutable record that retracts an assertion on Transaction Time.
///
/// The target assertion remains present in raw history and is not edited. This
/// record has no WorldTime field: its effective point is `created_revision`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AssertionRetraction {
    id: AssertionRetractionId,
    assertion_id: AssertionId,
    reason: String,
    created_revision: Revision,
}

impl AssertionRetraction {
    /// Creates a retraction for an existing assertion at a later Transaction-Time revision.
    pub fn new(
        id: AssertionRetractionId,
        assertion: &Assertion,
        reason: impl Into<String>,
        created_revision: Revision,
    ) -> Result<Self, AssertionRecordError> {
        validate_lifecycle_revision(assertion, created_revision)?;
        Ok(Self {
            id,
            assertion_id: assertion.id(),
            reason: reason.into(),
            created_revision,
        })
    }

    /// Reconstructs stored fields after scalar decoding; the history validator
    /// checks that the target exists and predates this retraction.
    pub(crate) fn from_wire_fields(
        id: AssertionRetractionId,
        assertion_id: AssertionId,
        reason: String,
        created_revision: Revision,
    ) -> Self {
        Self {
            id,
            assertion_id,
            reason,
            created_revision,
        }
    }

    /// Returns this concrete lifecycle-record identity.
    #[must_use]
    pub const fn id(&self) -> AssertionRetractionId {
        self.id
    }

    /// Returns the assertion that this record retracts.
    #[must_use]
    pub const fn assertion_id(&self) -> AssertionId {
        self.assertion_id
    }

    /// Returns the required explanation for this retraction.
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

fn validate_lifecycle_revision(
    assertion: &Assertion,
    created_revision: Revision,
) -> Result<(), AssertionRecordError> {
    if created_revision <= assertion.created_revision() {
        return Err(AssertionRecordError::LifecycleRevisionNotAfterAssertion {
            assertion_revision: assertion.created_revision(),
            lifecycle_revision: created_revision,
        });
    }
    Ok(())
}

/// A rejected lifecycle record construction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AssertionRecordError {
    /// A lifecycle record must be created after its target Assertion.
    LifecycleRevisionNotAfterAssertion {
        /// The target Assertion's creation revision.
        assertion_revision: Revision,
        /// The proposed lifecycle record's creation revision.
        lifecycle_revision: Revision,
    },
    /// The closure time belongs to a different timeline than target validity.
    CloseTime(TemporalError),
}

impl fmt::Display for AssertionRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LifecycleRevisionNotAfterAssertion {
                assertion_revision,
                lifecycle_revision,
            } => write!(
                formatter,
                "lifecycle revision {lifecycle_revision} must be after assertion revision {assertion_revision}"
            ),
            Self::CloseTime(error) => write!(formatter, "invalid assertion close time: {error}"),
        }
    }
}

impl std::error::Error for AssertionRecordError {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::{
        Assertion, AssertionDraft, AssertionRecordError, AssertionRetraction,
        AssertionValidityClosure, Polarity, Subject,
    };
    use crate::context::{ContextError, ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{
        AssertionId, AssertionRetractionId, AssertionValidityClosureId, DomainId, EntityId,
        HistorySpaceId, IdValidationError, LayerId, PredicateId, Revision, RevisionError,
        TimelineId,
    };
    use crate::temporal::{AssertionValidity, TemporalError, TimeInterval, Timeline, WorldTime};
    use crate::values::Value;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Assertion(AssertionRecordError),
        Context(ContextError),
        Id(IdValidationError),
        Revision(RevisionError),
        Temporal(TemporalError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Assertion(error) => write!(formatter, "{error}"),
                Self::Context(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
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

    error_conversion!(AssertionRecordError, Assertion);
    error_conversion!(ContextError, Context);
    error_conversion!(IdValidationError, Id);
    error_conversion!(RevisionError, Revision);
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

    fn assertion_fixture() -> Result<Assertion, TestError> {
        let assertion_id = value!(uuid::<AssertionId>(1));
        let history_space_id = value!(uuid::<HistorySpaceId>(2));
        let layer_id = value!(uuid::<LayerId>(3));
        let entity_id = value!(uuid::<EntityId>(4));
        let predicate_id = value!(uuid::<PredicateId>(5));
        let timeline = Timeline::new(value!(uuid::<TimelineId>(6)));
        let context = value!(ContextKey::new(
            history_space_id,
            layer_id,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        ));
        let start = WorldTime::from_nanoseconds(timeline, 10);
        let validity =
            AssertionValidity::new(value!(TimeInterval::new(timeline, Some(start), None,)));

        Ok(Assertion::new(
            assertion_id,
            AssertionDraft::new(
                context,
                Subject::new(entity_id),
                predicate_id,
                Value::String(String::from("value")),
                Polarity::Positive,
                validity,
            ),
            value!(revision(3)),
        ))
    }

    #[test]
    fn closure_and_retraction_are_additional_records_on_distinct_axes() -> TestResult {
        let assertion = assertion_fixture()?;
        let original_id = assertion.id();
        let original_revision = assertion.created_revision();
        let original_end = assertion.validity().interval().end();
        let timeline = assertion.validity().interval().timeline();
        let close_id = value!(uuid::<AssertionValidityClosureId>(7));
        let retraction_id = value!(uuid::<AssertionRetractionId>(8));
        let lifecycle_revision = value!(revision(4));

        let closure = AssertionValidityClosure::new(
            close_id,
            &assertion,
            WorldTime::from_nanoseconds(timeline, 20),
            lifecycle_revision,
        )?;
        let retraction = AssertionRetraction::new(
            retraction_id,
            &assertion,
            "source correction",
            lifecycle_revision,
        )?;

        assert_eq!(closure.assertion_id(), original_id);
        assert_eq!(closure.created_revision(), lifecycle_revision);
        assert_eq!(retraction.assertion_id(), original_id);
        assert_eq!(retraction.created_revision(), lifecycle_revision);
        assert_eq!(retraction.reason(), "source correction");
        assert_eq!(assertion.id(), original_id);
        assert_eq!(assertion.created_revision(), original_revision);
        assert_eq!(assertion.validity().interval().end(), original_end);
        assert!(matches!(assertion.value(), Value::String(value) if value == "value"));
        Ok(())
    }

    #[test]
    fn lifecycle_records_reject_non_later_revisions_and_cross_timeline_closure() -> TestResult {
        let assertion = assertion_fixture()?;
        let close_id = value!(uuid::<AssertionValidityClosureId>(9));
        let retraction_id = value!(uuid::<AssertionRetractionId>(10));
        let assertion_revision = assertion.created_revision();
        let same_revision = value!(revision(assertion_revision.value()));
        let timeline = assertion.validity().interval().timeline();

        assert_eq!(
            AssertionValidityClosure::new(
                close_id,
                &assertion,
                WorldTime::from_nanoseconds(timeline, 20),
                same_revision,
            ),
            Err(AssertionRecordError::LifecycleRevisionNotAfterAssertion {
                assertion_revision,
                lifecycle_revision: same_revision,
            })
        );
        assert_eq!(
            AssertionRetraction::new(retraction_id, &assertion, "reason", same_revision,),
            Err(AssertionRecordError::LifecycleRevisionNotAfterAssertion {
                assertion_revision,
                lifecycle_revision: same_revision,
            })
        );

        let other_timeline = Timeline::new(value!(uuid::<TimelineId>(11)));
        assert_eq!(
            AssertionValidityClosure::new(
                close_id,
                &assertion,
                WorldTime::from_nanoseconds(other_timeline, 20),
                value!(revision(4)),
            ),
            Err(AssertionRecordError::CloseTime(
                TemporalError::IncomparableTimelines {
                    expected: timeline.id(),
                    actual: other_timeline.id(),
                }
            ))
        );
        Ok(())
    }

    #[test]
    fn proposition_projection_contains_exactly_the_equality_components() -> TestResult {
        let assertion = assertion_fixture()?;
        let (subject, predicate, value, polarity) = assertion.proposition_components();

        assert_eq!(subject, assertion.subject());
        assert_eq!(predicate, assertion.predicate_id());
        assert_eq!(polarity, assertion.polarity());
        assert!(matches!(value, Value::String(value) if value == "value"));
        Ok(())
    }

    #[test]
    fn proposition_projection_excludes_context_identity_validity_and_revision() -> TestResult {
        let original = assertion_fixture()?;
        let timeline = original.validity().interval().timeline();
        let other_context = value!(ContextKey::new(
            value!(uuid::<HistorySpaceId>(21)),
            value!(uuid::<LayerId>(22)),
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        ));
        let other_validity = AssertionValidity::new(value!(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, 30)),
            None,
        )));
        let other = Assertion::new(
            value!(uuid::<AssertionId>(23)),
            AssertionDraft::new(
                other_context,
                original.subject(),
                original.predicate_id(),
                original.value().clone(),
                original.polarity(),
                other_validity,
            ),
            value!(revision(4)),
        );

        assert_ne!(original.id(), other.id());
        assert_ne!(original.context(), other.context());
        assert_ne!(
            original.validity().interval().start(),
            other.validity().interval().start()
        );
        assert_ne!(original.created_revision(), other.created_revision());

        let (original_subject, original_predicate, original_value, original_polarity) =
            original.proposition_components();
        let (other_subject, other_predicate, other_value, other_polarity) =
            other.proposition_components();
        assert_eq!(original_subject, other_subject);
        assert_eq!(original_predicate, other_predicate);
        assert_eq!(original_polarity, other_polarity);
        assert!(matches!(original_value, Value::String(value) if value == "value"));
        assert!(matches!(other_value, Value::String(value) if value == "value"));
        Ok(())
    }
}
