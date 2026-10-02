//! Canonical, explicitly scoped logical exports over a pinned history snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use worlddb_core::{
    ArchiveTargetRef, AuthorizationDecision, Capability, DatabaseId, DecodedRecord, DomainId,
    HistorySpaceDefinition, HistorySpaceId, PolicyTarget, Record, RecordCodecError, RecordKind,
    RecordRef, Revision, SecurityPolicyView, decode_record, encode_decoded_record,
};

use crate::{
    CompactionError, CompactionManager, DatabaseLayout, HistorySegmentStore, Manifest,
    ManifestError, ManifestSegmentKind, ManifestStore, SegmentError, StorageFileError,
    StorageVerifier, StorageVerifyError, WalError, WalPrepareLog, WriterLockError,
};

const LOGICAL_EXPORT_MAGIC: &[u8; 8] = b"WDBLEX\0\x01";
const LOGICAL_EXPORT_CONTEXT: &[u8] = b"WorldDB.LogicalExport.v1\0";
const LOGICAL_EXPORT_MAX_BYTES: usize = 512 * 1024 * 1024;
const LOGICAL_EXPORT_MAX_RECORDS: usize = 1_000_000;
const LOGICAL_EXPORT_MAX_SPACES: usize = 65_536;
const LOGICAL_EXPORT_HEADER_BYTES: usize = 8 + 8;
const LOGICAL_EXPORT_DIGEST_BYTES: usize = 32;

/// An explicit inclusive Transaction-Time and HistorySpace/class selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalExportScope {
    from_revision: Revision,
    through_revision: Revision,
    history_spaces: Vec<HistorySpaceId>,
    record_kinds: Vec<RecordKind>,
}

impl LogicalExportScope {
    /// Builds a canonical scope. HistorySpaces and record kinds are sorted;
    /// duplicates, empty selections, reversed ranges, and revisionless
    /// migration classes are rejected.
    pub fn new(
        from_revision: Revision,
        through_revision: Revision,
        mut history_spaces: Vec<HistorySpaceId>,
        mut record_kinds: Vec<RecordKind>,
    ) -> Result<Self, LogicalExportError> {
        if from_revision > through_revision
            || history_spaces.is_empty()
            || history_spaces.len() > LOGICAL_EXPORT_MAX_SPACES
            || record_kinds.is_empty()
            || !record_kinds.contains(&RecordKind::HistorySpaceDefinition)
        {
            return Err(LogicalExportError::InvalidScope);
        }
        history_spaces.sort_unstable();
        if history_spaces
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left == right))
        {
            return Err(LogicalExportError::DuplicateHistorySpace);
        }
        record_kinds.sort_unstable();
        if record_kinds
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left == right))
        {
            return Err(LogicalExportError::DuplicateRecordKind);
        }
        if record_kinds.iter().any(|kind| is_revisionless_kind(*kind)) {
            return Err(LogicalExportError::UnsupportedRecordKind);
        }
        Ok(Self {
            from_revision,
            through_revision,
            history_spaces,
            record_kinds,
        })
    }

    /// Inclusive lower Transaction-Time bound.
    #[must_use]
    pub const fn from_revision(&self) -> Revision {
        self.from_revision
    }

    /// Inclusive upper Transaction-Time bound.
    #[must_use]
    pub const fn through_revision(&self) -> Revision {
        self.through_revision
    }

    /// Explicitly selected HistorySpaces in canonical ID order.
    #[must_use]
    pub fn history_spaces(&self) -> &[HistorySpaceId] {
        &self.history_spaces
    }

    /// Explicitly selected record classes in canonical wire-kind order.
    #[must_use]
    pub fn record_kinds(&self) -> &[RecordKind] {
        &self.record_kinds
    }
}

/// Whether a logical-export row is in the requested range or identifies the
/// selected HistorySpace ancestry whose definition has no revision field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalExportInclusion {
    /// The record is inside the declared revision and HistorySpace scope.
    InScope,
    /// A HistorySpace definition identifies a selected space's ancestry.
    HistorySpaceDependency,
}

/// One decoded record retained in an export, including unknown optional frame flags.
#[derive(Clone, Debug)]
pub struct LogicalExportEntry {
    record: DecodedRecord,
    inclusion: LogicalExportInclusion,
}

impl LogicalExportEntry {
    /// Canonical typed record and preserved optional wire flags.
    #[must_use]
    pub const fn record(&self) -> &DecodedRecord {
        &self.record
    }

    /// Why this record is present in the exported record stream.
    #[must_use]
    pub const fn inclusion(&self) -> LogicalExportInclusion {
        self.inclusion
    }
}

/// Per-record-kind selection and included-record count.
///
/// No source counts are emitted for omitted records, so a scoped export cannot
/// disclose the size of another HistorySpace through its omission manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalExportClassSummary {
    kind: RecordKind,
    selected: bool,
    exported_records: u64,
}

impl LogicalExportClassSummary {
    /// Closed wire class summarized by this row.
    #[must_use]
    pub const fn kind(self) -> RecordKind {
        self.kind
    }

    /// Whether the caller selected this class.
    #[must_use]
    pub const fn selected(self) -> bool {
        self.selected
    }

    /// Number of records included in the export, including HistorySpace definitions.
    #[must_use]
    pub const fn exported_records(self) -> u64 {
        self.exported_records
    }
}

/// Storage classes that are always excluded from a logical export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum LogicalExportStorageClass {
    /// Separate policy-history segments and policy snapshots.
    SecurityPolicyHistory = 1,
    /// Independent raw-read audit WAL and segments.
    AuditHistory = 2,
    /// Commit WAL, prepare records, recovery journals, and writer state.
    TransactionAndRecoveryState = 3,
    /// Physical manifest generations, CURRENT, and storage-format profile.
    StorageManifestAndFormat = 4,
    /// Rebuildable index generations and index pointers.
    DerivedIndexes = 5,
}

impl LogicalExportStorageClass {
    /// Every non-logical storage class that the manifest explicitly excludes.
    pub const ALL: [Self; 5] = [
        Self::SecurityPolicyHistory,
        Self::AuditHistory,
        Self::TransactionAndRecoveryState,
        Self::StorageManifestAndFormat,
        Self::DerivedIndexes,
    ];
}

/// One selected HistorySpace and its visible Transaction-Time cutoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalExportHistorySpace {
    id: HistorySpaceId,
    visible_through: Revision,
    selected: bool,
}

impl LogicalExportHistorySpace {
    /// HistorySpace identity.
    #[must_use]
    pub const fn id(self) -> HistorySpaceId {
        self.id
    }

    /// Latest revision visible from the selected scope through ancestry.
    #[must_use]
    pub const fn visible_through(self) -> Revision {
        self.visible_through
    }

    /// Whether this is an explicitly selected space or an ancestry dependency.
    #[must_use]
    pub const fn selected(self) -> bool {
        self.selected
    }
}

/// Explicit scope, source snapshot identity, and omissions for one export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalExportManifest {
    database_id: DatabaseId,
    snapshot_revision: Revision,
    from_revision: Revision,
    through_revision: Revision,
    selected_history_spaces: Vec<HistorySpaceId>,
    visible_history_spaces: Vec<LogicalExportHistorySpace>,
    selected_record_kinds: Vec<RecordKind>,
    classes: Vec<LogicalExportClassSummary>,
    omitted_storage_classes: Vec<LogicalExportStorageClass>,
}

impl LogicalExportManifest {
    /// Source database identity.
    #[must_use]
    pub const fn database_id(&self) -> DatabaseId {
        self.database_id
    }

    /// Pinned source commit revision from which records were read.
    #[must_use]
    pub const fn snapshot_revision(&self) -> Revision {
        self.snapshot_revision
    }

    /// Inclusive lower Transaction-Time bound.
    #[must_use]
    pub const fn from_revision(&self) -> Revision {
        self.from_revision
    }

    /// Inclusive upper Transaction-Time bound.
    #[must_use]
    pub const fn through_revision(&self) -> Revision {
        self.through_revision
    }

    /// Explicitly selected HistorySpaces.
    #[must_use]
    pub fn selected_history_spaces(&self) -> &[HistorySpaceId] {
        &self.selected_history_spaces
    }

    /// Selected HistorySpaces and the ancestry cutoffs needed to interpret them.
    #[must_use]
    pub fn visible_history_spaces(&self) -> &[LogicalExportHistorySpace] {
        &self.visible_history_spaces
    }

    /// Explicitly selected record classes.
    #[must_use]
    pub fn selected_record_kinds(&self) -> &[RecordKind] {
        &self.selected_record_kinds
    }

    /// Complete row for every closed record class, including zero-count classes.
    #[must_use]
    pub fn classes(&self) -> &[LogicalExportClassSummary] {
        &self.classes
    }

    /// Non-logical storage classes intentionally excluded from this artifact.
    #[must_use]
    pub fn omitted_storage_classes(&self) -> &[LogicalExportStorageClass] {
        &self.omitted_storage_classes
    }
}

/// Canonical standalone logical-export artifact.
#[derive(Clone, Debug)]
pub struct LogicalExport {
    manifest: LogicalExportManifest,
    records: Vec<LogicalExportEntry>,
}

impl LogicalExport {
    /// Export manifest, including selected and omitted classes.
    #[must_use]
    pub const fn manifest(&self) -> &LogicalExportManifest {
        &self.manifest
    }

    /// Canonically ordered typed records.
    #[must_use]
    pub fn records(&self) -> &[LogicalExportEntry] {
        &self.records
    }

    /// Encodes a stable, versioned artifact with a BLAKE3 integrity digest.
    pub fn encode(&self) -> Result<Vec<u8>, LogicalExportError> {
        validate_export(self)?;
        let mut payload = Vec::new();
        encode_manifest(&mut payload, &self.manifest)?;
        push_u32(
            &mut payload,
            u32::try_from(self.records.len()).map_err(|_| LogicalExportError::ResourceLimit)?,
        );
        for entry in &self.records {
            let frame = encode_decoded_record(&entry.record).map_err(LogicalExportError::Record)?;
            push_u8(
                &mut payload,
                match entry.inclusion {
                    LogicalExportInclusion::InScope => 1,
                    LogicalExportInclusion::HistorySpaceDependency => 2,
                },
            );
            push_bytes_u32(&mut payload, &frame)?;
            if payload.len() > LOGICAL_EXPORT_MAX_BYTES {
                return Err(LogicalExportError::ResourceLimit);
            }
        }
        let mut bytes = Vec::new();
        let total_len = LOGICAL_EXPORT_HEADER_BYTES
            .checked_add(payload.len())
            .and_then(|value| value.checked_add(LOGICAL_EXPORT_DIGEST_BYTES))
            .ok_or(LogicalExportError::ResourceLimit)?;
        if total_len > LOGICAL_EXPORT_MAX_BYTES {
            return Err(LogicalExportError::ResourceLimit);
        }
        bytes
            .try_reserve_exact(total_len)
            .map_err(|_| LogicalExportError::AllocationFailed)?;
        bytes.extend_from_slice(LOGICAL_EXPORT_MAGIC);
        push_u64(
            &mut bytes,
            u64::try_from(payload.len()).map_err(|_| LogicalExportError::ResourceLimit)?,
        );
        bytes.extend_from_slice(&payload);
        let mut hasher = blake3::Hasher::new();
        hasher.update(LOGICAL_EXPORT_CONTEXT);
        hasher.update(&bytes);
        bytes.extend_from_slice(hasher.finalize().as_bytes());
        Ok(bytes)
    }

