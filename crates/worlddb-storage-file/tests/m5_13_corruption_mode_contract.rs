use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    DomainId, HistorySpaceDefinition, HistorySpaceId, OperationId, Principal, PrincipalId, Record,
    Revision, SecurityEpoch, SecurityPolicySnapshot, SecurityPolicyVersion,
};
use worlddb_storage_file::{
    CurrentManifestState, DatabaseLayout, HistorySegmentStore, ManifestError, ManifestSegmentKind,
    ManifestSegmentReference, ManifestSnapshot, ManifestStore, RecoveryDisposition, RecoveryError,
    RecoveryFinding, RecoveryManager, RecoveryScanner, SecurityPolicyHistoryStore,
    SecurityPolicyStorageError, SegmentError, WalError, WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-13-{}-{sequence}", std::process::id()));
        DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn layout(&self) -> Result<DatabaseLayout, String> {
        DatabaseLayout::open(&self.0).map_err(|error| error.to_string())
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn operation_id(tail: u8) -> Result<OperationId, String> {
    OperationId::try_from_bytes([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        tail,
    ])
    .map_err(|error| error.to_string())
}

fn id<T: DomainId>(tail: u8) -> Result<T, String> {
    let mut bytes = [0_u8; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = tail;
    T::try_from_bytes(bytes).map_err(|error| error.to_string())
}

fn revision(value: u64) -> Result<Revision, String> {
    Revision::try_from(value).map_err(|error| error.to_string())
}

fn mutate_first_byte(path: &PathBuf) -> Result<Vec<u8>, String> {
    let mut bytes = fs::read(path).map_err(|error| error.to_string())?;
    let first = bytes
        .first_mut()
        .ok_or_else(|| "segment file is unexpectedly empty".to_owned())?;
    *first ^= 0x40;
    fs::write(path, &bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn first_file(directory: PathBuf) -> Result<PathBuf, String> {
    fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .next()
        .ok_or_else(|| "expected one immutable segment".to_owned())?
        .map(|entry| entry.path())
        .map_err(|error| error.to_string())
}

#[test]
fn corrupted_referenced_history_is_quarantined_read_only_and_cannot_be_written()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let history = HistorySegmentStore::new(layout.clone());
    let history_space_id = id::<HistorySpaceId>(1)?;
    let history_record = Record::HistorySpaceDefinition(
        HistorySpaceDefinition::new(history_space_id, None, Revision::GENESIS)
            .map_err(|error| error.to_string())?,
    );
    let history_staged = history
        .stage_segment(&lock, &[history_record])
        .map_err(|error| error.to_string())?;
    let committed_revision = revision(1)?;
    let history_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        history_staged.id(),
        history_staged.content_digest(),
        committed_revision,
    );

    let principal_id = id::<PrincipalId>(2)?;
    let policy =
        SecurityPolicySnapshot::new(vec![Principal::new(principal_id)], vec![], vec![], vec![])
            .map_err(|error| error.to_string())?;
    let security = SecurityPolicyHistoryStore::new(layout.clone());
    let security_staged = security
        .stage_version(
            &lock,
            &SecurityPolicyVersion::new(committed_revision, SecurityEpoch::new(0), policy.clone()),
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
    let security_reference = ManifestSegmentReference::new(
        ManifestSegmentKind::SecurityPolicy,
        security_staged.id(),
        security_staged.content_digest(),
        committed_revision,
    );

    let wal = WalPrepareLog::new(&layout);
    let receipt = wal
        .commit_manifest_snapshot(
            &lock,
            operation_id(1)?,
            vec![history_reference, security_reference],
            &[history_reference, security_reference],
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(receipt.revision(), committed_revision);

    let pending = RecoveryScanner::new(layout.clone())
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.disposition(), RecoveryDisposition::RecoveryRequired);
    assert!(matches!(
        wal.commit_operation(&lock, operation_id(2)?, b"blocked before manifest replay"),
        Err(WalError::RecoveryRequired)
    ));
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;

    let history_path = first_file(layout.segments_directory())?;
    let damaged_history = mutate_first_byte(&history_path)?;
    let security_path = first_file(layout.security_segments_directory())?;
    let damaged_security = mutate_first_byte(&security_path)?;
    let wal_path = first_file(layout.wal_directory())?;
    let wal_before = fs::read(&wal_path).map_err(|error| error.to_string())?;
    drop(lock);

    let reopened = database.layout()?;
    let reopened_lock = reopened
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let report = RecoveryScanner::new(reopened.clone())
        .scan(&reopened_lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        report.disposition(),
        RecoveryDisposition::QuarantinedReadOnly
    );
    assert!(matches!(
        report.findings().first(),
        Some(RecoveryFinding::ReferencedSegmentCorrupt {
            kind: ManifestSegmentKind::History,
            id,
        }) if *id == history_staged.id()
    ));
    assert!(report.findings().iter().any(|finding| matches!(
        finding,
        RecoveryFinding::ReferencedSegmentCorrupt {
            kind: ManifestSegmentKind::SecurityPolicy,
            id,
        } if *id == security_staged.id()
    )));
    let safe_report = report.to_string();
    assert!(safe_report.starts_with("QUARANTINED_READ_ONLY:"));
    assert!(!safe_report.contains("blocked before manifest replay"));
    assert!(matches!(
        report.current_manifest(),
        CurrentManifestState::Loaded(manifest) if manifest.revision() == receipt.revision()
    ));

    assert!(matches!(
        WalPrepareLog::new(&reopened).commit_operation(
            &reopened_lock,
            operation_id(3)?,
            b"must not append"
        ),
        Err(WalError::RecoveryRequired)
    ));
    assert!(matches!(
        HistorySegmentStore::new(reopened.clone()).stage_segment(
            &reopened_lock,
            &[Record::HistorySpaceDefinition(
                HistorySpaceDefinition::new(id::<HistorySpaceId>(3)?, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?
            )]
        ),
        Err(SegmentError::RecoveryRequired)
    ));
    assert!(matches!(
        SecurityPolicyHistoryStore::new(reopened.clone()).stage_version(
            &reopened_lock,
            &SecurityPolicyVersion::new(committed_revision, SecurityEpoch::new(0), policy),
            None,
            None,
        ),
        Err(SecurityPolicyStorageError::RecoveryRequired)
    ));
    assert!(matches!(
        ManifestStore::new(reopened.clone()).publish(
            &reopened_lock,
            &WalPrepareLog::new(&reopened),
            ManifestSnapshot::new(receipt.revision(), vec![]).map_err(|error| error.to_string())?,
        ),
        Err(ManifestError::RecoveryRequired)
    ));
    assert!(matches!(
        RecoveryManager::new(reopened.clone()).recover(&reopened_lock),
        Err(RecoveryError::UnsafeFindings(_))
    ));
    assert_eq!(
        fs::read(history_path).map_err(|error| error.to_string())?,
        damaged_history
    );
    assert_eq!(
        fs::read(security_path).map_err(|error| error.to_string())?,
        damaged_security
    );
    assert_eq!(
        fs::read(wal_path).map_err(|error| error.to_string())?,
        wal_before
    );

    drop(reopened_lock);
    assert!(matches!(
        reopened.resave_format(),
        Err(worlddb_storage_file::StorageFileError::RecoveryRequired)
    ));
    Ok(())
}

#[test]
fn same_revision_manifest_inventory_mismatch_is_never_repaired() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let revision = revision(1)?;
    let history = HistorySegmentStore::new(layout.clone());
    let segment = history
        .write_segment(
            &lock,
            &[Record::HistorySpaceDefinition(
                HistorySpaceDefinition::new(id::<HistorySpaceId>(6)?, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
            )],
        )
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        segment.id(),
        segment.content_digest(),
        revision,
    );
    let wal = WalPrepareLog::new(&layout);
    wal.commit_manifest_snapshot(&lock, operation_id(6)?, vec![], &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;

    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(revision, vec![reference]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let current_before = fs::read(layout.current_file()).map_err(|error| error.to_string())?;
    let report = RecoveryScanner::new(layout.clone())
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        report.disposition(),
        RecoveryDisposition::QuarantinedReadOnly
    );
    assert!(
        report
            .findings()
            .contains(&RecoveryFinding::ManifestSnapshotMismatch {
                snapshot_revision: revision,
            })
    );
    assert!(matches!(
        RecoveryManager::new(layout.clone()).recover(&lock),
        Err(RecoveryError::UnsafeFindings(_))
    ));
    assert_eq!(
        fs::read(layout.current_file()).map_err(|error| error.to_string())?,
        current_before
    );
    assert!(matches!(
        wal.commit_operation(&lock, operation_id(7)?, b"blocked by manifest mismatch"),
        Err(WalError::RecoveryRequired)
    ));
    Ok(())
}

#[test]
fn recoverable_tail_blocks_writes_until_recovery_finishes() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    wal.append_prepare(&lock, operation_id(4)?, b"valid but uncommitted")
        .map_err(|error| error.to_string())?;
    drop(lock);

    let reopened = database.layout()?;
    let reopened_lock = reopened
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let report = RecoveryScanner::new(reopened.clone())
        .scan(&reopened_lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.disposition(), RecoveryDisposition::RecoveryRequired);
    assert!(matches!(
        WalPrepareLog::new(&reopened).commit_operation(
            &reopened_lock,
            operation_id(5)?,
            b"wait for recovery"
        ),
        Err(WalError::RecoveryRequired)
    ));

    let recovered = RecoveryManager::new(reopened.clone())
        .recover(&reopened_lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(recovered.report().disposition(), RecoveryDisposition::Clean);
    WalPrepareLog::new(&reopened)
        .commit_operation(&reopened_lock, operation_id(5)?, b"write after recovery")
        .map_err(|error| error.to_string())?;
    Ok(())
}
