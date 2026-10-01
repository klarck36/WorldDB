//! Atomic reference model for explicit three-record Assertion corrections.

use std::collections::HashSet;
use std::fmt;

use crate::assertions::{Assertion, AssertionDraft, AssertionRecordError, AssertionRetraction};
use crate::ids::{AssertionId, AssertionRetractionId, ProvenanceId, Revision};
use crate::revision_backend::RevisionBackend;
use crate::source_provenance::{
    ProvenanceEdge, ProvenanceEndpointRef, ProvenanceRelation, SourceEvidenceProvenanceError,
};
use crate::temporal::RecordedAsOf;
use crate::transaction_flow::{
    MixedRecordCommitError, OpenTransaction, TransactionBeginError, commit_mixed_record_batch,
};
use crate::wire_records::Record;

/// Explicit, complete input for one Assertion correction command.
#[derive(Clone, Debug)]
pub struct AssertionCorrectionCommand {
    expected_target_created_revision: Revision,
    replacement_id: AssertionId,
    replacement: AssertionDraft,
    retraction_id: AssertionRetractionId,
    retraction_reason: String,
    provenance_id: ProvenanceId,
    commit_revision: Revision,
}

impl AssertionCorrectionCommand {
    /// Captures every ID, target expectation, replacement field, reason, and
    /// transaction revision required to prepare the three-record effect.
    #[must_use]
    pub const fn new(
        expected_target_created_revision: Revision,
        replacement_id: AssertionId,
        replacement: AssertionDraft,
        retraction_id: AssertionRetractionId,
        retraction_reason: String,
        provenance_id: ProvenanceId,
        commit_revision: Revision,
    ) -> Self {
        Self {
            expected_target_created_revision,
            replacement_id,
            replacement,
            retraction_id,
            retraction_reason,
            provenance_id,
            commit_revision,
        }
    }
}

/// One prepared, all-or-nothing Assertion correction record set.
///
/// This reference model contains exactly the new Assertion, explicit target
/// AssertionRetraction, and `Corrects(new, old)` ProvenanceEdge. A transaction
/// writer must publish or discard the bundle as one write set.
#[derive(Clone, Debug)]
pub struct PreparedAssertionCorrection {
    replacement: Assertion,
    retraction: AssertionRetraction,
    corrects: ProvenanceEdge,
}

impl PreparedAssertionCorrection {
    /// Returns the immutable replacement Assertion.
    #[must_use]
    pub const fn replacement(&self) -> &Assertion {
        &self.replacement
    }

    /// Returns the explicit Retraction targeting the old Assertion.
    #[must_use]
    pub const fn retraction(&self) -> &AssertionRetraction {
        &self.retraction
    }

    /// Returns the explanatory `Corrects(new, old)` edge.
    #[must_use]
    pub const fn corrects(&self) -> &ProvenanceEdge {
        &self.corrects
    }

    /// Returns the common transaction revision of all three records.
    #[must_use]
    pub const fn commit_revision(&self) -> Revision {
        self.replacement.created_revision()
    }

    /// Consumes the prepared effect into its exact three-record batch.
    #[must_use]
    pub fn into_records(self) -> [Record; 3] {
        [
            Record::Assertion(self.replacement),
            Record::AssertionRetraction(self.retraction),
            Record::Provenance(self.corrects),
        ]
    }
}

