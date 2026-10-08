//! Immutable, canonical history-segment files.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use worlddb_core::storage_internal::SegmentId;
use worlddb_core::{
    DecodeResource, DecodedRecord, DecoderLimits, DomainId, IdValidationError, Record,
    RecordCodecError, decode_record_batch_with_limits, encode_decoded_record, encode_record,
};

use crate::{DatabaseLayout, ManifestSegmentKind, ManifestSegmentReference, WriterLock};

pub(crate) const SEGMENT_MAGIC: [u8; 8] = *b"WDBSEG\0\x01";
const SEGMENT_FORMAT_MAJOR: u16 = 1;
const SEGMENT_FORMAT_MINOR: u16 = 0;
pub(crate) const ENVELOPE_BYTES: usize = 8 + 16 + 32;
const CANONICAL_HEADER_BYTES: usize = 8;
const INDEX_ENTRY_BYTES: usize = 16;
pub(crate) const MAX_CANONICAL_BYTES: usize = CANONICAL_HEADER_BYTES
    + DecoderLimits::DEFAULT.max_records_per_batch * INDEX_ENTRY_BYTES
    + DecoderLimits::DEFAULT.max_batch_bytes;
pub(crate) const MAX_FILE_BYTES: usize = ENVELOPE_BYTES + MAX_CANONICAL_BYTES;
const SEGMENT_ID_ATTEMPTS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SegmentIoCheckpoint {
    StageFileCreated,
    MagicWritten,
    IdentityWritten,
    DigestWritten,
    ContentWritten,
    FileSynced,
    StagingDirectorySynced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReclamationCheckpoint {
    SegmentRemoved,
    DirectorySynced,
}

/// Cryptographic digest of canonical segment content, excluding the random file identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContentDigest([u8; 32]);

impl ContentDigest {
    /// Constructs a digest value from its exact 32-byte representation.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Copies the exact digest bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Why a history segment could not be written or verified.
#[derive(Debug)]
pub enum SegmentError {
    /// The writer lock belongs to a different database directory.
    ForeignWriterLock,
    /// Recovery verification has not authorized ordinary database writes.
    RecoveryRequired,
    /// The operating system could not generate a random SegmentId.
    EntropyUnavailable,
    /// Generated or stored bytes do not satisfy the registered SegmentId grammar.
    InvalidSegmentId(IdValidationError),
    /// Every bounded random SegmentId attempt collided with an existing file.
    SegmentIdCollisionExhausted,
    /// A file operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// A typed WorldDB record failed canonical encoding or decoding.
    RecordCodec(RecordCodecError),
    /// The segment contains no records.
    EmptySegment,
    /// The segment exceeds the default 1.0 record-count or byte budget.
    ResourceLimit {
        resource: DecodeResource,
        limit: usize,
        actual: usize,
    },
    /// A required memory reservation failed.
    AllocationFailed,
    /// The file envelope has the wrong magic or is truncated.
    InvalidEnvelope,
    /// The requested SegmentId does not match the file's envelope identity.
    SegmentIdMismatch,
    /// The segment uses a major/minor version this implementation cannot read.
    UnsupportedVersion { major: u16, minor: u16 },
    /// The canonical content digest does not match the stored digest.
    ContentDigestMismatch,
    /// The canonical content index or its record ordering is malformed.
    InvalidContentIndex,
    /// A referenced path is not a regular file or directory inside the database root.
    PathOutsideDatabase,
}

impl fmt::Display for SegmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWriterLock => {
                formatter.write_str("writer lock belongs to another database")
            }
            Self::RecoveryRequired => formatter
                .write_str("segment writes are blocked until recovery verifies a clean state"),
            Self::EntropyUnavailable => {
                formatter.write_str("operating-system randomness is unavailable")
            }
            Self::InvalidSegmentId(error) => write!(formatter, "SegmentId is invalid: {error}"),
            Self::SegmentIdCollisionExhausted => {
                formatter.write_str("could not generate a unique SegmentId")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::RecordCodec(error) => write!(formatter, "segment record codec failed: {error}"),
            Self::EmptySegment => formatter.write_str("a history segment must contain records"),
            Self::ResourceLimit {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "segment resource {resource:?} exceeds limit {limit} with {actual}"
            ),
            Self::AllocationFailed => formatter.write_str("segment memory reservation failed"),
            Self::InvalidEnvelope => formatter.write_str("segment envelope is malformed"),
            Self::SegmentIdMismatch => {
                formatter.write_str("segment envelope identity differs from the requested ID")
            }
            Self::UnsupportedVersion { major, minor } => {
                write!(
                    formatter,
                    "unsupported segment format version {major}.{minor}"
                )
            }
            Self::ContentDigestMismatch => {
                formatter.write_str("canonical segment content digest does not match")
            }
            Self::InvalidContentIndex => {
                formatter.write_str("segment content index is not canonical")
            }
            Self::PathOutsideDatabase => {
                formatter.write_str("segment path is not a regular entry inside the database")
            }
        }
    }
}

impl std::error::Error for SegmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::RecordCodec(error) => Some(error),
            Self::InvalidSegmentId(error) => Some(error),
            Self::ForeignWriterLock
            | Self::RecoveryRequired
            | Self::EntropyUnavailable
            | Self::SegmentIdCollisionExhausted
            | Self::EmptySegment
            | Self::ResourceLimit { .. }
            | Self::AllocationFailed
            | Self::InvalidEnvelope
            | Self::SegmentIdMismatch
            | Self::UnsupportedVersion { .. }
            | Self::ContentDigestMismatch
            | Self::InvalidContentIndex
            | Self::PathOutsideDatabase => None,
        }
    }
}

/// A record set decoded from one immutable history segment.
#[derive(Clone, Debug)]
pub struct HistorySegment {
    id: SegmentId,
    content_digest: ContentDigest,
    records: Vec<DecodedRecord>,
}

impl HistorySegment {
    /// Stable random identity stored in the file envelope.
    #[must_use]
    pub const fn id(&self) -> SegmentId {
        self.id
    }

