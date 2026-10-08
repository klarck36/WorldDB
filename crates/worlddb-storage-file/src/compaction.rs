//! Immutable history compaction and pin-aware reclamation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use worlddb_core::storage_internal::generate_storage_maintenance_operation_id;
use worlddb_core::{
    DecodedRecord, DecoderLimits, DomainId, IdGenerationError, Revision, encode_decoded_record,
};

use crate::manifest::compare_segment_references;
use crate::segment::MAX_CANONICAL_BYTES;
use crate::{
    ContentDigest, DatabaseLayout, HistorySegmentStore, Manifest, ManifestError,
    ManifestSegmentKind, ManifestSegmentReference, ManifestSnapshot, ManifestStore, RecoveryError,
    RecoveryManager, SegmentError, SegmentId, SnapshotCommitError, WalPrepareLog, WriterLock,
};

type PinKey = (ManifestSegmentKind, SegmentId);
type PinMap = BTreeMap<PinKey, PinCounts>;
type SharedPinMap = Arc<Mutex<PinMap>>;

const BACKUP_PIN_MAGIC: &[u8; 8] = b"WDBPIN\0\x01";
const BACKUP_PIN_CONTEXT: &[u8] = b"worlddb.backup.pin.v1\0";
const BACKUP_PIN_SUFFIX: &str = ".wdbpin";
const BACKUP_PIN_ITEM_BYTES: usize = 1 + 16 + 32 + 8;
const BACKUP_PIN_MAX_ITEMS: usize = 262_144;
const BACKUP_PIN_MAX_BYTES: usize = 16 * 1024 * 1024;

static PIN_REGISTRIES: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<PinMap>>>>> = OnceLock::new();
static NEXT_BACKUP_PIN: AtomicU64 = AtomicU64::new(0);

/// A reader or durable operation that keeps one immutable segment alive.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SegmentPinKind {
    /// A live query or snapshot lease.
    Snapshot,
    /// A backup that still needs the source segment bytes.
    Backup,
    /// A recovery checkpoint that may replay from the segment.
    RecoveryCheckpoint,
    /// An export that has captured a specific history view.
    Export,
}

#[derive(Clone, Copy, Debug, Default)]
struct PinCounts {
    snapshot: usize,
    backup: usize,
    recovery_checkpoint: usize,
    export: usize,
}

impl PinCounts {
    fn count_mut(&mut self, kind: SegmentPinKind) -> &mut usize {
        match kind {
            SegmentPinKind::Snapshot => &mut self.snapshot,
            SegmentPinKind::Backup => &mut self.backup,
            SegmentPinKind::RecoveryCheckpoint => &mut self.recovery_checkpoint,
            SegmentPinKind::Export => &mut self.export,
        }
    }

    fn is_empty(self) -> bool {
        self.snapshot == 0 && self.backup == 0 && self.recovery_checkpoint == 0 && self.export == 0
    }
}

/// RAII pin that prevents reclamation until its owner releases it.
///
/// Pins are process-local contracts in M5-15. Backup and export code connect
/// their real lifecycle to this registry in M7-08 and M7-11.
pub struct SegmentPin {
    pins: SharedPinMap,
    key: PinKey,
    kind: SegmentPinKind,
    reference: ManifestSegmentReference,
}

/// Cross-process lease for a complete exact-backup segment inventory.
pub(crate) struct DurableBackupPin {
    directory: PathBuf,
    path: Option<PathBuf>,
    file: Option<File>,
}

impl Drop for DurableBackupPin {
    fn drop(&mut self) {
        self.file.take();
        if let Some(path) = self.path.take() {
            if fs::remove_file(path).is_ok() {
                let _ = crate::manifest::sync_directory(&self.directory);
            }
        }
    }
}

impl SegmentPin {
    /// Segment protected by this lease.
    #[must_use]
    pub const fn reference(&self) -> ManifestSegmentReference {
        self.reference
    }

    /// Pin category represented by this lease.
    #[must_use]
    pub const fn kind(&self) -> SegmentPinKind {
        self.kind
    }
}

