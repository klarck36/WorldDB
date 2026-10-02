//! Immutable manifest generations and the atomically replaced `CURRENT` pointer.

use std::cmp::Ordering as CmpOrdering;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{DomainId, IdValidationError, Revision, RevisionError};

use crate::segment::ContentDigest;
use crate::{DatabaseLayout, SegmentId, WalCommitHash, WalError, WalPrepareLog, WriterLock};

const MANIFEST_MAGIC: [u8; 8] = *b"WDBMAN\0\x01";
const CURRENT_V1_MAGIC: [u8; 8] = *b"WDBCUR\0\x01";
const CURRENT_V2_MAGIC: [u8; 8] = *b"WDBCUR\0\x02";
const MANIFEST_MAJOR: u16 = 1;
const MANIFEST_MINOR: u16 = 0;
const CURRENT_V1_BYTES: usize = 8 + 8 + 32 + 32;
const CURRENT_V2_BYTES: usize = 8 + 8 + 32 + 32 + 32;
const MAX_CURRENT_BYTES: usize = CURRENT_V2_BYTES;
const CURRENT_BYTES: usize = CURRENT_V1_BYTES;
const MANIFEST_HEADER_BYTES: usize = 8 + 2 + 2 + 8 + 8 + 32 + 4;
const SEGMENT_REFERENCE_BYTES: usize = 1 + 8 + 16 + 32;
const DIGEST_BYTES: usize = 32;
const MAX_MANIFEST_SEGMENTS: usize = 262_144;
const MAX_MANIFEST_BYTES: usize =
    MANIFEST_HEADER_BYTES + MAX_MANIFEST_SEGMENTS * SEGMENT_REFERENCE_BYTES + DIGEST_BYTES;
const MANIFEST_PREFIX: &str = "manifest-";
const MANIFEST_SUFFIX: &str = ".wdbm";
const MANIFEST_NUMBER_WIDTH: usize = 20;
const STAGE_ATTEMPTS: u8 = 16;

static NEXT_STAGE_FILE: AtomicU64 = AtomicU64::new(0);

/// Segment namespace included in a data manifest.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ManifestSegmentKind {
    /// Canonical domain, schema, lifecycle, or project-metadata segment.
    History,
    /// Complete security and audit-policy snapshot segment.
    SecurityPolicy,
}

impl ManifestSegmentKind {
    const fn tag(self) -> u8 {
        match self {
            Self::History => 1,
            Self::SecurityPolicy => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ManifestError> {
        match tag {
            1 => Ok(Self::History),
            2 => Ok(Self::SecurityPolicy),
            _ => Err(ManifestError::InvalidManifest),
        }
    }
}

/// One segment identity, content digest, and highest logical revision in it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestSegmentReference {
    kind: ManifestSegmentKind,
    id: SegmentId,
    content_digest: ContentDigest,
    through_revision: Revision,
}

impl ManifestSegmentReference {
    /// Creates a typed manifest reference.
    #[must_use]
    pub const fn new(
        kind: ManifestSegmentKind,
        id: SegmentId,
        content_digest: ContentDigest,
        through_revision: Revision,
    ) -> Self {
        Self {
            kind,
            id,
            content_digest,
            through_revision,
        }
    }

    /// Segment namespace.
    #[must_use]
    pub const fn kind(self) -> ManifestSegmentKind {
        self.kind
    }

    /// Random immutable segment identity.
    #[must_use]
    pub const fn id(self) -> SegmentId {
        self.id
    }

    /// Digest of the canonical segment content.
    #[must_use]
    pub const fn content_digest(self) -> ContentDigest {
        self.content_digest
    }

    /// Highest WorldDB revision represented by this segment.
    #[must_use]
    pub const fn through_revision(self) -> Revision {
        self.through_revision
    }
}

/// A caller-built view of the data that a new manifest generation will cover.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestSnapshot {
    revision: Revision,
    segments: Vec<ManifestSegmentReference>,
}

impl ManifestSnapshot {
    /// Builds a canonical manifest snapshot and rejects segments whose declared
    /// logical contents extend beyond the snapshot revision.
    pub fn new(
        revision: Revision,
        mut segments: Vec<ManifestSegmentReference>,
    ) -> Result<Self, ManifestError> {
        if segments.len() > MAX_MANIFEST_SEGMENTS {
            return Err(ManifestError::ResourceLimit {
                limit: MAX_MANIFEST_SEGMENTS,
                actual: segments.len(),
            });
        }
        if segments
            .iter()
            .any(|segment| segment.through_revision > revision)
        {
            return Err(ManifestError::SegmentBeyondSnapshot);
        }
        segments.sort_unstable_by(compare_segment_references);
        if segments.windows(2).any(|pair| match pair {
            [left, right] => left.kind == right.kind && left.id == right.id,
            _ => false,
        }) {
            return Err(ManifestError::DuplicateSegmentReference);
        }
        Ok(Self { revision, segments })
    }

    /// Highest logical data revision represented by this snapshot.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Canonically ordered immutable segment references.
    #[must_use]
    pub fn segments(&self) -> &[ManifestSegmentReference] {
        &self.segments
    }
}

/// One fully decoded, immutable manifest generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    generation: u64,
    revision: Revision,
    commit_hash: WalCommitHash,
    segments: Vec<ManifestSegmentReference>,
    content_digest: ContentDigest,
}

impl Manifest {
    /// Monotonic on-disk generation number.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Highest WorldDB revision materialized by this generation.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// WAL commit-chain hash at exactly `revision`.
    #[must_use]
    pub const fn commit_hash(&self) -> WalCommitHash {
        self.commit_hash
    }

    /// Canonically ordered segment inventory.
    #[must_use]
    pub fn segments(&self) -> &[ManifestSegmentReference] {
        &self.segments
    }

    /// Digest bound by the `CURRENT` pointer.
    #[must_use]
    pub const fn content_digest(&self) -> ContentDigest {
        self.content_digest
    }
}

/// Receipt returned after a manifest and its `CURRENT` pointer are published.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestReceipt {
    generation: u64,
    revision: Revision,
    commit_hash: WalCommitHash,
    content_digest: ContentDigest,
}

impl ManifestReceipt {
    /// Published generation number.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }

    /// Published data revision.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Commit-chain hash at the published revision.
    #[must_use]
    pub const fn commit_hash(self) -> WalCommitHash {
        self.commit_hash
    }

    /// Digest of the complete immutable manifest file.
    #[must_use]
    pub const fn content_digest(self) -> ContentDigest {
        self.content_digest
    }
}