/// Commits one prepared correction through the normal mixed-record transaction.
///
/// The supplied callback remains the complete engine validator. This helper
/// fixes the correction's three-record shape and revision before invoking it.
pub fn commit_assertion_correction<B, E>(
    backend: &mut B,
    base_revision: Revision,
    prepared: PreparedAssertionCorrection,
    validate: impl FnOnce(Revision, &[Record]) -> Result<(), E>,
) -> Result<Revision, AssertionCorrectionCommitError<E>>
where
    B: RevisionBackend<Record>,
{
    let actual_revision = backend.latest_published();
    if actual_revision != base_revision {
        return Err(AssertionCorrectionCommitError::Conflict {
            expected: base_revision,
            actual: actual_revision,
        });
    }
    let expected_revision = base_revision
        .next_commit()
        .map_err(AssertionCorrectionCommitError::Revision)?;
    if prepared.commit_revision() != expected_revision {
        return Err(AssertionCorrectionCommitError::PreparedRevisionMismatch {
            expected: expected_revision,
            actual: prepared.commit_revision(),
        });
    }
    let records = prepared.into_records();
    let mut transaction = OpenTransaction::begin(backend, base_revision)
        .map_err(AssertionCorrectionCommitError::Begin)?;
    for record in records {
        transaction.stage(record);
    }
    commit_mixed_record_batch(transaction, |base, records| {
        validate_correction_record_set::<E>(base, expected_revision, records)
            .map_err(|_| AssertionCorrectionCommitValidationError::InvalidRecordSet)?;
        validate(base, records).map_err(AssertionCorrectionCommitValidationError::Engine)
    })
    .map_err(AssertionCorrectionCommitError::Commit)
}

fn validate_correction_record_set<E>(
    base: Revision,
    expected_revision: Revision,
    records: &[Record],
) -> Result<(), AssertionCorrectionCommitValidationError<E>> {
    if base.next_commit() != Ok(expected_revision) || records.len() != 3 {
        return Err(AssertionCorrectionCommitValidationError::InvalidRecordSet);
    }
    let [
        Record::Assertion(replacement),
        Record::AssertionRetraction(retraction),
        Record::Provenance(edge),
    ] = records
    else {
        return Err(AssertionCorrectionCommitValidationError::InvalidRecordSet);
    };
    if replacement.created_revision() != expected_revision
        || retraction.created_revision() != expected_revision
        || retraction.assertion_id() == replacement.id()
        || edge.created_revision() != expected_revision
        || edge.relation() != ProvenanceRelation::Corrects
        || edge.from() != ProvenanceEndpointRef::Assertion(replacement.id())
        || edge.to() != ProvenanceEndpointRef::Assertion(retraction.assertion_id())
    {
        return Err(AssertionCorrectionCommitValidationError::InvalidRecordSet);
    }
    Ok(())
}

/// Failure while binding or publishing an atomic Assertion correction.
#[derive(Debug)]
pub enum AssertionCorrectionCommitError<E> {
    /// Shared revision could not advance.
    Revision(crate::ids::RevisionError),
    /// Backend advanced since the correction's base snapshot.
    Conflict {
        expected: Revision,
        actual: Revision,
    },
    /// Prepared records target a revision other than the next shared commit.
    PreparedRevisionMismatch {
        expected: Revision,
        actual: Revision,
    },
    /// Backend transaction could not begin at the requested base.
    Begin(TransactionBeginError),
    /// Fixed record-set check, engine validation, or publication failed.
    Commit(MixedRecordCommitError<AssertionCorrectionCommitValidationError<E>>),
}

/// Validation failure for the fixed correction shape or caller's full validator.
#[derive(Debug)]
pub enum AssertionCorrectionCommitValidationError<E> {
    /// Replacement, target retraction and Corrects edge are not exactly one set.
    InvalidRecordSet,
    /// Caller-supplied engine validation rejected the whole three-record set.
    Engine(E),
}

impl<E: fmt::Display> fmt::Display for AssertionCorrectionCommitError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Revision(error) => write!(formatter, "correction revision failed: {error}"),
            Self::Conflict { expected, actual } => write!(
                formatter,
                "correction base revision {expected} is stale; backend head is {actual}"
            ),
            Self::PreparedRevisionMismatch { expected, actual } => write!(
                formatter,
                "prepared correction revision {actual} does not match next commit {expected}"
            ),
            Self::Begin(error) => {
                write!(formatter, "correction transaction could not begin: {error}")
            }
            Self::Commit(error) => write!(formatter, "correction transaction failed: {error}"),
        }
    }
}

impl<E: fmt::Display> fmt::Display for AssertionCorrectionCommitValidationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRecordSet => formatter.write_str("invalid three-record correction set"),
            Self::Engine(error) => {
                write!(formatter, "correction engine validation failed: {error}")
            }
        }
    }
}

