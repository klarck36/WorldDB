use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, OperationId};
use worlddb_storage_file::{DatabaseLayout, WalError, WalLogEntry, WalPrepareLog};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-04-{}-{sequence}", std::process::id()));
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
fn commit_markers_publish_revisions_and_extend_the_verified_hash_chain() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let log = WalPrepareLog::new(&layout);
    let genesis = log.commit_head(&lock).map_err(|error| error.to_string())?;
    assert_eq!(genesis.revision().value(), 0);
    assert_eq!(genesis.commit_hash().as_bytes(), &[0; 32]);

    let abandoned = log
        .append_prepare(&lock, operation_id(1)?, b"not the selected prepare")
        .map_err(|error| error.to_string())?;
    let first_prepare = log
        .append_prepare(&lock, operation_id(2)?, b"first committed payload")
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        log.commit_prepared(&lock, abandoned),
        Err(WalError::PrepareNotAppendTail)
    ));
    assert_eq!(
        log.commit_head(&lock)
            .map_err(|error| error.to_string())?
            .revision()
            .value(),
        0
    );

    let first = log
        .commit_prepared(&lock, first_prepare)
        .map_err(|error| error.to_string())?;
    assert_eq!(first.revision().value(), 1);
    assert_eq!(first.reference(), first_prepare);
    assert_eq!(
        first.payload_hash(),
        *blake3::hash(b"first committed payload").as_bytes()
    );
    assert_eq!(first.previous_commit_hash(), genesis.commit_hash());

    let second_prepare = log
        .append_prepare(&lock, operation_id(3)?, b"second committed payload")
        .map_err(|error| error.to_string())?;
    let second = log
        .commit_prepared(&lock, second_prepare)
        .map_err(|error| error.to_string())?;
    assert_eq!(second.revision().value(), 2);
    assert_eq!(second.previous_commit_hash(), first.commit_hash());

    let head = log.commit_head(&lock).map_err(|error| error.to_string())?;
    assert_eq!(head.revision(), second.revision());
    assert_eq!(head.commit_hash(), second.commit_hash());

    let entries = log
        .read_segment(&lock, 1)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        entries.as_slice(),
        [
            WalLogEntry::PreparedUncommitted(uncommitted),
            WalLogEntry::Committed(first_frame),
            WalLogEntry::Committed(second_frame),
        ] if uncommitted.reference() == abandoned
            && first_frame.prepare().reference() == first_prepare
            && first_frame.prepare().payload() == b"first committed payload"
            && first_frame.receipt() == first
            && second_frame.prepare().reference() == second_prepare
            && second_frame.prepare().payload() == b"second committed payload"
            && second_frame.receipt() == second
    ));
    Ok(())
}
