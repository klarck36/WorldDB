//! Typed audit values kept separate from data revisions and transaction IDs.

use std::fmt;

use crate::ids::{
    AuditOperationId, AuditRecordId, ClientRequestId, OperationId, PrincipalId, Revision,
    SecurityEpoch, SnapshotId,
};
use crate::values::Bytes;

/// Monotonic sequence value owned by the separate audit log.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AuditSequence(u64);

impl AuditSequence {
    /// Constructs a sequence value without reserving a numeric sentinel.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the exact sequence number.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Advances once or reports that the sequence space is exhausted.
    pub const fn next(self) -> Result<Self, AuditSequenceError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(AuditSequenceError::Exhausted),
        }
    }
}

impl fmt::Display for AuditSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Failure to allocate the next audit sequence number.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditSequenceError {
    /// The unsigned sequence space is exhausted.
    Exhausted,
}

impl fmt::Display for AuditSequenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted => formatter.write_str("audit sequence is exhausted"),
        }
    }
}

impl std::error::Error for AuditSequenceError {}

/// Zero-based or one-based page ordinal supplied by the owning pagination contract.
///
/// The core imposes no sentinel or indexing convention; it preserves the exact
/// ordinal chosen by that protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageOrdinal(u64);

impl PageOrdinal {
    /// Constructs an ordinal without reserving a numeric sentinel.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the exact ordinal.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Opaque fingerprint of the authorized raw-read scope.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AuditScopeFingerprint(Bytes);

impl AuditScopeFingerprint {
    /// Stores a non-empty opaque fingerprint supplied by the security boundary.
    pub fn new(bytes: Bytes) -> Result<Self, AuditFingerprintError> {
        if bytes.is_empty() {
            return Err(AuditFingerprintError::Empty);
        }
        Ok(Self(bytes))
    }

    /// Returns the exact opaque fingerprint bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// Opaque fingerprint of the policy snapshot used for an audit action.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AuditPolicyFingerprint(Bytes);

impl AuditPolicyFingerprint {
    /// Stores a non-empty opaque fingerprint supplied by the policy subsystem.
    pub fn new(bytes: Bytes) -> Result<Self, AuditFingerprintError> {
        if bytes.is_empty() {
            return Err(AuditFingerprintError::Empty);
        }
        Ok(Self(bytes))
    }

    /// Returns the exact opaque fingerprint bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// Invalid opaque audit fingerprint data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditFingerprintError {
    /// A fingerprint must contain at least one byte.
    Empty,
}

impl fmt::Display for AuditFingerprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("audit fingerprint cannot be empty"),
        }
    }
}

impl std::error::Error for AuditFingerprintError {}

/// Closed audit action catalog for the operations named by the 1.0 contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditAction {
    /// A security-policy mutation or required record.
    SecurityPolicyChange,
    /// A page of raw data requested through an administrative path.
    RawReadAttempt,
    /// Authorization to create or export protected data.
    ExportAuthorization,
    /// Completion of an export operation.
    ExportCompletion,
    /// A migration plan or execution action.
    Migration,
    /// A backup creation or verification action.
    Backup,
    /// Publication of a restored database state.
    RestorePublication,
    /// Publication of a data purge.
    PurgePublication,
    /// An audit configuration change.
    AuditConfigurationChange,
    /// Creation or lifecycle update of a project schema definition.
    SchemaManagement,
}

/// Closed object classes that can be named without copying sensitive payloads.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditObjectClass {
    /// The database or project as a whole.
    Database,
    /// A security principal or authorization definition.
    SecurityPolicy,
    /// A migration plan, step, or run.
    Migration,
    /// A background job.
    Job,
    /// A backup artifact.
    Backup,
    /// An export artifact.
    Export,
    /// A raw-read scope rather than the read payload.
    RawReadScope,
    /// Audit retention or access configuration.
    AuditConfiguration,
    /// EntityType, Predicate, or EventKind schema definition.
    SchemaDefinition,
}

/// Safe public result class for an audited operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditOutcome {
    /// The audited operation succeeded.
    Succeeded,
    /// Authorization denied the operation.
    Denied,
    /// The operation failed before successful completion.
    Failed,
    /// The operation result is not known to the caller.
    Indeterminate,
}

