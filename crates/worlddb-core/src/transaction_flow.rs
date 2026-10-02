//! Consuming typestate flow for model write transactions.
//!
//! This flow provides no implicit commit on `Drop`. Engine validation runs
//! before the validated batch is handed to the backend. The in-memory model
//! commit is not a durability guarantee.
//!
//! A transaction cannot publish while it remains open:
//!
//! ```compile_fail
//! use worlddb_core::{OpenTransaction, RevisionBackend};
//! fn publish_before_validation<B: RevisionBackend<u8>>(
//!     open: OpenTransaction<'_, B, u8>,
//! ) {
//!     let _ = open.commit();
//! }
//! ```
//!
//! Validation consumes the open transaction, and commit consumes the
//! validated transaction, so the same value cannot be committed twice:
//!
//! ```compile_fail
//! use worlddb_core::{RevisionBackend, ValidatedTransaction};
//! fn commit_twice<B: RevisionBackend<u8>>(
//!     validated: ValidatedTransaction<'_, B, u8>,
//! ) {
//!     let _first = validated.commit();
//!     let _second = validated.commit();
//! }
//! ```
//!
//! The transaction resource itself is not cloneable:
//!
//! ```compile_fail
//! use worlddb_core::{OpenTransaction, RevisionBackend};
//! fn clone_transaction<B: RevisionBackend<u8>>(
//!     open: &OpenTransaction<'_, B, u8>,
//! ) {
//!     let _: OpenTransaction<'_, B, u8> = open.clone();
//! }
//! ```

use std::marker::PhantomData;

use crate::commit_cancellation::CommitCancellation;
use crate::ids::Revision;
use crate::revision_backend::RevisionBackend;
use crate::transaction::TransactionState;
use crate::wire_records::Record;

pub use crate::revision_backend::CancellablePublishError as CancellableCommitError;

/// Marker for a transaction that may still stage entries or validate.
pub struct Open {
    _private: (),
}

/// Marker for a transaction whose engine validation has completed.
pub struct Validated {
    _private: (),
}

/// Transaction resource consumed by validation, commit, or abort.
pub struct WriteTransaction<'backend, B, T, Phase> {
    backend: &'backend mut B,
    base_revision: Revision,
    entries: Vec<T>,
    cancellation: CommitCancellation,
    phase: PhantomData<Phase>,
}

/// Open transaction state.
pub type OpenTransaction<'backend, B, T> = WriteTransaction<'backend, B, T, Open>;

/// Validated transaction state.
pub type ValidatedTransaction<'backend, B, T> = WriteTransaction<'backend, B, T, Validated>;

/// Runs the complete engine validation callback before publishing a mixed typed Record batch.
///
/// The callback receives the transaction base and every staged record at once.
/// A validation error consumes the open transaction and publishes nothing;
/// success publishes the entire heterogeneous batch through one backend call.
/// Cancellation uses the control handle created with the transaction.
pub fn commit_mixed_record_batch<B, E>(
    transaction: OpenTransaction<'_, B, Record>,
    validate: impl FnOnce(Revision, &[Record]) -> Result<(), E>,
) -> Result<Revision, MixedRecordCommitError<E>>
where
    B: RevisionBackend<Record>,
{
    let validated = transaction
        .validate(validate)
        .map_err(MixedRecordCommitError::Validation)?;
    validated.commit().map_err(MixedRecordCommitError::Publish)
}

/// Validation or one-batch publication failure for a mixed typed Record transaction.
#[derive(Debug)]
pub enum MixedRecordCommitError<E> {
    /// The complete staged record set failed engine validation.
    Validation(E),
    /// Backend rejected publication of the validated complete batch.
    Publish(CancellableCommitError),
}

impl<E: std::fmt::Display> std::fmt::Display for MixedRecordCommitError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(error) => write!(formatter, "mixed write validation failed: {error}"),
            Self::Publish(error) => write!(formatter, "mixed write publication failed: {error}"),
        }
    }
}

impl<E> std::error::Error for MixedRecordCommitError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Publish(error) => Some(error),
        }
    }
}