impl Drop for SegmentPin {
    fn drop(&mut self) {
        let mut pins = match self.pins.lock() {
            Ok(pins) => pins,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(counts) = pins.get_mut(&self.key) {
            let count = counts.count_mut(self.kind);
            *count = count.saturating_sub(1);
            if counts.is_empty() {
                pins.remove(&self.key);
            }
        }
    }
}

/// Why a compaction, pin, or reclamation request could not complete safely.
#[derive(Debug)]
pub enum CompactionError {
    /// The writer lock belongs to a different database.
    ForeignWriterLock,
    /// Recovery has not authorized storage mutation.
    RecoveryRequired,
    /// The referenced segment is not in the current manifest.
    SegmentNotCurrent,
    /// Reclamation accepts only immutable History segments.
    UnsupportedReclamationKind,
    /// Two candidate references claim one segment identity with different metadata.
    ConflictingReclamationReference,
    /// The current manifest did not match the inventory just committed to the WAL.
    ManifestSnapshotMismatch,
    /// A pin counter reached its bounded integer maximum.
    PinCountOverflow,
    /// An active durable backup pin is malformed, so reclamation must fail closed.
    BackupPinInvalid,
    /// A durable backup-pin file operation failed.
    BackupPinIo {
        operation: &'static str,
        source: io::Error,
    },
    /// The pin registry mutex was poisoned before a pin could be created.
    PinRegistryPoisoned,
    /// The core UUIDv7 policy could not generate a maintenance OperationId.
    Identity(IdGenerationError),
    /// An immutable history segment failed encoding, decoding, or file I/O.
    Segment(SegmentError),
    /// A manifest could not be read or validated.
    Manifest(ManifestError),
    /// The typed snapshot could not be committed to the WAL.
    SnapshotCommit(SnapshotCommitError),
    /// Recovery could not materialize the committed snapshot.
    Recovery(RecoveryError),
}

impl fmt::Display for CompactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWriterLock => {
                formatter.write_str("writer lock belongs to another database")
            }
            Self::RecoveryRequired => {
                formatter.write_str("storage writes are blocked until recovery completes")
            }
            Self::SegmentNotCurrent => {
                formatter.write_str("segment is not present in the current manifest")
            }
            Self::UnsupportedReclamationKind => {
                formatter.write_str("only History segments may be reclaimed by compaction")
            }
            Self::ConflictingReclamationReference => {
                formatter.write_str("reclamation candidates disagree about one segment identity")
            }
            Self::ManifestSnapshotMismatch => formatter
                .write_str("published manifest differs from the committed compaction inventory"),
            Self::PinCountOverflow => formatter.write_str("segment pin count overflowed"),
            Self::BackupPinInvalid => {
                formatter.write_str("durable backup pin is malformed; reclamation is blocked")
            }
            Self::BackupPinIo { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::PinRegistryPoisoned => {
                formatter.write_str("segment pin registry is unavailable after a panic")
            }
            Self::Identity(error) => write!(
                formatter,
                "maintenance OperationId generation failed: {error}"
            ),
            Self::Segment(error) => write!(formatter, "history segment operation failed: {error}"),
            Self::Manifest(error) => write!(formatter, "manifest operation failed: {error}"),
            Self::SnapshotCommit(error) => {
                write!(formatter, "compaction WAL commit failed: {error}")
            }
            Self::Recovery(error) => write!(formatter, "compaction recovery failed: {error}"),
        }
    }
}

impl std::error::Error for CompactionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::Segment(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::SnapshotCommit(error) => Some(error),
            Self::Recovery(error) => Some(error),
            Self::BackupPinIo { source, .. } => Some(source),
            Self::ForeignWriterLock
            | Self::RecoveryRequired
            | Self::SegmentNotCurrent
            | Self::UnsupportedReclamationKind
            | Self::ConflictingReclamationReference
            | Self::ManifestSnapshotMismatch
            | Self::PinCountOverflow
            | Self::BackupPinInvalid
            | Self::PinRegistryPoisoned => None,
        }
    }
}

/// Result of one safe reclamation attempt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReclamationOutcome {
    reclaimed: Vec<ManifestSegmentReference>,
    retained_by_pin: Vec<ManifestSegmentReference>,
    still_referenced: Vec<ManifestSegmentReference>,
}

impl ReclamationOutcome {
    /// Candidate files removed, or already absent, after verification.
    #[must_use]
    pub fn reclaimed(&self) -> &[ManifestSegmentReference] {
        &self.reclaimed
    }

    /// Segments still protected by at least one live pin.
    #[must_use]
    pub fn retained_by_pin(&self) -> &[ManifestSegmentReference] {
        &self.retained_by_pin
    }

    /// Candidates that remain part of the current manifest inventory.
    #[must_use]
    pub fn still_referenced(&self) -> &[ManifestSegmentReference] {
        &self.still_referenced
    }
}

/// Result of one history compaction transaction.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompactionOutcome {
    compacted: bool,
    input_segments: Vec<ManifestSegmentReference>,
    output_segments: Vec<ManifestSegmentReference>,
    maintenance_revision: Option<Revision>,
    reclamation: ReclamationOutcome,
}

impl CompactionOutcome {
    /// Whether a smaller immutable History inventory was committed.
    #[must_use]
    pub const fn compacted(&self) -> bool {
        self.compacted
    }

    /// Previous manifest History references replaced by this compaction.
    #[must_use]
    pub fn input_segments(&self) -> &[ManifestSegmentReference] {
        &self.input_segments
    }

