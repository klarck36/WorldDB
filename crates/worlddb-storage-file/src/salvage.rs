//! Read-only extraction of verified immutable segments into a marked fork archive.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use worlddb_core::storage_internal::generate_salvage_fork_database_id;
use worlddb_core::{DatabaseId, DomainId, IdGenerationError, Revision};

use crate::recovery::{
    CurrentManifestState, RecoveryDisposition, RecoveryFinding, RecoveryReport, RecoveryScanError,
    RecoveryScanner,
};
use crate::replay_payload::decode_replay_payload;
use crate::security_segment::{SecurityPolicyHistoryStore, security_staging_path};
use crate::segment::{ContentDigest, HistorySegmentStore, history_staging_path, segment_path};
use crate::wal::WalCommittedFrame;
use crate::{DatabaseLayout, ManifestSegmentKind, ManifestSegmentReference, WriterLock};

const BUILDING_MARKER: &str = "SALVAGE_BUILDING";
const COMPLETE_MARKER: &str = "SALVAGE";
const REPORT_FILE: &str = "SALVAGE_REPORT.tsv";

/// The inventory selected as the source of records for a salvage fork.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SalvageInventorySource {
    /// Latest valid typed snapshot in the verified committed WAL prefix.
    CommittedWalSnapshot { revision: Revision },
    /// A current manifest bound to the verified WAL commit hash.
    CurrentManifest { revision: Revision },
    /// No verified committed inventory was available.
    NoVerifiedInventory,
}

/// Unit counted for one recovered immutable segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SalvageUnit {
    /// Number of fully decoded history records.
    HistoryRecords,
    /// One complete security-policy and audit-retention snapshot.
    SecurityPolicySnapshots,
}

/// Where the exact verified source bytes were found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SalvageSegmentSource {
    /// Published immutable segment directory.
    Final,
    /// WAL-named staging directory.
    Staged,
}

/// Outcome for one segment in the selected committed inventory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SalvageSegmentOutcome {
    /// Exact immutable bytes were copied to the fork archive.
    Copied {
        /// Source location of the verified bytes.
        source: SalvageSegmentSource,
        /// Decoded records or snapshots represented by this segment.
        units: usize,
    },
    /// The segment was not copied; this reason is included in the saved report.
    Omitted {
        /// Verification or destination-write failure, with control characters escaped on disk.
        reason: String,
    },
}

/// Auditable result for one candidate segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalvageSegmentResult {
    reference: ManifestSegmentReference,
    unit: SalvageUnit,
    outcome: SalvageSegmentOutcome,
}

impl SalvageSegmentResult {
    /// Source manifest reference, including identity, digest, and through-revision.
    #[must_use]
    pub const fn reference(&self) -> ManifestSegmentReference {
        self.reference
    }

    /// Kind of count represented by this segment.
    #[must_use]
    pub const fn unit(&self) -> SalvageUnit {
        self.unit
    }

    /// Copy or omission result.
    #[must_use]
    pub const fn outcome(&self) -> &SalvageSegmentOutcome {
        &self.outcome
    }
}

/// Completed, explicitly marked salvage fork and its complete verification report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SalvageReport {
    archive_path: PathBuf,
    database_id: DatabaseId,
    safe_revision: Revision,
    disposition: RecoveryDisposition,
    inventory_source: SalvageInventorySource,
    findings: Vec<RecoveryFinding>,
    uncertainties: Vec<String>,
    segments: Vec<SalvageSegmentResult>,
    report_digest: ContentDigest,
}

impl SalvageReport {
    /// Directory containing the fork marker, report, and verified segment copies.
    #[must_use]
    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }

    /// New, independent WorldDB database identity recorded in the fork marker.
    #[must_use]
    pub const fn database_id(&self) -> DatabaseId {
        self.database_id
    }

    /// Highest WAL revision whose commit chain fully verified before salvage.
    #[must_use]
    pub const fn safe_revision(&self) -> Revision {
        self.safe_revision
    }

    /// Recovery classification observed before copying.
    #[must_use]
    pub const fn disposition(&self) -> RecoveryDisposition {
        self.disposition
    }

    /// Inventory authority used to select candidate references.
    #[must_use]
    pub const fn inventory_source(&self) -> SalvageInventorySource {
        self.inventory_source
    }

    /// Recovery findings retained from the read-only source scan.
    #[must_use]
    pub fn findings(&self) -> &[RecoveryFinding] {
        &self.findings
    }

    /// Additional unresolved inventory and snapshot-selection uncertainties.
    #[must_use]
    pub fn uncertainties(&self) -> &[String] {
        &self.uncertainties
    }

    /// Copy or omission outcome for every reference in the chosen inventory.
    #[must_use]
    pub fn segments(&self) -> &[SalvageSegmentResult] {
        &self.segments
    }

    /// BLAKE3 digest of the durable `SALVAGE_REPORT.tsv` contents.
    #[must_use]
    pub const fn report_digest(&self) -> ContentDigest {
        self.report_digest
    }
}