impl<'backend, B, T> WriteTransaction<'backend, B, T, Open>
where
    B: RevisionBackend<T>,
{
    /// Begins staging against an already published base revision.
    pub fn begin(
        backend: &'backend mut B,
        base_revision: Revision,
    ) -> Result<Self, TransactionBeginError> {
        let published = backend.latest_published();
        if base_revision > published {
            return Err(TransactionBeginError::BaseRevisionNotPublished {
                requested: base_revision,
                published,
            });
        }
        Ok(Self {
            backend,
            base_revision,
            entries: Vec::new(),
            cancellation: CommitCancellation::new(),
            phase: PhantomData,
        })
    }

    /// Adds an owned entry to the private staging batch.
    pub fn stage(&mut self, entry: T) {
        self.entries.push(entry);
    }

    /// Returns the backend head while the exclusive transaction borrow is held.
    #[must_use]
    pub fn latest_published(&self) -> Revision {
        self.backend.latest_published()
    }

    /// Clones the host control handle for this transaction before it is moved
    /// into validation or a worker task.
    #[must_use]
    pub fn cancellation_handle(&self) -> CommitCancellation {
        self.cancellation.clone()
    }

    /// Runs engine validation and consumes the open state on either outcome.
    pub fn validate<E>(
        self,
        validate: impl FnOnce(Revision, &[T]) -> Result<(), E>,
    ) -> Result<ValidatedTransaction<'backend, B, T>, E> {
        let Self {
            backend,
            base_revision,
            entries,
            cancellation,
            phase: _,
        } = self;
        validate(base_revision, &entries)?;
        Ok(WriteTransaction {
            backend,
            base_revision,
            entries,
            cancellation,
            phase: PhantomData,
        })
    }
}

impl<B, T> WriteTransaction<'_, B, T, Validated>
where
    B: RevisionBackend<T>,
{
    /// Publishes the whole validated batch using its transaction cancellation
    /// handle and the backend's exact commitpoint.
    pub fn commit(self) -> Result<Revision, CancellableCommitError> {
        self.backend
            .publish_cancellable(self.entries, &self.cancellation)
    }
}

impl<B, T, Phase> WriteTransaction<'_, B, T, Phase> {
    /// Returns the pinned base revision recorded by this transaction.
    #[must_use]
    pub const fn base_revision(&self) -> Revision {
        self.base_revision
    }

    /// Consumes staging without publishing and returns an explicit abort state.
    #[must_use]
    pub fn abort(self) -> TransactionState {
        TransactionState::Aborted
    }
}

/// Failure to begin a model transaction against the requested base revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionBeginError {
    /// The requested base is newer than the current published head.
    BaseRevisionNotPublished {
        requested: Revision,
        published: Revision,
    },
}

impl std::fmt::Display for TransactionBeginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BaseRevisionNotPublished {
                requested,
                published,
            } => write!(
                formatter,
                "base revision {requested} is not published; latest is {published}"
            ),
        }
    }
}