    /// Digest over canonical version, index, and record-frame bytes.
    #[must_use]
    pub const fn content_digest(&self) -> ContentDigest {
        self.content_digest
    }

    /// Canonically ordered decoded records.
    #[must_use]
    pub fn records(&self) -> &[DecodedRecord] {
        &self.records
    }

    pub(crate) fn into_records(self) -> Vec<DecodedRecord> {
        self.records
    }
}

/// Result of writing one new immutable segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistorySegmentReceipt {
    id: SegmentId,
    content_digest: ContentDigest,
    record_count: usize,
    file_bytes: u64,
}

impl HistorySegmentReceipt {
    /// Stable random identity assigned to this file.
    #[must_use]
    pub const fn id(self) -> SegmentId {
        self.id
    }

    /// Digest over canonical version, index, and record-frame bytes.
    #[must_use]
    pub const fn content_digest(self) -> ContentDigest {
        self.content_digest
    }

    /// Number of record frames stored in the segment.
    #[must_use]
    pub const fn record_count(self) -> usize {
        self.record_count
    }

    /// Total envelope plus canonical bytes written to the file.
    #[must_use]
    pub const fn file_bytes(self) -> u64 {
        self.file_bytes
    }
}

/// Writes and reads immutable canonical segment files in one database layout.
#[derive(Clone, Debug)]
pub struct HistorySegmentStore {
    layout: DatabaseLayout,
}

impl HistorySegmentStore {
    /// Binds a segment store to a validated database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Writes typed records with no optional wire flags.
    ///
    /// Records are ordered by their canonical frame bytes before the content
    /// index is built. The digest covers the canonical format version, index,
    /// and frames. It excludes the separate random SegmentId envelope.
    pub fn write_segment(
        &self,
        writer_lock: &WriterLock,
        records: &[Record],
    ) -> Result<HistorySegmentReceipt, SegmentError> {
        let frames = encode_records(records)?;
        self.write_frames(writer_lock, frames, false)
    }

    /// Writes a canonical immutable segment to same-volume staging for a
    /// WAL-backed manifest transaction. Recovery publishes it only after the
    /// corresponding WAL commitpoint is verified.
    pub fn stage_segment(
        &self,
        writer_lock: &WriterLock,
        records: &[Record],
    ) -> Result<HistorySegmentReceipt, SegmentError> {
        let frames = encode_records(records)?;
        self.write_frames(writer_lock, frames, true)
    }

    /// Writes decoded records while retaining their optional wire capability flags.
    pub fn write_decoded_segment(
        &self,
        writer_lock: &WriterLock,
        records: &[DecodedRecord],
    ) -> Result<HistorySegmentReceipt, SegmentError> {
        let frames = encode_decoded_records(records)?;
        self.write_frames(writer_lock, frames, false)
    }

    /// Stages decoded records while retaining optional wire capability flags.
    pub fn stage_decoded_segment(
        &self,
        writer_lock: &WriterLock,
        records: &[DecodedRecord],
    ) -> Result<HistorySegmentReceipt, SegmentError> {
        let frames = encode_decoded_records(records)?;
        self.write_frames(writer_lock, frames, true)
    }

    pub(crate) fn remove_staged_reference(
        &self,
        reference: ManifestSegmentReference,
    ) -> Result<(), SegmentError> {
        if reference.kind() != ManifestSegmentKind::History {
            return Err(SegmentError::InvalidEnvelope);
        }
        let directory = self.validate_staging_directory()?;
        let path = history_staging_path(&directory, reference.id());
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(SegmentError::Io {
                    operation: "inspect staged history segment for cleanup",
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SegmentError::PathOutsideDatabase);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| SegmentError::Io {
            operation: "resolve staged history segment for cleanup",
            source,
        })?;
        if !canonical.starts_with(&directory) {
            return Err(SegmentError::PathOutsideDatabase);
        }
        fs::remove_file(&path).map_err(|source| SegmentError::Io {
            operation: "remove uncommitted staged history segment",
            source,
        })?;
        crate::manifest::sync_directory(&directory).map_err(|source| SegmentError::Io {
            operation: "sync staging directory after cleanup",
            source,
        })
    }

    pub(crate) fn reclaim_unreferenced_references(
        &self,
        references: &[ManifestSegmentReference],
    ) -> Result<Vec<ManifestSegmentReference>, SegmentError> {
        self.reclaim_unreferenced_references_with_checkpoint(references, |_| {})
    }

