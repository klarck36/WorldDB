//! Pinned-snapshot index rebuilding and atomic per-family generation publication.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use worlddb_core::{
    DatabaseId, DecoderLimits, IndexBuildVersion, IndexFamily, IndexFormatVersion,
    IndexGenerationMetadata, IndexMetadataError, IndexRevisionCoverage, IndexSchemaVersion,
    Revision, RevisionError,
};

use crate::{
    DatabaseLayout, IndexGenerationError, WriterLock, WriterLockError, decode_index_generation,
    encode_index_generation,
};

const POINTER_MAGIC: &[u8; 8] = b"WDBIDXCP";
const POINTER_VERSION: u16 = 1;
const POINTER_BODY_BYTES: usize = 56;
const POINTER_BYTES: usize = POINTER_BODY_BYTES + 32;
const GENERATION_FILE_SUFFIX: &str = ".wdig";
const MAX_STAGE_ATTEMPTS: usize = 32;
static NEXT_STAGE_FILE: AtomicU64 = AtomicU64::new(0);

/// A snapshot pin that keeps its immutable source revision available until dropped.
///
/// Implementations must retain all storage needed to read `revision()` while the value lives.
/// The rebuild manager keeps this value alive through publication of the new generation.
pub trait PinnedIndexSnapshot {
    /// Exact database revision represented by this pinned snapshot.
    fn revision(&self) -> Revision;
}

/// Source operations needed to build one index family from a pinned snapshot and committed deltas.
///
/// `deltas_after` must return exactly one ordered delta for every committed revision in the
/// requested inclusive interval. `current_head` and publication must observe the same database
/// identified by `database_root`. Commits must use the database writer lock so the final locked
/// head check remains stable through the atomic pointer switch.
pub trait IndexRebuildSource {
    /// Snapshot pin type retained until the new generation is durably published.
    type Snapshot: PinnedIndexSnapshot;
    /// Mutable in-memory representation consumed by the family-specific encoder.
    type State;
    /// One deterministic change for a single committed revision.
    type Delta;
    /// Source-specific failure value.
    type Error: fmt::Display;

    /// Canonical or resolvable database-root path backing this source.
    fn database_root(&self) -> &Path;
    /// Pins the current source revision and its backing data.
    fn pin_snapshot(&self) -> Result<Self::Snapshot, Self::Error>;
    /// Builds the complete family state from the pinned snapshot only.
    fn build_from_snapshot(&self, snapshot: &Self::Snapshot) -> Result<Self::State, Self::Error>;
    /// Reads the latest committed data/schema revision.
    fn current_head(&self) -> Result<Revision, Self::Error>;
    /// Loads every family-relevant commit delta in `(after, through]`, in revision order.
    fn deltas_after(
        &self,
        after: Revision,
        through: Revision,
    ) -> Result<Vec<Self::Delta>, Self::Error>;
    /// Returns the revision represented by one delta.
    fn delta_revision(&self, delta: &Self::Delta) -> Revision;
    /// Applies one validated contiguous delta to the in-memory family state.
    fn apply_delta(&self, state: &mut Self::State, delta: &Self::Delta) -> Result<(), Self::Error>;
    /// Encodes the complete state deterministically as the family-specific payload.
    fn encode_index(&self, state: &Self::State) -> Result<Vec<u8>, Self::Error>;
}

/// Bounds online catch-up work while allowing the caller to retry an incomplete rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexRebuildLimits {
    /// Maximum number of batches read while the database remains writable.
    pub max_catch_up_rounds: usize,
    /// Maximum total per-revision deltas applied to one generation.
    pub max_deltas: usize,
}

impl IndexRebuildLimits {
    /// Creates explicit catch-up bounds.
    #[must_use]
    pub const fn new(max_catch_up_rounds: usize, max_deltas: usize) -> Self {
        Self {
            max_catch_up_rounds,
            max_deltas,
        }
    }
}

impl Default for IndexRebuildLimits {
    fn default() -> Self {
        Self::new(64, 1_000_000)
    }
}

/// One fully checked, immutable generation published through the family's atomic pointer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredIndexGeneration {
    metadata: IndexGenerationMetadata,
    payload: Vec<u8>,
    file_digest: [u8; 32],
}

/// One verified immutable index file found by a complete directory inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexGenerationInventoryEntry {
    family: IndexFamily,
    generation_id: u64,
    file_digest: [u8; 32],
    current: bool,
}

impl IndexGenerationInventoryEntry {
    /// Index family encoded in the generation metadata.
    #[must_use]
    pub const fn family(self) -> IndexFamily {
        self.family
    }

    /// Immutable generation number.
    #[must_use]
    pub const fn generation_id(self) -> u64 {
        self.generation_id
    }

    /// Digest of the complete encoded generation file.
    #[must_use]
    pub const fn file_digest(self) -> [u8; 32] {
        self.file_digest
    }

    /// Whether the family's validated current pointer selects this generation.
    #[must_use]
    pub const fn current(self) -> bool {
        self.current
    }
}

/// Complete, verified inventory of a database's local derived-index generations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexStorageInventory {
    database_id: DatabaseId,
    generations: Vec<IndexGenerationInventoryEntry>,
}

impl IndexStorageInventory {
    /// Database identity whose index directory was scanned.
    #[must_use]
    pub const fn database_id(&self) -> DatabaseId {
        self.database_id
    }

    /// Every recognized generation file in canonical family/generation order.
    #[must_use]
    pub fn generations(&self) -> &[IndexGenerationInventoryEntry] {
        &self.generations
    }
}

impl StoredIndexGeneration {
    /// Verified family, generation, version, and revision coverage.
    #[must_use]
    pub const fn metadata(&self) -> IndexGenerationMetadata {
        self.metadata
    }

    /// Opaque family-specific index bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Digest of the complete encoded generation file.
    #[must_use]
    pub const fn file_digest(&self) -> [u8; 32] {
        self.file_digest
    }
}

/// Result of one successful online rebuild and atomic publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceipt {
    metadata: IndexGenerationMetadata,
    file_digest: [u8; 32],
    applied_deltas: usize,
    catch_up_rounds: usize,
}

impl IndexRebuildReceipt {
    /// Published generation metadata.
    #[must_use]
    pub const fn metadata(self) -> IndexGenerationMetadata {
        self.metadata
    }

    /// Digest referenced by the family's checksummed current pointer.
    #[must_use]
    pub const fn file_digest(self) -> [u8; 32] {
        self.file_digest
    }

    /// Number of post-snapshot revisions applied before publication.
    #[must_use]
    pub const fn applied_deltas(self) -> usize {
        self.applied_deltas
    }

    /// Number of batches used to catch the family state up to the locked head.
    #[must_use]
    pub const fn catch_up_rounds(self) -> usize {
        self.catch_up_rounds
    }
}

