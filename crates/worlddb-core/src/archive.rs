//! Typed operational archive records and reversible archive state.

use std::fmt;

use crate::ids::{
    ArchiveTransitionId, AssertionId, AssertionRetractionId, AssertionValidityClosureId,
    EntityRetirementId, EventId, EventMaskId, EventMaskRetractionId, EventRelationId,
    EventRelationRetractionId, EventRetractionId, EventSpanClosureId, EvidenceId,
    EvidenceRetractionId, MaskId, MaskRetractionId, MaskValidityClosureId, PerspectiveRetirementId,
    ProvenanceId, ProvenanceRetractionId, ReplacementBoundaryId, ReplacementBoundaryRetractionId,
    ReplacementBoundaryValidityClosureId, Revision, SourceId,
};

/// Closed set of persisted record identities that may receive an archive transition.
///
/// There is intentionally no `ArchiveTransition` variant: archive state changes
/// cannot target themselves. It is a validated subset of the exhaustive
/// `RecordRef` family.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ArchiveTargetRef {
    /// An Assertion record.
    Assertion(AssertionId),
    /// A Mask record.
    Mask(MaskId),
    /// A ReplacementBoundary record.
    ReplacementBoundary(ReplacementBoundaryId),
    /// An Event record.
    Event(EventId),
    /// An EventMask record.
    EventMask(EventMaskId),
    /// An EventRelation record.
    EventRelation(EventRelationId),
    /// A Source record.
    Source(SourceId),
    /// An Evidence record.
    Evidence(EvidenceId),
    /// A Provenance record.
    Provenance(ProvenanceId),
    /// An AssertionValidityClosure record.
    AssertionValidityClosure(AssertionValidityClosureId),
    /// An AssertionRetraction record.
    AssertionRetraction(AssertionRetractionId),
    /// A MaskValidityClosure record.
    MaskValidityClosure(MaskValidityClosureId),
    /// A MaskRetraction record.
    MaskRetraction(MaskRetractionId),
    /// A ReplacementBoundaryValidityClosure record.
    ReplacementBoundaryValidityClosure(ReplacementBoundaryValidityClosureId),
    /// A ReplacementBoundaryRetraction record.
    ReplacementBoundaryRetraction(ReplacementBoundaryRetractionId),
    /// An EventSpanClosure record.
    EventSpanClosure(EventSpanClosureId),
    /// An EventRetraction record.
    EventRetraction(EventRetractionId),
    /// An EventMaskRetraction record.
    EventMaskRetraction(EventMaskRetractionId),
    /// An EventRelationRetraction record.
    EventRelationRetraction(EventRelationRetractionId),
    /// An EvidenceRetraction record.
    EvidenceRetraction(EvidenceRetractionId),
    /// A ProvenanceRetraction record.
    ProvenanceRetraction(ProvenanceRetractionId),
    /// An EntityRetirement record.
    EntityRetirement(EntityRetirementId),
    /// A PerspectiveRetirement record.
    PerspectiveRetirement(PerspectiveRetirementId),
}

/// One explicit operational visibility action.
///
/// Archive actions are not domain-time Closure, transaction-time Retraction,
/// or physical Purge.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArchiveAction {
    /// Hides a target from ordinary operational projections.
    Archive,
    /// Restores a target to ordinary operational projections.
    Unarchive,
}

/// Operational archive state at a selected historical revision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArchiveState {
    /// The target is operationally visible.
    Unarchived,
    /// The target is excluded from ordinary operational projections.
    Archived,
}

impl ArchiveState {
    /// Applies one valid reversible state transition.
    pub const fn transition(self, action: ArchiveAction) -> Result<Self, ArchiveTransitionError> {
        match (self, action) {
            (Self::Unarchived, ArchiveAction::Archive) => Ok(Self::Archived),
            (Self::Archived, ArchiveAction::Unarchive) => Ok(Self::Unarchived),
            (Self::Archived, ArchiveAction::Archive) => {
                Err(ArchiveTransitionError::AlreadyArchived)
            }
            (Self::Unarchived, ArchiveAction::Unarchive) => {
                Err(ArchiveTransitionError::NotArchived)
            }
        }
    }
}