    /// New staged-and-published History references.
    #[must_use]
    pub fn output_segments(&self) -> &[ManifestSegmentReference] {
        &self.output_segments
    }

    /// New WAL/manifest revision for a successful maintenance commit.
    #[must_use]
    pub const fn maintenance_revision(&self) -> Option<Revision> {
        self.maintenance_revision
    }

    /// Physical reclamation result for replaced source references.
    #[must_use]
    pub const fn reclamation(&self) -> &ReclamationOutcome {
        &self.reclamation
    }
}

/// Rewrites the current History inventory into fewer immutable segments and
/// reclaims replaced files only after WAL replay and pin checks complete.
#[derive(Clone, Debug)]
pub struct CompactionManager {
    layout: DatabaseLayout,
    pins: SharedPinMap,
}

impl CompactionManager {
    /// Binds compaction to a validated database layout.
    #[must_use]
    pub fn new(layout: DatabaseLayout) -> Self {
        let pins = shared_pin_map(layout.root());
        Self { layout, pins }
    }

    /// Pins a segment that still belongs to the current manifest.
    ///
    /// The manifest check and pin registration share the same mutex used by
    /// reclamation, so a reader cannot register an old reference after its file
    /// has been removed.
    pub fn pin_current_segment(
        &self,
        reference: ManifestSegmentReference,
        kind: SegmentPinKind,
    ) -> Result<SegmentPin, CompactionError> {
        let mut pins = self.lock_pins()?;
        let manifest = ManifestStore::new(self.layout.clone())
            .read_current()
            .map_err(CompactionError::Manifest)?
            .ok_or(CompactionError::SegmentNotCurrent)?;
        if !manifest.segments().contains(&reference) {
            return Err(CompactionError::SegmentNotCurrent);
        }
        let key = (reference.kind(), reference.id());
        let count = pins.entry(key).or_default().count_mut(kind);
        *count = count
            .checked_add(1)
            .ok_or(CompactionError::PinCountOverflow)?;
        Ok(SegmentPin {
            pins: Arc::clone(&self.pins),
            key,
            kind,
            reference,
        })
    }

    /// Pins every segment in one current exact-backup snapshot, including a
    /// durable cross-process lease consumed by reclamation.
    pub(crate) fn pin_backup_snapshot(
        &self,
        writer_lock: &WriterLock,
        references: &[ManifestSegmentReference],
    ) -> Result<(Vec<SegmentPin>, DurableBackupPin), CompactionError> {
        self.require_write_lock(writer_lock)?;
        let current = ManifestStore::new(self.layout.clone())
            .read_current()
            .map_err(CompactionError::Manifest)?;
        if current.as_ref().map_or(&[][..], Manifest::segments) != references {
            return Err(CompactionError::SegmentNotCurrent);
        }
        let mut pins = Vec::new();
        pins.try_reserve_exact(references.len())
            .map_err(|_| CompactionError::PinCountOverflow)?;
        for reference in references {
            pins.push(self.pin_current_segment(*reference, SegmentPinKind::Backup)?);
        }
        let durable = create_durable_backup_pin(&self.layout, references)?;
        Ok((pins, durable))
    }

    /// Pins every segment in one current logical-export snapshot, including a
    /// durable cross-process lease consumed by reclamation.
    pub(crate) fn pin_export_snapshot(
        &self,
        writer_lock: &WriterLock,
        references: &[ManifestSegmentReference],
    ) -> Result<(Vec<SegmentPin>, DurableBackupPin), CompactionError> {
        self.require_write_lock(writer_lock)?;
        let current = ManifestStore::new(self.layout.clone())
            .read_current()
            .map_err(CompactionError::Manifest)?;
        if current.as_ref().map_or(&[][..], Manifest::segments) != references {
            return Err(CompactionError::SegmentNotCurrent);
        }
        let mut pins = Vec::new();
        pins.try_reserve_exact(references.len())
            .map_err(|_| CompactionError::PinCountOverflow)?;
        for reference in references {
            pins.push(self.pin_current_segment(*reference, SegmentPinKind::Export)?);
        }
        // Backup and logical export leases share the same durable inventory
        // format and reclamation scan; the lease's consumer does not affect
        // which immutable History segments it protects.
        let durable = create_durable_backup_pin(&self.layout, references)?;
        Ok((pins, durable))
    }