    fn reclaim_unreferenced_references_with_checkpoint(
        &self,
        references: &[ManifestSegmentReference],
        mut checkpoint: impl FnMut(ReclamationCheckpoint),
    ) -> Result<Vec<ManifestSegmentReference>, SegmentError> {
        let directory = self.validate_segments_directory()?;
        let mut reclaimed = Vec::new();
        reclaimed
            .try_reserve_exact(references.len())
            .map_err(|_| SegmentError::AllocationFailed)?;
        let mut changed = false;
        let result = (|| {
            for reference in references {
                if reference.kind() != ManifestSegmentKind::History {
                    return Err(SegmentError::InvalidEnvelope);
                }
                let path = segment_path(&directory, reference.id());
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        reclaimed.push(*reference);
                        continue;
                    }
                    Err(source) => {
                        return Err(SegmentError::Io {
                            operation: "inspect retired history segment",
                            source,
                        });
                    }
                };
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(SegmentError::PathOutsideDatabase);
                }
                let canonical = fs::canonicalize(&path).map_err(|source| SegmentError::Io {
                    operation: "resolve retired history segment",
                    source,
                })?;
                if !canonical.starts_with(&directory) {
                    return Err(SegmentError::PathOutsideDatabase);
                }
                let segment = self.read_segment(reference.id())?;
                if segment.content_digest() != reference.content_digest() {
                    return Err(SegmentError::ContentDigestMismatch);
                }
                fs::remove_file(&path).map_err(|source| SegmentError::Io {
                    operation: "reclaim retired history segment",
                    source,
                })?;
                changed = true;
                reclaimed.push(*reference);
                checkpoint(ReclamationCheckpoint::SegmentRemoved);
            }
            Ok(reclaimed)
        })();
        if changed {
            crate::manifest::sync_directory(&directory).map_err(|source| SegmentError::Io {
                operation: "sync history-segment directory after reclamation",
                source,
            })?;
            checkpoint(ReclamationCheckpoint::DirectorySynced);
        }
        result
    }

    /// Reads and validates a segment by its separate stable identity.
    pub fn read_segment(&self, id: SegmentId) -> Result<HistorySegment, SegmentError> {
        let directory = self.validate_segments_directory()?;
        let path = segment_path(&directory, id);
        let metadata = fs::symlink_metadata(&path).map_err(|source| SegmentError::Io {
            operation: "inspect history segment",
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SegmentError::PathOutsideDatabase);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| SegmentError::Io {
            operation: "resolve history segment",
            source,
        })?;
        if !canonical.starts_with(&directory) {
            return Err(SegmentError::PathOutsideDatabase);
        }
        let file_size = metadata.len();
        let file_size_usize = usize::try_from(file_size).unwrap_or(usize::MAX);
        if file_size_usize > MAX_FILE_BYTES {
            return Err(SegmentError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: MAX_FILE_BYTES,
                actual: file_size_usize,
            });
        }
        let file = File::open(&path).map_err(|source| SegmentError::Io {
            operation: "open history segment",
            source,
        })?;
        let opened_metadata = file.metadata().map_err(|source| SegmentError::Io {
            operation: "inspect opened history segment",
            source,
        })?;
        if !opened_metadata.is_file() || opened_metadata.len() != file_size {
            return Err(SegmentError::PathOutsideDatabase);
        }
        let reserve = usize::try_from(file_size).map_err(|_| SegmentError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: MAX_FILE_BYTES,
            actual: usize::MAX,
        })?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(reserve)
            .map_err(|_| SegmentError::AllocationFailed)?;
        file.take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| SegmentError::Io {
                operation: "read history segment",
                source,
            })?;
        if bytes.len() > MAX_FILE_BYTES || bytes.len() != reserve {
            return Err(SegmentError::InvalidEnvelope);
        }
        decode_segment_file(id, &bytes)
    }

    pub(crate) fn validate_manifest_reference(
        &self,
        reference: ManifestSegmentReference,
    ) -> Result<(), SegmentError> {
        if reference.kind() != ManifestSegmentKind::History {
            return Err(SegmentError::InvalidEnvelope);
        }
        let segment = self.read_segment(reference.id())?;
        if segment.content_digest() != reference.content_digest() {
            return Err(SegmentError::InvalidEnvelope);
        }
        Ok(())
    }

    pub(crate) fn read_verified_reference_bytes(
        &self,
        reference: ManifestSegmentReference,
        allow_staged: bool,
    ) -> Result<(Vec<u8>, usize, bool), SegmentError> {
        if reference.kind() != ManifestSegmentKind::History {
            return Err(SegmentError::InvalidEnvelope);
        }
        let final_directory = self.validate_segments_directory()?;
        let final_path = segment_path(&final_directory, reference.id());
        match read_verified_history_file(&final_path, &final_directory, reference) {
            Ok((bytes, segment)) => return Ok((bytes, segment.records().len(), false)),
            Err(final_error) if !allow_staged => return Err(final_error),
            Err(_) => {}
        }
        let staging_directory = self.validate_staging_directory()?;
        let staged_path = history_staging_path(&staging_directory, reference.id());
        let (bytes, segment) =
            read_verified_history_file(&staged_path, &staging_directory, reference)?;
        Ok((bytes, segment.records().len(), true))
    }

    pub(crate) fn read_verified_reference(
        &self,
        reference: ManifestSegmentReference,
        allow_staged: bool,
    ) -> Result<HistorySegment, SegmentError> {
        let (bytes, _, _) = self.read_verified_reference_bytes(reference, allow_staged)?;
        decode_segment_file(reference.id(), &bytes)
    }

    pub(crate) fn validate_staged_reference(
        &self,
        reference: ManifestSegmentReference,
    ) -> Result<(), SegmentError> {
        let path = self.staged_path(reference)?;
        read_and_validate_history_file(&path, reference)
    }

    pub(crate) fn materialize_staged_reference(
        &self,
        reference: ManifestSegmentReference,
    ) -> Result<(), SegmentError> {
        let destination_directory = self.validate_segments_directory()?;
        let source_directory = self.validate_staging_directory()?;
        let destination = segment_path(&destination_directory, reference.id());
        match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(SegmentError::PathOutsideDatabase);
                }
                self.validate_manifest_reference(reference)?;
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(SegmentError::Io {
                    operation: "inspect history-segment publication target",
                    source,
                });
            }
        }
        let source = history_staging_path(&source_directory, reference.id());
        read_and_validate_history_file(&source, reference)?;
        publish_staged_file(&source, &destination).map_err(|source| SegmentError::Io {
            operation: "publish staged immutable history segment",
            source,
        })?;
        crate::manifest::sync_directory(&destination_directory).map_err(|source| {
            SegmentError::Io {
                operation: "sync history-segment directory after publication",
                source,
            }
        })?;
        crate::manifest::sync_directory(&source_directory).map_err(|source| SegmentError::Io {
            operation: "sync staging directory after history-segment publication",
            source,
        })?;
        self.validate_manifest_reference(reference)
    }

    fn staged_path(&self, reference: ManifestSegmentReference) -> Result<PathBuf, SegmentError> {
        if reference.kind() != ManifestSegmentKind::History {
            return Err(SegmentError::InvalidEnvelope);
        }
        let directory = self.validate_staging_directory()?;
        Ok(history_staging_path(&directory, reference.id()))
    }

    fn write_frames(
        &self,
        writer_lock: &WriterLock,
        frames: Vec<Vec<u8>>,
        staged: bool,
    ) -> Result<HistorySegmentReceipt, SegmentError> {
        self.write_frames_with_checkpoint(writer_lock, frames, staged, |_| {})
    }

    fn write_frames_with_checkpoint(
        &self,
        writer_lock: &WriterLock,
        frames: Vec<Vec<u8>>,
        staged: bool,
        mut checkpoint: impl FnMut(SegmentIoCheckpoint),
    ) -> Result<HistorySegmentReceipt, SegmentError> {
        if !writer_lock.belongs_to_database_root(self.layout.root()) {
            return Err(SegmentError::ForeignWriterLock);
        }
        if !writer_lock.require_write_access() {
            return Err(SegmentError::RecoveryRequired);
        }
        let directory = if staged {
            self.validate_staging_directory()?
        } else {
            self.validate_segments_directory()?
        };
        let canonical = encode_canonical_content(frames)?;
        let content_digest = digest(&canonical);

        for _ in 0..SEGMENT_ID_ATTEMPTS {
            let id = generate_segment_id()?;
            let path = if staged {
                history_staging_path(&directory, id)
            } else {
                segment_path(&directory, id)
            };
            let mut file = match create_new_segment_file(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(SegmentError::Io {
                        operation: "create immutable history segment",
                        source,
                    });
                }
            };
            checkpoint(SegmentIoCheckpoint::StageFileCreated);
            let write_result = write_segment_file_with_checkpoint(
                &mut file,
                id,
                content_digest,
                &canonical,
                &mut checkpoint,
            );
            drop(file);
            if let Err(error) = write_result {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            if staged {
                crate::manifest::sync_directory(&directory).map_err(|source| SegmentError::Io {
                    operation: "sync history-segment staging directory",
                    source,
                })?;
                checkpoint(SegmentIoCheckpoint::StagingDirectorySynced);
            }
            let file_bytes = u64::try_from(ENVELOPE_BYTES + canonical.len()).map_err(|_| {
                SegmentError::ResourceLimit {
                    resource: DecodeResource::BatchBytes,
                    limit: MAX_FILE_BYTES,
                    actual: usize::MAX,
                }
            })?;
            return Ok(HistorySegmentReceipt {
                id,
                content_digest,
                record_count: index_record_count(&canonical)?,
                file_bytes,
            });
        }
        Err(SegmentError::SegmentIdCollisionExhausted)
    }

    fn validate_segments_directory(&self) -> Result<PathBuf, SegmentError> {
        let path = self.layout.segments_directory();
        let metadata = fs::symlink_metadata(&path).map_err(|source| SegmentError::Io {
            operation: "inspect history segment directory",
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SegmentError::PathOutsideDatabase);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| SegmentError::Io {
            operation: "resolve history segment directory",
            source,
        })?;
        if !canonical.starts_with(self.layout.root()) {
            return Err(SegmentError::PathOutsideDatabase);
        }
        Ok(canonical)
    }

    fn validate_staging_directory(&self) -> Result<PathBuf, SegmentError> {
        let path = self.layout.staging_directory();
        let metadata = fs::symlink_metadata(&path).map_err(|source| SegmentError::Io {
            operation: "inspect history-segment staging directory",
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SegmentError::PathOutsideDatabase);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| SegmentError::Io {
            operation: "resolve history-segment staging directory",
            source,
        })?;
        if !canonical.starts_with(self.layout.root()) {
            return Err(SegmentError::PathOutsideDatabase);
        }
        Ok(canonical)
    }
}