/// One immutable project-wide operational archive transition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ArchiveTransition {
    id: ArchiveTransitionId,
    target: ArchiveTargetRef,
    action: ArchiveAction,
    created_revision: Revision,
}

impl ArchiveTransition {
    /// Creates a transition only when its action changes the supplied prior state.
    pub fn new(
        id: ArchiveTransitionId,
        target: ArchiveTargetRef,
        action: ArchiveAction,
        prior_state: ArchiveState,
        created_revision: Revision,
    ) -> Result<Self, ArchiveTransitionError> {
        prior_state.transition(action)?;
        Ok(Self {
            id,
            target,
            action,
            created_revision,
        })
    }

    /// Returns this concrete archive-transition identity.
    #[must_use]
    pub const fn id(self) -> ArchiveTransitionId {
        self.id
    }

    /// Returns the persisted target record identity.
    #[must_use]
    pub const fn target(self) -> ArchiveTargetRef {
        self.target
    }

    /// Returns the explicit Archive/Unarchive action.
    #[must_use]
    pub const fn action(self) -> ArchiveAction {
        self.action
    }

    /// Returns the shared Transaction-Time revision that created this record.
    #[must_use]
    pub const fn created_revision(self) -> Revision {
        self.created_revision
    }
}

/// A rejected archive action for the target's current state.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArchiveTransitionError {
    /// Archive is valid only when the current state is Unarchived.
    AlreadyArchived,
    /// Unarchive is valid only when the current state is Archived.
    NotArchived,
}

impl fmt::Display for ArchiveTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyArchived => formatter.write_str("record is already archived"),
            Self::NotArchived => formatter.write_str("record is not archived"),
        }
    }
}

impl std::error::Error for ArchiveTransitionError {}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition, ArchiveTransitionError,
    };
    use crate::ids::{
        ArchiveTransitionId, AssertionId, DomainId, IdValidationError, Revision, RevisionError,
    };
    use std::error::Error;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Revision(RevisionError),
        Transition(ArchiveTransitionError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
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

    error_conversion!(IdValidationError, Id);
    error_conversion!(RevisionError, Revision);
    error_conversion!(ArchiveTransitionError, Transition);

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

    #[test]
    fn archive_and_unarchive_are_reversible_and_reject_repeated_actions() -> TestResult {
        assert_eq!(
            ArchiveState::Unarchived.transition(ArchiveAction::Archive),
            Ok(ArchiveState::Archived)
        );
        assert_eq!(
            ArchiveState::Archived.transition(ArchiveAction::Unarchive),
            Ok(ArchiveState::Unarchived)
        );
        assert_eq!(
            ArchiveState::Archived.transition(ArchiveAction::Archive),
            Err(ArchiveTransitionError::AlreadyArchived)
        );
        assert_eq!(
            ArchiveState::Unarchived.transition(ArchiveAction::Unarchive),
            Err(ArchiveTransitionError::NotArchived)
        );
        Ok(())
    }

    #[test]
    fn transition_record_has_a_concrete_id_typed_target_and_revision() -> TestResult {
        let id = value!(uuid::<ArchiveTransitionId>(1));
        let assertion_id = value!(uuid::<AssertionId>(2));
        let revision = value!(Revision::new(3));
        let transition = value!(ArchiveTransition::new(
            id,
            ArchiveTargetRef::Assertion(assertion_id),
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            revision,
        ));

        assert_eq!(transition.id(), id);
        assert_eq!(
            transition.target(),
            ArchiveTargetRef::Assertion(assertion_id)
        );
        assert_eq!(transition.action(), ArchiveAction::Archive);
        assert_eq!(transition.created_revision(), revision);
        Ok(())
    }
}