    /// Decodes an artifact, checking integrity, bounds, scope, omissions, and canonical order.
    pub fn decode(bytes: &[u8]) -> Result<Self, LogicalExportError> {
        if bytes.len() < LOGICAL_EXPORT_HEADER_BYTES + LOGICAL_EXPORT_DIGEST_BYTES
            || bytes.len() > LOGICAL_EXPORT_MAX_BYTES
            || bytes.get(..8) != Some(LOGICAL_EXPORT_MAGIC.as_slice())
        {
            return Err(LogicalExportError::InvalidEncoding);
        }
        let digest_offset = bytes
            .len()
            .checked_sub(LOGICAL_EXPORT_DIGEST_BYTES)
            .ok_or(LogicalExportError::InvalidEncoding)?;
        let claimed_digest: [u8; 32] = bytes
            .get(digest_offset..)
            .ok_or(LogicalExportError::InvalidEncoding)?
            .try_into()
            .map_err(|_| LogicalExportError::InvalidEncoding)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(LOGICAL_EXPORT_CONTEXT);
        hasher.update(
            bytes
                .get(..digest_offset)
                .ok_or(LogicalExportError::InvalidEncoding)?,
        );
        if *hasher.finalize().as_bytes() != claimed_digest {
            return Err(LogicalExportError::DigestMismatch);
        }
        let payload_length = read_u64(bytes, 8)?;
        let payload_length =
            usize::try_from(payload_length).map_err(|_| LogicalExportError::ResourceLimit)?;
        let payload_end = LOGICAL_EXPORT_HEADER_BYTES
            .checked_add(payload_length)
            .ok_or(LogicalExportError::InvalidEncoding)?;
        if payload_end != digest_offset {
            return Err(LogicalExportError::InvalidEncoding);
        }
        let mut cursor = Cursor::new(
            bytes
                .get(LOGICAL_EXPORT_HEADER_BYTES..payload_end)
                .ok_or(LogicalExportError::InvalidEncoding)?,
        );
        let manifest = decode_manifest(&mut cursor)?;
        let record_count =
            usize::try_from(cursor.u32()?).map_err(|_| LogicalExportError::ResourceLimit)?;
        if record_count > LOGICAL_EXPORT_MAX_RECORDS {
            return Err(LogicalExportError::ResourceLimit);
        }
        let mut records = Vec::new();
        records
            .try_reserve_exact(record_count)
            .map_err(|_| LogicalExportError::AllocationFailed)?;
        for _ in 0..record_count {
            let inclusion = match cursor.u8()? {
                1 => LogicalExportInclusion::InScope,
                2 => LogicalExportInclusion::HistorySpaceDependency,
                _ => return Err(LogicalExportError::InvalidEncoding),
            };
            let frame = cursor.bytes_u32(LOGICAL_EXPORT_MAX_BYTES)?;
            records.push(LogicalExportEntry {
                record: decode_record(frame).map_err(LogicalExportError::Record)?,
                inclusion,
            });
        }
        if !cursor.is_empty() {
            return Err(LogicalExportError::InvalidEncoding);
        }
        let export = Self { manifest, records };
        validate_export(&export)?;
        if export.encode()?.as_slice() != bytes {
            return Err(LogicalExportError::NonCanonicalEncoding);
        }
        Ok(export)
    }
}

/// Creates a logical export from one verified and pinned current storage snapshot.
#[derive(Clone, Debug)]
pub struct LogicalExportManager {
    layout: DatabaseLayout,
}

impl LogicalExportManager {
    /// Binds the exporter to an opened database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Captures and encodes the requested logical scope from a clean snapshot.
    /// Authorization is checked against the always-current policy view before
    /// any source records are read.
    pub fn export(
        &self,
        scope: LogicalExportScope,
        policy: SecurityPolicyView<'_>,
    ) -> Result<LogicalExport, LogicalExportError> {
        authorize_scope(&scope, policy)?;
        let layout =
            DatabaseLayout::open(self.layout.root()).map_err(LogicalExportError::Layout)?;
        let lock = layout
            .try_writer_lock()
            .map_err(LogicalExportError::WriterLock)?;
        let report = StorageVerifier::new(layout.clone())
            .verify(&lock)
            .map_err(LogicalExportError::StorageVerify)?;
        if !report.is_clean() {
            return Err(LogicalExportError::SourceNotClean);
        }
        let database_id = layout
            .database_id()
            .ok_or(LogicalExportError::DatabaseIdentityMissing)?;
        let head = WalPrepareLog::new(&layout)
            .commit_head(&lock)
            .map_err(LogicalExportError::Wal)?;
        if head.revision() != report.safe_revision() || scope.through_revision > head.revision() {
            return Err(LogicalExportError::SnapshotMismatch);
        }
        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(LogicalExportError::Manifest)?;
        match manifest.as_ref() {
            Some(current)
                if current.revision() == head.revision()
                    && current.commit_hash() == head.commit_hash() => {}
            None if head.revision() == Revision::GENESIS => {}
            _ => return Err(LogicalExportError::SnapshotMismatch),
        }
        let references = manifest.as_ref().map_or(&[][..], Manifest::segments);
        let compaction = CompactionManager::new(layout.clone());
        let (_segment_pins, _durable_pin) = compaction
            .pin_export_snapshot(&lock, references)
            .map_err(LogicalExportError::Compaction)?;
        drop(lock);

        let mut records = Vec::new();
        let history_store = HistorySegmentStore::new(layout);
        for reference in references
            .iter()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
        {
            let segment = history_store
                .read_segment(reference.id())
                .map_err(LogicalExportError::Segment)?;
            if segment.content_digest() != reference.content_digest() {
                return Err(LogicalExportError::SnapshotMismatch);
            }
            records
                .try_reserve(segment.records().len())
                .map_err(|_| LogicalExportError::AllocationFailed)?;
            records.extend(segment.records().iter().cloned());
            if records.len() > LOGICAL_EXPORT_MAX_RECORDS {
                return Err(LogicalExportError::ResourceLimit);
            }
        }
        build_export(database_id, head.revision(), &scope, records)
    }
}

/// Why the requested logical export could not be materialized or decoded.
#[derive(Debug)]
pub enum LogicalExportError {
    /// Scope is empty, reversed, too large, or selects a revisionless class.
    InvalidScope,
    /// A selected HistorySpace occurs more than once.
    DuplicateHistorySpace,
    /// A selected record kind occurs more than once.
    DuplicateRecordKind,
    /// A requested HistorySpace or one of its ancestors is absent.
    UnknownHistorySpace,
    /// One or more selected spaces lack current DataExport permission.
    AuthorizationDenied,
    /// Global records are selected without project-level DataExport permission.
    ProjectAuthorizationDenied,
    /// An export selection contains a record class with no durable commit revision.
    UnsupportedRecordKind,
    /// The source data contains a selected class with no durable commit revision.
    RevisionlessRecord,
    /// An included relation has no complete HistorySpace ownership information.
    MissingDependency,
    /// Current storage verification found damage or recovery requirements.
    SourceNotClean,
    /// The WAL and current manifest did not describe the same pinned snapshot.
    SnapshotMismatch,
    /// A database layout has no persistent database identity.
    DatabaseIdentityMissing,
    /// An artifact is malformed, truncated, or contains trailing bytes.
    InvalidEncoding,
    /// An artifact integrity digest differs from its content.
    DigestMismatch,
    /// A valid artifact is not in canonical byte form.
    NonCanonicalEncoding,
    /// The artifact exceeds one of the registered count or byte limits.
    ResourceLimit,
    /// A bounded allocation failed.
    AllocationFailed,
    /// Storage layout open failed.
    Layout(StorageFileError),
    /// Database lock acquisition failed.
    WriterLock(WriterLockError),
    /// Read-only storage verification failed.
    StorageVerify(StorageVerifyError),
    /// WAL commit-head scan failed.
    Wal(WalError),
    /// The current manifest could not be validated.
    Manifest(ManifestError),
    /// Segment retention could not be established safely.
    Compaction(CompactionError),
    /// An immutable history segment could not be read or verified.
    Segment(SegmentError),
    /// A typed record could not be encoded or decoded.
    Record(RecordCodecError),
}

impl fmt::Display for LogicalExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScope => formatter.write_str("logical export scope is invalid"),
            Self::DuplicateHistorySpace => {
                formatter.write_str("HistorySpace selection has duplicates")
            }
            Self::DuplicateRecordKind => {
                formatter.write_str("record-class selection has duplicates")
            }
            Self::UnknownHistorySpace => {
                formatter.write_str("selected HistorySpace or its ancestry is missing")
            }
            Self::AuthorizationDenied => {
                formatter.write_str("DataExport is denied for a selected HistorySpace")
            }
            Self::ProjectAuthorizationDenied => formatter
                .write_str("project-level DataExport is denied for selected global classes"),
            Self::UnsupportedRecordKind => {
                formatter.write_str("selected record class has no durable revision coordinate")
            }
            Self::RevisionlessRecord => {
                formatter.write_str("record has no durable Transaction-Time revision")
            }
            Self::MissingDependency => {
                formatter.write_str("record ownership dependency is incomplete")
            }
            Self::SourceNotClean => {
                formatter.write_str("logical export requires a clean verified source snapshot")
            }
            Self::SnapshotMismatch => {
                formatter.write_str("logical export source snapshot changed or is inconsistent")
            }
            Self::DatabaseIdentityMissing => {
                formatter.write_str("source database identity is missing")
            }
            Self::InvalidEncoding => formatter.write_str("logical export artifact is malformed"),
            Self::DigestMismatch => {
                formatter.write_str("logical export digest does not match its content")
            }
            Self::NonCanonicalEncoding => {
                formatter.write_str("logical export artifact is not canonical")
            }
            Self::ResourceLimit => {
                formatter.write_str("logical export exceeds a registered resource limit")
            }
            Self::AllocationFailed => formatter.write_str("logical export allocation failed"),
            Self::Layout(error) => write!(formatter, "database layout: {error}"),
            Self::WriterLock(error) => write!(formatter, "database writer lock: {error}"),
            Self::StorageVerify(error) => write!(formatter, "storage verification: {error}"),
            Self::Wal(error) => write!(formatter, "WAL snapshot: {error}"),
            Self::Manifest(error) => write!(formatter, "manifest: {error}"),
            Self::Compaction(error) => write!(formatter, "export segment pin: {error}"),
            Self::Segment(error) => write!(formatter, "history segment: {error}"),
            Self::Record(error) => write!(formatter, "record frame: {error}"),
        }
    }
}

