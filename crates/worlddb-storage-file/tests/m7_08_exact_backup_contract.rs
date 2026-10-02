use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use worlddb_core::{
    DecodedRecord, DomainId, HistorySpaceDefinition, HistorySpaceId, OperationId, Record, Revision,
    decode_record, encode_record_with_flags,
};
use worlddb_storage_file::{
    BackupAuthenticity, BackupError, BackupMacKey, BackupProgressEvent, CompactionManager,
    DatabaseLayout, ExactBackupManager, HistorySegmentStore, ManifestSegmentKind,
    ManifestSegmentReference, ManifestStore, RecoveryManager, StorageVerifier, WalPrepareLog,
    verify_exact_backup,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempArea(PathBuf);

impl TempArea {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m7-08-{}-{sequence}", std::process::id()));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn database(&self) -> Result<DatabaseLayout, String> {
        DatabaseLayout::create(self.0.join("source")).map_err(|error| error.to_string())
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

fn operation_id(tail: u8) -> Result<OperationId, String> {
    OperationId::try_from_bytes([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
        tail,
    ])
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

fn install_snapshot(layout: &DatabaseLayout) -> Result<ManifestSegmentReference, String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[history_record(1)?])
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    );
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, operation_id(1)?, vec![reference], &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok(reference)
}

fn advance_snapshot(layout: &DatabaseLayout) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let current = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "current manifest was not published".to_owned())?;
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[history_record(2)?])
        .map_err(|error| error.to_string())?;
    let mut references = current.segments().to_vec();
    references.push(ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    ));
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, operation_id(2)?, references, &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn replace_snapshot_inventory(layout: &DatabaseLayout) -> Result<(), String> {
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let receipt = HistorySegmentStore::new(layout.clone())
        .write_decoded_segment(&lock, &[history_record(1)?])
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        Revision::FIRST_COMMIT,
    );
    WalPrepareLog::new(layout)
        .commit_manifest_snapshot(&lock, operation_id(2)?, vec![reference], &[])
        .map_err(|error| error.to_string())?;
    RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn mac_key(id: &str, fill: u8) -> Result<BackupMacKey, String> {
    BackupMacKey::new(id, [fill; 32]).map_err(|error| error.to_string())
}

#[test]
fn hot_backup_stays_at_its_pinned_revision_while_source_writes_continue() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.database()?;
    let _first_segment = install_snapshot(&source)?;
    let target = area.target("backup-hot");
    let writer_layout = source.clone();
    let writer_handle = RefCell::new(None);
    let writer_finished = RefCell::new(false);
    let key = mac_key("backup-key-1", 0x41)?;

    let verification = ExactBackupManager::new(source.clone())
        .create_exact_backup_with_progress(&target, Some(&key), |event| match event {
            BackupProgressEvent::SnapshotPinned { revision, .. } => {
                if revision == Revision::FIRST_COMMIT && writer_handle.borrow().is_none() {
                    let layout = writer_layout.clone();
                    let result = thread::Builder::new()
                        .name("m7-08-concurrent-writer".to_owned())
                        .spawn(move || advance_snapshot(&layout));
                    if let Ok(handle) = result {
                        *writer_handle.borrow_mut() = Some(handle);
                    }
                }
            }
            BackupProgressEvent::ItemCopied { .. } => {
                let handle = writer_handle.borrow_mut().take();
                if let Some(handle) = handle {
                    let result = handle
                        .join()
                        .map_err(|_| "concurrent writer thread panicked".to_owned())
                        .and_then(|result| result);
                    if result.is_ok() {
                        *writer_finished.borrow_mut() = true;
                    } else if let Err(error) = result {
                        *writer_finished.borrow_mut() = false;
                        let _ = error;
                    }
                }
            }
        })
        .map_err(|error| error.to_string())?;

    assert!(*writer_finished.borrow());
    assert_eq!(verification.revision(), Revision::FIRST_COMMIT);
    assert_eq!(
        verification.database_id(),
        source.database_id().ok_or("missing source ID")?
    );
    assert!(matches!(
        verification.authenticity(),
        BackupAuthenticity::Verified { key_id, .. } if key_id == "backup-key-1"
    ));
    assert!(verification.storage_report().is_clean());
    let source_report = StorageVerifier::new(source.clone())
        .verify(
            &source
                .try_writer_lock()
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(
        source_report.safe_revision(),
        Revision::new(2).map_err(|error| error.to_string())?
    );
    let reopened = DatabaseLayout::open(&target).map_err(|error| error.to_string())?;
    let source_wal_segments = fs::read_dir(source.wal_directory())
        .map_err(|error| error.to_string())?
        .count();
    let backup_wal_segments = fs::read_dir(reopened.wal_directory())
        .map_err(|error| error.to_string())?
        .count();
    assert_eq!(source_wal_segments, 2);
    assert_eq!(backup_wal_segments, 1);
    assert_eq!(
        ManifestStore::new(reopened)
            .read_current()
            .map_err(|error| error.to_string())?
            .map(|manifest| manifest.revision()),
        Some(Revision::FIRST_COMMIT)
    );
    Ok(())
}