    /// Rewrites the current History segments when the result uses fewer files.
    ///
    /// This is a storage maintenance commit: it advances the durable revision
    /// axis but does not add, remove, or alter logical History records. Security
    /// policy references are carried into the new manifest unchanged. If a
    /// segment cannot be combined within the registered per-segment limits,
    /// it remains an output chunk with its exact high-water revision.
    pub fn compact_history(
        &self,
        writer_lock: &WriterLock,
    ) -> Result<CompactionOutcome, CompactionError> {
        self.require_write_lock(writer_lock)?;
        let current = ManifestStore::new(self.layout.clone())
            .read_current()
            .map_err(CompactionError::Manifest)?;
        let Some(current) = current else {
            return Ok(CompactionOutcome::default());
        };
        let input_segments = current
            .segments()
            .iter()
            .copied()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
            .collect::<Vec<_>>();
        if input_segments.len() < 2 {
            return Ok(CompactionOutcome {
                input_segments,
                ..CompactionOutcome::default()
            });
        }

        let store = HistorySegmentStore::new(self.layout.clone());
        let mut staged = StagedSegments::new(store.clone());
        let output_segments =
            stage_compacted_history(writer_lock, &store, &input_segments, &mut staged)?;
        if output_segments.len() >= input_segments.len() {
            return Ok(CompactionOutcome {
                input_segments,
                ..CompactionOutcome::default()
            });
        }

        let mut next_inventory = current
            .segments()
            .iter()
            .copied()
            .filter(|reference| reference.kind() != ManifestSegmentKind::History)
            .collect::<Vec<_>>();
        next_inventory.extend(output_segments.iter().copied());
        let operation_id =
            generate_storage_maintenance_operation_id().map_err(CompactionError::Identity)?;
        // A failed commit may have an unknown outcome. Preserve staged bytes
        // on every commit error so recovery can safely decide from the WAL.
        staged.mark_committed();
        let receipt = WalPrepareLog::new(&self.layout)
            .commit_manifest_snapshot(
                writer_lock,
                operation_id,
                next_inventory.clone(),
                &output_segments,
            )
            .map_err(CompactionError::SnapshotCommit)?;

        RecoveryManager::new(self.layout.clone())
            .recover(writer_lock)
            .map_err(CompactionError::Recovery)?;
        let expected = ManifestSnapshot::new(receipt.revision(), next_inventory)
            .map_err(CompactionError::Manifest)?;
        let published = ManifestStore::new(self.layout.clone())
            .read_current()
            .map_err(CompactionError::Manifest)?
            .ok_or(CompactionError::ManifestSnapshotMismatch)?;
        if published.revision() != receipt.revision() || published.segments() != expected.segments()
        {
            return Err(CompactionError::ManifestSnapshotMismatch);
        }

        let reclamation = self.reclaim_retired(writer_lock, &input_segments)?;
        Ok(CompactionOutcome {
            compacted: true,
            input_segments,
            output_segments,
            maintenance_revision: Some(receipt.revision()),
            reclamation,
        })
    }