impl std::error::Error for LogicalExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::WriterLock(error) => Some(error),
            Self::StorageVerify(error) => Some(error),
            Self::Wal(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::Compaction(error) => Some(error),
            Self::Segment(error) => Some(error),
            Self::Record(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordOwnership {
    Global,
    Spaces {
        ids: [Option<HistorySpaceId>; 2],
        all: bool,
    },
}

fn authorize_scope(
    scope: &LogicalExportScope,
    policy: SecurityPolicyView<'_>,
) -> Result<(), LogicalExportError> {
    let current = policy.current_snapshot();
    for history_space in &scope.history_spaces {
        let target = PolicyTarget::new(Some(*history_space), None, None, None, None);
        if current.authorize(policy.principal_id(), Capability::DataExport, target)
            != AuthorizationDecision::Allow
        {
            return Err(LogicalExportError::AuthorizationDenied);
        }
    }
    if scope
        .record_kinds
        .iter()
        .copied()
        .any(is_project_scoped_kind)
        && current.authorize(
            policy.principal_id(),
            Capability::DataExport,
            PolicyTarget::default(),
        ) != AuthorizationDecision::Allow
    {
        return Err(LogicalExportError::ProjectAuthorizationDenied);
    }
    Ok(())
}

fn is_project_scoped_kind(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::Entity
            | RecordKind::EntityRetirement
            | RecordKind::PerspectiveDefinitionRevision
            | RecordKind::PerspectiveRetirement
            | RecordKind::LayerDefinition
            | RecordKind::LayerSchemaSnapshot
            | RecordKind::EntityTypeDefinition
            | RecordKind::PredicateDefinition
            | RecordKind::EventKindDefinition
            | RecordKind::MigrationPlan
            | RecordKind::MigrationRun
            | RecordKind::MigrationStepCommitIdentity
            | RecordKind::Source
            | RecordKind::Evidence
            | RecordKind::Provenance
            | RecordKind::EvidenceRetraction
            | RecordKind::ProvenanceRetraction
            | RecordKind::ArchiveTransition
    )
}

const fn is_revisionless_kind(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::MigrationPlan
            | RecordKind::MigrationRun
            | RecordKind::MigrationStepCommitIdentity
    )
}

fn build_export(
    database_id: DatabaseId,
    snapshot_revision: Revision,
    scope: &LogicalExportScope,
    decoded_records: Vec<DecodedRecord>,
) -> Result<LogicalExport, LogicalExportError> {
    if scope.through_revision > snapshot_revision {
        return Err(LogicalExportError::SnapshotMismatch);
    }
    let selected: BTreeSet<_> = scope.record_kinds.iter().copied().collect();
    let definitions = decoded_records
        .iter()
        .filter_map(|decoded| match decoded.record() {
            Record::HistorySpaceDefinition(definition) => Some(*definition),
            _ => None,
        })
        .collect::<Vec<_>>();
    let visible_spaces = visible_history_spaces(scope, &definitions)?;
    let visible_cutoffs = visible_spaces
        .iter()
        .map(|space| (space.id, space.visible_through))
        .collect::<BTreeMap<_, _>>();
    let ownership = record_ownerships(&decoded_records)?;
    let mut counts = BTreeMap::new();
    for kind in RecordKind::ALL {
        counts.insert(kind, 0_u64);
    }
    let mut included = Vec::new();
    let mut encoded_bytes = 0usize;
    for record in decoded_records {
        let value = record.record();
        let kind = RecordKind::of(value);
        if !selected.contains(&kind) {
            continue;
        }
        let is_space_definition = matches!(value, Record::HistorySpaceDefinition(_));
        let revision = record_revision(value);
        let space_definition_dependency = match value {
            Record::HistorySpaceDefinition(definition) => {
                visible_cutoffs.contains_key(&definition.history_space_id())
            }
            _ => false,
        };
        if is_space_definition && !space_definition_dependency {
            continue;
        }
        if !is_space_definition && revision.is_none() {
            return Err(LogicalExportError::RevisionlessRecord);
        }
        if !space_definition_dependency
            && revision
                .is_some_and(|value| value < scope.from_revision || value > scope.through_revision)
        {
            continue;
        }
        if !space_definition_dependency
            && !record_is_in_visible_scope(value, revision, &ownership, &visible_cutoffs)?
        {
            continue;
        }
        let frame = encode_decoded_record(&record).map_err(LogicalExportError::Record)?;
        encoded_bytes = encoded_bytes
            .checked_add(frame.len())
            .ok_or(LogicalExportError::ResourceLimit)?;
        if encoded_bytes > LOGICAL_EXPORT_MAX_BYTES || included.len() >= LOGICAL_EXPORT_MAX_RECORDS
        {
            return Err(LogicalExportError::ResourceLimit);
        }
        let class = counts
            .get_mut(&kind)
            .ok_or(LogicalExportError::InvalidEncoding)?;
        *class = class
            .checked_add(1)
            .ok_or(LogicalExportError::ResourceLimit)?;
        let inclusion = if space_definition_dependency {
            LogicalExportInclusion::HistorySpaceDependency
        } else {
            LogicalExportInclusion::InScope
        };
        included.push((
            revision.unwrap_or(Revision::GENESIS),
            kind,
            frame,
            LogicalExportEntry { record, inclusion },
        ));
    }
    included
        .sort_by(|left, right| (&left.0, &left.1, &left.2).cmp(&(&right.0, &right.1, &right.2)));
    let classes = RecordKind::ALL
        .into_iter()
        .map(|kind| {
            let exported_records = counts
                .get(&kind)
                .copied()
                .ok_or(LogicalExportError::InvalidEncoding)?;
            Ok(LogicalExportClassSummary {
                kind,
                selected: selected.contains(&kind),
                exported_records,
            })
        })
        .collect::<Result<Vec<_>, LogicalExportError>>()?;
    let records = included.into_iter().map(|(_, _, _, entry)| entry).collect();
    let export = LogicalExport {
        manifest: LogicalExportManifest {
            database_id,
            snapshot_revision,
            from_revision: scope.from_revision,
            through_revision: scope.through_revision,
            selected_history_spaces: scope.history_spaces.clone(),
            visible_history_spaces: visible_spaces,
            selected_record_kinds: scope.record_kinds.clone(),
            classes,
            omitted_storage_classes: LogicalExportStorageClass::ALL.to_vec(),
        },
        records,
    };
    validate_export(&export)?;
    Ok(export)
}

fn visible_history_spaces(
    scope: &LogicalExportScope,
    definitions: &[HistorySpaceDefinition],
) -> Result<Vec<LogicalExportHistorySpace>, LogicalExportError> {
    let mut by_id = BTreeMap::new();
    for definition in definitions {
        if by_id
            .insert(definition.history_space_id(), *definition)
            .is_some()
        {
            return Err(LogicalExportError::MissingDependency);
        }
    }
    let mut cutoffs = BTreeMap::<HistorySpaceId, (Revision, bool)>::new();
    for selected_id in &scope.history_spaces {
        if !by_id.contains_key(selected_id) {
            return Err(LogicalExportError::UnknownHistorySpace);
        }
        let mut current_id = *selected_id;
        let mut cutoff = scope.through_revision;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(current_id) {
                return Err(LogicalExportError::MissingDependency);
            }
            let definition = by_id
                .get(&current_id)
                .ok_or(LogicalExportError::UnknownHistorySpace)?;
            cutoffs
                .entry(current_id)
                .and_modify(|entry| {
                    entry.0 = entry.0.max(cutoff);
                    entry.1 |= current_id == *selected_id;
                })
                .or_insert((cutoff, current_id == *selected_id));
            match definition.parent_history_space_id() {
                Some(parent_id) => {
                    cutoff = cutoff.min(definition.base_revision());
                    current_id = parent_id;
                }
                None => break,
            }
        }
    }
    Ok(cutoffs
        .into_iter()
        .map(
            |(id, (visible_through, selected))| LogicalExportHistorySpace {
                id,
                visible_through,
                selected,
            },
        )
        .collect())
}

fn record_ownerships(
    records: &[DecodedRecord],
) -> Result<BTreeMap<RecordRef, RecordOwnership>, LogicalExportError> {
    let mut ownerships = BTreeMap::new();
    let mut event_spaces = BTreeMap::new();
    for decoded in records {
        match decoded.record() {
            Record::Assertion(value) => {
                let space = value.context().history_space_id();
                insert_ownership(
                    &mut ownerships,
                    RecordRef::Assertion(value.id()),
                    one_space(space),
                )?;
            }
            Record::Mask(value) => {
                let space = value.context().history_space_id();
                insert_ownership(
                    &mut ownerships,
                    RecordRef::Mask(value.id()),
                    one_space(space),
                )?;
            }
            Record::ReplacementBoundary(value) => {
                let space = value.context().history_space_id();
                insert_ownership(
                    &mut ownerships,
                    RecordRef::ReplacementBoundary(value.id()),
                    one_space(space),
                )?;
            }
            Record::Event(value) => {
                let space = value.history_space_id();
                event_spaces.insert(value.id(), space);
                insert_ownership(
                    &mut ownerships,
                    RecordRef::Event(value.id()),
                    one_space(space),
                )?;
            }
            Record::EventMask(value) => {
                let space = value.history_space_id();
                insert_ownership(
                    &mut ownerships,
                    RecordRef::EventMask(value.id()),
                    one_space(space),
                )?;
            }
            Record::Source(value) => {
                insert_ownership(
                    &mut ownerships,
                    RecordRef::Source(value.id()),
                    RecordOwnership::Global,
                )?;
            }
            Record::Evidence(value) => {
                insert_ownership(
                    &mut ownerships,
                    RecordRef::Evidence(value.id()),
                    RecordOwnership::Global,
                )?;
            }
            Record::Provenance(value) => {
                insert_ownership(
                    &mut ownerships,
                    RecordRef::Provenance(value.id()),
                    RecordOwnership::Global,
                )?;
            }
            Record::EntityRetirement(value) => {
                insert_ownership(
                    &mut ownerships,
                    RecordRef::EntityRetirement(value.entity_retirement_id()),
                    RecordOwnership::Global,
                )?;
            }
            Record::PerspectiveRetirement(value) => {
                insert_ownership(
                    &mut ownerships,
                    RecordRef::PerspectiveRetirement(value.perspective_retirement_id()),
                    RecordOwnership::Global,
                )?;
            }
            Record::TransferLineage(value) => {
                insert_ownership(
                    &mut ownerships,
                    RecordRef::TransferLineage(value.id()),
                    two_spaces(
                        value.source_history_space_id(),
                        value.target_history_space_id(),
                    ),
                )?;
            }
            _ => {}
        }
    }
    for decoded in records {
        if let Record::EventRelation(value) = decoded.record() {
            let from = *event_spaces
                .get(&value.from_event())
                .ok_or(LogicalExportError::MissingDependency)?;
            let to = *event_spaces
                .get(&value.to_event())
                .ok_or(LogicalExportError::MissingDependency)?;
            insert_ownership(
                &mut ownerships,
                RecordRef::EventRelation(value.id()),
                two_spaces(from, to),
            )?;
        }
    }
    for decoded in records {
        if let Some((reference, target)) = lifecycle_identity_and_target(decoded.record()) {
            let inherited = ownerships
                .get(&target)
                .copied()
                .ok_or(LogicalExportError::MissingDependency)?;
            insert_ownership(&mut ownerships, reference, inherited)?;
        }
    }
    for decoded in records {
        if let Record::ArchiveTransition(value) = decoded.record() {
            let target = archive_target_record_ref(value.target());
            let inherited = ownerships
                .get(&target)
                .copied()
                .ok_or(LogicalExportError::MissingDependency)?;
            insert_ownership(
                &mut ownerships,
                RecordRef::ArchiveTransition(value.id()),
                inherited,
            )?;
        }
    }
    Ok(ownerships)
}