impl<E> std::error::Error for AssertionCorrectionCommitError<E> where E: std::error::Error + 'static {}

impl<E> std::error::Error for AssertionCorrectionCommitValidationError<E> where
    E: std::error::Error + 'static
{
}

/// Prepares a complete Assertion correction after validating its base target.
///
/// The target must exist at `recorded_as_of`, match the caller's expected
/// creation revision, and be active at that snapshot. Replacement context,
/// Subject, and Predicate must stay in the exact target slot. Value, polarity,
/// and validity are supplied in the complete replacement draft and may
/// change. No record is returned unless all three records can be built.
pub fn prepare_assertion_correction(
    target: &Assertion,
    target_retractions: &[AssertionRetraction],
    recorded_as_of: RecordedAsOf,
    command: AssertionCorrectionCommand,
) -> Result<PreparedAssertionCorrection, AssertionCorrectionError> {
    if command.commit_revision <= recorded_as_of.revision() {
        return Err(AssertionCorrectionError::CommitRevisionNotAfterSnapshot {
            snapshot_revision: recorded_as_of.revision(),
            commit_revision: command.commit_revision,
        });
    }
    if target.created_revision() > recorded_as_of.revision() {
        return Err(AssertionCorrectionError::TargetNotVisibleAtSnapshot {
            assertion_id: target.id(),
            created_revision: target.created_revision(),
            snapshot_revision: recorded_as_of.revision(),
        });
    }
    if command.expected_target_created_revision != target.created_revision() {
        return Err(AssertionCorrectionError::ExpectedTargetRevisionMismatch {
            assertion_id: target.id(),
            expected: command.expected_target_created_revision,
            actual: target.created_revision(),
        });
    }

    validate_target_active_at_snapshot(
        target,
        target_retractions,
        recorded_as_of,
        command.commit_revision,
    )?;

    let replacement = Assertion::new(
        command.replacement_id,
        command.replacement,
        command.commit_revision,
    );
    if replacement.context() != target.context() {
        return Err(AssertionCorrectionError::CorrectionContextMismatch {
            assertion_id: target.id(),
        });
    }
    if replacement.subject() != target.subject()
        || replacement.predicate_id() != target.predicate_id()
    {
        return Err(
            AssertionCorrectionError::CorrectionPropositionSlotMismatch {
                assertion_id: target.id(),
            },
        );
    }

    let retraction = AssertionRetraction::new(
        command.retraction_id,
        target,
        command.retraction_reason,
        command.commit_revision,
    )?;
    let corrects = ProvenanceEdge::corrects_assertion(command.provenance_id, &replacement, target)?;
    Ok(PreparedAssertionCorrection {
        replacement,
        retraction,
        corrects,
    })
}

fn validate_target_active_at_snapshot(
    target: &Assertion,
    retractions: &[AssertionRetraction],
    recorded_as_of: RecordedAsOf,
    commit_revision: Revision,
) -> Result<(), AssertionCorrectionError> {
    let mut ids = HashSet::new();
    for retraction in retractions {
        if !ids.insert(retraction.id()) {
            return Err(AssertionCorrectionError::DuplicateTargetRetractionId {
                retraction_id: retraction.id(),
            });
        }
        if retraction.assertion_id() != target.id() {
            continue;
        }
        if retraction.created_revision() <= target.created_revision() {
            return Err(AssertionRecordError::LifecycleRevisionNotAfterAssertion {
                assertion_revision: target.created_revision(),
                lifecycle_revision: retraction.created_revision(),
            }
            .into());
        }
        if retraction.created_revision() <= recorded_as_of.revision() {
            return Err(AssertionCorrectionError::TargetAlreadyRetracted {
                assertion_id: target.id(),
                retraction_id: retraction.id(),
                retracted_at: retraction.created_revision(),
            });
        }
        if retraction.created_revision() <= commit_revision {
            return Err(AssertionCorrectionError::TargetRetractedSinceSnapshot {
                assertion_id: target.id(),
                retraction_id: retraction.id(),
                retracted_at: retraction.created_revision(),
                snapshot_revision: recorded_as_of.revision(),
                commit_revision,
            });
        }
    }
    Ok(())
}