/// Why a salvage fork could not be completed.
#[derive(Debug)]
pub enum SalvageError {
    /// The lock does not belong to the source database.
    ForeignWriterLock,
    /// Read-only source verification failed operationally.
    Scan(RecoveryScanError),
    /// A new fork identity could not be generated.
    Identity(IdGenerationError),
    /// Target is inside the source, contains the source, or has no usable parent/name.
    InvalidTarget,
    /// The requested archive path already exists and will not be overwritten.
    TargetExists,
    /// The archive/report could not be durably written.
    Io {
        /// Operation being performed.
        operation: &'static str,
        /// Operating-system error.
        source: io::Error,
    },
}

impl fmt::Display for SalvageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWriterLock => {
                formatter.write_str("writer lock belongs to another database")
            }
            Self::Scan(error) => write!(formatter, "read-only salvage scan failed: {error}"),
            Self::Identity(error) => write!(
                formatter,
                "could not create salvage database identity: {error}"
            ),
            Self::InvalidTarget => formatter.write_str(
                "salvage target must be a new sibling/outside directory with an existing parent",
            ),
            Self::TargetExists => {
                formatter.write_str("salvage target already exists and will not be overwritten")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl std::error::Error for SalvageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scan(error) => Some(error),
            Self::Identity(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::ForeignWriterLock | Self::InvalidTarget | Self::TargetExists => None,
        }
    }
}

/// Creates a forensic fork archive without repairing or rewriting the source database.
#[derive(Clone, Debug)]
pub struct SalvageManager {
    layout: DatabaseLayout,
}

