#![cfg(windows)]

use std::env;
use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, OperationId, Revision};
use worlddb_storage_file::{
    DatabaseLayout, ManifestError, ManifestSnapshot, ManifestStore, WalPrepareLog,
};

const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-09-{}-{sequence}", std::process::id()));
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

#[test]
fn a_nondeletable_current_handle_preserves_the_old_pointer_and_allows_retry() -> Result<(), String>
{
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = ManifestStore::new(layout.clone());
    store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(Revision::GENESIS, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;

    let held_current = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(layout.current_file())
        .map_err(|error| error.to_string())?;
    wal.commit_operation(&lock, operation_id(1)?, b"windows replace test")
        .map_err(|error| error.to_string())?;

    let failed = match store.publish(
        &lock,
        &wal,
        ManifestSnapshot::new(revision(1)?, vec![]).map_err(|error| error.to_string())?,
    ) {
        Err(error) => error,
        Ok(_) => {
            return Err(
                "CURRENT replacement should fail while delete sharing is denied".to_owned(),
            );
        }
    };
    assert!(matches!(
        failed,
        ManifestError::Io {
            operation: "atomically replace CURRENT pointer",
            ..
        }
    ));
    assert_eq!(
        store
            .read_current()
            .map_err(|error| error.to_string())?
            .map(|manifest| (manifest.generation(), manifest.revision())),
        Some((1, Revision::GENESIS))
    );
    assert!(
        layout
            .manifests_directory()
            .join("manifest-00000000000000000002.wdbm")
            .is_file()
    );

    drop(held_current);
    let retry = store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(revision(1)?, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(retry.generation(), 3);
    assert_eq!(
        store
            .read_current()
            .map_err(|error| error.to_string())?
            .map(|manifest| manifest.revision()),
        Some(revision(1)?)
    );
    Ok(())
}

#[test]
fn a_current_directory_entry_is_rejected_without_following_it() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = ManifestStore::new(layout.clone());
    store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(Revision::GENESIS, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    fs::remove_file(layout.current_file()).map_err(|error| error.to_string())?;
    fs::create_dir(layout.current_file()).map_err(|error| error.to_string())?;

    assert!(matches!(
        store.read_current(),
        Err(ManifestError::PathOutsideDatabase)
    ));
    Ok(())
}
