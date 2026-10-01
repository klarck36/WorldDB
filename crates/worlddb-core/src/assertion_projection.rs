//! Index-free assertion validity and lifecycle projection for M2 reference reads.

use std::collections::BTreeSet;
use std::fmt;

use crate::assertions::{Assertion, AssertionRetraction, AssertionValidityClosure};
use crate::ids::{AssertionId, AssertionRetractionId, AssertionValidityClosureId};
use crate::temporal::{RecordedAsOf, TemporalError, WorldTime};

/// Validated immutable assertion history used by a slow reference projection.
///
/// This model deliberately scans assertions, closures, and retractions. It
/// preserves raw history and does not merge archive state or resolution rules.
#[derive(Clone, Debug, Default)]
pub struct AssertionLifecycleProjection {
    assertions: Vec<Assertion>,
    validity_closures: Vec<AssertionValidityClosure>,
    retractions: Vec<AssertionRetraction>,
}

impl AssertionLifecycleProjection {
    /// Validates and retains one immutable set of assertion and lifecycle records.
    pub fn new(
        mut assertions: Vec<Assertion>,
        mut validity_closures: Vec<AssertionValidityClosure>,
        mut retractions: Vec<AssertionRetraction>,
    ) -> Result<Self, AssertionProjectionError> {
        assertions.sort_by_key(Assertion::id);
        validity_closures.sort_by_key(|record| record.id());
        retractions.sort_by_key(|record| record.id());

        let mut assertion_ids = BTreeSet::new();
        for assertion in &assertions {
            if !assertion_ids.insert(assertion.id()) {
                return Err(AssertionProjectionError::DuplicateAssertion {
                    assertion_id: assertion.id(),
                });
            }
        }

        let assertions_by_id = assertions
            .iter()
            .map(|assertion| (assertion.id(), assertion))
            .collect::<std::collections::BTreeMap<_, _>>();

        let mut closure_ids = BTreeSet::new();
        for closure in &validity_closures {
            if !closure_ids.insert(closure.id()) {
                return Err(AssertionProjectionError::DuplicateValidityClosure {
                    closure_id: closure.id(),
                });
            }
            let assertion = assertions_by_id.get(&closure.assertion_id()).ok_or(
                AssertionProjectionError::UnknownClosureTarget {
                    closure_id: closure.id(),
                    assertion_id: closure.assertion_id(),
                },
            )?;
            validate_lifecycle_revision(
                assertion,
                closure.created_revision(),
                "AssertionValidityClosure",
            )?;
            validate_closure_range(assertion, *closure)?;
        }

        let mut retraction_ids = BTreeSet::new();
        for retraction in &retractions {
            if !retraction_ids.insert(retraction.id()) {
                return Err(AssertionProjectionError::DuplicateRetraction {
                    retraction_id: retraction.id(),
                });
            }
            let assertion = assertions_by_id.get(&retraction.assertion_id()).ok_or(
                AssertionProjectionError::UnknownRetractionTarget {
                    retraction_id: retraction.id(),
                    assertion_id: retraction.assertion_id(),
                },
            )?;
            validate_lifecycle_revision(
                assertion,
                retraction.created_revision(),
                "AssertionRetraction",
            )?;
        }

        Ok(Self {
            assertions,
            validity_closures,
            retractions,
        })
    }

    /// Returns every assertion active at both the selected RecordedAsOf and WorldTime.
    ///
    /// `RecordedAsOf` controls whether the assertion or lifecycle record existed
    /// in transaction history. `WorldTime` independently controls its
    /// half-open domain validity and any visible validity closure. A visible
    /// retraction removes the assertion at and after its own creation revision.
    pub fn candidates(
        &self,
        recorded_as_of: RecordedAsOf,
        world_time: WorldTime,
    ) -> Result<Vec<&Assertion>, AssertionProjectionError> {
        let revision = recorded_as_of.revision();
        let mut candidates = Vec::new();
        for assertion in &self.assertions {
            if assertion.created_revision() > revision {
                continue;
            }
            if self.retractions.iter().any(|retraction| {
                retraction.assertion_id() == assertion.id()
                    && retraction.created_revision() <= revision
            }) {
                continue;
            }
            if self.validity_closures.iter().any(|closure| {
                closure.assertion_id() == assertion.id()
                    && closure.created_revision() <= revision
                    && world_time.nanoseconds() >= closure.close_at_world_time().nanoseconds()
            }) {
                continue;
            }
            if assertion.validity().contains(world_time)? {
                candidates.push(assertion);
            }
        }
        Ok(candidates)
    }