/// A rejected or stale Assertion correction command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssertionCorrectionError {
    /// The command commit revision must follow its base snapshot.
    CommitRevisionNotAfterSnapshot {
        /// Base snapshot revision.
        snapshot_revision: Revision,
        /// Proposed commit revision.
        commit_revision: Revision,
    },
    /// The target record was not yet visible at the base snapshot.
    TargetNotVisibleAtSnapshot {
        /// Requested Assertion.
        assertion_id: AssertionId,
        /// Target creation revision.
        created_revision: Revision,
        /// Selected base revision.
        snapshot_revision: Revision,
    },
    /// The immutable target creation revision differs from the caller's expectation.
    ExpectedTargetRevisionMismatch {
        /// Requested Assertion.
        assertion_id: AssertionId,
        /// Caller-provided expected revision.
        expected: Revision,
        /// Actual immutable target revision.
        actual: Revision,
    },
    /// Target is already retracted at the base snapshot.
    TargetAlreadyRetracted {
        /// Requested Assertion.
        assertion_id: AssertionId,
        /// Existing Retraction record.
        retraction_id: AssertionRetractionId,
        /// Effective transaction revision.
        retracted_at: Revision,
    },
    /// Target became inactive after the base snapshot and before the candidate commit.
    TargetRetractedSinceSnapshot {
        /// Requested Assertion.
        assertion_id: AssertionId,
        /// Concurrent Retraction record.
        retraction_id: AssertionRetractionId,
        /// Retraction transaction revision.
        retracted_at: Revision,
        /// Pinned base snapshot revision.
        snapshot_revision: Revision,
        /// Proposed correction commit revision.
        commit_revision: Revision,
    },
    /// Retraction identities in the supplied target history are duplicated.
    DuplicateTargetRetractionId {
        /// Repeated Retraction identity.
        retraction_id: AssertionRetractionId,
    },
    /// Replacement does not remain in the target's exact context.
    CorrectionContextMismatch {
        /// Target Assertion.
        assertion_id: AssertionId,
    },
    /// Replacement changes Subject or Predicate instead of correcting the same slot.
    CorrectionPropositionSlotMismatch {
        /// Target Assertion.
        assertion_id: AssertionId,
    },
    /// Underlying Assertion lifecycle record validation failed.
    Assertion(AssertionRecordError),
    /// Underlying Provenance endpoint validation failed.
    Provenance(SourceEvidenceProvenanceError),
}

impl From<AssertionRecordError> for AssertionCorrectionError {
    fn from(error: AssertionRecordError) -> Self {
        Self::Assertion(error)
    }
}

impl From<SourceEvidenceProvenanceError> for AssertionCorrectionError {
    fn from(error: SourceEvidenceProvenanceError) -> Self {
        Self::Provenance(error)
    }
}