    /// Retries reclamation after pins have been released.
    ///
    /// Candidates still in the current manifest are preserved, and only
    /// explicitly supplied History references are considered. Orphaned or
    /// unknown files are never discovered and swept by this method.
    pub fn reclaim_retired(
        &self,
        writer_lock: &WriterLock,
        candidates: &[ManifestSegmentReference],
    ) -> Result<ReclamationOutcome, CompactionError> {
        self.require_write_lock(writer_lock)?;
        let pins = self.lock_pins()?;
        let durable_pins = active_backup_pin_keys(&self.layout)?;
        let current = ManifestStore::new(self.layout.clone())
            .read_current()
            .map_err(CompactionError::Manifest)?;
        let current_keys = current
            .as_ref()
            .map(|manifest| {
                manifest
                    .segments()
                    .iter()
                    .map(|reference| (reference.kind(), reference.id()))
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();

        let mut unique = BTreeMap::new();
        for reference in candidates {
            if reference.kind() != ManifestSegmentKind::History {
                return Err(CompactionError::UnsupportedReclamationKind);
            }
            let key = (reference.kind(), reference.id());
            if let Some(previous) = unique.insert(key, *reference) {
                if previous != *reference {
                    return Err(CompactionError::ConflictingReclamationReference);
                }
            }
        }

        let mut outcome = ReclamationOutcome::default();
        let mut eligible = Vec::new();
        for (key, reference) in unique {
            if current_keys.contains(&key) {
                outcome.still_referenced.push(reference);
            } else if pins.get(&key).is_some_and(|counts| !counts.is_empty())
                || durable_pins.contains(&key)
            {
                outcome.retained_by_pin.push(reference);
            } else {
                eligible.push(reference);
            }
        }
        outcome.reclaimed = HistorySegmentStore::new(self.layout.clone())
            .reclaim_unreferenced_references(&eligible)
            .map_err(CompactionError::Segment)?;
        drop(pins);
        Ok(outcome)
    }

    fn require_write_lock(&self, writer_lock: &WriterLock) -> Result<(), CompactionError> {
        if !writer_lock.belongs_to_database_root(self.layout.root()) {
            return Err(CompactionError::ForeignWriterLock);
        }
        if !writer_lock.require_write_access() {
            return Err(CompactionError::RecoveryRequired);
        }
        Ok(())
    }

    fn lock_pins(&self) -> Result<MutexGuard<'_, PinMap>, CompactionError> {
        self.pins
            .lock()
            .map_err(|_| CompactionError::PinRegistryPoisoned)
    }
}

fn create_durable_backup_pin(
    layout: &DatabaseLayout,
    references: &[ManifestSegmentReference],
) -> Result<DurableBackupPin, CompactionError> {
    let directory = layout.staging_directory().join("backup-pins");
    fs::create_dir_all(&directory).map_err(|source| CompactionError::BackupPinIo {
        operation: "create durable backup-pin directory",
        source,
    })?;
    validate_backup_pin_directory(layout, &directory)?;
    if references.is_empty() {
        return Ok(DurableBackupPin {
            directory,
            path: None,
            file: None,
        });
    }
    let bytes = encode_backup_pin_manifest(references)?;
    let mut attempt = 0_u8;
    while attempt < 16 {
        let sequence = NEXT_BACKUP_PIN.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            "pin-{}-{sequence:020}{BACKUP_PIN_SUFFIX}",
            std::process::id()
        ));
        let mut file = match OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                attempt += 1;
                continue;
            }
            Err(source) => {
                return Err(CompactionError::BackupPinIo {
                    operation: "create durable backup-pin file",
                    source,
                });
            }
        };
        file.write_all(&bytes)
            .map_err(|source| CompactionError::BackupPinIo {
                operation: "write durable backup-pin file",
                source,
            })?;
        crate::platform_sync::sync_file(&file).map_err(|source| CompactionError::BackupPinIo {
            operation: "sync durable backup-pin file",
            source,
        })?;
        crate::manifest::sync_directory(&directory).map_err(|source| {
            CompactionError::BackupPinIo {
                operation: "sync durable backup-pin directory",
                source,
            }
        })?;
        match fs4::FileExt::try_lock_shared(&file) {
            Ok(()) => {
                return Ok(DurableBackupPin {
                    directory,
                    path: Some(path),
                    file: Some(file),
                });
            }
            Err(fs4::TryLockError::WouldBlock) => {
                return Err(CompactionError::BackupPinIo {
                    operation: "lock durable backup-pin file",
                    source: io::Error::new(io::ErrorKind::WouldBlock, "backup pin is locked"),
                });
            }
            Err(fs4::TryLockError::Error(source)) => {
                return Err(CompactionError::BackupPinIo {
                    operation: "lock durable backup-pin file",
                    source,
                });
            }
        }
    }
    Err(CompactionError::BackupPinIo {
        operation: "allocate unique durable backup-pin name",
        source: io::Error::new(io::ErrorKind::AlreadyExists, "backup pin names exhausted"),
    })
}

fn active_backup_pin_keys(layout: &DatabaseLayout) -> Result<BTreeSet<PinKey>, CompactionError> {
    let directory = layout.staging_directory().join("backup-pins");
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => {
            return Err(CompactionError::BackupPinIo {
                operation: "inspect durable backup-pin directory",
                source,
            });
        }
        Ok(_) => {}
    }
    validate_backup_pin_directory(layout, &directory)?;
    let mut pinned = BTreeSet::new();
    for entry in fs::read_dir(&directory).map_err(|source| CompactionError::BackupPinIo {
        operation: "list durable backup pins",
        source,
    })? {
        let entry = entry.map_err(|source| CompactionError::BackupPinIo {
            operation: "read durable backup-pin entry",
            source,
        })?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| CompactionError::BackupPinIo {
                operation: "inspect durable backup-pin file",
                source,
            })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || path.extension().and_then(|value| value.to_str()) != Some("wdbpin")
        {
            return Err(CompactionError::BackupPinInvalid);
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| CompactionError::BackupPinIo {
                operation: "open durable backup-pin file",
                source,
            })?;
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => {
                drop(file);
                match fs::remove_file(&path) {
                    Ok(()) => {
                        crate::manifest::sync_directory(&directory).map_err(|source| {
                            CompactionError::BackupPinIo {
                                operation: "sync stale backup-pin cleanup",
                                source,
                            }
                        })?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(CompactionError::BackupPinIo {
                            operation: "remove stale backup pin",
                            source,
                        });
                    }
                }
            }
            Err(fs4::TryLockError::WouldBlock) => {
                let length = usize::try_from(metadata.len())
                    .map_err(|_| CompactionError::BackupPinInvalid)?;
                if length > BACKUP_PIN_MAX_BYTES {
                    return Err(CompactionError::BackupPinInvalid);
                }
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(length)
                    .map_err(|_| CompactionError::BackupPinInvalid)?;
                file.take(
                    u64::try_from(BACKUP_PIN_MAX_BYTES)
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                )
                .read_to_end(&mut bytes)
                .map_err(|source| CompactionError::BackupPinIo {
                    operation: "read active durable backup pin",
                    source,
                })?;
                if bytes.len() != length {
                    return Err(CompactionError::BackupPinInvalid);
                }
                pinned.extend(decode_backup_pin_manifest(&bytes)?);
            }
            Err(fs4::TryLockError::Error(source)) => {
                return Err(CompactionError::BackupPinIo {
                    operation: "check durable backup-pin lease",
                    source,
                });
            }
        }
    }
    Ok(pinned)
}