/// Failure to rebuild, validate, or atomically publish an index generation.
#[derive(Debug)]
pub enum IndexRebuildError {
    /// The supplied source and storage layout refer to different database roots.
    SourceDatabaseMismatch,
    /// The database layout has no durable database identity.
    DatabaseIdentityMissing,
    /// The acquired writer lock belongs to a different database.
    ForeignWriterLock,
    /// Recovery has not authorized writes to this database.
    RecoveryRequired,
    /// The source head regressed behind the generation already built in memory.
    HeadBehindCoverage { head: Revision, covered: Revision },
    /// A revision delta is absent, duplicated, or out of sequence.
    RevisionGap {
        expected: Revision,
        actual: Option<Revision>,
    },
    /// A source returned a delta later than the requested head.
    DeltaBeyondHead { delta: Revision, head: Revision },
    /// The source returned an incomplete batch for a known head.
    IncompleteDeltaRange { covered: Revision, head: Revision },
    /// The source head changed after the exclusive writer lock was acquired.
    HeadChangedWhileLocked {
        observed: Revision,
        covered: Revision,
    },
    /// The rebuild exceeded its bounded catch-up rounds.
    CatchUpRoundLimit { limit: usize },
    /// The rebuild exceeded its bounded total delta count.
    DeltaLimitExceeded { limit: usize },
    /// Revision numbering could not advance to the next required commit.
    RevisionOverflow(RevisionError),
    /// The family pointer has an invalid length, checksum, version, or family tag.
    InvalidCurrentPointer,
    /// A family generation filename is malformed or names generation zero.
    InvalidGenerationFileName,
    /// An immutable generation number already exists and will never be overwritten.
    GenerationAlreadyExists,
    /// The family generation sequence cannot advance.
    GenerationOverflow,
    /// A database path is a link, wrong entry type, or resolves outside the database root.
    PathOutsideDatabase,
    /// An on-disk index file exceeds the finite configured read bound.
    ResourceLimitExceeded { limit: usize, actual: usize },
    /// An index file changed size while it was being read.
    FileChangedDuringRead,
    /// A generation's declared metadata is invalid.
    Metadata(IndexMetadataError),
    /// An encoded index generation is invalid or exceeds finite decoder limits.
    Generation(IndexGenerationError),
    /// A writer lock could not be acquired.
    WriterLock(WriterLockError),
    /// A source operation failed.
    Source {
        operation: &'static str,
        message: String,
    },
    /// A filesystem read or publication step failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for IndexRebuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceDatabaseMismatch => {
                formatter.write_str("index source belongs to a different database")
            }
            Self::DatabaseIdentityMissing => {
                formatter.write_str("index inventory requires a durable database identity")
            }
            Self::ForeignWriterLock => {
                formatter.write_str("index publication received a foreign writer lock")
            }
            Self::RecoveryRequired => {
                formatter.write_str("index publication is blocked until recovery succeeds")
            }
            Self::HeadBehindCoverage { head, covered } => write!(
                formatter,
                "source head {head} is behind already-built revision {covered}"
            ),
            Self::RevisionGap { expected, actual } => match actual {
                Some(actual) => write!(
                    formatter,
                    "index delta revision {actual} does not match expected revision {expected}"
                ),
                None => write!(
                    formatter,
                    "index delta stream ended before expected revision {expected}"
                ),
            },
            Self::DeltaBeyondHead { delta, head } => write!(
                formatter,
                "index delta revision {delta} is beyond requested head {head}"
            ),
            Self::IncompleteDeltaRange { covered, head } => write!(
                formatter,
                "index deltas reached {covered}, short of requested head {head}"
            ),
            Self::HeadChangedWhileLocked { observed, covered } => write!(
                formatter,
                "source head changed to {observed} while writer lock was held at {covered}"
            ),
            Self::CatchUpRoundLimit { limit } => {
                write!(formatter, "index catch-up exceeded {limit} rounds")
            }
            Self::DeltaLimitExceeded { limit } => {
                write!(formatter, "index catch-up exceeded {limit} deltas")
            }
            Self::RevisionOverflow(error) => {
                write!(formatter, "revision sequence overflow: {error}")
            }
            Self::InvalidCurrentPointer => {
                formatter.write_str("index-family current pointer is invalid")
            }
            Self::InvalidGenerationFileName => {
                formatter.write_str("index-generation filename is invalid")
            }
            Self::GenerationAlreadyExists => {
                formatter.write_str("immutable index generation already exists")
            }
            Self::GenerationOverflow => {
                formatter.write_str("index-family generation sequence overflow")
            }
            Self::PathOutsideDatabase => {
                formatter.write_str("index path is not a regular entry inside the database root")
            }
            Self::ResourceLimitExceeded { limit, actual } => write!(
                formatter,
                "index file is {actual} bytes; configured read maximum is {limit}"
            ),
            Self::FileChangedDuringRead => {
                formatter.write_str("index file changed size while it was being read")
            }
            Self::Metadata(error) => write!(formatter, "index metadata is invalid: {error}"),
            Self::Generation(error) => write!(formatter, "index generation is invalid: {error}"),
            Self::WriterLock(error) => error.fmt(formatter),
            Self::Source { operation, message } => write!(formatter, "{operation}: {message}"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl std::error::Error for IndexRebuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata(error) => Some(error),
            Self::Generation(error) => Some(error),
            Self::RevisionOverflow(error) => Some(error),
            Self::WriterLock(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::SourceDatabaseMismatch
            | Self::DatabaseIdentityMissing
            | Self::ForeignWriterLock
            | Self::RecoveryRequired
            | Self::HeadBehindCoverage { .. }
            | Self::RevisionGap { .. }
            | Self::DeltaBeyondHead { .. }
            | Self::IncompleteDeltaRange { .. }
            | Self::HeadChangedWhileLocked { .. }
            | Self::CatchUpRoundLimit { .. }
            | Self::DeltaLimitExceeded { .. }
            | Self::InvalidCurrentPointer
            | Self::InvalidGenerationFileName
            | Self::GenerationAlreadyExists
            | Self::GenerationOverflow
            | Self::PathOutsideDatabase
            | Self::ResourceLimitExceeded { .. }
            | Self::FileChangedDuringRead
            | Self::Source { .. } => None,
        }
    }
}

/// Read-only access to one family's immutable generations and current pointer.
#[derive(Clone, Debug)]
pub struct IndexGenerationStore {
    layout: DatabaseLayout,
    family: IndexFamily,
}

