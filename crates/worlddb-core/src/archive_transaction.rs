//! Commit-time validation for explicit operational archive transitions.

use std::fmt;

use crate::archive::{ArchiveAction, ArchiveTargetRef, ArchiveTransition};
use crate::archive_projection::{
    ArchiveHistoryReferenceModel, ArchiveProjectionError, ArchiveTargetRecord,
};
use crate::ids::{PrincipalId, Revision};
use crate::record_refs::RecordRef;
use crate::security::{AuthorizationDecision, Capability, PolicyTarget, SecurityPolicySnapshot};
use crate::wire_records::{Record, RecordCodecError, encode_record};

/// Revalidates an append-only Archive post-state before the transaction publishes.
///
/// `staged_records` may contain the complete mixed transaction. Only its
/// ArchiveTransition subset is compared with `transition_additions`; a newly
/// introduced archive target must also have its concrete target Record staged.
pub struct ArchiveTransitionValidation<'a> {
    /// Revision pinned by the open transaction.
    pub base_revision: Revision,
    /// Revision assigned to this commit.
    pub commit_revision: Revision,
    /// Immutable archive projection at the pinned base.
    pub base: &'a ArchiveHistoryReferenceModel,
    /// Complete target inventory after this transaction.
    pub target_records_after: Vec<ArchiveTargetRecord>,
    /// New ArchiveTransition records in this transaction.
    pub transition_additions: &'a [ArchiveTransition],
    /// Current policy revalidated at commit.
    pub policy: &'a SecurityPolicySnapshot,
    /// Authenticated principal performing the operation.
    pub principal: PrincipalId,
}

/// Revalidates an append-only Archive post-state before the transaction publishes.
pub fn validate_archive_transition_transaction(
    candidate: ArchiveTransitionValidation<'_>,
    staged_records: &[Record],
) -> Result<ArchiveHistoryReferenceModel, ArchiveTransactionError> {
    let ArchiveTransitionValidation {
        base_revision,
        commit_revision,
        base,
        target_records_after,
        transition_additions,
        policy,
        principal,
    } = candidate;
    if commit_revision <= base_revision {
        return Err(ArchiveTransactionError::RevisionNotIncreasing);
    }
    if base
        .targets()
        .iter()
        .any(|target| target.created_revision() > base_revision)
        || base
            .transitions()
            .iter()
            .any(|transition| transition.created_revision() > base_revision)
    {
        return Err(ArchiveTransactionError::BaseContainsFutureHistory);
    }

    for previous in base.targets() {
        if !target_records_after.contains(previous) {
            return Err(ArchiveTransactionError::TargetHistoryChanged);
        }
    }
    for target in &target_records_after {
        if !base.targets().contains(target) {
            if target.created_revision() != commit_revision {
                return Err(ArchiveTransactionError::NewTargetRevisionMismatch {
                    target: target.target(),
                    expected: commit_revision,
                    actual: target.created_revision(),
                });
            }
            if staged_records
                .iter()
                .filter(|record| {
                    record_archive_target(record) == Some(target.target())
                        && record_archive_created_revision(record) == Some(commit_revision)
                })
                .count()
                != 1
            {
                return Err(ArchiveTransactionError::TargetRecordMissing {
                    target: target.target(),
                });
            }
        }
    }

    for transition in transition_additions {
        if transition.created_revision() != commit_revision {
            return Err(ArchiveTransactionError::TransitionRevisionMismatch {
                transition_id: transition.id(),
                expected: commit_revision,
                actual: transition.created_revision(),
            });
        }
        let record_ref = archive_target_record_ref(transition.target());
        let capability = match transition.action() {
            ArchiveAction::Archive => Capability::Archive,
            ArchiveAction::Unarchive => Capability::Unarchive,
        };
        require(
            policy,
            principal,
            capability,
            PolicyTarget::new(None, None, Some(record_ref), None, None),
        )?;
    }

    let mut expected = transition_additions
        .iter()
        .copied()
        .map(Record::ArchiveTransition)
        .map(|record| encode_record(&record))
        .collect::<Result<Vec<_>, _>>()?;
    let mut actual = staged_records
        .iter()
        .filter(|record| matches!(record, Record::ArchiveTransition(_)))
        .map(encode_record)
        .collect::<Result<Vec<_>, _>>()?;
    expected.sort();
    actual.sort();
    if expected != actual {
        return Err(ArchiveTransactionError::ArchiveBatchMismatch);
    }

    let mut transitions_after = base.transitions().to_vec();
    transitions_after.extend_from_slice(transition_additions);
    Ok(ArchiveHistoryReferenceModel::new(
        target_records_after,
        transitions_after,
    )?)
}