/// Whether an audit record accompanies a committed data operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditCommitContext {
    /// The audited mutation committed at this revision and operation identity.
    Committed {
        /// Shared WorldDB data/schema revision.
        revision: Revision,
        /// Stable idempotency identity of the committed logical operation.
        operation_id: OperationId,
    },
    /// No WorldDB data commit is associated with this audit record.
    NotCommitted,
}

/// Independent IDs and sequence assigned to one audit record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AuditRecordIdentity {
    /// Stable identity of this audit record.
    pub record_id: AuditRecordId,
    /// Monotonic sequence in the separate audit log.
    pub sequence: AuditSequence,
    /// Identity of this audit operation.
    pub audit_operation_id: AuditOperationId,
}

/// Safe facts recorded about an audited operation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AuditRecordDetails {
    /// Principal that performed or requested the operation.
    pub actor: PrincipalId,
    /// Closed action classification.
    pub action: AuditAction,
    /// Closed affected-object classification.
    pub object_class: AuditObjectClass,
    /// Safe result classification.
    pub outcome: AuditOutcome,
    /// Explicit commit association, with absence represented as a domain value.
    pub commit_context: AuditCommitContext,
    /// Security-policy epoch used for the action.
    pub security_epoch: SecurityEpoch,
    /// Opaque policy fingerprint, without policy secrets.
    pub policy_fingerprint: AuditPolicyFingerprint,
}

/// A safe, typed audit record for a committed or policy-selected rejected action.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AuditRecord {
    identity: AuditRecordIdentity,
    details: AuditRecordDetails,
}

impl AuditRecord {
    /// Creates a record from its independent identity and safe operation details.
    #[must_use]
    pub const fn new(identity: AuditRecordIdentity, details: AuditRecordDetails) -> Self {
        Self { identity, details }
    }

    /// Returns the audit record identity.
    #[must_use]
    pub const fn record_id(&self) -> AuditRecordId {
        self.identity.record_id
    }

    /// Returns its independent monotonic audit sequence.
    #[must_use]
    pub const fn sequence(&self) -> AuditSequence {
        self.identity.sequence
    }

    /// Returns the identity of this audit operation.
    #[must_use]
    pub const fn audit_operation_id(&self) -> AuditOperationId {
        self.identity.audit_operation_id
    }

    /// Returns the safe actor identity.
    #[must_use]
    pub const fn actor(&self) -> PrincipalId {
        self.details.actor
    }

    /// Returns the audited action.
    #[must_use]
    pub const fn action(&self) -> AuditAction {
        self.details.action
    }

    /// Returns the audited object class.
    #[must_use]
    pub const fn object_class(&self) -> AuditObjectClass {
        self.details.object_class
    }

    /// Returns the safe result class.
    #[must_use]
    pub const fn outcome(&self) -> AuditOutcome {
        self.details.outcome
    }

    /// Returns the explicit committed or not-committed context.
    #[must_use]
    pub const fn commit_context(&self) -> AuditCommitContext {
        self.details.commit_context
    }

    /// Returns the authorization epoch used for the action.
    #[must_use]
    pub const fn security_epoch(&self) -> SecurityEpoch {
        self.details.security_epoch
    }

    /// Returns the policy fingerprint without exposing policy secrets.
    #[must_use]
    pub const fn policy_fingerprint(&self) -> &AuditPolicyFingerprint {
        &self.details.policy_fingerprint
    }
}

/// IDs retained or refreshed for one raw-read attempt.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RawReadAttemptIdentity {
    /// Record identity allocated for this attempt.
    pub record_id: AuditRecordId,
    /// Fresh sequence for this attempt.
    pub sequence: AuditSequence,
    /// Fresh audit-operation identity for this attempt.
    pub audit_operation_id: AuditOperationId,
    /// Stable client request identity shared by retries.
    pub client_request_id: ClientRequestId,
}

/// Security and pagination context bound to one raw-read page attempt.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RawReadAttemptScope {
    /// Secure principal identity.
    pub principal_id: PrincipalId,
    /// Fingerprint of the authorized scope, not the raw payload.
    pub scope_fingerprint: AuditScopeFingerprint,
    /// Snapshot pinned for the page.
    pub snapshot_id: SnapshotId,
    /// Current security-policy epoch.
    pub security_epoch: SecurityEpoch,
    /// Page ordinal supplied by the pagination contract.
    pub page_ordinal: PageOrdinal,
}

