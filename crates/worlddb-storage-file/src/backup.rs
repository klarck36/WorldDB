//! Exact, snapshot-consistent filesystem backups and independent verification.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use worlddb_core::{DatabaseId, DomainId, Revision};

use crate::compaction::{CompactionError, DurableBackupPin, SegmentPin};
use crate::manifest::{Manifest, ManifestError, ManifestSegmentKind, ManifestStore, manifest_path};
use crate::segment::segment_path;
use crate::wal::segment_file_name;
use crate::{
    CompactionManager, DatabaseLayout, SegmentId, StorageFileError, StorageVerifier,
    StorageVerifyError, StorageVerifyReport, WalCommitHash, WalError, WalPrepareLog,
    WriterLockError,
};

const BACKUP_MANIFEST_FILE: &str = "EXACT_BACKUP";
const BACKUP_INCOMPLETE_FILE: &str = "EXACT_BACKUP.INCOMPLETE";
const BACKUP_MAGIC: &[u8; 8] = b"WDBBKP\0\x01";
const INVENTORY_CONTEXT: &[u8] = b"worlddb.exact-backup.inventory.v1\0";
const AUTH_CONTEXT: &[u8] = b"worlddb.exact-backup.auth.v1\0";
const BACKUP_MANIFEST_LIMIT: usize = 64 * 1024 * 1024;
const BACKUP_ITEM_LIMIT: usize = 1_000_000;
const ITEM_PATH_LIMIT: usize = 512;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const INCOMPLETE_MARKER: &[u8] = b"WorldDB ExactDatabaseBackup is incomplete.\n";

/// Why an exact backup could not be created or independently verified.
#[derive(Debug)]
pub enum BackupError {
    /// A filesystem operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// The source layout could not be opened or initialized.
    StorageFile(StorageFileError),
    /// The source writer lock could not be acquired.
    WriterLock(WriterLockError),
    /// WAL verification or checkpointing failed.
    Wal(WalError),
    /// Manifest verification failed.
    Manifest(ManifestError),
    /// Segment pinning failed.
    Compaction(CompactionError),
    /// An independent storage verification pass could not finish.
    StorageVerify(StorageVerifyError),
    /// The source database has no durable database identity.
    DatabaseIdentityMissing,
    /// The source storage is not a fully clean, materialized snapshot.
    SourceSnapshotNotClean,
    /// The current manifest and verified WAL head do not identify one snapshot.
    SourceSnapshotMismatch,
    /// The selected target exists, is an ancestor of the source, or is inside it.
    InvalidTarget,
    /// The selected target already exists and will not be overwritten.
    TargetAlreadyExists,
    /// The parent of the target directory does not exist or is not a directory.
    TargetParentMissing,
    /// The target has no complete backup manifest or retains its incomplete marker.
    IncompleteTarget,
    /// The backup manifest is malformed, noncanonical, or unsupported.
    InvalidBackupManifest,
    /// An item digest, manifest digest, or inventory digest did not verify.
    IntegrityMismatch,
    /// The target file set is not exactly the declared backup inventory.
    InventoryMismatch,
    /// The backup identity, revision, manifest, or format binding disagrees with storage.
    TargetSnapshotMismatch,
    /// Storage Verify found a target that is not clean.
    TargetNotClean(Box<StorageVerifyReport>),
    /// A requested MAC key identifier is not a valid bounded ASCII symbol.
    InvalidKeyId,
    /// A backup item path is not a safe canonical relative path.
    InvalidItemPath,
    /// The requested backup exceeds one of its explicit resource limits.
    ResourceLimit,
    /// The process could not reserve bounded inventory memory.
    AllocationFailed,
}

impl fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::StorageFile(error) => write!(formatter, "storage layout error: {error}"),
            Self::WriterLock(error) => write!(formatter, "database lock error: {error}"),
            Self::Wal(error) => write!(formatter, "WAL backup checkpoint failed: {error}"),
            Self::Manifest(error) => write!(formatter, "manifest backup failed: {error}"),
            Self::Compaction(error) => write!(formatter, "backup snapshot pin failed: {error}"),
            Self::StorageVerify(error) => write!(formatter, "storage verification failed: {error}"),
            Self::DatabaseIdentityMissing => {
                formatter.write_str("source database has no persistent DatabaseId")
            }
            Self::SourceSnapshotNotClean => {
                formatter.write_str("source database does not have a clean verified snapshot")
            }
            Self::SourceSnapshotMismatch => {
                formatter.write_str("source manifest does not match the verified WAL head")
            }
            Self::InvalidTarget => {
                formatter.write_str("backup target overlaps the source database")
            }
            Self::TargetAlreadyExists => {
                formatter.write_str("backup target already exists and will not be overwritten")
            }
            Self::TargetParentMissing => {
                formatter.write_str("backup target parent directory does not exist")
            }
            Self::IncompleteTarget => {
                formatter.write_str("backup target is incomplete or lacks its final manifest")
            }
            Self::InvalidBackupManifest => {
                formatter.write_str("backup manifest is malformed or unsupported")
            }
            Self::IntegrityMismatch => {
                formatter.write_str("backup item, inventory, or manifest digest does not match")
            }
            Self::InventoryMismatch => {
                formatter.write_str("backup target files differ from the declared inventory")
            }
            Self::TargetSnapshotMismatch => {
                formatter.write_str("backup metadata disagrees with its verified storage snapshot")
            }
            Self::TargetNotClean(report) => {
                write!(formatter, "backup target is not clean: {report}")
            }
            Self::InvalidKeyId => formatter.write_str("backup MAC key ID is invalid"),
            Self::InvalidItemPath => formatter.write_str("backup item path is not canonical"),
            Self::ResourceLimit => formatter.write_str("backup exceeds a bounded resource limit"),
            Self::AllocationFailed => formatter.write_str("backup inventory allocation failed"),
        }
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::StorageFile(error) => Some(error),
            Self::WriterLock(error) => Some(error),
            Self::Wal(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::Compaction(error) => Some(error),
            Self::StorageVerify(error) => Some(error),
            _ => None,
        }
    }
}

/// BLAKE3 keyed-MAC material used to bind an exact backup to a named key.
pub struct BackupMacKey {
    key_id: String,
    key: [u8; 32],
}

