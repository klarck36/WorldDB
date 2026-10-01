//! Atomic security-policy, SecurityEpoch, and Required Audit transactions.

use std::fmt;

use crate::audit::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord,
};
use crate::ids::{
    OperationId, PolicyRuleId, PrincipalId, Revision, RoleAssignmentId, RoleId, SecurityEpoch,
    SecurityEpochError, SecurityPolicyRecordId,
};
use crate::revision_backend::{CancellablePublishError, RevisionBackend};
use crate::security::{
    AuthorizationDecision, Capability, GrantEffect, PolicyScope, PolicySubject, PolicyTarget,
    PrincipalState, SecurityPolicyHistory, SecurityPolicyHistoryError, SecurityPolicySnapshot,
    SecurityPolicyVersion,
};
use crate::transaction_flow::{OpenTransaction, TransactionBeginError};
use crate::values::Symbol;

/// The policy version and its required audit record, committed as one backend value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityPolicyCommitBatch {
    policy_record: SecurityPolicyRecord,
    version: SecurityPolicyVersion,
    audit_record: AuditRecord,
}

/// One immutable principal, role, assignment, or capability mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecurityPolicyChange {
    /// Registers a new active authenticated principal.
    PrincipalRegistered { principal_id: PrincipalId },
    /// Appends a nonterminal or terminal principal state transition.
    PrincipalStateChanged {
        principal_id: PrincipalId,
        state: PrincipalState,
    },
    /// Registers a new role with its stable validated symbol.
    RoleRegistered { role_id: RoleId, symbol: Symbol },
    /// Retires a role without rewriting prior policy history.
    RoleRetired { role_id: RoleId },
    /// Assigns a role to a principal within one exact scope.
    RoleAssigned {
        assignment_id: RoleAssignmentId,
        principal_id: PrincipalId,
        role_id: RoleId,
        scope: PolicyScope,
    },
    /// Revokes one role assignment.
    RoleAssignmentRevoked { assignment_id: RoleAssignmentId },
    /// Adds one explicit allow or deny capability rule.
    CapabilityRuleAdded {
        rule_id: PolicyRuleId,
        subject: PolicySubject,
        capability: Capability,
        effect: GrantEffect,
        scope: PolicyScope,
    },
    /// Revokes one capability rule.
    CapabilityRuleRevoked { rule_id: crate::ids::PolicyRuleId },
}

/// Append-only non-empty policy-change record for one transaction revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityPolicyRecord {
    id: SecurityPolicyRecordId,
    recorded_revision: Revision,
    actor: PrincipalId,
    security_epoch_after: SecurityEpoch,
    changes: Vec<SecurityPolicyChange>,
}

impl SecurityPolicyRecord {
    /// Creates a typed policy record and rejects an empty change list.
    pub fn new(
        id: SecurityPolicyRecordId,
        recorded_revision: Revision,
        actor: PrincipalId,
        security_epoch_after: SecurityEpoch,
        changes: Vec<SecurityPolicyChange>,
    ) -> Result<Self, SecurityPolicyRecordError> {
        if changes.is_empty() {
            return Err(SecurityPolicyRecordError::EmptyChanges);
        }
        Ok(Self {
            id,
            recorded_revision,
            actor,
            security_epoch_after,
            changes,
        })
    }

    /// Stable policy-record identity.
    #[must_use]
    pub const fn id(&self) -> SecurityPolicyRecordId {
        self.id
    }
    /// Shared revision that records the complete policy change set.
    #[must_use]
    pub const fn recorded_revision(&self) -> Revision {
        self.recorded_revision
    }
    /// Authenticated principal responsible for these changes.
    #[must_use]
    pub const fn actor(&self) -> PrincipalId {
        self.actor
    }
    /// Epoch established by this policy record.
    #[must_use]
    pub const fn security_epoch_after(&self) -> SecurityEpoch {
        self.security_epoch_after
    }
    /// Non-empty ordered change list.
    #[must_use]
    pub fn changes(&self) -> &[SecurityPolicyChange] {
        &self.changes
    }
}

/// A policy record cannot exist without at least one change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityPolicyRecordError {
    EmptyChanges,
}

impl fmt::Display for SecurityPolicyRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("security policy record requires at least one change")
    }
}