impl IndexGenerationStore {
    /// Binds the store to one index family under a validated database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout, family: IndexFamily) -> Self {
        Self { layout, family }
    }

    /// Scans and validates every local generation file and current-family pointer.
    ///
    /// Unknown files, malformed names, links, corrupt generations, and dangling pointers fail
    /// closed instead of producing an incomplete inventory.
    pub fn inventory_all(
        layout: &DatabaseLayout,
    ) -> Result<IndexStorageInventory, IndexRebuildError> {
        let writer_lock = layout
            .try_writer_lock()
            .map_err(IndexRebuildError::WriterLock)?;
        Self::inventory_all_locked(layout, &writer_lock)
    }

    pub(crate) fn inventory_all_locked(
        layout: &DatabaseLayout,
        writer_lock: &WriterLock,
    ) -> Result<IndexStorageInventory, IndexRebuildError> {
        if !writer_lock.belongs_to_database_root(layout.root()) {
            return Err(IndexRebuildError::ForeignWriterLock);
        }
        let database_id = layout
            .database_id()
            .ok_or(IndexRebuildError::DatabaseIdentityMissing)?;
        let directory = layout.indexes_directory();
        match fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(IndexStorageInventory {
                    database_id,
                    generations: Vec::new(),
                });
            }
            Err(source) => {
                return Err(IndexRebuildError::Io {
                    operation: "inspect index directory for purge inventory",
                    source,
                });
            }
            Ok(_) => validate_directory_inside_root(layout, &directory)?,
        }

        let mut generations = Vec::new();
        let mut current = std::collections::BTreeMap::<u16, (u64, [u8; 32])>::new();
        for entry in fs::read_dir(&directory).map_err(|source| IndexRebuildError::Io {
            operation: "list index files for purge inventory",
            source,
        })? {
            let entry = entry.map_err(|source| IndexRebuildError::Io {
                operation: "read index inventory directory entry",
                source,
            })?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| IndexRebuildError::InvalidGenerationFileName)?;
            if name.starts_with("index-") && name.ends_with(".CURRENT") {
                let family = family_from_index_file_name(&name)?;
                if current.contains_key(&family.wire_tag())
                    || current_pointer_path(&directory, family)
                        .file_name()
                        .and_then(|value| value.to_str())
                        != Some(name.as_str())
                {
                    return Err(IndexRebuildError::InvalidCurrentPointer);
                }
                let generation = IndexGenerationStore::new(layout.clone(), family)
                    .read_current()?
                    .ok_or(IndexRebuildError::InvalidCurrentPointer)?;
                current.insert(
                    family.wire_tag(),
                    (generation.metadata.generation_id(), generation.file_digest),
                );
            } else if name.starts_with("index-") && name.ends_with(GENERATION_FILE_SUFFIX) {
                let family = family_from_index_file_name(&name)?;
                let prefix = generation_file_prefix(family);
                let generation_id = parse_generation_file_name(&name, &prefix)?;
                let path = entry.path();
                let metadata =
                    fs::symlink_metadata(&path).map_err(|source| IndexRebuildError::Io {
                        operation: "inspect immutable index generation for purge inventory",
                        source,
                    })?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(IndexRebuildError::PathOutsideDatabase);
                }
                validate_file_inside_root(layout, &path)?;
                let bytes = read_bounded_file(&path, DecoderLimits::DEFAULT.max_frame_bytes)?;
                let generation = decode_index_generation(&bytes, &DecoderLimits::DEFAULT)
                    .map_err(IndexRebuildError::Generation)?;
                if generation.metadata().family() != family
                    || generation.metadata().generation_id() != generation_id
                {
                    return Err(IndexRebuildError::InvalidGenerationFileName);
                }
                generations.push(IndexGenerationInventoryEntry {
                    family,
                    generation_id,
                    file_digest: generation.file_digest(),
                    current: false,
                });
            } else {
                return Err(IndexRebuildError::InvalidGenerationFileName);
            }
        }

        for (family_tag, (generation_id, digest)) in current {
            let family = IndexFamily::from_wire_tag(family_tag)
                .ok_or(IndexRebuildError::InvalidCurrentPointer)?;
            let entry = generations
                .iter_mut()
                .find(|entry| entry.family == family && entry.generation_id == generation_id)
                .ok_or(IndexRebuildError::InvalidCurrentPointer)?;
            if entry.file_digest != digest || entry.current {
                return Err(IndexRebuildError::InvalidCurrentPointer);
            }
            entry.current = true;
        }
        generations.sort_by_key(|entry| (entry.family.wire_tag(), entry.generation_id));
        Ok(IndexStorageInventory {
            database_id,
            generations,
        })
    }

    /// Reads the exact checksummed current pointer and verifies its full generation file.
    pub fn read_current(&self) -> Result<Option<StoredIndexGeneration>, IndexRebuildError> {
        let directory = self.layout.indexes_directory();
        match fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(IndexRebuildError::Io {
                    operation: "inspect index directory",
                    source,
                });
            }
            Ok(_) => validate_directory_inside_root(&self.layout, &directory)?,
        }

        let pointer_path = current_pointer_path(&directory, self.family);
        let pointer = match fs::symlink_metadata(&pointer_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(IndexRebuildError::Io {
                    operation: "inspect index-family pointer",
                    source,
                });
            }
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(IndexRebuildError::PathOutsideDatabase);
                }
                validate_file_inside_root(&self.layout, &pointer_path)?;
                let bytes = read_bounded_file(&pointer_path, POINTER_BYTES)?;
                decode_pointer(&bytes, self.family)?
            }
        };

        let generation_path = generation_path(&directory, self.family, pointer.generation);
        validate_file_inside_root(&self.layout, &generation_path)?;
        let bytes = read_bounded_file(&generation_path, DecoderLimits::DEFAULT.max_frame_bytes)?;
        let generation = decode_index_generation(&bytes, &DecoderLimits::DEFAULT)
            .map_err(IndexRebuildError::Generation)?;
        let metadata = generation.metadata();
        if metadata.family() != self.family
            || metadata.generation_id() != pointer.generation
            || generation.file_digest() != pointer.file_digest
        {
            return Err(IndexRebuildError::InvalidCurrentPointer);
        }
        Ok(Some(StoredIndexGeneration {
            metadata,
            payload: generation.payload().to_vec(),
            file_digest: generation.file_digest(),
        }))
    }
}

/// Rebuilds and publishes one derived-index family without exposing partial generations.
#[derive(Clone, Debug)]
pub struct IndexRebuildManager {
    layout: DatabaseLayout,
    family: IndexFamily,
    schema_version: IndexSchemaVersion,
    format_version: IndexFormatVersion,
    build_version: IndexBuildVersion,
    limits: IndexRebuildLimits,
}

impl IndexRebuildManager {
    /// Binds one family, its supported versions, and finite catch-up limits.
    #[must_use]
    pub const fn new(
        layout: DatabaseLayout,
        family: IndexFamily,
        schema_version: IndexSchemaVersion,
        format_version: IndexFormatVersion,
        build_version: IndexBuildVersion,
        limits: IndexRebuildLimits,
    ) -> Self {
        Self {
            layout,
            family,
            schema_version,
            format_version,
            build_version,
            limits,
        }
    }

