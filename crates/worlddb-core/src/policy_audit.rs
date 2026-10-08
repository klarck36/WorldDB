//! Required-audit gate for in-memory security-policy actions.
//!
//! Durable backends must bind policy publication and audit append to one commitpoint. This
//! reference path proves the precondition ordering and no-policy-side-effect failure case.

use std::fmt;

use crate::audit::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord,
};
use crate::ids::{SecurityEpoch, SecurityEpochError};
use crate::security::{AuthorizationDecision, Capability, PolicyTarget, SecurityPolicySnapshot};

/// Capacity ceiling for the in-memory required-audit reference port.
pub const MAX_IN_MEMORY_AUDIT_RECORDS: usize = 4096;

/// Independent audit-retention configuration. It does not share telemetry queue limits.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AuditRetentionPolicy {
    minimum_retention_millis: u64,
    maximum_records: usize,
}

impl AuditRetentionPolicy {
    /// Defines the minimum retention duration and maximum records for one audit store.
    pub fn new(
        minimum_retention_millis: u64,
        maximum_records: usize,
    ) -> Result<Self, AuditRetentionError> {
        if minimum_retention_millis == 0 || maximum_records == 0 {
            return Err(AuditRetentionError::ZeroBound);
        }
        if maximum_records > MAX_IN_MEMORY_AUDIT_RECORDS {
            return Err(AuditRetentionError::CapacityExceeded);
        }
        Ok(Self {
            minimum_retention_millis,
            maximum_records,
        })
    }

    /// Minimum age before retention policy may consider a record for expiry.
    #[must_use]
    pub const fn minimum_retention_millis(self) -> u64 {
        self.minimum_retention_millis
    }

    /// Maximum retained-record count configured for the audit store.
    #[must_use]
    pub const fn maximum_records(self) -> usize {
        self.maximum_records
    }
}

/// Audit retention configuration rejected an unbounded zero value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuditRetentionError {
    ZeroBound,
    CapacityExceeded,
}

impl fmt::Display for AuditRetentionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroBound => "audit retention duration and record limit must be positive",
            Self::CapacityExceeded => "audit retention record limit exceeds the Core maximum",
        })
    }
}

impl std::error::Error for AuditRetentionError {}

/// Independent authorization decisions for audit read, export, and configuration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AuditAccessPermissions {
    read: bool,
    export: bool,
    configure: bool,
}

impl AuditAccessPermissions {
    /// Evaluates audit privileges separately; no one privilege implies another.
    #[must_use]
    pub fn evaluate(
        policy: &SecurityPolicySnapshot,
        actor: crate::PrincipalId,
        target: PolicyTarget,
    ) -> Self {
        Self {
            read: policy.authorize(actor, Capability::AuditRead, target)
                == AuthorizationDecision::Allow,
            export: policy.authorize(actor, Capability::AuditExport, target)
                == AuthorizationDecision::Allow,
            configure: policy.authorize(actor, Capability::AuditConfigure, target)
                == AuthorizationDecision::Allow,
        }
    }

    /// May read audit records.
    #[must_use]
    pub const fn may_read(self) -> bool {
        self.read
    }

    /// May export audit records.
    #[must_use]
    pub const fn may_export(self) -> bool {
        self.export
    }

    /// May configure audit retention/access.
    #[must_use]
    pub const fn may_configure(self) -> bool {
        self.configure
    }
}

/// A required audit port. On error it must guarantee that it appended no record.
pub trait RequiredAuditPort {
    /// Atomically accepts one complete required record or fails without an append side effect.
    fn append_required(&mut self, record: AuditRecord) -> Result<(), RequiredAuditError>;
}

/// Small bounded audit implementation used by reference-path tests.
#[derive(Debug)]
pub struct InMemoryRequiredAuditPort {
    capacity: usize,
    retention: Option<AuditRetentionPolicy>,
    records: Vec<AuditRecord>,
    fail_next: bool,
}

impl InMemoryRequiredAuditPort {
    /// Creates the test port with a positive, hard-bounded record capacity.
    pub fn new(capacity: usize) -> Result<Self, RequiredAuditError> {
        if capacity == 0 {
            return Err(RequiredAuditError::ZeroCapacity);
        }
        if capacity > MAX_IN_MEMORY_AUDIT_RECORDS {
            return Err(RequiredAuditError::CapacityExceeded);
        }
        Ok(Self {
            capacity,
            retention: None,
            records: Vec::with_capacity(capacity),
            fail_next: false,
        })
    }