impl std::error::Error for SecurityPolicyRecordError {}

/// Immutable inputs for one authorized policy mutation.
pub struct SecurityPolicyChangeRequest<'a> {
    /// Principal attempting the policy mutation.
    pub actor: PrincipalId,
    /// Complete validated candidate policy snapshot.
    pub next_policy: SecurityPolicySnapshot,
    /// Stable identity for the append-only policy change record.
    pub record_id: SecurityPolicyRecordId,
    /// Complete ordered set of typed changes represented by `next_policy`.
    pub changes: Vec<SecurityPolicyChange>,
    /// Fingerprint of the policy currently authorizing this operation.
    pub current_fingerprint: &'a AuditPolicyFingerprint,
    /// Required safe audit facts bound to the assigned next revision.
    pub audit_record: AuditRecord,
    /// Stable operation identity recorded in the audit commit context.
    pub operation_id: OperationId,
}

impl SecurityPolicyCommitBatch {
    /// Typed append-only change record committed at this revision.
    #[must_use]
    pub const fn policy_record(&self) -> &SecurityPolicyRecord {
        &self.policy_record
    }
    /// Policy projection activated at this shared revision.
    #[must_use]
    pub const fn version(&self) -> &SecurityPolicyVersion {
        &self.version
    }

    /// Required AuditRecord bound to the same revision and operation ID.
    #[must_use]
    pub const fn audit_record(&self) -> &AuditRecord {
        &self.audit_record
    }
}

/// Outcome of one policy commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityPolicyTransactionOutcome {
    /// The new policy version and required audit record share one publication.
    Committed {
        /// Shared transaction revision.
        revision: Revision,
        /// Newly committed authorization epoch.
        epoch: SecurityEpoch,
        /// Idempotency identity associated with the audited operation.
        operation_id: OperationId,
    },
    /// Another write advanced the backend past the policy history base.
    Conflict {
        /// Revision pinned by the policy history.
        expected: Revision,
        /// Latest revision observed by this operation.
        actual: Revision,
    },
}

/// Authorization, validation, audit-binding, or publication failure.
#[derive(Debug)]
pub enum SecurityPolicyTransactionError<E> {
    /// Current policy denied SecurityPolicyManage for the actor.
    Unauthorized,
    /// Supplied audit facts do not bind this actor and policy mutation.
    InvalidAuditRecord,
    /// A policy change transaction must describe at least one typed change.
    EmptyPolicyChangeSet,
    /// The staged policy/audit value differs from the validated candidate.
    BatchMismatch,
    /// Security epoch could not advance.
    EpochExhausted(SecurityEpochError),
    /// Candidate policy history could not be extended.
    History(SecurityPolicyHistoryError),
    /// Caller-supplied post-policy validation rejected the candidate.
    Validation(E),
    /// Open transaction could not be created at the pinned history revision.
    Begin(TransactionBeginError),
    /// Backend rejected the complete policy and audit batch.
    Publish(CancellablePublishError),
}