/// Why an immutable manifest generation could not be read or published.
#[derive(Debug)]
pub enum ManifestError {
    /// The supplied writer lock belongs to another database.
    ForeignWriterLock,
    /// Recovery verification has not authorized ordinary database writes.
    RecoveryRequired,
    /// WAL commit-chain verification failed.
    Wal(WalError),
    /// A requested manifest or segment revision is above the verified WAL prefix.
    ManifestAheadOfWal {
        manifest_revision: Revision,
        wal_revision: Revision,
    },
    /// An already-published manifest is ahead of the currently verified WAL.
    ExistingManifestAheadOfWal {
        manifest_revision: Revision,
        wal_revision: Revision,
    },
    /// A new generation cannot move the logical database revision backwards.
    RevisionRegression {
        current_revision: Revision,
        requested_revision: Revision,
    },
    /// A segment declares data newer than the manifest snapshot.
    SegmentBeyondSnapshot,
    /// A segment identity occurs more than once in one namespace.
    DuplicateSegmentReference,
    /// A bounded manifest field exceeds the registered limit.
    ResourceLimit { limit: usize, actual: usize },
    /// A persisted identity fails registered UUID validation.
    InvalidSegmentId(IdValidationError),
    /// A persisted revision is invalid or reserved.
    InvalidRevision(RevisionError),
    /// Manifest or pointer bytes are malformed, noncanonical, or corrupt.
    InvalidManifest,
    /// A manifest filename is malformed or noncanonical.
    InvalidManifestName { name: String },
    /// A manifest path is not a regular in-root file or directory.
    PathOutsideDatabase,
    /// Generation numbering reached the reserved maximum.
    GenerationOverflow,
    /// A bounded temporary-file name could not be reserved.
    StageNameExhausted,
    /// A filesystem operation failed; publication steps are identified by name.
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWriterLock => {
                formatter.write_str("writer lock belongs to another database")
            }
            Self::RecoveryRequired => formatter
                .write_str("manifest writes are blocked until recovery verifies a clean state"),
            Self::Wal(error) => write!(formatter, "manifest WAL verification failed: {error}"),
            Self::ManifestAheadOfWal {
                manifest_revision,
                wal_revision,
            } => write!(
                formatter,
                "manifest revision {} is above verified WAL revision {}",
                manifest_revision.value(),
                wal_revision.value()
            ),
            Self::ExistingManifestAheadOfWal {
                manifest_revision,
                wal_revision,
            } => write!(
                formatter,
                "current manifest revision {} is above verified WAL revision {}",
                manifest_revision.value(),
                wal_revision.value()
            ),
            Self::RevisionRegression {
                current_revision,
                requested_revision,
            } => write!(
                formatter,
                "manifest revision cannot move backwards from {} to {}",
                current_revision.value(),
                requested_revision.value()
            ),
            Self::SegmentBeyondSnapshot => {
                formatter.write_str("manifest segment extends beyond its snapshot revision")
            }
            Self::DuplicateSegmentReference => {
                formatter.write_str("manifest repeats a segment identity")
            }
            Self::ResourceLimit { limit, actual } => write!(
                formatter,
                "manifest has {actual} segments; limit is {limit}"
            ),
            Self::InvalidSegmentId(error) => {
                write!(formatter, "manifest SegmentId is invalid: {error}")
            }
            Self::InvalidRevision(error) => {
                write!(formatter, "manifest revision is invalid: {error}")
            }
            Self::InvalidManifest => {
                formatter.write_str("manifest or CURRENT pointer is malformed or corrupt")
            }
            Self::InvalidManifestName { name } => {
                write!(formatter, "invalid manifest filename {name:?}")
            }
            Self::PathOutsideDatabase => {
                formatter.write_str("manifest path is not a regular in-root entry")
            }
            Self::GenerationOverflow => {
                formatter.write_str("manifest generation number is exhausted")
            }
            Self::StageNameExhausted => {
                formatter.write_str("could not reserve a unique manifest staging file")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wal(error) => Some(error),
            Self::InvalidSegmentId(error) => Some(error),
            Self::InvalidRevision(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::ForeignWriterLock
            | Self::RecoveryRequired
            | Self::ManifestAheadOfWal { .. }
            | Self::ExistingManifestAheadOfWal { .. }
            | Self::RevisionRegression { .. }
            | Self::SegmentBeyondSnapshot
            | Self::DuplicateSegmentReference
            | Self::ResourceLimit { .. }
            | Self::InvalidManifest
            | Self::InvalidManifestName { .. }
            | Self::PathOutsideDatabase
            | Self::GenerationOverflow
            | Self::StageNameExhausted => None,
        }
    }
}

impl From<WalError> for ManifestError {
    fn from(error: WalError) -> Self {
        Self::Wal(error)
    }
}

/// Writes immutable data-manifest generations and atomically advances `CURRENT`.
#[derive(Clone, Debug)]
pub struct ManifestStore {
    layout: DatabaseLayout,
}

impl ManifestStore {
    /// Binds a manifest store to a validated database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Reads and verifies the manifest named by `CURRENT`; missing `CURRENT`
    /// is returned as `Ok(None)` for a new database or pre-recovery state.
    pub fn read_current(&self) -> Result<Option<Manifest>, ManifestError> {
        let Some(pointer) = self.read_current_pointer()? else {
            return Ok(None);
        };
        if pointer.generation == 0 {
            if pointer.version == CurrentPointerVersion::V2
                && pointer.manifest_digest == ContentDigest::from_bytes([0; 32])
            {
                return Ok(None);
            }
            return Err(ManifestError::InvalidManifest);
        }
        let manifest_path = manifest_path(&self.layout.manifests_directory(), pointer.generation);
        validate_regular_file_inside_root(&self.layout, &manifest_path)?;
        let bytes = read_bounded_file(&manifest_path, MAX_MANIFEST_BYTES)?;
        let manifest = decode_manifest(&bytes)?;
        if manifest.generation != pointer.generation
            || manifest.content_digest != pointer.manifest_digest
        {
            return Err(ManifestError::InvalidManifest);
        }
        Ok(Some(manifest))
    }

    /// Publishes one immutable manifest generation, then atomically points
    /// `CURRENT` at it. `snapshot.revision` may lag the verified WAL, but may
    /// never exceed it or regress behind the current manifest.
    pub fn publish(
        &self,
        writer_lock: &WriterLock,
        wal: &WalPrepareLog,
        snapshot: ManifestSnapshot,
    ) -> Result<ManifestReceipt, ManifestError> {
        self.publish_with_platform(writer_lock, wal, snapshot, &NativeManifestPublication)
    }