pub(crate) fn history_staging_path(directory: &Path, id: SegmentId) -> PathBuf {
    directory.join(format!("history-segment-{id}.stage"))
}

pub(crate) fn create_new_segment_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().create_new(true).write(true).open(path)
}

fn read_and_validate_history_file(
    path: &Path,
    reference: ManifestSegmentReference,
) -> Result<(), SegmentError> {
    let parent = path.parent().ok_or(SegmentError::PathOutsideDatabase)?;
    read_verified_history_file(path, parent, reference).map(|_| ())
}

fn read_verified_history_file(
    path: &Path,
    expected_directory: &Path,
    reference: ManifestSegmentReference,
) -> Result<(Vec<u8>, HistorySegment), SegmentError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| SegmentError::Io {
        operation: "inspect staged history segment",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SegmentError::PathOutsideDatabase);
    }
    let actual = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if actual > MAX_FILE_BYTES {
        return Err(SegmentError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: MAX_FILE_BYTES,
            actual,
        });
    }
    let canonical = fs::canonicalize(path).map_err(|source| SegmentError::Io {
        operation: "resolve staged history segment",
        source,
    })?;
    let canonical_parent =
        fs::canonicalize(expected_directory).map_err(|source| SegmentError::Io {
            operation: "resolve history-segment directory",
            source,
        })?;
    if canonical.parent() != Some(canonical_parent.as_path()) {
        return Err(SegmentError::PathOutsideDatabase);
    }
    let file = File::open(path).map_err(|source| SegmentError::Io {
        operation: "read staged history segment",
        source,
    })?;
    let opened = file.metadata().map_err(|source| SegmentError::Io {
        operation: "inspect opened staged history segment",
        source,
    })?;
    if !opened.is_file() || opened.len() != metadata.len() {
        return Err(SegmentError::InvalidEnvelope);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(actual)
        .map_err(|_| SegmentError::AllocationFailed)?;
    file.take(u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| SegmentError::Io {
            operation: "read staged history segment bytes",
            source,
        })?;
    if bytes.len() != actual || bytes.len() > MAX_FILE_BYTES {
        return Err(SegmentError::InvalidEnvelope);
    }
    let decoded = decode_segment_file(reference.id(), &bytes)?;
    if decoded.content_digest() != reference.content_digest() {
        return Err(SegmentError::InvalidEnvelope);
    }
    Ok((bytes, decoded))
}

fn publish_staged_file(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        crate::windows_publication::move_file(source, target, false)
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, target)
    }
}