/// Atomically commits a policy version and its required audit record.
///
/// The backend stores `SecurityPolicyCommitBatch` as one immutable value at one
/// revision. A durable implementation must map that publication to one atomic
/// durability unit. The in-memory policy history advances only after publish.
pub fn commit_security_policy_change<B, E>(
    backend: &mut B,
    history: &mut SecurityPolicyHistory,
    request: SecurityPolicyChangeRequest<'_>,
    validate_policy: impl FnOnce(
        &SecurityPolicySnapshot,
        &SecurityPolicySnapshot,
        &SecurityPolicyRecord,
    ) -> Result<(), E>,
) -> Result<SecurityPolicyTransactionOutcome, SecurityPolicyTransactionError<E>>
where
    B: RevisionBackend<SecurityPolicyCommitBatch>,
{
    let SecurityPolicyChangeRequest {
        actor,
        next_policy,
        record_id,
        changes,
        current_fingerprint,
        audit_record,
        operation_id,
    } = request;
    let base_revision = history.committed_revision();
    let backend_head = backend.latest_published();
    if backend_head != base_revision {
        return Ok(SecurityPolicyTransactionOutcome::Conflict {
            expected: base_revision,
            actual: backend_head,
        });
    }
    let current = history
        .latest_version()
        .map_err(SecurityPolicyTransactionError::History)?;
    let current_policy = current.snapshot();
    if current_policy.authorize(
        actor,
        Capability::SecurityPolicyManage,
        PolicyTarget::default(),
    ) != AuthorizationDecision::Allow
    {
        return Err(SecurityPolicyTransactionError::Unauthorized);
    }
    let next_epoch = current
        .epoch()
        .next()
        .map_err(SecurityPolicyTransactionError::EpochExhausted)?;
    let next_revision = base_revision.next_commit().map_err(|error| {
        SecurityPolicyTransactionError::History(SecurityPolicyHistoryError::RevisionExhausted(
            error,
        ))
    })?;
    let policy_record =
        SecurityPolicyRecord::new(record_id, next_revision, actor, next_epoch, changes)
            .map_err(|_| SecurityPolicyTransactionError::EmptyPolicyChangeSet)?;
    validate_policy(current_policy, &next_policy, &policy_record)
        .map_err(SecurityPolicyTransactionError::Validation)?;

    let expected_commit_context = AuditCommitContext::Committed {
        revision: next_revision,
        operation_id,
    };
    if audit_record.actor() != actor
        || audit_record.action() != AuditAction::SecurityPolicyChange
        || audit_record.object_class() != AuditObjectClass::SecurityPolicy
        || audit_record.outcome() != AuditOutcome::Succeeded
        || audit_record.commit_context() != expected_commit_context
        || audit_record.security_epoch() != current.epoch()
        || audit_record.policy_fingerprint() != current_fingerprint
    {
        return Err(SecurityPolicyTransactionError::InvalidAuditRecord);
    }

    let version = SecurityPolicyVersion::new(next_revision, next_epoch, next_policy);
    let candidate_history = history
        .append_policy_change(version.clone())
        .map_err(SecurityPolicyTransactionError::History)?;
    let batch = SecurityPolicyCommitBatch {
        policy_record,
        version,
        audit_record,
    };
    let mut transaction = OpenTransaction::begin(backend, base_revision)
        .map_err(SecurityPolicyTransactionError::Begin)?;
    transaction.stage(batch.clone());
    let committed = transaction
        .validate(|_, staged| {
            if staged.len() == 1 && staged.first() == Some(&batch) {
                Ok(())
            } else {
                Err(PolicyBatchMismatch)
            }
        })
        .map_err(|_| SecurityPolicyTransactionError::BatchMismatch)?
        .commit()
        .map_err(SecurityPolicyTransactionError::Publish)?;
    if committed != next_revision {
        return Err(SecurityPolicyTransactionError::Publish(
            CancellablePublishError::Publish(
                crate::revision_history::RevisionLogError::RevisionGap {
                    expected: next_revision,
                    actual: committed,
                },
            ),
        ));
    }
    *history = candidate_history;
    Ok(SecurityPolicyTransactionOutcome::Committed {
        revision: committed,
        epoch: next_epoch,
        operation_id,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PolicyBatchMismatch;

impl fmt::Display for PolicyBatchMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("staged security-policy batch changed during validation")
    }
}

impl<E: fmt::Display> fmt::Display for SecurityPolicyTransactionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => formatter.write_str("SecurityPolicyManage is required"),
            Self::InvalidAuditRecord => {
                formatter.write_str("required audit record does not match policy commit")
            }
            Self::EmptyPolicyChangeSet => {
                formatter.write_str("security policy transaction requires at least one change")
            }
            Self::BatchMismatch => {
                formatter.write_str("staged security-policy batch changed during validation")
            }
            Self::EpochExhausted(error) => write!(formatter, "security epoch exhausted: {error}"),
            Self::History(error) => write!(formatter, "security policy history failed: {error}"),
            Self::Validation(error) => write!(formatter, "policy validation failed: {error}"),
            Self::Begin(error) => write!(formatter, "policy transaction could not begin: {error}"),
            Self::Publish(error) => write!(formatter, "policy transaction publish failed: {error}"),
        }
    }
}

impl<E> std::error::Error for SecurityPolicyTransactionError<E> where E: std::error::Error + 'static {}

