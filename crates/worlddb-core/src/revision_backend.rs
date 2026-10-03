//! Backend boundary for publishing immutable revisions.
//!
//! The in-memory implementation is a model/test backend. It provides atomic
//! visibility within this process, but makes no crash or durability guarantee.

use std::fmt;

use crate::commit_cancellation::{CommitCancellation, CommitpointError};
use crate::ids::Revision;
use crate::revision_history::{HistoricalRead, InMemoryRevisionLog, RevisionLogError};

/// Synchronous backend operations required by the revision model.
///
/// The engine owns validation and authorization. A backend receives only the
/// complete batch it is asked to publish and assigns its revision. Publication
/// is atomic: success exposes every entry at one revision, and an error exposes
/// none. Production durability and concurrent reader coordination are separate
/// contracts.
pub trait RevisionBackend<T> {
    /// Borrowed iterator returned by a backend-specific historical read.
    type Read<'a>: Iterator<Item = (Revision, &'a T)>
    where
        Self: 'a,
        T: 'a;

    /// Returns the newest revision visible to readers.
    fn latest_published(&self) -> Revision;

    /// Publishes the complete batch atomically as one new revision.
    ///
    /// Returning an error must leave the visible head and every published
    /// entry unchanged.
    fn publish(&mut self, entries: Vec<T>) -> Result<Revision, RevisionLogError>;

    /// Publishes one cancellable batch and owns the backend-specific commitpoint.
    ///
    /// Implementations perform reversible preparation first, then call
    /// `cancellation.begin_commitpoint()` immediately before the irreversible
    /// publication step. Once that succeeds, they must finish the atomic batch
    /// and report its complete result; they must not poll cancellation again.
    /// A backend that cannot resolve a durable commit-marker outcome reports
    /// `OutcomeUnknown` so the caller can reconcile by operation identity.
    fn publish_cancellable(
        &mut self,
        entries: Vec<T>,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError>;

    /// Reads entries through a revision that has already been published.
    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError>;
}

/// Cancellation or atomic publication result for one backend commit attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellablePublishError {
    /// Cancellation or an invalid control state prevented the commitpoint.
    Commitpoint(CommitpointError),
    /// The backend rejected the batch without publishing any entries.
    Publish(RevisionLogError),
    /// The commitpoint was entered but durable publication could not be resolved.
    /// Reopen and reconcile by this operation identity before retrying.
    OutcomeUnknown(crate::OperationId),
}

impl fmt::Display for CancellablePublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commitpoint(error) => write!(formatter, "commitpoint unavailable: {error}"),
            Self::Publish(error) => write!(formatter, "commit publication failed: {error}"),
            Self::OutcomeUnknown(operation_id) => write!(
                formatter,
                "commit outcome for OperationId {operation_id} is unknown and requires reconciliation"
            ),
        }
    }
}

impl std::error::Error for CancellablePublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Commitpoint(error) => Some(error),
            Self::Publish(error) => Some(error),
            Self::OutcomeUnknown(_) => None,
        }
    }
}

/// In-memory revision backend for model tests and reference execution.
///
/// Each successful call to [`publish`](RevisionBackend::publish) appends one
/// complete immutable batch and advances the visible head once. This backend
/// is not durable and does not coordinate concurrent readers or writers.
#[derive(Debug, Eq, PartialEq)]
pub struct InMemoryRevisionBackend<T> {
    history: InMemoryRevisionLog<T>,
}

impl<T> Default for InMemoryRevisionBackend<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> InMemoryRevisionBackend<T> {
    /// Creates an empty model backend at Genesis.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: InMemoryRevisionLog::new(),
        }
    }

    /// Returns the immutable commits, primarily for reference-model tests.
    #[must_use]
    pub fn commits(&self) -> &[crate::PublishedCommit<T>] {
        self.history.commits()
    }
}

impl<T> RevisionBackend<T> for InMemoryRevisionBackend<T> {
    type Read<'a>
        = HistoricalRead<'a, T>
    where
        Self: 'a,
        T: 'a;

    fn latest_published(&self) -> Revision {
        self.history.latest_published()
    }

    fn publish(&mut self, entries: Vec<T>) -> Result<Revision, RevisionLogError> {
        let revision = self.history.reserve_next()?;
        match self.history.publish(revision, entries) {
            Ok(()) => Ok(revision),
            Err(error) => {
                // The reservation belongs to this adapter; do not strand it if
                // an invariant check ever rejects the internal publication.
                let _ = self.history.cancel_reservation();
                Err(error)
            }
        }
    }

    fn publish_cancellable(
        &mut self,
        entries: Vec<T>,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        // Reserving is invisible and reversible, so perform it before the
        // cancellation/commitpoint race. After the race is won, only the
        // already-validated atomic append remains.
        let revision = self
            .history
            .reserve_next()
            .map_err(CancellablePublishError::Publish)?;
        let permit = match cancellation.begin_commitpoint() {
            Ok(permit) => permit,
            Err(error) => {
                self.history
                    .cancel_reservation()
                    .map_err(CancellablePublishError::Publish)?;
                return Err(CancellablePublishError::Commitpoint(error));
            }
        };
        match self.history.publish(revision, entries) {
            Ok(()) => {
                permit.committed();
                Ok(revision)
            }
            Err(error) => {
                let _ = self.history.cancel_reservation();
                permit.not_committed();
                Err(CancellablePublishError::Publish(error))
            }
        }
    }

    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
        self.history.read_at(revision)
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemoryRevisionBackend, RevisionBackend};
    use crate::ids::Revision;

    #[test]
    fn one_publish_assigns_one_revision_to_the_complete_batch()
    -> Result<(), crate::RevisionLogError> {
        let mut backend = InMemoryRevisionBackend::new();
        let revision = backend.publish(vec!["schema", "assertion", "event"])?;

        assert_eq!(revision, Revision::FIRST_COMMIT);
        assert_eq!(backend.latest_published(), revision);
        assert_eq!(
            backend.read_at(revision)?.collect::<Vec<_>>(),
            vec![
                (revision, &"schema"),
                (revision, &"assertion"),
                (revision, &"event"),
            ]
        );
        Ok(())
    }

    #[test]
    fn unpublished_revision_is_not_readable() -> Result<(), crate::RevisionLogError> {
        let backend = InMemoryRevisionBackend::<u8>::new();
        assert!(backend.read_at(Revision::FIRST_COMMIT).is_err());
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        Ok(())
    }
}