fn require(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    capability: Capability,
    target: PolicyTarget,
) -> Result<(), ArchiveTransactionError> {
    if policy.authorize(principal, capability, target) == AuthorizationDecision::Allow {
        Ok(())
    } else {
        Err(ArchiveTransactionError::Denied { capability, target })
    }
}

fn archive_target_record_ref(target: ArchiveTargetRef) -> RecordRef {
    match target {
        ArchiveTargetRef::Assertion(id) => RecordRef::Assertion(id),
        ArchiveTargetRef::Mask(id) => RecordRef::Mask(id),
        ArchiveTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ArchiveTargetRef::Event(id) => RecordRef::Event(id),
        ArchiveTargetRef::EventMask(id) => RecordRef::EventMask(id),
        ArchiveTargetRef::EventRelation(id) => RecordRef::EventRelation(id),
        ArchiveTargetRef::Source(id) => RecordRef::Source(id),
        ArchiveTargetRef::Evidence(id) => RecordRef::Evidence(id),
        ArchiveTargetRef::Provenance(id) => RecordRef::Provenance(id),
        ArchiveTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        ArchiveTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ArchiveTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ArchiveTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ArchiveTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ArchiveTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ArchiveTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ArchiveTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ArchiveTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ArchiveTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        ArchiveTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ArchiveTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ArchiveTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ArchiveTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ArchiveTargetRef::TransferLineage(id) => RecordRef::TransferLineage(id),
    }
}

fn record_archive_target(record: &Record) -> Option<ArchiveTargetRef> {
    Some(match record {
        Record::Assertion(value) => ArchiveTargetRef::Assertion(value.id()),
        Record::Mask(value) => ArchiveTargetRef::Mask(value.id()),
        Record::ReplacementBoundary(value) => ArchiveTargetRef::ReplacementBoundary(value.id()),
        Record::Event(value) => ArchiveTargetRef::Event(value.id()),
        Record::EventMask(value) => ArchiveTargetRef::EventMask(value.id()),
        Record::EventRelation(value) => ArchiveTargetRef::EventRelation(value.id()),
        Record::Source(value) => ArchiveTargetRef::Source(value.id()),
        Record::Evidence(value) => ArchiveTargetRef::Evidence(value.id()),
        Record::Provenance(value) => ArchiveTargetRef::Provenance(value.id()),
        Record::AssertionValidityClosure(value) => {
            ArchiveTargetRef::AssertionValidityClosure(value.id())
        }
        Record::AssertionRetraction(value) => ArchiveTargetRef::AssertionRetraction(value.id()),
        Record::MaskValidityClosure(value) => ArchiveTargetRef::MaskValidityClosure(value.id()),
        Record::MaskRetraction(value) => ArchiveTargetRef::MaskRetraction(value.id()),
        Record::ReplacementBoundaryValidityClosure(value) => {
            ArchiveTargetRef::ReplacementBoundaryValidityClosure(value.id())
        }
        Record::ReplacementBoundaryRetraction(value) => {
            ArchiveTargetRef::ReplacementBoundaryRetraction(value.id())
        }
        Record::EventSpanClosure(value) => ArchiveTargetRef::EventSpanClosure(value.id()),
        Record::EventRetraction(value) => ArchiveTargetRef::EventRetraction(value.id()),
        Record::EventMaskRetraction(value) => ArchiveTargetRef::EventMaskRetraction(value.id()),
        Record::EventRelationRetraction(value) => {
            ArchiveTargetRef::EventRelationRetraction(value.id())
        }
        Record::EvidenceRetraction(value) => ArchiveTargetRef::EvidenceRetraction(value.id()),
        Record::ProvenanceRetraction(value) => ArchiveTargetRef::ProvenanceRetraction(value.id()),
        Record::EntityRetirement(value) => {
            ArchiveTargetRef::EntityRetirement(value.entity_retirement_id())
        }
        Record::PerspectiveRetirement(value) => {
            ArchiveTargetRef::PerspectiveRetirement(value.perspective_retirement_id())
        }
        Record::TransferLineage(value) => ArchiveTargetRef::TransferLineage(value.id()),
        _ => return None,
    })
}