#[test]
fn integrity_only_and_mac_authenticity_are_reported_separately() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.database()?;
    install_snapshot(&source)?;
    let signed_target = area.target("backup-signed");
    let key = mac_key("trusted-backup-key", 0x52)?;
    let wrong_key_id = mac_key("different-key", 0x52)?;
    let wrong_secret = mac_key("trusted-backup-key", 0x53)?;

    let signed = ExactBackupManager::new(source.clone())
        .create_exact_backup(&signed_target, Some(&key))
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        signed.authenticity(),
        BackupAuthenticity::Verified { .. }
    ));

    let unkeyed_check =
        verify_exact_backup(&signed_target, None).map_err(|error| error.to_string())?;
    assert!(matches!(
        unkeyed_check.authenticity(),
        BackupAuthenticity::ClaimedButUnverified { key_id, .. } if key_id == "trusted-backup-key"
    ));
    let wrong_id_check = verify_exact_backup(&signed_target, Some(&wrong_key_id))
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        wrong_id_check.authenticity(),
        BackupAuthenticity::WrongKey { expected_key_id, provided_key_id }
            if expected_key_id == "trusted-backup-key" && provided_key_id == "different-key"
    ));
    let wrong_secret_check = verify_exact_backup(&signed_target, Some(&wrong_secret))
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        wrong_secret_check.authenticity(),
        BackupAuthenticity::InvalidMac { key_id } if key_id == "trusted-backup-key"
    ));

    let unsigned_target = area.target("backup-unsigned");
    let unsigned = ExactBackupManager::new(source)
        .create_exact_backup(&unsigned_target, None)
        .map_err(|error| error.to_string())?;
    assert_eq!(unsigned.authenticity(), &BackupAuthenticity::NotClaimed);

    let manifest_path = signed_target.join("EXACT_BACKUP");
    let mut bytes = fs::read(&manifest_path).map_err(|error| error.to_string())?;
    let inventory_item_digest = bytes
        .get_mut(115)
        .ok_or_else(|| "backup manifest inventory item was missing".to_owned())?;
    *inventory_item_digest ^= 0x01;
    fs::write(&manifest_path, bytes).map_err(|error| error.to_string())?;
    assert!(matches!(
        verify_exact_backup(&signed_target, Some(&key)),
        Err(BackupError::IntegrityMismatch)
    ));
    Ok(())
}

#[test]
fn target_verify_failure_keeps_the_backup_marked_incomplete() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.database()?;
    install_snapshot(&source)?;
    let target = area.target("backup-damaged-before-publication");
    let mut tampered = false;

    let result =
        ExactBackupManager::new(source).create_exact_backup_with_progress(&target, None, |event| {
            if let BackupProgressEvent::ItemCopied { path, .. } = event {
                if path == "CURRENT" && !tampered {
                    if let Ok(mut bytes) = fs::read(target.join(&path)) {
                        if let Some(first) = bytes.first_mut() {
                            *first ^= 0x01;
                            tampered = fs::write(target.join(path), bytes).is_ok();
                        }
                    }
                }
            }
        });
    assert!(tampered);
    assert!(matches!(result, Err(BackupError::IntegrityMismatch)));
    assert!(matches!(
        verify_exact_backup(&target, None),
        Err(BackupError::IncompleteTarget)
    ));
    Ok(())
}

#[test]
fn backup_pin_protects_retired_snapshot_segments_until_copy_completion() -> Result<(), String> {
    let area = TempArea::create()?;
    let source = area.database()?;
    let old_reference = install_snapshot(&source)?;
    let target = area.target("backup-pinned-segment");
    let writer_layout = source.clone();
    let compaction_layout = source.clone();
    let writer_handle = RefCell::new(None);
    let retained = RefCell::new(false);

    let result = ExactBackupManager::new(source.clone()).create_exact_backup_with_progress(
        &target,
        None,
        |event| match event {
            BackupProgressEvent::SnapshotPinned { .. } => {
                let layout = writer_layout.clone();
                if let Ok(handle) = thread::Builder::new()
                    .name("m7-08-replacing-writer".to_owned())
                    .spawn(move || replace_snapshot_inventory(&layout))
                {
                    *writer_handle.borrow_mut() = Some(handle);
                }
            }
            BackupProgressEvent::ItemCopied { .. } => {
                if let Some(handle) = writer_handle.borrow_mut().take() {
                    let writer_result = handle
                        .join()
                        .map_err(|_| "replacement writer thread panicked".to_owned())
                        .and_then(|result| result);
                    if writer_result.is_ok() {
                        if let Ok(lock) = compaction_layout.try_writer_lock() {
                            let manager = CompactionManager::new(compaction_layout.clone());
                            if let Ok(outcome) = manager.reclaim_retired(&lock, &[old_reference]) {
                                *retained.borrow_mut() =
                                    outcome.retained_by_pin().contains(&old_reference);
                            }
                        }
                    }
                }
            }
        },
    );
    let verification = result.map_err(|error| error.to_string())?;
    assert_eq!(verification.revision(), Revision::FIRST_COMMIT);
    assert!(*retained.borrow());
    let old_path = source.segments_directory().join(format!(
        "segment-{}.wdbseg",
        old_reference.id().to_canonical_string()
    ));
    assert!(old_path.is_file());
    Ok(())
}