    /// Returns immutable source assertions in canonical ID order, including inactive history.
    #[must_use]
    pub fn assertions(&self) -> &[Assertion] {
        &self.assertions
    }
}

fn validate_lifecycle_revision(
    assertion: &Assertion,
    lifecycle_revision: crate::ids::Revision,
    record_kind: &'static str,
) -> Result<(), AssertionProjectionError> {
    if lifecycle_revision <= assertion.created_revision() {
        return Err(
            AssertionProjectionError::LifecycleRevisionNotAfterAssertion {
                assertion_id: assertion.id(),
                assertion_revision: assertion.created_revision(),
                lifecycle_revision,
                record: record_kind,
            },
        );
    }
    Ok(())
}

fn validate_closure_range(
    assertion: &Assertion,
    closure: AssertionValidityClosure,
) -> Result<(), AssertionProjectionError> {
    let interval = assertion.validity().interval();
    let close_at = closure.close_at_world_time();
    if interval.timeline().id() != close_at.timeline().id() {
        return Err(TemporalError::IncomparableTimelines {
            expected: interval.timeline().id(),
            actual: close_at.timeline().id(),
        }
        .into());
    }
    if let Some(start) = interval.start() {
        if close_at.checked_cmp(start)? == std::cmp::Ordering::Less {
            return Err(AssertionProjectionError::ClosureOutsideValidity {
                assertion_id: assertion.id(),
                closure_id: closure.id(),
            });
        }
    }
    if let Some(end) = interval.end() {
        if close_at.checked_cmp(end)? == std::cmp::Ordering::Greater {
            return Err(AssertionProjectionError::ClosureOutsideValidity {
                assertion_id: assertion.id(),
                closure_id: closure.id(),
            });
        }
    }
    Ok(())
}

/// Invalid or inconsistent assertion history encountered by the projection model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssertionProjectionError {
    /// Two stored assertion records use one persistent identity.
    DuplicateAssertion { assertion_id: AssertionId },
    /// Two stored validity closures use one persistent identity.
    DuplicateValidityClosure {
        closure_id: AssertionValidityClosureId,
    },
    /// A validity closure refers to an assertion absent from the loaded history.
    UnknownClosureTarget {
        closure_id: AssertionValidityClosureId,
        assertion_id: AssertionId,
    },
    /// Two stored retractions use one persistent identity.
    DuplicateRetraction {
        retraction_id: AssertionRetractionId,
    },
    /// A retraction refers to an assertion absent from the loaded history.
    UnknownRetractionTarget {
        retraction_id: AssertionRetractionId,
        assertion_id: AssertionId,
    },
    /// A lifecycle record must be committed after the assertion it targets.
    LifecycleRevisionNotAfterAssertion {
        assertion_id: AssertionId,
        assertion_revision: crate::ids::Revision,
        lifecycle_revision: crate::ids::Revision,
        record: &'static str,
    },
    /// The closure time falls outside the assertion's original validity range.
    ClosureOutsideValidity {
        assertion_id: AssertionId,
        closure_id: AssertionValidityClosureId,
    },
    /// A world-time comparison crossed Timeline identities.
    Temporal(TemporalError),
}

impl From<TemporalError> for AssertionProjectionError {
    fn from(error: TemporalError) -> Self {
        Self::Temporal(error)
    }
}

impl fmt::Display for AssertionProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateAssertion { assertion_id } => {
                write!(
                    formatter,
                    "duplicate assertion ID in history: {assertion_id}"
                )
            }
            Self::DuplicateValidityClosure { closure_id } => {
                write!(
                    formatter,
                    "duplicate validity closure ID in history: {closure_id}"
                )
            }
            Self::UnknownClosureTarget {
                closure_id,
                assertion_id,
            } => write!(
                formatter,
                "validity closure {closure_id} targets unknown assertion {assertion_id}"
            ),
            Self::DuplicateRetraction { retraction_id } => {
                write!(
                    formatter,
                    "duplicate retraction ID in history: {retraction_id}"
                )
            }
            Self::UnknownRetractionTarget {
                retraction_id,
                assertion_id,
            } => write!(
                formatter,
                "retraction {retraction_id} targets unknown assertion {assertion_id}"
            ),
            Self::LifecycleRevisionNotAfterAssertion {
                assertion_id,
                assertion_revision,
                lifecycle_revision,
                record,
            } => write!(
                formatter,
                "lifecycle record {record:?} at revision {lifecycle_revision} is not after assertion {assertion_id} at revision {assertion_revision}"
            ),
            Self::ClosureOutsideValidity {
                assertion_id,
                closure_id,
            } => write!(
                formatter,
                "validity closure {closure_id} falls outside assertion {assertion_id}'s validity interval"
            ),
            Self::Temporal(error) => write!(formatter, "invalid world-time comparison: {error}"),
        }
    }
}