fn record_archive_created_revision(record: &Record) -> Option<Revision> {
    Some(match record {
        Record::Assertion(value) => value.created_revision(),
        Record::Mask(value) => value.created_revision(),
        Record::ReplacementBoundary(value) => value.created_revision(),
        Record::Event(value) => value.created_revision(),
        Record::EventMask(value) => value.created_revision(),
        Record::EventRelation(value) => value.created_revision(),
        Record::Source(value) => value.created_revision(),
        Record::Evidence(value) => value.created_revision(),
        Record::Provenance(value) => value.created_revision(),
        Record::AssertionValidityClosure(value) => value.created_revision(),
        Record::AssertionRetraction(value) => value.created_revision(),
        Record::MaskValidityClosure(value) => value.created_revision(),
        Record::MaskRetraction(value) => value.created_revision(),
        Record::ReplacementBoundaryValidityClosure(value) => value.created_revision(),
        Record::ReplacementBoundaryRetraction(value) => value.created_revision(),
        Record::EventSpanClosure(value) => value.created_revision(),
        Record::EventRetraction(value) => value.created_revision(),
        Record::EventMaskRetraction(value) => value.created_revision(),
        Record::EventRelationRetraction(value) => value.created_revision(),
        Record::EvidenceRetraction(value) => value.created_revision(),
        Record::ProvenanceRetraction(value) => value.created_revision(),
        Record::EntityRetirement(value) => value.created_revision(),
        Record::PerspectiveRetirement(value) => value.created_revision(),
        Record::TransferLineage(value) => value.created_revision(),
        _ => return None,
    })
}

/// Rejected or inconsistent archive Transaction post-state.
#[derive(Debug)]
pub enum ArchiveTransactionError {
    /// The commit revision must follow the pinned base.
    RevisionNotIncreasing,
    /// The base archive projection contains target or transition rows after its pin.
    BaseContainsFutureHistory,
    /// Previously known target records cannot be removed or rewritten.
    TargetHistoryChanged,
    /// A new archive target must be introduced at the same revision as this commit.
    NewTargetRevisionMismatch {
        /// Target identity with an invalid creation revision.
        target: ArchiveTargetRef,
        /// Required current commit revision.
        expected: Revision,
        /// Declared creation revision.
        actual: Revision,
    },
    /// A new target's concrete record must appear in the staged mixed batch.
    TargetRecordMissing {
        /// Target identity absent from the staged records.
        target: ArchiveTargetRef,
    },
    /// Every new transition must use the transaction's assigned revision.
    TransitionRevisionMismatch {
        /// Transition identity with an invalid revision.
        transition_id: crate::ids::ArchiveTransitionId,
        /// Required current commit revision.
        expected: Revision,
        /// Declared creation revision.
        actual: Revision,
    },
    /// The staged ArchiveTransition subset differs from the supplied additions.
    ArchiveBatchMismatch,
    /// Current policy denied one exact archive target.
    Denied {
        /// Required operation capability.
        capability: Capability,
        /// Exact target evaluated by current policy.
        target: PolicyTarget,
    },
    /// Full append-only archive state validation failed.
    Projection(ArchiveProjectionError),
    /// Canonical Record encoding failed during exact batch validation.
    RecordCodec(RecordCodecError),
}

