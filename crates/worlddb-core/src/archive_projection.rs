//! Index-free operational archive projection over immutable target history.

use std::collections::BTreeSet;
use std::fmt;

use crate::archive::{
    ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition, ArchiveTransitionError,
};
use crate::ids::{ArchiveTransitionId, Revision};
use crate::temporal::RecordedAsOf;

/// One archivable record identity and the revision that introduced the target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveTargetRecord {
    target: ArchiveTargetRef,
    created_revision: Revision,
}

impl ArchiveTargetRecord {
    /// Binds one closed archive target to its immutable creation revision.
    #[must_use]
    pub const fn new(target: ArchiveTargetRef, created_revision: Revision) -> Self {
        Self {
            target,
            created_revision,
        }
    }

    /// Returns the closed identity of this stored record.
    #[must_use]
    pub const fn target(self) -> ArchiveTargetRef {
        self.target
    }

    /// Returns the Transaction-Time revision at which the target was created.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// Validated, append-only archive history for a closed set of stored records.
///
/// Archive state changes ordinary operational visibility only. Raw history
/// remains queryable at every revision where the target itself existed.
#[derive(Clone, Debug, Default)]
pub struct ArchiveHistoryReferenceModel {
    targets: Vec<ArchiveTargetRecord>,
    transitions: Vec<ArchiveTransition>,
}

impl ArchiveHistoryReferenceModel {
    /// Validates a complete immutable target inventory and its archive transitions.
    pub fn new(
        mut targets: Vec<ArchiveTargetRecord>,
        mut transitions: Vec<ArchiveTransition>,
    ) -> Result<Self, ArchiveProjectionError> {
        targets.sort_by_key(|record| record.target());
        transitions.sort_by_key(|record| (record.target(), record.created_revision(), record.id()));

        let mut target_ids = BTreeSet::new();
        for target in &targets {
            if !target_ids.insert(target.target()) {
                return Err(ArchiveProjectionError::DuplicateTarget {
                    target: target.target(),
                });
            }
        }

        let target_by_id = targets
            .iter()
            .map(|record| (record.target(), record))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut transition_ids = BTreeSet::new();
        let mut target_state = std::collections::BTreeMap::new();
        let mut last_revision = std::collections::BTreeMap::<ArchiveTargetRef, Revision>::new();
        for transition in &transitions {
            if !transition_ids.insert(transition.id()) {
                return Err(ArchiveProjectionError::DuplicateTransition {
                    transition_id: transition.id(),
                });
            }
            let target = target_by_id.get(&transition.target()).ok_or(
                ArchiveProjectionError::UnknownTarget {
                    transition_id: Some(transition.id()),
                    target: transition.target(),
                },
            )?;
            if transition.created_revision() <= target.created_revision() {
                return Err(ArchiveProjectionError::TransitionNotAfterTarget {
                    transition_id: transition.id(),
                    target: transition.target(),
                    target_revision: target.created_revision(),
                    transition_revision: transition.created_revision(),
                });
            }
            if let Some(previous) = last_revision.get(&transition.target()).copied() {
                if transition.created_revision() <= previous {
                    return Err(ArchiveProjectionError::TransitionRevisionNotIncreasing {
                        target: transition.target(),
                        previous,
                        requested: transition.created_revision(),
                    });
                }
            }
            last_revision.insert(transition.target(), transition.created_revision());

            let prior = target_state
                .get(&transition.target())
                .copied()
                .unwrap_or(ArchiveState::Unarchived);
            let next = prior.transition(transition.action()).map_err(|error| {
                ArchiveProjectionError::InvalidStateTransition {
                    transition_id: transition.id(),
                    prior_state: prior,
                    action: transition.action(),
                    error,
                }
            })?;
            target_state.insert(transition.target(), next);
        }

        Ok(Self {
            targets,
            transitions,
        })
    }

    /// Returns a target's operational state at a published historical revision.
    pub fn state_at(
        &self,
        target: ArchiveTargetRef,
        recorded_as_of: RecordedAsOf,
    ) -> Result<ArchiveState, ArchiveProjectionError> {
        let target_record =
            self.target_record(target)
                .ok_or(ArchiveProjectionError::UnknownTarget {
                    transition_id: None,
                    target,
                })?;
        let revision = recorded_as_of.revision();
        if target_record.created_revision() > revision {
            return Err(ArchiveProjectionError::TargetNotYetCreated {
                target,
                target_revision: target_record.created_revision(),
                query_revision: revision,
            });
        }
        let mut state = ArchiveState::Unarchived;
        let transition_start = self
            .transitions
            .partition_point(|transition| transition.target() < target);
        for transition in self
            .transitions
            .iter()
            .skip(transition_start)
            .take_while(|transition| transition.target() == target)
        {
            if transition.created_revision() <= revision {
                state = state.transition(transition.action()).map_err(|error| {
                    ArchiveProjectionError::InvalidStateTransition {
                        transition_id: transition.id(),
                        prior_state: state,
                        action: transition.action(),
                        error,
                    }
                })?;
            }
        }
        Ok(state)
    }

