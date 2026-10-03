//! Typed authorization and restore-point evidence for guarded migrations.

use std::fmt;

use crate::audit::AuditRecord;
use crate::commit_cancellation::CommitCancellation;
use crate::ids::{
    DatabaseId, DomainId, MigrationRunId, MigrationStepId, OperationId, PrincipalId, Revision,
};
use crate::migration::{
    MigrationCategory, MigrationPlan, MigrationPlanFingerprint, MigrationStepCommitIdentity,
    MigrationStepTargetSchema,
};
use crate::revision_backend::{CancellablePublishError, RevisionBackend};
use crate::security::{AuthorizationDecision, Capability, PolicyTarget, SecurityPolicySnapshot};
use crate::wire_records::Record;

/// Evidence that an exact backup was restored and verified against one breaking plan's source.
///
/// The constructor is reserved for the storage verification adapter. The token is bound to the
/// plan, database identity, source revision/schema, backup inventory, and restored clone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationSafeRestorePoint {
    plan_fingerprint: MigrationPlanFingerprint,
    source_database_id: DatabaseId,
    source_revision: Revision,
    source_schema_fingerprint: [u8; 32],
    source_commit_hash: [u8; 32],
    backup_inventory_digest: [u8; 32],
    backup_manifest_digest: [u8; 32],
    restored_database_id: DatabaseId,
    restored_revision: Revision,
    restore_destination_fingerprint: [u8; 32],
    verification_fingerprint: [u8; 32],
    proof_fingerprint: [u8; 32],
}

impl MigrationSafeRestorePoint {
    // Each parameter is a separately verified source, backup, clone, or verification binding.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0004")]
    pub(crate) fn from_verified_storage(
        plan: &MigrationPlan,
        source_database_id: DatabaseId,
        source_revision: Revision,
        source_commit_hash: [u8; 32],
        backup_inventory_digest: [u8; 32],
        backup_manifest_digest: [u8; 32],
        restored_source_database_id: DatabaseId,
        restored_source_revision: Revision,
        restored_database_id: DatabaseId,
        restored_revision: Revision,
        restore_destination_fingerprint: [u8; 32],
        verification_fingerprint: [u8; 32],
    ) -> Result<Self, MigrationSafeRestorePointError> {
        if plan.category() != MigrationCategory::Breaking {
            return Err(MigrationSafeRestorePointError::PlanNotBreaking);
        }
        let source = plan.source_schema_precondition();
        if source.revision().revision() != source_revision {
            return Err(MigrationSafeRestorePointError::BackupRevisionMismatch);
        }
        if restored_source_database_id != source_database_id
            || restored_source_revision != source_revision
        {
            return Err(MigrationSafeRestorePointError::RestoreSourceMismatch);
        }
        if restored_database_id == source_database_id {
            return Err(MigrationSafeRestorePointError::CloneIdentityMismatch);
        }
        if restored_revision <= source_revision {
            return Err(MigrationSafeRestorePointError::RestoreRevisionNotAdvanced);
        }
        if verification_fingerprint == [0; 32] {
            return Err(MigrationSafeRestorePointError::VerificationMissing);
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"WorldDB.MigrationSafeRestorePoint.v1\0");
        hasher.update(plan.fingerprint().as_bytes());
        hasher.update(&source_database_id.to_bytes());
        hasher.update(&source_revision.value().to_be_bytes());
        hasher.update(source.fingerprint());
        hasher.update(&source_commit_hash);
        hasher.update(&backup_inventory_digest);
        hasher.update(&backup_manifest_digest);
        hasher.update(&restored_database_id.to_bytes());
        hasher.update(&restored_revision.value().to_be_bytes());
        hasher.update(&restore_destination_fingerprint);
        hasher.update(&verification_fingerprint);
        let proof_fingerprint = *hasher.finalize().as_bytes();

        Ok(Self {
            plan_fingerprint: plan.fingerprint(),
            source_database_id,
            source_revision,
            source_schema_fingerprint: *source.fingerprint(),
            source_commit_hash,
            backup_inventory_digest,
            backup_manifest_digest,
            restored_database_id,
            restored_revision,
            restore_destination_fingerprint,
            verification_fingerprint,
            proof_fingerprint,
        })
    }

