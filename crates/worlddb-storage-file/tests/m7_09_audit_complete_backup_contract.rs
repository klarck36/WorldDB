use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    AuditScopeFingerprint, AuthorizationMode, Capability, CapabilityGrant, CapabilityRule,
    ClientRequestId, DomainId, GrantEffect, PageOrdinal, PolicyRuleId, PolicyScope, PolicySubject,
    Principal, PrincipalId, Revision, SecurityEpoch, SecurityPolicyHistory, SecurityPolicySnapshot,
    SecurityPolicyVersion, SnapshotId,
};
use worlddb_storage_file::{
    BackupError, BackupProfile, BackupProgressEvent, ExactBackupManager, RawReadAuditAccessError,
    RawReadAuditWal, RawReadAuditWriter, verify_audit_complete_backup, verify_exact_backup,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m7-09-{}-{sequence}", std::process::id()));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn source(&self) -> Result<worlddb_storage_file::DatabaseLayout, String> {
        worlddb_storage_file::DatabaseLayout::create(self.0.join("source"))
            .map_err(|error| error.to_string())
    }

    fn target(&self, name: &str) -> PathBuf {
        self.0.join(name)
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

fn append_audit_attempt(writer: &RawReadAuditWriter, tail: u8) -> Result<(), String> {
    writer
        .append_attempt(
            worlddb_core::RawReadAttemptScope {
                principal_id: id::<PrincipalId>(1)?,
                scope_fingerprint: AuditScopeFingerprint::new(worlddb_core::Bytes::new(vec![
                    0x31, tail,
                ]))
                .map_err(|error| error.to_string())?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(u64::from(tail)),
            },
            id::<ClientRequestId>(tail)?,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn audit_policy(include_export: bool) -> Result<SecurityPolicyHistory, String> {
    let principal = id::<PrincipalId>(1)?;
    let mut capabilities = vec![
        Capability::ProjectRead,
        Capability::BackupCreate,
        Capability::AuditRead,
    ];
    if include_export {
        capabilities.push(Capability::AuditExport);
    }
    let rules = capabilities
        .iter()
        .enumerate()
        .map(|(index, capability)| -> Result<_, String> {
            Ok(CapabilityRule::new(
                id::<PolicyRuleId>(u8::try_from(index + 1).map_err(|error| error.to_string())?)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(*capability, GrantEffect::Allow),
                PolicyScope::project(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot =
        SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
            .map_err(|error| error.to_string())?;
    let revision = Revision::GENESIS;
    SecurityPolicyHistory::new(
        revision,
        vec![SecurityPolicyVersion::new(
            revision,
            SecurityEpoch::INITIAL,
            snapshot,
        )],
    )
    .map_err(|error| error.to_string())
}

#[test]
fn data_and_audit_profiles_are_distinct_and_keep_independent_watermarks() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.source()?;
    let audit_writer = RawReadAuditWal::new(source.clone())
        .try_writer()
        .map_err(|error| error.to_string())?;
    append_audit_attempt(&audit_writer, 3)?;
    append_audit_attempt(&audit_writer, 4)?;

    let data_lock = source
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let data_revision = worlddb_storage_file::WalPrepareLog::new(&source)
        .commit_head(&data_lock)
        .map_err(|error| error.to_string())?
        .revision();
    assert_eq!(data_revision, Revision::GENESIS);
    drop(data_lock);

    let manager = ExactBackupManager::new(source.clone());
    let data_target = area.target("data-only");
    let data_backup = manager
        .create_exact_backup(&data_target, None)
        .map_err(|error| error.to_string())?;
    assert_eq!(data_backup.profile(), BackupProfile::ExactDatabase);
    assert_eq!(data_backup.audit_safe_sequence(), None);
    let data_manifest =
        fs::read(data_target.join("EXACT_BACKUP")).map_err(|error| error.to_string())?;
    assert_eq!(data_manifest.get(8), Some(&1));
    assert_eq!(data_manifest.get(9), Some(&0));
    assert!(!data_target.join("audit/wal/raw-read.wal").exists());
    assert!(matches!(
        verify_audit_complete_backup(&data_target, None),
        Err(BackupError::ProfileMismatch)
    ));

    let audit_target = area.target("audit-complete");
    let policy_history = audit_policy(true)?;
    let policy = policy_history
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(1)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let appended_after_snapshot = std::cell::Cell::new(false);
    let append_error = std::cell::RefCell::new(None);
    let audit_backup = manager
        .create_audit_complete_backup_with_progress(
            &audit_target,
            None,
            &audit_writer,
            policy,
            worlddb_core::PolicyTarget::default(),
            |event| {
                if matches!(event, BackupProgressEvent::SnapshotPinned { .. })
                    && !appended_after_snapshot.replace(true)
                {
                    if let Err(error) = append_audit_attempt(&audit_writer, 5) {
                        *append_error.borrow_mut() = Some(error);
                    }
                }
            },
        )
        .map_err(|error| error.to_string())?;
    if let Some(error) = append_error.into_inner() {
        return Err(error);
    }
    assert_eq!(audit_backup.profile(), BackupProfile::AuditComplete);
    assert_eq!(audit_backup.revision(), Revision::GENESIS);
    assert_eq!(
        audit_backup
            .audit_safe_sequence()
            .map(|sequence| sequence.value()),
        Some(2)
    );
    assert_eq!(
        audit_writer
            .head()
            .map_err(|error| error.to_string())?
            .sequence()
            .value(),
        3
    );
    assert!(audit_target.join("audit/AUDIT_MANIFEST").is_file());
    let complete_manifest =
        fs::read(audit_target.join("EXACT_BACKUP")).map_err(|error| error.to_string())?;
    assert_eq!(complete_manifest.get(8), Some(&2));
    assert_eq!(complete_manifest.get(9), Some(&1));
    assert_eq!(
        verify_audit_complete_backup(&audit_target, None)
            .map_err(|error| error.to_string())?
            .audit_safe_sequence()
            .map(|sequence| sequence.value()),
        Some(2)
    );
    assert_eq!(
        verify_exact_backup(&audit_target, None)
            .map_err(|error| error.to_string())?
            .profile(),
        BackupProfile::AuditComplete
    );

    let wal_path = audit_target.join("audit/wal/raw-read.wal");
    let mut wal_bytes = fs::read(&wal_path).map_err(|error| error.to_string())?;
    let first_byte = wal_bytes
        .get_mut(0)
        .ok_or_else(|| "audit WAL backup was unexpectedly empty".to_owned())?;
    *first_byte ^= 1;
    fs::write(&wal_path, wal_bytes).map_err(|error| error.to_string())?;
    assert!(matches!(
        verify_audit_complete_backup(&audit_target, None),
        Err(BackupError::IntegrityMismatch)
    ));
    Ok(())
}

#[test]
fn unexpected_audit_segment_files_fail_closed() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.source()?;
    let audit_writer = RawReadAuditWal::new(source.clone())
        .try_writer()
        .map_err(|error| error.to_string())?;
    fs::write(
        source.audit_segments_directory().join("future-segment"),
        b"unknown",
    )
    .map_err(|error| error.to_string())?;
    let policy_history = audit_policy(true)?;
    let policy = policy_history
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(1)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let result = ExactBackupManager::new(source).create_audit_complete_backup(
        area.target("unsupported-segments"),
        None,
        &audit_writer,
        policy,
        worlddb_core::PolicyTarget::default(),
    );
    assert!(matches!(result, Err(BackupError::AuditSegmentsUnsupported)));
    Ok(())
}

#[test]
fn audit_complete_backup_requires_current_export_authorization() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.source()?;
    let audit_writer = RawReadAuditWal::new(source.clone())
        .try_writer()
        .map_err(|error| error.to_string())?;
    let policy_history = audit_policy(false)?;
    let policy = policy_history
        .select(
            AuthorizationMode::Now,
            id::<PrincipalId>(1)?,
            Revision::GENESIS,
        )
        .map_err(|error| error.to_string())?;
    let result = ExactBackupManager::new(source).create_audit_complete_backup(
        area.target("unauthorized-export"),
        None,
        &audit_writer,
        policy,
        worlddb_core::PolicyTarget::default(),
    );
    assert!(matches!(
        result,
        Err(BackupError::AuditAccess(
            RawReadAuditAccessError::ExportUnauthorized
        ))
    ));
    assert!(!area.target("unauthorized-export").exists());
    Ok(())
}
