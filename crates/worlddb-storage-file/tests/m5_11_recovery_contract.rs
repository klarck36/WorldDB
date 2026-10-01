use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, OperationId, Revision};
use worlddb_storage_file::{
    CurrentManifestState, DatabaseLayout, ManifestSnapshot, ManifestStore, RecoveryCorruptionKind,
    RecoveryFinding, RecoveryScanner, WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-11-{}-{sequence}", std::process::id()));
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

fn revision(value: u64) -> Result<Revision, String> {
    Revision::try_from(value).map_err(|error| error.to_string())
}

fn segment_path(layout: &DatabaseLayout, sequence: u64) -> PathBuf {
    layout
        .wal_directory()
        .join(format!("segment-{sequence:020}.wal"))
}

#[test]
fn an_empty_database_scans_as_clean_genesis_without_a_manifest() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let report = RecoveryScanner::new(layout)
        .scan(&lock)
        .map_err(|e| e.to_string())?;

    assert_eq!(report.safe_revision(), Revision::GENESIS);
    assert!(report.is_clean());
    assert_eq!(report.current_manifest(), &CurrentManifestState::Missing);
    Ok(())
}

#[test]
fn a_current_manifest_binds_to_the_verified_wal_commit_prefix() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let receipt = wal
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(receipt.revision(), vec![]).map_err(|e| e.to_string())?,
        )
        .map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout)
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.safe_revision(), revision(1)?);
    assert!(report.is_clean());
    assert!(matches!(
        report.current_manifest(),
        CurrentManifestState::Loaded(manifest)
            if manifest.revision() == receipt.revision()
                && manifest.commit_hash() == receipt.commit_hash()
    ));
    Ok(())
}

#[test]
fn a_complete_uncommitted_prepare_is_reported_without_advancing_safe_revision() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let committed = wal
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    let prepare = wal
        .append_prepare(&lock, operation_id(2)?, b"prepared but not committed")
        .map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout)
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.safe_revision(), committed.revision());
    assert!(
        report
            .findings()
            .contains(&RecoveryFinding::UncommittedTail {
                segment_sequence: prepare.segment_sequence(),
                prepare_offset: prepare.byte_offset(),
            })
    );
    Ok(())
}

#[test]
fn a_torn_tail_is_distinguished_and_keeps_only_prior_commits_safe() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let committed = wal
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    let prepare = wal
        .append_prepare(&lock, operation_id(2)?, b"prepare before torn marker")
        .map_err(|error| error.to_string())?;
    let mut segment = OpenOptions::new()
        .append(true)
        .open(segment_path(&layout, prepare.segment_sequence()))
        .map_err(|error| error.to_string())?;
    segment
        .write_all(&[0x57, 0x44, 0x42, 0x43, 0x01])
        .map_err(|e| e.to_string())?;
    segment.sync_all().map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout)
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.safe_revision(), committed.revision());
    assert!(report.findings().contains(&RecoveryFinding::TornTail {
        segment_sequence: prepare.segment_sequence(),
        offset: prepare.byte_offset() + prepare.frame_length(),
        pending_prepare_offset: Some(prepare.byte_offset()),
    }));
    Ok(())
}