impl SalvageManager {
    /// Binds the salvage action to one opened source database.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Copies every fully verified segment in the best available committed inventory.
    ///
    /// The target must not exist and its parent must already exist. The result is an
    /// explicitly marked salvage archive for later import; it is not silently exposed
    /// as an ordinary, writable database. Source files are only scanned and read.
    pub fn salvage(
        &self,
        writer_lock: &WriterLock,
        target: impl AsRef<Path>,
    ) -> Result<SalvageReport, SalvageError> {
        if !writer_lock.belongs_to_database_root(self.layout.root()) {
            return Err(SalvageError::ForeignWriterLock);
        }
        let scanner = RecoveryScanner::new(self.layout.clone());
        let (recovery, frames) = match scanner.scan_with_frames(writer_lock) {
            Ok(result) => result,
            Err(error) => {
                writer_lock.recovery_scan_failed();
                return Err(SalvageError::Scan(error));
            }
        };
        writer_lock.note_recovery_disposition(recovery.disposition());

        let target = validate_target(self.layout.root(), target.as_ref())?;
        let database_id = generate_salvage_fork_database_id().map_err(SalvageError::Identity)?;
        let candidate = choose_inventory(&recovery, &frames);
        create_archive_root(&target, database_id)?;

        let unreferenced = collect_unreferenced_entries(&self.layout, &candidate)?;
        let mut uncertainties = candidate.uncertainties;
        uncertainties.extend(unreferenced);
        uncertainties.push(
            "the separate audit WAL/segment namespace is outside the M5-14 data inventory; audit-specific recovery and salvage remain in M5-18/M5-19".to_owned(),
        );
        if matches!(
            candidate.source,
            SalvageInventorySource::NoVerifiedInventory
        ) && recovery.safe_revision() > Revision::GENESIS
        {
            uncertainties.push(format!(
                "safe WAL revision {} exists, but no verified committed segment inventory was available; source data coverage is unknown",
                recovery.safe_revision().value()
            ));
        }

        let history_store = HistorySegmentStore::new(self.layout.clone());
        let security_store = SecurityPolicyHistoryStore::new(self.layout.clone());
        let mut segment_results = Vec::new();
        segment_results
            .try_reserve_exact(candidate.references.len())
            .map_err(|_| {
                io_error(
                    "reserve salvage report rows",
                    io::Error::other("allocation failed"),
                )
            })?;

        for reference in candidate.references {
            let unit = match reference.kind() {
                ManifestSegmentKind::History => SalvageUnit::HistoryRecords,
                ManifestSegmentKind::SecurityPolicy => SalvageUnit::SecurityPolicySnapshots,
            };
            let bytes_result = read_candidate_segment(
                &history_store,
                &security_store,
                reference,
                candidate.staged.contains(&reference),
            );
            let outcome = match bytes_result {
                Err(reason) => SalvageSegmentOutcome::Omitted {
                    reason: single_line(&reason),
                },
                Ok((bytes, units, staged)) => {
                    let destination = archive_segment_path(&target, reference);
                    match write_segment_copy(&destination, &bytes) {
                        Ok(()) => {
                            let parent = destination.parent().ok_or(SalvageError::InvalidTarget)?;
                            sync_dir(parent)?;
                            SalvageSegmentOutcome::Copied {
                                source: if staged {
                                    SalvageSegmentSource::Staged
                                } else {
                                    SalvageSegmentSource::Final
                                },
                                units,
                            }
                        }
                        Err(reason) => SalvageSegmentOutcome::Omitted {
                            reason: single_line(&reason),
                        },
                    }
                }
            };
            segment_results.push(SalvageSegmentResult {
                reference,
                unit,
                outcome,
            });
        }

        sync_dir(&target.join("history").join("segments"))?;
        sync_dir(&target.join("security").join("segments"))?;

        let mut report = SalvageReport {
            archive_path: target.clone(),
            database_id,
            safe_revision: recovery.safe_revision(),
            disposition: recovery.disposition(),
            inventory_source: candidate.source,
            findings: recovery.findings().to_vec(),
            uncertainties,
            segments: segment_results,
            report_digest: ContentDigest::from_bytes([0; 32]),
        };
        let report_bytes = render_report(self.layout.root(), &report).into_bytes();
        let report_hash = ContentDigest::from_bytes(*blake3::hash(&report_bytes).as_bytes());
        report.report_digest = report_hash;
        write_new_synced(
            &target.join(REPORT_FILE),
            &report_bytes,
            "write salvage report",
        )?;
        sync_dir(&target)?;
        let marker = format!(
            "worlddb-salvage-fork\t1\ndatabase_id\t{}\nsafe_revision\t{}\nreport\t{}\nreport_digest\t{}\n",
            database_id,
            report.safe_revision.value(),
            REPORT_FILE,
            report_hash
        );
        write_new_synced(
            &target.join(COMPLETE_MARKER),
            marker.as_bytes(),
            "publish salvage completion marker",
        )?;
        sync_dir(&target)?;
        fs::remove_file(target.join(BUILDING_MARKER))
            .map_err(|source| io_error("remove salvage building marker", source))?;
        sync_dir(&target)?;
        if let Some(parent) = target.parent() {
            sync_dir(parent)?;
        }
        Ok(report)
    }
}

fn read_candidate_segment(
    history: &HistorySegmentStore,
    security: &SecurityPolicyHistoryStore,
    reference: ManifestSegmentReference,
    allow_staged: bool,
) -> Result<(Vec<u8>, usize, bool), String> {
    let final_result = match reference.kind() {
        ManifestSegmentKind::History => history
            .read_verified_reference_bytes(reference, false)
            .map_err(|error| error.to_string()),
        ManifestSegmentKind::SecurityPolicy => security
            .read_verified_reference_bytes(reference, false)
            .map(|(bytes, staged)| (bytes, 1, staged))
            .map_err(|error| error.to_string()),
    };
    match final_result {
        Ok(result) => Ok(result),
        Err(final_error) if allow_staged => {
            let staged_result = match reference.kind() {
                ManifestSegmentKind::History => history
                    .read_verified_reference_bytes(reference, true)
                    .map_err(|error| error.to_string()),
                ManifestSegmentKind::SecurityPolicy => security
                    .read_verified_reference_bytes(reference, true)
                    .map(|(bytes, staged)| (bytes, 1, staged))
                    .map_err(|error| error.to_string()),
            };
            staged_result.map_err(|staged_error| {
                format!(
                    "published segment failed verification: {final_error}; staged fallback failed: {staged_error}"
                )
            })
        }
        Err(final_error) => Err(final_error),
    }
}