fn encode_records(records: &[Record]) -> Result<Vec<Vec<u8>>, SegmentError> {
    if records.len() > DecoderLimits::DEFAULT.max_records_per_batch {
        return Err(SegmentError::ResourceLimit {
            resource: DecodeResource::BatchRecords,
            limit: DecoderLimits::DEFAULT.max_records_per_batch,
            actual: records.len(),
        });
    }
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(records.len())
        .map_err(|_| SegmentError::AllocationFailed)?;
    for record in records {
        frames.push(encode_record(record).map_err(SegmentError::RecordCodec)?);
    }
    Ok(frames)
}

fn encode_decoded_records(records: &[DecodedRecord]) -> Result<Vec<Vec<u8>>, SegmentError> {
    if records.len() > DecoderLimits::DEFAULT.max_records_per_batch {
        return Err(SegmentError::ResourceLimit {
            resource: DecodeResource::BatchRecords,
            limit: DecoderLimits::DEFAULT.max_records_per_batch,
            actual: records.len(),
        });
    }
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(records.len())
        .map_err(|_| SegmentError::AllocationFailed)?;
    for record in records {
        frames.push(encode_decoded_record(record).map_err(SegmentError::RecordCodec)?);
    }
    Ok(frames)
}

pub(crate) fn encode_canonical_content(mut frames: Vec<Vec<u8>>) -> Result<Vec<u8>, SegmentError> {
    if frames.is_empty() {
        return Err(SegmentError::EmptySegment);
    }
    if frames.len() > DecoderLimits::DEFAULT.max_records_per_batch {
        return Err(SegmentError::ResourceLimit {
            resource: DecodeResource::BatchRecords,
            limit: DecoderLimits::DEFAULT.max_records_per_batch,
            actual: frames.len(),
        });
    }
    frames.sort_unstable();
    let mut frame_bytes = 0_usize;
    for frame in &frames {
        if frame.len() > DecoderLimits::DEFAULT.max_frame_bytes {
            return Err(SegmentError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: DecoderLimits::DEFAULT.max_frame_bytes,
                actual: frame.len(),
            });
        }
        frame_bytes = frame_bytes
            .checked_add(frame.len())
            .ok_or(SegmentError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: DecoderLimits::DEFAULT.max_batch_bytes,
                actual: usize::MAX,
            })?;
        if frame_bytes > DecoderLimits::DEFAULT.max_batch_bytes {
            return Err(SegmentError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: DecoderLimits::DEFAULT.max_batch_bytes,
                actual: frame_bytes,
            });
        }
    }
    let index_bytes =
        frames
            .len()
            .checked_mul(INDEX_ENTRY_BYTES)
            .ok_or(SegmentError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: MAX_CANONICAL_BYTES,
                actual: usize::MAX,
            })?;
    let total = CANONICAL_HEADER_BYTES
        .checked_add(index_bytes)
        .and_then(|length| length.checked_add(frame_bytes))
        .ok_or(SegmentError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: MAX_CANONICAL_BYTES,
            actual: usize::MAX,
        })?;
    if total > MAX_CANONICAL_BYTES {
        return Err(SegmentError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: MAX_CANONICAL_BYTES,
            actual: total,
        });
    }
    let count = u32::try_from(frames.len()).map_err(|_| SegmentError::ResourceLimit {
        resource: DecodeResource::BatchRecords,
        limit: u32::MAX as usize,
        actual: frames.len(),
    })?;
    let mut canonical = Vec::new();
    canonical
        .try_reserve_exact(total)
        .map_err(|_| SegmentError::AllocationFailed)?;
    canonical.extend_from_slice(&SEGMENT_FORMAT_MAJOR.to_le_bytes());
    canonical.extend_from_slice(&SEGMENT_FORMAT_MINOR.to_le_bytes());
    canonical.extend_from_slice(&count.to_le_bytes());

    let mut offset = u64::try_from(CANONICAL_HEADER_BYTES + index_bytes).map_err(|_| {
        SegmentError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: MAX_CANONICAL_BYTES,
            actual: usize::MAX,
        }
    })?;
    for frame in &frames {
        canonical.extend_from_slice(&offset.to_le_bytes());
        let length = u64::try_from(frame.len()).map_err(|_| SegmentError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: MAX_CANONICAL_BYTES,
            actual: usize::MAX,
        })?;
        canonical.extend_from_slice(&length.to_le_bytes());
        offset = offset
            .checked_add(length)
            .ok_or(SegmentError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: MAX_CANONICAL_BYTES,
                actual: usize::MAX,
            })?;
    }
    for frame in frames {
        canonical.extend_from_slice(&frame);
    }
    Ok(canonical)
}

pub(crate) fn digest(canonical: &[u8]) -> ContentDigest {
    ContentDigest(*blake3::hash(canonical).as_bytes())
}

pub(crate) fn generate_segment_id() -> Result<SegmentId, SegmentError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| SegmentError::EntropyUnavailable)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    SegmentId::try_from_bytes(bytes).map_err(SegmentError::InvalidSegmentId)
}

pub(crate) fn segment_path(directory: &Path, id: SegmentId) -> PathBuf {
    directory.join(format!("segment-{}.wdbseg", id.to_canonical_string()))
}

pub(crate) fn write_segment_file_with_checkpoint(
    file: &mut File,
    id: SegmentId,
    content_digest: ContentDigest,
    canonical: &[u8],
    checkpoint: &mut impl FnMut(SegmentIoCheckpoint),
) -> Result<(), SegmentError> {
    file.write_all(&SEGMENT_MAGIC)
        .map_err(|source| SegmentError::Io {
            operation: "write history segment magic",
            source,
        })?;
    checkpoint(SegmentIoCheckpoint::MagicWritten);
    file.write_all(&id.to_bytes())
        .map_err(|source| SegmentError::Io {
            operation: "write history segment identity",
            source,
        })?;
    checkpoint(SegmentIoCheckpoint::IdentityWritten);
    file.write_all(content_digest.as_bytes())
        .map_err(|source| SegmentError::Io {
            operation: "write history segment digest",
            source,
        })?;
    checkpoint(SegmentIoCheckpoint::DigestWritten);
    file.write_all(canonical)
        .map_err(|source| SegmentError::Io {
            operation: "write canonical history segment content",
            source,
        })?;
    checkpoint(SegmentIoCheckpoint::ContentWritten);
    crate::platform_sync::sync_file(file).map_err(|source| SegmentError::Io {
        operation: "sync immutable history segment",
        source,
    })?;
    checkpoint(SegmentIoCheckpoint::FileSynced);
    Ok(())
}

