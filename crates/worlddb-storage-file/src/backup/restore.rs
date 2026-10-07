//! Verified clone restoration with an atomic destination publication boundary.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence, AuthorizationDecision,
    Capability, DatabaseId, DomainId, OperationId, PolicyTarget, Revision, SecurityPolicyView,
};

use super::{
    BACKUP_INCOMPLETE_FILE, BACKUP_MANIFEST_FILE, BackupAuthenticity, BackupError, BackupManifest,
    BackupProfile, CopyInput, INCOMPLETE_MARKER, SourceItem, copy_source_item, normalize_target,
    read_bounded_file, verify_audit_backup, verify_exact_backup_internal, write_new_file,
};
use crate::manifest::sync_directory;
use crate::{
    DatabaseLayout, ManifestError, ManifestStore, RecoveryError, RecoveryManager,
    RequiredAuditError, StorageFileError, StorageVerifier, StorageVerifyError, WalError,
    WalPrepareLog, WriterLockError,
};

const RESTORE_STAGE_ATTEMPTS: u8 = 32;
const RESTORE_ID_TEMP_ATTEMPTS: u8 = 16;

static NEXT_RESTORE_STAGE: AtomicU64 = AtomicU64::new(0);
static NEXT_RESTORE_ID_TEMP: AtomicU64 = AtomicU64::new(0);

/// Failure while authorizing, preparing, verifying, or publishing a clone restore.
#[derive(Debug)]
pub enum RestoreError {
    /// The source backup failed independent verification or safe copying.
    Backup(BackupError),
    /// One current capability required by the selected backup profile is denied.
    AuthorizationDenied { capability: Capability },
    /// The database layout could not be opened or prepared.
    StorageFile(StorageFileError),
    /// A filesystem operation failed before or during publication.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// The target directory already exists; restore never overwrites a target.
    TargetAlreadyExists,
    /// The restore staging directory name could not be allocated.
    StagingNameExhausted,
    /// A copied file did not match its authenticated source inventory entry.
    CopyInventoryMismatch,
    /// The source backup and its staged copy do not describe the same snapshot.
    SourceSnapshotMismatch,
    /// The cloned database identity could not be generated.
    Identity(worlddb_core::IdGenerationError),
    /// The identity generator repeatedly returned the source database identity.
    CloneIdentityCollision,
    /// The restore publication writer lock could not be acquired.
    WriterLock(WriterLockError),
    /// The restore commit could not be read or published to the data WAL.
    Wal(WalError),
    /// The restore Required Audit Record could not be validated or read.
    RequiredAudit(RequiredAuditError),
    /// The manifest snapshot and Required Audit Record could not be committed together.
    SnapshotCommit(crate::SnapshotCommitError),
    /// Crash recovery could not safely replay the restore publication commit.
    Recovery(RecoveryError),
    /// Independent verification did not establish a clean restored database.
    StorageVerify(StorageVerifyError),
    /// The current manifest could not be read while preparing the snapshot commit.
    Manifest(ManifestError),
    /// A Required Audit sequence could not be advanced.
    AuditSequence(worlddb_core::AuditSequenceError),
    /// The copied audit namespace no longer matches its independently verified lineage.
    AuditLineageMismatch,
    /// A fault was injected before destination publication.
    InjectedBeforePublish,
    /// Publication succeeded but its durability or final verification is uncertain.
    PublishedOutcomeUnknown {
        target: PathBuf,
        operation_id: OperationId,
        source: Option<io::Error>,
    },
}

