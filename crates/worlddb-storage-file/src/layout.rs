//! Stable database paths and format-probe lifecycle.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DatabaseId, DomainId, IdGenerationError, IdValidationError};

use crate::format::{FORMAT_FILE_BYTES, FormatCapabilities, FormatProbeError, probe_format};
use crate::recovery::RecoveryScanner;
use crate::writer_lock::{WriterLock, WriterLockError};

const FORMAT_FILE_NAME: &str = "FORMAT";
const DATABASE_ID_FILE_NAME: &str = "DATABASE_ID";
const WRITER_LOCK_FILE_NAME: &str = "LOCK";
const CURRENT_FILE_NAME: &str = "CURRENT";
const LAYOUT_DIRECTORIES: &[&str] = &[
    "wal",
    "segments",
    "manifests",
    "audit",
    "audit/wal",
    "audit/segments",
    "security",
    "security/segments",
    "staging",
    "quarantine",
];

static NEXT_STAGE_FILE: AtomicU64 = AtomicU64::new(0);

/// Why a database directory could not be initialized or opened.
#[derive(Debug)]
pub enum StorageFileError {
    /// The requested database path already exists and will not be overwritten.
    DatabaseAlreadyExists,
    /// The system could not generate a persistent database identity.
    Identity(IdGenerationError),
    /// A persisted database identity is malformed.
    InvalidDatabaseId(IdValidationError),
    /// A required directory or file is missing or has the wrong type.
    InvalidLayoutEntry { relative_path: &'static str },
    /// A required database entry resolves outside the canonical database root.
    PathEscapesDatabaseRoot { relative_path: &'static str },
    /// Opening or publishing a database file failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// The format probe was not supported by this implementation.
    Format(FormatProbeError),
    /// Another process already owns the database writer lock.
    WriterLock(WriterLockError),
    /// Recovery verification has not authorized ordinary database writes.
    RecoveryRequired,
    /// Every bounded temporary-file name was already taken.
    StagingNameExhausted,
}

impl fmt::Display for StorageFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseAlreadyExists => formatter.write_str("database directory already exists"),
            Self::Identity(error) => {
                write!(formatter, "database identity generation failed: {error}")
            }
            Self::InvalidDatabaseId(error) => {
                write!(formatter, "database identity is invalid: {error}")
            }
            Self::InvalidLayoutEntry { relative_path } => {
                write!(
                    formatter,
                    "database layout entry is missing or invalid: {relative_path}"
                )
            }
            Self::PathEscapesDatabaseRoot { relative_path } => write!(
                formatter,
                "database layout entry resolves outside its root: {relative_path}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Format(error) => write!(formatter, "database format probe failed: {error}"),
            Self::WriterLock(error) => error.fmt(formatter),
            Self::RecoveryRequired => formatter
                .write_str("database writes are blocked until recovery verifies a clean state"),
            Self::StagingNameExhausted => {
                formatter.write_str("could not allocate a unique format staging file")
            }
        }
    }
}

impl std::error::Error for StorageFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Identity(error) => Some(error),
            Self::InvalidDatabaseId(error) => Some(error),
            Self::Format(error) => Some(error),
            Self::WriterLock(error) => Some(error),
            _ => None,
        }
    }
}

/// Canonical paths for one initialized WorldDB directory tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseLayout {
    root: PathBuf,
    format_capabilities: FormatCapabilities,
    database_id: Option<DatabaseId>,
}

