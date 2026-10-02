//! Durable sidecar storage for migration-run coordination snapshots.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    DomainId, MAX_MIGRATION_RUN_JOURNAL_BYTES, MigrationRunId, MigrationRunJournalCodecError,
    MigrationRunJournalSnapshot, MigrationRunJournalState, MigrationRunJournalStepState,
    MigrationRunJournalStore,
};

use crate::{DatabaseLayout, WriterLock};

const JOURNAL_DIRECTORY: &str = "migration-runs";
const JOURNAL_EXTENSION: &str = "mjr";
const STAGE_ATTEMPTS: u8 = 16;

static NEXT_STAGE_FILE: AtomicU64 = AtomicU64::new(0);

/// Durable sidecar store for run journals, guarded by the database writer lock.
///
/// Snapshots live under `migration-runs/`, outside the normative record history. Each update is
/// checksum-validated, written and synced to a same-volume staging file, then atomically renamed.
pub struct MigrationRunJournalFileStore<'a> {
    root: PathBuf,
    lock: &'a WriterLock,
}

impl<'a> MigrationRunJournalFileStore<'a> {
    /// Binds the store to one open database and its exclusive writer lock.
    pub fn new(
        layout: &DatabaseLayout,
        lock: &'a WriterLock,
    ) -> Result<Self, MigrationRunJournalFileStoreError> {
        let root = layout.root().to_path_buf();
        if !lock.belongs_to_database_root(&root) {
            return Err(MigrationRunJournalFileStoreError::ForeignWriterLock);
        }
        Ok(Self { root, lock })
    }

    fn directory(&self) -> PathBuf {
        self.root.join(JOURNAL_DIRECTORY)
    }

    fn path_for(&self, run_id: MigrationRunId) -> PathBuf {
        self.directory().join(format!(
            "{}.{}",
            run_id.to_canonical_string(),
            JOURNAL_EXTENSION
        ))
    }

    fn require_lock(&self) -> Result<(), MigrationRunJournalFileStoreError> {
        if self.lock.belongs_to_database_root(&self.root) {
            Ok(())
        } else {
            Err(MigrationRunJournalFileStoreError::ForeignWriterLock)
        }
    }

    fn ensure_directory(&self) -> Result<(), MigrationRunJournalFileStoreError> {
        self.require_lock()?;
        let directory = self.directory();
        match fs::symlink_metadata(&directory) {
            Ok(metadata) => validate_directory(&self.root, &directory, &metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !self.lock.require_write_access() {
                    return Err(MigrationRunJournalFileStoreError::RecoveryRequired);
                }
                fs::create_dir(&directory).map_err(|source| {
                    MigrationRunJournalFileStoreError::Io {
                        operation: "create migration-run journal directory",
                        source,
                    }
                })?;
                validate_directory(
                    &self.root,
                    &directory,
                    &fs::symlink_metadata(&directory).map_err(|source| {
                        MigrationRunJournalFileStoreError::Io {
                            operation: "inspect created migration-run journal directory",
                            source,
                        }
                    })?,
                )?;
                crate::manifest::sync_directory(&self.root).map_err(|source| {
                    MigrationRunJournalFileStoreError::Io {
                        operation: "sync database root after creating migration-run journal directory",
                        source,
                    }
                })?;
                Ok(())
            }
            Err(source) => Err(MigrationRunJournalFileStoreError::Io {
                operation: "inspect migration-run journal directory",
                source,
            }),
        }
    }

