//! Authenticated, append-only policy administration backed by required-audit WAL commits.

use std::fmt;

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuthorizationDecision, Bytes, Capability,
    CapabilityGrant, CapabilityRule, GrantEffect, OperationId, PolicyRuleId, PolicyScope,
    PolicySubject, PolicyTarget, PrincipalId, PrincipalState, Revision, RoleAssignment,
    RoleAssignmentId, RoleDefinition, RoleId, SecurityEpoch, SecurityPolicyChange,
    SecurityPolicyRecord, SecurityPolicyRecordId, SecurityPolicySnapshot, SecurityPolicyVersion,
    Symbol,
};

use crate::{
    DatabaseLayout, Manifest, ManifestSegmentKind, ManifestSegmentReference, ManifestStore,
    RecoveryDisposition, RecoveryManager, SecurityPolicyHistorySnapshot,
    SecurityPolicyHistoryStore, StorageVerifier, WalOperationStatus, WalPrepareLog, WriterLock,
};

/// File-backed policy administration bound to one host-authenticated principal.
///
/// The host supplies the principal identity. Renderer input can select known
/// policy objects but cannot choose the actor or supply capabilities.
pub struct FileSecurityPolicyManager<'a> {
    layout: DatabaseLayout,
    writer_lock: &'a WriterLock,
    principal: PrincipalId,
    manifest: Manifest,
    history: SecurityPolicyHistorySnapshot,
}

/// Receipt for an atomically published policy change and Required Audit record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPublicationReceipt {
    operation_id: OperationId,
    revision: Revision,
    epoch: SecurityEpoch,
}

impl PolicyPublicationReceipt {
    /// Stable idempotency identity for this commit.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Shared revision assigned to the policy change.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Security epoch made active by the policy change.
    #[must_use]
    pub const fn epoch(self) -> SecurityEpoch {
        self.epoch
    }
}

impl<'a> FileSecurityPolicyManager<'a> {
    /// Opens a clean database and loads its complete policy history.
    pub fn open(
        layout: DatabaseLayout,
        writer_lock: &'a WriterLock,
        principal: PrincipalId,
    ) -> Result<Self, PolicyManagementError> {
        let recovery = RecoveryManager::new(layout.clone())
            .recover(writer_lock)
            .map_err(storage_error)?;
        if !recovery.report().is_clean() {
            return Err(PolicyManagementError::Storage(
                "database requires recovery".to_owned(),
            ));
        }
        let verified = StorageVerifier::new(layout.clone())
            .verify(writer_lock)
            .map_err(storage_error)?;
        if verified.disposition() != RecoveryDisposition::Clean {
            return Err(PolicyManagementError::Storage(
                "database verification is not clean".to_owned(),
            ));
        }
        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(storage_error)?
            .ok_or_else(|| {
                PolicyManagementError::Storage("database manifest is missing".to_owned())
            })?;
        let wal_head = WalPrepareLog::new(&layout)
            .commit_head(writer_lock)
            .map_err(storage_error)?
            .revision();
        if manifest.revision() != wal_head {
            return Err(PolicyManagementError::Storage(
                "database manifest and WAL head disagree".to_owned(),
            ));
        }
        let history = load_policy_history(&layout, &manifest)?;
        Ok(Self {
            layout,
            writer_lock,
            principal,
            manifest,
            history,
        })
    }