    /// Builds from a pinned snapshot, catches up committed deltas, then publishes under the
    /// database writer lock. A failed build leaves the previous current pointer untouched.
    pub fn rebuild<S: IndexRebuildSource>(
        &self,
        source: &S,
    ) -> Result<IndexRebuildReceipt, IndexRebuildError> {
        if fs::canonicalize(source.database_root()).ok().as_deref() != Some(self.layout.root()) {
            return Err(IndexRebuildError::SourceDatabaseMismatch);
        }

        let snapshot = source
            .pin_snapshot()
            .map_err(|error| source_error("pin index snapshot", error))?;
        let base_revision = snapshot.revision();
        let mut state = source
            .build_from_snapshot(&snapshot)
            .map_err(|error| source_error("build index from pinned snapshot", error))?;
        let mut covered = base_revision;
        let mut catch_up_rounds = 0_usize;
        let mut applied_deltas = 0_usize;

        loop {
            let head = source
                .current_head()
                .map_err(|error| source_error("read source head", error))?;
            if head < covered {
                return Err(IndexRebuildError::HeadBehindCoverage { head, covered });
            }
            if head == covered {
                break;
            }
            if catch_up_rounds >= self.limits.max_catch_up_rounds {
                return Err(IndexRebuildError::CatchUpRoundLimit {
                    limit: self.limits.max_catch_up_rounds,
                });
            }
            catch_up_rounds += 1;
            applied_deltas += apply_delta_batch(
                source,
                &mut state,
                &mut covered,
                head,
                applied_deltas,
                self.limits.max_deltas,
            )?;
        }
        let mut payload = source
            .encode_index(&state)
            .map_err(|error| source_error("encode rebuilt index", error))?;

        // The expensive snapshot build and online catch-up happen before taking the exclusive
        // writer lock. The short locked tail closes the race between the stable-head check and
        // publication while ordinary commits are stopped at the shared commit point.
        let writer_lock = self
            .layout
            .try_writer_lock()
            .map_err(IndexRebuildError::WriterLock)?;
        if !writer_lock.require_write_access() {
            return Err(IndexRebuildError::RecoveryRequired);
        }
        let locked_head = source
            .current_head()
            .map_err(|error| source_error("read locked source head", error))?;
        if locked_head < covered {
            return Err(IndexRebuildError::HeadBehindCoverage {
                head: locked_head,
                covered,
            });
        }
        if locked_head > covered {
            if catch_up_rounds >= self.limits.max_catch_up_rounds {
                return Err(IndexRebuildError::CatchUpRoundLimit {
                    limit: self.limits.max_catch_up_rounds,
                });
            }
            catch_up_rounds += 1;
            applied_deltas += apply_delta_batch(
                source,
                &mut state,
                &mut covered,
                locked_head,
                applied_deltas,
                self.limits.max_deltas,
            )?;
            payload = source.encode_index(&state).map_err(|error| {
                source_error("encode rebuilt index after locked catch-up", error)
            })?;
        }
        let final_head = source
            .current_head()
            .map_err(|error| source_error("verify locked source head", error))?;
        if final_head != covered {
            return Err(IndexRebuildError::HeadChangedWhileLocked {
                observed: final_head,
                covered,
            });
        }

        let coverage = IndexRevisionCoverage::new(base_revision, covered)
            .map_err(IndexRebuildError::Metadata)?;
        let generation = IndexGenerationStore::new(self.layout.clone(), self.family)
            .publish_complete(
                &writer_lock,
                GenerationDescriptor {
                    schema_version: self.schema_version,
                    format_version: self.format_version,
                    build_version: self.build_version,
                    coverage,
                },
                &payload,
                &NativeIndexPublication,
            )?;
        drop(snapshot);
        Ok(IndexRebuildReceipt {
            metadata: generation.metadata,
            file_digest: generation.file_digest,
            applied_deltas,
            catch_up_rounds,
        })
    }
}

impl IndexGenerationStore {
    fn publish_complete(
        &self,
        writer_lock: &WriterLock,
        descriptor: GenerationDescriptor,
        payload: &[u8],
        publication: &impl IndexPublication,
    ) -> Result<StoredIndexGeneration, IndexRebuildError> {
        if !writer_lock.belongs_to_database_root(self.layout.root()) {
            return Err(IndexRebuildError::ForeignWriterLock);
        }
        if !writer_lock.require_write_access() {
            return Err(IndexRebuildError::RecoveryRequired);
        }
        ensure_index_directory(&self.layout)?;
        validate_directory_inside_root(&self.layout, &self.layout.staging_directory())?;

        let current = self.read_current()?;
        let generation_id =
            self.next_generation(current.as_ref().map(|item| item.metadata.generation_id()))?;
        let metadata = IndexGenerationMetadata::new(
            self.family,
            generation_id,
            descriptor.schema_version,
            descriptor.format_version,
            descriptor.build_version,
            descriptor.coverage,
        )
        .map_err(IndexRebuildError::Metadata)?;
        let generation_bytes = encode_index_generation(metadata, payload, &DecoderLimits::DEFAULT)
            .map_err(IndexRebuildError::Generation)?;
        let file_digest = *blake3::hash(&generation_bytes).as_bytes();
        let indexes_directory = self.layout.indexes_directory();
        let target_path = generation_path(&indexes_directory, self.family, generation_id);
        ensure_generation_target_absent(&target_path)?;

        self.publish_new_file(
            "index-generation",
            &target_path,
            &generation_bytes,
            IndexFileKind::Generation,
            publication,
        )?;
        publication
            .sync_directory(&indexes_directory, IndexDirectory::Indexes)
            .map_err(|source| IndexRebuildError::Io {
                operation: "sync immutable index-generation directory",
                source,
            })?;

        let pointer_bytes = encode_pointer(CurrentPointer {
            family: self.family,
            generation: generation_id,
            file_digest,
        });
        let pointer_path = current_pointer_path(&indexes_directory, self.family);
        match fs::symlink_metadata(&pointer_path) {
            Ok(_) => validate_file_inside_root(&self.layout, &pointer_path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(IndexRebuildError::Io {
                    operation: "inspect index-family pointer before replacement",
                    source,
                });
            }
        }
        self.publish_replacing_file(
            "index-current",
            &pointer_path,
            &pointer_bytes,
            IndexFileKind::CurrentPointer,
            publication,
        )?;
        publication
            .sync_directory(&indexes_directory, IndexDirectory::Indexes)
            .map_err(|source| IndexRebuildError::Io {
                operation: "sync index-family pointer directory",
                source,
            })?;

        let published = self
            .read_current()?
            .ok_or(IndexRebuildError::InvalidCurrentPointer)?;
        if published.metadata != metadata || published.file_digest != file_digest {
            return Err(IndexRebuildError::InvalidCurrentPointer);
        }
        Ok(published)
    }