fn decode_segment_file(
    requested_id: SegmentId,
    bytes: &[u8],
) -> Result<HistorySegment, SegmentError> {
    let (stored_digest, canonical) = decode_segment_envelope(requested_id, bytes)?;
    let records = decode_canonical_content(canonical)?;
    Ok(HistorySegment {
        id: requested_id,
        content_digest: stored_digest,
        records,
    })
}

/// Validates the shared segment envelope and returns its canonical payload.
pub(crate) fn decode_segment_envelope(
    requested_id: SegmentId,
    bytes: &[u8],
) -> Result<(ContentDigest, &[u8]), SegmentError> {
    if bytes.len() < ENVELOPE_BYTES + CANONICAL_HEADER_BYTES
        || bytes.get(..SEGMENT_MAGIC.len()) != Some(SEGMENT_MAGIC.as_slice())
    {
        return Err(SegmentError::InvalidEnvelope);
    }
    let mut id_bytes = [0_u8; 16];
    id_bytes.copy_from_slice(bytes.get(8..24).ok_or(SegmentError::InvalidEnvelope)?);
    let file_id = SegmentId::try_from_bytes(id_bytes).map_err(SegmentError::InvalidSegmentId)?;
    if file_id != requested_id {
        return Err(SegmentError::SegmentIdMismatch);
    }
    let mut digest_bytes = [0_u8; 32];
    digest_bytes.copy_from_slice(
        bytes
            .get(24..ENVELOPE_BYTES)
            .ok_or(SegmentError::InvalidEnvelope)?,
    );
    let stored_digest = ContentDigest::from_bytes(digest_bytes);
    let canonical = bytes
        .get(ENVELOPE_BYTES..)
        .ok_or(SegmentError::InvalidEnvelope)?;
    let calculated_digest = digest(canonical);
    if calculated_digest != stored_digest {
        return Err(SegmentError::ContentDigestMismatch);
    }
    Ok((stored_digest, canonical))
}

fn decode_canonical_content(canonical: &[u8]) -> Result<Vec<DecodedRecord>, SegmentError> {
    let frames = decode_canonical_frames(canonical)?;
    decode_record_batch_with_limits(&frames, &DecoderLimits::DEFAULT)
        .map_err(SegmentError::RecordCodec)
}

/// Verifies a segment's canonical index and returns its ordered borrowed frames.
pub(crate) fn decode_canonical_frames(canonical: &[u8]) -> Result<Vec<&[u8]>, SegmentError> {
    if canonical.len() < CANONICAL_HEADER_BYTES || canonical.len() > MAX_CANONICAL_BYTES {
        return Err(SegmentError::InvalidContentIndex);
    }
    let major = read_u16(canonical, 0).ok_or(SegmentError::InvalidContentIndex)?;
    let minor = read_u16(canonical, 2).ok_or(SegmentError::InvalidContentIndex)?;
    if major != SEGMENT_FORMAT_MAJOR || minor != SEGMENT_FORMAT_MINOR {
        return Err(SegmentError::UnsupportedVersion { major, minor });
    }
    let count = usize::try_from(read_u32(canonical, 4).ok_or(SegmentError::InvalidContentIndex)?)
        .map_err(|_| SegmentError::InvalidContentIndex)?;
    if count == 0 {
        return Err(SegmentError::EmptySegment);
    }
    if count > DecoderLimits::DEFAULT.max_records_per_batch {
        return Err(SegmentError::ResourceLimit {
            resource: DecodeResource::BatchRecords,
            limit: DecoderLimits::DEFAULT.max_records_per_batch,
            actual: count,
        });
    }
    let index_bytes = count
        .checked_mul(INDEX_ENTRY_BYTES)
        .ok_or(SegmentError::InvalidContentIndex)?;
    let index_end = CANONICAL_HEADER_BYTES
        .checked_add(index_bytes)
        .ok_or(SegmentError::InvalidContentIndex)?;
    if index_end > canonical.len() {
        return Err(SegmentError::InvalidContentIndex);
    }
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(count)
        .map_err(|_| SegmentError::AllocationFailed)?;
    let mut expected_offset =
        u64::try_from(index_end).map_err(|_| SegmentError::InvalidContentIndex)?;
    let mut total_frame_bytes = 0_usize;
    let mut previous_frame: Option<&[u8]> = None;
    for index in 0..count {
        let entry_start = CANONICAL_HEADER_BYTES
            .checked_add(
                index
                    .checked_mul(INDEX_ENTRY_BYTES)
                    .ok_or(SegmentError::InvalidContentIndex)?,
            )
            .ok_or(SegmentError::InvalidContentIndex)?;
        let offset = read_u64(canonical, entry_start).ok_or(SegmentError::InvalidContentIndex)?;
        let length =
            read_u64(canonical, entry_start + 8).ok_or(SegmentError::InvalidContentIndex)?;
        if offset != expected_offset || length == 0 {
            return Err(SegmentError::InvalidContentIndex);
        }
        let end = offset
            .checked_add(length)
            .ok_or(SegmentError::InvalidContentIndex)?;
        let start = usize::try_from(offset).map_err(|_| SegmentError::InvalidContentIndex)?;
        let end_index = usize::try_from(end).map_err(|_| SegmentError::InvalidContentIndex)?;
        if end_index > canonical.len() {
            return Err(SegmentError::InvalidContentIndex);
        }
        let frame = canonical
            .get(start..end_index)
            .ok_or(SegmentError::InvalidContentIndex)?;
        if frame.len() > DecoderLimits::DEFAULT.max_frame_bytes {
            return Err(SegmentError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: DecoderLimits::DEFAULT.max_frame_bytes,
                actual: frame.len(),
            });
        }
        if previous_frame.is_some_and(|previous| previous > frame) {
            return Err(SegmentError::InvalidContentIndex);
        }
        previous_frame = Some(frame);
        total_frame_bytes = total_frame_bytes
            .checked_add(frame.len())
            .ok_or(SegmentError::InvalidContentIndex)?;
        frames.push(frame);
        expected_offset = end;
    }
    if usize::try_from(expected_offset).ok() != Some(canonical.len())
        || total_frame_bytes > DecoderLimits::DEFAULT.max_batch_bytes
    {
        return Err(SegmentError::InvalidContentIndex);
    }
    Ok(frames)
}