    /// Creates the in-memory port from a separately configured audit retention policy.
    pub fn with_retention(retention: AuditRetentionPolicy) -> Result<Self, RequiredAuditError> {
        let mut port = Self::new(retention.maximum_records())?;
        port.retention = Some(retention);
        Ok(port)
    }

    /// Causes the next required append to fail before changing audit state.
    pub fn fail_next_append(&mut self) {
        self.fail_next = true;
    }

    /// Accepted records in append order.
    #[must_use]
    pub fn records(&self) -> &[AuditRecord] {
        &self.records
    }

    /// Retention policy configured for this audit store, independent of telemetry settings.
    #[must_use]
    pub const fn retention_policy(&self) -> Option<AuditRetentionPolicy> {
        self.retention
    }
}

impl RequiredAuditPort for InMemoryRequiredAuditPort {
    fn append_required(&mut self, record: AuditRecord) -> Result<(), RequiredAuditError> {
        if self.fail_next {
            self.fail_next = false;
            return Err(RequiredAuditError::Unavailable);
        }
        if self.records.len() >= self.capacity {
            return Err(RequiredAuditError::CapacityExceeded);
        }
        self.records.push(record);
        Ok(())
    }
}

/// Generic in-memory policy state; production security stores remain the authority.
#[derive(Debug)]
pub struct InMemoryPolicyState {
    policy: SecurityPolicySnapshot,
    epoch: SecurityEpoch,
    fingerprint: AuditPolicyFingerprint,
}

impl InMemoryPolicyState {
    /// Creates policy state from its current revision identity and audit fingerprint.
    #[must_use]
    pub const fn new(
        policy: SecurityPolicySnapshot,
        epoch: SecurityEpoch,
        fingerprint: AuditPolicyFingerprint,
    ) -> Self {
        Self {
            policy,
            epoch,
            fingerprint,
        }
    }

    /// Current in-memory policy value.
    #[must_use]
    pub const fn policy(&self) -> &SecurityPolicySnapshot {
        &self.policy
    }

    /// Current security epoch.
    #[must_use]
    pub const fn epoch(&self) -> SecurityEpoch {
        self.epoch
    }

    /// Current opaque policy fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &AuditPolicyFingerprint {
        &self.fingerprint
    }
}

/// Applies a pre-authorized policy replacement only after its Required Audit record is accepted.
///
/// The caller is responsible for authenticating the actor and authorizing SecurityPolicyManage.
/// The next policy value is moved into place only after all checks and the fail-closed audit
/// append succeed; publication after that append is infallible in this in-memory reference path.
pub fn apply_required_policy_change(
    state: &mut InMemoryPolicyState,
    next_policy: SecurityPolicySnapshot,
    next_fingerprint: AuditPolicyFingerprint,
    record: AuditRecord,
    audit: &mut impl RequiredAuditPort,
) -> Result<SecurityEpoch, PolicyAuditError> {
    let next_epoch = state
        .epoch
        .next()
        .map_err(PolicyAuditError::EpochExhausted)?;
    if record.action() != AuditAction::SecurityPolicyChange
        || record.object_class() != AuditObjectClass::SecurityPolicy
        || record.outcome() != AuditOutcome::Succeeded
        || record.commit_context() != AuditCommitContext::NotCommitted
        || record.security_epoch() != state.epoch
        || record.policy_fingerprint() != &state.fingerprint
    {
        return Err(PolicyAuditError::InvalidAuditRecord);
    }
    if state.policy.authorize(
        record.actor(),
        Capability::SecurityPolicyManage,
        PolicyTarget::default(),
    ) != AuthorizationDecision::Allow
    {
        return Err(PolicyAuditError::Unauthorized);
    }

    audit
        .append_required(record)
        .map_err(PolicyAuditError::RequiredAuditUnavailable)?;

    state.policy = next_policy;
    state.epoch = next_epoch;
    state.fingerprint = next_fingerprint;
    Ok(next_epoch)
}

/// Required audit resource/configuration failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RequiredAuditError {
    ZeroCapacity,
    CapacityExceeded,
    Unavailable,
}

impl fmt::Display for RequiredAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroCapacity => "required audit capacity must be positive",
            Self::CapacityExceeded => "required audit capacity is exhausted or exceeds its limit",
            Self::Unavailable => "required audit port is unavailable",
        })
    }
}

impl std::error::Error for RequiredAuditError {}