    /// Current shared project revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.manifest.revision()
    }

    /// Returns the current policy only when the authenticated actor may read it.
    pub fn policy_snapshot(&self) -> Result<&SecurityPolicySnapshot, PolicyManagementError> {
        let version = self
            .history
            .policy()
            .latest_version()
            .map_err(storage_error)?;
        authorize(
            version.snapshot(),
            self.principal,
            Capability::SecurityPolicyRead,
        )?;
        Ok(version.snapshot())
    }

    /// Security epoch of the current committed policy view.
    pub fn security_epoch(&self) -> Result<SecurityEpoch, PolicyManagementError> {
        self.history
            .policy()
            .latest_version()
            .map(SecurityPolicyVersion::epoch)
            .map_err(storage_error)
    }

    /// Changes the append-only state of a known principal.
    pub fn set_principal_state(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        principal_id: PrincipalId,
        state: PrincipalState,
    ) -> Result<PolicyPublicationReceipt, PolicyManagementError> {
        self.publish_change(
            expected_base,
            operation_id,
            SecurityPolicyChange::PrincipalStateChanged {
                principal_id,
                state,
            },
        )
    }

    /// Registers a role with an empty explicit bundle.
    pub fn register_role(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        role_id: RoleId,
        symbol: Symbol,
    ) -> Result<PolicyPublicationReceipt, PolicyManagementError> {
        self.publish_change(
            expected_base,
            operation_id,
            SecurityPolicyChange::RoleRegistered { role_id, symbol },
        )
    }

    /// Assigns a known role to a known principal at project scope.
    pub fn assign_role(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        assignment_id: RoleAssignmentId,
        principal_id: PrincipalId,
        role_id: RoleId,
    ) -> Result<PolicyPublicationReceipt, PolicyManagementError> {
        self.publish_change(
            expected_base,
            operation_id,
            SecurityPolicyChange::RoleAssigned {
                assignment_id,
                principal_id,
                role_id,
                scope: PolicyScope::project(),
            },
        )
    }

    /// Revokes one existing role assignment without rewriting its history.
    pub fn revoke_role_assignment(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        assignment_id: RoleAssignmentId,
    ) -> Result<PolicyPublicationReceipt, PolicyManagementError> {
        self.publish_change(
            expected_base,
            operation_id,
            SecurityPolicyChange::RoleAssignmentRevoked { assignment_id },
        )
    }

    /// Adds one explicit project-scoped Allow or Deny rule to a principal or role.
    pub fn add_capability_rule(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        rule_id: PolicyRuleId,
        subject: PolicySubject,
        capability: Capability,
        effect: GrantEffect,
    ) -> Result<PolicyPublicationReceipt, PolicyManagementError> {
        self.publish_change(
            expected_base,
            operation_id,
            SecurityPolicyChange::CapabilityRuleAdded {
                rule_id,
                subject,
                capability,
                effect,
                scope: PolicyScope::project(),
            },
        )
    }

    /// Revokes one explicit capability rule without rewriting its history.
    pub fn revoke_capability_rule(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        rule_id: PolicyRuleId,
    ) -> Result<PolicyPublicationReceipt, PolicyManagementError> {
        self.publish_change(
            expected_base,
            operation_id,
            SecurityPolicyChange::CapabilityRuleRevoked { rule_id },
        )
    }

    fn publish_change(
        &mut self,
        expected_base: Revision,
        operation_id: OperationId,
        change: SecurityPolicyChange,
    ) -> Result<PolicyPublicationReceipt, PolicyManagementError> {
        if expected_base != self.revision() {
            return Err(PolicyManagementError::Conflict);
        }
        let wal = WalPrepareLog::new(&self.layout);
        if wal
            .commit_head(self.writer_lock)
            .map_err(storage_error)?
            .revision()
            != expected_base
        {
            return Err(PolicyManagementError::Conflict);
        }
        if wal
            .operation_status(self.writer_lock, operation_id)
            .map_err(storage_error)?
            != WalOperationStatus::NotCommitted
        {
            return Err(PolicyManagementError::OperationAlreadyUsed);
        }

        let current = self
            .history
            .policy()
            .latest_version()
            .map_err(storage_error)?;
        authorize(
            current.snapshot(),
            self.principal,
            Capability::SecurityPolicyManage,
        )?;
        validate_identity_not_reused(&self.history, self.revision(), &change)?;
        let next_snapshot = apply_change(current.snapshot(), &change)?;
        let next_revision = expected_base
            .next_commit()
            .map_err(|error| PolicyManagementError::Storage(error.to_string()))?;
        let next_epoch = current
            .epoch()
            .next()
            .map_err(|error| PolicyManagementError::Storage(error.to_string()))?;
        let policy_record_id = worlddb_core::storage_internal::generate_project_bootstrap_id::<
            SecurityPolicyRecordId,
        >()
        .map_err(storage_error)?;
        let policy_record = SecurityPolicyRecord::new(
            policy_record_id,
            next_revision,
            self.principal,
            next_epoch,
            vec![change],
        )
        .map_err(storage_error)?;
        let audit_sequence = super::next_audit_sequence(&wal, self.writer_lock)
            .map_err(|error| PolicyManagementError::Storage(error.to_string()))?;
        let fingerprint = AuditPolicyFingerprint::new(Bytes::new(
            current
                .snapshot()
                .effective_capability_fingerprint(self.principal, PolicyTarget::default())
                .to_vec(),
        ))
        .map_err(storage_error)?;
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id:
                    worlddb_core::storage_internal::generate_security_policy_audit_record_id()
                        .map_err(storage_error)?,
                sequence: audit_sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_security_policy_audit_operation_id()
                        .map_err(storage_error)?,
            },
            AuditRecordDetails {
                actor: self.principal,
                action: AuditAction::SecurityPolicyChange,
                object_class: AuditObjectClass::SecurityPolicy,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: next_revision,
                    operation_id,
                },
                security_epoch: current.epoch(),
                policy_fingerprint: fingerprint,
            },
        );
        let next_version = SecurityPolicyVersion::new(next_revision, next_epoch, next_snapshot);
        let retention = self
            .history
            .audit_retention_at(self.revision())
            .map_err(storage_error)?;
        let store = SecurityPolicyHistoryStore::new(self.layout.clone());
        let staged = store
            .stage_version(
                self.writer_lock,
                &next_version,
                Some(&policy_record),
                retention,
            )
            .map_err(storage_error)?;
        let reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            staged.id(),
            staged.content_digest(),
            next_revision,
        );
        let mut references = self.manifest.segments().to_vec();
        references.push(reference);
        let commit_result = wal.commit_audited_manifest_snapshot(
            self.writer_lock,
            operation_id,
            references,
            &[reference],
            &audit_record,
        );
        match commit_result {
            Ok(receipt) if receipt.revision() == next_revision => {}
            Ok(_) => return Err(PolicyManagementError::UnknownCommit(operation_id)),
            Err(error) => {
                let recovered = RecoveryManager::new(self.layout.clone()).recover(self.writer_lock);
                let status = wal.operation_status(self.writer_lock, operation_id);
                match (recovered, status) {
                    (Ok(report), Ok(WalOperationStatus::Committed(receipt)))
                        if report.report().is_clean() && receipt.revision() == next_revision => {}
                    (Ok(report), Ok(WalOperationStatus::NotCommitted))
                        if report.report().is_clean() =>
                    {
                        let cleanup = store.remove_staged_reference(reference);
                        return Err(PolicyManagementError::Storage(match cleanup {
                            Ok(()) => error.to_string(),
                            Err(cleanup_error) => {
                                format!("{error}; staged policy cleanup failed: {cleanup_error}")
                            }
                        }));
                    }
                    _ => return Err(PolicyManagementError::UnknownCommit(operation_id)),
                }
            }
        }
        let recovered = RecoveryManager::new(self.layout.clone())
            .recover(self.writer_lock)
            .map_err(|_| PolicyManagementError::UnknownCommit(operation_id))?;
        if !recovered.report().is_clean() {
            return Err(PolicyManagementError::UnknownCommit(operation_id));
        }
        let manifest = ManifestStore::new(self.layout.clone())
            .read_current()
            .map_err(storage_error)?
            .ok_or(PolicyManagementError::UnknownCommit(operation_id))?;
        if manifest.revision() != next_revision {
            return Err(PolicyManagementError::UnknownCommit(operation_id));
        }
        let history = load_policy_history(&self.layout, &manifest)?;
        let published = history.policy().latest_version().map_err(storage_error)?;
        if published.revision() != next_revision || published.epoch() != next_epoch {
            return Err(PolicyManagementError::UnknownCommit(operation_id));
        }
        if history
            .policy_record_at(next_revision)
            .map_err(storage_error)?
            != Some(&policy_record)
        {
            return Err(PolicyManagementError::UnknownCommit(operation_id));
        }
        self.manifest = manifest;
        self.history = history;
        Ok(PolicyPublicationReceipt {
            operation_id,
            revision: next_revision,
            epoch: next_epoch,
        })
    }
}