#[cfg(test)]
pub(crate) fn fuzz_salvage_scanner(layout: &DatabaseLayout, bytes: &[u8]) -> Result<bool, String> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TARGET: AtomicU64 = AtomicU64::new(0);

    let manifest = crate::ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "salvage fuzz fixture has no current manifest".to_owned())?;
    let reference = manifest
        .segments()
        .iter()
        .copied()
        .find(|item| item.kind() == ManifestSegmentKind::History)
        .ok_or_else(|| "salvage fuzz fixture has no history segment".to_owned())?;
    let segment_path = layout.segments_directory().join(format!(
        "segment-{}.wdbseg",
        reference.id().to_canonical_string()
    ));
    let original = fs::read(&segment_path).map_err(|error| error.to_string())?;
    let _restore = SalvageFuzzSegment(segment_path.clone(), original);
    fs::write(&segment_path, bytes).map_err(|error| error.to_string())?;

    let history = HistorySegmentStore::new(layout.clone());
    let security = SecurityPolicyHistoryStore::new(layout.clone());
    let decoded = read_candidate_segment(&history, &security, reference, false).is_ok();
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let sequence = NEXT_TARGET.fetch_add(1, Ordering::Relaxed);
    let target = std::env::temp_dir().join(format!(
        "worlddb-salvage-fuzz-{}-{sequence}",
        std::process::id()
    ));
    let _cleanup = SalvageFuzzTarget(target.clone());
    let salvaged = SalvageManager::new(layout.clone())
        .salvage(&lock, &target)
        .is_ok();
    Ok(decoded || salvaged)
}

#[cfg(test)]
struct SalvageFuzzSegment(PathBuf, Vec<u8>);

#[cfg(test)]
impl Drop for SalvageFuzzSegment {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, &self.1);
    }
}

#[cfg(test)]
struct SalvageFuzzTarget(PathBuf);

#[cfg(test)]
impl Drop for SalvageFuzzTarget {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct CandidateInventory {
    source: SalvageInventorySource,
    references: Vec<ManifestSegmentReference>,
    staged: Vec<ManifestSegmentReference>,
    uncertainties: Vec<String>,
}

fn choose_inventory(recovery: &RecoveryReport, frames: &[WalCommittedFrame]) -> CandidateInventory {
    let mut latest_valid_snapshot = None;
    let mut invalid_snapshots = Vec::new();
    for frame in frames.iter().rev() {
        match decode_replay_payload(
            frame.prepare().payload(),
            frame.receipt().revision(),
            frame.prepare().reference().operation_id(),
        ) {
            Ok(Some(decoded)) => {
                latest_valid_snapshot = Some((decoded.snapshot, decoded.staged));
                break;
            }
            Ok(None) => {}
            Err(_) => invalid_snapshots.push(frame.receipt().revision()),
        }
    }

    let current_manifest = match recovery.current_manifest() {
        CurrentManifestState::Loaded(manifest)
            if manifest.revision() <= recovery.safe_revision()
                && !recovery.findings().iter().any(|finding| {
                    matches!(finding, RecoveryFinding::ManifestCommitHashMismatch { manifest_revision } if *manifest_revision == manifest.revision())
                }) => Some((manifest.revision(), manifest.segments().to_vec())),
        _ => None,
    };

    let mut uncertainties = Vec::new();
    for revision in invalid_snapshots {
        uncertainties.push(format!(
            "committed replay snapshot at revision {} was malformed; the next older verified inventory was considered",
            revision.value()
        ));
    }

    match (latest_valid_snapshot, current_manifest) {
        (Some((snapshot, _staged)), Some((manifest_revision, references)))
            if manifest_revision > snapshot.revision() =>
        {
            CandidateInventory {
                source: SalvageInventorySource::CurrentManifest {
                    revision: manifest_revision,
                },
                references,
                staged: Vec::new(),
                uncertainties,
            }
        }
        (Some((snapshot, staged)), _) => CandidateInventory {
            source: SalvageInventorySource::CommittedWalSnapshot {
                revision: snapshot.revision(),
            },
            references: snapshot.segments().to_vec(),
            staged,
            uncertainties,
        },
        (None, Some((revision, references))) => CandidateInventory {
            source: SalvageInventorySource::CurrentManifest { revision },
            references,
            staged: Vec::new(),
            uncertainties,
        },
        (None, None) => CandidateInventory {
            source: SalvageInventorySource::NoVerifiedInventory,
            references: Vec::new(),
            staged: Vec::new(),
            uncertainties,
        },
    }
}

fn validate_target(source_root: &Path, requested: &Path) -> Result<PathBuf, SalvageError> {
    let parent = requested
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = requested.file_name().ok_or(SalvageError::InvalidTarget)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|_| SalvageError::InvalidTarget)?;
    if !fs::metadata(&canonical_parent).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(SalvageError::InvalidTarget);
    }
    let target = canonical_parent.join(name);
    if target.starts_with(source_root) || source_root.starts_with(&target) {
        return Err(SalvageError::InvalidTarget);
    }
    match fs::symlink_metadata(&target) {
        Ok(_) => Err(SalvageError::TargetExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(target),
        Err(_) => Err(SalvageError::InvalidTarget),
    }
}