impl fmt::Display for RestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backup(error) => write!(formatter, "restore backup verification failed: {error}"),
            Self::AuthorizationDenied { capability } => {
                write!(
                    formatter,
                    "restore requires current {capability:?} permission"
                )
            }
            Self::StorageFile(error) => write!(formatter, "restore layout failed: {error}"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::TargetAlreadyExists => {
                formatter.write_str("restore destination already exists and was left unchanged")
            }
            Self::StagingNameExhausted => {
                formatter.write_str("restore could not allocate a unique staging directory")
            }
            Self::CopyInventoryMismatch => {
                formatter.write_str("restored files did not match the verified backup inventory")
            }
            Self::SourceSnapshotMismatch => {
                formatter.write_str("staged restore differs from the verified source backup")
            }
            Self::Identity(error) => write!(formatter, "restore clone identity failed: {error}"),
            Self::CloneIdentityCollision => {
                formatter.write_str("restore clone identity matched the source database identity")
            }
            Self::WriterLock(error) => write!(formatter, "restore writer lock failed: {error}"),
            Self::Wal(error) => write!(formatter, "restore data-WAL operation failed: {error}"),
            Self::RequiredAudit(error) => {
                write!(
                    formatter,
                    "restore required-audit operation failed: {error}"
                )
            }
            Self::SnapshotCommit(error) => {
                write!(formatter, "restore snapshot commit failed: {error}")
            }
            Self::Recovery(error) => write!(formatter, "restore recovery failed: {error}"),
            Self::StorageVerify(error) => {
                write!(formatter, "restored storage verification failed: {error}")
            }
            Self::Manifest(error) => {
                write!(formatter, "restore manifest operation failed: {error}")
            }
            Self::AuditSequence(error) => {
                write!(formatter, "restore audit sequence failed: {error}")
            }
            Self::AuditLineageMismatch => formatter
                .write_str("restored audit namespace failed independent lineage verification"),
            Self::InjectedBeforePublish => {
                formatter.write_str("restore fault injected before publication")
            }
            Self::PublishedOutcomeUnknown {
                target,
                operation_id,
                source,
            } => {
                write!(
                    formatter,
                    "restore publication for operation {operation_id} is visible at {} but its final outcome is uncertain",
                    target.display()
                )?;
                if let Some(source) = source {
                    write!(formatter, ": {source}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for RestoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backup(error) => Some(error),
            Self::StorageFile(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Identity(error) => Some(error),
            Self::WriterLock(error) => Some(error),
            Self::Wal(error) => Some(error),
            Self::RequiredAudit(error) => Some(error),
            Self::SnapshotCommit(error) => Some(error),
            Self::Recovery(error) => Some(error),
            Self::StorageVerify(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::AuditSequence(error) => Some(error),
            Self::PublishedOutcomeUnknown { source, .. } => source
                .as_ref()
                .map(|error| error as &(dyn std::error::Error + 'static)),
            Self::AuthorizationDenied { .. }
            | Self::TargetAlreadyExists
            | Self::StagingNameExhausted
            | Self::CopyInventoryMismatch
            | Self::SourceSnapshotMismatch
            | Self::CloneIdentityCollision
            | Self::AuditLineageMismatch
            | Self::InjectedBeforePublish => None,
        }
    }
}

/// Result of a successfully verified clone restore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreReport {
    destination: PathBuf,
    source_database_id: DatabaseId,
    restored_database_id: DatabaseId,
    source_revision: Revision,
    restored_revision: Revision,
    profile: BackupProfile,
    source_authenticity: BackupAuthenticity,
    audit_safe_sequence: Option<AuditSequence>,
    operation_id: OperationId,
    audit_record_id: worlddb_core::AuditRecordId,
}

impl RestoreReport {
    /// Published clone directory.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Database identity named by the source backup.
    #[must_use]
    pub const fn source_database_id(&self) -> DatabaseId {
        self.source_database_id
    }

    /// Newly generated database identity of this clone.
    #[must_use]
    pub const fn restored_database_id(&self) -> DatabaseId {
        self.restored_database_id
    }

    /// Data revision verified in the source backup.
    #[must_use]
    pub const fn source_revision(&self) -> Revision {
        self.source_revision
    }

    /// Revision containing the atomic restore-publication action and audit record.
    #[must_use]
    pub const fn restored_revision(&self) -> Revision {
        self.restored_revision
    }

    /// Profile independently verified in the source backup.
    #[must_use]
    pub const fn profile(&self) -> BackupProfile {
        self.profile
    }

    /// Backup authenticity result, reported separately from byte integrity.
    #[must_use]
    pub const fn source_authenticity(&self) -> &BackupAuthenticity {
        &self.source_authenticity
    }

    /// Independent raw-read audit watermark, when present in an AuditComplete backup.
    #[must_use]
    pub const fn audit_safe_sequence(&self) -> Option<AuditSequence> {
        self.audit_safe_sequence
    }

    /// WAL operation identity for RestorePublication.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Required Audit Record identity committed with RestorePublication.
    #[must_use]
    pub const fn audit_record_id(&self) -> worlddb_core::AuditRecordId {
        self.audit_record_id
    }
}

/// Creates verified clone restores into previously unused destination paths.
#[derive(Clone, Copy, Debug, Default)]
pub struct RestoreManager;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RestoreCheckpoint {
    BeforePublish,
    AfterPublish,
}

struct RestoreRequest<'a> {
    backup_root: &'a Path,
    destination: &'a Path,
    key: Option<&'a super::BackupMacKey>,
    policy: SecurityPolicyView<'a>,
    policy_target: PolicyTarget,
    policy_fingerprint: AuditPolicyFingerprint,
}

impl RestoreManager {
    /// Creates a stateless restore manager.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Restores a verified backup as a new database identity and publishes it atomically.
    ///
    /// Same-identity disaster recovery is intentionally not exposed here: this storage layer
    /// cannot enforce the required exclusivity between the original database and its recovery.
    pub fn restore_clone(
        &self,
        backup_root: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        key: Option<&super::BackupMacKey>,
        policy: SecurityPolicyView<'_>,
        policy_target: PolicyTarget,
        policy_fingerprint: AuditPolicyFingerprint,
    ) -> Result<RestoreReport, RestoreError> {
        self.restore_clone_with_checkpoint(
            RestoreRequest {
                backup_root: backup_root.as_ref(),
                destination: destination.as_ref(),
                key,
                policy,
                policy_target,
                policy_fingerprint,
            },
            |_| false,
        )
    }

    fn restore_clone_with_checkpoint(
        &self,
        request: RestoreRequest<'_>,
        mut checkpoint: impl FnMut(RestoreCheckpoint) -> bool,
    ) -> Result<RestoreReport, RestoreError> {
        let RestoreRequest {
            backup_root,
            destination,
            key,
            policy,
            policy_target,
            policy_fingerprint,
        } = request;
        authorize(policy, Capability::BackupRestore, policy_target)?;

        let source_root = fs::canonicalize(backup_root).map_err(|source| RestoreError::Io {
            operation: "resolve restore backup directory",
            source,
        })?;
        if !source_root.is_dir() {
            return Err(RestoreError::Backup(BackupError::InvalidTarget));
        }
        let backup_manifest_path = source_root.join(BACKUP_MANIFEST_FILE);
        let backup_manifest_bytes = read_bounded_file(
            &backup_manifest_path,
            super::BACKUP_MANIFEST_LIMIT,
            &source_root,
        )
        .map_err(RestoreError::Backup)?;
        let backup_manifest =
            BackupManifest::decode(&backup_manifest_bytes).map_err(RestoreError::Backup)?;

        if backup_manifest.metadata.profile == BackupProfile::AuditComplete {
            authorize(policy, Capability::AuditRead, policy_target)?;
            authorize(policy, Capability::AuditExport, policy_target)?;
        }

        let source_verification =
            verify_exact_backup_internal(&source_root, key, false).map_err(RestoreError::Backup)?;
        if source_verification.profile() != backup_manifest.metadata.profile
            || source_verification.database_id() != backup_manifest.metadata.database_id
            || source_verification.revision() != backup_manifest.metadata.revision
            || source_verification.manifest_digest() != &backup_manifest.manifest_digest
            || source_verification.inventory_digest() != &backup_manifest.inventory_digest
        {
            return Err(RestoreError::SourceSnapshotMismatch);
        }
        if key.is_some()
            && matches!(
                source_verification.authenticity(),
                BackupAuthenticity::WrongKey { .. } | BackupAuthenticity::InvalidMac { .. }
            )
        {
            return Err(RestoreError::Backup(BackupError::IntegrityMismatch));
        }

        let target = normalize_target(destination, &source_root).map_err(|error| match error {
            BackupError::TargetAlreadyExists => RestoreError::TargetAlreadyExists,
            other => RestoreError::Backup(other),
        })?;
        match fs::symlink_metadata(&target) {
            Ok(_) => return Err(RestoreError::TargetAlreadyExists),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(RestoreError::Io {
                    operation: "inspect restore destination",
                    source,
                });
            }
        }
        let stage_path = create_stage_directory(&target)?;
        let mut stage_guard = RestoreStageGuard::new(stage_path.clone());
        let stage_root = DatabaseLayout::prepare_restore_staging_root(&stage_path)
            .map_err(RestoreError::StorageFile)?;

        let mut copied_items = Vec::new();
        copied_items
            .try_reserve_exact(backup_manifest.items.len())
            .map_err(|_| RestoreError::Backup(BackupError::AllocationFailed))?;
        for item in &backup_manifest.items {
            let source_path =
                super::join_item_path(&source_root, &item.path).map_err(RestoreError::Backup)?;
            let source = SourceItem {
                path: item.path.clone(),
                length: item.length,
                input: CopyInput::File(source_path),
            };
            let copied = copy_source_item(&source, &source_root, &stage_root)
                .map_err(RestoreError::Backup)?;
            if copied != *item {
                return Err(RestoreError::CopyInventoryMismatch);
            }
            copied_items.push(copied);
        }
        if copied_items.as_slice() != backup_manifest.items.as_slice() {
            return Err(RestoreError::CopyInventoryMismatch);
        }

        write_new_file(
            &stage_root.join(BACKUP_MANIFEST_FILE),
            &backup_manifest_bytes,
            "write staged restore verification manifest",
        )
        .map_err(RestoreError::Backup)?;
        write_new_file(
            &stage_root.join(BACKUP_INCOMPLETE_FILE),
            INCOMPLETE_MARKER,
            "write staged restore incomplete marker",
        )
        .map_err(RestoreError::Backup)?;
        sync_directory_tree(&stage_root)?;

        let staged_verification =
            verify_exact_backup_internal(&stage_root, key, true).map_err(RestoreError::Backup)?;
        if staged_verification.manifest_digest() != source_verification.manifest_digest()
            || staged_verification.inventory_digest() != source_verification.inventory_digest()
            || staged_verification.database_id() != source_verification.database_id()
            || staged_verification.profile() != source_verification.profile()
            || staged_verification.revision() != source_verification.revision()
        {
            return Err(RestoreError::SourceSnapshotMismatch);
        }
        if verify_audit_backup(&stage_root, &backup_manifest).map_err(RestoreError::Backup)?
            != source_verification.audit_safe_sequence()
        {
            return Err(RestoreError::AuditLineageMismatch);
        }

        remove_file(
            &stage_root.join(BACKUP_MANIFEST_FILE),
            "remove staged backup manifest",
        )?;
        remove_file(
            &stage_root.join(BACKUP_INCOMPLETE_FILE),
            "remove staged incomplete marker",
        )?;

        let mut restored_database_id = None;
        for _ in 0..RESTORE_ID_TEMP_ATTEMPTS {
            let generated = worlddb_core::storage_internal::generate_restore_clone_database_id()
                .map_err(RestoreError::Identity)?;
            if generated != source_verification.database_id() {
                restored_database_id = Some(generated);
                break;
            }
        }
        let restored_database_id =
            restored_database_id.ok_or(RestoreError::CloneIdentityCollision)?;
        replace_database_id(&stage_root, restored_database_id)?;
        sync_directory_tree(&stage_root)?;
        if verify_audit_backup(&stage_root, &backup_manifest).map_err(RestoreError::Backup)?
            != source_verification.audit_safe_sequence()
        {
            return Err(RestoreError::AuditLineageMismatch);
        }

        let stage_layout = DatabaseLayout::open(&stage_root).map_err(RestoreError::StorageFile)?;
        if stage_layout.database_id() != Some(restored_database_id) {
            return Err(RestoreError::SourceSnapshotMismatch);
        }
        let lock = stage_layout
            .try_writer_lock()
            .map_err(RestoreError::WriterLock)?;
        RecoveryManager::new(stage_layout.clone())
            .recover(&lock)
            .map_err(RestoreError::Recovery)?;

        let wal = WalPrepareLog::new(&stage_layout);
        let current = ManifestStore::new(stage_layout.clone())
            .read_current()
            .map_err(RestoreError::Manifest)?;
        let segments = current
            .as_ref()
            .map_or_else(Vec::new, |manifest| manifest.segments().to_vec());
        let next_revision = wal
            .commit_head(&lock)
            .map_err(RestoreError::Wal)?
            .revision()
            .next_commit()
            .map_err(|error| RestoreError::Wal(WalError::Revision(error)))?;
        let operation_id =
            worlddb_core::storage_internal::generate_restore_publication_operation_id()
                .map_err(RestoreError::Identity)?;
        let committed_audits = wal
            .committed_required_audit_records(&lock)
            .map_err(RestoreError::RequiredAudit)?;
        let previous_sequence = committed_audits
            .last()
            .map_or(AuditSequence::new(0), |entry| entry.record().sequence());
        let audit_sequence = previous_sequence
            .next()
            .map_err(RestoreError::AuditSequence)?;
        let audit_record = AuditRecord::new(
            AuditRecordIdentity {
                record_id:
                    worlddb_core::storage_internal::generate_restore_publication_audit_record_id()
                        .map_err(RestoreError::Identity)?,
                sequence: audit_sequence,
                audit_operation_id:
                    worlddb_core::storage_internal::generate_restore_publication_audit_operation_id(
                    )
                    .map_err(RestoreError::Identity)?,
            },
            AuditRecordDetails {
                actor: policy.principal_id(),
                action: AuditAction::RestorePublication,
                object_class: AuditObjectClass::Database,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: next_revision,
                    operation_id,
                },
                security_epoch: policy.current_epoch(),
                policy_fingerprint,
            },
        );

        let receipt = wal
            .commit_audited_manifest_snapshot(&lock, operation_id, segments, &[], &audit_record)
            .map_err(snapshot_commit_error)?;
        if receipt.revision() != next_revision {
            return Err(RestoreError::SourceSnapshotMismatch);
        }
        RecoveryManager::new(stage_layout.clone())
            .recover(&lock)
            .map_err(RestoreError::Recovery)?;
        verify_committed_restore(
            &stage_layout,
            &lock,
            receipt.revision(),
            operation_id,
            &audit_record,
        )?;
        if verify_audit_backup(&stage_root, &backup_manifest).map_err(RestoreError::Backup)?
            != source_verification.audit_safe_sequence()
        {
            return Err(RestoreError::AuditLineageMismatch);
        }
        drop(lock);

        if checkpoint(RestoreCheckpoint::BeforePublish) {
            return Err(RestoreError::InjectedBeforePublish);
        }
        if let Err(error) = publish_directory(&stage_root, &target) {
            if error.kind() == io::ErrorKind::AlreadyExists {
                return Err(RestoreError::TargetAlreadyExists);
            }
            return Err(RestoreError::Io {
                operation: "atomically publish restored database directory",
                source: error,
            });
        }
        stage_guard.mark_published();
        if let Err(source) = sync_directory(target.parent().unwrap_or_else(|| Path::new("."))) {
            return Err(RestoreError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: Some(source),
            });
        }
        if checkpoint(RestoreCheckpoint::AfterPublish) {
            return Err(RestoreError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: None,
            });
        }

        let published_layout = DatabaseLayout::open(&target).map_err(|error| {
            RestoreError::PublishedOutcomeUnknown {
                target: target.clone(),
                operation_id,
                source: Some(io::Error::other(error.to_string())),
            }
        })?;
        if published_layout.database_id() != Some(restored_database_id) {
            return Err(RestoreError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: None,
            });
        }
        let published_lock = published_layout.try_writer_lock().map_err(|error| {
            RestoreError::PublishedOutcomeUnknown {
                target: target.clone(),
                operation_id,
                source: Some(io::Error::other(error.to_string())),
            }
        })?;
        verify_committed_restore(
            &published_layout,
            &published_lock,
            receipt.revision(),
            operation_id,
            &audit_record,
        )
        .map_err(|error| RestoreError::PublishedOutcomeUnknown {
            target: target.clone(),
            operation_id,
            source: Some(io::Error::other(error.to_string())),
        })?;
        if verify_audit_backup(&target, &backup_manifest).map_err(|error| {
            RestoreError::PublishedOutcomeUnknown {
                target: target.clone(),
                operation_id,
                source: Some(io::Error::other(error.to_string())),
            }
        })? != source_verification.audit_safe_sequence()
        {
            return Err(RestoreError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: None,
            });
        }
        drop(published_lock);

        Ok(RestoreReport {
            destination: target,
            source_database_id: source_verification.database_id(),
            restored_database_id,
            source_revision: source_verification.revision(),
            restored_revision: receipt.revision(),
            profile: source_verification.profile(),
            source_authenticity: source_verification.authenticity().clone(),
            audit_safe_sequence: source_verification.audit_safe_sequence(),
            operation_id,
            audit_record_id: audit_record.record_id(),
        })
    }
}