impl BackupMacKey {
    /// Creates a key with a stable ASCII identity and 256 bits of secret key material.
    pub fn new(key_id: impl Into<String>, key: [u8; 32]) -> Result<Self, BackupError> {
        let key_id = key_id.into();
        if key_id.is_empty()
            || key_id.len() > 128
            || !key_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(BackupError::InvalidKeyId);
        }
        Ok(Self { key_id, key })
    }

    /// Stable identifier recorded in the backup manifest; the secret is never stored there.
    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

impl fmt::Debug for BackupMacKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackupMacKey")
            .field("key_id", &self.key_id)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// Authenticity result, reported separately from backup byte integrity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupAuthenticity {
    /// The manifest contains only digests and makes no origin claim.
    NotClaimed,
    /// A MAC is present but no key was supplied for verification.
    ClaimedButUnverified {
        algorithm: &'static str,
        key_id: String,
    },
    /// The claimed MAC verified with the supplied key.
    Verified {
        algorithm: &'static str,
        key_id: String,
    },
    /// The supplied key ID differs from the identity claimed by the manifest.
    WrongKey {
        expected_key_id: String,
        provided_key_id: String,
    },
    /// The key ID matches, but the supplied key does not verify the MAC bytes.
    InvalidMac { key_id: String },
}

/// Progress emitted after the snapshot is pinned and as target files are copied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupProgressEvent {
    /// The exact source revision and immutable manifest inventory are pinned.
    SnapshotPinned {
        database_id: DatabaseId,
        revision: Revision,
        item_count: usize,
    },
    /// One declared inventory item was copied and hashed.
    ItemCopied {
        completed: usize,
        total: usize,
        path: String,
    },
}

/// Result of independent exact-backup verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupVerification {
    database_id: DatabaseId,
    revision: Revision,
    commit_hash: WalCommitHash,
    manifest_generation: Option<u64>,
    item_count: usize,
    inventory_digest: [u8; 32],
    manifest_digest: [u8; 32],
    authenticity: BackupAuthenticity,
    storage: StorageVerifyReport,
}

impl BackupVerification {
    /// Stable identity of the backed-up database.
    #[must_use]
    pub const fn database_id(&self) -> DatabaseId {
        self.database_id
    }

    /// Exact logical revision included by the backup.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Commit-chain hash at the backed-up revision.
    #[must_use]
    pub const fn commit_hash(&self) -> WalCommitHash {
        self.commit_hash
    }

    /// Manifest generation included by the backup, or `None` for genesis.
    #[must_use]
    pub const fn manifest_generation(&self) -> Option<u64> {
        self.manifest_generation
    }

    /// Number of files in the exact database inventory.
    #[must_use]
    pub const fn item_count(&self) -> usize {
        self.item_count
    }

    /// Digest over the canonically ordered item paths, lengths, kinds, and digests.
    #[must_use]
    pub const fn inventory_digest(&self) -> &[u8; 32] {
        &self.inventory_digest
    }

    /// Digest over the complete backup metadata and inventory payload.
    #[must_use]
    pub const fn manifest_digest(&self) -> &[u8; 32] {
        &self.manifest_digest
    }

    /// Optional keyed authenticity result; it does not replace inventory integrity.
    #[must_use]
    pub const fn authenticity(&self) -> &BackupAuthenticity {
        &self.authenticity
    }

    /// Independent WorldDB storage verification of the target snapshot.
    #[must_use]
    pub const fn storage_report(&self) -> &StorageVerifyReport {
        &self.storage
    }
}

/// Creates exact backups for one opened filesystem database.
#[derive(Clone, Debug)]
pub struct ExactBackupManager {
    source: DatabaseLayout,
}

impl ExactBackupManager {
    /// Binds backup creation to one validated source database layout.
    #[must_use]
    pub const fn new(source: DatabaseLayout) -> Self {
        Self { source }
    }

    /// Creates and verifies an `ExactDatabaseBackup` with an optional BLAKE3 MAC.
    pub fn create_exact_backup(
        &self,
        target: impl AsRef<Path>,
        key: Option<&BackupMacKey>,
    ) -> Result<BackupVerification, BackupError> {
        self.create_exact_backup_with_progress(target, key, |_| {})
    }

    /// Creates an exact backup and emits progress after pinning and each item copy.
    pub fn create_exact_backup_with_progress(
        &self,
        target: impl AsRef<Path>,
        key: Option<&BackupMacKey>,
        mut progress: impl FnMut(BackupProgressEvent),
    ) -> Result<BackupVerification, BackupError> {
        let target_root = normalize_target(target.as_ref(), self.source.root())?;
        let snapshot = self.capture_snapshot()?;
        let target_layout =
            DatabaseLayout::create(&target_root).map_err(BackupError::StorageFile)?;
        write_incomplete_marker(target_layout.root())?;

        progress(BackupProgressEvent::SnapshotPinned {
            database_id: snapshot.metadata.database_id,
            revision: snapshot.metadata.revision,
            item_count: snapshot.sources.len(),
        });

        let total = snapshot.sources.len();
        let mut items = Vec::new();
        items
            .try_reserve_exact(total)
            .map_err(|_| BackupError::AllocationFailed)?;
        for (index, item) in snapshot.sources.iter().enumerate() {
            let copied = copy_source_item(item, self.source.root(), target_layout.root())?;
            let completed = index.checked_add(1).ok_or(BackupError::ResourceLimit)?;
            progress(BackupProgressEvent::ItemCopied {
                completed,
                total,
                path: copied.path.clone(),
            });
            items.push(copied);
        }
        items.sort_by(|left, right| left.path.cmp(&right.path));

        let manifest = BackupManifest::new(snapshot.metadata, items)?;
        let manifest_bytes = manifest.encode(key)?;
        write_new_file(
            &target_layout.root().join(BACKUP_MANIFEST_FILE),
            &manifest_bytes,
            "write exact backup manifest",
        )?;
        crate::manifest::sync_directory(target_layout.root()).map_err(|source| {
            BackupError::Io {
                operation: "sync exact backup manifest directory",
                source,
            }
        })?;

        let _prepublication_check = verify_exact_backup_internal(target_layout.root(), key, true)?;
        fs::remove_file(target_layout.root().join(BACKUP_INCOMPLETE_FILE)).map_err(|source| {
            BackupError::Io {
                operation: "publish exact backup completion",
                source,
            }
        })?;
        crate::manifest::sync_directory(target_layout.root()).map_err(|source| {
            BackupError::Io {
                operation: "sync exact backup completion",
                source,
            }
        })?;

        let verification = verify_exact_backup(target_layout.root(), key)?;
        if key.is_some()
            && !matches!(
                verification.authenticity,
                BackupAuthenticity::Verified { .. }
            )
        {
            return Err(BackupError::IntegrityMismatch);
        }
        drop(snapshot);
        Ok(verification)
    }