fn index_record_count(canonical: &[u8]) -> Result<usize, SegmentError> {
    read_u32(canonical, 4)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or(SegmentError::InvalidContentIndex)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let array: [u8; 2] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(array))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let array: [u8; 4] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(array))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let array: [u8; 8] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(array))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    #[cfg(windows)]
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::create_new_segment_file;
    #[cfg(windows)]
    use super::{ContentDigest, segment_path};
    #[cfg(windows)]
    use super::{HistorySegmentStore, ReclamationCheckpoint, SegmentIoCheckpoint};
    #[cfg(windows)]
    use crate::DatabaseLayout;
    #[cfg(windows)]
    use crate::SegmentId;
    #[cfg(windows)]
    use crate::{
        ManifestSegmentKind, ManifestSegmentReference, ManifestStore, RecoveryManager,
        RecoveryScanner, WalPrepareLog,
    };
    #[cfg(windows)]
    use worlddb_core::{
        DomainId, HistorySpaceDefinition, HistorySpaceId, OperationId, Record, Revision,
    };

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);
    #[cfg(windows)]
    static NEXT_TEMP_DATABASE: AtomicU64 = AtomicU64::new(0);

    struct TempFile(PathBuf);

    impl TempFile {
        fn create() -> io::Result<Self> {
            let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-segment-create-new-{}-{sequence}.tmp",
                std::process::id()
            ));
            fs::write(&path, b"original immutable segment bytes")?;
            Ok(Self(path))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[cfg(windows)]
    struct TempDatabase(PathBuf);

    #[cfg(windows)]
    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DATABASE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-segment-crash-{}-{sequence}",
                std::process::id()
            ));
            DatabaseLayout::create(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        fn layout(&self) -> Result<DatabaseLayout, String> {
            DatabaseLayout::open(&self.0).map_err(|error| error.to_string())
        }
    }

    #[cfg(windows)]
    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(windows)]
    fn operation_id(tail: u8) -> Result<OperationId, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        OperationId::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    #[cfg(windows)]
    fn hex<const N: usize>(bytes: [u8; N]) -> String {
        use std::fmt::Write as _;

        let mut output = String::with_capacity(N * 2);
        for byte in bytes {
            let _ = write!(output, "{byte:02x}");
        }
        output
    }

    #[cfg(windows)]
    fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], String> {
        if value.len() != N * 2 {
            return Err(format!("expected {} hexadecimal bytes", N));
        }
        let mut output = [0_u8; N];
        for (index, byte) in output.iter_mut().enumerate() {
            let start = index * 2;
            *byte = u8::from_str_radix(&value[start..start + 2], 16)
                .map_err(|error| error.to_string())?;
        }
        Ok(output)
    }

    #[test]
    fn create_new_file_refuses_to_overwrite_existing_segment_bytes() -> io::Result<()> {
        let existing = TempFile::create()?;
        assert!(matches!(
            create_new_segment_file(&existing.0),
            Err(ref error) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(fs::read(&existing.0)?, b"original immutable segment bytes");
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn process_crash_at_each_history_segment_stage_write_recovers_genesis() -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M5_22_SEGMENT_WRITE_CRASH_ROOT";
        const POINT_ENV: &str = "WORLDDB_M5_22_SEGMENT_WRITE_CRASH_POINT";
        const TEST_NAME: &str =
            "segment::tests::process_crash_at_each_history_segment_stage_write_recovers_genesis";

        if let (Ok(root), Ok(point_name)) = (env::var(ROOT_ENV), env::var(POINT_ENV)) {
            let point = match point_name.as_str() {
                "stage_file_created" => SegmentIoCheckpoint::StageFileCreated,
                "magic_written" => SegmentIoCheckpoint::MagicWritten,
                "identity_written" => SegmentIoCheckpoint::IdentityWritten,
                "digest_written" => SegmentIoCheckpoint::DigestWritten,
                "content_written" => SegmentIoCheckpoint::ContentWritten,
                "file_synced" => SegmentIoCheckpoint::FileSynced,
                "staging_directory_synced" => SegmentIoCheckpoint::StagingDirectorySynced,
                _ => return Err(format!("unknown history-segment checkpoint {point_name}")),
            };
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let store = HistorySegmentStore::new(layout);
            let _ = store.write_frames_with_checkpoint(
                &lock,
                vec![b"m5-22-test-frame".to_vec()],
                true,
                |checkpoint| {
                    if checkpoint == point {
                        std::process::exit(86);
                    }
                },
            );
            return Err(format!(
                "child did not reach segment checkpoint {point_name}"
            ));
        }

        for (point, point_name) in [
            (SegmentIoCheckpoint::StageFileCreated, "stage_file_created"),
            (SegmentIoCheckpoint::MagicWritten, "magic_written"),
            (SegmentIoCheckpoint::IdentityWritten, "identity_written"),
            (SegmentIoCheckpoint::DigestWritten, "digest_written"),
            (SegmentIoCheckpoint::ContentWritten, "content_written"),
            (SegmentIoCheckpoint::FileSynced, "file_synced"),
            (
                SegmentIoCheckpoint::StagingDirectorySynced,
                "staging_directory_synced",
            ),
        ] {
            let database = TempDatabase::create()?;
            let executable = env::current_exe().map_err(|error| error.to_string())?;
            let status = crate::writer_lock::test_command_status(
                Command::new(executable)
                    .args(["--exact", TEST_NAME, "--nocapture"])
                    .env(ROOT_ENV, &database.0)
                    .env(POINT_ENV, point_name)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null()),
            )
            .map_err(|error| error.to_string())?;
            if status.code() != Some(86) {
                return Err(format!(
                    "child for {point:?} exited with {:?}, expected crash code 86",
                    status.code()
                ));
            }
            let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let report = RecoveryScanner::new(layout.clone())
                .scan(&lock)
                .map_err(|error| error.to_string())?;
            assert!(report.is_clean());
            assert_eq!(report.safe_revision(), Revision::GENESIS);
            let recovered = RecoveryManager::new(layout)
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            assert!(recovered.report().is_clean());
            assert_eq!(recovered.report().safe_revision(), Revision::GENESIS);
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn process_crash_at_each_retired_segment_delete_boundary_is_retryable() -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M5_22_RECLAIM_CRASH_ROOT";
        const POINT_ENV: &str = "WORLDDB_M5_22_RECLAIM_CRASH_POINT";
        const ID_ENV: &str = "WORLDDB_M5_22_RECLAIM_SEGMENT_ID";
        const DIGEST_ENV: &str = "WORLDDB_M5_22_RECLAIM_SEGMENT_DIGEST";
        const TEST_NAME: &str =
            "segment::tests::process_crash_at_each_retired_segment_delete_boundary_is_retryable";

        if let (Ok(root), Ok(point_name), Ok(id_hex), Ok(digest_hex)) = (
            env::var(ROOT_ENV),
            env::var(POINT_ENV),
            env::var(ID_ENV),
            env::var(DIGEST_ENV),
        ) {
            let point = match point_name.as_str() {
                "segment_removed" => ReclamationCheckpoint::SegmentRemoved,
                "directory_synced" => ReclamationCheckpoint::DirectorySynced,
                _ => return Err(format!("unknown reclamation checkpoint {point_name}")),
            };
            let id = SegmentId::try_from_bytes(decode_hex::<16>(&id_hex)?)
                .map_err(|error| error.to_string())?;
            let reference = ManifestSegmentReference::new(
                ManifestSegmentKind::History,
                id,
                ContentDigest::from_bytes(decode_hex::<32>(&digest_hex)?),
                Revision::FIRST_COMMIT,
            );
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            RecoveryManager::new(layout.clone())
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            let store = HistorySegmentStore::new(layout);
            let _ =
                store.reclaim_unreferenced_references_with_checkpoint(&[reference], |checkpoint| {
                    if checkpoint == point {
                        std::process::exit(86);
                    }
                });
            return Err(format!(
                "child did not reach reclamation checkpoint {point_name}"
            ));
        }

        for (point, point_name) in [
            (ReclamationCheckpoint::SegmentRemoved, "segment_removed"),
            (ReclamationCheckpoint::DirectorySynced, "directory_synced"),
        ] {
            let database = TempDatabase::create()?;
            let layout = database.layout()?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            RecoveryManager::new(layout.clone())
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            let store = HistorySegmentStore::new(layout.clone());
            let mut history_space_bytes = [0_u8; 16];
            history_space_bytes[6] = 0x70;
            history_space_bytes[8] = 0x80;
            history_space_bytes[15] = 1;
            let history_space = HistorySpaceId::try_from_bytes(history_space_bytes)
                .map_err(|error| error.to_string())?;
            let record = Record::HistorySpaceDefinition(
                HistorySpaceDefinition::new(history_space, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
            );
            let frame = super::encode_record(&record).map_err(|error| error.to_string())?;
            let receipt = store
                .write_frames(&lock, vec![frame], false)
                .map_err(|error| error.to_string())?;
            let reference = ManifestSegmentReference::new(
                ManifestSegmentKind::History,
                receipt.id(),
                receipt.content_digest(),
                Revision::FIRST_COMMIT,
            );
            let wal = WalPrepareLog::new(&layout);
            wal.commit_manifest_snapshot(&lock, operation_id(1)?, vec![reference], &[])
                .map_err(|error| error.to_string())?;
            RecoveryManager::new(layout.clone())
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            wal.commit_manifest_snapshot(&lock, operation_id(2)?, vec![], &[])
                .map_err(|error| error.to_string())?;
            RecoveryManager::new(layout.clone())
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            let retired_path = segment_path(&layout.segments_directory(), reference.id());
            assert!(retired_path.is_file());
            drop(lock);

            let status = crate::writer_lock::test_command_status(
                Command::new(env::current_exe().map_err(|error| error.to_string())?)
                    .args(["--exact", TEST_NAME, "--nocapture"])
                    .env(ROOT_ENV, &database.0)
                    .env(POINT_ENV, point_name)
                    .env(ID_ENV, hex(reference.id().to_bytes()))
                    .env(DIGEST_ENV, hex(*reference.content_digest().as_bytes()))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null()),
            )
            .map_err(|error| error.to_string())?;
            if status.code() != Some(86) {
                return Err(format!(
                    "child for {point:?} exited with {:?}, expected crash code 86",
                    status.code()
                ));
            }

            let reopened = database.layout()?;
            let reopened_lock = reopened
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let recovered = RecoveryManager::new(reopened.clone())
                .recover(&reopened_lock)
                .map_err(|error| error.to_string())?;
            assert!(recovered.report().is_clean());
            assert_eq!(
                recovered.report().safe_revision(),
                Revision::new(2).map_err(|e| e.to_string())?
            );
            let current = ManifestStore::new(reopened.clone())
                .read_current()
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "current manifest is missing after recovery".to_owned())?;
            assert!(current.segments().is_empty());

            let retried = HistorySegmentStore::new(reopened)
                .reclaim_unreferenced_references(&[reference])
                .map_err(|error| error.to_string())?;
            assert_eq!(retried, vec![reference]);
            assert!(!retired_path.exists());
        }
        Ok(())
    }
}