    fn publish_with_platform(
        &self,
        writer_lock: &WriterLock,
        wal: &WalPrepareLog,
        snapshot: ManifestSnapshot,
        platform: &impl ManifestPublication,
    ) -> Result<ManifestReceipt, ManifestError> {
        if !writer_lock.belongs_to_database_root(self.layout.root()) {
            return Err(ManifestError::ForeignWriterLock);
        }
        if !writer_lock.require_write_access() {
            return Err(ManifestError::RecoveryRequired);
        }
        self.validate_directories()?;

        let current = self.read_current()?;
        if let Some(current) = &current {
            if wal
                .verified_head_at(writer_lock, current.revision)?
                .is_none()
            {
                let wal_head = wal.commit_head(writer_lock)?;
                return Err(ManifestError::ExistingManifestAheadOfWal {
                    manifest_revision: current.revision,
                    wal_revision: wal_head.revision(),
                });
            }
            if snapshot.revision < current.revision {
                return Err(ManifestError::RevisionRegression {
                    current_revision: current.revision,
                    requested_revision: snapshot.revision,
                });
            }
        }

        let Some(verified_head) = wal.verified_head_at(writer_lock, snapshot.revision)? else {
            let wal_head = wal.commit_head(writer_lock)?;
            return Err(ManifestError::ManifestAheadOfWal {
                manifest_revision: snapshot.revision,
                wal_revision: wal_head.revision(),
            });
        };

        let generation = self.next_generation(current.as_ref().map(Manifest::generation))?;
        let manifest_bytes = encode_manifest(
            generation,
            snapshot.revision,
            verified_head.commit_hash(),
            &snapshot.segments,
        )?;
        let manifest_digest = ContentDigest::from_bytes(*blake3::hash(&manifest_bytes).as_bytes());
        let target_path = manifest_path(&self.layout.manifests_directory(), generation);
        if target_path.exists() {
            return Err(ManifestError::InvalidManifest);
        }
        self.publish_new_file("manifest", &target_path, &manifest_bytes, platform)?;
        platform
            .sync_directory(
                &self.layout.manifests_directory(),
                DirectorySyncTarget::ManifestGenerations,
            )
            .map_err(|source| ManifestError::Io {
                operation: "sync manifest-generation directory after publication",
                source,
            })?;

        let pointer_bytes = match self.read_current_pointer()?.map(|pointer| pointer.version) {
            Some(CurrentPointerVersion::V2) => encode_current_v2(CurrentPointer {
                generation,
                manifest_digest,
                version: CurrentPointerVersion::V2,
                profile_fingerprint: crate::storage_upgrade::profile_fingerprint(
                    CurrentPointerVersion::V2,
                    self.layout.format_capabilities(),
                ),
            })?
            .to_vec(),
            Some(CurrentPointerVersion::V1) | None => encode_current_v1(CurrentPointer {
                generation,
                manifest_digest,
                version: CurrentPointerVersion::V1,
                profile_fingerprint: [0; 32],
            })?
            .to_vec(),
        };
        let current_path = self.layout.current_file();
        if fs::symlink_metadata(&current_path).is_ok() {
            validate_regular_file_inside_root(&self.layout, &current_path)?;
        }
        self.publish_replacing_file("current", &current_path, &pointer_bytes, platform)?;
        platform
            .sync_directory(self.layout.root(), DirectorySyncTarget::DatabaseRoot)
            .map_err(|source| ManifestError::Io {
                operation: "sync database directory after CURRENT publication",
                source,
            })?;

        let published = self.read_current()?.ok_or(ManifestError::InvalidManifest)?;
        if published.generation != generation
            || published.revision != snapshot.revision
            || published.commit_hash != verified_head.commit_hash()
            || published.content_digest != manifest_digest
        {
            return Err(ManifestError::InvalidManifest);
        }
        Ok(ManifestReceipt {
            generation,
            revision: snapshot.revision,
            commit_hash: verified_head.commit_hash(),
            content_digest: manifest_digest,
        })
    }