pub(crate) fn validate_import_references(export: &LogicalExport) -> Result<(), LogicalExportError> {
    let records = export
        .records
        .iter()
        .map(|entry| entry.record.clone())
        .collect::<Vec<_>>();
    record_ownerships(&records)?;
    Ok(())
}

fn lifecycle_identity_and_target(record: &Record) -> Option<(RecordRef, RecordRef)> {
    Some(match record {
        Record::AssertionValidityClosure(value) => (
            RecordRef::AssertionValidityClosure(value.id()),
            RecordRef::Assertion(value.assertion_id()),
        ),
        Record::AssertionRetraction(value) => (
            RecordRef::AssertionRetraction(value.id()),
            RecordRef::Assertion(value.assertion_id()),
        ),
        Record::MaskValidityClosure(value) => (
            RecordRef::MaskValidityClosure(value.id()),
            RecordRef::Mask(value.mask_id()),
        ),
        Record::MaskRetraction(value) => (
            RecordRef::MaskRetraction(value.id()),
            RecordRef::Mask(value.mask_id()),
        ),
        Record::ReplacementBoundaryValidityClosure(value) => (
            RecordRef::ReplacementBoundaryValidityClosure(value.id()),
            RecordRef::ReplacementBoundary(value.replacement_boundary_id()),
        ),
        Record::ReplacementBoundaryRetraction(value) => (
            RecordRef::ReplacementBoundaryRetraction(value.id()),
            RecordRef::ReplacementBoundary(value.replacement_boundary_id()),
        ),
        Record::EventSpanClosure(value) => (
            RecordRef::EventSpanClosure(value.id()),
            RecordRef::Event(value.event_id()),
        ),
        Record::EventRetraction(value) => (
            RecordRef::EventRetraction(value.id()),
            RecordRef::Event(value.event_id()),
        ),
        Record::EventMaskRetraction(value) => (
            RecordRef::EventMaskRetraction(value.id()),
            RecordRef::EventMask(value.event_mask_id()),
        ),
        Record::EventRelationRetraction(value) => (
            RecordRef::EventRelationRetraction(value.id()),
            RecordRef::EventRelation(value.event_relation_id()),
        ),
        Record::EvidenceRetraction(value) => (
            RecordRef::EvidenceRetraction(value.id()),
            RecordRef::Evidence(value.evidence_id()),
        ),
        Record::ProvenanceRetraction(value) => (
            RecordRef::ProvenanceRetraction(value.id()),
            RecordRef::Provenance(value.provenance_id()),
        ),
        _ => return None,
    })
}

fn archive_target_record_ref(target: ArchiveTargetRef) -> RecordRef {
    match target {
        ArchiveTargetRef::Assertion(id) => RecordRef::Assertion(id),
        ArchiveTargetRef::Mask(id) => RecordRef::Mask(id),
        ArchiveTargetRef::ReplacementBoundary(id) => RecordRef::ReplacementBoundary(id),
        ArchiveTargetRef::Event(id) => RecordRef::Event(id),
        ArchiveTargetRef::EventMask(id) => RecordRef::EventMask(id),
        ArchiveTargetRef::EventRelation(id) => RecordRef::EventRelation(id),
        ArchiveTargetRef::Source(id) => RecordRef::Source(id),
        ArchiveTargetRef::Evidence(id) => RecordRef::Evidence(id),
        ArchiveTargetRef::Provenance(id) => RecordRef::Provenance(id),
        ArchiveTargetRef::AssertionValidityClosure(id) => RecordRef::AssertionValidityClosure(id),
        ArchiveTargetRef::AssertionRetraction(id) => RecordRef::AssertionRetraction(id),
        ArchiveTargetRef::MaskValidityClosure(id) => RecordRef::MaskValidityClosure(id),
        ArchiveTargetRef::MaskRetraction(id) => RecordRef::MaskRetraction(id),
        ArchiveTargetRef::ReplacementBoundaryValidityClosure(id) => {
            RecordRef::ReplacementBoundaryValidityClosure(id)
        }
        ArchiveTargetRef::ReplacementBoundaryRetraction(id) => {
            RecordRef::ReplacementBoundaryRetraction(id)
        }
        ArchiveTargetRef::EventSpanClosure(id) => RecordRef::EventSpanClosure(id),
        ArchiveTargetRef::EventRetraction(id) => RecordRef::EventRetraction(id),
        ArchiveTargetRef::EventMaskRetraction(id) => RecordRef::EventMaskRetraction(id),
        ArchiveTargetRef::EventRelationRetraction(id) => RecordRef::EventRelationRetraction(id),
        ArchiveTargetRef::EvidenceRetraction(id) => RecordRef::EvidenceRetraction(id),
        ArchiveTargetRef::ProvenanceRetraction(id) => RecordRef::ProvenanceRetraction(id),
        ArchiveTargetRef::EntityRetirement(id) => RecordRef::EntityRetirement(id),
        ArchiveTargetRef::PerspectiveRetirement(id) => RecordRef::PerspectiveRetirement(id),
        ArchiveTargetRef::TransferLineage(id) => RecordRef::TransferLineage(id),
    }
}

fn insert_ownership(
    ownerships: &mut BTreeMap<RecordRef, RecordOwnership>,
    reference: RecordRef,
    ownership: RecordOwnership,
) -> Result<(), LogicalExportError> {
    if ownerships.insert(reference, ownership).is_some() {
        return Err(LogicalExportError::MissingDependency);
    }
    Ok(())
}

fn one_space(id: HistorySpaceId) -> RecordOwnership {
    RecordOwnership::Spaces {
        ids: [Some(id), None],
        all: true,
    }
}

fn two_spaces(first: HistorySpaceId, second: HistorySpaceId) -> RecordOwnership {
    if first == second {
        one_space(first)
    } else {
        RecordOwnership::Spaces {
            ids: [Some(first), Some(second)],
            all: true,
        }
    }
}

fn record_is_in_visible_scope(
    record: &Record,
    revision: Option<Revision>,
    ownerships: &BTreeMap<RecordRef, RecordOwnership>,
    visible_cutoffs: &BTreeMap<HistorySpaceId, Revision>,
) -> Result<bool, LogicalExportError> {
    let revision = revision.ok_or(LogicalExportError::RevisionlessRecord)?;
    let Some(reference) = record_ref(record) else {
        return Ok(true);
    };
    let ownership = ownerships
        .get(&reference)
        .copied()
        .unwrap_or(RecordOwnership::Global);
    match ownership {
        RecordOwnership::Global => Ok(true),
        RecordOwnership::Spaces { ids, all } => {
            let present = ids.into_iter().flatten().collect::<Vec<_>>();
            if all {
                Ok(present.iter().all(|id| {
                    visible_cutoffs
                        .get(id)
                        .is_some_and(|cutoff| revision <= *cutoff)
                }))
            } else {
                Ok(present.iter().any(|id| {
                    visible_cutoffs
                        .get(id)
                        .is_some_and(|cutoff| revision <= *cutoff)
                }))
            }
        }
    }
}

pub(crate) fn record_ref(record: &Record) -> Option<RecordRef> {
    Some(match record {
        Record::Assertion(value) => RecordRef::Assertion(value.id()),
        Record::AssertionValidityClosure(value) => RecordRef::AssertionValidityClosure(value.id()),
        Record::AssertionRetraction(value) => RecordRef::AssertionRetraction(value.id()),
        Record::Mask(value) => RecordRef::Mask(value.id()),
        Record::MaskValidityClosure(value) => RecordRef::MaskValidityClosure(value.id()),
        Record::MaskRetraction(value) => RecordRef::MaskRetraction(value.id()),
        Record::ReplacementBoundary(value) => RecordRef::ReplacementBoundary(value.id()),
        Record::ReplacementBoundaryValidityClosure(value) => {
            RecordRef::ReplacementBoundaryValidityClosure(value.id())
        }
        Record::ReplacementBoundaryRetraction(value) => {
            RecordRef::ReplacementBoundaryRetraction(value.id())
        }
        Record::ArchiveTransition(value) => RecordRef::ArchiveTransition(value.id()),
        Record::Event(value) => RecordRef::Event(value.id()),
        Record::EventMask(value) => RecordRef::EventMask(value.id()),
        Record::EventSpanClosure(value) => RecordRef::EventSpanClosure(value.id()),
        Record::EventRetraction(value) => RecordRef::EventRetraction(value.id()),
        Record::EventMaskRetraction(value) => RecordRef::EventMaskRetraction(value.id()),
        Record::EventRelation(value) => RecordRef::EventRelation(value.id()),
        Record::EventRelationRetraction(value) => RecordRef::EventRelationRetraction(value.id()),
        Record::Source(value) => RecordRef::Source(value.id()),
        Record::Evidence(value) => RecordRef::Evidence(value.id()),
        Record::EvidenceRetraction(value) => RecordRef::EvidenceRetraction(value.id()),
        Record::Provenance(value) => RecordRef::Provenance(value.id()),
        Record::ProvenanceRetraction(value) => RecordRef::ProvenanceRetraction(value.id()),
        Record::EntityRetirement(value) => {
            RecordRef::EntityRetirement(value.entity_retirement_id())
        }
        Record::PerspectiveRetirement(value) => {
            RecordRef::PerspectiveRetirement(value.perspective_retirement_id())
        }
        Record::TransferLineage(value) => RecordRef::TransferLineage(value.id()),
        _ => return None,
    })
}