    fn read_snapshot(
        &self,
        run_id: MigrationRunId,
    ) -> Result<Option<MigrationRunJournalSnapshot>, MigrationRunJournalFileStoreError> {
        self.require_lock()?;
        let directory = self.directory();
        match fs::symlink_metadata(&directory) {
            Ok(metadata) => validate_directory(&self.root, &directory, &metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(MigrationRunJournalFileStoreError::Io {
                    operation: "inspect migration-run journal directory",
                    source,
                });
            }
        }

        let path = self.path_for(run_id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(MigrationRunJournalFileStoreError::Io {
                    operation: "inspect migration-run journal snapshot",
                    source,
                });
            }
        };
        validate_file(&directory, &path, &metadata)?;
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if length > MAX_MIGRATION_RUN_JOURNAL_BYTES {
            return Err(MigrationRunJournalFileStoreError::ResourceLimit {
                limit: MAX_MIGRATION_RUN_JOURNAL_BYTES,
                actual: length,
            });
        }
        let file = File::open(&path).map_err(|source| MigrationRunJournalFileStoreError::Io {
            operation: "open migration-run journal snapshot",
            source,
        })?;
        let opened = file
            .metadata()
            .map_err(|source| MigrationRunJournalFileStoreError::Io {
                operation: "inspect opened migration-run journal snapshot",
                source,
            })?;
        if !opened.is_file() || opened.len() != metadata.len() {
            return Err(MigrationRunJournalFileStoreError::InvalidPath);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| {
            MigrationRunJournalFileStoreError::ResourceLimit {
                limit: MAX_MIGRATION_RUN_JOURNAL_BYTES,
                actual: length,
            }
        })?;
        file.take(u64::try_from(MAX_MIGRATION_RUN_JOURNAL_BYTES).unwrap_or(u64::MAX) + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| MigrationRunJournalFileStoreError::Io {
                operation: "read migration-run journal snapshot",
                source,
            })?;
        if bytes.len() != length || bytes.len() > MAX_MIGRATION_RUN_JOURNAL_BYTES {
            return Err(MigrationRunJournalFileStoreError::Corrupt);
        }
        let snapshot = MigrationRunJournalSnapshot::decode(&bytes)
            .map_err(MigrationRunJournalFileStoreError::Codec)?;
        if snapshot.spec().run_id() != run_id {
            return Err(MigrationRunJournalFileStoreError::Corrupt);
        }
        Ok(Some(snapshot))
    }
}

impl MigrationRunJournalStore for MigrationRunJournalFileStore<'_> {
    type Error = MigrationRunJournalFileStoreError;

    fn load(
        &self,
        run_id: MigrationRunId,
    ) -> Result<Option<MigrationRunJournalSnapshot>, Self::Error> {
        self.read_snapshot(run_id)
    }

    fn save(&mut self, snapshot: &MigrationRunJournalSnapshot) -> Result<(), Self::Error> {
        self.require_lock()?;
        if !self.lock.require_write_access() {
            return Err(MigrationRunJournalFileStoreError::RecoveryRequired);
        }
        self.ensure_directory()?;
        let run_id = snapshot.spec().run_id();
        match self.read_snapshot(run_id)? {
            Some(previous) => snapshot
                .validate_successor(&previous)
                .map_err(MigrationRunJournalFileStoreError::Transition)?,
            None => {
                let is_initial = snapshot.state() == MigrationRunJournalState::Running
                    && snapshot
                        .steps()
                        .iter()
                        .all(|step| step.state() == MigrationRunJournalStepState::Pending);
                if !is_initial {
                    return Err(MigrationRunJournalFileStoreError::Transition(
                        worlddb_core::MigrationRunJournalError::InvalidStateTransition,
                    ));
                }
            }
        }

        let bytes = snapshot
            .encode()
            .map_err(MigrationRunJournalFileStoreError::Codec)?;
        let directory = self.directory();
        let target = self.path_for(run_id);
        let metadata = match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                validate_file(&directory, &target, &metadata)?;
                Some(metadata)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(MigrationRunJournalFileStoreError::Io {
                    operation: "inspect migration-run journal before replacement",
                    source,
                });
            }
        };
        let (stage_path, mut stage_file) = self.create_stage_file()?;
        let mut stage_guard = StagePathGuard::new(stage_path.clone());
        stage_file
            .write_all(&bytes)
            .and_then(|()| stage_file.sync_all())
            .map_err(|source| MigrationRunJournalFileStoreError::Io {
                operation: "write and sync migration-run journal snapshot",
                source,
            })?;
        drop(stage_file);
        if metadata.is_some() {
            replace_file(&stage_path, &target).map_err(|source| {
                MigrationRunJournalFileStoreError::Io {
                    operation: "atomically replace migration-run journal snapshot",
                    source,
                }
            })?;
        } else {
            publish_new_file(&stage_path, &target).map_err(|source| {
                MigrationRunJournalFileStoreError::Io {
                    operation: "publish migration-run journal snapshot",
                    source,
                }
            })?;
        }
        stage_guard.mark_published();
        crate::manifest::sync_directory(&directory).map_err(|source| {
            MigrationRunJournalFileStoreError::Io {
                operation: "sync migration-run journal directory",
                source,
            }
        })?;
        Ok(())
    }
}