    fn capture_snapshot(&self) -> Result<PinnedSnapshot, BackupError> {
        let database_id = self
            .source
            .database_id()
            .ok_or(BackupError::DatabaseIdentityMissing)?;
        let lock = self
            .source
            .try_writer_lock()
            .map_err(BackupError::WriterLock)?;
        let storage_report = StorageVerifier::new(self.source.clone())
            .verify(&lock)
            .map_err(BackupError::StorageVerify)?;
        if !storage_report.is_clean() {
            return Err(BackupError::SourceSnapshotNotClean);
        }

        let wal = WalPrepareLog::new(&self.source);
        let head = wal.commit_head(&lock).map_err(BackupError::Wal)?;
        if head.revision() != storage_report.safe_revision() {
            return Err(BackupError::SourceSnapshotMismatch);
        }
        let manifest = ManifestStore::new(self.source.clone())
            .read_current()
            .map_err(BackupError::Manifest)?;
        match manifest.as_ref() {
            Some(manifest)
                if manifest.revision() == head.revision()
                    && manifest.commit_hash() == head.commit_hash() => {}
            None if head.revision() == Revision::GENESIS => {}
            _ => return Err(BackupError::SourceSnapshotMismatch),
        }

        let wal_checkpoint = wal
            .checkpoint(&lock, head.revision())
            .map_err(BackupError::Wal)?;
        if wal_checkpoint.commit_hash() != head.commit_hash() {
            return Err(BackupError::SourceSnapshotMismatch);
        }

        let mut sources = Vec::new();
        let manifest_segments = manifest.as_ref().map_or(&[][..], Manifest::segments);
        let compaction = CompactionManager::new(self.source.clone());
        let (segment_pins, durable_pin) = compaction
            .pin_backup_snapshot(&lock, manifest_segments)
            .map_err(BackupError::Compaction)?;

        sources
            .try_reserve_exact(
                3_usize
                    .checked_add(manifest_segments.len())
                    .and_then(|count| count.checked_add(wal_checkpoint.segments().len()))
                    .ok_or(BackupError::ResourceLimit)?,
            )
            .map_err(|_| BackupError::AllocationFailed)?;
        sources.push(inline_source_item(
            "FORMAT",
            read_small_file(&self.source.format_file(), self.source.root(), "FORMAT")?,
        )?);
        let database_id_bytes = read_small_file(
            &self.source.database_id_file(),
            self.source.root(),
            "DATABASE_ID",
        )?;
        if database_id_bytes.as_slice() != database_id.to_bytes().as_slice() {
            return Err(BackupError::SourceSnapshotMismatch);
        }
        sources.push(inline_source_item("DATABASE_ID", database_id_bytes)?);
        let manifest_generation = manifest.as_ref().map(Manifest::generation);
        if let Some(manifest) = &manifest {
            let current_bytes =
                read_small_file(&self.source.current_file(), self.source.root(), "CURRENT")?;
            sources.push(inline_source_item("CURRENT", current_bytes)?);
            let manifest_file_path =
                manifest_path(&self.source.manifests_directory(), manifest.generation());
            sources.push(file_source_item(
                manifest_relative_path(manifest.generation()),
                manifest_file_path.clone(),
                self.source.root(),
                file_length_from_path(&manifest_file_path, self.source.root())?,
            )?);
            for reference in manifest_segments {
                let (relative_path, path) = match reference.kind() {
                    ManifestSegmentKind::History => (
                        segment_relative_path(reference.id()),
                        segment_path(&self.source.segments_directory(), reference.id()),
                    ),
                    ManifestSegmentKind::SecurityPolicy => (
                        security_segment_relative_path(reference.id()),
                        segment_path(&self.source.security_segments_directory(), reference.id()),
                    ),
                };
                let length = file_length_from_path(&path, self.source.root())?;
                sources.push(file_source_item(
                    relative_path,
                    path,
                    self.source.root(),
                    length,
                )?);
            }
        }
        for checkpoint_segment in wal_checkpoint.segments() {
            let path = self
                .source
                .wal_directory()
                .join(segment_file_name(checkpoint_segment.sequence()));
            sources.push(file_source_item(
                format!("wal/{}", segment_file_name(checkpoint_segment.sequence())),
                path,
                self.source.root(),
                checkpoint_segment.byte_length(),
            )?);
        }
        sources.sort_by(|left, right| left.path.cmp(&right.path));

        let metadata = BackupMetadata {
            database_id,
            revision: head.revision(),
            commit_hash: head.commit_hash(),
            manifest_generation,
            required_format_flags: self.source.format_capabilities().required_flags(),
            optional_format_flags: self.source.format_capabilities().optional_flags(),
        };
        drop(lock);
        Ok(PinnedSnapshot {
            metadata,
            sources,
            _segment_pins: segment_pins,
            _durable_pin: durable_pin,
        })
    }
}

/// Independently verifies a completed exact-backup directory.
pub fn verify_exact_backup(
    target: impl AsRef<Path>,
    key: Option<&BackupMacKey>,
) -> Result<BackupVerification, BackupError> {
    verify_exact_backup_internal(target.as_ref(), key, false)
}

struct PinnedSnapshot {
    metadata: BackupMetadata,
    sources: Vec<SourceItem>,
    _segment_pins: Vec<SegmentPin>,
    _durable_pin: DurableBackupPin,
}

#[derive(Clone, Copy)]
struct BackupMetadata {
    database_id: DatabaseId,
    revision: Revision,
    commit_hash: WalCommitHash,
    manifest_generation: Option<u64>,
    required_format_flags: u64,
    optional_format_flags: u64,
}

enum CopyInput {
    File(PathBuf),
    Inline(Vec<u8>),
}