fn record_revision(record: &Record) -> Option<Revision> {
    Some(match record {
        Record::HistorySpaceDefinition(_) => return None,
        Record::Entity(value) => value.created_revision(),
        Record::EntityRetirement(value) => value.created_revision(),
        Record::PerspectiveDefinitionRevision(value) => value.recorded_revision(),
        Record::PerspectiveRetirement(value) => value.created_revision(),
        Record::LayerDefinition(value) => value.created_revision().revision(),
        Record::LayerSchemaSnapshot(value) => value.revision().revision(),
        Record::EntityTypeDefinition(value) => value.created_revision(),
        Record::PredicateDefinition(value) => value.created_revision(),
        Record::EventKindDefinition(value) => value.created_revision(),
        Record::MigrationPlan(_)
        | Record::MigrationRun(_)
        | Record::MigrationStepCommitIdentity(_) => {
            return None;
        }
        Record::Assertion(value) => value.created_revision(),
        Record::AssertionValidityClosure(value) => value.created_revision(),
        Record::AssertionRetraction(value) => value.created_revision(),
        Record::Mask(value) => value.created_revision(),
        Record::MaskValidityClosure(value) => value.created_revision(),
        Record::MaskRetraction(value) => value.created_revision(),
        Record::ReplacementBoundary(value) => value.created_revision(),
        Record::ReplacementBoundaryValidityClosure(value) => value.created_revision(),
        Record::ReplacementBoundaryRetraction(value) => value.created_revision(),
        Record::ArchiveTransition(value) => value.created_revision(),
        Record::Event(value) => value.created_revision(),
        Record::EventMask(value) => value.created_revision(),
        Record::EventSpanClosure(value) => value.created_revision(),
        Record::EventRetraction(value) => value.created_revision(),
        Record::EventMaskRetraction(value) => value.created_revision(),
        Record::EventRelation(value) => value.created_revision(),
        Record::EventRelationRetraction(value) => value.created_revision(),
        Record::Source(value) => value.created_revision(),
        Record::Evidence(value) => value.created_revision(),
        Record::Provenance(value) => value.created_revision(),
        Record::EvidenceRetraction(value) => value.created_revision(),
        Record::ProvenanceRetraction(value) => value.created_revision(),
        Record::TransferLineage(value) => value.created_revision(),
    })
}

fn encode_manifest(
    output: &mut Vec<u8>,
    manifest: &LogicalExportManifest,
) -> Result<(), LogicalExportError> {
    output.extend_from_slice(&manifest.database_id.to_bytes());
    push_u64(output, manifest.snapshot_revision.value());
    push_u64(output, manifest.from_revision.value());
    push_u64(output, manifest.through_revision.value());
    push_count(output, manifest.selected_history_spaces.len())?;
    for id in &manifest.selected_history_spaces {
        output.extend_from_slice(&id.to_bytes());
    }
    push_count(output, manifest.visible_history_spaces.len())?;
    for space in &manifest.visible_history_spaces {
        output.extend_from_slice(&space.id.to_bytes());
        push_u64(output, space.visible_through.value());
        push_u8(output, u8::from(space.selected));
    }
    push_count(output, manifest.selected_record_kinds.len())?;
    for kind in &manifest.selected_record_kinds {
        push_u32(output, kind.number());
    }
    push_count(output, manifest.classes.len())?;
    for class in &manifest.classes {
        push_u32(output, class.kind.number());
        push_u8(output, u8::from(class.selected));
        push_u64(output, class.exported_records);
    }
    push_count(output, manifest.omitted_storage_classes.len())?;
    for class in &manifest.omitted_storage_classes {
        push_u8(output, *class as u8);
    }
    Ok(())
}

fn decode_manifest(cursor: &mut Cursor<'_>) -> Result<LogicalExportManifest, LogicalExportError> {
    let database_id = DatabaseId::try_from_bytes(cursor.array_16()?)
        .map_err(|_| LogicalExportError::InvalidEncoding)?;
    let snapshot_revision = cursor.revision()?;
    let from_revision = cursor.revision()?;
    let through_revision = cursor.revision()?;
    let history_space_count = cursor.count(LOGICAL_EXPORT_MAX_SPACES)?;
    let mut selected_history_spaces = Vec::new();
    selected_history_spaces
        .try_reserve_exact(history_space_count)
        .map_err(|_| LogicalExportError::AllocationFailed)?;
    for _ in 0..history_space_count {
        selected_history_spaces.push(
            HistorySpaceId::try_from_bytes(cursor.array_16()?)
                .map_err(|_| LogicalExportError::InvalidEncoding)?,
        );
    }
    let visible_space_count = cursor.count(LOGICAL_EXPORT_MAX_SPACES)?;
    let mut visible_history_spaces = Vec::new();
    visible_history_spaces
        .try_reserve_exact(visible_space_count)
        .map_err(|_| LogicalExportError::AllocationFailed)?;
    for _ in 0..visible_space_count {
        let id = HistorySpaceId::try_from_bytes(cursor.array_16()?)
            .map_err(|_| LogicalExportError::InvalidEncoding)?;
        let visible_through = cursor.revision()?;
        let selected = match cursor.u8()? {
            0 => false,
            1 => true,
            _ => return Err(LogicalExportError::InvalidEncoding),
        };
        visible_history_spaces.push(LogicalExportHistorySpace {
            id,
            visible_through,
            selected,
        });
    }
    let selected_kind_count = cursor.count(RecordKind::ALL.len())?;
    let mut selected_record_kinds = Vec::new();
    selected_record_kinds
        .try_reserve_exact(selected_kind_count)
        .map_err(|_| LogicalExportError::AllocationFailed)?;
    for _ in 0..selected_kind_count {
        selected_record_kinds.push(record_kind_from_number(cursor.u32()?)?);
    }
    let class_count = cursor.count(RecordKind::ALL.len())?;
    if class_count != RecordKind::ALL.len() {
        return Err(LogicalExportError::InvalidEncoding);
    }
    let mut classes = Vec::new();
    classes
        .try_reserve_exact(class_count)
        .map_err(|_| LogicalExportError::AllocationFailed)?;
    for _ in 0..class_count {
        let kind = record_kind_from_number(cursor.u32()?)?;
        let selected = match cursor.u8()? {
            0 => false,
            1 => true,
            _ => return Err(LogicalExportError::InvalidEncoding),
        };
        classes.push(LogicalExportClassSummary {
            kind,
            selected,
            exported_records: cursor.u64()?,
        });
    }
    let omitted_storage_count = cursor.count(LogicalExportStorageClass::ALL.len())?;
    let mut omitted_storage_classes = Vec::new();
    omitted_storage_classes
        .try_reserve_exact(omitted_storage_count)
        .map_err(|_| LogicalExportError::AllocationFailed)?;
    for _ in 0..omitted_storage_count {
        omitted_storage_classes.push(storage_class_from_tag(cursor.u8()?)?);
    }
    Ok(LogicalExportManifest {
        database_id,
        snapshot_revision,
        from_revision,
        through_revision,
        selected_history_spaces,
        visible_history_spaces,
        selected_record_kinds,
        classes,
        omitted_storage_classes,
    })
}

fn validate_export(export: &LogicalExport) -> Result<(), LogicalExportError> {
    let manifest = &export.manifest;
    if manifest.from_revision > manifest.through_revision
        || manifest.through_revision > manifest.snapshot_revision
        || manifest.selected_history_spaces.is_empty()
        || manifest.selected_history_spaces.len() > LOGICAL_EXPORT_MAX_SPACES
        || manifest.selected_record_kinds.is_empty()
        || manifest.classes.len() != RecordKind::ALL.len()
        || manifest.visible_history_spaces.is_empty()
        || manifest.omitted_storage_classes != LogicalExportStorageClass::ALL
        || export.records.len() > LOGICAL_EXPORT_MAX_RECORDS
        || is_not_strictly_sorted(manifest.selected_history_spaces.iter().copied())
        || is_not_strictly_sorted(manifest.selected_record_kinds.iter().copied())
        || manifest
            .selected_record_kinds
            .iter()
            .any(|kind| is_revisionless_kind(*kind))
        || !manifest
            .selected_record_kinds
            .contains(&RecordKind::HistorySpaceDefinition)
        || manifest
            .visible_history_spaces
            .windows(2)
            .any(|pair| matches!(pair, [left, right] if left.id >= right.id))
    {
        return Err(LogicalExportError::InvalidEncoding);
    }
    let visible_selected = manifest
        .visible_history_spaces
        .iter()
        .filter(|space| space.selected)
        .map(|space| space.id)
        .collect::<Vec<_>>();
    if visible_selected != manifest.selected_history_spaces
        || manifest
            .visible_history_spaces
            .iter()
            .any(|space| space.visible_through > manifest.through_revision)
    {
        return Err(LogicalExportError::InvalidEncoding);
    }
    validate_history_space_dependencies(manifest, &export.records)?;
    let mut actual_counts = BTreeMap::<RecordKind, u64>::new();
    let mut previous_key: Option<(Revision, u32, Vec<u8>)> = None;
    let mut frame_bytes = 0usize;
    for entry in &export.records {
        let value = entry.record.record();
        let kind = RecordKind::of(value);
        if !manifest.selected_record_kinds.contains(&kind) {
            return Err(LogicalExportError::InvalidEncoding);
        }
        let revision = record_revision(value);
        match entry.inclusion {
            LogicalExportInclusion::InScope
                if revision.is_none_or(|revision| {
                    revision < manifest.from_revision || revision > manifest.through_revision
                }) =>
            {
                return Err(LogicalExportError::InvalidEncoding);
            }
            LogicalExportInclusion::HistorySpaceDependency => {
                let Record::HistorySpaceDefinition(definition) = value else {
                    return Err(LogicalExportError::InvalidEncoding);
                };
                if !manifest
                    .visible_history_spaces
                    .iter()
                    .any(|space| space.id == definition.history_space_id())
                {
                    return Err(LogicalExportError::InvalidEncoding);
                }
            }
            _ => {}
        }
        let frame = encode_decoded_record(&entry.record).map_err(LogicalExportError::Record)?;
        frame_bytes = frame_bytes
            .checked_add(frame.len())
            .ok_or(LogicalExportError::ResourceLimit)?;
        if frame_bytes > LOGICAL_EXPORT_MAX_BYTES {
            return Err(LogicalExportError::ResourceLimit);
        }
        let key = (revision.unwrap_or(Revision::GENESIS), kind.number(), frame);
        if previous_key
            .as_ref()
            .is_some_and(|previous| previous >= &key)
        {
            return Err(LogicalExportError::NonCanonicalEncoding);
        }
        previous_key = Some(key);
        let count = actual_counts.entry(kind).or_default();
        *count = count
            .checked_add(1)
            .ok_or(LogicalExportError::ResourceLimit)?;
    }
    for (index, kind) in RecordKind::ALL.into_iter().enumerate() {
        let class = manifest
            .classes
            .get(index)
            .ok_or(LogicalExportError::InvalidEncoding)?;
        let selected = manifest.selected_record_kinds.contains(&kind);
        if class.kind != kind || class.selected != selected {
            return Err(LogicalExportError::InvalidEncoding);
        }
        let actual_exported = actual_counts.get(&kind).copied().unwrap_or(0);
        if class.exported_records != actual_exported || (!selected && class.exported_records != 0) {
            return Err(LogicalExportError::InvalidEncoding);
        }
    }
    Ok(())
}

