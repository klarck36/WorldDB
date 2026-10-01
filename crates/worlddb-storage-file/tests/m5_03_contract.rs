use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, OperationId};
use worlddb_storage_file::{DatabaseLayout, WalError, WalLogEntry, WalPrepareLog};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-03-{}-{sequence}", std::process::id()));
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
fn prepare_frames_append_sync_and_read_back_as_uncommitted() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let log = WalPrepareLog::new(&layout);
    let first_operation = operation_id(1)?;
    let second_operation = operation_id(2)?;

    let first = log
        .append_prepare(&lock, first_operation, b"first canonical payload")
        .map_err(|error| error.to_string())?;
    let second = log
        .append_prepare(&lock, second_operation, b"second canonical payload")
        .map_err(|error| error.to_string())?;
    assert_eq!(first.segment_sequence(), 1);
    assert_eq!(first.byte_offset(), 0);
    assert_eq!(second.segment_sequence(), 1);
    assert_eq!(second.byte_offset(), first.frame_length());

    let entries = log
        .read_segment(&lock, 1)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        entries.as_slice(),
        [
            WalLogEntry::PreparedUncommitted(first_entry),
            WalLogEntry::PreparedUncommitted(second_entry)
        ] if first_entry.reference() == first
            && first_entry.payload() == b"first canonical payload"
            && second_entry.reference() == second
            && second_entry.payload() == b"second canonical payload"
    ));
    Ok(())
}

#[test]
fn wal_rolls_to_the_next_numbered_segment_at_the_size_boundary() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let log = WalPrepareLog::new(&layout);
    let large_payload = vec![0x5a; 16 * 1024 * 1024];

    let first = log
        .append_prepare(&lock, operation_id(3)?, &large_payload)
        .map_err(|error| error.to_string())?;
    let second = log
        .append_prepare(&lock, operation_id(4)?, b"next segment")
        .map_err(|error| error.to_string())?;

    assert_eq!(first.segment_sequence(), 1);
    assert_eq!(first.byte_offset(), 0);
    assert_eq!(second.segment_sequence(), 2);
    assert_eq!(second.byte_offset(), 0);
    let first_entries = log
        .read_segment(&lock, 1)
        .map_err(|error| error.to_string())?;
    let second_entries = log
        .read_segment(&lock, 2)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        first_entries.as_slice(),
        [WalLogEntry::PreparedUncommitted(entry)]
            if entry.reference() == first && entry.payload() == large_payload
    ));
    assert!(matches!(
        second_entries.as_slice(),
        [WalLogEntry::PreparedUncommitted(entry)]
            if entry.reference() == second && entry.payload() == b"next segment"
    ));
    Ok(())
}

#[test]
fn a_lock_from_another_database_cannot_write_this_wal() -> Result<(), String> {
    let first_database = TempDatabase::create()?;
    let second_database = TempDatabase::create()?;
    let first_layout = first_database.layout()?;
    let second_layout = second_database.layout()?;
    let lock = first_layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let log = WalPrepareLog::new(&second_layout);

    assert!(matches!(
        log.append_prepare(&lock, operation_id(5)?, b"payload"),
        Err(WalError::WriterLockMismatch)
    ));
    assert!(
        fs::read_dir(second_layout.wal_directory())
            .map_err(|error| error.to_string())?
            .next()
            .is_none()
    );
    Ok(())
}

#[test]
fn a_torn_segment_tail_is_not_extended_by_a_later_prepare() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let log = WalPrepareLog::new(&layout);
    log.append_prepare(&lock, operation_id(6)?, b"complete frame")
        .map_err(|error| error.to_string())?;

    let segment = layout
        .wal_directory()
        .join("segment-00000000000000000001.wal");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&segment)
        .map_err(|error| error.to_string())?;
    file.write_all(&[0xaa]).map_err(|error| error.to_string())?;
    drop(file);
    let length_with_torn_tail = fs::metadata(&segment)
        .map_err(|error| error.to_string())?
        .len();

    assert!(matches!(
        log.append_prepare(&lock, operation_id(7)?, b"must not append"),
        Err(WalError::TornTail { sequence: 1, .. })
    ));
    assert_eq!(
        fs::metadata(&segment)
            .map_err(|error| error.to_string())?
            .len(),
        length_with_torn_tail
    );
    Ok(())
}