fn authorize(
    policy: SecurityPolicyView<'_>,
    capability: Capability,
    target: PolicyTarget,
) -> Result<(), RestoreError> {
    if policy
        .current_snapshot()
        .authorize(policy.principal_id(), capability, target)
        == AuthorizationDecision::Allow
    {
        Ok(())
    } else {
        Err(RestoreError::AuthorizationDenied { capability })
    }
}

fn snapshot_commit_error(error: crate::SnapshotCommitError) -> RestoreError {
    RestoreError::SnapshotCommit(error)
}

fn verify_committed_restore(
    layout: &DatabaseLayout,
    lock: &crate::WriterLock,
    expected_revision: Revision,
    operation_id: OperationId,
    expected_record: &AuditRecord,
) -> Result<(), RestoreError> {
    let report = StorageVerifier::new(layout.clone())
        .verify(lock)
        .map_err(RestoreError::StorageVerify)?;
    if !report.is_clean() || report.safe_revision() != expected_revision {
        return Err(RestoreError::SourceSnapshotMismatch);
    }
    let audits = WalPrepareLog::new(layout)
        .committed_required_audit_records(lock)
        .map_err(RestoreError::RequiredAudit)?;
    if !audits.iter().any(|entry| {
        entry.revision() == expected_revision
            && entry.operation_id() == operation_id
            && entry.record() == expected_record
    }) {
        return Err(RestoreError::SourceSnapshotMismatch);
    }
    Ok(())
}