fn validate_history_space_dependencies(
    manifest: &LogicalExportManifest,
    records: &[LogicalExportEntry],
) -> Result<(), LogicalExportError> {
    let mut definitions = BTreeMap::new();
    for entry in records {
        match (entry.inclusion, entry.record.record()) {
            (
                LogicalExportInclusion::HistorySpaceDependency,
                Record::HistorySpaceDefinition(definition),
            ) => {
                if definitions
                    .insert(definition.history_space_id(), *definition)
                    .is_some()
                {
                    return Err(LogicalExportError::InvalidEncoding);
                }
            }
            (LogicalExportInclusion::HistorySpaceDependency, _) => {
                return Err(LogicalExportError::InvalidEncoding);
            }
            (LogicalExportInclusion::InScope, Record::HistorySpaceDefinition(_)) => {
                return Err(LogicalExportError::InvalidEncoding);
            }
            _ => {}
        }
    }

    let mut expected = BTreeMap::<HistorySpaceId, (Revision, bool)>::new();
    for selected_id in &manifest.selected_history_spaces {
        let mut current_id = *selected_id;
        let mut cutoff = manifest.through_revision;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(current_id) {
                return Err(LogicalExportError::InvalidEncoding);
            }
            let definition = definitions
                .get(&current_id)
                .ok_or(LogicalExportError::InvalidEncoding)?;
            expected
                .entry(current_id)
                .and_modify(|entry| {
                    entry.0 = entry.0.max(cutoff);
                    entry.1 |= current_id == *selected_id;
                })
                .or_insert((cutoff, current_id == *selected_id));
            if let Some(parent_id) = definition.parent_history_space_id() {
                cutoff = cutoff.min(definition.base_revision());
                current_id = parent_id;
            } else {
                break;
            }
        }
    }
    let expected = expected
        .into_iter()
        .map(
            |(id, (visible_through, selected))| LogicalExportHistorySpace {
                id,
                visible_through,
                selected,
            },
        )
        .collect::<Vec<_>>();
    if definitions.len() != manifest.visible_history_spaces.len()
        || expected != manifest.visible_history_spaces
    {
        return Err(LogicalExportError::InvalidEncoding);
    }
    Ok(())
}

fn is_not_strictly_sorted<T: Ord + Copy>(values: impl Iterator<Item = T>) -> bool {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|prior| prior >= value) {
            return true;
        }
        previous = Some(value);
    }
    false
}

fn record_kind_from_number(number: u32) -> Result<RecordKind, LogicalExportError> {
    RecordKind::ALL
        .into_iter()
        .find(|kind| kind.number() == number)
        .ok_or(LogicalExportError::InvalidEncoding)
}

fn storage_class_from_tag(tag: u8) -> Result<LogicalExportStorageClass, LogicalExportError> {
    LogicalExportStorageClass::ALL
        .into_iter()
        .find(|class| *class as u8 == tag)
        .ok_or(LogicalExportError::InvalidEncoding)
}

fn push_count(output: &mut Vec<u8>, count: usize) -> Result<(), LogicalExportError> {
    push_u32(
        output,
        u32::try_from(count).map_err(|_| LogicalExportError::ResourceLimit)?,
    );
    Ok(())
}