    fn next_generation(&self, current: Option<u64>) -> Result<u64, IndexRebuildError> {
        let directory = self.layout.indexes_directory();
        validate_directory_inside_root(&self.layout, &directory)?;
        let prefix = generation_file_prefix(self.family);
        let mut maximum = current.unwrap_or(0);
        for entry in fs::read_dir(&directory).map_err(|source| IndexRebuildError::Io {
            operation: "list index-family generations",
            source,
        })? {
            let entry = entry.map_err(|source| IndexRebuildError::Io {
                operation: "read index-generation directory entry",
                source,
            })?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !name.starts_with(&prefix) {
                continue;
            }
            let generation = parse_generation_file_name(&name, &prefix)?;
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|source| IndexRebuildError::Io {
                    operation: "inspect immutable index generation",
                    source,
                })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(IndexRebuildError::PathOutsideDatabase);
            }
            maximum = maximum.max(generation);
        }
        maximum
            .checked_add(1)
            .filter(|generation| *generation != 0)
            .ok_or(IndexRebuildError::GenerationOverflow)
    }

    fn publish_new_file(
        &self,
        label: &'static str,
        target: &Path,
        bytes: &[u8],
        kind: IndexFileKind,
        publication: &impl IndexPublication,
    ) -> Result<(), IndexRebuildError> {
        let (stage_path, mut stage_file) = create_stage_file(&self.layout, label)?;
        let mut guard = StagePathGuard::new(stage_path.clone());
        stage_file
            .write_all(bytes)
            .map_err(|source| IndexRebuildError::Io {
                operation: "write staged index generation",
                source,
            })?;
        publication
            .sync_staged_file(&stage_file, kind)
            .map_err(|source| IndexRebuildError::Io {
                operation: "sync staged index generation",
                source,
            })?;
        drop(stage_file);
        ensure_generation_target_absent(target)?;
        publication
            .publish_generation(&stage_path, target)
            .map_err(|source| IndexRebuildError::Io {
                operation: "publish immutable index generation",
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
        kind: IndexFileKind,
        publication: &impl IndexPublication,
    ) -> Result<(), IndexRebuildError> {
        let (stage_path, mut stage_file) = create_stage_file(&self.layout, label)?;
        let mut guard = StagePathGuard::new(stage_path.clone());
        stage_file
            .write_all(bytes)
            .map_err(|source| IndexRebuildError::Io {
                operation: "write staged index-family pointer",
                source,
            })?;
        publication
            .sync_staged_file(&stage_file, kind)
            .map_err(|source| IndexRebuildError::Io {
                operation: "sync staged index-family pointer",
                source,
            })?;
        drop(stage_file);
        publication
            .replace_pointer(&stage_path, target)
            .map_err(|source| IndexRebuildError::Io {
                operation: "atomically replace index-family pointer",
                source,
            })?;
        guard.mark_published();
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct GenerationDescriptor {
    schema_version: IndexSchemaVersion,
    format_version: IndexFormatVersion,
    build_version: IndexBuildVersion,
    coverage: IndexRevisionCoverage,
}

fn apply_delta_batch<S: IndexRebuildSource>(
    source: &S,
    state: &mut S::State,
    covered: &mut Revision,
    head: Revision,
    already_applied: usize,
    delta_limit: usize,
) -> Result<usize, IndexRebuildError> {
    if head < *covered {
        return Err(IndexRebuildError::HeadBehindCoverage {
            head,
            covered: *covered,
        });
    }
    if head == *covered {
        return Ok(0);
    }
    let deltas = source
        .deltas_after(*covered, head)
        .map_err(|error| source_error("load index catch-up deltas", error))?;
    let remaining = delta_limit.saturating_sub(already_applied);
    if deltas.len() > remaining {
        return Err(IndexRebuildError::DeltaLimitExceeded { limit: delta_limit });
    }
    let mut expected = covered
        .next_commit()
        .map_err(IndexRebuildError::RevisionOverflow)?;
    for delta in &deltas {
        let actual = source.delta_revision(delta);
        if actual > head {
            return Err(IndexRebuildError::DeltaBeyondHead {
                delta: actual,
                head,
            });
        }
        if actual != expected {
            return Err(IndexRebuildError::RevisionGap {
                expected,
                actual: Some(actual),
            });
        }
        source
            .apply_delta(state, delta)
            .map_err(|error| source_error("apply index catch-up delta", error))?;
        *covered = actual;
        if actual < head {
            expected = actual
                .next_commit()
                .map_err(IndexRebuildError::RevisionOverflow)?;
        }
    }
    if *covered != head {
        return Err(IndexRebuildError::IncompleteDeltaRange {
            covered: *covered,
            head,
        });
    }
    Ok(deltas.len())
}

#[derive(Clone, Copy)]
struct CurrentPointer {
    family: IndexFamily,
    generation: u64,
    file_digest: [u8; 32],
}

fn encode_pointer(pointer: CurrentPointer) -> [u8; POINTER_BYTES] {
    let mut bytes = [0_u8; POINTER_BYTES];
    bytes[..8].copy_from_slice(POINTER_MAGIC);
    bytes[8..10].copy_from_slice(&POINTER_VERSION.to_le_bytes());
    bytes[10..12].copy_from_slice(&pointer.family.wire_tag().to_le_bytes());
    bytes[12..16].copy_from_slice(&0_u32.to_le_bytes());
    bytes[16..24].copy_from_slice(&pointer.generation.to_le_bytes());
    bytes[24..56].copy_from_slice(&pointer.file_digest);
    let (pointer_body, pointer_checksum) = bytes.split_at_mut(POINTER_BODY_BYTES);
    let checksum = blake3::hash(pointer_body);
    pointer_checksum.copy_from_slice(checksum.as_bytes());
    bytes
}

fn decode_pointer(
    bytes: &[u8],
    expected_family: IndexFamily,
) -> Result<CurrentPointer, IndexRebuildError> {
    if bytes.len() != POINTER_BYTES
        || bytes.get(..8) != Some(POINTER_MAGIC.as_slice())
        || read_u16(bytes, 8)? != POINTER_VERSION
        || read_u32(bytes, 12)? != 0
    {
        return Err(IndexRebuildError::InvalidCurrentPointer);
    }
    let family_tag = read_u16(bytes, 10)?;
    let family =
        IndexFamily::from_wire_tag(family_tag).ok_or(IndexRebuildError::InvalidCurrentPointer)?;
    let generation = read_u64(bytes, 16)?;
    let digest: [u8; 32] = bytes
        .get(24..56)
        .ok_or(IndexRebuildError::InvalidCurrentPointer)?
        .try_into()
        .map_err(|_| IndexRebuildError::InvalidCurrentPointer)?;
    let pointer_body = bytes
        .get(..POINTER_BODY_BYTES)
        .ok_or(IndexRebuildError::InvalidCurrentPointer)?;
    let checksum = blake3::hash(pointer_body);
    if family != expected_family
        || generation == 0
        || bytes.get(POINTER_BODY_BYTES..) != Some(checksum.as_bytes().as_slice())
    {
        return Err(IndexRebuildError::InvalidCurrentPointer);
    }
    Ok(CurrentPointer {
        family,
        generation,
        file_digest: digest,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, IndexRebuildError> {
    bytes
        .get(offset..offset + 2)
        .and_then(|value| value.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(IndexRebuildError::InvalidCurrentPointer)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, IndexRebuildError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(IndexRebuildError::InvalidCurrentPointer)
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, IndexRebuildError> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(IndexRebuildError::InvalidCurrentPointer)
}

fn current_pointer_path(directory: &Path, family: IndexFamily) -> PathBuf {
    directory.join(format!("index-{:04x}.CURRENT", family.wire_tag()))
}

fn family_from_index_file_name(name: &str) -> Result<IndexFamily, IndexRebuildError> {
    let tag_text = name
        .strip_prefix("index-")
        .and_then(|value| value.get(..4))
        .ok_or(IndexRebuildError::InvalidGenerationFileName)?;
    let tag = u16::from_str_radix(tag_text, 16)
        .map_err(|_| IndexRebuildError::InvalidGenerationFileName)?;
    if format!("{tag:04x}") != tag_text {
        return Err(IndexRebuildError::InvalidGenerationFileName);
    }
    IndexFamily::from_wire_tag(tag).ok_or(IndexRebuildError::InvalidGenerationFileName)
}

fn generation_file_prefix(family: IndexFamily) -> String {
    format!("index-{:04x}-", family.wire_tag())
}

fn generation_path(directory: &Path, family: IndexFamily, generation: u64) -> PathBuf {
    directory.join(format!(
        "{}{:020}{GENERATION_FILE_SUFFIX}",
        generation_file_prefix(family),
        generation
    ))
}

fn parse_generation_file_name(name: &str, prefix: &str) -> Result<u64, IndexRebuildError> {
    let number = name
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(GENERATION_FILE_SUFFIX))
        .ok_or(IndexRebuildError::InvalidGenerationFileName)?;
    if number.len() != 20 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(IndexRebuildError::InvalidGenerationFileName);
    }
    let generation = number
        .parse::<u64>()
        .map_err(|_| IndexRebuildError::InvalidGenerationFileName)?;
    if generation == 0 {
        return Err(IndexRebuildError::InvalidGenerationFileName);
    }
    Ok(generation)
}

fn source_error(operation: &'static str, error: impl fmt::Display) -> IndexRebuildError {
    IndexRebuildError::Source {
        operation,
        message: error.to_string(),
    }
}

fn ensure_index_directory(layout: &DatabaseLayout) -> Result<(), IndexRebuildError> {
    let directory = layout.indexes_directory();
    match fs::symlink_metadata(&directory) {
        Ok(_) => validate_directory_inside_root(layout, &directory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&directory).map_err(|source| IndexRebuildError::Io {
                operation: "create index-generation directory",
                source,
            })?;
            crate::manifest::sync_directory(layout.root()).map_err(|source| {
                IndexRebuildError::Io {
                    operation: "sync database root after creating index directory",
                    source,
                }
            })?;
            validate_directory_inside_root(layout, &directory)
        }
        Err(source) => Err(IndexRebuildError::Io {
            operation: "inspect index-generation directory",
            source,
        }),
    }
}

fn validate_directory_inside_root(
    layout: &DatabaseLayout,
    path: &Path,
) -> Result<(), IndexRebuildError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| IndexRebuildError::Io {
        operation: "inspect index directory",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(IndexRebuildError::PathOutsideDatabase);
    }
    let canonical = fs::canonicalize(path).map_err(|source| IndexRebuildError::Io {
        operation: "resolve index directory",
        source,
    })?;
    if !canonical.starts_with(layout.root()) {
        return Err(IndexRebuildError::PathOutsideDatabase);
    }
    Ok(())
}

fn validate_file_inside_root(
    layout: &DatabaseLayout,
    path: &Path,
) -> Result<(), IndexRebuildError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| IndexRebuildError::Io {
        operation: "inspect index file",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(IndexRebuildError::PathOutsideDatabase);
    }
    let canonical = fs::canonicalize(path).map_err(|source| IndexRebuildError::Io {
        operation: "resolve index file",
        source,
    })?;
    if !canonical.starts_with(layout.root()) {
        return Err(IndexRebuildError::PathOutsideDatabase);
    }
    Ok(())
}

fn read_bounded_file(path: &Path, maximum: usize) -> Result<Vec<u8>, IndexRebuildError> {
    let file = File::open(path).map_err(|source| IndexRebuildError::Io {
        operation: "open index file",
        source,
    })?;
    let file_size = file
        .metadata()
        .map_err(|source| IndexRebuildError::Io {
            operation: "inspect index file size",
            source,
        })?
        .len();
    let limit = u64::try_from(maximum).unwrap_or(u64::MAX);
    if file_size > limit {
        return Err(IndexRebuildError::ResourceLimitExceeded {
            limit: maximum,
            actual: usize::try_from(file_size).unwrap_or(usize::MAX),
        });
    }
    let capacity =
        usize::try_from(file_size).map_err(|_| IndexRebuildError::ResourceLimitExceeded {
            limit: maximum,
            actual: usize::MAX,
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| IndexRebuildError::ResourceLimitExceeded {
            limit: maximum,
            actual: capacity,
        })?;
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| IndexRebuildError::Io {
            operation: "read index file",
            source,
        })?;
    if bytes.len() != capacity {
        return Err(IndexRebuildError::FileChangedDuringRead);
    }
    Ok(bytes)
}

fn ensure_generation_target_absent(path: &Path) -> Result<(), IndexRebuildError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(IndexRebuildError::GenerationAlreadyExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(IndexRebuildError::Io {
            operation: "inspect immutable index-generation destination",
            source,
        }),
    }
}

fn create_stage_file(
    layout: &DatabaseLayout,
    label: &'static str,
) -> Result<(PathBuf, File), IndexRebuildError> {
    let directory = layout.staging_directory();
    validate_directory_inside_root(layout, &directory)?;
    let mut attempt = 0_usize;
    while attempt < MAX_STAGE_ATTEMPTS {
        let sequence = NEXT_STAGE_FILE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("{label}-{}-{sequence}.tmp", std::process::id()));
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(source) => {
                return Err(IndexRebuildError::Io {
                    operation: "create staged index file",
                    source,
                });
            }
        }
    }
    Err(IndexRebuildError::Io {
        operation: "create staged index file",
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "staged index filename attempts were exhausted",
        ),
    })
}