    fn read_current_pointer(&self) -> Result<Option<CurrentPointer>, ManifestError> {
        let path = self.layout.current_file();
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ManifestError::Io {
                operation: "inspect CURRENT pointer",
                source,
            }),
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(ManifestError::PathOutsideDatabase);
                }
                validate_regular_file_inside_root(&self.layout, &path)?;
                let bytes = read_bounded_file(&path, MAX_CURRENT_BYTES)?;
                let pointer = decode_current(&bytes)?;
                if pointer.version == CurrentPointerVersion::V2
                    && pointer.profile_fingerprint
                        != crate::storage_upgrade::profile_fingerprint(
                            CurrentPointerVersion::V2,
                            self.layout.format_capabilities(),
                        )
                {
                    return Err(ManifestError::InvalidManifest);
                }
                Ok(Some(pointer))
            }
        }
    }

    pub(crate) fn read_current_pointer_for_upgrade(
        &self,
    ) -> Result<Option<CurrentPointer>, ManifestError> {
        self.read_current_pointer()
    }

    pub(crate) fn replace_staged_current_v2(
        &self,
        writer_lock: &WriterLock,
        staged_pointer: &Path,
    ) -> Result<(), ManifestError> {
        if !writer_lock.belongs_to_database_root(self.layout.root()) {
            return Err(ManifestError::ForeignWriterLock);
        }
        if !writer_lock.require_write_access() {
            return Err(ManifestError::RecoveryRequired);
        }
        self.validate_directories()?;
        validate_regular_file_inside_root(&self.layout, staged_pointer)?;
        let bytes = read_bounded_file(staged_pointer, CURRENT_V2_BYTES)?;
        let pointer = decode_current(&bytes)?;
        if pointer.version != CurrentPointerVersion::V2
            || pointer.profile_fingerprint
                != crate::storage_upgrade::profile_fingerprint(
                    CurrentPointerVersion::V2,
                    self.layout.format_capabilities(),
                )
        {
            return Err(ManifestError::InvalidManifest);
        }

        let current_path = self.layout.current_file();
        if fs::symlink_metadata(&current_path).is_ok() {
            validate_regular_file_inside_root(&self.layout, &current_path)?;
        }
        self.publish_replacing_file("current", &current_path, &bytes, &NativeManifestPublication)?;
        let published = self
            .read_current_pointer()?
            .ok_or(ManifestError::InvalidManifest)?;
        if published.version != CurrentPointerVersion::V2
            || published.generation != pointer.generation
            || published.manifest_digest != pointer.manifest_digest
            || published.profile_fingerprint != pointer.profile_fingerprint
        {
            return Err(ManifestError::InvalidManifest);
        }
        Ok(())
    }

    pub(crate) fn sync_current_root(&self) -> Result<(), ManifestError> {
        NativeManifestPublication
            .sync_directory(self.layout.root(), DirectorySyncTarget::DatabaseRoot)
            .map_err(|source| ManifestError::Io {
                operation: "sync database root after storage-format upgrade",
                source,
            })
    }

    fn next_generation(&self, current: Option<u64>) -> Result<u64, ManifestError> {
        let directory = self.layout.manifests_directory();
        validate_directory_inside_root(&self.layout, &directory)?;
        let mut maximum = current.unwrap_or(0);
        for entry in fs::read_dir(&directory).map_err(|source| ManifestError::Io {
            operation: "list manifest generations",
            source,
        })? {
            let entry = entry.map_err(|source| ManifestError::Io {
                operation: "read manifest directory entry",
                source,
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(MANIFEST_PREFIX) {
                continue;
            }
            let generation = parse_manifest_name(&name)?;
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|source| ManifestError::Io {
                    operation: "inspect manifest generation entry",
                    source,
                })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(ManifestError::PathOutsideDatabase);
            }
            maximum = maximum.max(generation);
        }
        maximum
            .checked_add(1)
            .ok_or(ManifestError::GenerationOverflow)
    }

    fn publish_new_file(
        &self,
        label: &'static str,
        target: &Path,
        bytes: &[u8],
        platform: &impl ManifestPublication,
    ) -> Result<(), ManifestError> {
        let (stage_path, mut stage_file) = self.create_stage_file(label)?;
        let mut guard = StagePathGuard::new(stage_path.clone());
        stage_file
            .write_all(bytes)
            .map_err(|source| ManifestError::Io {
                operation: "write staged manifest generation",
                source,
            })?;
        platform
            .sync_staged_file(&stage_file, StagedFileKind::Manifest)
            .map_err(|source| ManifestError::Io {
                operation: "sync staged manifest generation",
                source,
            })?;
        drop(stage_file);
        if target.exists() {
            return Err(ManifestError::InvalidManifest);
        }
        platform
            .publish_manifest_generation(&stage_path, target)
            .map_err(|source| ManifestError::Io {
                operation: "publish immutable manifest generation",
                source,
            })?;
        guard.mark_published();
        Ok(())
    }

    fn publish_replacing_file(
        &self,
        label: &'static str,
        target: &Path,
        bytes: &[u8],
        platform: &impl ManifestPublication,
    ) -> Result<(), ManifestError> {
        let (stage_path, mut stage_file) = self.create_stage_file(label)?;
        let mut guard = StagePathGuard::new(stage_path.clone());
        stage_file
            .write_all(bytes)
            .map_err(|source| ManifestError::Io {
                operation: "write staged CURRENT pointer",
                source,
            })?;
        platform
            .sync_staged_file(&stage_file, StagedFileKind::Current)
            .map_err(|source| ManifestError::Io {
                operation: "sync staged CURRENT pointer",
                source,
            })?;
        drop(stage_file);
        platform
            .replace_current(&stage_path, target)
            .map_err(|source| ManifestError::Io {
                operation: "atomically replace CURRENT pointer",
                source,
            })?;
        guard.mark_published();
        Ok(())
    }

    fn create_stage_file(&self, label: &'static str) -> Result<(PathBuf, File), ManifestError> {
        let directory = self.layout.staging_directory();
        validate_directory_inside_root(&self.layout, &directory)?;
        let mut attempt = 0;
        while attempt < STAGE_ATTEMPTS {
            let sequence = NEXT_STAGE_FILE.fetch_add(1, Ordering::Relaxed);
            let name = format!("{label}-{}-{sequence}.tmp", std::process::id());
            let path = directory.join(name);
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                }
                Err(source) => {
                    return Err(ManifestError::Io {
                        operation: "create staged manifest file",
                        source,
                    });
                }
            }
        }
        Err(ManifestError::StageNameExhausted)
    }

    pub(crate) fn validate_directories(&self) -> Result<(), ManifestError> {
        validate_directory_inside_root(&self.layout, &self.layout.manifests_directory())?;
        validate_directory_inside_root(&self.layout, &self.layout.staging_directory())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StagedFileKind {
    Manifest,
    Current,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectorySyncTarget {
    ManifestGenerations,
    DatabaseRoot,
}

trait ManifestPublication {
    fn sync_staged_file(&self, file: &File, kind: StagedFileKind) -> io::Result<()>;
    fn publish_manifest_generation(&self, stage: &Path, target: &Path) -> io::Result<()>;
    fn replace_current(&self, stage: &Path, target: &Path) -> io::Result<()>;
    fn sync_directory(&self, path: &Path, target: DirectorySyncTarget) -> io::Result<()>;
}

struct NativeManifestPublication;

impl ManifestPublication for NativeManifestPublication {
    fn sync_staged_file(&self, file: &File, _kind: StagedFileKind) -> io::Result<()> {
        file.sync_all()
    }

    fn publish_manifest_generation(&self, stage: &Path, target: &Path) -> io::Result<()> {
        if target.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "immutable manifest generation already exists",
            ));
        }
        #[cfg(windows)]
        {
            crate::windows_publication::move_file(stage, target, false)
        }
        #[cfg(not(windows))]
        {
            fs::rename(stage, target)
        }
    }

    fn replace_current(&self, stage: &Path, target: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            crate::windows_publication::move_file(stage, target, true)
        }
        #[cfg(not(windows))]
        {
            fs::rename(stage, target)
        }
    }

    fn sync_directory(&self, path: &Path, _target: DirectorySyncTarget) -> io::Result<()> {
        sync_directory(path)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CurrentPointerVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CurrentPointer {
    pub(crate) generation: u64,
    pub(crate) manifest_digest: ContentDigest,
    pub(crate) version: CurrentPointerVersion,
    pub(crate) profile_fingerprint: [u8; 32],
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

pub(crate) fn compare_segment_references(
    left: &ManifestSegmentReference,
    right: &ManifestSegmentReference,
) -> CmpOrdering {
    left.kind
        .cmp(&right.kind)
        .then_with(|| left.id.to_bytes().cmp(&right.id.to_bytes()))
}

fn encode_manifest(
    generation: u64,
    revision: Revision,
    commit_hash: WalCommitHash,
    segments: &[ManifestSegmentReference],
) -> Result<Vec<u8>, ManifestError> {
    if segments.len() > MAX_MANIFEST_SEGMENTS {
        return Err(ManifestError::ResourceLimit {
            limit: MAX_MANIFEST_SEGMENTS,
            actual: segments.len(),
        });
    }
    if segments
        .iter()
        .any(|segment| segment.through_revision > revision)
    {
        return Err(ManifestError::SegmentBeyondSnapshot);
    }
    let mut bytes = Vec::with_capacity(
        MANIFEST_HEADER_BYTES + segments.len() * SEGMENT_REFERENCE_BYTES + DIGEST_BYTES,
    );
    bytes.extend_from_slice(&MANIFEST_MAGIC);
    bytes.extend_from_slice(&MANIFEST_MAJOR.to_le_bytes());
    bytes.extend_from_slice(&MANIFEST_MINOR.to_le_bytes());
    bytes.extend_from_slice(&generation.to_le_bytes());
    bytes.extend_from_slice(&revision.value().to_le_bytes());
    bytes.extend_from_slice(commit_hash.as_bytes());
    bytes.extend_from_slice(
        &u32::try_from(segments.len())
            .map_err(|_| ManifestError::ResourceLimit {
                limit: MAX_MANIFEST_SEGMENTS,
                actual: segments.len(),
            })?
            .to_le_bytes(),
    );
    for segment in segments {
        bytes.push(segment.kind.tag());
        bytes.extend_from_slice(&segment.through_revision.value().to_le_bytes());
        bytes.extend_from_slice(&segment.id.to_bytes());
        bytes.extend_from_slice(segment.content_digest.as_bytes());
    }
    let checksum = *blake3::hash(&bytes).as_bytes();
    bytes.extend_from_slice(&checksum);
    Ok(bytes)
}

fn decode_manifest(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    if bytes.len() < MANIFEST_HEADER_BYTES + DIGEST_BYTES || bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::InvalidManifest);
    }
    let content_length = bytes.len() - DIGEST_BYTES;
    let content = bytes
        .get(..content_length)
        .ok_or(ManifestError::InvalidManifest)?;
    let checksum = bytes
        .get(content_length..)
        .ok_or(ManifestError::InvalidManifest)?;
    let expected_checksum = blake3::hash(content);
    if expected_checksum.as_bytes() != checksum {
        return Err(ManifestError::InvalidManifest);
    }
    let mut reader = ByteReader::new(content);
    if reader.array::<8>()? != MANIFEST_MAGIC
        || reader.u16()? != MANIFEST_MAJOR
        || reader.u16()? != MANIFEST_MINOR
    {
        return Err(ManifestError::InvalidManifest);
    }
    let generation = reader.u64()?;
    if generation == 0 {
        return Err(ManifestError::InvalidManifest);
    }
    let revision = Revision::try_from(reader.u64()?).map_err(ManifestError::InvalidRevision)?;
    let commit_hash = WalCommitHash::from_bytes(reader.array::<32>()?);
    let count = usize::try_from(reader.u32()?).map_err(|_| ManifestError::InvalidManifest)?;
    if count > MAX_MANIFEST_SEGMENTS {
        return Err(ManifestError::ResourceLimit {
            limit: MAX_MANIFEST_SEGMENTS,
            actual: count,
        });
    }
    let mut segments = Vec::new();
    segments
        .try_reserve_exact(count)
        .map_err(|_| ManifestError::InvalidManifest)?;
    for _ in 0..count {
        let kind = ManifestSegmentKind::from_tag(reader.u8()?)?;
        let through_revision =
            Revision::try_from(reader.u64()?).map_err(ManifestError::InvalidRevision)?;
        if through_revision > revision {
            return Err(ManifestError::SegmentBeyondSnapshot);
        }
        let id_bytes = reader.array::<16>()?;
        let id = SegmentId::try_from_bytes(id_bytes).map_err(ManifestError::InvalidSegmentId)?;
        let content_digest = ContentDigest::from_bytes(reader.array::<32>()?);
        segments.push(ManifestSegmentReference::new(
            kind,
            id,
            content_digest,
            through_revision,
        ));
    }
    reader.finish()?;
    if segments.windows(2).any(|pair| match pair {
        [left, right] => compare_segment_references(left, right) != CmpOrdering::Less,
        _ => false,
    }) {
        return Err(ManifestError::InvalidManifest);
    }
    let content_digest = ContentDigest::from_bytes(*blake3::hash(bytes).as_bytes());
    Ok(Manifest {
        generation,
        revision,
        commit_hash,
        segments,
        content_digest,
    })
}

