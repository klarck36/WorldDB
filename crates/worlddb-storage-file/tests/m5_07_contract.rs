use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, OperationId, Revision};
use worlddb_storage_file::{
    ContentDigest, DatabaseLayout, ManifestError, ManifestSegmentKind, ManifestSegmentReference,
    ManifestSnapshot, ManifestStore, WalPrepareLog,
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn create() -> Result<Self, String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!("worlddb-m5-07-{}-{sequence}", std::process::id()));
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
fn manifest_publication_advances_current_only_after_a_verified_generation() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = ManifestStore::new(layout.clone());

    assert_eq!(
        store.read_current().map_err(|error| error.to_string())?,
        None
    );
    let first = store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(Revision::GENESIS, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(first.generation(), 1);
    assert_eq!(first.revision(), Revision::GENESIS);
    assert_eq!(
        store
            .read_current()
            .map_err(|error| error.to_string())?
            .map(|manifest| manifest.generation()),
        Some(1)
    );

    wal.commit_operation(&lock, operation_id(1)?, b"revision one")
        .map_err(|error| error.to_string())?;
    let second = store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(revision(1)?, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(second.generation(), 2);
    assert_eq!(second.revision(), revision(1)?);
    assert!(
        layout
            .manifests_directory()
            .join("manifest-00000000000000000001.wdbm")
            .is_file()
    );
    assert!(
        layout
            .manifests_directory()
            .join("manifest-00000000000000000002.wdbm")
            .is_file()
    );
    Ok(())
}

#[test]
fn a_lagging_manifest_is_allowed_but_ahead_of_wal_manifest_is_rejected() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = ManifestStore::new(layout);
    let first = wal
        .commit_operation(&lock, operation_id(2)?, b"first")
        .map_err(|error| error.to_string())?;
    wal.commit_operation(&lock, operation_id(3)?, b"second")
        .map_err(|error| error.to_string())?;

    let receipt = store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(first.revision(), vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(receipt.revision(), first.revision());
    assert_eq!(receipt.commit_hash(), first.commit_hash());

    let error = match store.publish(
        &lock,
        &wal,
        ManifestSnapshot::new(revision(3)?, vec![]).map_err(|error| error.to_string())?,
    ) {
        Err(error) => error,
        Ok(_) => return Err("manifest must not move beyond the verified WAL head".to_owned()),
    };
    assert!(matches!(
        error,
        ManifestError::ManifestAheadOfWal {
            manifest_revision,
            wal_revision,
        } if manifest_revision == revision(3).unwrap_or(Revision::GENESIS)
            && wal_revision == revision(2).unwrap_or(Revision::GENESIS)
    ));
    assert_eq!(
        store
            .read_current()
            .map_err(|error| error.to_string())?
            .map(|manifest| (manifest.revision(), manifest.commit_hash())),
        Some((first.revision(), first.commit_hash()))
    );
    Ok(())
}

#[test]
fn manifest_revision_cannot_regress_and_segment_revisions_are_bounded() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = ManifestStore::new(layout);
    wal.commit_operation(&lock, operation_id(4)?, b"first")
        .map_err(|error| error.to_string())?;
    store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(revision(1)?, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;

    let error = match store.publish(
        &lock,
        &wal,
        ManifestSnapshot::new(Revision::GENESIS, vec![]).map_err(|error| error.to_string())?,
    ) {
        Err(error) => error,
        Ok(_) => return Err("manifest may not regress behind CURRENT".to_owned()),
    };
    assert!(matches!(error, ManifestError::RevisionRegression { .. }));

    let mut id_bytes = [0_u8; 16];
    id_bytes[6] = 0x40;
    id_bytes[8] = 0x80;
    id_bytes[15] = 9;
    let segment_id = worlddb_storage_file::SegmentId::try_from_bytes(id_bytes)
        .map_err(|error| error.to_string())?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        segment_id,
        ContentDigest::from_bytes([0x55; 32]),
        revision(2)?,
    );
    assert!(matches!(
        ManifestSnapshot::new(revision(1)?, vec![reference]),
        Err(ManifestError::SegmentBeyondSnapshot)
    ));
    Ok(())
}

#[test]
fn current_pointer_digest_detects_manifest_tampering() -> Result<(), String> {
    let database = TempDatabase::create()?;
    let layout = database.layout()?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let store = ManifestStore::new(layout.clone());
    let receipt = store
        .publish(
            &lock,
            &wal,
            ManifestSnapshot::new(Revision::GENESIS, vec![]).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;

    let manifest_path = layout
        .manifests_directory()
        .join("manifest-00000000000000000001.wdbm");
    let mut bytes = fs::read(&manifest_path).map_err(|error| error.to_string())?;
    if let Some(byte) = bytes.get_mut(12) {
        *byte ^= 1;
    }
    fs::write(&manifest_path, bytes).map_err(|error| error.to_string())?;
    assert!(matches!(
        store.read_current(),
        Err(ManifestError::InvalidManifest)
    ));
    assert_eq!(receipt.generation(), 1);
    Ok(())
}