fn collect_unreferenced_entries(
    layout: &DatabaseLayout,
    candidate: &CandidateInventory,
) -> Result<Vec<String>, SalvageError> {
    let mut scopes: Vec<(PathBuf, BTreeSet<OsString>, &'static str)> = Vec::new();
    let mut history_final = BTreeSet::new();
    let mut security_final = BTreeSet::new();
    let mut history_staged = BTreeSet::new();
    let mut security_staged = BTreeSet::new();
    for reference in &candidate.references {
        let destination = match reference.kind() {
            ManifestSegmentKind::History => &mut history_final,
            ManifestSegmentKind::SecurityPolicy => &mut security_final,
        };
        if let Some(name) = segment_path(Path::new(""), reference.id()).file_name() {
            destination.insert(name.to_os_string());
        }
        if candidate.staged.contains(reference) {
            let destination = match reference.kind() {
                ManifestSegmentKind::History => &mut history_staged,
                ManifestSegmentKind::SecurityPolicy => &mut security_staged,
            };
            let staged_name = match reference.kind() {
                ManifestSegmentKind::History => history_staging_path(Path::new(""), reference.id()),
                ManifestSegmentKind::SecurityPolicy => {
                    security_staging_path(Path::new(""), reference.id())
                }
            };
            if let Some(name) = staged_name.file_name() {
                destination.insert(name.to_os_string());
            }
        }
    }
    scopes.push((layout.segments_directory(), history_final, "segments"));
    scopes.push((
        layout.security_segments_directory(),
        security_final,
        "security/segments",
    ));
    scopes.push((
        layout.staging_directory(),
        history_staged.union(&security_staged).cloned().collect(),
        "staging",
    ));

    let mut uncertainties = Vec::new();
    for (directory, authoritative_names, label) in scopes {
        let entries = fs::read_dir(&directory)
            .map_err(|source| io_error("list salvage source segment directory", source))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|source| io_error("read salvage source directory entry", source))?;
            names.push(entry.file_name());
        }
        names.sort();
        for name in names {
            if !authoritative_names.contains(&name) {
                uncertainties.push(format!(
                    "unreferenced source entry {label}/{} was excluded because it is not named by the selected committed inventory",
                    name.to_string_lossy()
                ));
            }
        }
    }

    for (directory, label) in [
        (layout.audit_wal_directory(), "audit/wal"),
        (layout.audit_segments_directory(), "audit/segments"),
    ] {
        let entries = fs::read_dir(&directory)
            .map_err(|source| io_error("list excluded audit source directory", source))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|source| io_error("read excluded audit directory entry", source))?;
            names.push(entry.file_name());
        }
        names.sort();
        for name in names {
            uncertainties.push(format!(
                "excluded audit entry {label}/{} requires the later audit-specific salvage path",
                name.to_string_lossy()
            ));
        }
    }
    Ok(uncertainties)
}