fn push_bytes_u32(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), LogicalExportError> {
    push_u32(
        output,
        u32::try_from(bytes.len()).map_err(|_| LogicalExportError::ResourceLimit)?,
    );
    output
        .try_reserve(bytes.len())
        .map_err(|_| LogicalExportError::AllocationFailed)?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn push_u8(output: &mut Vec<u8>, value: u8) {
    output.push(value);
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LogicalExportError> {
    let end = offset
        .checked_add(8)
        .ok_or(LogicalExportError::InvalidEncoding)?;
    let raw: [u8; 8] = bytes
        .get(offset..end)
        .ok_or(LogicalExportError::InvalidEncoding)?
        .try_into()
        .map_err(|_| LogicalExportError::InvalidEncoding)?;
    Ok(u64::from_be_bytes(raw))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], LogicalExportError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(LogicalExportError::InvalidEncoding)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(LogicalExportError::InvalidEncoding)?;
        self.offset = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, LogicalExportError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(LogicalExportError::InvalidEncoding)
    }

    fn u32(&mut self) -> Result<u32, LogicalExportError> {
        let value: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| LogicalExportError::InvalidEncoding)?;
        Ok(u32::from_be_bytes(value))
    }

    fn u64(&mut self) -> Result<u64, LogicalExportError> {
        let value: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| LogicalExportError::InvalidEncoding)?;
        Ok(u64::from_be_bytes(value))
    }

    fn revision(&mut self) -> Result<Revision, LogicalExportError> {
        Revision::new(self.u64()?).map_err(|_| LogicalExportError::InvalidEncoding)
    }

    fn array_16(&mut self) -> Result<[u8; 16], LogicalExportError> {
        self.take(16)?
            .try_into()
            .map_err(|_| LogicalExportError::InvalidEncoding)
    }

    fn count(&mut self, limit: usize) -> Result<usize, LogicalExportError> {
        let count = usize::try_from(self.u32()?).map_err(|_| LogicalExportError::ResourceLimit)?;
        if count > limit {
            return Err(LogicalExportError::ResourceLimit);
        }
        Ok(count)
    }

    fn bytes_u32(&mut self, limit: usize) -> Result<&'a [u8], LogicalExportError> {
        let length = self.count(limit)?;
        self.take(length)
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LogicalExportError, LogicalExportInclusion, LogicalExportManager, LogicalExportScope,
        LogicalExportStorageClass,
    };
    use crate::{
        CompactionManager, DatabaseLayout, HistorySegmentStore, LogicalImportDestinationInventory,
        LogicalImportError, LogicalImportIdMapping, LogicalImportIdentity, LogicalImportManager,
        LogicalImportPlan, ManifestSegmentKind, ManifestSegmentReference, ManifestSnapshot,
        ManifestStore, RecoveryManager, SecurityPolicyHistoryStore, WalPrepareLog,
    };
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use worlddb_core::{
        AuthorizationMode, Capability, CapabilityGrant, CapabilityRule, DatabaseId, DomainId,
        Entity, EntityId, EntityRetirement, EntityRetirementId, EntityTypeId, GrantEffect,
        HistorySpaceDefinition, HistorySpaceId, LayerDefinition, LayerId, Lifecycle, OperationId,
        PerspectiveDefinitionRevision, PerspectiveId, PolicyRuleId, PolicyScope, PolicySubject,
        Principal, PrincipalId, Record, RecordKind, RecordRef, Revision, SchemaRevision,
        SecurityEpoch, SecurityPolicyHistory, SecurityPolicySnapshot, SecurityPolicyVersion,
        Symbol,
    };

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestArea(PathBuf);

    impl TestArea {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("worlddb-m7-11-{}-{sequence}", std::process::id()));
            fs::create_dir(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        fn layout(&self) -> Result<DatabaseLayout, String> {
            DatabaseLayout::create(self.0.join("database")).map_err(|error| error.to_string())
        }
    }

    impl Drop for TestArea {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn id<T: DomainId>(tail: u8) -> Result<T, String> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = tail;
        T::try_from_bytes(bytes).map_err(|error| error.to_string())
    }

    fn policy(history_space: Option<HistorySpaceId>) -> Result<SecurityPolicyHistory, String> {
        let principal = id::<PrincipalId>(1)?;
        let rules = if history_space.is_some() {
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(2)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::DataExport, GrantEffect::Allow),
                PolicyScope::new(history_space, None, None, None, None),
            )]
        } else {
            vec![CapabilityRule::new(
                id::<PolicyRuleId>(2)?,
                PolicySubject::Principal(principal),
                CapabilityGrant::new(Capability::DataExport, GrantEffect::Allow),
                PolicyScope::project(),
            )]
        };
        let snapshot =
            SecurityPolicySnapshot::new(vec![Principal::new(principal)], vec![], vec![], rules)
                .map_err(|error| error.to_string())?;
        SecurityPolicyHistory::new(
            Revision::GENESIS,
            vec![SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                snapshot,
            )],
        )
        .map_err(|error| error.to_string())
    }

    fn policy_view(
        history: &SecurityPolicyHistory,
    ) -> Result<worlddb_core::SecurityPolicyView<'_>, String> {
        history
            .select(
                AuthorizationMode::Now,
                id::<PrincipalId>(1)?,
                Revision::GENESIS,
            )
            .map_err(|error| error.to_string())
    }

    fn install_history(layout: &DatabaseLayout) -> Result<HistorySpaceId, String> {
        let history_space = id::<HistorySpaceId>(3)?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let records = [
            Record::HistorySpaceDefinition(
                HistorySpaceDefinition::new(history_space, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
            ),
            Record::HistorySpaceDefinition(
                HistorySpaceDefinition::new(id::<HistorySpaceId>(14)?, None, Revision::GENESIS)
                    .map_err(|error| error.to_string())?,
            ),
            Record::Entity(Entity::new(
                id::<EntityId>(4)?,
                id::<EntityTypeId>(5)?,
                Revision::FIRST_COMMIT,
            )),
            Record::EntityRetirement(EntityRetirement::new(
                id::<EntityRetirementId>(15)?,
                id::<EntityId>(4)?,
                Revision::FIRST_COMMIT,
            )),
            Record::PerspectiveDefinitionRevision(
                PerspectiveDefinitionRevision::new(
                    id::<PerspectiveId>(16)?,
                    Some(String::from("research")),
                    Some(String::from("Export fixture perspective")),
                    Revision::FIRST_COMMIT,
                )
                .map_err(|error| error.to_string())?,
            ),
            Record::LayerDefinition(LayerDefinition::new(
                id::<LayerId>(17)?,
                Symbol::new("base").map_err(|error| error.to_string())?,
                Some(String::from("Export fixture base layer")),
                0,
                Lifecycle::Active,
                SchemaRevision::from_published_revision(Revision::FIRST_COMMIT),
            )),
        ];
        let history_receipt = HistorySegmentStore::new(layout.clone())
            .write_segment(&lock, &records)
            .map_err(|error| error.to_string())?;
        let history_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            history_receipt.id(),
            history_receipt.content_digest(),
            Revision::FIRST_COMMIT,
        );
        let policy_version = SecurityPolicyVersion::new(
            Revision::GENESIS,
            SecurityEpoch::INITIAL,
            SecurityPolicySnapshot::default(),
        );
        let policy_receipt = SecurityPolicyHistoryStore::new(layout.clone())
            .write_version(&lock, &policy_version, None, None)
            .map_err(|error| error.to_string())?;
        let policy_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            policy_receipt.id(),
            policy_receipt.content_digest(),
            Revision::GENESIS,
        );
        let references = vec![history_reference, policy_reference];
        ManifestSnapshot::new(Revision::FIRST_COMMIT, references.clone())
            .map_err(|error| error.to_string())?;
        let operation_id = id::<OperationId>(6)?;
        WalPrepareLog::new(layout)
            .commit_manifest_snapshot(&lock, operation_id, references, &[])
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(layout.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        Ok(history_space)
    }

    fn install_two_segment_history(layout: &DatabaseLayout) -> Result<(), String> {
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let store = HistorySegmentStore::new(layout.clone());
        let first = Record::HistorySpaceDefinition(
            HistorySpaceDefinition::new(id::<HistorySpaceId>(7)?, None, Revision::GENESIS)
                .map_err(|error| error.to_string())?,
        );
        let first = store
            .write_segment(&lock, &[first])
            .map_err(|error| error.to_string())?;
        let second = Record::Entity(Entity::new(
            id::<EntityId>(8)?,
            id::<EntityTypeId>(9)?,
            Revision::FIRST_COMMIT,
        ));
        let second = store
            .write_segment(&lock, &[second])
            .map_err(|error| error.to_string())?;
        let first_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            first.id(),
            first.content_digest(),
            Revision::GENESIS,
        );
        let second_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::History,
            second.id(),
            second.content_digest(),
            Revision::FIRST_COMMIT,
        );
        let policy_receipt = SecurityPolicyHistoryStore::new(layout.clone())
            .write_version(
                &lock,
                &SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    SecurityPolicySnapshot::default(),
                ),
                None,
                None,
            )
            .map_err(|error| error.to_string())?;
        let policy_reference = ManifestSegmentReference::new(
            ManifestSegmentKind::SecurityPolicy,
            policy_receipt.id(),
            policy_receipt.content_digest(),
            Revision::GENESIS,
        );
        let references = vec![first_reference, second_reference, policy_reference];
        ManifestSnapshot::new(Revision::FIRST_COMMIT, references.clone())
            .map_err(|error| error.to_string())?;
        WalPrepareLog::new(layout)
            .commit_manifest_snapshot(&lock, id::<OperationId>(10)?, references, &[])
            .map_err(|error| error.to_string())?;
        RecoveryManager::new(layout.clone())
            .recover(&lock)
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn scope(
        history_space: HistorySpaceId,
        through_revision: Revision,
    ) -> Result<LogicalExportScope, LogicalExportError> {
        LogicalExportScope::new(
            Revision::GENESIS,
            through_revision,
            vec![history_space],
            vec![RecordKind::Entity, RecordKind::HistorySpaceDefinition],
        )
    }

    #[test]
    fn logical_export_pins_snapshot_and_roundtrips_scope_and_omissions() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let export = LogicalExportManager::new(layout)
            .export(
                scope(history_space, Revision::GENESIS).map_err(|error| error.to_string())?,
                policy_view(&permissions)?,
            )
            .map_err(|error| error.to_string())?;

        assert_eq!(export.records().len(), 1);
        let entry = export
            .records()
            .first()
            .ok_or_else(|| "export dependency record is missing".to_owned())?;
        assert_eq!(
            entry.inclusion(),
            LogicalExportInclusion::HistorySpaceDependency
        );
        assert_eq!(
            export.manifest().selected_history_spaces(),
            &[history_space]
        );
        assert_eq!(
            export.manifest().omitted_storage_classes(),
            &LogicalExportStorageClass::ALL
        );
        assert_eq!(export.manifest().classes().len(), RecordKind::ALL.len());
        let entity = export
            .manifest()
            .classes()
            .iter()
            .find(|class| class.kind() == RecordKind::Entity)
            .ok_or_else(|| "Entity omission row is missing".to_owned())?;
        assert!(entity.selected());
        assert_eq!(entity.exported_records(), 0);
        let event = export
            .manifest()
            .classes()
            .iter()
            .find(|class| class.kind() == RecordKind::Event)
            .ok_or_else(|| "Event omission row is missing".to_owned())?;
        assert!(!event.selected());

        let bytes = export.encode().map_err(|error| error.to_string())?;
        let decoded = super::LogicalExport::decode(&bytes).map_err(|error| error.to_string())?;
        assert_eq!(decoded.manifest(), export.manifest());
        assert_eq!(decoded.records().len(), export.records().len());
        assert_eq!(decoded.encode().map_err(|error| error.to_string())?, bytes);
        Ok(())
    }

    #[test]
    fn logical_export_rejects_missing_history_space_dependency() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let mut export = LogicalExportManager::new(layout)
            .export(
                scope(history_space, Revision::GENESIS).map_err(|error| error.to_string())?,
                policy_view(&permissions)?,
            )
            .map_err(|error| error.to_string())?;
        export
            .records
            .retain(|entry| !matches!(entry.record.record(), Record::HistorySpaceDefinition(_)));
        let class = export
            .manifest
            .classes
            .iter_mut()
            .find(|class| class.kind() == RecordKind::HistorySpaceDefinition)
            .ok_or_else(|| "HistorySpaceDefinition summary is missing".to_owned())?;
        class.exported_records = 0;
        assert!(matches!(
            export.encode(),
            Err(LogicalExportError::InvalidEncoding)
        ));
        Ok(())
    }

    #[test]
    fn logical_export_includes_selected_schema_metadata_and_lifecycle_classes() -> Result<(), String>
    {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let mut kinds = vec![
            RecordKind::Entity,
            RecordKind::EntityRetirement,
            RecordKind::HistorySpaceDefinition,
            RecordKind::LayerDefinition,
            RecordKind::PerspectiveDefinitionRevision,
        ];
        kinds.sort_unstable();
        let export = LogicalExportManager::new(layout)
            .export(
                LogicalExportScope::new(
                    Revision::FIRST_COMMIT,
                    Revision::FIRST_COMMIT,
                    vec![history_space],
                    kinds.clone(),
                )
                .map_err(|error| error.to_string())?,
                policy_view(&permissions)?,
            )
            .map_err(|error| error.to_string())?;

        assert_eq!(export.manifest().selected_record_kinds(), kinds);
        assert!(
            export
                .records()
                .iter()
                .any(|entry| matches!(entry.record().record(), Record::Entity(_)))
        );
        assert!(
            export
                .records()
                .iter()
                .any(|entry| matches!(entry.record().record(), Record::EntityRetirement(_)))
        );
        assert!(
            export
                .records()
                .iter()
                .any(|entry| matches!(entry.record().record(), Record::LayerDefinition(_)))
        );
        assert!(export.records().iter().any(|entry| matches!(
            entry.record().record(),
            Record::PerspectiveDefinitionRevision(_)
        )));
        assert!(export.records().iter().any(|entry| {
            matches!(
                entry.inclusion(),
                LogicalExportInclusion::HistorySpaceDependency
            )
        }));

        let bytes = export.encode().map_err(|error| error.to_string())?;
        let decoded = super::LogicalExport::decode(&bytes).map_err(|error| error.to_string())?;
        assert_eq!(decoded.manifest(), export.manifest());
        assert_eq!(decoded.records().len(), export.records().len());
        assert_eq!(decoded.encode().map_err(|error| error.to_string())?, bytes);
        Ok(())
    }

    #[test]
    fn logical_export_uses_current_data_export_permissions_for_each_scope() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let scoped_permissions = policy(Some(history_space))?;
        let error = LogicalExportManager::new(layout.clone())
            .export(
                scope(history_space, Revision::GENESIS).map_err(|error| error.to_string())?,
                policy_view(&scoped_permissions)?,
            )
            .err()
            .ok_or_else(|| {
                "project-wide class was allowed by a HistorySpace-only grant".to_owned()
            })?;
        assert!(matches!(
            error,
            LogicalExportError::ProjectAuthorizationDenied
        ));

        let denied = SecurityPolicyHistory::new(
            Revision::GENESIS,
            vec![SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                SecurityPolicySnapshot::new(
                    vec![Principal::new(id::<PrincipalId>(1)?)],
                    vec![],
                    vec![],
                    vec![],
                )
                .map_err(|error| error.to_string())?,
            )],
        )
        .map_err(|error| error.to_string())?;
        let error = LogicalExportManager::new(layout)
            .export(
                scope(history_space, Revision::GENESIS).map_err(|error| error.to_string())?,
                policy_view(&denied)?,
            )
            .err()
            .ok_or_else(|| "missing DataExport permission was accepted".to_owned())?;
        assert!(matches!(error, LogicalExportError::AuthorizationDenied));
        Ok(())
    }

    #[test]
    fn historical_data_export_allow_does_not_override_current_denial() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let principal = id::<PrincipalId>(1)?;
        let historical_rule = CapabilityRule::new(
            id::<PolicyRuleId>(2)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(Capability::DataExport, GrantEffect::Allow),
            PolicyScope::project(),
        );
        let current_rule = CapabilityRule::new(
            id::<PolicyRuleId>(3)?,
            PolicySubject::Principal(principal),
            CapabilityGrant::new(
                Capability::SecurityPermissionHistoryRead,
                GrantEffect::Allow,
            ),
            PolicyScope::project(),
        );
        let historical_snapshot = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![],
            vec![],
            vec![historical_rule],
        )
        .map_err(|error| error.to_string())?;
        let current_snapshot = SecurityPolicySnapshot::new(
            vec![Principal::new(principal)],
            vec![],
            vec![],
            vec![current_rule],
        )
        .map_err(|error| error.to_string())?;
        let current_epoch = SecurityEpoch::INITIAL
            .next()
            .map_err(|error| error.to_string())?;
        let permissions = SecurityPolicyHistory::new(
            Revision::FIRST_COMMIT,
            vec![
                SecurityPolicyVersion::new(
                    Revision::GENESIS,
                    SecurityEpoch::INITIAL,
                    historical_snapshot,
                ),
                SecurityPolicyVersion::new(Revision::FIRST_COMMIT, current_epoch, current_snapshot),
            ],
        )
        .map_err(|error| error.to_string())?;
        let historical_view = permissions
            .select(
                AuthorizationMode::AtRevision(Revision::GENESIS),
                principal,
                Revision::FIRST_COMMIT,
            )
            .map_err(|error| error.to_string())?;
        let error = LogicalExportManager::new(layout)
            .export(
                scope(history_space, Revision::GENESIS).map_err(|error| error.to_string())?,
                historical_view,
            )
            .err()
            .ok_or_else(|| "historical DataExport grant bypassed the current policy".to_owned())?;
        assert!(matches!(error, LogicalExportError::AuthorizationDenied));
        Ok(())
    }

    #[test]
    fn scope_rejects_record_classes_without_durable_revision_coordinates() -> Result<(), String> {
        assert!(matches!(
            LogicalExportScope::new(
                Revision::GENESIS,
                Revision::FIRST_COMMIT,
                vec![id::<HistorySpaceId>(3)?],
                vec![RecordKind::HistorySpaceDefinition, RecordKind::MigrationRun],
            ),
            Err(LogicalExportError::UnsupportedRecordKind)
        ));
        Ok(())
    }

    #[test]
    fn logical_export_digest_detects_changed_content() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let export = LogicalExportManager::new(layout)
            .export(
                scope(history_space, Revision::GENESIS).map_err(|error| error.to_string())?,
                policy_view(&permissions)?,
            )
            .map_err(|error| error.to_string())?;
        let mut bytes = export.encode().map_err(|error| error.to_string())?;
        let content_index = bytes
            .len()
            .checked_sub(33)
            .ok_or_else(|| "encoded export was too short".to_owned())?;
        let content_byte = bytes
            .get_mut(content_index)
            .ok_or_else(|| "encoded export content byte was missing".to_owned())?;
        *content_byte ^= 0x40;
        assert!(matches!(
            super::LogicalExport::decode(&bytes),
            Err(LogicalExportError::DigestMismatch)
        ));
        Ok(())
    }

    #[test]
    fn logical_export_rejects_missing_omission_class_manifest_row() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let mut export = LogicalExportManager::new(layout)
            .export(
                scope(history_space, Revision::GENESIS).map_err(|error| error.to_string())?,
                policy_view(&permissions)?,
            )
            .map_err(|error| error.to_string())?;
        export.manifest.classes.pop();
        assert!(matches!(
            export.encode(),
            Err(LogicalExportError::InvalidEncoding)
        ));
        Ok(())
    }

    #[test]
    fn durable_export_pin_keeps_replaced_history_until_export_releases_it() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        install_two_segment_history(&layout)?;
        let source_lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let manifest = ManifestStore::new(layout.clone())
            .read_current()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "current manifest is missing".to_owned())?;
        let history_references = manifest
            .segments()
            .iter()
            .copied()
            .filter(|reference| reference.kind() == ManifestSegmentKind::History)
            .collect::<Vec<_>>();
        let manager = CompactionManager::new(layout.clone());
        let (local_pins, durable_pin) = manager
            .pin_export_snapshot(&source_lock, manifest.segments())
            .map_err(|error| error.to_string())?;
        assert_eq!(local_pins.len(), manifest.segments().len());
        drop(local_pins);
        drop(source_lock);

        let compaction_lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let result = CompactionManager::new(layout.clone())
            .compact_history(&compaction_lock)
            .map_err(|error| error.to_string())?;
        assert!(result.compacted());
        assert_eq!(result.reclamation().retained_by_pin(), history_references);

        drop(durable_pin);
        let reclaimed = manager
            .reclaim_retired(&compaction_lock, &history_references)
            .map_err(|error| error.to_string())?;
        assert_eq!(reclaimed.reclaimed(), history_references);
        Ok(())
    }

    #[test]
    fn logical_import_requires_and_replays_explicit_collision_remaps() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let import_scope = LogicalExportScope::new(
            Revision::GENESIS,
            Revision::FIRST_COMMIT,
            vec![history_space],
            vec![
                RecordKind::Entity,
                RecordKind::EntityRetirement,
                RecordKind::HistorySpaceDefinition,
            ],
        )
        .map_err(|error| error.to_string())?;
        let source_bytes = LogicalExportManager::new(layout)
            .export(import_scope, policy_view(&permissions)?)
            .and_then(|export| export.encode())
            .map_err(|error| error.to_string())?;
        let destination_id = id::<DatabaseId>(41)?;
        let occupied = LogicalImportDestinationInventory::new(
            destination_id,
            [
                LogicalImportIdentity::Entity(id::<EntityId>(4)?),
                LogicalImportIdentity::EntityType(id::<EntityTypeId>(5)?),
                LogicalImportIdentity::Record(RecordRef::EntityRetirement(
                    id::<EntityRetirementId>(15)?,
                )),
            ],
        )
        .map_err(|error| error.to_string())?;

        let unplanned = LogicalImportPlan::new(&source_bytes, destination_id, Vec::new())
            .and_then(|plan| plan.encode())
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            LogicalImportManager::prepare(&source_bytes, &unplanned, &occupied),
            Err(LogicalImportError::IdentityCollision(
                LogicalImportIdentity::Entity(_)
            ))
        ));

        let mappings = vec![
            LogicalImportIdMapping::new(
                LogicalImportIdentity::Entity(id::<EntityId>(4)?),
                LogicalImportIdentity::Entity(id::<EntityId>(42)?),
            )
            .map_err(|error| error.to_string())?,
            LogicalImportIdMapping::new(
                LogicalImportIdentity::Record(RecordRef::EntityRetirement(
                    id::<EntityRetirementId>(15)?,
                )),
                LogicalImportIdentity::Record(RecordRef::EntityRetirement(
                    id::<EntityRetirementId>(43)?,
                )),
            )
            .map_err(|error| error.to_string())?,
        ];
        let plan = LogicalImportPlan::new(&source_bytes, destination_id, mappings)
            .map_err(|error| error.to_string())?;
        let plan_bytes = plan.encode().map_err(|error| error.to_string())?;
        assert_eq!(
            LogicalImportPlan::decode(&plan_bytes)
                .and_then(|decoded| decoded.encode())
                .map_err(|error| error.to_string())?,
            plan_bytes
        );

        let first = LogicalImportManager::prepare(&source_bytes, &plan_bytes, &occupied)
            .map_err(|error| error.to_string())?;
        let second = LogicalImportManager::prepare(&source_bytes, &plan_bytes, &occupied)
            .map_err(|error| error.to_string())?;
        assert_eq!(first.stream_fingerprint(), second.stream_fingerprint());
        assert_eq!(
            first.map_identity(LogicalImportIdentity::Entity(id::<EntityId>(4)?)),
            LogicalImportIdentity::Entity(id::<EntityId>(42)?)
        );
        assert_eq!(
            first.mapped_record_identity(&Record::EntityRetirement(EntityRetirement::new(
                id::<EntityRetirementId>(15)?,
                id::<EntityId>(4)?,
                Revision::FIRST_COMMIT,
            ))),
            Some(RecordRef::EntityRetirement(id::<EntityRetirementId>(43)?))
        );
        assert_eq!(first.records().len(), 3);
        Ok(())
    }

    #[test]
    fn logical_import_rejects_unlisted_or_occupied_remap_targets() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let source_bytes = LogicalExportManager::new(layout)
            .export(
                LogicalExportScope::new(
                    Revision::GENESIS,
                    Revision::FIRST_COMMIT,
                    vec![history_space],
                    vec![RecordKind::Entity, RecordKind::HistorySpaceDefinition],
                )
                .map_err(|error| error.to_string())?,
                policy_view(&permissions)?,
            )
            .and_then(|export| export.encode())
            .map_err(|error| error.to_string())?;
        let destination_id = id::<DatabaseId>(51)?;
        let occupied = LogicalImportDestinationInventory::new(
            destination_id,
            [
                LogicalImportIdentity::Entity(id::<EntityId>(4)?),
                LogicalImportIdentity::Entity(id::<EntityId>(52)?),
                LogicalImportIdentity::EntityType(id::<EntityTypeId>(5)?),
            ],
        )
        .map_err(|error| error.to_string())?;
        let mapping = LogicalImportIdMapping::new(
            LogicalImportIdentity::Entity(id::<EntityId>(4)?),
            LogicalImportIdentity::Entity(id::<EntityId>(52)?),
        )
        .map_err(|error| error.to_string())?;
        let unlisted_mapping = LogicalImportIdMapping::new(
            LogicalImportIdentity::Entity(id::<EntityId>(53)?),
            LogicalImportIdentity::Entity(id::<EntityId>(54)?),
        )
        .map_err(|error| error.to_string())?;
        let unlisted_plan =
            LogicalImportPlan::new(&source_bytes, destination_id, vec![unlisted_mapping])
                .and_then(|plan| plan.encode())
                .map_err(|error| error.to_string())?;
        assert!(matches!(
            LogicalImportManager::prepare(&source_bytes, &unlisted_plan, &occupied),
            Err(LogicalImportError::MappingSourceNotInExport(
                LogicalImportIdentity::Entity(_)
            ))
        ));
        let plan_bytes = LogicalImportPlan::new(&source_bytes, destination_id, vec![mapping])
            .and_then(|plan| plan.encode())
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            LogicalImportManager::prepare(&source_bytes, &plan_bytes, &occupied),
            Err(LogicalImportError::RemapTargetOccupied(
                LogicalImportIdentity::Entity(_)
            ))
        ));
        Ok(())
    }

    #[test]
    fn logical_import_rejects_missing_schema_references() -> Result<(), String> {
        let area = TestArea::create()?;
        let layout = area.layout()?;
        let history_space = install_history(&layout)?;
        let permissions = policy(None)?;
        let source_bytes = LogicalExportManager::new(layout)
            .export(
                LogicalExportScope::new(
                    Revision::GENESIS,
                    Revision::FIRST_COMMIT,
                    vec![history_space],
                    vec![RecordKind::Entity, RecordKind::HistorySpaceDefinition],
                )
                .map_err(|error| error.to_string())?,
                policy_view(&permissions)?,
            )
            .and_then(|export| export.encode())
            .map_err(|error| error.to_string())?;
        let destination_id = id::<DatabaseId>(61)?;
        let destination = LogicalImportDestinationInventory::new(destination_id, [])
            .map_err(|error| error.to_string())?;
        let plan_bytes = LogicalImportPlan::new(&source_bytes, destination_id, Vec::new())
            .and_then(|plan| plan.encode())
            .map_err(|error| error.to_string())?;

        assert!(matches!(
            LogicalImportManager::prepare(&source_bytes, &plan_bytes, &destination),
            Err(LogicalImportError::MissingReference)
        ));
        Ok(())
    }
}