fn create_stage_directory(target: &Path) -> Result<PathBuf, RestoreError> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..RESTORE_STAGE_ATTEMPTS {
        let sequence = NEXT_RESTORE_STAGE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".worlddb-restore-stage-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(RestoreError::Io {
                    operation: "create restore staging directory",
                    source,
                });
            }
        }
    }
    Err(RestoreError::StagingNameExhausted)
}

struct RestoreStageGuard {
    path: PathBuf,
    published: bool,
}

impl RestoreStageGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            published: false,
        }
    }

    fn mark_published(&mut self) {
        self.published = true;
    }
}

impl Drop for RestoreStageGuard {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn remove_file(path: &Path, operation: &'static str) -> Result<(), RestoreError> {
    fs::remove_file(path).map_err(|source| RestoreError::Io { operation, source })
}

fn replace_database_id(root: &Path, database_id: DatabaseId) -> Result<(), RestoreError> {
    let target = root.join("DATABASE_ID");
    for _ in 0..RESTORE_ID_TEMP_ATTEMPTS {
        let sequence = NEXT_RESTORE_ID_TEMP.fetch_add(1, Ordering::Relaxed);
        let stage = root.join(format!(
            ".DATABASE_ID-restore-{}-{sequence}",
            std::process::id()
        ));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&stage) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(RestoreError::Io {
                    operation: "create staged clone database identity",
                    source,
                });
            }
        };
        if let Err(source) = file
            .write_all(&database_id.to_bytes())
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = fs::remove_file(&stage);
            return Err(RestoreError::Io {
                operation: "write staged clone database identity",
                source,
            });
        }
        drop(file);
        let publication = replace_file(&stage, &target);
        if let Err(source) = publication {
            let _ = fs::remove_file(&stage);
            return Err(RestoreError::Io {
                operation: "publish clone database identity",
                source,
            });
        }
        sync_directory(root).map_err(|source| RestoreError::Io {
            operation: "sync clone database identity directory",
            source,
        })?;
        return Ok(());
    }
    Err(RestoreError::Io {
        operation: "allocate staged clone database identity",
        source: io::Error::new(io::ErrorKind::AlreadyExists, "staging names exhausted"),
    })
}