    /// Immutable migration plan this restore proof authorizes.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> MigrationPlanFingerprint {
        self.plan_fingerprint
    }

    /// Source database identity captured in the exact backup.
    #[must_use]
    pub const fn source_database_id(&self) -> DatabaseId {
        self.source_database_id
    }

    /// Source revision captured in the exact backup.
    #[must_use]
    pub const fn source_revision(&self) -> Revision {
        self.source_revision
    }

    /// Verified source WAL commit hash included by the exact backup.
    #[must_use]
    pub const fn source_commit_hash(&self) -> &[u8; 32] {
        &self.source_commit_hash
    }

    /// Digest of every exact-backup inventory path, length, kind, and file digest.
    #[must_use]
    pub const fn backup_inventory_digest(&self) -> &[u8; 32] {
        &self.backup_inventory_digest
    }

    /// Digest of the exact-backup manifest and its metadata.
    #[must_use]
    pub const fn backup_manifest_digest(&self) -> &[u8; 32] {
        &self.backup_manifest_digest
    }

    /// New database identity of the successfully restored clone.
    #[must_use]
    pub const fn restored_database_id(&self) -> DatabaseId {
        self.restored_database_id
    }

    /// Published revision of the successfully restored clone.
    #[must_use]
    pub const fn restored_revision(&self) -> Revision {
        self.restored_revision
    }

    /// Fingerprint of the unique isolated restore destination used for verification.
    #[must_use]
    pub const fn restore_destination_fingerprint(&self) -> &[u8; 32] {
        &self.restore_destination_fingerprint
    }

    /// Digest of the complete backup and post-publication restore verification report.
    #[must_use]
    pub const fn verification_fingerprint(&self) -> &[u8; 32] {
        &self.verification_fingerprint
    }

    /// Digest binding plan, source, backup inventory, and real restore verification.
    #[must_use]
    pub const fn proof_fingerprint(&self) -> &[u8; 32] {
        &self.proof_fingerprint
    }

    pub(crate) fn matches_source(
        &self,
        plan: &MigrationPlan,
        database_id: DatabaseId,
        source_schema_fingerprint: &[u8; 32],
    ) -> bool {
        self.plan_fingerprint == plan.fingerprint()
            && self.source_database_id == database_id
            && self.source_revision == plan.source_schema_precondition().revision().revision()
            && self.source_schema_fingerprint == *source_schema_fingerprint
            && self.restored_database_id != database_id
            && self.restored_revision > self.source_revision
    }
}

/// Invalid source/restore binding supplied by the storage verifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationSafeRestorePointError {
    /// Restore-point evidence is only valid for a Breaking plan.
    PlanNotBreaking,
    /// Backup revision differs from the plan's exact source revision.
    BackupRevisionMismatch,
    /// Real restore report names a different source identity or revision.
    RestoreSourceMismatch,
    /// Clone restore reused the original database identity.
    CloneIdentityMismatch,
    /// Restore did not publish a new revision after the captured source revision.
    RestoreRevisionNotAdvanced,
    /// Independent verification evidence is missing.
    VerificationMissing,
}

impl fmt::Display for MigrationSafeRestorePointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PlanNotBreaking => "safe restore points are required for Breaking plans",
            Self::BackupRevisionMismatch => "backup revision differs from the migration source",
            Self::RestoreSourceMismatch => {
                "real restore did not reproduce the backup source binding"
            }
            Self::CloneIdentityMismatch => "real restore did not create a distinct database clone",
            Self::RestoreRevisionNotAdvanced => "real restore did not publish its audit revision",
            Self::VerificationMissing => {
                "safe restore point lacks independent verification evidence"
            }
        })
    }
}

impl std::error::Error for MigrationSafeRestorePointError {}

/// Explicit administrator confirmation bound to one Breaking plan and restore proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BreakingMigrationAdminAction {
    plan_fingerprint: MigrationPlanFingerprint,
    actor: PrincipalId,
    target: PolicyTarget,
    authorization_fingerprint: [u8; 32],
    restore_point_fingerprint: [u8; 32],
}

impl BreakingMigrationAdminAction {
    /// Records the explicit call to authorize this exact Breaking plan after a verified restore.
    pub fn confirm(
        plan: &MigrationPlan,
        database_id: DatabaseId,
        source_schema_fingerprint: &[u8; 32],
        actor: PrincipalId,
        policy: &SecurityPolicySnapshot,
        target: PolicyTarget,
        restore_point: &MigrationSafeRestorePoint,
    ) -> Result<Self, BreakingMigrationAdminActionError> {
        if plan.category() != MigrationCategory::Breaking {
            return Err(BreakingMigrationAdminActionError::PlanNotBreaking);
        }
        if policy.authorize(actor, Capability::MigrationExecute, target)
            != AuthorizationDecision::Allow
        {
            return Err(BreakingMigrationAdminActionError::Unauthorized);
        }
        if !restore_point.matches_source(plan, database_id, source_schema_fingerprint) {
            return Err(BreakingMigrationAdminActionError::RestorePointMismatch);
        }
        Ok(Self {
            plan_fingerprint: plan.fingerprint(),
            actor,
            target,
            authorization_fingerprint: policy.effective_capability_fingerprint(actor, target),
            restore_point_fingerprint: *restore_point.proof_fingerprint(),
        })
    }

    pub(crate) fn matches_current(
        &self,
        plan: &MigrationPlan,
        actor: PrincipalId,
        policy: &SecurityPolicySnapshot,
        target: PolicyTarget,
        restore_point: &MigrationSafeRestorePoint,
    ) -> bool {
        self.plan_fingerprint == plan.fingerprint()
            && self.actor == actor
            && self.target == target
            && self.authorization_fingerprint
                == policy.effective_capability_fingerprint(actor, target)
            && self.restore_point_fingerprint == *restore_point.proof_fingerprint()
    }
}