impl From<ArchiveProjectionError> for ArchiveTransactionError {
    fn from(error: ArchiveProjectionError) -> Self {
        Self::Projection(error)
    }
}

impl From<RecordCodecError> for ArchiveTransactionError {
    fn from(error: RecordCodecError) -> Self {
        Self::RecordCodec(error)
    }
}

impl fmt::Display for ArchiveTransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RevisionNotIncreasing => {
                formatter.write_str("archive commit revision must advance")
            }
            Self::BaseContainsFutureHistory => {
                formatter.write_str("archive base contains future rows")
            }
            Self::TargetHistoryChanged => {
                formatter.write_str("archive target history cannot be rewritten")
            }
            Self::NewTargetRevisionMismatch {
                target,
                expected,
                actual,
            } => write!(
                formatter,
                "archive target {target:?} uses revision {actual}; commit is {expected}"
            ),
            Self::TargetRecordMissing { target } => write!(
                formatter,
                "archive target {target:?} is absent from staged records"
            ),
            Self::TransitionRevisionMismatch {
                transition_id,
                expected,
                actual,
            } => write!(
                formatter,
                "archive transition {transition_id} uses revision {actual}; commit is {expected}"
            ),
            Self::ArchiveBatchMismatch => formatter
                .write_str("staged ArchiveTransition Records do not match the candidate delta"),
            Self::Denied { capability, .. } => {
                write!(formatter, "archive operation requires {capability:?}")
            }
            Self::Projection(error) => write!(formatter, "invalid archive projection: {error}"),
            Self::RecordCodec(error) => {
                write!(formatter, "archive record encoding failed: {error}")
            }
        }
    }
}

impl std::error::Error for ArchiveTransactionError {}

