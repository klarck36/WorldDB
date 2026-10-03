//! Exact-backup plus real-restore evidence for Breaking schema migrations.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use worlddb_core::{
    AuditFingerprintError, AuditPolicyFingerprint, AuthorizationDecision, Bytes, Capability,
    DomainId, MigrationCategory, MigrationPlan, MigrationSafeRestorePoint,
    MigrationSafeRestorePointError, PolicyTarget, SecurityPolicyView,
};

use super::{
    BackupError, BackupMacKey, BackupProfile, ExactBackupManager, RestoreError, RestoreManager,
};
use crate::{DatabaseLayout, StorageFileError};

/// Why an exact backup and real restore could not establish migration recovery evidence.
#[derive(Debug)]
pub enum MigrationRestorePointError {
    /// The immutable plan is not a Breaking migration.
    PlanNotBreaking,
    /// A current capability required to create and restore the proof is denied.
    AuthorizationDenied { capability: Capability },
    /// Backup, restore, or source layout verification failed.
    Backup(BackupError),
    /// The real restore failed before producing a verified clone.
    Restore(RestoreError),
    /// Source identity or revision differs from the plan's exact precondition.
    SourceBindingMismatch,
    /// The restore did not return the exact source snapshot as a distinct clone.
    RestoreBindingMismatch,
    /// The published clone could not be reopened with the reported identity.
    RestoredLayoutMismatch,
    /// A filesystem operation failed while binding the restore destination identity.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// The policy fingerprint could not be represented as an audit value.
    AuditFingerprint(AuditFingerprintError),
    /// The core rejected the resulting typed proof binding.
    Proof(MigrationSafeRestorePointError),
    /// A source layout could not be opened for verification.
    StorageFile(StorageFileError),
}

impl fmt::Display for MigrationRestorePointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanNotBreaking => {
                formatter.write_str("restore points are only built for Breaking plans")
            }
            Self::AuthorizationDenied { capability } => {
                write!(
                    formatter,
                    "migration restore-point creation requires current {capability:?} permission"
                )
            }
            Self::Backup(error) => write!(formatter, "exact migration backup failed: {error}"),
            Self::Restore(error) => {
                write!(formatter, "real migration restore test failed: {error}")
            }
            Self::SourceBindingMismatch => formatter.write_str(
                "exact backup does not match the migration source identity and revision",
            ),
            Self::RestoreBindingMismatch => {
                formatter.write_str("real restore does not match the verified exact backup")
            }
            Self::RestoredLayoutMismatch => {
                formatter.write_str("restored clone failed its independent identity reopen check")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::AuditFingerprint(error) => {
                write!(formatter, "audit policy fingerprint failed: {error}")
            }
            Self::Proof(error) => {
                write!(formatter, "migration restore-point proof failed: {error}")
            }
            Self::StorageFile(error) => write!(
                formatter,
                "restored database layout could not be reopened: {error}"
            ),
        }
    }
}

impl std::error::Error for MigrationRestorePointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backup(error) => Some(error),
            Self::Restore(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::AuditFingerprint(error) => Some(error),
            Self::Proof(error) => Some(error),
            Self::StorageFile(error) => Some(error),
            Self::PlanNotBreaking
            | Self::AuthorizationDenied { .. }
            | Self::SourceBindingMismatch
            | Self::RestoreBindingMismatch
            | Self::RestoredLayoutMismatch => None,
        }
    }
}