impl MigrationRunJournalFileStore<'_> {
    fn create_stage_file(&self) -> Result<(PathBuf, File), MigrationRunJournalFileStoreError> {
        let staging_directory = self.root.join("staging");
        let mut attempt = 0_u8;
        while attempt < STAGE_ATTEMPTS {
            let sequence = NEXT_STAGE_FILE.fetch_add(1, Ordering::Relaxed);
            let file_name = format!("migration-run-{}-{sequence}.tmp", std::process::id());
            let path = staging_directory.join(file_name);
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
                Err(source) => {
                    return Err(MigrationRunJournalFileStoreError::Io {
                        operation: "create migration-run journal staging file",
                        source,
                    });
                }
            }
        }
        Err(MigrationRunJournalFileStoreError::StagingNameExhausted)
    }
}

/// Filesystem, lock, checksum, or monotonic-transition error for the journal sidecar.
#[derive(Debug)]
pub enum MigrationRunJournalFileStoreError {
    /// The supplied writer lock belongs to a different database root.
    ForeignWriterLock,
    /// Recovery verification has not authorized database writes.
    RecoveryRequired,
    /// The snapshot path or contents are malformed or inconsistent.
    Corrupt,
    /// A snapshot path resolves through a symlink or outside its database directory.
    InvalidPath,
    /// The encoded journal exceeds a fixed storage resource limit.
    ResourceLimit { limit: usize, actual: usize },
    /// Every bounded staging-file name was already in use.
    StagingNameExhausted,
    /// Encoded bytes were not a valid core journal snapshot.
    Codec(MigrationRunJournalCodecError),
    /// A new snapshot skips or reverses a legal journal transition.
    Transition(worlddb_core::MigrationRunJournalError),
    /// A filesystem operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for MigrationRunJournalFileStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWriterLock => {
                formatter.write_str("migration-run journal requires this database's writer lock")
            }
            Self::RecoveryRequired => {
                formatter.write_str("migration-run journal writes require a clean recovery state")
            }
            Self::Corrupt => {
                formatter.write_str("migration-run journal snapshot is corrupt or inconsistent")
            }
            Self::InvalidPath => formatter.write_str(
                "migration-run journal path is not a regular file or directory inside the database",
            ),
            Self::ResourceLimit { limit, actual } => write!(
                formatter,
                "migration-run journal uses {actual} bytes; limit is {limit}"
            ),
            Self::StagingNameExhausted => formatter
                .write_str("could not allocate a unique migration-run journal staging file"),
            Self::Codec(error) => write!(formatter, "migration-run journal codec failed: {error}"),
            Self::Transition(error) => write!(
                formatter,
                "migration-run journal transition failed: {error}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl std::error::Error for MigrationRunJournalFileStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(error) => Some(error),
            Self::Transition(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::ForeignWriterLock
            | Self::RecoveryRequired
            | Self::Corrupt
            | Self::InvalidPath
            | Self::ResourceLimit { .. }
            | Self::StagingNameExhausted => None,
        }
    }
}

fn validate_directory(
    root: &Path,
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), MigrationRunJournalFileStoreError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MigrationRunJournalFileStoreError::InvalidPath);
    }
    let canonical =
        fs::canonicalize(path).map_err(|source| MigrationRunJournalFileStoreError::Io {
            operation: "resolve migration-run journal directory",
            source,
        })?;
    if canonical.parent() != Some(root) {
        return Err(MigrationRunJournalFileStoreError::InvalidPath);
    }
    Ok(())
}

fn validate_file(
    directory: &Path,
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), MigrationRunJournalFileStoreError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MigrationRunJournalFileStoreError::InvalidPath);
    }
    let canonical =
        fs::canonicalize(path).map_err(|source| MigrationRunJournalFileStoreError::Io {
            operation: "resolve migration-run journal snapshot",
            source,
        })?;
    if canonical.parent() != Some(directory) {
        return Err(MigrationRunJournalFileStoreError::InvalidPath);
    }
    Ok(())
}

fn publish_new_file(stage: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        crate::windows_publication::move_file(stage, target, false)
    }
    #[cfg(not(windows))]
    {
        fs::rename(stage, target)
    }
}

fn replace_file(stage: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        crate::windows_publication::move_file(stage, target, true)
    }
    #[cfg(not(windows))]
    {
        fs::rename(stage, target)
    }
}

struct StagePathGuard {
    path: PathBuf,
    published: bool,
}