struct SourceItem {
    path: String,
    length: u64,
    input: CopyInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum BackupItemKind {
    Format = 1,
    DatabaseId = 2,
    Current = 3,
    Manifest = 4,
    Wal = 5,
    HistorySegment = 6,
    SecuritySegment = 7,
}

impl BackupItemKind {
    fn from_path(path: &str) -> Result<Self, BackupError> {
        match path {
            "FORMAT" => Ok(Self::Format),
            "DATABASE_ID" => Ok(Self::DatabaseId),
            "CURRENT" => Ok(Self::Current),
            value if is_canonical_manifest_path(value) => Ok(Self::Manifest),
            value if is_canonical_wal_path(value) => Ok(Self::Wal),
            value if is_canonical_segment_path(value, "segments/") => Ok(Self::HistorySegment),
            value if is_canonical_segment_path(value, "security/segments/") => {
                Ok(Self::SecuritySegment)
            }
            _ => Err(BackupError::InvalidItemPath),
        }
    }

    fn from_tag(tag: u8) -> Result<Self, BackupError> {
        match tag {
            1 => Ok(Self::Format),
            2 => Ok(Self::DatabaseId),
            3 => Ok(Self::Current),
            4 => Ok(Self::Manifest),
            5 => Ok(Self::Wal),
            6 => Ok(Self::HistorySegment),
            7 => Ok(Self::SecuritySegment),
            _ => Err(BackupError::InvalidBackupManifest),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BackupItem {
    path: String,
    kind: BackupItemKind,
    length: u64,
    digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BackupMacReceipt {
    algorithm: u8,
    key_id: String,
    tag: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BackupManifest {
    metadata: BackupMetadataOwned,
    items: Vec<BackupItem>,
    inventory_digest: [u8; 32],
    manifest_digest: [u8; 32],
    mac: Option<BackupMacReceipt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackupMetadataOwned {
    database_id: DatabaseId,
    revision: Revision,
    commit_hash: WalCommitHash,
    manifest_generation: Option<u64>,
    required_format_flags: u64,
    optional_format_flags: u64,
}

impl From<BackupMetadata> for BackupMetadataOwned {
    fn from(value: BackupMetadata) -> Self {
        Self {
            database_id: value.database_id,
            revision: value.revision,
            commit_hash: value.commit_hash,
            manifest_generation: value.manifest_generation,
            required_format_flags: value.required_format_flags,
            optional_format_flags: value.optional_format_flags,
        }
    }
}

impl BackupManifest {
    fn new(metadata: BackupMetadata, items: Vec<BackupItem>) -> Result<Self, BackupError> {
        validate_items(&items)?;
        let metadata = BackupMetadataOwned::from(metadata);
        let inventory_digest = calculate_inventory_digest(&items)?;
        let mut manifest = Self {
            metadata,
            items,
            inventory_digest,
            manifest_digest: [0; 32],
            mac: None,
        };
        let unsigned = manifest.encode_unsigned()?;
        manifest.manifest_digest = *blake3::hash(&unsigned).as_bytes();
        Ok(manifest)
    }

    fn encode(&self, key: Option<&BackupMacKey>) -> Result<Vec<u8>, BackupError> {
        let unsigned = self.encode_unsigned()?;
        let manifest_digest = *blake3::hash(&unsigned).as_bytes();
        if manifest_digest != self.manifest_digest {
            return Err(BackupError::IntegrityMismatch);
        }
        let encoded_size = unsigned
            .len()
            .checked_add(32 + 1)
            .and_then(|size| size.checked_add(key.map_or(0, |key| 1 + 2 + key.key_id.len() + 32)))
            .ok_or(BackupError::ResourceLimit)?;
        if encoded_size > BACKUP_MANIFEST_LIMIT {
            return Err(BackupError::ResourceLimit);
        }
        let mac = key.map(|key| BackupMacReceipt {
            algorithm: 1,
            key_id: key.key_id.clone(),
            tag: calculate_mac(&key.key, &unsigned, &manifest_digest),
        });
        let mut bytes = unsigned;
        bytes.extend_from_slice(&manifest_digest);
        match mac {
            Some(receipt) => {
                bytes.push(1);
                bytes.push(receipt.algorithm);
                push_u16(
                    &mut bytes,
                    u16::try_from(receipt.key_id.len()).map_err(|_| BackupError::ResourceLimit)?,
                );
                bytes.extend_from_slice(receipt.key_id.as_bytes());
                bytes.extend_from_slice(&receipt.tag);
            }
            None => bytes.push(0),
        }
        if bytes.len() > BACKUP_MANIFEST_LIMIT {
            return Err(BackupError::ResourceLimit);
        }
        Ok(bytes)
    }

    fn encode_unsigned(&self) -> Result<Vec<u8>, BackupError> {
        validate_items(&self.items)?;
        let expected_inventory = calculate_inventory_digest(&self.items)?;
        if expected_inventory != self.inventory_digest {
            return Err(BackupError::IntegrityMismatch);
        }
        let fixed_header = if self.metadata.manifest_generation.is_some() {
            97_usize
        } else {
            89_usize
        };
        let encoded_size = self.items.iter().try_fold(
            fixed_header
                .checked_add(32)
                .ok_or(BackupError::ResourceLimit)?,
            |size, item| {
                size.checked_add(43)
                    .and_then(|size| size.checked_add(item.path.len()))
                    .ok_or(BackupError::ResourceLimit)
            },
        )?;
        if encoded_size > BACKUP_MANIFEST_LIMIT {
            return Err(BackupError::ResourceLimit);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(encoded_size)
            .map_err(|_| BackupError::AllocationFailed)?;
        bytes.extend_from_slice(BACKUP_MAGIC);
        bytes.push(1); // ExactDatabaseBackup profile.
        bytes.push(0); // audit_scope=Excluded.
        push_u16(&mut bytes, 1); // Backup manifest schema version.
        push_u64(&mut bytes, self.metadata.required_format_flags);
        push_u64(&mut bytes, self.metadata.optional_format_flags);
        bytes.extend_from_slice(&self.metadata.database_id.to_bytes());
        push_u64(&mut bytes, self.metadata.revision.value());
        bytes.extend_from_slice(self.metadata.commit_hash.as_bytes());
        match self.metadata.manifest_generation {
            Some(generation) => {
                bytes.push(1);
                push_u64(&mut bytes, generation);
            }
            None => bytes.push(0),
        }
        push_u32(
            &mut bytes,
            u32::try_from(self.items.len()).map_err(|_| BackupError::ResourceLimit)?,
        );
        for item in &self.items {
            let path_bytes = item.path.as_bytes();
            push_u16(
                &mut bytes,
                u16::try_from(path_bytes.len()).map_err(|_| BackupError::ResourceLimit)?,
            );
            bytes.push(item.kind as u8);
            push_u64(&mut bytes, item.length);
            bytes.extend_from_slice(path_bytes);
            bytes.extend_from_slice(&item.digest);
        }
        bytes.extend_from_slice(&self.inventory_digest);
        if bytes.len() != encoded_size {
            return Err(BackupError::ResourceLimit);
        }
        Ok(bytes)
    }

    fn decode(bytes: &[u8]) -> Result<Self, BackupError> {
        if bytes.len() > BACKUP_MANIFEST_LIMIT {
            return Err(BackupError::ResourceLimit);
        }
        let mut cursor = ByteCursor::new(bytes);
        if cursor.take(8)? != BACKUP_MAGIC {
            return Err(BackupError::InvalidBackupManifest);
        }
        if cursor.u8()? != 1 || cursor.u8()? != 0 || cursor.u16()? != 1 {
            return Err(BackupError::InvalidBackupManifest);
        }
        let required_format_flags = cursor.u64()?;
        let optional_format_flags = cursor.u64()?;
        let database_id = DatabaseId::try_from_bytes(cursor.array16()?)
            .map_err(|_| BackupError::InvalidBackupManifest)?;
        let revision =
            Revision::new(cursor.u64()?).map_err(|_| BackupError::InvalidBackupManifest)?;
        let commit_hash = WalCommitHash::from_bytes(cursor.array32()?);
        let manifest_generation = match cursor.u8()? {
            0 => None,
            1 => Some(cursor.u64()?),
            _ => return Err(BackupError::InvalidBackupManifest),
        };
        let item_count = usize::try_from(cursor.u32()?).map_err(|_| BackupError::ResourceLimit)?;
        if item_count > BACKUP_ITEM_LIMIT {
            return Err(BackupError::ResourceLimit);
        }
        let mut items = Vec::new();
        items
            .try_reserve_exact(item_count)
            .map_err(|_| BackupError::AllocationFailed)?;
        for _ in 0..item_count {
            let path_length = usize::from(cursor.u16()?);
            if path_length == 0 || path_length > ITEM_PATH_LIMIT {
                return Err(BackupError::InvalidItemPath);
            }
            let kind = BackupItemKind::from_tag(cursor.u8()?)?;
            let length = cursor.u64()?;
            let path = std::str::from_utf8(cursor.take(path_length)?)
                .map_err(|_| BackupError::InvalidItemPath)?
                .to_owned();
            let digest = cursor.array32()?;
            if BackupItemKind::from_path(&path)? != kind {
                return Err(BackupError::InvalidItemPath);
            }
            items.push(BackupItem {
                path,
                kind,
                length,
                digest,
            });
        }
        let inventory_digest = cursor.array32()?;
        let unsigned_end = cursor.position();
        let manifest_digest = cursor.array32()?;
        let unsigned = bytes
            .get(..unsigned_end)
            .ok_or(BackupError::InvalidBackupManifest)?;
        if *blake3::hash(unsigned).as_bytes() != manifest_digest
            || calculate_inventory_digest(&items)? != inventory_digest
        {
            return Err(BackupError::IntegrityMismatch);
        }
        validate_items(&items)?;
        let mac = match cursor.u8()? {
            0 => None,
            1 => {
                let algorithm = cursor.u8()?;
                if algorithm != 1 {
                    return Err(BackupError::InvalidBackupManifest);
                }
                let key_length = usize::from(cursor.u16()?);
                if key_length == 0 || key_length > 128 {
                    return Err(BackupError::InvalidKeyId);
                }
                let key_id = std::str::from_utf8(cursor.take(key_length)?)
                    .map_err(|_| BackupError::InvalidKeyId)?
                    .to_owned();
                validate_key_id(&key_id)?;
                Some(BackupMacReceipt {
                    algorithm,
                    key_id,
                    tag: cursor.array32()?,
                })
            }
            _ => return Err(BackupError::InvalidBackupManifest),
        };
        if !cursor.is_at_end() {
            return Err(BackupError::InvalidBackupManifest);
        }
        let mut manifest = Self {
            metadata: BackupMetadataOwned {
                database_id,
                revision,
                commit_hash,
                manifest_generation,
                required_format_flags,
                optional_format_flags,
            },
            items,
            inventory_digest,
            manifest_digest,
            mac,
        };
        let canonical_unsigned = manifest.encode_unsigned()?;
        if canonical_unsigned != unsigned {
            return Err(BackupError::InvalidBackupManifest);
        }
        manifest.manifest_digest = *blake3::hash(&canonical_unsigned).as_bytes();
        Ok(manifest)
    }
}

fn verify_exact_backup_internal(
    target: &Path,
    key: Option<&BackupMacKey>,
    allow_incomplete_marker: bool,
) -> Result<BackupVerification, BackupError> {
    let target_root = fs::canonicalize(target).map_err(|source| BackupError::Io {
        operation: "resolve exact backup target",
        source,
    })?;
    if !target_root.is_dir() {
        return Err(BackupError::InvalidTarget);
    }
    let marker_path = target_root.join(BACKUP_INCOMPLETE_FILE);
    if marker_path.exists() && !allow_incomplete_marker {
        return Err(BackupError::IncompleteTarget);
    }
    if !target_root.join(BACKUP_MANIFEST_FILE).is_file() {
        return Err(BackupError::IncompleteTarget);
    }
    let manifest_bytes = read_bounded_file(
        &target_root.join(BACKUP_MANIFEST_FILE),
        BACKUP_MANIFEST_LIMIT,
        &target_root,
    )?;
    let manifest = BackupManifest::decode(&manifest_bytes)?;
    verify_item_bytes(&target_root, &manifest.items)?;
    if collect_target_inventory(&target_root)?
        != manifest
            .items
            .iter()
            .map(|item| item.path.clone())
            .collect::<BTreeSet<_>>()
    {
        return Err(BackupError::InventoryMismatch);
    }

    let layout = DatabaseLayout::open(&target_root).map_err(BackupError::StorageFile)?;
    if layout.database_id() != Some(manifest.metadata.database_id)
        || layout.format_capabilities().required_flags() != manifest.metadata.required_format_flags
        || layout.format_capabilities().optional_flags() != manifest.metadata.optional_format_flags
    {
        return Err(BackupError::TargetSnapshotMismatch);
    }
    let authenticity = verify_authenticity(&manifest, key);
    let lock = layout.try_writer_lock().map_err(BackupError::WriterLock)?;
    let storage = StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(BackupError::StorageVerify)?;
    if !storage.is_clean() {
        return Err(BackupError::TargetNotClean(Box::new(storage)));
    }
    let head = WalPrepareLog::new(&layout)
        .commit_head(&lock)
        .map_err(BackupError::Wal)?;
    let current = ManifestStore::new(layout)
        .read_current()
        .map_err(BackupError::Manifest)?;
    let current_matches = match (&current, manifest.metadata.manifest_generation) {
        (Some(current), Some(generation)) => {
            current.generation() == generation
                && current.revision() == manifest.metadata.revision
                && current.commit_hash() == manifest.metadata.commit_hash
        }
        (None, None) => manifest.metadata.revision == Revision::GENESIS,
        _ => false,
    };
    if !current_matches
        || head.revision() != manifest.metadata.revision
        || head.commit_hash() != manifest.metadata.commit_hash
        || storage.safe_revision() != manifest.metadata.revision
    {
        return Err(BackupError::TargetSnapshotMismatch);
    }
    drop(lock);
    Ok(BackupVerification {
        database_id: manifest.metadata.database_id,
        revision: manifest.metadata.revision,
        commit_hash: manifest.metadata.commit_hash,
        manifest_generation: manifest.metadata.manifest_generation,
        item_count: manifest.items.len(),
        inventory_digest: manifest.inventory_digest,
        manifest_digest: manifest.manifest_digest,
        authenticity,
        storage,
    })
}

fn verify_authenticity(
    manifest: &BackupManifest,
    key: Option<&BackupMacKey>,
) -> BackupAuthenticity {
    let Some(receipt) = &manifest.mac else {
        return BackupAuthenticity::NotClaimed;
    };
    let Some(key) = key else {
        return BackupAuthenticity::ClaimedButUnverified {
            algorithm: "BLAKE3-KEYED",
            key_id: receipt.key_id.clone(),
        };
    };
    if receipt.key_id != key.key_id {
        return BackupAuthenticity::WrongKey {
            expected_key_id: receipt.key_id.clone(),
            provided_key_id: key.key_id.clone(),
        };
    }
    let unsigned = match manifest.encode_unsigned() {
        Ok(bytes) => bytes,
        Err(_) => {
            return BackupAuthenticity::InvalidMac {
                key_id: receipt.key_id.clone(),
            };
        }
    };
    let expected = calculate_mac(&key.key, &unsigned, &manifest.manifest_digest);
    if constant_time_equal(&expected, &receipt.tag) {
        BackupAuthenticity::Verified {
            algorithm: "BLAKE3-KEYED",
            key_id: receipt.key_id.clone(),
        }
    } else {
        BackupAuthenticity::InvalidMac {
            key_id: receipt.key_id.clone(),
        }
    }
}

fn capture_source_file(
    path: String,
    file: PathBuf,
    root: &Path,
    length: u64,
) -> Result<SourceItem, BackupError> {
    validate_regular_file(&file, root)?;
    let actual_length = fs::metadata(&file)
        .map_err(|source| BackupError::Io {
            operation: "inspect source backup item",
            source,
        })?
        .len();
    if actual_length < length {
        return Err(BackupError::IntegrityMismatch);
    }
    validate_item_path(&path)?;
    Ok(SourceItem {
        path,
        length,
        input: CopyInput::File(file),
    })
}

fn file_source_item(
    path: String,
    file: PathBuf,
    root: &Path,
    length: u64,
) -> Result<SourceItem, BackupError> {
    capture_source_file(path, file, root, length)
}

fn inline_source_item(path: &str, bytes: Vec<u8>) -> Result<SourceItem, BackupError> {
    let length = u64::try_from(bytes.len()).map_err(|_| BackupError::ResourceLimit)?;
    Ok(SourceItem {
        path: path.to_owned(),
        length,
        input: CopyInput::Inline(bytes),
    })
}

fn read_small_file(path: &Path, root: &Path, label: &'static str) -> Result<Vec<u8>, BackupError> {
    validate_regular_file(path, root)?;
    let metadata = fs::metadata(path).map_err(|source| BackupError::Io {
        operation: "inspect fixed backup metadata file",
        source,
    })?;
    let length = usize::try_from(metadata.len()).map_err(|_| BackupError::ResourceLimit)?;
    if length > 1024 * 1024 {
        return Err(BackupError::ResourceLimit);
    }
    let bytes = fs::read(path).map_err(|source| BackupError::Io {
        operation: label,
        source,
    })?;
    if bytes.len() != length {
        return Err(BackupError::IntegrityMismatch);
    }
    Ok(bytes)
}

fn file_length_from_path(path: &Path, root: &Path) -> Result<u64, BackupError> {
    validate_regular_file(path, root)?;
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|source| BackupError::Io {
            operation: "inspect exact backup source item",
            source,
        })
}

fn copy_source_item(
    source: &SourceItem,
    source_root: &Path,
    target_root: &Path,
) -> Result<BackupItem, BackupError> {
    let destination = join_item_path(target_root, &source.path)?;
    validate_destination_parent(target_root, &destination)?;
    let kind = BackupItemKind::from_path(&source.path)?;
    let overwrite_created_layout_file =
        matches!(kind, BackupItemKind::Format | BackupItemKind::DatabaseId);
    if overwrite_created_layout_file && destination.exists() {
        validate_regular_file(&destination, target_root)?;
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if overwrite_created_layout_file {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut output = options
        .open(&destination)
        .map_err(|source| BackupError::Io {
            operation: "create exact backup item",
            source,
        })?;
    let mut input: Box<dyn Read + '_> = match &source.input {
        CopyInput::File(path) => {
            validate_regular_file(path, source_root)?;
            Box::new(File::open(path).map_err(|source| BackupError::Io {
                operation: "open exact backup source item",
                source,
            })?)
        }
        CopyInput::Inline(bytes) => Box::new(io::Cursor::new(bytes.as_slice())),
    };
    let mut hasher = blake3::Hasher::new();
    let mut remaining = source.length;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        let chunk_length = usize::try_from(remaining.min(COPY_BUFFER_BYTES as u64))
            .map_err(|_| BackupError::ResourceLimit)?;
        let chunk = buffer
            .get_mut(..chunk_length)
            .ok_or(BackupError::ResourceLimit)?;
        let read = input.read(chunk).map_err(|source| BackupError::Io {
            operation: "read exact backup source item",
            source,
        })?;
        if read == 0 {
            return Err(BackupError::IntegrityMismatch);
        }
        let bytes = buffer.get(..read).ok_or(BackupError::ResourceLimit)?;
        output.write_all(bytes).map_err(|source| BackupError::Io {
            operation: "write exact backup item",
            source,
        })?;
        hasher.update(bytes);
        remaining = remaining
            .checked_sub(u64::try_from(read).map_err(|_| BackupError::ResourceLimit)?)
            .ok_or(BackupError::ResourceLimit)?;
    }
    output.sync_all().map_err(|source| BackupError::Io {
        operation: "sync exact backup item",
        source,
    })?;
    Ok(BackupItem {
        path: source.path.clone(),
        kind,
        length: source.length,
        digest: *hasher.finalize().as_bytes(),
    })
}

fn normalize_target(target: &Path, source_root: &Path) -> Result<PathBuf, BackupError> {
    let file_name = target.file_name().ok_or(BackupError::InvalidTarget)?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(BackupError::TargetParentMissing);
    }
    let canonical_parent = fs::canonicalize(parent).map_err(|source| BackupError::Io {
        operation: "resolve backup target parent",
        source,
    })?;
    let candidate = canonical_parent.join(file_name);
    if candidate.exists() {
        return Err(BackupError::TargetAlreadyExists);
    }
    if candidate.starts_with(source_root) || source_root.starts_with(&candidate) {
        return Err(BackupError::InvalidTarget);
    }
    Ok(candidate)
}

fn validate_regular_file(path: &Path, root: &Path) -> Result<(), BackupError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| BackupError::Io {
        operation: "inspect exact backup file",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BackupError::InventoryMismatch);
    }
    let canonical = fs::canonicalize(path).map_err(|source| BackupError::Io {
        operation: "resolve exact backup file",
        source,
    })?;
    if !canonical.starts_with(root) {
        return Err(BackupError::InventoryMismatch);
    }
    Ok(())
}

fn validate_destination_parent(root: &Path, destination: &Path) -> Result<(), BackupError> {
    let parent = destination.parent().ok_or(BackupError::InvalidItemPath)?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| BackupError::InvalidItemPath)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(BackupError::InvalidItemPath);
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).map_err(|source| BackupError::Io {
            operation: "inspect exact backup destination directory",
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(BackupError::InventoryMismatch);
        }
        let canonical = fs::canonicalize(&current).map_err(|source| BackupError::Io {
            operation: "resolve exact backup destination directory",
            source,
        })?;
        if !canonical.starts_with(root) {
            return Err(BackupError::InventoryMismatch);
        }
    }
    Ok(())
}

fn write_incomplete_marker(root: &Path) -> Result<(), BackupError> {
    write_new_file(
        &root.join(BACKUP_INCOMPLETE_FILE),
        INCOMPLETE_MARKER,
        "write incomplete-backup marker",
    )
}

fn write_new_file(path: &Path, bytes: &[u8], operation: &'static str) -> Result<(), BackupError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| BackupError::Io { operation, source })?;
    file.write_all(bytes)
        .map_err(|source| BackupError::Io { operation, source })?;
    file.sync_all()
        .map_err(|source| BackupError::Io { operation, source })
}

fn validate_items(items: &[BackupItem]) -> Result<(), BackupError> {
    if items.len() > BACKUP_ITEM_LIMIT {
        return Err(BackupError::ResourceLimit);
    }
    let mut previous: Option<&str> = None;
    for item in items {
        validate_item_path(&item.path)?;
        if BackupItemKind::from_path(&item.path)? != item.kind
            || previous.is_some_and(|path| path >= item.path.as_str())
        {
            return Err(BackupError::InvalidBackupManifest);
        }
        previous = Some(&item.path);
    }
    Ok(())
}

fn validate_item_path(path: &str) -> Result<(), BackupError> {
    if path.is_empty()
        || path.len() > ITEM_PATH_LIMIT
        || !path.is_ascii()
        || path.contains('\\')
        || path.starts_with('/')
        || path.ends_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(BackupError::InvalidItemPath);
    }
    Ok(())
}

fn calculate_inventory_digest(items: &[BackupItem]) -> Result<[u8; 32], BackupError> {
    validate_items(items)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(INVENTORY_CONTEXT);
    for item in items {
        let path_length = u16::try_from(item.path.len()).map_err(|_| BackupError::ResourceLimit)?;
        hasher.update(&path_length.to_be_bytes());
        hasher.update(&[item.kind as u8]);
        hasher.update(&item.length.to_be_bytes());
        hasher.update(item.path.as_bytes());
        hasher.update(&item.digest);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn verify_item_bytes(root: &Path, items: &[BackupItem]) -> Result<(), BackupError> {
    for item in items {
        let path = join_item_path(root, &item.path)?;
        validate_regular_file(&path, root)?;
        let metadata = fs::metadata(&path).map_err(|source| BackupError::Io {
            operation: "inspect exact backup item",
            source,
        })?;
        if metadata.len() != item.length {
            return Err(BackupError::IntegrityMismatch);
        }
        let mut file = File::open(&path).map_err(|source| BackupError::Io {
            operation: "open exact backup item for verification",
            source,
        })?;
        let mut hasher = blake3::Hasher::new();
        let mut remaining = item.length;
        let mut buffer = [0_u8; COPY_BUFFER_BYTES];
        while remaining > 0 {
            let chunk_length = usize::try_from(remaining.min(COPY_BUFFER_BYTES as u64))
                .map_err(|_| BackupError::ResourceLimit)?;
            let read = file
                .read(
                    buffer
                        .get_mut(..chunk_length)
                        .ok_or(BackupError::ResourceLimit)?,
                )
                .map_err(|source| BackupError::Io {
                    operation: "read exact backup item for verification",
                    source,
                })?;
            if read == 0 {
                return Err(BackupError::IntegrityMismatch);
            }
            hasher.update(buffer.get(..read).ok_or(BackupError::ResourceLimit)?);
            remaining = remaining
                .checked_sub(u64::try_from(read).map_err(|_| BackupError::ResourceLimit)?)
                .ok_or(BackupError::ResourceLimit)?;
        }
        if *hasher.finalize().as_bytes() != item.digest {
            return Err(BackupError::IntegrityMismatch);
        }
    }
    Ok(())
}

fn collect_target_inventory(root: &Path) -> Result<BTreeSet<String>, BackupError> {
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(root).map_err(|source| BackupError::Io {
        operation: "list exact backup root",
        source,
    })? {
        let entry = entry.map_err(|source| BackupError::Io {
            operation: "read exact backup root entry",
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| BackupError::Io {
            operation: "inspect exact backup root entry",
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(BackupError::InventoryMismatch);
        }
        if metadata.is_file() {
            let name = entry.file_name();
            let name = name.to_str().ok_or(BackupError::InvalidItemPath)?;
            if matches!(name, "LOCK" | BACKUP_MANIFEST_FILE | BACKUP_INCOMPLETE_FILE) {
                continue;
            }
            actual.insert(name.to_owned());
        } else if metadata.is_dir() {
            collect_tree_inventory(root, &path, &mut actual)?;
        } else {
            return Err(BackupError::InventoryMismatch);
        }
    }
    Ok(actual)
}

fn collect_tree_inventory(
    root: &Path,
    directory: &Path,
    actual: &mut BTreeSet<String>,
) -> Result<(), BackupError> {
    for entry in fs::read_dir(directory).map_err(|source| BackupError::Io {
        operation: "list exact backup data directory",
        source,
    })? {
        let entry = entry.map_err(|source| BackupError::Io {
            operation: "read exact backup data entry",
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| BackupError::Io {
            operation: "inspect exact backup data entry",
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(BackupError::InventoryMismatch);
        }
        if metadata.is_dir() {
            collect_tree_inventory(root, &path, actual)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| BackupError::InventoryMismatch)?;
            let mut parts = Vec::new();
            for component in relative.components() {
                let Component::Normal(part) = component else {
                    return Err(BackupError::InvalidItemPath);
                };
                parts.push(part.to_str().ok_or(BackupError::InvalidItemPath)?);
            }
            actual.insert(parts.join("/"));
        } else {
            return Err(BackupError::InventoryMismatch);
        }
    }
    Ok(())
}

fn join_item_path(root: &Path, item_path: &str) -> Result<PathBuf, BackupError> {
    validate_item_path(item_path)?;
    let mut output = root.to_path_buf();
    for component in item_path.split('/') {
        output.push(component);
    }
    Ok(output)
}

fn manifest_relative_path(generation: u64) -> String {
    format!("manifests/manifest-{generation:020}.wdbm")
}

fn is_canonical_manifest_path(path: &str) -> bool {
    let Some(number) = path
        .strip_prefix("manifests/manifest-")
        .and_then(|path| path.strip_suffix(".wdbm"))
    else {
        return false;
    };
    number
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .is_some_and(|generation| manifest_relative_path(generation) == path)
}

fn is_canonical_wal_path(path: &str) -> bool {
    let Some(name) = path.strip_prefix("wal/") else {
        return false;
    };
    let Some(number) = name
        .strip_prefix("segment-")
        .and_then(|name| name.strip_suffix(".wal"))
    else {
        return false;
    };
    number
        .parse::<u64>()
        .ok()
        .filter(|sequence| *sequence > 0)
        .is_some_and(|sequence| format!("wal/{}", segment_file_name(sequence)) == path)
}

fn is_canonical_segment_path(path: &str, prefix: &str) -> bool {
    let Some(name) = path
        .strip_prefix(prefix)
        .and_then(|path| path.strip_prefix("segment-"))
        .and_then(|path| path.strip_suffix(".wdbseg"))
    else {
        return false;
    };
    SegmentId::from_str(name).ok().is_some_and(|segment_id| {
        path == format!(
            "{prefix}segment-{}.wdbseg",
            segment_id.to_canonical_string()
        )
    })
}

fn segment_relative_path(id: crate::SegmentId) -> String {
    format!("segments/segment-{}.wdbseg", id.to_canonical_string())
}

fn security_segment_relative_path(id: crate::SegmentId) -> String {
    format!(
        "security/segments/segment-{}.wdbseg",
        id.to_canonical_string()
    )
}

fn calculate_mac(key: &[u8; 32], unsigned: &[u8], manifest_digest: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(key);
    hasher.update(AUTH_CONTEXT);
    hasher.update(unsigned);
    hasher.update(manifest_digest);
    *hasher.finalize().as_bytes()
}

fn constant_time_equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn validate_key_id(key_id: &str) -> Result<(), BackupError> {
    if key_id.is_empty()
        || key_id.len() > 128
        || !key_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(BackupError::InvalidKeyId);
    }
    Ok(())
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

struct ByteCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], BackupError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(BackupError::ResourceLimit)?;
        let selected = self
            .bytes
            .get(self.offset..end)
            .ok_or(BackupError::InvalidBackupManifest)?;
        self.offset = end;
        Ok(selected)
    }

    fn u8(&mut self) -> Result<u8, BackupError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(BackupError::InvalidBackupManifest)
    }

    fn u16(&mut self) -> Result<u16, BackupError> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| BackupError::InvalidBackupManifest)?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, BackupError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| BackupError::InvalidBackupManifest)?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, BackupError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| BackupError::InvalidBackupManifest)?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn array16(&mut self) -> Result<[u8; 16], BackupError> {
        self.take(16)?
            .try_into()
            .map_err(|_| BackupError::InvalidBackupManifest)
    }

    fn array32(&mut self) -> Result<[u8; 32], BackupError> {
        self.take(32)?
            .try_into()
            .map_err(|_| BackupError::InvalidBackupManifest)
    }

    const fn position(&self) -> usize {
        self.offset
    }

    const fn is_at_end(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn read_bounded_file(path: &Path, limit: usize, root: &Path) -> Result<Vec<u8>, BackupError> {
    validate_regular_file(path, root)?;
    let metadata = fs::metadata(path).map_err(|source| BackupError::Io {
        operation: "inspect bounded backup metadata",
        source,
    })?;
    let length = usize::try_from(metadata.len()).map_err(|_| BackupError::ResourceLimit)?;
    if length > limit {
        return Err(BackupError::ResourceLimit);
    }
    let file = File::open(path).map_err(|source| BackupError::Io {
        operation: "open bounded backup metadata",
        source,
    })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| BackupError::AllocationFailed)?;
    file.take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| BackupError::Io {
            operation: "read bounded backup metadata",
            source,
        })?;
    if bytes.len() != length || bytes.len() > limit {
        return Err(BackupError::IntegrityMismatch);
    }
    Ok(bytes)
}