impl fmt::Display for AssertionCorrectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommitRevisionNotAfterSnapshot {
                snapshot_revision,
                commit_revision,
            } => write!(
                formatter,
                "correction commit revision {commit_revision} must follow snapshot {snapshot_revision}"
            ),
            Self::TargetNotVisibleAtSnapshot {
                assertion_id,
                created_revision,
                snapshot_revision,
            } => write!(
                formatter,
                "Assertion {assertion_id} created at {created_revision} is not visible at snapshot {snapshot_revision}"
            ),
            Self::ExpectedTargetRevisionMismatch {
                assertion_id,
                expected,
                actual,
            } => write!(
                formatter,
                "Assertion {assertion_id} expected revision {expected}, found {actual}"
            ),
            Self::TargetAlreadyRetracted {
                assertion_id,
                retraction_id,
                retracted_at,
            } => write!(
                formatter,
                "Assertion {assertion_id} was retracted by {retraction_id} at {retracted_at}"
            ),
            Self::TargetRetractedSinceSnapshot {
                assertion_id,
                retraction_id,
                retracted_at,
                snapshot_revision,
                commit_revision,
            } => write!(
                formatter,
                "Assertion {assertion_id} was retracted by {retraction_id} at {retracted_at} after snapshot {snapshot_revision} and before commit {commit_revision}"
            ),
            Self::DuplicateTargetRetractionId { retraction_id } => {
                write!(
                    formatter,
                    "duplicate target AssertionRetraction ID {retraction_id}"
                )
            }
            Self::CorrectionContextMismatch { assertion_id } => write!(
                formatter,
                "replacement for Assertion {assertion_id} must keep its exact context"
            ),
            Self::CorrectionPropositionSlotMismatch { assertion_id } => write!(
                formatter,
                "replacement for Assertion {assertion_id} must keep Subject and Predicate"
            ),
            Self::Assertion(error) => write!(formatter, "{error}"),
            Self::Provenance(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for AssertionCorrectionError {}

#[cfg(test)]
mod tests {
    use super::{
        AssertionCorrectionCommand, AssertionCorrectionCommitError, AssertionCorrectionError,
        commit_assertion_correction, prepare_assertion_correction,
    };
    use crate::assertions::{Assertion, AssertionDraft, AssertionRetraction, Polarity, Subject};
    use crate::context::{ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{
        AssertionId, AssertionRetractionId, DomainId, EntityId, HistorySpaceId, LayerId,
        PredicateId, ProvenanceId, Revision, RevisionError, TimelineId,
    };
    use crate::revision_backend::{InMemoryRevisionBackend, RevisionBackend};
    use crate::revision_history::RevisionLogError;
    use crate::source_provenance::{ProvenanceEndpointRef, ProvenanceRelation};
    use crate::temporal::{
        AssertionValidity, RecordedAsOf, TemporalError, TimeInterval, Timeline, WorldTime,
    };
    use crate::values::{SymbolError, Value};
    use crate::wire_records::Record;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(crate::ids::IdValidationError),
        Revision(RevisionError),
        Context(crate::context::ContextError),
        Temporal(TemporalError),
        Symbol(SymbolError),
        Correction(AssertionCorrectionError),
        CorrectionCommit(AssertionCorrectionCommitError<RevisionLogError>),
        Backend(RevisionLogError),
        Assertion(crate::assertions::AssertionRecordError),
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Revision(error) => write!(formatter, "{error}"),
                Self::Context(error) => write!(formatter, "{error}"),
                Self::Temporal(error) => write!(formatter, "{error}"),
                Self::Symbol(error) => write!(formatter, "{error}"),
                Self::Correction(error) => write!(formatter, "{error}"),
                Self::CorrectionCommit(error) => write!(formatter, "{error}"),
                Self::Backend(error) => write!(formatter, "{error}"),
                Self::Assertion(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl std::error::Error for TestError {}

    macro_rules! convert_error {
        ($type:ty, $variant:ident) => {
            impl From<$type> for TestError {
                fn from(error: $type) -> Self {
                    Self::$variant(error)
                }
            }
        };
    }

    convert_error!(crate::ids::IdValidationError, Id);
    convert_error!(RevisionError, Revision);
    convert_error!(crate::context::ContextError, Context);
    convert_error!(TemporalError, Temporal);
    convert_error!(SymbolError, Symbol);
    convert_error!(AssertionCorrectionError, Correction);
    convert_error!(
        AssertionCorrectionCommitError<RevisionLogError>,
        CorrectionCommit
    );
    convert_error!(RevisionLogError, Backend);
    convert_error!(crate::assertions::AssertionRecordError, Assertion);

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    fn uuid<T: DomainId>(byte: u8) -> Result<T, crate::ids::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    fn revision(value: u64) -> Result<Revision, RevisionError> {
        Revision::new(value)
    }

    fn target(created: u64) -> Result<Assertion, TestError> {
        assertion(
            1,
            (2, 3),
            (4, 5),
            6,
            created,
            "old value",
            Polarity::Positive,
        )
    }

    fn assertion(
        id: u8,
        context_ids: (u8, u8),
        proposition_ids: (u8, u8),
        timeline_id: u8,
        created: u64,
        value: &str,
        polarity: Polarity,
    ) -> Result<Assertion, TestError> {
        let timeline = Timeline::new(value!(uuid::<TimelineId>(timeline_id)));
        let context = value!(ContextKey::new(
            value!(uuid::<HistorySpaceId>(context_ids.0)),
            value!(uuid::<LayerId>(context_ids.1)),
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        ));
        let validity = AssertionValidity::new(value!(TimeInterval::new(
            timeline,
            Some(WorldTime::from_nanoseconds(timeline, 10)),
            None,
        )));
        Ok(Assertion::new(
            value!(uuid::<AssertionId>(id)),
            AssertionDraft::new(
                context,
                Subject::new(value!(uuid::<EntityId>(proposition_ids.0))),
                value!(uuid::<PredicateId>(proposition_ids.1)),
                Value::String(value.to_owned()),
                polarity,
                validity,
            ),
            value!(revision(created)),
        ))
    }

    fn draft(target: &Assertion, value: &str, polarity: Polarity) -> AssertionDraft {
        AssertionDraft::new(
            target.context(),
            target.subject(),
            target.predicate_id(),
            Value::String(value.to_owned()),
            polarity,
            target.validity(),
        )
    }

    fn command(
        expected_revision: u64,
        created_revision: u64,
        replacement: AssertionDraft,
    ) -> Result<AssertionCorrectionCommand, TestError> {
        Ok(AssertionCorrectionCommand::new(
            value!(revision(expected_revision)),
            value!(uuid::<AssertionId>(8)),
            replacement,
            value!(uuid::<AssertionRetractionId>(9)),
            String::from("explicit correction"),
            value!(uuid::<ProvenanceId>(10)),
            value!(revision(created_revision)),
        ))
    }

    #[test]
    fn correction_prepares_exactly_three_records_at_one_commit_revision() -> TestResult {
        let target = target(2)?;
        let command = command(2, 4, draft(&target, "corrected value", Polarity::Negative))?;
        let prepared = prepare_assertion_correction(
            &target,
            &[],
            RecordedAsOf::from_published_revision(value!(revision(3))),
            command,
        )?;
        let commit_revision = value!(revision(4));
        assert_eq!(prepared.commit_revision(), commit_revision);
        assert_eq!(prepared.replacement().created_revision(), commit_revision);
        assert_eq!(prepared.retraction().created_revision(), commit_revision);
        assert_eq!(prepared.corrects().created_revision(), commit_revision);
        assert_eq!(prepared.retraction().assertion_id(), target.id());
        assert_eq!(
            prepared.corrects().from(),
            ProvenanceEndpointRef::Assertion(prepared.replacement().id())
        );
        assert_eq!(
            prepared.corrects().to(),
            ProvenanceEndpointRef::Assertion(target.id())
        );
        assert_eq!(prepared.corrects().relation(), ProvenanceRelation::Corrects);
        assert!(
            matches!(prepared.replacement().value(), Value::String(value) if value == "corrected value")
        );
        assert_eq!(prepared.replacement().polarity(), Polarity::Negative);
        assert_eq!(target.created_revision(), value!(revision(2)));
        assert!(matches!(target.value(), Value::String(value) if value == "old value"));
        Ok(())
    }

    #[test]
    fn correction_commit_publishes_only_the_exact_three_record_effect() -> TestResult {
        let target = target(1)?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        assert_eq!(
            backend.publish(vec![Record::Assertion(target.clone())])?,
            revision(1)?
        );
        let prepared = prepare_assertion_correction(
            &target,
            &[],
            RecordedAsOf::from_published_revision(revision(1)?),
            command(1, 2, draft(&target, "corrected", Polarity::Negative))?,
        )?;
        let committed =
            commit_assertion_correction(&mut backend, revision(1)?, prepared, |base, records| {
                if base == Revision::FIRST_COMMIT && records.len() == 3 {
                    Ok(())
                } else {
                    Err(RevisionLogError::NoReservation)
                }
            })?;
        assert_eq!(committed, revision(2)?);
        let committed_records = backend
            .read_at(committed)?
            .filter(|(at, _)| *at == committed)
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        assert_eq!(committed_records.len(), 3);
        assert!(matches!(
            committed_records.as_slice(),
            [
                Record::Assertion(_),
                Record::AssertionRetraction(_),
                Record::Provenance(_)
            ]
        ));
        Ok(())
    }

    #[test]
    fn correction_rejects_wrong_slot_or_context_without_returning_any_record() -> TestResult {
        let target = target(2)?;
        let other_slot = assertion(
            11,
            (2, 3),
            (12, 5),
            7,
            4,
            "wrong subject",
            Polarity::Positive,
        )?;
        let slot_error = prepare_assertion_correction(
            &target,
            &[],
            RecordedAsOf::from_published_revision(value!(revision(3))),
            command(2, 4, draft(&other_slot, "wrong", Polarity::Positive))?,
        );
        assert_eq!(
            slot_error.err(),
            Some(
                AssertionCorrectionError::CorrectionPropositionSlotMismatch {
                    assertion_id: target.id(),
                }
            )
        );

        let other_context = assertion(
            13,
            (14, 3),
            (4, 5),
            7,
            4,
            "wrong context",
            Polarity::Positive,
        )?;
        let context_error = prepare_assertion_correction(
            &target,
            &[],
            RecordedAsOf::from_published_revision(value!(revision(3))),
            command(2, 4, draft(&other_context, "wrong", Polarity::Positive))?,
        );
        assert_eq!(
            context_error.err(),
            Some(AssertionCorrectionError::CorrectionContextMismatch {
                assertion_id: target.id(),
            })
        );
        Ok(())
    }

    #[test]
    fn correction_rejects_stale_future_or_retracted_targets() -> TestResult {
        let target = target(2)?;
        let stale = prepare_assertion_correction(
            &target,
            &[],
            RecordedAsOf::from_published_revision(value!(revision(3))),
            command(1, 4, draft(&target, "new", Polarity::Positive))?,
        );
        assert!(matches!(
            stale,
            Err(AssertionCorrectionError::ExpectedTargetRevisionMismatch { .. })
        ));

        let future = prepare_assertion_correction(
            &target,
            &[],
            RecordedAsOf::from_published_revision(value!(revision(1))),
            command(2, 4, draft(&target, "new", Polarity::Positive))?,
        );
        assert!(matches!(
            future,
            Err(AssertionCorrectionError::TargetNotVisibleAtSnapshot { .. })
        ));

        let retraction = AssertionRetraction::new(
            value!(uuid::<AssertionRetractionId>(15)),
            &target,
            "already corrected",
            value!(revision(3)),
        )?;
        let retracted = prepare_assertion_correction(
            &target,
            &[retraction],
            RecordedAsOf::from_published_revision(value!(revision(3))),
            command(2, 4, draft(&target, "new", Polarity::Positive))?,
        );
        assert!(matches!(
            retracted,
            Err(AssertionCorrectionError::TargetAlreadyRetracted { .. })
        ));

        let intervening_retraction = AssertionRetraction::new(
            value!(uuid::<AssertionRetractionId>(16)),
            &target,
            "concurrent correction",
            value!(revision(4)),
        )?;
        let stale_snapshot = prepare_assertion_correction(
            &target,
            &[intervening_retraction],
            RecordedAsOf::from_published_revision(value!(revision(3))),
            command(2, 5, draft(&target, "new", Polarity::Positive))?,
        );
        assert!(matches!(
            stale_snapshot,
            Err(AssertionCorrectionError::TargetRetractedSinceSnapshot { .. })
        ));
        Ok(())
    }

    #[test]
    fn correction_commit_must_follow_the_base_snapshot() -> TestResult {
        let target = target(2)?;
        let result = prepare_assertion_correction(
            &target,
            &[],
            RecordedAsOf::from_published_revision(value!(revision(4))),
            command(2, 4, draft(&target, "new", Polarity::Positive))?,
        );
        assert_eq!(
            result.err(),
            Some(AssertionCorrectionError::CommitRevisionNotAfterSnapshot {
                snapshot_revision: value!(revision(4)),
                commit_revision: value!(revision(4)),
            })
        );
        Ok(())
    }
}