fn encode_current_v1(pointer: CurrentPointer) -> Result<[u8; CURRENT_V1_BYTES], ManifestError> {
    let mut bytes = Vec::with_capacity(CURRENT_BYTES);
    bytes.extend_from_slice(&CURRENT_V1_MAGIC);
    bytes.extend_from_slice(&pointer.generation.to_le_bytes());
    bytes.extend_from_slice(pointer.manifest_digest.as_bytes());
    let checksum = *blake3::hash(&bytes).as_bytes();
    bytes.extend_from_slice(&checksum);
    bytes.try_into().map_err(|_| ManifestError::InvalidManifest)
}

pub(crate) fn encode_current_v2(
    pointer: CurrentPointer,
) -> Result<[u8; CURRENT_V2_BYTES], ManifestError> {
    let mut bytes = Vec::with_capacity(CURRENT_V2_BYTES);
    bytes.extend_from_slice(&CURRENT_V2_MAGIC);
    bytes.extend_from_slice(&pointer.generation.to_le_bytes());
    bytes.extend_from_slice(pointer.manifest_digest.as_bytes());
    bytes.extend_from_slice(&pointer.profile_fingerprint);
    let checksum = *blake3::hash(&bytes).as_bytes();
    bytes.extend_from_slice(&checksum);
    bytes.try_into().map_err(|_| ManifestError::InvalidManifest)
}

#[cfg(test)]
fn encode_current(pointer: CurrentPointer) -> Result<[u8; CURRENT_BYTES], ManifestError> {
    encode_current_v1(pointer)
}

pub(crate) fn decode_current(bytes: &[u8]) -> Result<CurrentPointer, ManifestError> {
    if bytes.len() != CURRENT_V1_BYTES && bytes.len() != CURRENT_V2_BYTES {
        return Err(ManifestError::InvalidManifest);
    }
    let checksum_offset = bytes
        .len()
        .checked_sub(DIGEST_BYTES)
        .ok_or(ManifestError::InvalidManifest)?;
    let content = bytes
        .get(..checksum_offset)
        .ok_or(ManifestError::InvalidManifest)?;
    let checksum = bytes
        .get(checksum_offset..)
        .ok_or(ManifestError::InvalidManifest)?;
    if blake3::hash(content).as_bytes() != checksum {
        return Err(ManifestError::InvalidManifest);
    }
    let mut reader = ByteReader::new(content);
    let magic = reader.array::<8>()?;
    let version = match magic {
        CURRENT_V1_MAGIC if bytes.len() == CURRENT_V1_BYTES => CurrentPointerVersion::V1,
        CURRENT_V2_MAGIC if bytes.len() == CURRENT_V2_BYTES => CurrentPointerVersion::V2,
        _ => return Err(ManifestError::InvalidManifest),
    };
    let generation = reader.u64()?;
    if generation == 0 && version == CurrentPointerVersion::V1 {
        return Err(ManifestError::InvalidManifest);
    }
    let manifest_digest = ContentDigest::from_bytes(reader.array::<32>()?);
    let profile_fingerprint = if version == CurrentPointerVersion::V2 {
        reader.array::<32>()?
    } else {
        [0; 32]
    };
    if generation == 0 && manifest_digest != ContentDigest::from_bytes([0; 32]) {
        return Err(ManifestError::InvalidManifest);
    }
    reader.finish()?;
    Ok(CurrentPointer {
        generation,
        manifest_digest,
        version,
        profile_fingerprint,
    })
}

pub(crate) fn manifest_path(directory: &Path, generation: u64) -> PathBuf {
    directory.join(format!(
        "{MANIFEST_PREFIX}{generation:0MANIFEST_NUMBER_WIDTH$}{MANIFEST_SUFFIX}"
    ))
}