#[derive(Clone, Copy)]
enum IndexFileKind {
    Generation,
    CurrentPointer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IndexDirectory {
    Indexes,
}

trait IndexPublication {
    fn sync_staged_file(&self, file: &File, kind: IndexFileKind) -> io::Result<()>;
    fn publish_generation(&self, stage: &Path, target: &Path) -> io::Result<()>;
    fn replace_pointer(&self, stage: &Path, target: &Path) -> io::Result<()>;
    fn sync_directory(&self, path: &Path, directory: IndexDirectory) -> io::Result<()>;
}

struct NativeIndexPublication;

impl IndexPublication for NativeIndexPublication {
    fn sync_staged_file(&self, file: &File, _kind: IndexFileKind) -> io::Result<()> {
        file.sync_all()
    }

    fn publish_generation(&self, stage: &Path, target: &Path) -> io::Result<()> {
        if target.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "immutable index generation already exists",
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

    fn replace_pointer(&self, stage: &Path, target: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            crate::windows_publication::move_file(stage, target, true)
        }
        #[cfg(not(windows))]
        {
            fs::rename(stage, target)
        }
    }

    fn sync_directory(&self, path: &Path, _directory: IndexDirectory) -> io::Result<()> {
        crate::manifest::sync_directory(path)
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
    use super::{
        IndexDirectory, IndexFileKind, IndexGenerationStore, IndexPublication, IndexRebuildError,
        IndexRebuildLimits, IndexRebuildManager, IndexRebuildSource, NativeIndexPublication,
        PinnedIndexSnapshot, StoredIndexGeneration,
    };
    use crate::{DatabaseLayout, IndexGenerationError};
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;
    use std::io;
    use std::ops::Bound::{Excluded, Included};
    use std::path::{Path, PathBuf};
    #[cfg(windows)]
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use worlddb_core::{
        IndexBuildVersion, IndexFamily, IndexFormatVersion, IndexRevisionCoverage,
        IndexSchemaVersion, Revision,
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDatabase(PathBuf);

    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-index-rebuild-unit-{}-{sequence}",
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

    struct FakeSnapshot {
        revision: Revision,
        payload: Vec<u8>,
        dropped: Arc<AtomicBool>,
    }

    impl PinnedIndexSnapshot for FakeSnapshot {
        fn revision(&self) -> Revision {
            self.revision
        }
    }

    impl Drop for FakeSnapshot {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[derive(Clone)]
    struct FakeDelta {
        revision: Revision,
        payload: Vec<u8>,
    }

    #[derive(Default)]
    struct FakeSourceData {
        head: Option<Revision>,
        deltas: BTreeMap<u64, FakeDelta>,
        apply_hook_delta: Option<FakeDelta>,
    }

    struct FakeSource {
        root: PathBuf,
        snapshot_revision: Revision,
        snapshot_payload: Vec<u8>,
        dropped: Arc<AtomicBool>,
        data: Mutex<FakeSourceData>,
        publish_revision_two_during_build: bool,
        publish_delta_during_apply: bool,
    }

    impl FakeSource {
        fn new(root: &Path, snapshot_revision: Revision, payload: &[u8]) -> Self {
            Self {
                root: root.to_path_buf(),
                snapshot_revision,
                snapshot_payload: payload.to_vec(),
                dropped: Arc::new(AtomicBool::new(false)),
                data: Mutex::new(FakeSourceData {
                    head: Some(snapshot_revision),
                    ..FakeSourceData::default()
                }),
                publish_revision_two_during_build: false,
                publish_delta_during_apply: false,
            }
        }

        fn with_online_changes(mut self) -> Result<Self, String> {
            self.publish_revision_two_during_build = true;
            self.publish_delta_during_apply = true;
            let mut data = self.data.lock().map_err(|error| error.to_string())?;
            data.apply_hook_delta = Some(FakeDelta {
                revision: revision(3)?,
                payload: b"-three".to_vec(),
            });
            drop(data);
            Ok(self)
        }

        fn with_gap(self, head: Revision, delta: FakeDelta) -> Result<Self, String> {
            let mut data = self.data.lock().map_err(|error| error.to_string())?;
            data.head = Some(head);
            data.deltas.insert(delta.revision.value(), delta);
            drop(data);
            Ok(self)
        }
    }

    impl IndexRebuildSource for FakeSource {
        type Snapshot = FakeSnapshot;
        type State = Vec<u8>;
        type Delta = FakeDelta;
        type Error = String;

        fn database_root(&self) -> &Path {
            &self.root
        }

        fn pin_snapshot(&self) -> Result<Self::Snapshot, Self::Error> {
            Ok(FakeSnapshot {
                revision: self.snapshot_revision,
                payload: self.snapshot_payload.clone(),
                dropped: Arc::clone(&self.dropped),
            })
        }

        fn build_from_snapshot(
            &self,
            snapshot: &Self::Snapshot,
        ) -> Result<Self::State, Self::Error> {
            if self.publish_revision_two_during_build {
                let revision_two = revision(2)?;
                let mut data = self.data.lock().map_err(|error| error.to_string())?;
                data.head = Some(revision_two);
                data.deltas.insert(
                    revision_two.value(),
                    FakeDelta {
                        revision: revision_two,
                        payload: b"-two".to_vec(),
                    },
                );
            }
            Ok(snapshot.payload.clone())
        }

        fn current_head(&self) -> Result<Revision, Self::Error> {
            self.data
                .lock()
                .map_err(|error| error.to_string())?
                .head
                .ok_or_else(|| String::from("fake source has no head"))
        }

        fn deltas_after(
            &self,
            after: Revision,
            through: Revision,
        ) -> Result<Vec<Self::Delta>, Self::Error> {
            let data = self.data.lock().map_err(|error| error.to_string())?;
            Ok(data
                .deltas
                .range((Excluded(after.value()), Included(through.value())))
                .map(|(_, delta)| delta.clone())
                .collect())
        }

        fn delta_revision(&self, delta: &Self::Delta) -> Revision {
            delta.revision
        }

        fn apply_delta(
            &self,
            state: &mut Self::State,
            delta: &Self::Delta,
        ) -> Result<(), Self::Error> {
            state.extend_from_slice(&delta.payload);
            if self.publish_delta_during_apply && delta.revision == revision(2)? {
                let mut data = self.data.lock().map_err(|error| error.to_string())?;
                if let Some(hook_delta) = data.apply_hook_delta.take() {
                    data.head = Some(hook_delta.revision);
                    data.deltas.insert(hook_delta.revision.value(), hook_delta);
                }
            }
            Ok(())
        }

        fn encode_index(&self, state: &Self::State) -> Result<Vec<u8>, Self::Error> {
            Ok(state.clone())
        }
    }

    fn revision(value: u64) -> Result<Revision, String> {
        Revision::try_from(value).map_err(|error| error.to_string())
    }

    fn manager(layout: DatabaseLayout) -> IndexRebuildManager {
        IndexRebuildManager::new(
            layout,
            IndexFamily::RecordId,
            IndexSchemaVersion::V1_0,
            IndexFormatVersion::V1_0,
            IndexBuildVersion::V1,
            IndexRebuildLimits::default(),
        )
    }

    fn publish_direct(
        layout: &DatabaseLayout,
        payload: &[u8],
        through: Revision,
        publication: &impl IndexPublication,
    ) -> Result<StoredIndexGeneration, IndexRebuildError> {
        let lock = layout
            .try_writer_lock()
            .map_err(IndexRebuildError::WriterLock)?;
        let coverage = IndexRevisionCoverage::new(Revision::GENESIS, through)
            .map_err(IndexRebuildError::Metadata)?;
        IndexGenerationStore::new(layout.clone(), IndexFamily::RecordId).publish_complete(
            &lock,
            super::GenerationDescriptor {
                schema_version: IndexSchemaVersion::V1_0,
                format_version: IndexFormatVersion::V1_0,
                build_version: IndexBuildVersion::V1,
                coverage,
            },
            payload,
            publication,
        )
    }

    #[test]
    fn online_rebuild_catches_deltas_arriving_during_build_and_apply() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let source = FakeSource::new(layout.root(), revision(1)?, b"base").with_online_changes()?;

        let receipt = manager(layout.clone())
            .rebuild(&source)
            .map_err(|error| error.to_string())?;
        let generation = IndexGenerationStore::new(layout, IndexFamily::RecordId)
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| String::from("published generation is missing"))?;

        assert_eq!(generation.payload(), b"base-two-three");
        assert_eq!(
            generation.metadata().coverage().from_inclusive(),
            revision(1)?
        );
        assert_eq!(
            generation.metadata().coverage().through_inclusive(),
            revision(3)?
        );
        assert_eq!(receipt.applied_deltas(), 2);
        assert_eq!(receipt.catch_up_rounds(), 2);
        assert!(source.dropped.load(Ordering::Acquire));
        Ok(())
    }

    #[test]
    fn a_missing_revision_delta_preserves_the_previous_complete_generation() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let first_source = FakeSource::new(layout.root(), revision(1)?, b"old");
        manager(layout.clone())
            .rebuild(&first_source)
            .map_err(|error| error.to_string())?;
        let before = IndexGenerationStore::new(layout.clone(), IndexFamily::RecordId)
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| String::from("initial generation is missing"))?;