    /// Looks up one immutable archive target in the canonical sorted inventory.
    #[must_use]
    pub fn target_record(&self, target: ArchiveTargetRef) -> Option<ArchiveTargetRecord> {
        self.targets
            .binary_search_by_key(&target, |record| record.target())
            .ok()
            .and_then(|index| self.targets.get(index).copied())
    }

    /// Returns all raw-history targets that existed by this revision, archived or not.
    #[must_use]
    pub fn raw_targets_at(&self, recorded_as_of: RecordedAsOf) -> Vec<ArchiveTargetRef> {
        self.targets
            .iter()
            .filter(|record| record.created_revision() <= recorded_as_of.revision())
            .map(|record| record.target())
            .collect()
    }

    /// Returns targets visible to an ordinary operational projection at this revision.
    pub fn ordinary_targets_at(
        &self,
        recorded_as_of: RecordedAsOf,
    ) -> Result<Vec<ArchiveTargetRef>, ArchiveProjectionError> {
        self.raw_targets_at(recorded_as_of)
            .into_iter()
            .filter_map(|target| match self.state_at(target, recorded_as_of) {
                Ok(ArchiveState::Unarchived) => Some(Ok(target)),
                Ok(ArchiveState::Archived) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    /// Returns the immutable target inventory, including archived records.
    #[must_use]
    pub fn targets(&self) -> &[ArchiveTargetRecord] {
        &self.targets
    }

    /// Returns immutable transition records in target/revision/ID order.
    #[must_use]
    pub fn transitions(&self) -> &[ArchiveTransition] {
        &self.transitions
    }
}

/// Invalid or inconsistent operational archive history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveProjectionError {
    /// Two target records use the same closed identity.
    DuplicateTarget { target: ArchiveTargetRef },
    /// Two transitions use the same persistent transition identity.
    DuplicateTransition { transition_id: ArchiveTransitionId },
    /// A transition targets a record not in the loaded immutable inventory.
    UnknownTarget {
        transition_id: Option<ArchiveTransitionId>,
        target: ArchiveTargetRef,
    },
    /// An archive target was created after the transition attempting to change it.
    TransitionNotAfterTarget {
        transition_id: ArchiveTransitionId,
        target: ArchiveTargetRef,
        target_revision: Revision,
        transition_revision: Revision,
    },
    /// One target has two transitions at the same or decreasing revisions.
    TransitionRevisionNotIncreasing {
        target: ArchiveTargetRef,
        previous: Revision,
        requested: Revision,
    },
    /// The supplied archive action does not follow the effective prior state.
    InvalidStateTransition {
        transition_id: ArchiveTransitionId,
        prior_state: ArchiveState,
        action: ArchiveAction,
        error: ArchiveTransitionError,
    },
    /// The record identity did not exist at the requested transaction-time revision.
    TargetNotYetCreated {
        target: ArchiveTargetRef,
        target_revision: Revision,
        query_revision: Revision,
    },
}

impl fmt::Display for ArchiveProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTarget { target } => {
                write!(formatter, "duplicate archive target: {target:?}")
            }
            Self::DuplicateTransition { transition_id } => {
                write!(
                    formatter,
                    "duplicate archive transition ID: {transition_id}"
                )
            }
            Self::UnknownTarget {
                transition_id,
                target,
            } => {
                write!(
                    formatter,
                    "archive target {target:?} is absent from loaded history (transition {transition_id:?})"
                )
            }
            Self::TransitionNotAfterTarget {
                transition_id,
                target,
                target_revision,
                transition_revision,
            } => write!(
                formatter,
                "archive transition {transition_id} at {transition_revision} is not after target {target:?} at {target_revision}"
            ),
            Self::TransitionRevisionNotIncreasing {
                target,
                previous,
                requested,
            } => write!(
                formatter,
                "archive transition revision for {target:?} is not increasing: {requested} after {previous}"
            ),
            Self::InvalidStateTransition {
                transition_id,
                prior_state,
                action,
                error,
            } => write!(
                formatter,
                "archive transition {transition_id} cannot apply {action:?} to {prior_state:?}: {error}"
            ),
            Self::TargetNotYetCreated {
                target,
                target_revision,
                query_revision,
            } => write!(
                formatter,
                "archive target {target:?} was created at {target_revision}, after query revision {query_revision}"
            ),
        }
    }
}

impl std::error::Error for ArchiveProjectionError {}