fn load_policy_history(
    layout: &DatabaseLayout,
    manifest: &Manifest,
) -> Result<SecurityPolicyHistorySnapshot, PolicyManagementError> {
    let ids = manifest
        .segments()
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
        .map(|reference| reference.id())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Err(PolicyManagementError::Storage(
            "security-policy history is missing".to_owned(),
        ));
    }
    SecurityPolicyHistoryStore::new(layout.clone())
        .load_history(manifest.revision(), &ids)
        .map_err(storage_error)
}

fn apply_change(
    current: &SecurityPolicySnapshot,
    change: &SecurityPolicyChange,
) -> Result<SecurityPolicySnapshot, PolicyManagementError> {
    let mut principals = current.principals().to_vec();
    let mut roles = current.roles().to_vec();
    let mut assignments = current.assignments().to_vec();
    let mut rules = current.rules().to_vec();
    match change {
        SecurityPolicyChange::PrincipalStateChanged {
            principal_id,
            state,
        } => {
            let principal = principals
                .iter_mut()
                .find(|principal| principal.id() == *principal_id)
                .ok_or(PolicyManagementError::InvalidChange(
                    "principal is not registered",
                ))?;
            if principal.state() == *state {
                return Err(PolicyManagementError::InvalidChange(
                    "principal already has the requested state",
                ));
            }
            if principal.state() == PrincipalState::Retired && *state != PrincipalState::Retired {
                return Err(PolicyManagementError::InvalidChange(
                    "a retired principal cannot be reactivated",
                ));
            }
            *principal = principal.with_state(*state);
        }
        SecurityPolicyChange::RoleRegistered { role_id, symbol } => {
            let role =
                RoleDefinition::new(*role_id, symbol.as_str(), worlddb_core::PolicyBundle::new())
                    .map_err(|_| PolicyManagementError::InvalidChange("invalid role symbol"))?;
            roles.push(role);
        }
        SecurityPolicyChange::RoleAssigned {
            assignment_id,
            principal_id,
            role_id,
            scope,
        } => {
            let principal = principals
                .iter()
                .find(|principal| principal.id() == *principal_id)
                .ok_or(PolicyManagementError::InvalidChange(
                    "principal is not registered",
                ))?;
            if principal.state() != PrincipalState::Active {
                return Err(PolicyManagementError::InvalidChange(
                    "a role cannot be assigned to an inactive principal",
                ));
            }
            if !roles.iter().any(|role| role.id() == *role_id) {
                return Err(PolicyManagementError::InvalidChange(
                    "role is not registered",
                ));
            }
            if assignments.iter().any(|assignment| {
                assignment.principal() == *principal_id
                    && assignment.role() == *role_id
                    && assignment.scope() == *scope
            }) {
                return Err(PolicyManagementError::InvalidChange(
                    "the principal already has this role at the selected scope",
                ));
            }
            assignments.push(RoleAssignment::new(
                *assignment_id,
                *principal_id,
                *role_id,
                *scope,
            ));
        }
        SecurityPolicyChange::RoleAssignmentRevoked { assignment_id } => {
            remove_exact(&mut assignments, |assignment| {
                assignment.id() == *assignment_id
            })?;
        }
        SecurityPolicyChange::CapabilityRuleAdded {
            rule_id,
            subject,
            capability,
            effect,
            scope,
        } => {
            match subject {
                PolicySubject::Principal(id) if !principals.iter().any(|item| item.id() == *id) => {
                    return Err(PolicyManagementError::InvalidChange(
                        "rule principal is not registered",
                    ));
                }
                PolicySubject::Role(id) if !roles.iter().any(|item| item.id() == *id) => {
                    return Err(PolicyManagementError::InvalidChange(
                        "rule role is not registered",
                    ));
                }
                _ => {}
            }
            if rules.iter().any(|rule| {
                rule.subject() == *subject
                    && rule.grant() == CapabilityGrant::new(*capability, *effect)
                    && rule.scope() == *scope
            }) {
                return Err(PolicyManagementError::InvalidChange(
                    "the same explicit capability rule already exists",
                ));
            }
            rules.push(CapabilityRule::new(
                *rule_id,
                *subject,
                CapabilityGrant::new(*capability, *effect),
                *scope,
            ));
        }
        SecurityPolicyChange::CapabilityRuleRevoked { rule_id } => {
            remove_exact(&mut rules, |rule| rule.id() == *rule_id)?;
        }
        SecurityPolicyChange::PrincipalRegistered { .. }
        | SecurityPolicyChange::RoleRetired { .. } => {
            return Err(PolicyManagementError::InvalidChange(
                "this principal or role transition requires a trusted host enrollment flow",
            ));
        }
    }
    SecurityPolicySnapshot::new(principals, roles, assignments, rules)
        .map_err(|_| PolicyManagementError::InvalidChange("candidate policy is inconsistent"))
}

