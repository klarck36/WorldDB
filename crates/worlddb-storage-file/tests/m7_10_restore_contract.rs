use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditScopeFingerprint, AuditSequence, AuthorizationMode, Bytes, Capability,
    CapabilityGrant, CapabilityRule, ClientRequestId, DecodedRecord, DomainId, GrantEffect,
    HistorySpaceDefinition, HistorySpaceId, PageOrdinal, PolicyRuleId, PolicyScope, PolicySubject,
    Principal, PrincipalId, RawReadAttemptScope, Record, Revision, SecurityEpoch,
    SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion, SnapshotId,
    decode_record, encode_record_with_flags,
};
use worlddb_storage_file::{
    BackupProfile, DatabaseLayout, ExactBackupManager, HistorySegmentStore, ManifestSegmentKind,
    ManifestSegmentReference, ManifestStore, RawReadAuditWal, RawReadAuditWriter, RecoveryManager,
    RestoreError, RestoreManager, StorageVerifier, WalPrepareLog, verify_exact_backup,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m7-10-{}-{sequence}", std::process::id()));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn source(&self) -> Result<DatabaseLayout, String> {
        DatabaseLayout::create(self.path("source")).map_err(|error| error.to_string())
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

fn policy(capabilities: &[Capability]) -> Result<SecurityPolicyHistory, String> {
    let principal_id = id::<PrincipalId>(1)?;
    let rules = capabilities
        .iter()
        .enumerate()
        .map(|(index, capability)| -> Result<_, String> {
            Ok(CapabilityRule::new(
                id::<PolicyRuleId>(u8::try_from(index + 1).map_err(|error| error.to_string())?)?,
                PolicySubject::Principal(principal_id),
                CapabilityGrant::new(*capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal_id)], vec![], vec![], rules)
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

fn restore_policy(
    history: &SecurityPolicyHistory,
) -> Result<worlddb_core::SecurityPolicyView<'_>, String> {
    history
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(1)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())
}

fn policy_fingerprint() -> Result<AuditPolicyFingerprint, String> {
    AuditPolicyFingerprint::new(Bytes::new(vec![0x4d, 0x37, 0x10]))
        .map_err(|error| error.to_string())
}

fn all_restore_capabilities() -> [Capability; 3] {
    [
        Capability::BackupRestore,
        Capability::AuditRead,
        Capability::AuditExport,
    ]
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

fn history_record(tail: u8) -> Result<DecodedRecord, String> {
    let record = Record::HistorySpaceDefinition(
        HistorySpaceDefinition::new(id::<HistorySpaceId>(tail)?, None, Revision::GENESIS)
            .map_err(|error| error.to_string())?,
    );
    let encoded = encode_record_with_flags(&record, 0).map_err(|error| error.to_string())?;
    decode_record(&encoded).map_err(|error| error.to_string())
}

fn install_history_snapshot(layout: &DatabaseLayout) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[history_record(7)?])
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    );
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(
            &lock,
            id::<worlddb_core::OperationId>(8)?,
            vec![reference],
            &[],
        )
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn record_by_id(
    layout: &DatabaseLayout,
    record_id: worlddb_core::AuditRecordId,
) -> Result<AuditRecord, String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    WalPrepareLog::new(layout)
        .committed_required_audit_records(&lock)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|entry| entry.record().record_id() == record_id)
        .map(|entry| entry.record().clone())
        .ok_or_else(|| "restore publication audit record was not committed".to_owned())
}