impl StagePathGuard {
    const fn new(path: PathBuf) -> Self {
        Self {
            path,
            published: false,
        }
    }

    fn mark_published(&mut self) {
        self.published = true;
    }
}

impl Drop for StagePathGuard {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MigrationRunJournalFileStore, MigrationRunJournalFileStoreError};
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use worlddb_core::{
        DomainId, MigrationId, MigrationPlanFingerprint, MigrationRunId,
        MigrationRunJournalSnapshot, MigrationRunJournalSpec, MigrationRunJournalStepSpec,
        MigrationRunJournalStore, MigrationStepId, MigrationTransformFingerprint,
        MigrationTransformerVersion, OperationId, Revision, SchemaRevision,
    };

    use crate::DatabaseLayout;

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDatabase(PathBuf);

    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-migration-run-journal-{}-{sequence}",
                std::process::id()
            ));
            DatabaseLayout::create(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn id<T: DomainId>(last: u8) -> Result<T, String> {
        let mut bytes = [0; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = last;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn pending_snapshot() -> Result<MigrationRunJournalSnapshot, String> {
        let spec = MigrationRunJournalSpec::new(
            id::<MigrationId>(1)?,
            id::<MigrationRunId>(2)?,
            MigrationPlanFingerprint::from_bytes([3; 32]),
            SchemaRevision::from_published_revision(Revision::GENESIS),
            [4; 32],
            MigrationTransformerVersion::new(1).map_err(|error| error.to_string())?,
            vec![MigrationRunJournalStepSpec::new(
                id::<MigrationStepId>(5)?,
                id::<OperationId>(6)?,
                [7; 32],
                Revision::FIRST_COMMIT,
            )],
        )
        .map_err(|error| error.to_string())?;
        MigrationRunJournalSnapshot::start(spec).map_err(|error| error.to_string())
    }

    #[test]
    fn file_store_persists_and_reopens_monotone_sidecar_snapshots() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut store =
            MigrationRunJournalFileStore::new(&layout, &lock).map_err(|error| error.to_string())?;
        let mut snapshot = pending_snapshot()?;
        let run_id = snapshot.spec().run_id();
        assert_eq!(store.load(run_id).map_err(|error| error.to_string())?, None);

        store.save(&snapshot).map_err(|error| error.to_string())?;
        let step_id = snapshot
            .spec()
            .steps()
            .first()
            .map(|step| step.step_id())
            .ok_or_else(|| String::from("journal step spec is missing"))?;
        snapshot
            .prepare_step(step_id)
            .map_err(|error| error.to_string())?;
        store.save(&snapshot).map_err(|error| error.to_string())?;
        assert_eq!(
            store.load(run_id).map_err(|error| error.to_string())?,
            Some(snapshot.clone())
        );

        snapshot
            .mark_step_committed(
                step_id,
                Revision::FIRST_COMMIT,
                MigrationTransformFingerprint::from_bytes([8; 32]),
            )
            .map_err(|error| error.to_string())?;
        store.save(&snapshot).map_err(|error| error.to_string())?;
        snapshot
            .mark_completed()
            .map_err(|error| error.to_string())?;
        store.save(&snapshot).map_err(|error| error.to_string())?;
        assert_eq!(
            store.load(run_id).map_err(|error| error.to_string())?,
            Some(snapshot)
        );
        Ok(())
    }

    #[test]
    fn file_store_rejects_state_reversal() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let mut store =
            MigrationRunJournalFileStore::new(&layout, &lock).map_err(|error| error.to_string())?;
        let snapshot = pending_snapshot()?;
        store.save(&snapshot).map_err(|error| error.to_string())?;
        let mut later = snapshot.clone();
        let step_id = later
            .spec()
            .steps()
            .first()
            .map(|step| step.step_id())
            .ok_or_else(|| String::from("journal step spec is missing"))?;
        later
            .prepare_step(step_id)
            .map_err(|error| error.to_string())?;
        store.save(&later).map_err(|error| error.to_string())?;

        let error = match store.save(&snapshot) {
            Ok(()) => return Err(String::from("reversed journal state was persisted")),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            MigrationRunJournalFileStoreError::Transition(_)
        ));
        assert_eq!(
            store
                .load(snapshot.spec().run_id())
                .map_err(|error| error.to_string())?,
            Some(later)
        );
        Ok(())
    }
}