fn remove_exact<T>(
    items: &mut Vec<T>,
    predicate: impl Fn(&T) -> bool,
) -> Result<(), PolicyManagementError> {
    let Some(index) = items.iter().position(predicate) else {
        return Err(PolicyManagementError::InvalidChange(
            "policy object is not active",
        ));
    };
    items.remove(index);
    Ok(())
}

fn validate_identity_not_reused(
    history: &SecurityPolicyHistorySnapshot,
    through: Revision,
    change: &SecurityPolicyChange,
) -> Result<(), PolicyManagementError> {
    for value in 0..=through.value() {
        let revision = Revision::try_from(value).map_err(storage_error)?;
        let Some(record) = history.policy_record_at(revision).map_err(storage_error)? else {
            continue;
        };
        for prior in record.changes() {
            let reused = match (prior, change) {
                (
                    SecurityPolicyChange::RoleRegistered { role_id: left, .. },
                    SecurityPolicyChange::RoleRegistered { role_id: right, .. },
                ) => left == right,
                (
                    SecurityPolicyChange::RoleAssigned {
                        assignment_id: left,
                        ..
                    },
                    SecurityPolicyChange::RoleAssigned {
                        assignment_id: right,
                        ..
                    },
                ) => left == right,
                (
                    SecurityPolicyChange::CapabilityRuleAdded { rule_id: left, .. },
                    SecurityPolicyChange::CapabilityRuleAdded { rule_id: right, .. },
                ) => left == right,
                _ => false,
            };
            if reused {
                return Err(PolicyManagementError::InvalidChange(
                    "policy identity was already used in history",
                ));
            }
        }
    }
    Ok(())
}

