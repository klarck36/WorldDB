use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, OperationId};
use worlddb_storage_file::{DatabaseLayout, WalError, WalOperationStatus, WalPrepareLog};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-05-{}-{sequence}", std::process::id()));
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

#[test]
fn committed_operation_replay_returns_the_original_receipt_without_writing_again()
-> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let log = WalPrepareLog::new(&layout);
    let absent_id = operation_id(2)?;
    let operation_id = operation_id(1)?;
    assert_eq!(
        log.operation_status(&lock, absent_id)
            .map_err(|error| error.to_string())?,
        WalOperationStatus::NotCommitted
    );

    let first_receipt = log
        .commit_operation(&lock, operation_id, b"canonical operation payload")
        .map_err(|error| error.to_string())?;
    let segment_path = layout
        .wal_directory()
        .join("segment-00000000000000000001.wal");
    let length_after_first_commit = fs::metadata(&segment_path)
        .map_err(|error| error.to_string())?
        .len();

    let reopened_layout = database.layout()?;
    let reopened_log = WalPrepareLog::new(&reopened_layout);
    let replayed_receipt = reopened_log
        .commit_operation(&lock, operation_id, b"canonical operation payload")
        .map_err(|error| error.to_string())?;
    assert_eq!(replayed_receipt, first_receipt);
    assert_eq!(
        fs::metadata(&segment_path)
            .map_err(|error| error.to_string())?
            .len(),
        length_after_first_commit
    );
    assert_eq!(
        reopened_log
            .operation_status(&lock, operation_id)
            .map_err(|error| error.to_string())?,
        WalOperationStatus::Committed(first_receipt)
    );
    assert!(matches!(
        reopened_log.commit_operation(&lock, operation_id, b"different payload"),
        Err(WalError::IdempotencyMismatch { operation_id: mismatch_id })
            if mismatch_id == operation_id
    ));
    assert!(matches!(
        reopened_log.append_prepare(&lock, operation_id, b"canonical operation payload"),
        Err(WalError::OperationAlreadyCommitted { operation_id: committed_id })
            if committed_id == operation_id
    ));
    assert_eq!(
        fs::metadata(&segment_path)
            .map_err(|error| error.to_string())?
            .len(),
        length_after_first_commit
    );
    assert_eq!(
        reopened_log
            .commit_head(&lock)
            .map_err(|error| error.to_string())?
            .revision()
            .value(),
        1
    );
    Ok(())
}

#[test]
fn synced_uncommitted_prepare_remains_indeterminate_and_cannot_be_reexecuted() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let log = WalPrepareLog::new(&layout);
    let operation_id = operation_id(3)?;
    log.append_prepare(&lock, operation_id, b"pending operation")
        .map_err(|error| error.to_string())?;
    let segment_path = layout
        .wal_directory()
        .join("segment-00000000000000000001.wal");
    let length_after_prepare = fs::metadata(&segment_path)
        .map_err(|error| error.to_string())?
        .len();
    let reopened_layout = database.layout()?;
    let reopened_log = WalPrepareLog::new(&reopened_layout);

    assert_eq!(
        reopened_log
            .operation_status(&lock, operation_id)
            .map_err(|error| error.to_string())?,
        WalOperationStatus::Indeterminate
    );
    assert!(matches!(
        reopened_log.commit_operation(&lock, operation_id, b"pending operation"),
        Err(WalError::OperationIndeterminate { operation_id: pending_id })
            if pending_id == operation_id
    ));
    assert!(matches!(
        reopened_log.commit_operation(&lock, operation_id, b"different payload"),
        Err(WalError::IdempotencyMismatch { operation_id: mismatch_id })
            if mismatch_id == operation_id
    ));
    assert!(matches!(
        reopened_log.append_prepare(&lock, operation_id, b"pending operation"),
        Err(WalError::OperationIndeterminate { operation_id: pending_id })
            if pending_id == operation_id
    ));
    assert_eq!(
        fs::metadata(&segment_path)
            .map_err(|error| error.to_string())?
            .len(),
        length_after_prepare
    );
    Ok(())
}