/// Why an explicit Breaking migration confirmation could not be created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakingMigrationAdminActionError {
    /// Explicit irreversible confirmation only applies to Breaking plans.
    PlanNotBreaking,
    /// Current policy does not grant MigrationExecute for this target.
    Unauthorized,
    /// Restore evidence is stale, belongs to another plan, or names another source.
    RestorePointMismatch,
}

impl fmt::Display for BreakingMigrationAdminActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PlanNotBreaking => "explicit admin confirmation requires a Breaking plan",
            Self::Unauthorized => "current MigrationExecute permission is required",
            Self::RestorePointMismatch => "restore point does not match the Breaking plan source",
        })
    }
}

impl std::error::Error for BreakingMigrationAdminActionError {}

/// Canonical safe context committed beside one migration step and its required audit record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationAuditCommit {
    plan_fingerprint: MigrationPlanFingerprint,
    migration_id: crate::MigrationId,
    run_id: MigrationRunId,
    step_id: MigrationStepId,
    operation_id: OperationId,
    base_revision: Revision,
    commit_revision: Revision,
    target_schema: MigrationStepTargetSchema,
    input_fingerprint: [u8; 32],
    decision_fingerprint: [u8; 32],
}

impl MigrationAuditCommit {
    // Keep every commit-bound identity and fingerprint explicit at this canonical constructor.
    #[allow(clippy::too_many_arguments, reason = "WDB-EXC-0004")]
    pub(crate) const fn new(
        plan_fingerprint: MigrationPlanFingerprint,
        migration_id: crate::MigrationId,
        run_id: MigrationRunId,
        step_id: MigrationStepId,
        operation_id: OperationId,
        base_revision: Revision,
        commit_revision: Revision,
        target_schema: MigrationStepTargetSchema,
        input_fingerprint: [u8; 32],
        decision_fingerprint: [u8; 32],
    ) -> Self {
        Self {
            plan_fingerprint,
            migration_id,
            run_id,
            step_id,
            operation_id,
            base_revision,
            commit_revision,
            target_schema,
            input_fingerprint,
            decision_fingerprint,
        }
    }

    /// Immutable plan fingerprint to persist in the audit action payload.
    #[must_use]
    pub const fn plan_fingerprint(self) -> MigrationPlanFingerprint {
        self.plan_fingerprint
    }

    /// Migration and step identities for this audited commit.
    #[must_use]
    pub const fn identity(self) -> MigrationStepCommitIdentity {
        MigrationStepCommitIdentity::with_plan_and_input_fingerprint(
            self.migration_id,
            self.run_id,
            self.step_id,
            self.operation_id,
            self.input_fingerprint,
            *self.plan_fingerprint.as_bytes(),
            self.decision_fingerprint,
        )
    }

    /// Operation identity bound by the required audit record.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Revision expected from this OCC commit.
    #[must_use]
    pub const fn commit_revision(self) -> Revision {
        self.commit_revision
    }

    /// Deterministic action payload for the separate Required Audit log.
    #[must_use]
    pub fn canonical_action_payload(self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(256);
        payload.extend_from_slice(b"WorldDB.RequiredAudit.Migration.v1\0");
        payload.extend_from_slice(self.plan_fingerprint.as_bytes());
        payload.extend_from_slice(&self.migration_id.to_bytes());
        payload.extend_from_slice(&self.run_id.to_bytes());
        payload.extend_from_slice(&self.step_id.to_bytes());
        payload.extend_from_slice(&self.operation_id.to_bytes());
        payload.extend_from_slice(&self.base_revision.value().to_be_bytes());
        payload.extend_from_slice(&self.commit_revision.value().to_be_bytes());
        payload.extend_from_slice(
            &self
                .target_schema
                .schema()
                .revision()
                .revision()
                .value()
                .to_be_bytes(),
        );
        payload.extend_from_slice(self.target_schema.schema().fingerprint());
        payload.extend_from_slice(&self.input_fingerprint);
        payload.extend_from_slice(&self.decision_fingerprint);
        payload
    }
}

/// Backend port whose migration action and Required Audit record share one commitpoint.
///
/// Implementations must validate `expected_base_revision` and store the migration batch,
/// canonical audit action, and audit record atomically. A definite publication error means
/// neither side became visible; `OutcomeUnknown` requires reconciliation by OperationId. A
/// successful revision must equal `audit_commit.commit_revision()`.
pub trait MigrationCommitBackend: RevisionBackend<Record> {
    /// Atomically publishes one plan-bound migration step and its Required Audit record.
    fn publish_migration_step_with_required_audit(
        &mut self,
        expected_base_revision: Revision,
        entries: Vec<Record>,
        audit_commit: MigrationAuditCommit,
        audit_record: AuditRecord,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError>;
}