impl ExactBackupManager {
    /// Creates and verifies an exact backup, then restores it as a new clone for a Breaking plan.
    ///
    /// Both caller-selected paths must be new and outside the source. The verified backup and
    /// restored clone are retained as the concrete recovery point; neither is used as a live
    /// migration target.
    pub fn create_migration_safe_restore_point(
        &self,
        plan: &MigrationPlan,
        backup_target: impl AsRef<Path>,
        restore_destination: impl AsRef<Path>,
        key: Option<&BackupMacKey>,
        policy: SecurityPolicyView<'_>,
        policy_target: PolicyTarget,
    ) -> Result<MigrationSafeRestorePoint, MigrationRestorePointError> {
        if plan.category() != MigrationCategory::Breaking {
            return Err(MigrationRestorePointError::PlanNotBreaking);
        }
        for capability in [
            Capability::MigrationExecute,
            Capability::BackupCreate,
            Capability::BackupRestore,
        ] {
            if policy
                .current_snapshot()
                .authorize(policy.principal_id(), capability, policy_target)
                != AuthorizationDecision::Allow
            {
                return Err(MigrationRestorePointError::AuthorizationDenied { capability });
            }
        }

        let source_database_id =
            self.source
                .database_id()
                .ok_or(MigrationRestorePointError::Backup(
                    BackupError::DatabaseIdentityMissing,
                ))?;
        let backup = self
            .create_exact_backup(backup_target.as_ref(), key)
            .map_err(MigrationRestorePointError::Backup)?;
        if backup.profile() != BackupProfile::ExactDatabase
            || backup.database_id() != source_database_id
            || backup.revision() != plan.source_schema_precondition().revision().revision()
            || !backup.storage_report().is_clean()
        {
            return Err(MigrationRestorePointError::SourceBindingMismatch);
        }

        let policy_fingerprint = policy
            .current_snapshot()
            .effective_capability_fingerprint(policy.principal_id(), policy_target);
        let audit_policy_fingerprint =
            AuditPolicyFingerprint::new(Bytes::new(policy_fingerprint.to_vec()))
                .map_err(MigrationRestorePointError::AuditFingerprint)?;
        let restore = RestoreManager::new()
            .restore_clone(
                backup_target.as_ref(),
                restore_destination.as_ref(),
                key,
                policy,
                policy_target,
                audit_policy_fingerprint,
            )
            .map_err(MigrationRestorePointError::Restore)?;
        if restore.profile() != BackupProfile::ExactDatabase
            || restore.source_database_id() != backup.database_id()
            || restore.source_revision() != backup.revision()
            || restore.restored_database_id() == backup.database_id()
            || restore.restored_revision() <= backup.revision()
        {
            return Err(MigrationRestorePointError::RestoreBindingMismatch);
        }

        let restored_layout = DatabaseLayout::open(restore.destination())
            .map_err(MigrationRestorePointError::StorageFile)?;
        if restored_layout.database_id() != Some(restore.restored_database_id()) {
            return Err(MigrationRestorePointError::RestoredLayoutMismatch);
        }
        let destination = fs::canonicalize(restore.destination()).map_err(|source| {
            MigrationRestorePointError::Io {
                operation: "canonicalize verified restore destination",
                source,
            }
        })?;
        let restore_destination_fingerprint = fingerprint_path(&destination);
        let verification_fingerprint =
            fingerprint_verification(plan, &backup, &restore, &destination);
        worlddb_core::storage_internal::migration_safe_restore_point_from_verified_restore(
            plan,
            source_database_id,
            backup.revision(),
            *backup.commit_hash().as_bytes(),
            *backup.inventory_digest(),
            *backup.manifest_digest(),
            restore.source_database_id(),
            restore.source_revision(),
            restore.restored_database_id(),
            restore.restored_revision(),
            restore_destination_fingerprint,
            verification_fingerprint,
        )
        .map_err(MigrationRestorePointError::Proof)
    }