        let second_source = FakeSource::new(layout.root(), revision(1)?, b"new").with_gap(
            revision(3)?,
            FakeDelta {
                revision: revision(3)?,
                payload: b"-three".to_vec(),
            },
        )?;
        assert!(matches!(
            manager(layout.clone()).rebuild(&second_source),
            Err(IndexRebuildError::RevisionGap { .. })
        ));

        let after = IndexGenerationStore::new(layout, IndexFamily::RecordId)
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| String::from("previous generation disappeared"))?;
        assert_eq!(after, before);
        Ok(())
    }

    #[test]
    fn current_pointer_checksum_and_target_digest_are_both_verified() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        publish_direct(&layout, b"whole", revision(1)?, &NativeIndexPublication)
            .map_err(|error| error.to_string())?;
        let pointer_path =
            super::current_pointer_path(&layout.indexes_directory(), IndexFamily::RecordId);
        let mut pointer = fs::read(&pointer_path).map_err(|error| error.to_string())?;
        let byte = pointer
            .get_mut(20)
            .ok_or_else(|| String::from("current pointer is truncated"))?;
        *byte ^= 1;
        fs::write(&pointer_path, &pointer).map_err(|error| error.to_string())?;
        assert!(matches!(
            IndexGenerationStore::new(layout, IndexFamily::RecordId).read_current(),
            Err(IndexRebuildError::InvalidCurrentPointer)
        ));
        Ok(())
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PublicationPoint {
        SyncGenerationFile,
        PublishGeneration,
        SyncGenerationDirectory,
        SyncPointerFile,
        ReplacePointer,
        SyncPointerDirectory,
    }

    #[cfg(windows)]
    impl PublicationPoint {
        const fn name(self) -> &'static str {
            match self {
                Self::SyncGenerationFile => "sync_generation_file",
                Self::PublishGeneration => "publish_generation",
                Self::SyncGenerationDirectory => "sync_generation_directory",
                Self::SyncPointerFile => "sync_pointer_file",
                Self::ReplacePointer => "replace_pointer",
                Self::SyncPointerDirectory => "sync_pointer_directory",
            }
        }

        fn from_name(name: &str) -> Option<Self> {
            Some(match name {
                "sync_generation_file" => Self::SyncGenerationFile,
                "publish_generation" => Self::PublishGeneration,
                "sync_generation_directory" => Self::SyncGenerationDirectory,
                "sync_pointer_file" => Self::SyncPointerFile,
                "replace_pointer" => Self::ReplacePointer,
                "sync_pointer_directory" => Self::SyncPointerDirectory,
                _ => return None,
            })
        }
    }

    struct RecordingPublication {
        crash_after: Option<PublicationPoint>,
        directory_sync_count: AtomicUsize,
    }

    impl RecordingPublication {
        const fn crashing_after(point: PublicationPoint) -> Self {
            Self {
                crash_after: Some(point),
                directory_sync_count: AtomicUsize::new(0),
            }
        }

        fn record(&self, point: PublicationPoint) -> io::Result<()> {
            if self.crash_after == Some(point) {
                std::process::exit(86);
            }
            Ok(())
        }
    }

    impl IndexPublication for RecordingPublication {
        fn sync_staged_file(&self, file: &std::fs::File, kind: IndexFileKind) -> io::Result<()> {
            NativeIndexPublication.sync_staged_file(file, kind)?;
            match kind {
                IndexFileKind::Generation => self.record(PublicationPoint::SyncGenerationFile),
                IndexFileKind::CurrentPointer => self.record(PublicationPoint::SyncPointerFile),
            }
        }

        fn publish_generation(&self, stage: &Path, target: &Path) -> io::Result<()> {
            NativeIndexPublication.publish_generation(stage, target)?;
            self.record(PublicationPoint::PublishGeneration)
        }

        fn replace_pointer(&self, stage: &Path, target: &Path) -> io::Result<()> {
            NativeIndexPublication.replace_pointer(stage, target)?;
            self.record(PublicationPoint::ReplacePointer)
        }

        fn sync_directory(&self, path: &Path, directory: IndexDirectory) -> io::Result<()> {
            NativeIndexPublication.sync_directory(path, directory)?;
            match self.directory_sync_count.fetch_add(1, Ordering::AcqRel) {
                0 => self.record(PublicationPoint::SyncGenerationDirectory),
                _ => self.record(PublicationPoint::SyncPointerDirectory),
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn process_crash_at_each_index_publication_point_leaves_old_or_new_complete_generation()
    -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M6_07_INDEX_CRASH_ROOT";
        const POINT_ENV: &str = "WORLDDB_M6_07_INDEX_CRASH_POINT";
        const TEST_NAME: &str = "index_rebuild::tests::process_crash_at_each_index_publication_point_leaves_old_or_new_complete_generation";

        if let (Ok(root), Ok(point_name)) = (env::var(ROOT_ENV), env::var(POINT_ENV)) {
            let point = PublicationPoint::from_name(&point_name)
                .ok_or_else(|| format!("unknown index crash point {point_name}"))?;
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let revision_two = revision(2)?;
            publish_direct(
                &layout,
                b"new-complete",
                revision_two,
                &RecordingPublication::crashing_after(point),
            )
            .map_err(|error| error.to_string())?;
            return Err(format!("child did not crash after {point_name}"));
        }

        for point in [
            PublicationPoint::SyncGenerationFile,
            PublicationPoint::PublishGeneration,
            PublicationPoint::SyncGenerationDirectory,
            PublicationPoint::SyncPointerFile,
            PublicationPoint::ReplacePointer,
            PublicationPoint::SyncPointerDirectory,
        ] {
            let database = TempDatabase::create()?;
            let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            publish_direct(
                &layout,
                b"old-complete",
                revision(1)?,
                &NativeIndexPublication,
            )
            .map_err(|error| error.to_string())?;

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

            let reopened = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            let current = IndexGenerationStore::new(reopened, IndexFamily::RecordId)
                .read_current()
                .map_err(|error| format!("restart after {point:?}: {error}"))?
                .ok_or_else(|| format!("index pointer is missing after {point:?}"))?;
            let pointer_switched = matches!(
                point,
                PublicationPoint::ReplacePointer | PublicationPoint::SyncPointerDirectory
            );
            let (expected_payload, expected_revision) = if pointer_switched {
                (b"new-complete".as_slice(), revision(2)?)
            } else {
                (b"old-complete".as_slice(), revision(1)?)
            };
            assert_eq!(current.payload(), expected_payload, "after {point:?}");
            assert_eq!(
                current.metadata().coverage().through_inclusive(),
                expected_revision,
                "after {point:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn purge_inventory_includes_stale_and_current_generations_and_rejects_unknown_files()
    -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let first = publish_direct(&layout, b"old", revision(1)?, &NativeIndexPublication)
            .map_err(|error| error.to_string())?;
        let second = publish_direct(&layout, b"new", revision(2)?, &NativeIndexPublication)
            .map_err(|error| error.to_string())?;

        let inventory =
            IndexGenerationStore::inventory_all(&layout).map_err(|error| error.to_string())?;
        assert_eq!(
            inventory.database_id(),
            layout.database_id().ok_or("database ID missing")?
        );
        assert_eq!(inventory.generations().len(), 2);
        assert_eq!(
            inventory
                .generations()
                .iter()
                .filter(|entry| entry.current())
                .count(),
            1
        );
        assert!(inventory.generations().iter().any(|entry| {
            entry.generation_id() == first.metadata().generation_id() && !entry.current()
        }));
        assert!(inventory.generations().iter().any(|entry| {
            entry.generation_id() == second.metadata().generation_id() && entry.current()
        }));

        fs::write(
            layout.indexes_directory().join("unclassified-index-file"),
            b"residue",
        )
        .map_err(|error| error.to_string())?;
        assert!(matches!(
            IndexGenerationStore::inventory_all(&layout),
            Err(IndexRebuildError::InvalidGenerationFileName)
        ));
        fs::remove_file(layout.indexes_directory().join("unclassified-index-file"))
            .map_err(|error| error.to_string())?;
        assert_eq!(
            IndexGenerationStore::inventory_all(&layout)
                .map_err(|error| error.to_string())?
                .generations()
                .len(),
            2
        );
        Ok(())
    }

    #[test]
    fn purge_inventory_treats_an_uncreated_optional_index_directory_as_empty() -> Result<(), String>
    {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        assert!(!layout.indexes_directory().exists());
        let inventory =
            IndexGenerationStore::inventory_all(&layout).map_err(|error| error.to_string())?;
        assert!(inventory.generations().is_empty());
        assert_eq!(
            inventory.database_id(),
            layout.database_id().ok_or("database ID missing")?
        );
        Ok(())
    }

    #[test]
    fn corrupt_generation_bytes_are_never_returned_as_current_payload() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        publish_direct(&layout, b"whole", revision(1)?, &NativeIndexPublication)
            .map_err(|error| error.to_string())?;
        let generation_path =
            super::generation_path(&layout.indexes_directory(), IndexFamily::RecordId, 1);
        let mut generation = fs::read(&generation_path).map_err(|error| error.to_string())?;
        let byte = generation
            .get_mut(28)
            .ok_or_else(|| String::from("generation frame is truncated"))?;
        *byte ^= 1;
        fs::write(&generation_path, &generation).map_err(|error| error.to_string())?;
        assert!(matches!(
            IndexGenerationStore::new(layout, IndexFamily::RecordId).read_current(),
            Err(IndexRebuildError::Generation(IndexGenerationError::Frame(
                _
            )))
        ));
        Ok(())
    }
}