#[cfg(test)]
mod tests {
    use super::{ArchiveHistoryReferenceModel, ArchiveProjectionError, ArchiveTargetRecord};
    use crate::archive::{ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition};
    use crate::ids::{ArchiveTransitionId, AssertionId, AssertionRetractionId, DomainId, Revision};
    use crate::temporal::RecordedAsOf;
    use std::error::Error;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(crate::IdValidationError),
        Projection(ArchiveProjectionError),
        Revision(crate::RevisionError),
        Transition(crate::ArchiveTransitionError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Projection(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Transition(error) => write!(formatter, "{error}"),
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

    error_conversion!(crate::IdValidationError, Id);
    error_conversion!(ArchiveProjectionError, Projection);
    error_conversion!(crate::RevisionError, Revision);
    error_conversion!(crate::ArchiveTransitionError, Transition);

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, crate::RevisionError> {
        Revision::new(value)
    }

    fn as_of(value: u64) -> Result<RecordedAsOf, crate::RevisionError> {
        Ok(RecordedAsOf::from_published_revision(revision(value)?))
    }

    #[test]
    fn archive_changes_operational_visibility_but_raw_history_keeps_targets() -> TestResult {
        let assertion_target = ArchiveTargetRef::Assertion(id::<AssertionId>(1)?);
        let retraction_target =
            ArchiveTargetRef::AssertionRetraction(id::<AssertionRetractionId>(2)?);
        let archive = ArchiveTransition::new(
            id::<ArchiveTransitionId>(3)?,
            assertion_target,
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            revision(4)?,
        )?;
        let model = ArchiveHistoryReferenceModel::new(
            vec![
                ArchiveTargetRecord::new(assertion_target, revision(2)?),
                ArchiveTargetRecord::new(retraction_target, revision(3)?),
            ],
            vec![archive],
        )?;
        assert_eq!(model.raw_targets_at(as_of(4)?).len(), 2);
        assert_eq!(
            model.ordinary_targets_at(as_of(3)?)?,
            vec![assertion_target, retraction_target]
        );
        assert_eq!(
            model.ordinary_targets_at(as_of(4)?)?,
            vec![retraction_target]
        );
        assert_eq!(
            model.state_at(assertion_target, as_of(4)?)?,
            ArchiveState::Archived
        );
        assert_eq!(model.targets().len(), 2);
        Ok(())
    }

    #[test]
    fn unarchive_restores_operational_visibility_without_rewriting_history() -> TestResult {
        let target = ArchiveTargetRef::Assertion(id::<AssertionId>(4)?);
        let archive = ArchiveTransition::new(
            id::<ArchiveTransitionId>(5)?,
            target,
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            revision(3)?,
        )?;
        let unarchive = ArchiveTransition::new(
            id::<ArchiveTransitionId>(6)?,
            target,
            ArchiveAction::Unarchive,
            ArchiveState::Archived,
            revision(5)?,
        )?;
        let model = ArchiveHistoryReferenceModel::new(
            vec![ArchiveTargetRecord::new(target, revision(2)?)],
            vec![unarchive, archive],
        )?;
        assert!(model.ordinary_targets_at(as_of(3)?)?.is_empty());
        assert_eq!(model.ordinary_targets_at(as_of(5)?)?, vec![target]);
        assert_eq!(model.raw_targets_at(as_of(5)?), vec![target]);
        Ok(())
    }

    #[test]
    fn malformed_state_sequence_and_unknown_targets_are_rejected() -> TestResult {
        let target = ArchiveTargetRef::Assertion(id::<AssertionId>(7)?);
        let first = ArchiveTransition::new(
            id::<ArchiveTransitionId>(8)?,
            target,
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            revision(3)?,
        )?;
        let malformed_second = ArchiveTransition::new(
            id::<ArchiveTransitionId>(9)?,
            target,
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            revision(4)?,
        )?;
        assert!(matches!(
            ArchiveHistoryReferenceModel::new(
                vec![ArchiveTargetRecord::new(target, revision(2)?)],
                vec![first, malformed_second]
            ),
            Err(ArchiveProjectionError::InvalidStateTransition { .. })
        ));
        let missing = ArchiveTransition::new(
            id::<ArchiveTransitionId>(10)?,
            target,
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            revision(3)?,
        )?;
        assert!(matches!(
            ArchiveHistoryReferenceModel::new(vec![], vec![missing]),
            Err(ArchiveProjectionError::UnknownTarget { .. })
        ));
        Ok(())
    }

    #[test]
    fn raw_and_ordinary_reads_respect_target_creation_revision() -> TestResult {
        let target = ArchiveTargetRef::Assertion(id::<AssertionId>(11)?);
        let model = ArchiveHistoryReferenceModel::new(
            vec![ArchiveTargetRecord::new(target, revision(5)?)],
            vec![],
        )?;
        assert!(model.raw_targets_at(as_of(4)?).is_empty());
        assert!(model.ordinary_targets_at(as_of(4)?)?.is_empty());
        assert!(matches!(
            model.state_at(target, as_of(4)?),
            Err(ArchiveProjectionError::TargetNotYetCreated { .. })
        ));
        Ok(())
    }
}