fn authorize(
    policy: &SecurityPolicySnapshot,
    principal: PrincipalId,
    capability: Capability,
) -> Result<(), PolicyManagementError> {
    if policy.authorize(principal, capability, PolicyTarget::default())
        == AuthorizationDecision::Allow
    {
        Ok(())
    } else {
        Err(PolicyManagementError::Unauthorized(capability))
    }
}

fn storage_error(error: impl fmt::Display) -> PolicyManagementError {
    PolicyManagementError::Storage(error.to_string())
}

/// Authorization, validation, or durable policy publication error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyManagementError {
    /// The actor lacks the named current capability.
    Unauthorized(Capability),
    /// The database changed after the caller selected the base revision.
    Conflict,
    /// The requested state transition or object reference is invalid.
    InvalidChange(&'static str),
    /// The operation identity has already been committed or reserved.
    OperationAlreadyUsed,
    /// The commit point may have been reached but the outcome is unresolved.
    UnknownCommit(OperationId),
    /// Policy history, verification, audit, or file storage failed.
    Storage(String),
}

impl fmt::Display for PolicyManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized(capability) => {
                write!(formatter, "missing required capability {capability:?}")
            }
            Self::Conflict => {
                formatter.write_str("database changed; reload the policy before retrying")
            }
            Self::InvalidChange(reason) => formatter.write_str(reason),
            Self::OperationAlreadyUsed => formatter.write_str("OperationId is already used"),
            Self::UnknownCommit(operation_id) => {
                write!(
                    formatter,
                    "policy commit outcome is unknown for {operation_id}"
                )
            }
            Self::Storage(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PolicyManagementError {}

#[cfg(test)]
mod tests {
    use worlddb_core::{
        Capability, GrantEffect, PolicySubject, PrincipalState, Revision, SecurityEpoch,
    };

    use crate::{DatabaseLayout, StorageVerifier, WalPrepareLog};

    use super::{FileSecurityPolicyManager, PolicyManagementError};

    use crate::schema_management::tests::{TempArea, create_project, id};

    #[test]
    fn policy_rule_publication_advances_epoch_and_required_audit_atomically() -> Result<(), String>
    {
        let area = TempArea::create()?;
        let database = area.database();
        let principal = create_project(
            &database,
            &[
                Capability::SecurityPolicyRead,
                Capability::SecurityPolicyManage,
            ],
        )?;
        let layout = DatabaseLayout::open(&database).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut manager = FileSecurityPolicyManager::open(layout.clone(), &lock, principal)
            .map_err(|error| error.to_string())?;
        let base = manager.revision();
        let operation = id::<worlddb_core::OperationId>(61)?;
        let rule_id = id::<worlddb_core::PolicyRuleId>(62)?;
        let receipt = manager
            .add_capability_rule(
                base,
                operation,
                rule_id,
                PolicySubject::Principal(principal),
                Capability::AdminRawRead,
                GrantEffect::Allow,
            )
            .map_err(|error| error.to_string())?;

        assert_eq!(
            receipt.revision(),
            base.next_commit().map_err(|error| error.to_string())?
        );
        assert_eq!(receipt.epoch(), SecurityEpoch::new(2));
        let snapshot = manager
            .policy_snapshot()
            .map_err(|error| error.to_string())?;
        assert!(snapshot.rules().iter().any(|rule| rule.id() == rule_id));
        assert_eq!(
            manager
                .security_epoch()
                .map_err(|error| error.to_string())?,
            receipt.epoch()
        );
        let audits = WalPrepareLog::new(&layout)
            .committed_required_audit_records(&lock)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            audits.last().map(|entry| entry.record().action()),
            Some(worlddb_core::AuditAction::SecurityPolicyChange)
        );
        assert_eq!(
            audits.last().map(|entry| entry.record().commit_context()),
            Some(worlddb_core::AuditCommitContext::Committed {
                revision: receipt.revision(),
                operation_id: operation
            })
        );
        let verified = StorageVerifier::new(layout)
            .verify(&lock)
            .map_err(|error| error.to_string())?;
        assert!(verified.is_clean());
        Ok(())
    }

    #[test]
    fn policy_read_and_management_capabilities_are_independent() -> Result<(), String> {
        let reader_area = TempArea::create()?;
        let reader_database = reader_area.database();
        let reader = create_project(&reader_database, &[Capability::SecurityPolicyRead])?;
        let reader_layout =
            DatabaseLayout::open(&reader_database).map_err(|error| error.to_string())?;
        let reader_lock = reader_layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut reader_manager =
            FileSecurityPolicyManager::open(reader_layout, &reader_lock, reader)
                .map_err(|error| error.to_string())?;
        let base = reader_manager.revision();
        reader_manager
            .policy_snapshot()
            .map_err(|error| error.to_string())?;
        assert_eq!(
            reader_manager.set_principal_state(
                base,
                id::<worlddb_core::OperationId>(65)?,
                reader,
                PrincipalState::Disabled,
            ),
            Err(PolicyManagementError::Unauthorized(
                Capability::SecurityPolicyManage
            )),
        );
        assert_eq!(reader_manager.revision(), base);
        assert_eq!(
            reader_manager
                .security_epoch()
                .map_err(|error| error.to_string())?,
            SecurityEpoch::new(1)
        );

        let manager_area = TempArea::create()?;
        let manager_database = manager_area.database();
        let manager = create_project(&manager_database, &[Capability::SecurityPolicyManage])?;
        let manager_layout =
            DatabaseLayout::open(&manager_database).map_err(|error| error.to_string())?;
        let manager_lock = manager_layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut policy_manager =
            FileSecurityPolicyManager::open(manager_layout, &manager_lock, manager)
                .map_err(|error| error.to_string())?;
        assert_eq!(
            policy_manager.policy_snapshot().err(),
            Some(PolicyManagementError::Unauthorized(
                Capability::SecurityPolicyRead
            )),
        );
        let receipt = policy_manager
            .set_principal_state(
                Revision::FIRST_COMMIT,
                id::<worlddb_core::OperationId>(66)?,
                manager,
                PrincipalState::Disabled,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(receipt.epoch(), SecurityEpoch::new(2));
        assert_eq!(
            receipt.revision(),
            Revision::FIRST_COMMIT
                .next_commit()
                .map_err(|error| error.to_string())?
        );
        Ok(())
    }

    #[test]
    fn self_revocation_commits_and_takes_effect_at_the_same_revision() -> Result<(), String> {
        let area = TempArea::create()?;
        let database = area.database();
        let principal = create_project(
            &database,
            &[
                Capability::SecurityPolicyRead,
                Capability::SecurityPolicyManage,
            ],
        )?;
        let layout = DatabaseLayout::open(&database).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut manager = FileSecurityPolicyManager::open(layout, &lock, principal)
            .map_err(|error| error.to_string())?;
        let receipt = manager
            .set_principal_state(
                Revision::FIRST_COMMIT,
                id::<worlddb_core::OperationId>(63)?,
                principal,
                PrincipalState::Disabled,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(receipt.epoch(), SecurityEpoch::new(2));
        assert_eq!(
            manager.policy_snapshot().err(),
            Some(PolicyManagementError::Unauthorized(
                Capability::SecurityPolicyRead
            )),
        );
        Ok(())
    }
}