impl DatabaseLayout {
    /// Creates a new database directory and its fixed 1.0 path layout.
    ///
    /// An existing root is never reused or overwritten. The format probe is
    /// written last so a partially initialized directory cannot be opened as
    /// a valid database.
    pub fn create(root: impl AsRef<Path>) -> Result<Self, StorageFileError> {
        let requested_root = root.as_ref();
        if requested_root.exists() {
            return Err(StorageFileError::DatabaseAlreadyExists);
        }
        if let Some(parent) = requested_root.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|source| StorageFileError::Io {
                    operation: "create database parent directory",
                    source,
                })?;
            }
        }
        fs::create_dir(requested_root).map_err(|source| StorageFileError::Io {
            operation: "create database root",
            source,
        })?;
        let root = canonical_directory(requested_root)?;
        for relative_path in LAYOUT_DIRECTORIES {
            fs::create_dir_all(root.join(relative_path)).map_err(|source| {
                StorageFileError::Io {
                    operation: "create database layout directory",
                    source,
                }
            })?;
        }
        validate_layout_directories(&root)?;

        let database_id = worlddb_core::storage_internal::generate_database_id()
            .map_err(StorageFileError::Identity)?;
        let database_id_path = root.join(DATABASE_ID_FILE_NAME);
        let mut database_id_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&database_id_path)
            .map_err(|source| StorageFileError::Io {
                operation: "create database identity",
                source,
            })?;
        database_id_file
            .write_all(&database_id.to_bytes())
            .map_err(|source| StorageFileError::Io {
                operation: "write database identity",
                source,
            })?;
        database_id_file
            .sync_all()
            .map_err(|source| StorageFileError::Io {
                operation: "sync database identity",
                source,
            })?;
        crate::manifest::sync_directory(&root).map_err(|source| StorageFileError::Io {
            operation: "sync database identity directory",
            source,
        })?;

        let format_capabilities = FormatCapabilities::current();
        let format_bytes = format_capabilities
            .encode()
            .map_err(|error| StorageFileError::Format(FormatProbeError::Frame(error)))?;
        let format_path = root.join(FORMAT_FILE_NAME);
        let mut format_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&format_path)
            .map_err(|source| StorageFileError::Io {
                operation: "create database format probe",
                source,
            })?;
        format_file
            .write_all(&format_bytes)
            .map_err(|source| StorageFileError::Io {
                operation: "write database format probe",
                source,
            })?;
        format_file
            .sync_all()
            .map_err(|source| StorageFileError::Io {
                operation: "sync database format probe",
                source,
            })?;

        Ok(Self {
            root,
            format_capabilities,
            database_id: Some(database_id),
        })
    }

    pub(crate) fn prepare_restore_staging_root(
        root: impl AsRef<Path>,
    ) -> Result<PathBuf, StorageFileError> {
        let root = canonical_directory(root.as_ref())?;
        for relative_path in LAYOUT_DIRECTORIES {
            fs::create_dir_all(root.join(relative_path)).map_err(|source| {
                StorageFileError::Io {
                    operation: "prepare restore staging layout",
                    source,
                }
            })?;
        }
        validate_layout_directories(&root)?;
        Ok(root)
    }

    /// Opens the layout, validates all required directories, and probes the
    /// versioned capability frame without interpreting unknown optional bits.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StorageFileError> {
        let root = canonical_directory(root.as_ref())?;
        validate_layout_directories(&root)?;

        let format_path = root.join(FORMAT_FILE_NAME);
        validate_file_inside_root(&root, &format_path, FORMAT_FILE_NAME)?;
        let mut format_file = File::open(&format_path).map_err(|source| StorageFileError::Io {
            operation: "open database format probe",
            source,
        })?;
        let read_limit =
            u64::try_from(FORMAT_FILE_BYTES + 1).map_err(|source| StorageFileError::Io {
                operation: "compute format-probe read limit",
                source: io::Error::new(io::ErrorKind::InvalidData, source),
            })?;
        let mut format_bytes = Vec::with_capacity(FORMAT_FILE_BYTES);
        Read::by_ref(&mut format_file)
            .take(read_limit)
            .read_to_end(&mut format_bytes)
            .map_err(|source| StorageFileError::Io {
                operation: "read database format probe",
                source,
            })?;
        let format_capabilities = probe_format(&format_bytes).map_err(StorageFileError::Format)?;
        let database_id_path = root.join(DATABASE_ID_FILE_NAME);
        let database_id = match fs::symlink_metadata(&database_id_path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != 16
                {
                    return Err(StorageFileError::InvalidLayoutEntry {
                        relative_path: DATABASE_ID_FILE_NAME,
                    });
                }
                validate_file_inside_root(&root, &database_id_path, DATABASE_ID_FILE_NAME)?;
                let mut bytes = Vec::with_capacity(16);
                File::open(&database_id_path)
                    .map_err(|source| StorageFileError::Io {
                        operation: "open database identity",
                        source,
                    })?
                    .take(17)
                    .read_to_end(&mut bytes)
                    .map_err(|source| StorageFileError::Io {
                        operation: "read database identity",
                        source,
                    })?;
                if bytes.len() != 16 {
                    return Err(StorageFileError::InvalidLayoutEntry {
                        relative_path: DATABASE_ID_FILE_NAME,
                    });
                }
                let raw: [u8; 16] =
                    bytes
                        .try_into()
                        .map_err(|_| StorageFileError::InvalidLayoutEntry {
                            relative_path: DATABASE_ID_FILE_NAME,
                        })?;
                Some(DatabaseId::try_from_bytes(raw).map_err(StorageFileError::InvalidDatabaseId)?)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(StorageFileError::Io {
                    operation: "inspect database identity",
                    source,
                });
            }
        };

        Ok(Self {
            root,
            format_capabilities,
            database_id,
        })
    }

    /// Canonical database root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Versioned format capabilities read from the root probe.
    #[must_use]
    pub const fn format_capabilities(&self) -> FormatCapabilities {
        self.format_capabilities
    }

    /// Persistent identity assigned when the database was created, when present.
    #[must_use]
    pub const fn database_id(&self) -> Option<DatabaseId> {
        self.database_id
    }

    /// Path of the format-probe frame.
    #[must_use]
    pub fn format_file(&self) -> PathBuf {
        self.root.join(FORMAT_FILE_NAME)
    }

    /// Path of the persistent database identity.
    #[must_use]
    pub fn database_id_file(&self) -> PathBuf {
        self.root.join(DATABASE_ID_FILE_NAME)
    }

    /// Path of the current-manifest pointer.
    #[must_use]
    pub fn current_file(&self) -> PathBuf {
        self.root.join(CURRENT_FILE_NAME)
    }

    /// Path of the persistent writer-lock file.
    #[must_use]
    pub fn writer_lock_file(&self) -> PathBuf {
        self.root.join(WRITER_LOCK_FILE_NAME)
    }

    /// Path of the append-only data WAL directory.
    #[must_use]
    pub fn wal_directory(&self) -> PathBuf {
        self.root.join("wal")
    }

    /// Path of the immutable history-segment directory.
    #[must_use]
    pub fn segments_directory(&self) -> PathBuf {
        self.root.join("segments")
    }

    /// Path of the immutable manifest-generation directory.
    #[must_use]
    pub fn manifests_directory(&self) -> PathBuf {
        self.root.join("manifests")
    }

    /// Path of immutable derived-index generations and their family pointers.
    #[must_use]
    pub fn indexes_directory(&self) -> PathBuf {
        self.root.join("indexes")
    }

    /// Path of the independent audit namespace.
    #[must_use]
    pub fn audit_directory(&self) -> PathBuf {
        self.root.join("audit")
    }

    /// Path of the append-only audit WAL directory.
    #[must_use]
    pub fn audit_wal_directory(&self) -> PathBuf {
        self.root.join("audit").join("wal")
    }

    /// Path of the audit-segment directory.
    #[must_use]
    pub fn audit_segments_directory(&self) -> PathBuf {
        self.root.join("audit").join("segments")
    }

    /// Immutable security-policy and audit-configuration history segments.
    #[must_use]
    pub fn security_segments_directory(&self) -> PathBuf {
        self.root.join("security").join("segments")
    }

    /// Path of the same-volume temporary staging directory.
    #[must_use]
    pub fn staging_directory(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// Path of the quarantine directory.
    #[must_use]
    pub fn quarantine_directory(&self) -> PathBuf {
        self.root.join("quarantine")
    }

    /// Acquires the exclusive lock and verifies recovery state without blocking.
    ///
    /// The lock remains usable for scanning and recovery if the database is not
    /// clean, but storage mutations stay blocked until a clean scan completes.
    pub fn try_writer_lock(&self) -> Result<WriterLock, WriterLockError> {
        let lock = WriterLock::try_acquire(&self.root, &self.writer_lock_file())?;
        let _ = RecoveryScanner::new(self.clone()).scan(&lock);
        Ok(lock)
    }

    /// Acquires a shared lock for read-only scans without creating a lock file.
    ///
    /// This fails with [`WriterLockError::LockFileMissing`] when the database
    /// has never established its stable lock file. Callers can then report that
    /// read-only access is unavailable without modifying the database.
    pub fn try_read_only_lock(&self) -> Result<WriterLock, WriterLockError> {
        WriterLock::try_acquire_read_only(&self.root, &self.writer_lock_file())
    }

    /// Rewrites the format probe to a same-volume staging file and publishes
    /// it with one rename while holding the exclusive writer lock.
    pub fn resave_format(&self) -> Result<(), StorageFileError> {
        let lock = self
            .try_writer_lock()
            .map_err(StorageFileError::WriterLock)?;
        if !lock.require_write_access() {
            return Err(StorageFileError::RecoveryRequired);
        }
        validate_layout_directories(&self.root)?;

        let format_bytes = self
            .format_capabilities
            .encode()
            .map_err(|error| StorageFileError::Format(FormatProbeError::Frame(error)))?;
        let (stage_path, mut stage_file) = self.create_stage_file()?;
        let mut stage_guard = StagePathGuard::new(stage_path.clone());
        stage_file
            .write_all(&format_bytes)
            .map_err(|source| StorageFileError::Io {
                operation: "write staged format probe",
                source,
            })?;
        stage_file
            .sync_all()
            .map_err(|source| StorageFileError::Io {
                operation: "sync staged format probe",
                source,
            })?;
        drop(stage_file);
        fs::rename(&stage_path, self.format_file()).map_err(|source| StorageFileError::Io {
            operation: "publish format probe",
            source,
        })?;
        stage_guard.mark_published();
        Ok(())
    }

    fn create_stage_file(&self) -> Result<(PathBuf, File), StorageFileError> {
        let staging_directory = self.staging_directory();
        let mut attempt = 0_u8;
        while attempt < 16 {
            let sequence = NEXT_STAGE_FILE.fetch_add(1, Ordering::Relaxed);
            let file_name = format!("format-{}-{sequence}.tmp", std::process::id());
            let path = staging_directory.join(file_name);
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                }
                Err(source) => {
                    return Err(StorageFileError::Io {
                        operation: "create staged format probe",
                        source,
                    });
                }
            }
        }
        Err(StorageFileError::StagingNameExhausted)
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, StorageFileError> {
    let canonical = fs::canonicalize(path).map_err(|source| StorageFileError::Io {
        operation: "resolve database root directory",
        source,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|source| StorageFileError::Io {
        operation: "inspect database root directory",
        source,
    })?;
    if !metadata.is_dir() {
        return Err(StorageFileError::InvalidLayoutEntry { relative_path: "." });
    }
    Ok(canonical)
}

fn validate_layout_directories(root: &Path) -> Result<(), StorageFileError> {
    for relative_path in LAYOUT_DIRECTORIES {
        let path = root.join(relative_path);
        let metadata = fs::metadata(&path).map_err(|source| StorageFileError::Io {
            operation: "inspect database layout directory",
            source,
        })?;
        if !metadata.is_dir() {
            return Err(StorageFileError::InvalidLayoutEntry { relative_path });
        }
        let canonical = fs::canonicalize(&path).map_err(|source| StorageFileError::Io {
            operation: "resolve database layout directory",
            source,
        })?;
        if !canonical.starts_with(root) {
            return Err(StorageFileError::PathEscapesDatabaseRoot { relative_path });
        }
    }
    Ok(())
}

fn validate_file_inside_root(
    root: &Path,
    path: &Path,
    relative_path: &'static str,
) -> Result<(), StorageFileError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| StorageFileError::Io {
        operation: "inspect database file",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(StorageFileError::InvalidLayoutEntry { relative_path });
    }
    let canonical = fs::canonicalize(path).map_err(|source| StorageFileError::Io {
        operation: "resolve database file",
        source,
    })?;
    if !canonical.starts_with(root) {
        return Err(StorageFileError::PathEscapesDatabaseRoot { relative_path });
    }
    Ok(())
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