fn validate_backup_pin_directory(
    layout: &DatabaseLayout,
    directory: &Path,
) -> Result<(), CompactionError> {
    let metadata =
        fs::symlink_metadata(directory).map_err(|source| CompactionError::BackupPinIo {
            operation: "inspect durable backup-pin directory",
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CompactionError::BackupPinInvalid);
    }
    let canonical = fs::canonicalize(directory).map_err(|source| CompactionError::BackupPinIo {
        operation: "resolve durable backup-pin directory",
        source,
    })?;
    if !canonical.starts_with(layout.root()) {
        return Err(CompactionError::BackupPinInvalid);
    }
    Ok(())
}

fn encode_backup_pin_manifest(
    references: &[ManifestSegmentReference],
) -> Result<Vec<u8>, CompactionError> {
    if references.len() > BACKUP_PIN_MAX_ITEMS {
        return Err(CompactionError::BackupPinInvalid);
    }
    let capacity = 8_usize
        .checked_add(4)
        .and_then(|value| value.checked_add(references.len().checked_mul(BACKUP_PIN_ITEM_BYTES)?))
        .and_then(|value| value.checked_add(32))
        .ok_or(CompactionError::BackupPinInvalid)?;
    if capacity > BACKUP_PIN_MAX_BYTES {
        return Err(CompactionError::BackupPinInvalid);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| CompactionError::BackupPinInvalid)?;
    bytes.extend_from_slice(BACKUP_PIN_MAGIC);
    bytes.extend_from_slice(
        &u32::try_from(references.len())
            .map_err(|_| CompactionError::BackupPinInvalid)?
            .to_be_bytes(),
    );
    for reference in references {
        let tag = match reference.kind() {
            ManifestSegmentKind::History => 1,
            ManifestSegmentKind::SecurityPolicy => 2,
        };
        bytes.push(tag);
        bytes.extend_from_slice(&reference.id().to_bytes());
        bytes.extend_from_slice(reference.content_digest().as_bytes());
        bytes.extend_from_slice(&reference.through_revision().value().to_be_bytes());
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(BACKUP_PIN_CONTEXT);
    hasher.update(&bytes);
    bytes.extend_from_slice(hasher.finalize().as_bytes());
    Ok(bytes)
}

fn decode_backup_pin_manifest(bytes: &[u8]) -> Result<BTreeSet<PinKey>, CompactionError> {
    if bytes.len() < 8 + 4 + 32 || bytes.len() > BACKUP_PIN_MAX_BYTES {
        return Err(CompactionError::BackupPinInvalid);
    }
    let payload_end = bytes
        .len()
        .checked_sub(32)
        .ok_or(CompactionError::BackupPinInvalid)?;
    let payload = bytes
        .get(..payload_end)
        .ok_or(CompactionError::BackupPinInvalid)?;
    let claimed_digest: [u8; 32] = bytes
        .get(payload_end..)
        .ok_or(CompactionError::BackupPinInvalid)?
        .try_into()
        .map_err(|_| CompactionError::BackupPinInvalid)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(BACKUP_PIN_CONTEXT);
    hasher.update(payload);
    if *hasher.finalize().as_bytes() != claimed_digest
        || payload.get(..8) != Some(BACKUP_PIN_MAGIC.as_slice())
    {
        return Err(CompactionError::BackupPinInvalid);
    }
    let count_bytes: [u8; 4] = payload
        .get(8..12)
        .ok_or(CompactionError::BackupPinInvalid)?
        .try_into()
        .map_err(|_| CompactionError::BackupPinInvalid)?;
    let count = usize::try_from(u32::from_be_bytes(count_bytes))
        .map_err(|_| CompactionError::BackupPinInvalid)?;
    if count > BACKUP_PIN_MAX_ITEMS
        || payload.len()
            != 12_usize
                .checked_add(
                    count
                        .checked_mul(BACKUP_PIN_ITEM_BYTES)
                        .ok_or(CompactionError::BackupPinInvalid)?,
                )
                .ok_or(CompactionError::BackupPinInvalid)?
    {
        return Err(CompactionError::BackupPinInvalid);
    }
    let mut pinned = BTreeSet::new();
    let mut offset = 12_usize;
    for _ in 0..count {
        let end = offset
            .checked_add(BACKUP_PIN_ITEM_BYTES)
            .ok_or(CompactionError::BackupPinInvalid)?;
        let entry = payload
            .get(offset..end)
            .ok_or(CompactionError::BackupPinInvalid)?;
        let kind = match entry.first().copied() {
            Some(1) => ManifestSegmentKind::History,
            Some(2) => ManifestSegmentKind::SecurityPolicy,
            _ => return Err(CompactionError::BackupPinInvalid),
        };
        let id_bytes: [u8; 16] = entry
            .get(1..17)
            .ok_or(CompactionError::BackupPinInvalid)?
            .try_into()
            .map_err(|_| CompactionError::BackupPinInvalid)?;
        let id =
            SegmentId::try_from_bytes(id_bytes).map_err(|_| CompactionError::BackupPinInvalid)?;
        let digest_bytes: [u8; 32] = entry
            .get(17..49)
            .ok_or(CompactionError::BackupPinInvalid)?
            .try_into()
            .map_err(|_| CompactionError::BackupPinInvalid)?;
        let revision_bytes: [u8; 8] = entry
            .get(49..57)
            .ok_or(CompactionError::BackupPinInvalid)?
            .try_into()
            .map_err(|_| CompactionError::BackupPinInvalid)?;
        let revision = Revision::new(u64::from_be_bytes(revision_bytes))
            .map_err(|_| CompactionError::BackupPinInvalid)?;
        let _reference = ManifestSegmentReference::new(
            kind,
            id,
            ContentDigest::from_bytes(digest_bytes),
            revision,
        );
        if !pinned.insert((kind, id)) {
            return Err(CompactionError::BackupPinInvalid);
        }
        offset = end;
    }
    Ok(pinned)
}

fn shared_pin_map(root: &std::path::Path) -> SharedPinMap {
    let registries = PIN_REGISTRIES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut registries = match registries.lock() {
        Ok(registries) => registries,
        Err(poisoned) => poisoned.into_inner(),
    };
    registries.retain(|_, registry| registry.strong_count() > 0);
    if let Some(registry) = registries.get(root).and_then(Weak::upgrade) {
        return registry;
    }
    let registry = Arc::new(Mutex::new(BTreeMap::new()));
    registries.insert(root.to_path_buf(), Arc::downgrade(&registry));
    registry
}

struct StagedSegments {
    store: HistorySegmentStore,
    references: Vec<ManifestSegmentReference>,
    committed: bool,
}

impl StagedSegments {
    fn new(store: HistorySegmentStore) -> Self {
        Self {
            store,
            references: Vec::new(),
            committed: false,
        }
    }

    fn track(&mut self, reference: ManifestSegmentReference) {
        self.references.push(reference);
    }

    fn mark_committed(&mut self) {
        self.committed = true;
    }
}

impl Drop for StagedSegments {
    fn drop(&mut self) {
        if !self.committed {
            for reference in &self.references {
                let _ = self.store.remove_staged_reference(*reference);
            }
        }
    }
}

fn stage_compacted_history(
    writer_lock: &WriterLock,
    store: &HistorySegmentStore,
    inputs: &[ManifestSegmentReference],
    staged: &mut StagedSegments,
) -> Result<Vec<ManifestSegmentReference>, CompactionError> {
    let mut outputs = Vec::new();
    outputs
        .try_reserve_exact(inputs.len())
        .map_err(|_| CompactionError::Segment(SegmentError::AllocationFailed))?;
    staged
        .references
        .try_reserve_exact(inputs.len())
        .map_err(|_| CompactionError::Segment(SegmentError::AllocationFailed))?;
    let mut chunk = Vec::<DecodedRecord>::new();
    let mut chunk_frame_bytes = 0_usize;
    let mut chunk_revision = Revision::GENESIS;
    let limits = DecoderLimits::DEFAULT;

    for reference in inputs {
        let source = store
            .read_segment(reference.id())
            .map_err(CompactionError::Segment)?;
        if source.content_digest() != reference.content_digest() {
            return Err(CompactionError::Segment(
                SegmentError::ContentDigestMismatch,
            ));
        }
        let records = source.into_records();
        let mut source_frame_bytes = 0_usize;
        for record in &records {
            let frame = encode_decoded_record(record)
                .map_err(SegmentError::RecordCodec)
                .map_err(CompactionError::Segment)?;
            if frame.len() > limits.max_frame_bytes {
                return Err(CompactionError::Segment(SegmentError::ResourceLimit {
                    resource: worlddb_core::DecodeResource::BatchBytes,
                    limit: limits.max_frame_bytes,
                    actual: frame.len(),
                }));
            }
            source_frame_bytes = source_frame_bytes.checked_add(frame.len()).ok_or({
                CompactionError::Segment(SegmentError::ResourceLimit {
                    resource: worlddb_core::DecodeResource::BatchBytes,
                    limit: limits.max_batch_bytes,
                    actual: usize::MAX,
                })
            })?;
        }
        if records.is_empty() || !fits_segment(records.len(), source_frame_bytes, limits) {
            return Err(CompactionError::Segment(SegmentError::InvalidContentIndex));
        }
        let combined_count = chunk.len().checked_add(records.len()).unwrap_or(usize::MAX);
        let combined_bytes = chunk_frame_bytes
            .checked_add(source_frame_bytes)
            .unwrap_or(usize::MAX);
        if !chunk.is_empty() && !fits_segment(combined_count, combined_bytes, limits) {
            flush_chunk(
                writer_lock,
                store,
                staged,
                &mut outputs,
                &mut chunk,
                &mut chunk_frame_bytes,
                &mut chunk_revision,
            )?;
        }
        chunk
            .try_reserve(records.len())
            .map_err(|_| CompactionError::Segment(SegmentError::AllocationFailed))?;
        chunk.extend(records);
        chunk_frame_bytes = chunk_frame_bytes
            .checked_add(source_frame_bytes)
            .ok_or(CompactionError::Segment(SegmentError::AllocationFailed))?;
        chunk_revision = chunk_revision.max(reference.through_revision());
    }
    flush_chunk(
        writer_lock,
        store,
        staged,
        &mut outputs,
        &mut chunk,
        &mut chunk_frame_bytes,
        &mut chunk_revision,
    )?;
    outputs.sort_unstable_by(compare_segment_references);
    Ok(outputs)
}

fn fits_segment(count: usize, frame_bytes: usize, limits: DecoderLimits) -> bool {
    count > 0
        && count <= limits.max_records_per_batch
        && frame_bytes <= limits.max_batch_bytes
        && frame_bytes <= MAX_CANONICAL_BYTES
}

fn flush_chunk(
    writer_lock: &WriterLock,
    store: &HistorySegmentStore,
    staged: &mut StagedSegments,
    outputs: &mut Vec<ManifestSegmentReference>,
    chunk: &mut Vec<DecodedRecord>,
    chunk_frame_bytes: &mut usize,
    chunk_revision: &mut Revision,
) -> Result<(), CompactionError> {
    if chunk.is_empty() {
        return Ok(());
    }
    let receipt = store
        .stage_decoded_segment(writer_lock, chunk)
        .map_err(CompactionError::Segment)?;
    let reference = ManifestSegmentReference::new(
        ManifestSegmentKind::History,
        receipt.id(),
        receipt.content_digest(),
        *chunk_revision,
    );
    staged.track(reference);
    outputs.push(reference);
    chunk.clear();
    *chunk_frame_bytes = 0;
    *chunk_revision = Revision::GENESIS;
    Ok(())
}

#[cfg(test)]
pub(crate) fn fuzz_compaction_pin_manifest(bytes: &[u8]) -> bool {
    decode_backup_pin_manifest(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{active_backup_pin_keys, create_durable_backup_pin, fits_segment};
    use crate::{
        ContentDigest, DatabaseLayout, ManifestSegmentKind, ManifestSegmentReference, SegmentId,
    };
    use worlddb_core::{DecoderLimits, DomainId, Revision};

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn segment_chunking_respects_registered_frame_count_and_byte_limits() {
        let limits = DecoderLimits::DEFAULT;
        assert!(fits_segment(1, 1, limits));
        assert!(!fits_segment(0, 0, limits));
        assert!(!fits_segment(limits.max_records_per_batch + 1, 1, limits));
        assert!(!fits_segment(1, limits.max_batch_bytes + 1, limits));
    }

    #[test]
    fn durable_backup_pin_protects_history_until_its_lease_is_released() -> Result<(), String> {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "worlddb-durable-pin-{}-{sequence}",
            std::process::id()
        ));
        let layout = DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
        let mut id_bytes = [0_u8; 16];
        id_bytes[6] = 0x70;
        id_bytes[8] = 0x80;
        id_bytes[15] = 7;
        let id = SegmentId::try_from_bytes(id_bytes).map_err(|error| error.to_string())?;
        let reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            id,
            ContentDigest::from_bytes([0x39; 32]),
            Revision::GENESIS,
        );
        let pin =
            create_durable_backup_pin(&layout, &[reference]).map_err(|error| error.to_string())?;
        let active = active_backup_pin_keys(&layout).map_err(|error| error.to_string())?;
        assert!(active.contains(&(ManifestSegmentKind::History, id)));
        drop(pin);
        let released = active_backup_pin_keys(&layout).map_err(|error| error.to_string())?;
        assert!(!released.contains(&(ManifestSegmentKind::History, id)));
        fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        Ok(())
    }
}