#[cfg(test)]
mod tests {
    use super::{ArchiveTransitionValidation, validate_archive_transition_transaction};
    use crate::archive::{ArchiveAction, ArchiveState, ArchiveTargetRef, ArchiveTransition};
    use crate::archive_projection::{ArchiveHistoryReferenceModel, ArchiveTargetRecord};
    use crate::ids::{
        ArchiveTransitionId, AssertionId, DomainId, PolicyRuleId, PrincipalId, Revision,
    };
    use crate::record_refs::RecordRef;
    use crate::revision_backend::{InMemoryRevisionBackend, RevisionBackend};
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, GrantEffect, PolicyScope, PolicySubject,
        Principal, SecurityPolicySnapshot,
    };
    use crate::temporal::RecordedAsOf;
    use crate::transaction_flow::{OpenTransaction, commit_mixed_record_batch};
    use crate::wire_records::Record;
    use std::fmt;

    #[derive(Debug)]
    struct TestError(String);

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.0)
        }
    }

    impl std::error::Error for TestError {}

    macro_rules! error_from {
        ($error:ty) => {
            impl From<$error> for TestError {
                fn from(error: $error) -> Self {
                    Self(error.to_string())
                }
            }
        };
    }

    error_from!(crate::IdValidationError);
    error_from!(crate::SecurityPolicyError);
    error_from!(crate::ArchiveTransitionError);
    error_from!(crate::ArchiveProjectionError);
    error_from!(crate::TransactionBeginError);
    error_from!(super::ArchiveTransactionError);
    error_from!(crate::RevisionLogError);
    error_from!(crate::MixedRecordCommitError<super::ArchiveTransactionError>);

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn policy(
        principal: PrincipalId,
        target: RecordRef,
        allow: bool,
    ) -> Result<SecurityPolicySnapshot, TestError> {
        let rules = if allow {
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(1)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::Archive, GrantEffect::Allow),
                PolicyScope::new(None, None, Some(target), None, None),
            )]
        } else {
            vec![]
        };
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
            .map_err(Into::into)
    }

    fn base() -> Result<(ArchiveHistoryReferenceModel, AssertionId, AssertionId), TestError> {
        let first = id::<AssertionId>(2)?;
        let second = id::<AssertionId>(3)?;
        Ok((
            ArchiveHistoryReferenceModel::new(
                vec![
                    ArchiveTargetRecord::new(ArchiveTargetRef::Assertion(first), Revision::GENESIS),
                    ArchiveTargetRecord::new(
                        ArchiveTargetRef::Assertion(second),
                        Revision::GENESIS,
                    ),
                ],
                vec![],
            )?,
            first,
            second,
        ))
    }

    #[test]
    fn archive_transition_publishes_once_and_does_not_cascade() -> Result<(), TestError> {
        let (base, target, untouched) = base()?;
        let principal = id::<PrincipalId>(4)?;
        let transition = ArchiveTransition::new(
            id::<ArchiveTransitionId>(5)?,
            ArchiveTargetRef::Assertion(target),
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            Revision::FIRST_COMMIT,
        )?;
        let policy = policy(principal, RecordRef::Assertion(target), true)?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let mut transaction = OpenTransaction::begin(&mut backend, Revision::GENESIS)?;
        transaction.stage(Record::ArchiveTransition(transition));
        let mut candidate = None;
        let revision = commit_mixed_record_batch(transaction, |base_revision, records| {
            let model = validate_archive_transition_transaction(
                ArchiveTransitionValidation {
                    base_revision,
                    commit_revision: Revision::FIRST_COMMIT,
                    base: &base,
                    target_records_after: base.targets().to_vec(),
                    transition_additions: &[transition],
                    policy: &policy,
                    principal,
                },
                records,
            )?;
            candidate = Some(model);
            Ok::<_, super::ArchiveTransactionError>(())
        })?;

        assert_eq!(revision, Revision::FIRST_COMMIT);
        assert_eq!(backend.latest_published(), revision);
        let committed = candidate.ok_or_else(|| TestError("candidate missing".to_owned()))?;
        assert_eq!(
            committed.state_at(
                ArchiveTargetRef::Assertion(target),
                RecordedAsOf::from_published_revision(revision)
            )?,
            ArchiveState::Archived
        );
        assert_eq!(
            committed.state_at(
                ArchiveTargetRef::Assertion(untouched),
                RecordedAsOf::from_published_revision(revision)
            )?,
            ArchiveState::Unarchived
        );
        Ok(())
    }

    #[test]
    fn denied_archive_transaction_leaves_backend_unpublished() -> Result<(), TestError> {
        let (base, target, _) = base()?;
        let principal = id::<PrincipalId>(6)?;
        let transition = ArchiveTransition::new(
            id::<ArchiveTransitionId>(7)?,
            ArchiveTargetRef::Assertion(target),
            ArchiveAction::Archive,
            ArchiveState::Unarchived,
            Revision::FIRST_COMMIT,
        )?;
        let policy = policy(principal, RecordRef::Assertion(target), false)?;
        let mut backend = InMemoryRevisionBackend::<Record>::new();
        let mut transaction = OpenTransaction::begin(&mut backend, Revision::GENESIS)?;
        transaction.stage(Record::ArchiveTransition(transition));
        let result = commit_mixed_record_batch(transaction, |base_revision, records| {
            validate_archive_transition_transaction(
                ArchiveTransitionValidation {
                    base_revision,
                    commit_revision: Revision::FIRST_COMMIT,
                    base: &base,
                    target_records_after: base.targets().to_vec(),
                    transition_additions: &[transition],
                    policy: &policy,
                    principal,
                },
                records,
            )
            .map(|_| ())
        });
        assert!(result.is_err());
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(backend.commits().is_empty());
        Ok(())
    }
}