impl std::error::Error for TransactionBeginError {}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver, SyncSender};
    use std::thread;
    use std::time::Duration;

    use crate::commit_cancellation::{
        CancellationRequestDisposition, CommitCancellation, CommitCancellationState,
    };
    use crate::ids::Revision;
    use crate::revision_backend::{
        CancellablePublishError, InMemoryRevisionBackend, RevisionBackend,
    };
    use crate::revision_history::{HistoricalRead, RevisionLogError};
    use crate::transaction::TransactionState;
    use crate::{
        DomainId, JobBudget, MigrationCategory, MigrationId, MigrationPlan, MigrationPlanSpec,
        MigrationStepId, MigrationTargetSchema, MigrationTransformerVersion, Record,
        SchemaDefinitionId, SchemaIdentityTransition, SourceSchemaPrecondition,
    };

    use super::{
        CancellableCommitError, MixedRecordCommitError, OpenTransaction, TransactionBeginError,
        commit_mixed_record_batch,
    };

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn migration_plan(
        migration_id: MigrationId,
        step_id: MigrationStepId,
        predicate_id: crate::PredicateId,
    ) -> Result<MigrationPlan, String> {
        let schema_change = SchemaIdentityTransition::new(
            None,
            Some(SchemaDefinitionId::Predicate(predicate_id)),
            MigrationCategory::Additive,
        )
        .map_err(|error| error.to_string())?;
        MigrationPlan::new(MigrationPlanSpec {
            migration_id,
            category: MigrationCategory::Additive,
            source_schema: SourceSchemaPrecondition::new(
                crate::SchemaRevision::from_published_revision(Revision::GENESIS),
                [1; 32],
            ),
            target_schema: MigrationTargetSchema::new(
                crate::SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
                [2; 32],
            ),
            steps: vec![step_id],
            schema_changes: vec![schema_change],
            transformer_version: MigrationTransformerVersion::new(1)
                .map_err(|error| error.to_string())?,
            calendar_shift: None,
            budget: JobBudget::new(1_000, 1024 * 1024).map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())
    }

    struct GatedBackend {
        inner: InMemoryRevisionBackend<u8>,
        publish_started: SyncSender<()>,
        allow_publish: Receiver<()>,
    }

    impl RevisionBackend<u8> for GatedBackend {
        type Read<'a> = HistoricalRead<'a, u8>;

        fn latest_published(&self) -> Revision {
            self.inner.latest_published()
        }

        fn publish(&mut self, entries: Vec<u8>) -> Result<Revision, RevisionLogError> {
            if self.publish_started.send(()).is_err() || self.allow_publish.recv().is_err() {
                return Err(RevisionLogError::NoReservation);
            }
            self.inner.publish(entries)
        }

        fn publish_cancellable(
            &mut self,
            entries: Vec<u8>,
            cancellation: &CommitCancellation,
        ) -> Result<Revision, CancellablePublishError> {
            let permit = cancellation
                .begin_commitpoint()
                .map_err(CancellablePublishError::Commitpoint)?;
            if self.publish_started.send(()).is_err() || self.allow_publish.recv().is_err() {
                permit.not_committed();
                return Err(CancellablePublishError::Publish(
                    RevisionLogError::NoReservation,
                ));
            }
            match self.inner.publish(entries) {
                Ok(revision) => {
                    permit.committed();
                    Ok(revision)
                }
                Err(error) => {
                    permit.not_committed();
                    Err(CancellablePublishError::Publish(error))
                }
            }
        }

        fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
            self.inner.read_at(revision)
        }
    }

    #[test]
    fn validation_and_commit_consume_each_state_and_publish_one_batch() -> Result<(), String> {
        let mut backend = InMemoryRevisionBackend::new();
        let mut open = OpenTransaction::begin(&mut backend, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        open.stage(10_u8);
        open.stage(20_u8);
        let validated = open
            .validate(|base, entries| {
                if base == Revision::GENESIS && entries == [10, 20] {
                    Ok(())
                } else {
                    Err(())
                }
            })
            .map_err(|()| "engine rejected a valid test batch".to_owned())?;

        let revision = validated.commit().map_err(|error| error.to_string())?;
        assert_eq!(revision, Revision::FIRST_COMMIT);
        assert_eq!(
            backend
                .read_at(revision)
                .map_err(|error| error.to_string())?
                .collect::<Vec<_>>(),
            vec![(revision, &10), (revision, &20)]
        );
        Ok(())
    }

    #[test]
    fn abort_validation_failure_and_drop_leave_backend_unpublished() -> Result<(), String> {
        let mut backend = InMemoryRevisionBackend::new();
        let mut open = OpenTransaction::begin(&mut backend, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        open.stage(1_u8);
        assert_eq!(open.abort(), TransactionState::Aborted);
        assert_eq!(backend.latest_published(), Revision::GENESIS);

        let mut open = OpenTransaction::begin(&mut backend, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        open.stage(2_u8);
        assert!(open.validate(|_, _| Err::<(), _>("invalid batch")).is_err());
        assert_eq!(backend.latest_published(), Revision::GENESIS);

        {
            let mut dropped = OpenTransaction::begin(&mut backend, Revision::GENESIS)
                .map_err(|error| error.to_string())?;
            dropped.stage(3_u8);
        }
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(backend.commits().is_empty());
        Ok(())
    }

    #[test]
    fn begin_rejects_a_base_revision_that_is_not_published() -> Result<(), TransactionBeginError> {
        let mut backend = InMemoryRevisionBackend::<u8>::new();
        assert!(matches!(
            OpenTransaction::begin(&mut backend, Revision::FIRST_COMMIT),
            Err(TransactionBeginError::BaseRevisionNotPublished {
                requested: Revision::FIRST_COMMIT,
                published: Revision::GENESIS,
            })
        ));
        Ok(())
    }

    #[test]
    fn cancellation_before_final_publication_aborts_the_entire_batch() -> Result<(), String> {
        let mut backend = InMemoryRevisionBackend::new();
        let mut open = OpenTransaction::begin(&mut backend, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        open.stage(10_u8);
        open.stage(20_u8);
        let cancellation = open.cancellation_handle();
        let validated = open
            .validate(|_, entries| {
                if entries == [10, 20] {
                    Ok(())
                } else {
                    Err("unexpected staged batch")
                }
            })
            .map_err(str::to_owned)?;
        assert_eq!(
            cancellation.request_cancellation(),
            CancellationRequestDisposition::Signalled
        );

        assert!(matches!(
            validated.commit(),
            Err(CancellableCommitError::Commitpoint(
                crate::CommitpointError::CancelledBeforeCommitpoint
            ))
        ));
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(backend.commits().is_empty());
        assert_eq!(
            cancellation.state(),
            CommitCancellationState::CancellationRequested
        );
        assert_eq!(
            backend
                .publish(vec![30_u8])
                .map_err(|error| error.to_string())?,
            Revision::FIRST_COMMIT,
            "cancellation must release its invisible revision reservation"
        );
        Ok(())
    }

    #[test]
    fn cancellation_after_publication_starts_is_too_late_and_full_batch_finishes()
    -> Result<(), String> {
        let (publish_started_tx, publish_started_rx) = mpsc::sync_channel(1);
        let (allow_publish_tx, allow_publish_rx) = mpsc::sync_channel(1);
        let mut backend = GatedBackend {
            inner: InMemoryRevisionBackend::new(),
            publish_started: publish_started_tx,
            allow_publish: allow_publish_rx,
        };
        let mut open = OpenTransaction::begin(&mut backend, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        open.stage(10_u8);
        open.stage(20_u8);
        let cancellation = open.cancellation_handle();
        let validated = open
            .validate(|_, entries| {
                if entries == [10, 20] {
                    Ok(())
                } else {
                    Err("unexpected staged batch")
                }
            })
            .map_err(str::to_owned)?;
        let result = thread::scope(|scope| {
            let commit = scope.spawn(|| validated.commit());
            publish_started_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| format!("publication did not start: {error}"))?;
            assert_eq!(
                cancellation.request_cancellation(),
                CancellationRequestDisposition::TooLate
            );
            allow_publish_tx
                .send(())
                .map_err(|error| format!("could not release publication: {error}"))?;
            commit
                .join()
                .map_err(|_| "commit worker panicked".to_owned())?
                .map_err(|error| error.to_string())
        })?;

        assert_eq!(result, Revision::FIRST_COMMIT);
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);
        assert_eq!(backend.inner.commits().len(), 1);
        let committed = backend
            .inner
            .commits()
            .first()
            .ok_or_else(|| "committed batch was not retained".to_owned())?;
        assert_eq!(committed.entries(), &[10, 20]);
        assert_eq!(cancellation.state(), CommitCancellationState::Committed);
        Ok(())
    }

    #[test]
    fn cancellable_mixed_record_batch_aborts_without_publishing_any_record() -> Result<(), String> {
        let first = Record::MigrationPlan(migration_plan(
            id::<MigrationId>(1).map_err(|error| error.to_string())?,
            id::<MigrationStepId>(2).map_err(|error| error.to_string())?,
            id::<crate::PredicateId>(5).map_err(|error| error.to_string())?,
        )?);
        let second = Record::MigrationPlan(migration_plan(
            id::<MigrationId>(3).map_err(|error| error.to_string())?,
            id::<MigrationStepId>(4).map_err(|error| error.to_string())?,
            id::<crate::PredicateId>(6).map_err(|error| error.to_string())?,
        )?);
        let mut backend = InMemoryRevisionBackend::new();
        let mut transaction = OpenTransaction::begin(&mut backend, Revision::GENESIS)
            .map_err(|error| error.to_string())?;
        transaction.stage(first);
        transaction.stage(second);
        let cancellation = transaction.cancellation_handle();
        assert_eq!(
            cancellation.request_cancellation(),
            CancellationRequestDisposition::Signalled
        );

        let result = commit_mixed_record_batch(transaction, |_, records| {
            if records.len() == 2 {
                Ok(())
            } else {
                Err("mixed batch was incomplete")
            }
        });
        assert!(matches!(
            result,
            Err(MixedRecordCommitError::Publish(
                CancellableCommitError::Commitpoint(
                    crate::CommitpointError::CancelledBeforeCommitpoint
                )
            ))
        ));
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(backend.commits().is_empty());
        Ok(())
    }
}