fn create_archive_root(target: &Path, database_id: DatabaseId) -> Result<(), SalvageError> {
    fs::create_dir(target).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            SalvageError::TargetExists
        } else {
            io_error("create salvage archive", source)
        }
    })?;
    let building = format!("worlddb-salvage-building\t1\ndatabase_id\t{database_id}\n");
    write_new_synced(
        &target.join(BUILDING_MARKER),
        building.as_bytes(),
        "write salvage building marker",
    )?;
    fs::create_dir_all(target.join("history").join("segments"))
        .map_err(|source| io_error("create history salvage directory", source))?;
    fs::create_dir_all(target.join("security").join("segments"))
        .map_err(|source| io_error("create security salvage directory", source))?;
    sync_dir(&target.join("history").join("segments"))?;
    sync_dir(&target.join("history"))?;
    sync_dir(&target.join("security").join("segments"))?;
    sync_dir(&target.join("security"))?;
    sync_dir(target)?;
    if let Some(parent) = target.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn archive_segment_path(target: &Path, reference: ManifestSegmentReference) -> PathBuf {
    let namespace = match reference.kind() {
        ManifestSegmentKind::History => target.join("history").join("segments"),
        ManifestSegmentKind::SecurityPolicy => target.join("security").join("segments"),
    };
    segment_path(&namespace, reference.id())
}

fn write_segment_copy(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("could not create archive copy: {error}"))?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("could not durably write archive copy: {error}"));
    }
    Ok(())
}

fn write_new_synced(
    path: &Path,
    bytes: &[u8],
    operation: &'static str,
) -> Result<(), SalvageError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| io_error(operation, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error(operation, source))?;
    file.sync_all()
        .map_err(|source| io_error(operation, source))
}

fn sync_dir(path: &Path) -> Result<(), SalvageError> {
    crate::manifest::sync_directory(path)
        .map_err(|source| io_error("sync salvage archive directory", source))
}

fn io_error(operation: &'static str, source: io::Error) -> SalvageError {
    SalvageError::Io { operation, source }
}

fn render_report(source_root: &Path, report: &SalvageReport) -> String {
    let mut output = String::from("worlddb-salvage-report\t1\n");
    push_row(
        &mut output,
        "fork_database_id",
        &[&report.database_id.to_string()],
    );
    push_row(
        &mut output,
        "source_root",
        &[&source_root.to_string_lossy()],
    );
    push_row(
        &mut output,
        "safe_revision",
        &[&report.safe_revision.value().to_string()],
    );
    push_row(
        &mut output,
        "recovery_disposition",
        &[&format!("{:?}", report.disposition)],
    );
    match report.inventory_source {
        SalvageInventorySource::CommittedWalSnapshot { revision } => push_row(
            &mut output,
            "inventory_source",
            &["committed_wal_snapshot", &revision.value().to_string()],
        ),
        SalvageInventorySource::CurrentManifest { revision } => push_row(
            &mut output,
            "inventory_source",
            &["current_manifest", &revision.value().to_string()],
        ),
        SalvageInventorySource::NoVerifiedInventory => {
            push_row(&mut output, "inventory_source", &["none", "unknown"]);
        }
    }
    for finding in &report.findings {
        push_row(&mut output, "recovery_finding", &[&format!("{finding:?}")]);
    }
    for uncertainty in &report.uncertainties {
        push_row(&mut output, "uncertainty", &[uncertainty]);
    }
    for result in &report.segments {
        let kind = match result.reference.kind() {
            ManifestSegmentKind::History => "history",
            ManifestSegmentKind::SecurityPolicy => "security_policy",
        };
        let unit = match result.unit {
            SalvageUnit::HistoryRecords => "history_records",
            SalvageUnit::SecurityPolicySnapshots => "security_policy_snapshots",
        };
        match &result.outcome {
            SalvageSegmentOutcome::Copied { source, units } => {
                let source = match source {
                    SalvageSegmentSource::Final => "final",
                    SalvageSegmentSource::Staged => "staged",
                };
                push_row(
                    &mut output,
                    "segment",
                    &[
                        kind,
                        &result.reference.id().to_canonical_string(),
                        &result.reference.content_digest().to_string(),
                        &result.reference.through_revision().value().to_string(),
                        "copied",
                        unit,
                        &units.to_string(),
                        source,
                        "",
                    ],
                );
            }
            SalvageSegmentOutcome::Omitted { reason } => push_row(
                &mut output,
                "segment",
                &[
                    kind,
                    &result.reference.id().to_canonical_string(),
                    &result.reference.content_digest().to_string(),
                    &result.reference.through_revision().value().to_string(),
                    "omitted",
                    unit,
                    "unknown",
                    "none",
                    reason,
                ],
            ),
        }
    }
    output
}

fn push_row(output: &mut String, tag: &str, fields: &[&str]) {
    output.push_str(tag);
    for field in fields {
        output.push('\t');
        output.push_str(&single_line(field));
    }
    output.push('\n');
}

fn single_line(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}