/// Safe metadata durably recorded before each administrative raw-read page is emitted.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RawReadAttempt {
    identity: RawReadAttemptIdentity,
    scope: RawReadAttemptScope,
}

impl RawReadAttempt {
    /// Creates an attempt from its independent retry identity and bound scope.
    #[must_use]
    pub const fn new(identity: RawReadAttemptIdentity, scope: RawReadAttemptScope) -> Self {
        Self { identity, scope }
    }

    /// Returns the audit record identity.
    #[must_use]
    pub const fn record_id(&self) -> AuditRecordId {
        self.identity.record_id
    }

    /// Returns the append-only audit sequence.
    #[must_use]
    pub const fn sequence(&self) -> AuditSequence {
        self.identity.sequence
    }

    /// Returns this attempt's fresh audit-operation identity.
    #[must_use]
    pub const fn audit_operation_id(&self) -> AuditOperationId {
        self.identity.audit_operation_id
    }

    /// Returns the stable client request identity retained across retries.
    #[must_use]
    pub const fn client_request_id(&self) -> ClientRequestId {
        self.identity.client_request_id
    }

    /// Returns the securely bound principal identity.
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.scope.principal_id
    }

    /// Returns the fingerprint of the authorized raw-read scope.
    #[must_use]
    pub const fn scope_fingerprint(&self) -> &AuditScopeFingerprint {
        &self.scope.scope_fingerprint
    }

    /// Returns the pinned snapshot identity.
    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.scope.snapshot_id
    }

    /// Returns the bound security-policy epoch.
    #[must_use]
    pub const fn security_epoch(&self) -> SecurityEpoch {
        self.scope.security_epoch
    }

    /// Returns the page ordinal without imposing a zero- or one-based convention.
    #[must_use]
    pub const fn page_ordinal(&self) -> PageOrdinal {
        self.scope.page_ordinal
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuditAction, AuditCommitContext, AuditFingerprintError, AuditObjectClass, AuditOutcome,
        AuditPolicyFingerprint, AuditRecord, AuditRecordDetails, AuditRecordIdentity,
        AuditScopeFingerprint, AuditSequence, AuditSequenceError, PageOrdinal, RawReadAttempt,
        RawReadAttemptIdentity, RawReadAttemptScope,
    };
    use crate::ids::{
        AuditOperationId, AuditRecordId, ClientRequestId, DomainId, IdValidationError, JobId,
        OperationId, PrincipalId, Revision, RevisionError, SecurityEpoch, SnapshotId,
    };
    use crate::values::Bytes;

    #[derive(Debug)]
    enum TestError {
        Identity,
        Fingerprint,
        Revision,
    }

    impl From<IdValidationError> for TestError {
        fn from(_: IdValidationError) -> Self {
            Self::Identity
        }
    }

    impl From<AuditFingerprintError> for TestError {
        fn from(_: AuditFingerprintError) -> Self {
            Self::Fingerprint
        }
    }

    impl From<RevisionError> for TestError {
        fn from(_: RevisionError) -> Self {
            Self::Revision
        }
    }

    fn uuid<T: DomainId>(tail: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    #[test]
    fn raw_read_attempt_binds_safe_context_and_keeps_retry_ids_distinct() -> Result<(), TestError> {
        let record_id = uuid::<AuditRecordId>(1)?;
        let audit_operation_id = uuid::<AuditOperationId>(2)?;
        let client_request_id = uuid::<ClientRequestId>(3)?;
        let principal_id = uuid::<PrincipalId>(4)?;
        let snapshot_id = uuid::<SnapshotId>(5)?;
        let scope_fingerprint = AuditScopeFingerprint::new(Bytes::new(vec![0x11; 32]))?;
        let attempt = RawReadAttempt::new(
            RawReadAttemptIdentity {
                record_id,
                sequence: AuditSequence::new(7),
                audit_operation_id,
                client_request_id,
            },
            RawReadAttemptScope {
                principal_id,
                scope_fingerprint,
                snapshot_id,
                security_epoch: SecurityEpoch::new(2),
                page_ordinal: PageOrdinal::new(0),
            },
        );
        assert_eq!(attempt.record_id(), record_id);
        assert_eq!(attempt.sequence().value(), 7);
        assert_eq!(attempt.audit_operation_id(), audit_operation_id);
        assert_eq!(attempt.client_request_id(), client_request_id);
        assert_eq!(attempt.principal_id(), principal_id);
        assert_eq!(attempt.scope_fingerprint().as_bytes(), &[0x11; 32]);
        assert_eq!(attempt.snapshot_id(), snapshot_id);
        assert_eq!(attempt.security_epoch(), SecurityEpoch::new(2));
        assert_eq!(attempt.page_ordinal(), PageOrdinal::new(0));

        let retry = RawReadAttempt::new(
            RawReadAttemptIdentity {
                record_id: uuid::<AuditRecordId>(6)?,
                sequence: AuditSequence::new(8),
                audit_operation_id: uuid::<AuditOperationId>(7)?,
                client_request_id,
            },
            RawReadAttemptScope {
                principal_id,
                scope_fingerprint: AuditScopeFingerprint::new(Bytes::new(vec![0x11; 32]))?,
                snapshot_id,
                security_epoch: SecurityEpoch::new(2),
                page_ordinal: PageOrdinal::new(0),
            },
        );
        assert_eq!(retry.client_request_id(), attempt.client_request_id());
        assert_ne!(retry.audit_operation_id(), attempt.audit_operation_id());
        assert_ne!(retry.sequence(), attempt.sequence());
        Ok(())
    }

    #[test]
    fn audit_record_keeps_policy_version_commit_and_audit_ids_separate() -> Result<(), TestError> {
        let record_id = uuid::<AuditRecordId>(11)?;
        let audit_operation_id = uuid::<AuditOperationId>(12)?;
        let operation_id = uuid::<OperationId>(13)?;
        let actor = uuid::<PrincipalId>(14)?;
        let revision = Revision::new(9);
        assert!(revision.is_ok());
        if let Ok(revision) = revision {
            let record = AuditRecord::new(
                AuditRecordIdentity {
                    record_id,
                    sequence: AuditSequence::new(3),
                    audit_operation_id,
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
                    security_epoch: SecurityEpoch::new(4),
                    policy_fingerprint: AuditPolicyFingerprint::new(Bytes::new(vec![0x22; 32]))?,
                },
            );
            assert_eq!(record.record_id(), record_id);
            assert_eq!(record.audit_operation_id(), audit_operation_id);
            assert_eq!(record.actor(), actor);
            assert_eq!(record.action(), AuditAction::SecurityPolicyChange);
            assert_eq!(record.object_class(), AuditObjectClass::SecurityPolicy);
            assert_eq!(record.outcome(), AuditOutcome::Succeeded);
            assert_eq!(
                record.commit_context(),
                AuditCommitContext::Committed {
                    revision,
                    operation_id,
                }
            );
            assert_eq!(record.security_epoch(), SecurityEpoch::new(4));
            assert_eq!(record.policy_fingerprint().as_bytes(), &[0x22; 32]);
        }
        Ok(())
    }

    #[test]
    fn audit_sequence_is_not_a_data_revision_and_does_not_wrap() {
        assert_eq!(AuditSequence::new(0).next(), Ok(AuditSequence::new(1)));
        assert_eq!(
            AuditSequence::new(u64::MAX).next(),
            Err(AuditSequenceError::Exhausted)
        );
        let no_commit = AuditCommitContext::NotCommitted;
        assert_eq!(no_commit, AuditCommitContext::NotCommitted);
    }

    #[test]
    fn client_request_and_audit_operation_ids_have_different_scopes()
    -> Result<(), IdValidationError> {
        let request = uuid::<ClientRequestId>(21)?;
        let first_attempt = uuid::<AuditOperationId>(22)?;
        let retry_attempt = uuid::<AuditOperationId>(23)?;
        assert_ne!(first_attempt, retry_attempt);
        assert_ne!(request.to_bytes(), first_attempt.to_bytes());
        let distinct_job = uuid::<JobId>(24)?;
        assert_ne!(request.to_bytes(), distinct_job.to_bytes());
        Ok(())
    }

    #[test]
    fn fingerprints_reject_empty_values_without_fixing_a_wire_algorithm() {
        assert_eq!(
            AuditScopeFingerprint::new(Bytes::default()),
            Err(AuditFingerprintError::Empty)
        );
        assert_eq!(
            AuditPolicyFingerprint::new(Bytes::default()),
            Err(AuditFingerprintError::Empty)
        );
        let variable_length = AuditScopeFingerprint::new(Bytes::new(vec![1, 2, 3]));
        assert!(variable_length.is_ok());
    }
}