#[test]
fn complete_corruption_stops_at_the_prior_verified_commit() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let first = wal
        .commit_operation(&lock, operation_id(1)?, b"first committed")
        .map_err(|error| error.to_string())?;
    let second = wal
        .commit_operation(&lock, operation_id(2)?, b"second committed")
        .map_err(|error| error.to_string())?;
    let corruption_offset = second
        .marker_offset()
        .checked_add(second.marker_length() - 1)
        .ok_or_else(|| "commit-marker corruption offset overflowed".to_owned())?;
    let mut segment = OpenOptions::new()
        .read(true)
        .write(true)
        .open(segment_path(&layout, second.reference().segment_sequence()))
        .map_err(|error| error.to_string())?;
    segment
        .seek(SeekFrom::Start(corruption_offset))
        .map_err(|error| error.to_string())?;
    let mut byte = [0_u8; 1];
    segment
        .read_exact(&mut byte)
        .map_err(|error| error.to_string())?;
    byte[0] ^= 0x80;
    segment
        .seek(SeekFrom::Start(corruption_offset))
        .map_err(|error| error.to_string())?;
    segment
        .write_all(&byte)
        .map_err(|error| error.to_string())?;
    segment.sync_all().map_err(|error| error.to_string())?;
    let corrupted_path = segment_path(&layout, second.reference().segment_sequence());
    let corrupted_bytes = fs::read(&corrupted_path).map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout.clone())
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.safe_revision(), first.revision());
    assert!(
        report
            .findings()
            .contains(&RecoveryFinding::SafeCorruption {
                segment_sequence: Some(second.reference().segment_sequence()),
                offset: Some(second.marker_offset()),
                kind: RecoveryCorruptionKind::InvalidFrame,
            })
    );
    assert_eq!(
        fs::read(corrupted_path).map_err(|error| error.to_string())?,
        corrupted_bytes
    );
    Ok(())
}

#[test]
fn a_sequence_gap_is_safe_corruption_after_the_contiguous_prefix() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let committed = wal
        .commit_operation(&lock, operation_id(1)?, b"first committed")
        .map_err(|error| error.to_string())?;
    fs::write(segment_path(&layout, 3), []).map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout)
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.safe_revision(), committed.revision());
    assert!(
        report
            .findings()
            .contains(&RecoveryFinding::SafeCorruption {
                segment_sequence: Some(3),
                offset: Some(0),
                kind: RecoveryCorruptionKind::SegmentSequenceGap,
            })
    );
    Ok(())
}

#[test]
fn a_corrupt_current_pointer_does_not_inflate_the_safe_wal_revision() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let receipt = wal
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(receipt.revision(), vec![]).map_err(|e| e.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let current_path = layout.current_file();
    let mut current = fs::read(&current_path).map_err(|error| error.to_string())?;
    let digest_byte = current
        .get_mut(17)
        .ok_or_else(|| "CURRENT pointer is shorter than its digest field".to_owned())?;
    *digest_byte ^= 0x01;
    fs::write(&current_path, current).map_err(|error| error.to_string())?;
    let corrupted_current = fs::read(&current_path).map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout.clone())
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.safe_revision(), receipt.revision());
    assert_eq!(report.current_manifest(), &CurrentManifestState::Corrupt);
    assert!(
        report
            .findings()
            .contains(&RecoveryFinding::CurrentManifestCorrupt)
    );
    assert_eq!(
        fs::read(current_path).map_err(|error| error.to_string())?,
        corrupted_current
    );
    Ok(())
}

#[test]
fn a_manifest_ahead_of_a_corrupt_wal_prefix_is_reported_as_untrusted() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let receipt = wal
        .commit_operation(&lock, operation_id(1)?, b"committed")
        .map_err(|error| error.to_string())?;
    ManifestStore::new(layout.clone())
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(receipt.revision(), vec![]).map_err(|e| e.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let path = segment_path(&layout, receipt.reference().segment_sequence());
    let mut bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let checksum_offset = receipt
        .marker_offset()
        .checked_add(receipt.marker_length() - 1)
        .and_then(|offset| usize::try_from(offset).ok())
        .ok_or_else(|| "commit marker checksum offset overflowed".to_owned())?;
    let checksum_byte = bytes
        .get_mut(checksum_offset)
        .ok_or_else(|| "commit marker checksum byte is outside the WAL segment".to_owned())?;
    *checksum_byte ^= 0x80;
    fs::write(path, bytes).map_err(|error| error.to_string())?;

    let report = RecoveryScanner::new(layout)
        .scan(&lock)
        .map_err(|error| error.to_string())?;
    assert_eq!(report.safe_revision(), Revision::GENESIS);
    assert!(
        report
            .findings()
            .contains(&RecoveryFinding::ManifestAheadOfSafePrefix {
                manifest_revision: receipt.revision(),
                safe_revision: Revision::GENESIS,
            })
    );
    Ok(())
}