fn sync_directory_tree(root: &Path) -> Result<(), RestoreError> {
    let mut directories = vec![root.to_path_buf()];
    let mut index = 0;
    while index < directories.len() {
        let current = directories
            .get(index)
            .ok_or(RestoreError::CopyInventoryMismatch)?
            .clone();
        index += 1;
        for entry in fs::read_dir(&current).map_err(|source| RestoreError::Io {
            operation: "list restore staging directory",
            source,
        })? {
            let entry = entry.map_err(|source| RestoreError::Io {
                operation: "read restore staging directory entry",
                source,
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| RestoreError::Io {
                operation: "inspect restore staging entry",
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(RestoreError::CopyInventoryMismatch);
            }
            if metadata.is_dir() {
                directories.push(path);
            } else if !metadata.is_file() {
                return Err(RestoreError::CopyInventoryMismatch);
            }
        }
    }
    for directory in directories.into_iter().rev() {
        sync_directory(&directory).map_err(|source| RestoreError::Io {
            operation: "sync restore staging directory",
            source,
        })?;
    }
    Ok(())
}

#[cfg(windows)]
fn publish_directory(stage: &Path, target: &Path) -> io::Result<()> {
    crate::windows_publication::move_directory(stage, target)
}

#[cfg(not(windows))]
fn publish_directory(stage: &Path, target: &Path) -> io::Result<()> {
    fs::rename(stage, target)
}

pub(crate) fn publish_restore_directory(stage: &Path, target: &Path) -> io::Result<()> {
    publish_directory(stage, target)
}

#[cfg(windows)]
fn replace_file(stage: &Path, target: &Path) -> io::Result<()> {
    crate::windows_publication::move_file(stage, target, true)
}

#[cfg(not(windows))]
fn replace_file(stage: &Path, target: &Path) -> io::Result<()> {
    fs::rename(stage, target)
}

#[cfg(test)]
mod tests {
    use super::{BackupProfile, RestoreCheckpoint, RestoreError, RestoreManager, RestoreRequest};
    use crate::{
        DatabaseLayout, ExactBackupManager, RawReadAuditWal, RawReadAuditWriter, RecoveryManager,
        StorageVerifier, WalPrepareLog,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};
    use worlddb_core::{
        AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
        AuditScopeFingerprint, AuthorizationMode, Bytes, Capability, CapabilityGrant,
        CapabilityRule, ClientRequestId, DomainId, GrantEffect, PageOrdinal, PolicyRuleId,
        PolicyScope, PolicySubject, Principal, PrincipalId, RawReadAttemptScope, Revision,
        SecurityEpoch, SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion,
        SnapshotId,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    const CRASH_EXIT_CODE: i32 = 86;
    const BACKUP_ENV: &str = "WORLDDB_M7_16B_RESTORE_BACKUP";
    const TARGET_ENV: &str = "WORLDDB_M7_16B_RESTORE_TARGET";
    const CHECKPOINT_ENV: &str = "WORLDDB_M7_16B_RESTORE_CHECKPOINT";
    const CRASH_TEST_NAME: &str = "backup::restore::tests::process_crash_before_or_after_atomic_publication_leaves_only_absent_or_verified_audited_target";

    fn checkpoint_name(checkpoint: RestoreCheckpoint) -> &'static str {
        match checkpoint {
            RestoreCheckpoint::BeforePublish => "before_publish",
            RestoreCheckpoint::AfterPublish => "after_publish",
        }
    }

    fn restore_child(
        executable: &std::path::Path,
        backup: &std::path::Path,
        target: &std::path::Path,
        checkpoint: &str,
    ) -> Result<Output, String> {
        crate::writer_lock::test_command_output(
            Command::new(executable)
                .args(["--exact", CRASH_TEST_NAME, "--nocapture"])
                .env(BACKUP_ENV, backup)
                .env(TARGET_ENV, target)
                .env(CHECKPOINT_ENV, checkpoint),
        )
        .map_err(|error| format!("spawn restore child for {checkpoint}: {error}"))
    }

    fn output_text(output: &Output) -> String {
        format!(
            "stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    struct TempArea(PathBuf);

    impl TempArea {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "worlddb-m7-10-fault-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).map_err(|error| error.to_string())?;
            Ok(Self(root))
        }
    }

    impl Drop for TempArea {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn restore_policy() -> Result<SecurityPolicyHistory, String> {
        let principal = id::<PrincipalId>(1)?;
        let rules = [
            (2, Capability::ProjectRead),
            (3, Capability::BackupCreate),
            (4, Capability::BackupRestore),
            (5, Capability::AuditRead),
            (6, Capability::AuditExport),
        ]
        .into_iter()
        .map(|(tail, capability)| -> Result<_, String> {
            Ok(CapabilityRule::new(
                id::<PolicyRuleId>(tail)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
        let snapshot =
            SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
                .map_err(|error| error.to_string())?;
        SecurityPolicyHistory::new(
            Revision::GENESIS,
            vec![SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                snapshot,
            )],
        )
        .map_err(|error| error.to_string())
    }

    fn append_audit_attempt(writer: &RawReadAuditWriter) -> Result<(), String> {
        writer
            .append_attempt(
                RawReadAttemptScope {
                    principal_id: id::<PrincipalId>(1)?,
                    scope_fingerprint: AuditScopeFingerprint::new(Bytes::new(vec![0x55, 0xaa]))
                        .map_err(|error| error.to_string())?,
                    snapshot_id: id::<SnapshotId>(2)?,
                    security_epoch: SecurityEpoch::INITIAL,
                    page_ordinal: PageOrdinal::new(1),
                },
                id::<ClientRequestId>(3)?,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn fingerprint() -> Result<AuditPolicyFingerprint, String> {
        AuditPolicyFingerprint::new(Bytes::new(vec![0x4d, 0x37, 0x10]))
            .map_err(|error| error.to_string())
    }

    #[test]
    fn publication_faults_leave_either_no_target_or_a_verified_audited_target() -> Result<(), String>
    {
        let area = TempArea::create()?;
        let source =
            DatabaseLayout::create(area.0.join("source")).map_err(|error| error.to_string())?;
        let backup = area.0.join("backup");
        ExactBackupManager::new(source)
            .create_exact_backup(&backup, None)
            .map_err(|error| error.to_string())?;
        let permissions = restore_policy()?;
        let policy = permissions
            .select(
                AuthorizationMode::Now,
                id::<PrincipalId>(1)?,
                Revision::GENESIS,
            )
            .map_err(|error| error.to_string())?;

        let before_target = area.0.join("before-publish");
        let before = RestoreManager::new().restore_clone_with_checkpoint(
            RestoreRequest {
                backup_root: &backup,
                destination: &before_target,
                key: None,
                policy,
                policy_target: worlddb_core::PolicyTarget::default(),
                policy_fingerprint: fingerprint()?,
            },
            |checkpoint| checkpoint == RestoreCheckpoint::BeforePublish,
        );
        assert!(matches!(before, Err(RestoreError::InjectedBeforePublish)));
        assert!(!before_target.exists());
        assert!(
            fs::read_dir(&area.0)
                .map_err(|error| error.to_string())?
                .all(|entry| entry.ok().is_none_or(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".worlddb-restore-stage-")))
        );

        let after_target = area.0.join("after-publish");
        let after = RestoreManager::new().restore_clone_with_checkpoint(
            RestoreRequest {
                backup_root: &backup,
                destination: &after_target,
                key: None,
                policy,
                policy_target: worlddb_core::PolicyTarget::default(),
                policy_fingerprint: fingerprint()?,
            },
            |checkpoint| checkpoint == RestoreCheckpoint::AfterPublish,
        );
        let (operation_id, published_target) = match after {
            Err(RestoreError::PublishedOutcomeUnknown {
                target,
                operation_id,
                source: None,
            }) => (operation_id, target),
            other => return Err(format!("unexpected post-publish fault result: {other:?}")),
        };
        assert_eq!(
            published_target,
            fs::canonicalize(&after_target).map_err(|error| error.to_string())?
        );
        let restored = DatabaseLayout::open(&after_target).map_err(|error| error.to_string())?;
        let lock = restored
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(restored.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        let report = StorageVerifier::new(restored.clone())
            .verify(&lock)
            .map_err(|error| error.to_string())?;
        assert!(report.is_clean(), "{report}");
        assert!(
            WalPrepareLog::new(&restored)
                .committed_required_audit_records(&lock)
                .map_err(|error| error.to_string())?
                .iter()
                .any(|entry| entry.operation_id() == operation_id)
        );
        assert!(!after_target.join("EXACT_BACKUP").exists());
        Ok(())
    }

    #[test]
    fn process_crash_before_or_after_atomic_publication_leaves_only_absent_or_verified_audited_target()
    -> Result<(), String> {
        if std::env::var_os(BACKUP_ENV).is_some()
            || std::env::var_os(TARGET_ENV).is_some()
            || std::env::var_os(CHECKPOINT_ENV).is_some()
        {
            let backup = std::env::var_os(BACKUP_ENV)
                .map(PathBuf::from)
                .ok_or_else(|| format!("child environment is missing {BACKUP_ENV}"))?;
            let target = std::env::var_os(TARGET_ENV)
                .map(PathBuf::from)
                .ok_or_else(|| format!("child environment is missing {TARGET_ENV}"))?;
            let wanted = std::env::var(CHECKPOINT_ENV).map_err(|error| {
                format!("child environment is missing {CHECKPOINT_ENV}: {error}")
            })?;
            let permissions = restore_policy()?;
            let policy = permissions
                .select(
                    AuthorizationMode::Now,
                    id::<PrincipalId>(1)?,
                    Revision::GENESIS,
                )
                .map_err(|error| error.to_string())?;
            let result = RestoreManager::new().restore_clone_with_checkpoint(
                RestoreRequest {
                    backup_root: &backup,
                    destination: &target,
                    key: None,
                    policy,
                    policy_target: worlddb_core::PolicyTarget::default(),
                    policy_fingerprint: fingerprint()?,
                },
                |checkpoint| {
                    if checkpoint_name(checkpoint) == wanted {
                        std::process::exit(CRASH_EXIT_CODE);
                    }
                    false
                },
            );
            if wanted == "existing_destination" {
                return match result {
                    Err(RestoreError::TargetAlreadyExists) => Ok(()),
                    other => Err(format!("existing target was not rejected: {other:?}")),
                };
            }
            return Err(format!(
                "restore child did not exit at {wanted}; restore returned {result:?}"
            ));
        }

        let area = TempArea::create()?;
        let source =
            DatabaseLayout::create(area.0.join("source")).map_err(|error| error.to_string())?;
        let source_id = source
            .database_id()
            .ok_or("source database identity missing")?;
        let exact_backup = area.0.join("backup-exact");
        ExactBackupManager::new(source.clone())
            .create_exact_backup(&exact_backup, None)
            .map_err(|error| error.to_string())?;
        let audit_writer = RawReadAuditWal::new(source.clone())
            .try_writer()
            .map_err(|error| error.to_string())?;
        append_audit_attempt(&audit_writer)?;
        let audit_permissions = restore_policy()?;
        let audit_policy = audit_permissions
            .select(
                AuthorizationMode::Now,
                id::<PrincipalId>(1)?,
                Revision::GENESIS,
            )
            .map_err(|error| error.to_string())?;
        let audit_complete_backup = area.0.join("backup-audit-complete");
        ExactBackupManager::new(source.clone())
            .create_audit_complete_backup(
                &audit_complete_backup,
                None,
                &audit_writer,
                audit_policy,
                worlddb_core::PolicyTarget::default(),
            )
            .map_err(|error| error.to_string())?;
        drop(audit_writer);
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;

        let existing_target = area.0.join("existing-target");
        fs::create_dir(&existing_target).map_err(|error| error.to_string())?;
        let sentinel = existing_target.join("keep.bin");
        fs::write(&sentinel, b"preserve-existing-destination")
            .map_err(|error| error.to_string())?;
        let existing = restore_child(
            &executable,
            &exact_backup,
            &existing_target,
            "existing_destination",
        )?;
        if !existing.status.success() {
            return Err(format!(
                "existing-target child exited with {:?}; {}",
                existing.status.code(),
                output_text(&existing)
            ));
        }
        assert_eq!(
            fs::read(&sentinel).map_err(|error| error.to_string())?,
            b"preserve-existing-destination"
        );
        assert_eq!(
            fs::read_dir(&existing_target)
                .map_err(|error| error.to_string())?
                .count(),
            1,
            "restore must not add files to an existing destination"
        );

        for (profile, backup) in [
            ("exact", exact_backup.as_path()),
            ("audit-complete", audit_complete_backup.as_path()),
        ] {
            let initial_backup_verification = super::super::verify_exact_backup(backup, None)
                .map_err(|error| error.to_string())?;
            for checkpoint in ["before_publish", "after_publish"] {
                let target = area.0.join(format!("crash-{profile}-{checkpoint}"));
                let output = restore_child(&executable, backup, &target, checkpoint)?;
                if output.status.code() != Some(CRASH_EXIT_CODE) {
                    return Err(format!(
                        "restore child for {profile} at {checkpoint} exited with {:?}, expected process exit {CRASH_EXIT_CODE}; {}",
                        output.status.code(),
                        output_text(&output)
                    ));
                }

                if checkpoint == "before_publish" {
                    assert!(
                        !target.exists(),
                        "destination became visible before atomic publication for {profile}"
                    );
                } else {
                    assert!(
                        target.is_dir(),
                        "published {profile} destination disappeared after crash"
                    );
                    assert!(!target.join("EXACT_BACKUP").exists());
                    assert!(!target.join("EXACT_BACKUP.INCOMPLETE").exists());

                    let restored =
                        DatabaseLayout::open(&target).map_err(|error| error.to_string())?;
                    let restored_id = restored
                        .database_id()
                        .ok_or("published clone database identity missing")?;
                    assert_ne!(restored_id, source_id);
                    let lock = restored
                        .try_writer_lock()
                        .map_err(|error| error.to_string())?;
                    RecoveryManager::new(restored.clone())
                        .recover(&lock)
                        .map_err(|error| error.to_string())?;
                    let report = StorageVerifier::new(restored.clone())
                        .verify(&lock)
                        .map_err(|error| error.to_string())?;
                    assert!(report.is_clean(), "{report}");
                    let current = crate::ManifestStore::new(restored.clone())
                        .read_current()
                        .map_err(|error| error.to_string())?
                        .ok_or("published restore has no CURRENT manifest")?;
                    assert_eq!(current.revision(), report.safe_revision());

                    let audits = WalPrepareLog::new(&restored)
                        .committed_required_audit_records(&lock)
                        .map_err(|error| error.to_string())?;
                    assert_eq!(
                        audits.len(),
                        1,
                        "restore must publish one Required Audit record"
                    );
                    let entry = audits
                        .first()
                        .ok_or("restore publication audit record is missing")?;
                    let record = entry.record();
                    assert_eq!(record.action(), AuditAction::RestorePublication);
                    assert_eq!(record.object_class(), AuditObjectClass::Database);
                    assert_eq!(record.outcome(), AuditOutcome::Succeeded);
                    assert_eq!(record.actor(), id::<PrincipalId>(1)?);
                    assert_eq!(entry.revision(), report.safe_revision());
                    assert_eq!(
                        record.commit_context(),
                        AuditCommitContext::Committed {
                            revision: report.safe_revision(),
                            operation_id: entry.operation_id(),
                        }
                    );

                    if initial_backup_verification.profile() == BackupProfile::AuditComplete {
                        for audit_path in ["audit/AUDIT_MANIFEST", "audit/wal/raw-read.wal"] {
                            assert_eq!(
                                fs::read(target.join(audit_path))
                                    .map_err(|error| error.to_string())?,
                                fs::read(backup.join(audit_path))
                                    .map_err(|error| error.to_string())?,
                                "restored AuditComplete lineage changed at {audit_path}"
                            );
                        }
                    }
                }

                let backup_verification = super::super::verify_exact_backup(backup, None)
                    .map_err(|error| error.to_string())?;
                assert_eq!(
                    backup_verification.manifest_digest(),
                    initial_backup_verification.manifest_digest()
                );
                assert_eq!(
                    backup_verification.inventory_digest(),
                    initial_backup_verification.inventory_digest()
                );
                let source_lock = source
                    .try_writer_lock()
                    .map_err(|error| error.to_string())?;
                let source_report = StorageVerifier::new(source.clone())
                    .verify(&source_lock)
                    .map_err(|error| error.to_string())?;
                assert!(source_report.is_clean(), "{source_report}");
                drop(source_lock);
            }
        }

        assert_eq!(
            fs::read(&sentinel).map_err(|error| error.to_string())?,
            b"preserve-existing-destination"
        );
        Ok(())
    }
}