impl std::error::Error for AssertionProjectionError {}

#[cfg(test)]
mod tests {
    use super::{AssertionLifecycleProjection, AssertionProjectionError};
    use crate::assertions::{
        Assertion, AssertionDraft, AssertionRetraction, AssertionValidityClosure, Polarity, Subject,
    };
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{
        AssertionId, AssertionRetractionId, AssertionValidityClosureId, DomainId, EntityId,
        HistorySpaceId, IdValidationError, LayerId, PredicateId, Revision, RevisionError,
        TimelineId,
    };
    use crate::temporal::{
        AssertionValidity, RecordedAsOf, TemporalError, TimeInterval, Timeline, WorldTime,
    };
    use crate::values::Value;
    use std::error::Error;
    use std::fmt;

    type TestResult<T = ()> = Result<T, TestError>;

    #[derive(Debug)]
    enum TestError {
        Assertion(crate::assertions::AssertionRecordError),
        Context(crate::context::ContextError),
        Id(IdValidationError),
        Projection(AssertionProjectionError),
        Revision(RevisionError),
        Temporal(TemporalError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Assertion(error) => write!(formatter, "{error}"),
                Self::Context(error) => write!(formatter, "{error}"),
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Projection(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Temporal(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl Error for TestError {}

    macro_rules! impl_error_conversion {
        ($source:ty, $variant:ident) => {
            impl From<$source> for TestError {
                fn from(error: $source) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    impl_error_conversion!(crate::assertions::AssertionRecordError, Assertion);
    impl_error_conversion!(crate::context::ContextError, Context);
    impl_error_conversion!(IdValidationError, Id);
    impl_error_conversion!(AssertionProjectionError, Projection);
    impl_error_conversion!(RevisionError, Revision);
    impl_error_conversion!(TemporalError, Temporal);

    fn id<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn setup() -> TestResult<(Assertion, Timeline, Timeline)> {
        let timeline = Timeline::new(id::<TimelineId>(1)?);
        let other_timeline = Timeline::new(id::<TimelineId>(2)?);
        let context = ContextKey::new(
            id::<HistorySpaceId>(3)?,
            id::<LayerId>(4)?,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        let validity = AssertionValidity::new(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, 10)),
            Some(WorldTime::from_nanoseconds(timeline, 100)),
        )?);
        let assertion = Assertion::new(
            id::<AssertionId>(5)?,
            AssertionDraft::new(
                context,
                Subject::new(id::<EntityId>(6)?),
                id::<PredicateId>(7)?,
                Value::String(String::from("value")),
                Polarity::Positive,
                validity,
            ),
            revision(2)?,
        );
        Ok((assertion, timeline, other_timeline))
    }

    fn as_of(value: u64) -> TestResult<RecordedAsOf> {
        Ok(RecordedAsOf::from_published_revision(revision(value)?))
    }

    fn time(timeline: Timeline, value: i128) -> WorldTime {
        WorldTime::from_nanoseconds(timeline, value)
    }

    #[test]
    fn world_time_validity_is_half_open_and_separate_from_creation_revision() -> TestResult {
        let (assertion, timeline, _) = setup()?;
        let projection =
            AssertionLifecycleProjection::new(vec![assertion.clone()], vec![], vec![])?;
        assert_eq!(
            projection.candidates(as_of(1)?, time(timeline, 20))?.len(),
            0
        );
        assert_eq!(
            projection.candidates(as_of(2)?, time(timeline, 9))?.len(),
            0
        );
        assert_eq!(
            projection.candidates(as_of(2)?, time(timeline, 10))?.len(),
            1
        );
        assert_eq!(
            projection.candidates(as_of(2)?, time(timeline, 99))?.len(),
            1
        );
        assert_eq!(
            projection.candidates(as_of(2)?, time(timeline, 100))?.len(),
            0
        );
        Ok(())
    }

    #[test]
    fn closure_applies_only_after_its_transaction_revision_and_at_its_world_time() -> TestResult {
        let (assertion, timeline, _) = setup()?;
        let closure = AssertionValidityClosure::new(
            id::<AssertionValidityClosureId>(8)?,
            &assertion,
            time(timeline, 50),
            revision(4)?,
        )?;
        let projection = AssertionLifecycleProjection::new(vec![assertion], vec![closure], vec![])?;
        assert_eq!(
            projection.candidates(as_of(3)?, time(timeline, 75))?.len(),
            1
        );
        assert_eq!(
            projection.candidates(as_of(4)?, time(timeline, 49))?.len(),
            1
        );
        assert_eq!(
            projection.candidates(as_of(4)?, time(timeline, 50))?.len(),
            0
        );
        Ok(())
    }

    #[test]
    fn retraction_applies_only_at_and_after_its_recorded_revision() -> TestResult {
        let (assertion, timeline, _) = setup()?;
        let retraction = AssertionRetraction::new(
            id::<AssertionRetractionId>(9)?,
            &assertion,
            "retracted",
            revision(4)?,
        )?;
        let projection =
            AssertionLifecycleProjection::new(vec![assertion], vec![], vec![retraction])?;
        assert_eq!(
            projection.candidates(as_of(3)?, time(timeline, 75))?.len(),
            1
        );
        assert_eq!(
            projection.candidates(as_of(4)?, time(timeline, 75))?.len(),
            0
        );
        Ok(())
    }

    #[test]
    fn closure_bounds_and_query_timelines_fail_closed() -> TestResult {
        let (assertion, timeline, other_timeline) = setup()?;
        let before_start = AssertionValidityClosure::new(
            id::<AssertionValidityClosureId>(10)?,
            &assertion,
            time(timeline, 9),
            revision(4)?,
        )?;
        assert!(matches!(
            AssertionLifecycleProjection::new(vec![assertion.clone()], vec![before_start], vec![]),
            Err(AssertionProjectionError::ClosureOutsideValidity { .. })
        ));
        let after_end = AssertionValidityClosure::new(
            id::<AssertionValidityClosureId>(12)?,
            &assertion,
            time(timeline, 101),
            revision(4)?,
        )?;
        assert!(matches!(
            AssertionLifecycleProjection::new(vec![assertion.clone()], vec![after_end], vec![]),
            Err(AssertionProjectionError::ClosureOutsideValidity { .. })
        ));
        let malformed_wire_closure = AssertionValidityClosure::from_wire_fields(
            id::<AssertionValidityClosureId>(13)?,
            assertion.id(),
            time(other_timeline, 50),
            revision(4)?,
        );
        assert!(matches!(
            AssertionLifecycleProjection::new(
                vec![assertion.clone()],
                vec![malformed_wire_closure],
                vec![]
            ),
            Err(AssertionProjectionError::Temporal(
                TemporalError::IncomparableTimelines { .. }
            ))
        ));
        let projection = AssertionLifecycleProjection::new(vec![assertion], vec![], vec![])?;
        assert!(matches!(
            projection.candidates(as_of(2)?, time(other_timeline, 75)),
            Err(AssertionProjectionError::Temporal(
                TemporalError::IncomparableTimelines { .. }
            ))
        ));
        Ok(())
    }

    #[test]
    fn missing_lifecycle_targets_are_rejected_before_projection() -> TestResult {
        let (assertion, timeline, _) = setup()?;
        let closure = AssertionValidityClosure::new(
            id::<AssertionValidityClosureId>(11)?,
            &assertion,
            time(timeline, 50),
            revision(4)?,
        )?;
        assert!(matches!(
            AssertionLifecycleProjection::new(vec![], vec![closure], vec![]),
            Err(AssertionProjectionError::UnknownClosureTarget { .. })
        ));
        Ok(())
    }

    #[test]
    fn decoded_lifecycle_records_must_follow_their_target_revision() -> TestResult {
        let (assertion, timeline, _) = setup()?;
        let invalid_closure = AssertionValidityClosure::from_wire_fields(
            id::<AssertionValidityClosureId>(14)?,
            assertion.id(),
            time(timeline, 50),
            revision(2)?,
        );
        assert!(matches!(
            AssertionLifecycleProjection::new(
                vec![assertion.clone()],
                vec![invalid_closure],
                vec![]
            ),
            Err(
                AssertionProjectionError::LifecycleRevisionNotAfterAssertion {
                    record: "AssertionValidityClosure",
                    ..
                }
            )
        ));
        let invalid_retraction = AssertionRetraction::from_wire_fields(
            id::<AssertionRetractionId>(15)?,
            assertion.id(),
            String::from("malformed historical row"),
            revision(2)?,
        );
        assert!(matches!(
            AssertionLifecycleProjection::new(vec![assertion], vec![], vec![invalid_retraction]),
            Err(
                AssertionProjectionError::LifecycleRevisionNotAfterAssertion {
                    record: "AssertionRetraction",
                    ..
                }
            )
        ));
        Ok(())
    }
}