#[cfg(test)]
mod tests {
    use super::{
        SecurityPolicyChange, SecurityPolicyChangeRequest, SecurityPolicyCommitBatch,
        SecurityPolicyTransactionOutcome, commit_security_policy_change,
    };
    use crate::audit::{
        AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
        AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence,
    };
    use crate::commit_cancellation::CommitCancellation;
    use crate::ids::{
        AuditOperationId, AuditRecordId, DomainId, OperationId, PolicyRuleId, PrincipalId,
        Revision, SecurityEpoch, SecurityPolicyRecordId,
    };
    use crate::revision_backend::{
        CancellablePublishError, InMemoryRevisionBackend, RevisionBackend,
    };
    use crate::revision_history::RevisionLogError;
    use crate::security::{
        Capability, CapabilityGrant, CapabilityRule, GrantEffect, PolicyScope, PolicySubject,
        Principal, SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion,
    };

    fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn fingerprint(value: u8) -> Result<AuditPolicyFingerprint, String> {
        AuditPolicyFingerprint::new(crate::Bytes::new(vec![value]))
            .map_err(|error| error.to_string())
    }

    fn policy(actor: PrincipalId, allow_manage: bool) -> Result<SecurityPolicySnapshot, String> {
        let rules = if allow_manage {
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(3)?,
                PolicySubject::Principal(actor),
                CapabilityGrant::new(Capability::SecurityPolicyManage, GrantEffect::Allow),
                PolicyScope::project(),
            )]
        } else {
            vec![]
        };
        SecurityPolicySnapshot::new(vec![Principal::new(actor)], vec![], vec![], rules)
            .map_err(|error| error.to_string())
    }

    fn initial_history(
        actor: PrincipalId,
        allow_manage: bool,
    ) -> Result<SecurityPolicyHistory, String> {
        SecurityPolicyHistory::new(
            Revision::GENESIS,
            vec![SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                policy(actor, allow_manage)?,
            )],
        )
        .map_err(|error| error.to_string())
    }

    fn audit(
        actor: PrincipalId,
        epoch: SecurityEpoch,
        fingerprint: AuditPolicyFingerprint,
        revision: Revision,
        operation_id: OperationId,
        tail: u8,
    ) -> Result<AuditRecord, String> {
        Ok(AuditRecord::new(
            AuditRecordIdentity {
                record_id: id::<AuditRecordId>(tail)?,
                sequence: AuditSequence::new(u64::from(tail)),
                audit_operation_id: id::<AuditOperationId>(tail.wrapping_add(1))?,
            },
            AuditRecordDetails {
                actor,
                action: AuditAction::SecurityPolicyChange,
                object_class: AuditObjectClass::SecurityPolicy,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision,
                    operation_id,
                },
                security_epoch: epoch,
                policy_fingerprint: fingerprint,
            },
        ))
    }

    #[test]
    fn policy_version_epoch_and_required_audit_publish_together() -> Result<(), String> {
        let actor = id::<PrincipalId>(2)?;
        let operation_id = id::<OperationId>(6)?;
        let old_fingerprint = fingerprint(7)?;
        let mut history = initial_history(actor, true)?;
        let mut backend = InMemoryRevisionBackend::<SecurityPolicyCommitBatch>::new();
        let outcome = commit_security_policy_change(
            &mut backend,
            &mut history,
            SecurityPolicyChangeRequest {
                actor,
                next_policy: policy(actor, false)?,
                record_id: id::<SecurityPolicyRecordId>(23)?,
                changes: vec![SecurityPolicyChange::CapabilityRuleRevoked {
                    rule_id: id::<PolicyRuleId>(3)?,
                }],
                current_fingerprint: &old_fingerprint,
                audit_record: audit(
                    actor,
                    SecurityEpoch::INITIAL,
                    old_fingerprint.clone(),
                    Revision::FIRST_COMMIT,
                    operation_id,
                    8,
                )?,
                operation_id,
            },
            |_, _, record| {
                assert_eq!(record.recorded_revision(), Revision::FIRST_COMMIT);
                assert_eq!(record.security_epoch_after(), SecurityEpoch::new(1));
                assert_eq!(record.actor(), actor);
                Ok::<_, String>(())
            },
        )
        .map_err(|error| error.to_string())?;

        assert_eq!(
            outcome,
            SecurityPolicyTransactionOutcome::Committed {
                revision: Revision::FIRST_COMMIT,
                epoch: SecurityEpoch::new(1),
                operation_id,
            }
        );
        assert_eq!(history.committed_revision(), Revision::FIRST_COMMIT);
        assert_eq!(
            history.latest_version().map_err(|e| e.to_string())?.epoch(),
            SecurityEpoch::new(1)
        );
        assert_eq!(backend.latest_published(), Revision::FIRST_COMMIT);
        let rows = backend
            .read_at(Revision::FIRST_COMMIT)
            .map_err(|e| e.to_string())?
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        let (revision, batch) = rows.first().ok_or("policy commit batch missing")?;
        assert_eq!(*revision, Revision::FIRST_COMMIT);
        assert_eq!(batch.version().epoch(), SecurityEpoch::new(1));
        assert_eq!(batch.policy_record().recorded_revision(), *revision);
        assert_eq!(batch.policy_record().actor(), actor);
        assert_eq!(
            batch.policy_record().security_epoch_after(),
            SecurityEpoch::new(1)
        );
        assert_eq!(
            batch.policy_record().changes(),
            &[SecurityPolicyChange::CapabilityRuleRevoked {
                rule_id: id::<PolicyRuleId>(3)?,
            }]
        );
        assert_eq!(
            batch.audit_record().commit_context(),
            AuditCommitContext::Committed {
                revision: *revision,
                operation_id
            }
        );
        Ok(())
    }

    #[test]
    fn denied_invalid_and_rejected_policy_changes_have_no_partial_effect() -> Result<(), String> {
        let actor = id::<PrincipalId>(12)?;
        let operation_id = id::<OperationId>(13)?;
        let old_fingerprint = fingerprint(14)?;
        let mut history = initial_history(actor, false)?;
        let mut backend = InMemoryRevisionBackend::<SecurityPolicyCommitBatch>::new();
        let denied = commit_security_policy_change(
            &mut backend,
            &mut history,
            SecurityPolicyChangeRequest {
                actor,
                next_policy: policy(actor, true)?,
                record_id: id::<SecurityPolicyRecordId>(24)?,
                changes: vec![SecurityPolicyChange::CapabilityRuleAdded {
                    rule_id: id::<PolicyRuleId>(3)?,
                    subject: PolicySubject::Principal(actor),
                    capability: Capability::SecurityPolicyManage,
                    effect: GrantEffect::Allow,
                    scope: PolicyScope::project(),
                }],
                current_fingerprint: &old_fingerprint,
                audit_record: audit(
                    actor,
                    SecurityEpoch::INITIAL,
                    old_fingerprint.clone(),
                    Revision::FIRST_COMMIT,
                    operation_id,
                    15,
                )?,
                operation_id,
            },
            |_, _, _| Ok::<_, String>(()),
        );
        assert!(matches!(
            denied,
            Err(crate::SecurityPolicyTransactionError::Unauthorized)
        ));
        assert_eq!(history.committed_revision(), Revision::GENESIS);
        assert_eq!(backend.latest_published(), Revision::GENESIS);

        history = initial_history(actor, true)?;
        let invalid = commit_security_policy_change(
            &mut backend,
            &mut history,
            SecurityPolicyChangeRequest {
                actor,
                next_policy: policy(actor, false)?,
                record_id: id::<SecurityPolicyRecordId>(25)?,
                changes: vec![SecurityPolicyChange::CapabilityRuleRevoked {
                    rule_id: id::<PolicyRuleId>(3)?,
                }],
                current_fingerprint: &old_fingerprint,
                audit_record: audit(
                    actor,
                    SecurityEpoch::INITIAL,
                    old_fingerprint.clone(),
                    Revision::FIRST_COMMIT,
                    id::<OperationId>(16)?,
                    17,
                )?,
                operation_id,
            },
            |_, _, _| Ok::<_, String>(()),
        );
        assert!(matches!(
            invalid,
            Err(crate::SecurityPolicyTransactionError::InvalidAuditRecord)
        ));
        let rejected = commit_security_policy_change(
            &mut backend,
            &mut history,
            SecurityPolicyChangeRequest {
                actor,
                next_policy: policy(actor, false)?,
                record_id: id::<SecurityPolicyRecordId>(26)?,
                changes: vec![SecurityPolicyChange::CapabilityRuleRevoked {
                    rule_id: id::<PolicyRuleId>(3)?,
                }],
                current_fingerprint: &old_fingerprint,
                audit_record: audit(
                    actor,
                    SecurityEpoch::INITIAL,
                    old_fingerprint.clone(),
                    Revision::FIRST_COMMIT,
                    operation_id,
                    18,
                )?,
                operation_id,
            },
            |_, _, _| Err::<(), _>("candidate rejected"),
        );
        assert!(matches!(
            rejected,
            Err(crate::SecurityPolicyTransactionError::Validation(_))
        ));
        assert_eq!(history.committed_revision(), Revision::GENESIS);
        assert_eq!(
            history.latest_version().map_err(|e| e.to_string())?.epoch(),
            SecurityEpoch::INITIAL
        );
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        assert!(backend.commits().is_empty());

        let empty_changes = commit_security_policy_change(
            &mut backend,
            &mut history,
            SecurityPolicyChangeRequest {
                actor,
                next_policy: policy(actor, false)?,
                record_id: id::<SecurityPolicyRecordId>(28)?,
                changes: vec![],
                current_fingerprint: &old_fingerprint,
                audit_record: audit(
                    actor,
                    SecurityEpoch::INITIAL,
                    old_fingerprint.clone(),
                    Revision::FIRST_COMMIT,
                    operation_id,
                    29,
                )?,
                operation_id,
            },
            |_, _, _| Ok::<_, String>(()),
        );
        assert!(matches!(
            empty_changes,
            Err(crate::SecurityPolicyTransactionError::EmptyPolicyChangeSet)
        ));
        assert_eq!(history.committed_revision(), Revision::GENESIS);
        assert_eq!(backend.latest_published(), Revision::GENESIS);
        Ok(())
    }

    #[derive(Debug)]
    struct FailingBackend;

    impl RevisionBackend<SecurityPolicyCommitBatch> for FailingBackend {
        type Read<'a> = std::iter::Empty<(Revision, &'a SecurityPolicyCommitBatch)>;

        fn latest_published(&self) -> Revision {
            Revision::GENESIS
        }
        fn publish(
            &mut self,
            _: Vec<SecurityPolicyCommitBatch>,
        ) -> Result<Revision, RevisionLogError> {
            Err(RevisionLogError::NoReservation)
        }
        fn publish_cancellable(
            &mut self,
            _: Vec<SecurityPolicyCommitBatch>,
            cancellation: &CommitCancellation,
        ) -> Result<Revision, CancellablePublishError> {
            let permit = cancellation
                .begin_commitpoint()
                .map_err(CancellablePublishError::Commitpoint)?;
            permit.not_committed();
            Err(CancellablePublishError::Publish(
                RevisionLogError::NoReservation,
            ))
        }
        fn read_at(&self, _: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
            Ok(std::iter::empty())
        }
    }

    #[test]
    fn backend_failure_does_not_advance_policy_history() -> Result<(), String> {
        let actor = id::<PrincipalId>(19)?;
        let operation_id = id::<OperationId>(20)?;
        let old_fingerprint = fingerprint(21)?;
        let mut history = initial_history(actor, true)?;
        let result = commit_security_policy_change(
            &mut FailingBackend,
            &mut history,
            SecurityPolicyChangeRequest {
                actor,
                next_policy: policy(actor, false)?,
                record_id: id::<SecurityPolicyRecordId>(27)?,
                changes: vec![SecurityPolicyChange::CapabilityRuleRevoked {
                    rule_id: id::<PolicyRuleId>(3)?,
                }],
                current_fingerprint: &old_fingerprint,
                audit_record: audit(
                    actor,
                    SecurityEpoch::INITIAL,
                    old_fingerprint.clone(),
                    Revision::FIRST_COMMIT,
                    operation_id,
                    22,
                )?,
                operation_id,
            },
            |_, _, _| Ok::<_, String>(()),
        );
        assert!(matches!(
            result,
            Err(crate::SecurityPolicyTransactionError::Publish(_))
        ));
        assert_eq!(history.committed_revision(), Revision::GENESIS);
        assert_eq!(
            history.latest_version().map_err(|e| e.to_string())?.epoch(),
            SecurityEpoch::INITIAL
        );
        Ok(())
    }
}