    /// Re-verifies an existing exact source backup and restores a fresh clone for crash resume.
    ///
    /// A partially committed Breaking migration no longer has the plan's original source at the
    /// live project head. This method uses the retained exact backup to establish that source
    /// binding again, then performs and verifies a new real clone restore. The new clone target
    /// must not already exist.
    pub fn create_migration_safe_restore_point_from_backup(
        &self,
        plan: &MigrationPlan,
        backup_source: impl AsRef<Path>,
        restore_destination: impl AsRef<Path>,
        key: Option<&BackupMacKey>,
        policy: SecurityPolicyView<'_>,
        policy_target: PolicyTarget,
    ) -> Result<MigrationSafeRestorePoint, MigrationRestorePointError> {
        if plan.category() != MigrationCategory::Breaking {
            return Err(MigrationRestorePointError::PlanNotBreaking);
        }
        for capability in [
            Capability::MigrationExecute,
            Capability::BackupCreate,
            Capability::BackupRestore,
        ] {
            if policy
                .current_snapshot()
                .authorize(policy.principal_id(), capability, policy_target)
                != AuthorizationDecision::Allow
            {
                return Err(MigrationRestorePointError::AuthorizationDenied { capability });
            }
        }

        let backup = super::verify_exact_backup(backup_source.as_ref(), key)
            .map_err(MigrationRestorePointError::Backup)?;
        let source_database_id =
            self.source
                .database_id()
                .ok_or(MigrationRestorePointError::Backup(
                    super::BackupError::DatabaseIdentityMissing,
                ))?;
        if backup.profile() != super::BackupProfile::ExactDatabase
            || backup.database_id() != source_database_id
            || backup.revision() != plan.source_schema_precondition().revision().revision()
            || !backup.storage_report().is_clean()
        {
            return Err(MigrationRestorePointError::SourceBindingMismatch);
        }

        let policy_fingerprint = policy
            .current_snapshot()
            .effective_capability_fingerprint(policy.principal_id(), policy_target);
        let audit_policy_fingerprint =
            AuditPolicyFingerprint::new(Bytes::new(policy_fingerprint.to_vec()))
                .map_err(MigrationRestorePointError::AuditFingerprint)?;
        let restore = RestoreManager::new()
            .restore_clone(
                backup_source.as_ref(),
                restore_destination.as_ref(),
                key,
                policy,
                policy_target,
                audit_policy_fingerprint,
            )
            .map_err(MigrationRestorePointError::Restore)?;
        if restore.profile() != super::BackupProfile::ExactDatabase
            || restore.source_database_id() != backup.database_id()
            || restore.source_revision() != backup.revision()
            || restore.restored_database_id() == backup.database_id()
            || restore.restored_revision() <= backup.revision()
        {
            return Err(MigrationRestorePointError::RestoreBindingMismatch);
        }

        let restored_layout = DatabaseLayout::open(restore.destination())
            .map_err(MigrationRestorePointError::StorageFile)?;
        if restored_layout.database_id() != Some(restore.restored_database_id()) {
            return Err(MigrationRestorePointError::RestoredLayoutMismatch);
        }
        let destination = fs::canonicalize(restore.destination()).map_err(|source| {
            MigrationRestorePointError::Io {
                operation: "canonicalize verified resume restore destination",
                source,
            }
        })?;
        let restore_destination_fingerprint = fingerprint_path(&destination);
        let verification_fingerprint =
            fingerprint_verification(plan, &backup, &restore, &destination);
        worlddb_core::storage_internal::migration_safe_restore_point_from_verified_restore(
            plan,
            backup.database_id(),
            backup.revision(),
            *backup.commit_hash().as_bytes(),
            *backup.inventory_digest(),
            *backup.manifest_digest(),
            restore.source_database_id(),
            restore.source_revision(),
            restore.restored_database_id(),
            restore.restored_revision(),
            restore_destination_fingerprint,
            verification_fingerprint,
        )
        .map_err(MigrationRestorePointError::Proof)
    }
}

fn fingerprint_path(path: &Path) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.MigrationRestoreDestination.v1\0");
    hasher.update(path.to_string_lossy().as_bytes());
    *hasher.finalize().as_bytes()
}

fn fingerprint_verification(
    plan: &MigrationPlan,
    backup: &super::BackupVerification,
    restore: &super::RestoreReport,
    destination: &Path,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"WorldDB.MigrationSafeRestoreVerification.v1\0");
    hasher.update(plan.fingerprint().as_bytes());
    hasher.update(&backup.database_id().to_bytes());
    hasher.update(&backup.revision().value().to_be_bytes());
    hasher.update(backup.commit_hash().as_bytes());
    hasher.update(backup.inventory_digest());
    hasher.update(backup.manifest_digest());
    hasher.update(&[u8::from(backup.storage_report().is_clean())]);
    hasher.update(
        &backup
            .storage_report()
            .safe_revision()
            .value()
            .to_be_bytes(),
    );
    hasher.update(&restore.source_database_id().to_bytes());
    hasher.update(&restore.source_revision().value().to_be_bytes());
    hasher.update(&restore.restored_database_id().to_bytes());
    hasher.update(&restore.restored_revision().value().to_be_bytes());
    hasher.update(&restore.operation_id().to_bytes());
    hasher.update(&restore.audit_record_id().to_bytes());
    hasher.update(fingerprint_path(destination).as_slice());
    *hasher.finalize().as_bytes()
}