#[test]
fn exact_backup_restores_as_a_new_id_with_atomic_restore_audit() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.source()?;
    install_history_snapshot(&source)?;
    let source_id = source.database_id().ok_or("source has no identity")?;
    let backup_path = area.path("backup");
    let backup = ExactBackupManager::new(source.clone())
        .create_exact_backup(&backup_path, None)
        .map_err(|error| error.to_string())?;
    let permissions = policy(&all_restore_capabilities())?;
    let destination = area.path("restored");

    let restored = RestoreManager::new()
        .restore_clone(
            &backup_path,
            &destination,
            None,
            restore_policy(&permissions)?,
            worlddb_core::PolicyTarget::default(),
            policy_fingerprint()?,
        )
        .map_err(|error| error.to_string())?;

    assert_eq!(restored.source_database_id(), source_id);
    assert_ne!(restored.restored_database_id(), source_id);
    assert_eq!(restored.profile(), BackupProfile::ExactDatabase);
    assert_eq!(restored.source_revision(), backup.revision());
    assert_eq!(
        restored.restored_revision(),
        backup
            .revision()
            .next_commit()
            .map_err(|error| error.to_string())?
    );
    assert!(!destination.join("EXACT_BACKUP").exists());
    assert!(!destination.join("EXACT_BACKUP.INCOMPLETE").exists());

    let layout = DatabaseLayout::open(&destination).map_err(|error| error.to_string())?;
    assert_eq!(layout.database_id(), Some(restored.restored_database_id()));
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let report = StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(|error| error.to_string())?;
    assert!(report.is_clean(), "{report}");
    assert_eq!(report.safe_revision(), restored.restored_revision());
    let current = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or("restore publication did not publish a current manifest")?;
    assert_eq!(current.revision(), restored.restored_revision());
    drop(lock);

    let record = record_by_id(&layout, restored.audit_record_id())?;
    assert_eq!(record.action(), AuditAction::RestorePublication);
    assert_eq!(record.object_class(), AuditObjectClass::Database);
    assert_eq!(record.outcome(), AuditOutcome::Succeeded);
    assert_eq!(record.actor(), id::<PrincipalId>(1)?);
    assert_eq!(record.sequence(), AuditSequence::new(1));
    assert_eq!(
        record.commit_context(),
        AuditCommitContext::Committed {
            revision: restored.restored_revision(),
            operation_id: restored.operation_id(),
        }
    );
    assert_eq!(record.security_epoch(), SecurityEpoch::INITIAL);
    Ok(())
}