/// Policy action failed before or at the required audit gate.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PolicyAuditError {
    InvalidAuditRecord,
    Unauthorized,
    RequiredAuditUnavailable(RequiredAuditError),
    EpochExhausted(SecurityEpochError),
}

impl fmt::Display for PolicyAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidAuditRecord => "required policy audit record does not match the action",
            Self::Unauthorized => "security policy management is not authorized",
            Self::RequiredAuditUnavailable(_) => "required policy audit is unavailable",
            Self::EpochExhausted(_) => "security policy epoch is exhausted",
        })
    }
}

impl std::error::Error for PolicyAuditError {}

impl From<SecurityEpochError> for PolicyAuditError {
    fn from(value: SecurityEpochError) -> Self {
        Self::EpochExhausted(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuditAccessPermissions, AuditRetentionError, AuditRetentionPolicy, InMemoryPolicyState,
        InMemoryRequiredAuditPort, PolicyAuditError, RequiredAuditError,
        apply_required_policy_change,
    };
    use crate::audit::{
        AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
        AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence,
    };
    use crate::diagnostics::{
        BoundedDiagnostics, DiagnosticCounter, DiagnosticEvent, DiagnosticEventKind,
        DiagnosticField, DiagnosticFieldKey, DiagnosticPort, DiagnosticSpanName,
    };
    use crate::ids::{
        AuditOperationId, AuditRecordId, DomainId, PolicyRuleId, PrincipalId, SecurityEpoch,
    };
    use crate::non_interference::{
        CursorObservation, PairedWorld, PublicFailure, PublicObservation,
    };
    use crate::security::{
        AuthorizationDecision, Capability, CapabilityGrant, CapabilityRule, GrantEffect,
        PolicyScope, PolicySubject, PolicyTarget, Principal, SecurityPolicySnapshot,
    };
    use crate::values::Bytes;

    fn id<T: DomainId>(tail: u8) -> Result<T, crate::IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes)
    }

    fn fingerprint(value: u8) -> Result<AuditPolicyFingerprint, crate::AuditFingerprintError> {
        AuditPolicyFingerprint::new(Bytes::new(vec![value; 32]))
    }

    fn policy(actor: PrincipalId, can_manage: bool) -> SecurityPolicySnapshot {
        let rules = if can_manage {
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(4)
                    .unwrap_or_else(|_| unreachable!("test policy rule ID is valid")),
                PolicySubject::Principal(actor),
                CapabilityGrant::new(Capability::SecurityPolicyManage, GrantEffect::Allow),
                PolicyScope::project(),
            )]
        } else {
            Vec::new()
        };
        SecurityPolicySnapshot::new(vec![Principal::new(actor)], Vec::new(), Vec::new(), rules)
            .unwrap_or_else(|_| unreachable!("test policy snapshot is valid"))
    }

    #[test]
    fn audit_retention_and_read_export_configure_rights_are_independent() -> Result<(), TestError> {
        assert_eq!(
            AuditRetentionPolicy::new(0, 10),
            Err(AuditRetentionError::ZeroBound)
        );
        assert_eq!(
            AuditRetentionPolicy::new(1, super::MAX_IN_MEMORY_AUDIT_RECORDS + 1),
            Err(AuditRetentionError::CapacityExceeded)
        );
        let retention = AuditRetentionPolicy::new(86_400_000, 500)?;
        assert_eq!(retention.minimum_retention_millis(), 86_400_000);
        assert_eq!(retention.maximum_records(), 500);
        let audit = InMemoryRequiredAuditPort::with_retention(retention)?;
        assert_eq!(audit.retention_policy(), Some(retention));

        let actor = id::<PrincipalId>(12)?;
        let read_only = SecurityPolicySnapshot::new(
            vec![Principal::new(actor)],
            Vec::new(),
            Vec::new(),
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(13)?,
                PolicySubject::Principal(actor),
                CapabilityGrant::new(Capability::AuditRead, GrantEffect::Allow),
                PolicyScope::project(),
            )],
        )
        .map_err(|_| TestError::Audit)?;
        let permissions =
            AuditAccessPermissions::evaluate(&read_only, actor, PolicyTarget::default());
        assert!(permissions.may_read());
        assert!(!permissions.may_export());
        assert!(!permissions.may_configure());
        Ok(())
    }

    fn record(
        actor: PrincipalId,
        epoch: SecurityEpoch,
        policy_fingerprint: AuditPolicyFingerprint,
    ) -> Result<AuditRecord, crate::IdValidationError> {
        Ok(AuditRecord::new(
            AuditRecordIdentity {
                record_id: id::<AuditRecordId>(1)?,
                sequence: AuditSequence::new(1),
                audit_operation_id: id::<AuditOperationId>(2)?,
            },
            AuditRecordDetails {
                actor,
                action: AuditAction::SecurityPolicyChange,
                object_class: AuditObjectClass::SecurityPolicy,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::NotCommitted,
                security_epoch: epoch,
                policy_fingerprint,
            },
        ))
    }

    #[derive(Debug)]
    enum TestError {
        Identity,
        Fingerprint,
        Audit,
        Retention,
    }

    impl From<crate::IdValidationError> for TestError {
        fn from(_: crate::IdValidationError) -> Self {
            Self::Identity
        }
    }

    impl From<crate::AuditFingerprintError> for TestError {
        fn from(_: crate::AuditFingerprintError) -> Self {
            Self::Fingerprint
        }
    }

    impl From<RequiredAuditError> for TestError {
        fn from(_: RequiredAuditError) -> Self {
            Self::Audit
        }
    }

    impl From<AuditRetentionError> for TestError {
        fn from(_: AuditRetentionError) -> Self {
            Self::Retention
        }
    }

    #[test]
    fn required_audit_failure_blocks_policy_change_without_partial_effect() -> Result<(), TestError>
    {
        let actor = id::<PrincipalId>(3)?;
        let old_fingerprint = fingerprint(4)?;
        let new_fingerprint = fingerprint(5)?;
        let mut state = InMemoryPolicyState::new(
            policy(actor, true),
            SecurityEpoch::INITIAL,
            old_fingerprint.clone(),
        );
        let mut audit = InMemoryRequiredAuditPort::new(2)?;
        audit.fail_next_append();

        let result = apply_required_policy_change(
            &mut state,
            policy(actor, false),
            new_fingerprint,
            record(actor, SecurityEpoch::INITIAL, old_fingerprint.clone())?,
            &mut audit,
        );
        assert_eq!(
            result,
            Err(PolicyAuditError::RequiredAuditUnavailable(
                RequiredAuditError::Unavailable
            ))
        );
        assert_eq!(
            state.policy().authorize(
                actor,
                Capability::SecurityPolicyManage,
                PolicyTarget::default(),
            ),
            AuthorizationDecision::Allow
        );
        assert_eq!(state.epoch(), SecurityEpoch::INITIAL);
        assert_eq!(state.fingerprint(), &old_fingerprint);
        assert!(audit.records().is_empty());
        Ok(())
    }

    #[test]
    fn paired_required_audit_fault_hides_the_attempted_policy_fingerprint() -> Result<(), TestError>
    {
        let actor = id::<PrincipalId>(24)?;
        let old_fingerprint = fingerprint(25)?;
        let new_fingerprints = (fingerprint(26)?, fingerprint(27)?);
        let worlds = PairedWorld::new(actor, new_fingerprints.0, new_fingerprints.1);
        worlds
            .compare(|actor, new_fingerprint| {
                let mut state = InMemoryPolicyState::new(
                    policy(*actor, true),
                    SecurityEpoch::INITIAL,
                    old_fingerprint.clone(),
                );
                let mut audit = InMemoryRequiredAuditPort::new(1)
                    .unwrap_or_else(|_| unreachable!("positive audit capacity is valid"));
                audit.fail_next_append();
                match apply_required_policy_change(
                    &mut state,
                    policy(*actor, false),
                    new_fingerprint.clone(),
                    record(*actor, SecurityEpoch::INITIAL, old_fingerprint.clone())
                        .unwrap_or_else(|_| unreachable!("test audit record is valid")),
                    &mut audit,
                ) {
                    Err(error) => PublicObservation::<()>::failure(
                        PublicFailure::new(error.to_string(), vec!["code".to_owned()]),
                        vec!["error".to_owned()],
                        CursorObservation::Absent,
                    ),
                    Ok(_) => PublicObservation::success(
                        (),
                        vec!["result".to_owned()],
                        CursorObservation::Absent,
                    ),
                }
            })
            .map_err(|_| TestError::Audit)?;
        Ok(())
    }

    #[test]
    fn policy_change_requires_security_policy_manage_before_audit_or_mutation()
    -> Result<(), TestError> {
        let actor = id::<PrincipalId>(14)?;
        let old_fingerprint = fingerprint(15)?;
        let mut state = InMemoryPolicyState::new(
            policy(actor, false),
            SecurityEpoch::INITIAL,
            old_fingerprint.clone(),
        );
        let mut audit = InMemoryRequiredAuditPort::new(1)?;
        let result = apply_required_policy_change(
            &mut state,
            policy(actor, true),
            fingerprint(16)?,
            record(actor, SecurityEpoch::INITIAL, old_fingerprint)?,
            &mut audit,
        );
        assert_eq!(result, Err(PolicyAuditError::Unauthorized));
        assert_eq!(state.epoch(), SecurityEpoch::INITIAL);
        assert!(audit.records().is_empty());
        Ok(())
    }

    #[test]
    fn accepted_required_audit_precedes_the_infallible_policy_publication() -> Result<(), TestError>
    {
        let actor = id::<PrincipalId>(6)?;
        let old_fingerprint = fingerprint(7)?;
        let new_fingerprint = fingerprint(8)?;
        let mut state = InMemoryPolicyState::new(
            policy(actor, true),
            SecurityEpoch::INITIAL,
            old_fingerprint.clone(),
        );
        let mut audit = InMemoryRequiredAuditPort::new(1)?;
        let next = apply_required_policy_change(
            &mut state,
            policy(actor, false),
            new_fingerprint.clone(),
            record(actor, SecurityEpoch::INITIAL, old_fingerprint.clone())?,
            &mut audit,
        );
        let next_epoch = SecurityEpoch::INITIAL
            .next()
            .map_err(|_| TestError::Audit)?;
        assert_eq!(next, Ok(next_epoch));
        assert_eq!(
            state.policy().authorize(
                actor,
                Capability::SecurityPolicyManage,
                PolicyTarget::default(),
            ),
            AuthorizationDecision::Deny
        );
        assert_eq!(state.epoch(), SecurityEpoch::new(1));
        assert_eq!(state.fingerprint(), &new_fingerprint);
        let audit_record = audit.records().first().ok_or(TestError::Audit)?;
        assert_eq!(audit_record.action(), AuditAction::SecurityPolicyChange);
        assert_eq!(audit_record.outcome(), AuditOutcome::Succeeded);
        assert_eq!(audit_record.security_epoch(), SecurityEpoch::INITIAL);
        assert_eq!(audit_record.policy_fingerprint(), &old_fingerprint);
        assert_eq!(
            audit_record.commit_context(),
            AuditCommitContext::NotCommitted
        );
        Ok(())
    }

    #[test]
    fn telemetry_failure_is_independent_of_required_audit_failure() -> Result<(), TestError> {
        let diagnostics = BoundedDiagnostics::new(1).map_err(|_| TestError::Audit)?;
        let event = DiagnosticEvent::new(
            DiagnosticSpanName::TransactionCommit,
            DiagnosticEventKind::Failed,
            vec![DiagnosticField::omitted(DiagnosticFieldKey::OperationClass)],
        )
        .map_err(|_| TestError::Audit)?;
        diagnostics.report(event);
        diagnostics.report(
            DiagnosticEvent::new(
                DiagnosticSpanName::TransactionCommit,
                DiagnosticEventKind::Failed,
                vec![DiagnosticField::omitted(DiagnosticFieldKey::OperationClass)],
            )
            .map_err(|_| TestError::Audit)?,
        );
        let actor = id::<PrincipalId>(9)?;
        let old_fingerprint = fingerprint(10)?;
        let mut state = InMemoryPolicyState::new(
            policy(actor, true),
            SecurityEpoch::INITIAL,
            old_fingerprint.clone(),
        );
        let mut audit = InMemoryRequiredAuditPort::new(1)?;
        audit.fail_next_append();
        let action = apply_required_policy_change(
            &mut state,
            policy(actor, false),
            fingerprint(11)?,
            record(actor, SecurityEpoch::INITIAL, old_fingerprint)?,
            &mut audit,
        );
        assert_eq!(
            action,
            Err(PolicyAuditError::RequiredAuditUnavailable(
                RequiredAuditError::Unavailable
            ))
        );
        assert_eq!(
            state.policy().authorize(
                actor,
                Capability::SecurityPolicyManage,
                PolicyTarget::default(),
            ),
            AuthorizationDecision::Allow
        );
        assert_eq!(diagnostics.counter(DiagnosticCounter::DroppedTelemetry), 1);
        assert!(audit.records().is_empty());
        Ok(())
    }
}