fn parse_manifest_name(name: &str) -> Result<u64, ManifestError> {
    let Some(number) = name
        .strip_prefix(MANIFEST_PREFIX)
        .and_then(|name| name.strip_suffix(MANIFEST_SUFFIX))
    else {
        return Err(ManifestError::InvalidManifestName {
            name: name.to_owned(),
        });
    };
    if number.len() != MANIFEST_NUMBER_WIDTH || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ManifestError::InvalidManifestName {
            name: name.to_owned(),
        });
    }
    let generation = number
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| ManifestError::InvalidManifestName {
            name: name.to_owned(),
        })?;
    if format!("{generation:0MANIFEST_NUMBER_WIDTH$}") != number {
        return Err(ManifestError::InvalidManifestName {
            name: name.to_owned(),
        });
    }
    Ok(generation)
}

fn validate_directory_inside_root(
    layout: &DatabaseLayout,
    path: &Path,
) -> Result<(), ManifestError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ManifestError::Io {
        operation: "inspect manifest directory",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ManifestError::PathOutsideDatabase);
    }
    let canonical = fs::canonicalize(path).map_err(|source| ManifestError::Io {
        operation: "resolve manifest directory",
        source,
    })?;
    if !canonical.starts_with(layout.root()) {
        return Err(ManifestError::PathOutsideDatabase);
    }
    Ok(())
}

fn validate_regular_file_inside_root(
    layout: &DatabaseLayout,
    path: &Path,
) -> Result<(), ManifestError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ManifestError::Io {
        operation: "inspect manifest file",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ManifestError::PathOutsideDatabase);
    }
    let canonical = fs::canonicalize(path).map_err(|source| ManifestError::Io {
        operation: "resolve manifest file",
        source,
    })?;
    if !canonical.starts_with(layout.root()) {
        return Err(ManifestError::PathOutsideDatabase);
    }
    Ok(())
}

fn read_bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>, ManifestError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ManifestError::Io {
        operation: "inspect manifest file before read",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ManifestError::PathOutsideDatabase);
    }
    let file_size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if file_size > limit {
        return Err(ManifestError::ResourceLimit {
            limit,
            actual: file_size,
        });
    }
    let file = File::open(path).map_err(|source| ManifestError::Io {
        operation: "open manifest file",
        source,
    })?;
    let opened = file.metadata().map_err(|source| ManifestError::Io {
        operation: "inspect opened manifest file",
        source,
    })?;
    if !opened.is_file() || opened.len() != metadata.len() {
        return Err(ManifestError::PathOutsideDatabase);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(file_size)
        .map_err(|_| ManifestError::InvalidManifest)?;
    file.take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| ManifestError::Io {
            operation: "read manifest file",
            source,
        })?;
    if bytes.len() > limit || bytes.len() != file_size {
        return Err(ManifestError::InvalidManifest);
    }
    Ok(bytes)
}

pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?
            .sync_all()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory synchronization is unsupported on this platform",
        ))
    }
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ManifestError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ManifestError::InvalidManifest)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ManifestError::InvalidManifest)?;
        self.offset = end;
        Ok(value)
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], ManifestError> {
        self.take(LENGTH)?
            .try_into()
            .map_err(|_| ManifestError::InvalidManifest)
    }

    fn u8(&mut self) -> Result<u8, ManifestError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, ManifestError> {
        Ok(u16::from_le_bytes(self.array::<2>()?))
    }

    fn u32(&mut self) -> Result<u32, ManifestError> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    fn u64(&mut self) -> Result<u64, ManifestError> {
        Ok(u64::from_le_bytes(self.array::<8>()?))
    }

    fn finish(self) -> Result<(), ManifestError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ManifestError::InvalidManifest)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CURRENT_BYTES, CURRENT_V2_BYTES, CurrentPointer, CurrentPointerVersion, DIGEST_BYTES,
        DirectorySyncTarget, ManifestError, ManifestPublication, ManifestSegmentKind,
        ManifestSegmentReference, ManifestSnapshot, ManifestStore, NativeManifestPublication,
        StagedFileKind, compare_segment_references, decode_current, decode_manifest,
        encode_current, encode_current_v2, encode_manifest,
    };
    use crate::{ContentDigest, DatabaseLayout, SegmentId, WalCommitHash, WalPrepareLog};
    use std::env;
    use std::fs::{self, File};
    use std::io;
    use std::path::{Path, PathBuf};
    #[cfg(windows)]
    use std::process::{Command, Stdio};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use worlddb_core::{DomainId, OperationId, Revision};

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDatabase(PathBuf);

    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-manifest-unit-{}-{sequence}",
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

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PublicationPoint {
        SyncManifestFile,
        PublishManifestGeneration,
        SyncManifestDirectory,
        SyncCurrentFile,
        ReplaceCurrent,
        SyncDatabaseDirectory,
    }

    impl PublicationPoint {
        #[cfg(windows)]
        const fn name(self) -> &'static str {
            match self {
                Self::SyncManifestFile => "sync_manifest_file",
                Self::PublishManifestGeneration => "publish_manifest_generation",
                Self::SyncManifestDirectory => "sync_manifest_directory",
                Self::SyncCurrentFile => "sync_current_file",
                Self::ReplaceCurrent => "replace_current",
                Self::SyncDatabaseDirectory => "sync_database_directory",
            }
        }

        #[cfg(windows)]
        fn from_name(name: &str) -> Option<Self> {
            Some(match name {
                "sync_manifest_file" => Self::SyncManifestFile,
                "publish_manifest_generation" => Self::PublishManifestGeneration,
                "sync_manifest_directory" => Self::SyncManifestDirectory,
                "sync_current_file" => Self::SyncCurrentFile,
                "replace_current" => Self::ReplaceCurrent,
                "sync_database_directory" => Self::SyncDatabaseDirectory,
                _ => return None,
            })
        }
    }

    #[derive(Default)]
    struct RecordingPublication {
        events: Mutex<Vec<&'static str>>,
        fail_manifest_directory_sync: bool,
        fail_after: Option<PublicationPoint>,
        crash_after: Option<PublicationPoint>,
    }

    impl RecordingPublication {
        fn events(&self) -> Result<Vec<&'static str>, String> {
            self.events
                .lock()
                .map(|events| events.clone())
                .map_err(|error| error.to_string())
        }

        fn record(&self, point: PublicationPoint, event: &'static str) -> io::Result<()> {
            self.events
                .lock()
                .map_err(|_| io::Error::other("event recorder is poisoned"))?
                .push(event);
            if self.crash_after == Some(point) {
                std::process::exit(86);
            }
            if self.fail_after == Some(point) {
                return Err(io::Error::other("injected publication interruption"));
            }
            Ok(())
        }
    }

    impl ManifestPublication for RecordingPublication {
        fn sync_staged_file(&self, file: &File, kind: StagedFileKind) -> io::Result<()> {
            NativeManifestPublication.sync_staged_file(file, kind)?;
            match kind {
                StagedFileKind::Manifest => {
                    self.record(PublicationPoint::SyncManifestFile, "sync_manifest_file")
                }
                StagedFileKind::Current => {
                    self.record(PublicationPoint::SyncCurrentFile, "sync_current_file")
                }
            }
        }

        fn publish_manifest_generation(&self, stage: &Path, target: &Path) -> io::Result<()> {
            NativeManifestPublication.publish_manifest_generation(stage, target)?;
            self.record(
                PublicationPoint::PublishManifestGeneration,
                "publish_manifest",
            )
        }

        fn replace_current(&self, stage: &Path, target: &Path) -> io::Result<()> {
            NativeManifestPublication.replace_current(stage, target)?;
            self.record(PublicationPoint::ReplaceCurrent, "replace_current")
        }

        fn sync_directory(&self, path: &Path, target: DirectorySyncTarget) -> io::Result<()> {
            if target == DirectorySyncTarget::ManifestGenerations
                && self.fail_manifest_directory_sync
            {
                self.events
                    .lock()
                    .map_err(|_| io::Error::other("event recorder is poisoned"))?
                    .push("manifest_directory_sync_failed");
                return Err(io::Error::other("injected manifest directory sync failure"));
            }
            NativeManifestPublication.sync_directory(path, target)?;
            match target {
                DirectorySyncTarget::ManifestGenerations => self.record(
                    PublicationPoint::SyncManifestDirectory,
                    "sync_manifest_directory",
                ),
                DirectorySyncTarget::DatabaseRoot => self.record(
                    PublicationPoint::SyncDatabaseDirectory,
                    "sync_database_directory",
                ),
            }
        }
    }

    fn segment_id(tail: u8) -> Result<SegmentId, ManifestError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x40;
        bytes[8] = 0x80;
        bytes[15] = tail;
        SegmentId::try_from_bytes(bytes).map_err(ManifestError::InvalidSegmentId)
    }

    fn operation_id(tail: u8) -> Result<OperationId, String> {
        OperationId::try_from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89,
            0xab, tail,
        ])
        .map_err(|error| error.to_string())
    }

    #[test]
    fn publication_syncs_generation_before_current_in_platform_order() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let wal = WalPrepareLog::new(&layout);
        wal.commit_operation(&lock, operation_id(1)?, b"committed")
            .map_err(|error| error.to_string())?;
        let store = ManifestStore::new(layout);
        let platform = RecordingPublication::default();

        store
            .publish_with_platform(
                &lock,
                &wal,
                ManifestSnapshot::new(
                    Revision::try_from(1).map_err(|error| error.to_string())?,
                    vec![],
                )
                .map_err(|error| error.to_string())?,
                &platform,
            )
            .map_err(|error| error.to_string())?;

        assert_eq!(
            platform.events()?,
            [
                "sync_manifest_file",
                "publish_manifest",
                "sync_manifest_directory",
                "sync_current_file",
                "replace_current",
                "sync_database_directory",
            ]
        );
        Ok(())
    }

    #[test]
    fn manifest_directory_sync_failure_does_not_publish_current() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let wal = WalPrepareLog::new(&layout);
        let store = ManifestStore::new(layout.clone());
        let platform = RecordingPublication {
            fail_manifest_directory_sync: true,
            ..RecordingPublication::default()
        };

        assert!(matches!(
            store.publish_with_platform(
                &lock,
                &wal,
                ManifestSnapshot::new(Revision::GENESIS, vec![])
                    .map_err(|error| error.to_string())?,
                &platform,
            ),
            Err(ManifestError::Io {
                operation: "sync manifest-generation directory after publication",
                ..
            })
        ));
        assert!(!layout.current_file().exists());
        assert_eq!(
            platform.events()?,
            [
                "sync_manifest_file",
                "publish_manifest",
                "manifest_directory_sync_failed",
            ]
        );
        Ok(())
    }

    #[test]
    fn restart_after_each_manifest_publication_point_restores_the_committed_prefix()
    -> Result<(), String> {
        for fail_after in [
            PublicationPoint::SyncManifestFile,
            PublicationPoint::PublishManifestGeneration,
            PublicationPoint::SyncManifestDirectory,
            PublicationPoint::SyncCurrentFile,
            PublicationPoint::ReplaceCurrent,
            PublicationPoint::SyncDatabaseDirectory,
        ] {
            let database = TempDatabase::create()?;
            let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let wal = WalPrepareLog::new(&layout);
            let store = ManifestStore::new(layout.clone());
            let revision_one = Revision::try_from(1).map_err(|error| error.to_string())?;
            wal.commit_manifest_snapshot(&lock, operation_id(1)?, vec![], &[])
                .map_err(|error| error.to_string())?;
            store
                .publish(
                    &lock,
                    &wal,
                    ManifestSnapshot::new(revision_one, vec![])
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;

            let revision_two = Revision::try_from(2).map_err(|error| error.to_string())?;
            wal.commit_manifest_snapshot(&lock, operation_id(2)?, vec![], &[])
                .map_err(|error| error.to_string())?;
            let interrupted = store.publish_with_platform(
                &lock,
                &wal,
                ManifestSnapshot::new(revision_two, vec![]).map_err(|error| error.to_string())?,
                &RecordingPublication {
                    fail_after: Some(fail_after),
                    ..RecordingPublication::default()
                },
            );
            if !matches!(interrupted, Err(ManifestError::Io { .. })) {
                return Err(format!(
                    "manifest publication did not stop after {fail_after:?}"
                ));
            }
            drop(store);
            drop(wal);
            drop(lock);

            let reopened_layout =
                DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            let reopened_lock = reopened_layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let outcome = crate::RecoveryManager::new(reopened_layout.clone())
                .recover(&reopened_lock)
                .map_err(|error| format!("restart after {fail_after:?}: {error}"))?;
            assert_eq!(outcome.report().safe_revision(), revision_two);
            assert!(outcome.report().is_clean());
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn process_crash_after_each_manifest_publication_point_restores_the_committed_prefix()
    -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M5_22_MANIFEST_CRASH_ROOT";
        const POINT_ENV: &str = "WORLDDB_M5_22_MANIFEST_CRASH_POINT";
        const TEST_NAME: &str = "manifest::tests::process_crash_after_each_manifest_publication_point_restores_the_committed_prefix";

        if let (Ok(root), Ok(point_name)) = (env::var(ROOT_ENV), env::var(POINT_ENV)) {
            let point = PublicationPoint::from_name(&point_name)
                .ok_or_else(|| format!("unknown publication point {point_name}"))?;
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            crate::RecoveryManager::new(layout.clone())
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            let wal = WalPrepareLog::new(&layout);
            let store = ManifestStore::new(layout);
            store
                .publish_with_platform(
                    &lock,
                    &wal,
                    ManifestSnapshot::new(
                        Revision::try_from(2).map_err(|error| error.to_string())?,
                        vec![],
                    )
                    .map_err(|error| error.to_string())?,
                    &RecordingPublication {
                        crash_after: Some(point),
                        ..RecordingPublication::default()
                    },
                )
                .map_err(|error| error.to_string())?;
            return Err(format!("child did not crash after {point_name}"));
        }

        for point in [
            PublicationPoint::SyncManifestFile,
            PublicationPoint::PublishManifestGeneration,
            PublicationPoint::SyncManifestDirectory,
            PublicationPoint::SyncCurrentFile,
            PublicationPoint::ReplaceCurrent,
            PublicationPoint::SyncDatabaseDirectory,
        ] {
            let database = TempDatabase::create()?;
            let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let wal = WalPrepareLog::new(&layout);
            let store = ManifestStore::new(layout.clone());
            let revision_one = Revision::try_from(1).map_err(|error| error.to_string())?;
            wal.commit_manifest_snapshot(&lock, operation_id(1)?, vec![], &[])
                .map_err(|error| error.to_string())?;
            store
                .publish(
                    &lock,
                    &wal,
                    ManifestSnapshot::new(revision_one, vec![])
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
            let revision_two = Revision::try_from(2).map_err(|error| error.to_string())?;
            wal.commit_manifest_snapshot(&lock, operation_id(2)?, vec![], &[])
                .map_err(|error| error.to_string())?;
            store
                .publish(
                    &lock,
                    &wal,
                    ManifestSnapshot::new(revision_two, vec![])
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
            drop(store);
            drop(wal);
            drop(lock);

            let executable = env::current_exe().map_err(|error| error.to_string())?;
            let status = Command::new(executable)
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(ROOT_ENV, &database.0)
                .env(POINT_ENV, point.name())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|error| error.to_string())?;
            if status.code() != Some(86) {
                return Err(format!(
                    "child for {point:?} exited with {:?}, expected crash code 86",
                    status.code()
                ));
            }

            let reopened_layout =
                DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            let reopened_lock = reopened_layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let recovered = crate::RecoveryManager::new(reopened_layout.clone())
                .recover(&reopened_lock)
                .map_err(|error| format!("restart after {point:?}: {error}"))?;
            assert_eq!(recovered.report().safe_revision(), revision_two);
            assert!(recovered.report().is_clean());
            assert_eq!(
                ManifestStore::new(reopened_layout)
                    .read_current()
                    .map_err(|error| error.to_string())?
                    .map(|manifest| manifest.revision()),
                Some(revision_two)
            );
        }
        Ok(())
    }

    #[test]
    fn current_pointer_is_exact_and_checksummed() -> Result<(), ManifestError> {
        let digest = ContentDigest::from_bytes([0x42; 32]);
        let encoded = encode_current(CurrentPointer {
            generation: 7,
            manifest_digest: digest,
            version: CurrentPointerVersion::V1,
            profile_fingerprint: [0; 32],
        })?;
        assert_eq!(encoded.len(), CURRENT_BYTES);
        let decoded = decode_current(&encoded)?;
        assert_eq!(decoded.generation, 7);
        assert_eq!(decoded.manifest_digest, digest);

        let mut corrupted = encoded;
        if let Some(byte) = corrupted.get_mut(20) {
            *byte ^= 1;
        }
        assert!(matches!(
            decode_current(&corrupted),
            Err(ManifestError::InvalidManifest)
        ));
        Ok(())
    }

    #[test]
    fn current_v2_pointer_binds_profile_and_supports_genesis() -> Result<(), ManifestError> {
        let pointer = CurrentPointer {
            generation: 0,
            manifest_digest: ContentDigest::from_bytes([0; 32]),
            version: CurrentPointerVersion::V2,
            profile_fingerprint: [0x31; 32],
        };
        let encoded = encode_current_v2(pointer)?;
        assert_eq!(encoded.len(), CURRENT_V2_BYTES);
        assert_eq!(decode_current(&encoded)?, pointer);

        let mut invalid = encoded;
        *invalid.get_mut(16).ok_or(ManifestError::InvalidManifest)? = 1;
        let checksum_offset = CURRENT_V2_BYTES - DIGEST_BYTES;
        let checksum_input = invalid
            .get(..checksum_offset)
            .ok_or(ManifestError::InvalidManifest)?;
        let checksum = *blake3::hash(checksum_input).as_bytes();
        invalid
            .get_mut(checksum_offset..)
            .ok_or(ManifestError::InvalidManifest)?
            .copy_from_slice(&checksum);
        assert!(matches!(
            decode_current(&invalid),
            Err(ManifestError::InvalidManifest)
        ));
        Ok(())
    }

    #[test]
    fn manifest_encoding_is_canonical_and_rejects_future_segments() -> Result<(), ManifestError> {
        let revision = Revision::try_from(3).map_err(ManifestError::InvalidRevision)?;
        let digest = ContentDigest::from_bytes([0x22; 32]);
        let mut references = vec![
            ManifestSegmentReference::new(
                ManifestSegmentKind::History,
                segment_id(2)?,
                digest,
                revision,
            ),
            ManifestSegmentReference::new(
                ManifestSegmentKind::History,
                segment_id(1)?,
                digest,
                Revision::try_from(2).map_err(ManifestError::InvalidRevision)?,
            ),
        ];
        references.sort_unstable_by(compare_segment_references);
        let snapshot = ManifestSnapshot::new(revision, references.clone())?;
        let hash = WalCommitHash::from_bytes([0x33; 32]);
        let encoded = encode_manifest(1, revision, hash, snapshot.segments())?;
        let decoded = decode_manifest(&encoded)?;
        assert_eq!(decoded.revision, revision);
        assert_eq!(decoded.commit_hash, hash);
        assert_eq!(decoded.segments, references);

        let future = Revision::try_from(4).map_err(ManifestError::InvalidRevision)?;
        assert!(matches!(
            ManifestSnapshot::new(
                revision,
                vec![ManifestSegmentReference::new(
                    ManifestSegmentKind::History,
                    segment_id(3)?,
                    digest,
                    future,
                )],
            ),
            Err(ManifestError::SegmentBeyondSnapshot)
        ));
        Ok(())
    }

    #[test]
    fn manifest_decoder_rejects_checksum_and_trailing_data() -> Result<(), ManifestError> {
        let revision = Revision::GENESIS;
        let encoded = encode_manifest(1, revision, WalCommitHash::from_bytes([0; 32]), &[])?;
        assert!(decode_manifest(&encoded).is_ok());
        let mut corrupted = encoded.clone();
        if let Some(byte) = corrupted.get_mut(12) {
            *byte ^= 1;
        }
        assert!(matches!(
            decode_manifest(&corrupted),
            Err(ManifestError::InvalidManifest)
        ));
        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            decode_manifest(&trailing),
            Err(ManifestError::InvalidManifest)
        ));
        Ok(())
    }
}