#[test]
fn audit_complete_restore_checks_permissions_and_preserves_separate_audit_lineage()
-> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.source()?;
    let audit_writer = RawReadAuditWal::new(source.clone())
        .try_writer()
        .map_err(|error| error.to_string())?;
    append_audit_attempt(&audit_writer)?;
    let full_permissions = policy(&all_restore_capabilities())?;
    let backup_path = area.path("audit-backup");
    let backup = ExactBackupManager::new(source.clone())
        .create_audit_complete_backup(
            &backup_path,
            None,
            &audit_writer,
            restore_policy(&full_permissions)?,
            worlddb_core::PolicyTarget::default(),
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(backup.profile(), BackupProfile::AuditComplete);
    let original_audit_wal =
        fs::read(backup_path.join("audit/wal/raw-read.wal")).map_err(|error| error.to_string())?;
    let original_audit_manifest =
        fs::read(backup_path.join("audit/AUDIT_MANIFEST")).map_err(|error| error.to_string())?;

    let unreadable_backup = area.path("unreadable-backup");
    fs::create_dir(&unreadable_backup).map_err(|error| error.to_string())?;
    copy_tree(&backup_path, &unreadable_backup)?;
    fs::write(
        unreadable_backup.join("audit/wal/raw-read.wal"),
        b"unreadable payload",
    )
    .map_err(|error| error.to_string())?;
    let no_audit_read = policy(&[Capability::BackupRestore, Capability::AuditExport])?;
    let denied_target = area.path("denied-restored");
    let denied = RestoreManager::new().restore_clone(
        &unreadable_backup,
        &denied_target,
        None,
        restore_policy(&no_audit_read)?,
        worlddb_core::PolicyTarget::default(),
        policy_fingerprint()?,
    );
    assert!(matches!(
        denied,
        Err(RestoreError::AuthorizationDenied {
            capability: Capability::AuditRead
        })
    ));
    assert!(!denied_target.exists());

    let no_audit_export = policy(&[Capability::BackupRestore, Capability::AuditRead])?;
    let denied_target = area.path("denied-export-restored");
    let denied = RestoreManager::new().restore_clone(
        &backup_path,
        &denied_target,
        None,
        restore_policy(&no_audit_export)?,
        worlddb_core::PolicyTarget::default(),
        policy_fingerprint()?,
    );
    assert!(matches!(
        denied,
        Err(RestoreError::AuthorizationDenied {
            capability: Capability::AuditExport
        })
    ));
    assert!(!denied_target.exists());

    let destination = area.path("audit-restored");
    let restored = RestoreManager::new()
        .restore_clone(
            &backup_path,
            &destination,
            None,
            restore_policy(&full_permissions)?,
            worlddb_core::PolicyTarget::default(),
            policy_fingerprint()?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(restored.audit_safe_sequence(), backup.audit_safe_sequence());
    assert_eq!(
        fs::read(destination.join("audit/wal/raw-read.wal")).map_err(|error| error.to_string())?,
        original_audit_wal
    );
    assert_eq!(
        fs::read(destination.join("audit/AUDIT_MANIFEST")).map_err(|error| error.to_string())?,
        original_audit_manifest
    );
    assert_eq!(
        verify_exact_backup(&backup_path, None)
            .map_err(|error| error.to_string())?
            .audit_safe_sequence(),
        restored.audit_safe_sequence()
    );
    let restore_record = record_by_id(
        &DatabaseLayout::open(&destination).map_err(|error| error.to_string())?,
        restored.audit_record_id(),
    )?;
    assert_eq!(restore_record.action(), AuditAction::RestorePublication);
    Ok(())
}

#[test]
fn existing_destinations_and_corrupt_backups_are_never_published_or_overwritten()
-> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.source()?;
    let backup_path = area.path("backup");
    ExactBackupManager::new(source)
        .create_exact_backup(&backup_path, None)
        .map_err(|error| error.to_string())?;
    let permissions = policy(&all_restore_capabilities())?;

    let existing_target = area.path("existing");
    fs::create_dir(&existing_target).map_err(|error| error.to_string())?;
    fs::write(existing_target.join("keep.txt"), b"unchanged").map_err(|error| error.to_string())?;
    let existing = RestoreManager::new().restore_clone(
        &backup_path,
        &existing_target,
        None,
        restore_policy(&permissions)?,
        worlddb_core::PolicyTarget::default(),
        policy_fingerprint()?,
    );
    assert!(matches!(existing, Err(RestoreError::TargetAlreadyExists)));
    assert_eq!(
        fs::read(existing_target.join("keep.txt")).map_err(|error| error.to_string())?,
        b"unchanged"
    );

    let corrupted_backup = area.path("corrupted-backup");
    fs::create_dir(&corrupted_backup).map_err(|error| error.to_string())?;
    copy_tree(&backup_path, &corrupted_backup)?;
    fs::write(corrupted_backup.join("DATABASE_ID"), [0_u8; 16])
        .map_err(|error| error.to_string())?;
    let corrupt_target = area.path("corrupt-restored");
    assert!(
        RestoreManager::new()
            .restore_clone(
                &corrupted_backup,
                &corrupt_target,
                None,
                restore_policy(&permissions)?,
                worlddb_core::PolicyTarget::default(),
                policy_fingerprint()?,
            )
            .is_err()
    );
    assert!(!corrupt_target.exists());
    Ok(())
}

#[test]
fn backup_restore_permission_is_checked_before_staging() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.source()?;
    let backup_path = area.path("backup");
    ExactBackupManager::new(source)
        .create_exact_backup(&backup_path, None)
        .map_err(|error| error.to_string())?;
    let permissions = policy(&[Capability::AuditRead, Capability::AuditExport])?;
    let destination = area.path("denied");
    let result = RestoreManager::new().restore_clone(
        &backup_path,
        &destination,
        None,
        restore_policy(&permissions)?,
        worlddb_core::PolicyTarget::default(),
        policy_fingerprint()?,
    );
    assert!(matches!(
        result,
        Err(RestoreError::AuthorizationDenied {
            capability: Capability::BackupRestore
        })
    ));
    assert!(!destination.exists());
    Ok(())
}

fn copy_tree(source: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            fs::create_dir(&target_path).map_err(|error| error.to_string())?;
            copy_tree(&source_path, &target_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path).map_err(|error| error.to_string())?;
        } else {
            return Err("backup fixture contains an unexpected filesystem entry".to_owned());
        }
    }
    Ok(())
}
