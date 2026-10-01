//! Immutable persistence for complete security-policy and audit-policy snapshots.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use worlddb_core::{
    AuditRetentionError, AuditRetentionPolicy, Capability, CapabilityGrant, CapabilityRule,
    DecodeResource, DecoderLimits, DomainId, EvidenceRelationship, FieldSelector, GrantEffect,
    IdValidationError, PolicyBundle, PolicyBundleError, PolicyEventRelationKind, PolicyRuleId,
    PolicyScope, PolicySubject, Principal, PrincipalId, PrincipalState, ProvenanceRelationship,
    RecordCodecError, Revision, RevisionError, RoleAssignment, RoleAssignmentId, RoleDefinition,
    RoleDefinitionError, RoleId, SecurityEpoch, SecurityPolicyChange, SecurityPolicyError,
    SecurityPolicyHistory, SecurityPolicyHistoryError, SecurityPolicyRecord,
    SecurityPolicyRecordError, SecurityPolicyRecordId, SecurityPolicySnapshot,
    SecurityPolicyVersion, Symbol, decode_record_ref, encode_record_ref,
};

use crate::segment::{
    ContentDigest, ENVELOPE_BYTES, SegmentError, SegmentIoCheckpoint, create_new_segment_file,
    decode_canonical_frames, decode_segment_envelope, digest, encode_canonical_content,
    generate_segment_id, segment_path, write_segment_file_with_checkpoint,
};
use crate::{DatabaseLayout, ManifestSegmentKind, ManifestSegmentReference, SegmentId, WriterLock};

const SECURITY_FORMAT_MAJOR: u16 = 1;
const SECURITY_FORMAT_MINOR: u16 = 0;
const MAX_SECURITY_FILE_BYTES: usize =
    ENVELOPE_BYTES + 8 + 16 + DecoderLimits::DEFAULT.max_frame_bytes;
const MAX_SEGMENT_ID_ATTEMPTS: usize = 16;

/// Why an immutable security-policy history segment could not be written or read.
#[derive(Debug)]
pub enum SecurityPolicyStorageError {
    /// The writer lock belongs to a different database.
    ForeignWriterLock,
    /// Recovery verification has not authorized ordinary database writes.
    RecoveryRequired,
    /// A shared segment-envelope or canonical-index check failed.
    Segment(SegmentError),
    /// A filesystem operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// The security segment path is not a regular entry below the database root.
    PathOutsideDatabase,
    /// Security history must contain exactly one segment for each revision.
    DuplicateRevision,
    /// The same immutable security-segment identity was supplied more than once.
    DuplicateSegmentId,
    /// One segment must contain exactly one security-policy frame.
    InvalidFrameCount,
    /// The security frame is truncated, malformed, or has trailing bytes.
    InvalidSecurityFrame,
    /// The security frame uses an unsupported major/minor version.
    UnsupportedVersion { major: u16, minor: u16 },
    /// A closed enum uses an unknown stable tag.
    InvalidTag { kind: &'static str, tag: u8 },
    /// A symbol is not valid UTF-8 or a canonical ASCII schema symbol.
    InvalidSymbol,
    /// A collection, string, or frame exceeds the default 1.0 decoder budget.
    ResourceLimit {
        resource: DecodeResource,
        limit: usize,
        actual: usize,
    },
    /// A bounded fallible memory reservation failed.
    AllocationFailed,
    /// An identity byte sequence failed registered UUID validation.
    InvalidIdentity(IdValidationError),
    /// A stored revision uses the reserved or invalid numeric value.
    InvalidRevision(RevisionError),
    /// A decoded snapshot contains duplicate identities or invalid references.
    InvalidPolicy(SecurityPolicyError),
    /// A decoded role uses an invalid symbol.
    InvalidRole(RoleDefinitionError),
    /// A decoded capability bundle contains a duplicate pair.
    InvalidBundle(PolicyBundleError),
    /// A decoded security-policy record is empty or otherwise invalid.
    InvalidPolicyRecord(SecurityPolicyRecordError),
    /// A decoded audit-retention policy is invalid under Core's policy bounds.
    InvalidAuditRetention(AuditRetentionError),
    /// A policy record does not identify the same revision and epoch as its snapshot.
    PolicyRecordVersionMismatch,
    /// A policy epoch advanced without a same-revision policy-change record.
    MissingPolicyRecord,
    /// A policy-change record exists although the security epoch did not advance.
    UnexpectedPolicyRecord,
    /// A persistent policy snapshot changed without advancing the security epoch.
    SnapshotChangedWithoutEpochAdvance,
    /// The append-only policy-record identity was reused in the supplied history.
    DuplicatePolicyRecordId,
    /// A policy-scope record coordinate failed its typed RecordRef codec.
    RecordReference(RecordCodecError),
    /// The supplied or decoded versions do not form a complete valid history.
    History(SecurityPolicyHistoryError),
}

impl fmt::Display for SecurityPolicyStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWriterLock => {
                formatter.write_str("writer lock belongs to another database")
            }
            Self::RecoveryRequired => formatter.write_str(
                "security-policy writes are blocked until recovery verifies a clean state",
            ),
            Self::Segment(error) => write!(formatter, "security segment envelope failed: {error}"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::PathOutsideDatabase => {
                formatter.write_str("security segment is not a regular entry inside the database")
            }
            Self::DuplicateRevision => formatter
                .write_str("security history contains more than one snapshot for a revision"),
            Self::DuplicateSegmentId => {
                formatter.write_str("security history repeats a segment identity")
            }
            Self::InvalidFrameCount => {
                formatter.write_str("security segment must contain exactly one frame")
            }
            Self::InvalidSecurityFrame => {
                formatter.write_str("security-policy frame is malformed or non-canonical")
            }
            Self::UnsupportedVersion { major, minor } => {
                write!(
                    formatter,
                    "unsupported security-policy format {major}.{minor}"
                )
            }
            Self::InvalidTag { kind, tag } => write!(formatter, "unknown {kind} tag {tag}"),
            Self::InvalidSymbol => formatter.write_str("stored role symbol is invalid"),
            Self::ResourceLimit {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "security-policy resource {resource:?} exceeds limit {limit} with {actual}"
            ),
            Self::AllocationFailed => formatter.write_str("security-policy allocation failed"),
            Self::InvalidIdentity(error) => {
                write!(formatter, "stored identity is invalid: {error}")
            }
            Self::InvalidRevision(error) => {
                write!(formatter, "stored revision is invalid: {error}")
            }
            Self::InvalidPolicy(error) => {
                write!(formatter, "stored policy snapshot is invalid: {error}")
            }
            Self::InvalidRole(error) => write!(formatter, "stored role is invalid: {error}"),
            Self::InvalidBundle(error) => {
                write!(formatter, "stored capability bundle is invalid: {error}")
            }
            Self::InvalidPolicyRecord(error) => {
                write!(
                    formatter,
                    "stored security-policy record is invalid: {error}"
                )
            }
            Self::InvalidAuditRetention(error) => {
                write!(
                    formatter,
                    "stored audit-retention policy is invalid: {error}"
                )
            }
            Self::RecordReference(error) => {
                write!(formatter, "stored policy scope is invalid: {error}")
            }
            Self::PolicyRecordVersionMismatch => formatter
                .write_str("security-policy record revision or epoch differs from its snapshot"),
            Self::MissingPolicyRecord => {
                formatter.write_str("security epoch advanced without a policy-change record")
            }
            Self::UnexpectedPolicyRecord => {
                formatter.write_str("policy-change record exists without a security-epoch advance")
            }
            Self::SnapshotChangedWithoutEpochAdvance => formatter.write_str(
                "persistent policy snapshot changed without advancing its security epoch",
            ),
            Self::DuplicatePolicyRecordId => {
                formatter.write_str("security-policy record identity is reused in history")
            }
            Self::History(error) => write!(formatter, "security history is incomplete: {error}"),
        }
    }
}

impl std::error::Error for SecurityPolicyStorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Segment(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::InvalidIdentity(error) => Some(error),
            Self::InvalidRevision(error) => Some(error),
            Self::InvalidPolicy(error) => Some(error),
            Self::InvalidRole(error) => Some(error),
            Self::InvalidBundle(error) => Some(error),
            Self::InvalidPolicyRecord(error) => Some(error),
            Self::InvalidAuditRetention(error) => Some(error),
            Self::RecordReference(error) => Some(error),
            Self::History(error) => Some(error),
            Self::ForeignWriterLock
            | Self::RecoveryRequired
            | Self::PathOutsideDatabase
            | Self::DuplicateRevision
            | Self::DuplicateSegmentId
            | Self::InvalidFrameCount
            | Self::InvalidSecurityFrame
            | Self::UnsupportedVersion { .. }
            | Self::InvalidTag { .. }
            | Self::InvalidSymbol
            | Self::PolicyRecordVersionMismatch
            | Self::MissingPolicyRecord
            | Self::UnexpectedPolicyRecord
            | Self::SnapshotChangedWithoutEpochAdvance
            | Self::DuplicatePolicyRecordId
            | Self::ResourceLimit { .. }
            | Self::AllocationFailed => None,
        }
    }
}

/// One complete, decoded immutable security and audit-policy snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityPolicySegment {
    id: SegmentId,
    content_digest: ContentDigest,
    version: SecurityPolicyVersion,
    policy_record: Option<SecurityPolicyRecord>,
    audit_retention: Option<AuditRetentionPolicy>,
}

impl SecurityPolicySegment {
    /// Random file identity from the shared segment envelope.
    #[must_use]
    pub const fn id(&self) -> SegmentId {
        self.id
    }

    /// Digest over the canonical security-policy frame.
    #[must_use]
    pub const fn content_digest(&self) -> ContentDigest {
        self.content_digest
    }

    /// Complete principal, role, capability, and epoch projection.
    #[must_use]
    pub const fn version(&self) -> &SecurityPolicyVersion {
        &self.version
    }

    /// Typed append-only policy changes committed at this revision, if any.
    #[must_use]
    pub const fn policy_record(&self) -> Option<&SecurityPolicyRecord> {
        self.policy_record.as_ref()
    }

    /// Audit-retention configuration active at this shared revision, if enabled.
    #[must_use]
    pub const fn audit_retention(&self) -> Option<AuditRetentionPolicy> {
        self.audit_retention
    }
}

/// Receipt for writing a new immutable security-policy history segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityPolicySegmentReceipt {
    id: SegmentId,
    content_digest: ContentDigest,
    revision: Revision,
    file_bytes: u64,
}

impl SecurityPolicySegmentReceipt {
    /// Random file identity to include in the next manifest generation.
    #[must_use]
    pub const fn id(self) -> SegmentId {
        self.id
    }

    /// Digest over the canonical security-policy frame.
    #[must_use]
    pub const fn content_digest(self) -> ContentDigest {
        self.content_digest
    }

    /// Shared revision represented by the segment.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Exact envelope plus canonical payload byte length.
    #[must_use]
    pub const fn file_bytes(self) -> u64 {
        self.file_bytes
    }
}

/// Complete policy history and audit-retention configuration through one commit.
#[derive(Clone, Debug)]
pub struct SecurityPolicyHistorySnapshot {
    policy: SecurityPolicyHistory,
    policy_records: Vec<Option<SecurityPolicyRecord>>,
    audit_retention: Vec<Option<AuditRetentionPolicy>>,
}

impl SecurityPolicyHistorySnapshot {
    /// Complete fail-closed policy projection.
    #[must_use]
    pub const fn policy(&self) -> &SecurityPolicyHistory {
        &self.policy
    }

    /// Append-only policy-change record at a committed shared revision, if any.
    pub fn policy_record_at(
        &self,
        revision: Revision,
    ) -> Result<Option<&SecurityPolicyRecord>, SecurityPolicyHistoryError> {
        self.policy.version_at(revision)?;
        let index = usize::try_from(revision.value())
            .map_err(|_| SecurityPolicyHistoryError::MissingInitialSnapshot)?;
        self.policy_records
            .get(index)
            .map(Option::as_ref)
            .ok_or(SecurityPolicyHistoryError::MissingInitialSnapshot)
    }

    /// Retention configuration at a committed shared revision.
    pub fn audit_retention_at(
        &self,
        revision: Revision,
    ) -> Result<Option<AuditRetentionPolicy>, SecurityPolicyHistoryError> {
        self.policy.version_at(revision)?;
        let index = usize::try_from(revision.value())
            .map_err(|_| SecurityPolicyHistoryError::MissingInitialSnapshot)?;
        self.audit_retention
            .get(index)
            .copied()
            .ok_or(SecurityPolicyHistoryError::MissingInitialSnapshot)
    }
}

/// Reads and writes immutable policy snapshots in the shared segment envelope.
#[derive(Clone, Debug)]
pub struct SecurityPolicyHistoryStore {
    layout: DatabaseLayout,
}

impl SecurityPolicyHistoryStore {
    /// Binds the store to one validated database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Writes one complete policy and audit-configuration projection for a shared revision.
    ///
    /// Callers carry forward the prior policy snapshot and retention setting on data-only
    /// commits, then reference this segment from the same manifest as the domain commit.
    pub fn write_version(
        &self,
        writer_lock: &WriterLock,
        version: &SecurityPolicyVersion,
        policy_record: Option<&SecurityPolicyRecord>,
        audit_retention: Option<AuditRetentionPolicy>,
    ) -> Result<SecurityPolicySegmentReceipt, SecurityPolicyStorageError> {
        self.write_version_at(writer_lock, version, policy_record, audit_retention, false)
    }

    /// Writes a complete policy snapshot to same-volume staging. The WAL
    /// manifest transaction names it before recovery publishes the immutable
    /// segment at its final path.
    pub fn stage_version(
        &self,
        writer_lock: &WriterLock,
        version: &SecurityPolicyVersion,
        policy_record: Option<&SecurityPolicyRecord>,
        audit_retention: Option<AuditRetentionPolicy>,
    ) -> Result<SecurityPolicySegmentReceipt, SecurityPolicyStorageError> {
        self.write_version_at(writer_lock, version, policy_record, audit_retention, true)
    }

    fn write_version_at(
        &self,
        writer_lock: &WriterLock,
        version: &SecurityPolicyVersion,
        policy_record: Option<&SecurityPolicyRecord>,
        audit_retention: Option<AuditRetentionPolicy>,
        staged: bool,
    ) -> Result<SecurityPolicySegmentReceipt, SecurityPolicyStorageError> {
        self.write_version_at_with_checkpoint(
            writer_lock,
            version,
            policy_record,
            audit_retention,
            staged,
            |_| {},
        )
    }

    fn write_version_at_with_checkpoint(
        &self,
        writer_lock: &WriterLock,
        version: &SecurityPolicyVersion,
        policy_record: Option<&SecurityPolicyRecord>,
        audit_retention: Option<AuditRetentionPolicy>,
        staged: bool,
        mut checkpoint: impl FnMut(SegmentIoCheckpoint),
    ) -> Result<SecurityPolicySegmentReceipt, SecurityPolicyStorageError> {
        if !writer_lock.belongs_to_database_root(self.layout.root()) {
            return Err(SecurityPolicyStorageError::ForeignWriterLock);
        }
        if !writer_lock.require_write_access() {
            return Err(SecurityPolicyStorageError::RecoveryRequired);
        }
        let directory = if staged {
            self.validate_staging_directory()?
        } else {
            self.validate_security_segments_directory()?
        };
        validate_policy_record_version(version, policy_record)?;
        let frame = encode_security_version(version, policy_record, audit_retention)?;
        let canonical =
            encode_canonical_content(vec![frame]).map_err(SecurityPolicyStorageError::Segment)?;
        let content_digest = digest(&canonical);

        for _ in 0..MAX_SEGMENT_ID_ATTEMPTS {
            let id = generate_segment_id().map_err(SecurityPolicyStorageError::Segment)?;
            let path = if staged {
                security_staging_path(&directory, id)
            } else {
                segment_path(&directory, id)
            };
            let mut file = match create_new_segment_file(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(SecurityPolicyStorageError::Io {
                        operation: "create immutable security-policy segment",
                        source,
                    });
                }
            };
            checkpoint(SegmentIoCheckpoint::StageFileCreated);
            let result = write_segment_file_with_checkpoint(
                &mut file,
                id,
                content_digest,
                &canonical,
                &mut checkpoint,
            )
            .map_err(SecurityPolicyStorageError::Segment);
            drop(file);
            if let Err(error) = result {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            if staged {
                crate::manifest::sync_directory(&directory).map_err(|source| {
                    SecurityPolicyStorageError::Io {
                        operation: "sync security-policy staging directory",
                        source,
                    }
                })?;
                checkpoint(SegmentIoCheckpoint::StagingDirectorySynced);
            }
            let file_bytes = u64::try_from(ENVELOPE_BYTES + canonical.len()).map_err(|_| {
                SecurityPolicyStorageError::ResourceLimit {
                    resource: DecodeResource::BatchBytes,
                    limit: MAX_SECURITY_FILE_BYTES,
                    actual: usize::MAX,
                }
            })?;
            return Ok(SecurityPolicySegmentReceipt {
                id,
                content_digest,
                revision: version.revision(),
                file_bytes,
            });
        }
        Err(SecurityPolicyStorageError::Segment(
            SegmentError::SegmentIdCollisionExhausted,
        ))
    }

    /// Reads and validates one security-policy segment by its stable ID.
    pub fn read_version(
        &self,
        id: SegmentId,
    ) -> Result<SecurityPolicySegment, SecurityPolicyStorageError> {
        let directory = self.validate_security_segments_directory()?;
        let path = segment_path(&directory, id);
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| SecurityPolicyStorageError::Io {
                operation: "inspect security-policy segment",
                source,
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SecurityPolicyStorageError::PathOutsideDatabase);
        }
        let canonical_path =
            fs::canonicalize(&path).map_err(|source| SecurityPolicyStorageError::Io {
                operation: "resolve security-policy segment",
                source,
            })?;
        if !canonical_path.starts_with(&directory) {
            return Err(SecurityPolicyStorageError::PathOutsideDatabase);
        }
        let file_size = metadata.len();
        let file_size_usize = usize::try_from(file_size).unwrap_or(usize::MAX);
        if file_size_usize > MAX_SECURITY_FILE_BYTES {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: MAX_SECURITY_FILE_BYTES,
                actual: file_size_usize,
            });
        }
        let file = File::open(&path).map_err(|source| SecurityPolicyStorageError::Io {
            operation: "open security-policy segment",
            source,
        })?;
        let opened_metadata = file
            .metadata()
            .map_err(|source| SecurityPolicyStorageError::Io {
                operation: "inspect opened security-policy segment",
                source,
            })?;
        if !opened_metadata.is_file() || opened_metadata.len() != file_size {
            return Err(SecurityPolicyStorageError::PathOutsideDatabase);
        }
        let expected_len =
            usize::try_from(file_size).map_err(|_| SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: MAX_SECURITY_FILE_BYTES,
                actual: usize::MAX,
            })?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(expected_len)
            .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
        file.take(MAX_SECURITY_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| SecurityPolicyStorageError::Io {
                operation: "read security-policy segment",
                source,
            })?;
        if bytes.len() > MAX_SECURITY_FILE_BYTES || bytes.len() != expected_len {
            return Err(SecurityPolicyStorageError::Segment(
                SegmentError::InvalidEnvelope,
            ));
        }
        decode_security_segment(id, &bytes)
    }

    pub(crate) fn validate_manifest_reference(
        &self,
        reference: ManifestSegmentReference,
    ) -> Result<(), SecurityPolicyStorageError> {
        if reference.kind() != ManifestSegmentKind::SecurityPolicy {
            return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
        }
        let segment = self.read_version(reference.id())?;
        if segment.content_digest() != reference.content_digest()
            || segment.version().revision() != reference.through_revision()
        {
            return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
        }
        Ok(())
    }

    pub(crate) fn read_verified_reference_bytes(
        &self,
        reference: ManifestSegmentReference,
        allow_staged: bool,
    ) -> Result<(Vec<u8>, bool), SecurityPolicyStorageError> {
        if reference.kind() != ManifestSegmentKind::SecurityPolicy {
            return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
        }
        let final_directory = self.validate_security_segments_directory()?;
        let final_path = segment_path(&final_directory, reference.id());
        match read_verified_security_file(&final_path, &final_directory, reference) {
            Ok((bytes, _)) => return Ok((bytes, false)),
            Err(final_error) if !allow_staged => return Err(final_error),
            Err(_) => {}
        }
        let staging_directory = self.validate_staging_directory()?;
        let staged_path = security_staging_path(&staging_directory, reference.id());
        let (bytes, _) = read_verified_security_file(&staged_path, &staging_directory, reference)?;
        Ok((bytes, true))
    }

    pub(crate) fn read_verified_reference(
        &self,
        reference: ManifestSegmentReference,
        allow_staged: bool,
    ) -> Result<SecurityPolicySegment, SecurityPolicyStorageError> {
        let (bytes, _) = self.read_verified_reference_bytes(reference, allow_staged)?;
        decode_security_segment(reference.id(), &bytes)
    }

    pub(crate) fn validate_staged_reference(
        &self,
        reference: ManifestSegmentReference,
    ) -> Result<(), SecurityPolicyStorageError> {
        if reference.kind() != ManifestSegmentKind::SecurityPolicy {
            return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
        }
        let directory = self.validate_staging_directory()?;
        read_and_validate_security_file(
            &security_staging_path(&directory, reference.id()),
            reference,
        )
    }

    pub(crate) fn materialize_staged_reference(
        &self,
        reference: ManifestSegmentReference,
    ) -> Result<(), SecurityPolicyStorageError> {
        let destination_directory = self.validate_security_segments_directory()?;
        let source_directory = self.validate_staging_directory()?;
        let destination = segment_path(&destination_directory, reference.id());
        match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(SecurityPolicyStorageError::PathOutsideDatabase);
                }
                self.validate_manifest_reference(reference)?;
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(SecurityPolicyStorageError::Io {
                    operation: "inspect security-policy publication target",
                    source,
                });
            }
        }
        let source = security_staging_path(&source_directory, reference.id());
        read_and_validate_security_file(&source, reference)?;
        publish_staged_file(&source, &destination).map_err(|source| {
            SecurityPolicyStorageError::Io {
                operation: "publish staged immutable security-policy segment",
                source,
            }
        })?;
        crate::manifest::sync_directory(&destination_directory).map_err(|source| {
            SecurityPolicyStorageError::Io {
                operation: "sync security-policy segment directory after publication",
                source,
            }
        })?;
        crate::manifest::sync_directory(&source_directory).map_err(|source| {
            SecurityPolicyStorageError::Io {
                operation: "sync staging directory after security-policy publication",
                source,
            }
        })?;
        self.validate_manifest_reference(reference)
    }

    /// Reconstructs all policy and audit-configuration revisions; any gap fails closed.
    pub fn load_history(
        &self,
        committed_revision: Revision,
        segment_ids: &[SegmentId],
    ) -> Result<SecurityPolicyHistorySnapshot, SecurityPolicyStorageError> {
        if segment_ids.len() > DecoderLimits::DEFAULT.max_array_items {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::ArrayItems,
                limit: DecoderLimits::DEFAULT.max_array_items,
                actual: segment_ids.len(),
            });
        }
        let mut unique_ids = BTreeSet::new();
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(segment_ids.len())
            .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
        for id in segment_ids {
            if !unique_ids.insert(*id) {
                return Err(SecurityPolicyStorageError::DuplicateSegmentId);
            }
            segments.push(self.read_version(*id)?);
        }
        segments.sort_by_key(|segment| segment.version.revision());
        let mut unique_record_ids = BTreeSet::new();
        for segment in &segments {
            if let Some(record) = segment.policy_record.as_ref() {
                if !unique_record_ids.insert(record.id()) {
                    return Err(SecurityPolicyStorageError::DuplicatePolicyRecordId);
                }
            }
        }
        for pair in segments.windows(2) {
            let previous = pair
                .first()
                .ok_or(SecurityPolicyStorageError::InvalidSecurityFrame)?;
            let current = pair
                .get(1)
                .ok_or(SecurityPolicyStorageError::InvalidSecurityFrame)?;
            if current.version.epoch() == previous.version.epoch() {
                if current.version.snapshot() != previous.version.snapshot() {
                    return Err(SecurityPolicyStorageError::SnapshotChangedWithoutEpochAdvance);
                }
                if current.policy_record.is_some() {
                    return Err(SecurityPolicyStorageError::UnexpectedPolicyRecord);
                }
            } else if current.policy_record.is_none() {
                return Err(SecurityPolicyStorageError::MissingPolicyRecord);
            }
        }
        let mut versions = Vec::new();
        let mut policy_records = Vec::new();
        let mut retention = Vec::new();
        versions
            .try_reserve_exact(segments.len())
            .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
        policy_records
            .try_reserve_exact(segments.len())
            .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
        retention
            .try_reserve_exact(segments.len())
            .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
        let mut previous_revision = None;
        for segment in segments {
            let revision = segment.version.revision();
            if previous_revision == Some(revision) {
                return Err(SecurityPolicyStorageError::DuplicateRevision);
            }
            previous_revision = Some(revision);
            versions.push(segment.version);
            policy_records.push(segment.policy_record);
            retention.push(segment.audit_retention);
        }
        let policy = SecurityPolicyHistory::new(committed_revision, versions)
            .map_err(SecurityPolicyStorageError::History)?;
        if retention.len() != policy.versions().len() {
            return Err(SecurityPolicyStorageError::History(
                SecurityPolicyHistoryError::IncompleteRevisionCoverage,
            ));
        }
        Ok(SecurityPolicyHistorySnapshot {
            policy,
            policy_records,
            audit_retention: retention,
        })
    }

    fn validate_security_segments_directory(&self) -> Result<PathBuf, SecurityPolicyStorageError> {
        let path = self.layout.security_segments_directory();
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| SecurityPolicyStorageError::Io {
                operation: "inspect security-policy segment directory",
                source,
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SecurityPolicyStorageError::PathOutsideDatabase);
        }
        let canonical =
            fs::canonicalize(&path).map_err(|source| SecurityPolicyStorageError::Io {
                operation: "resolve security-policy segment directory",
                source,
            })?;
        if !canonical.starts_with(self.layout.root()) {
            return Err(SecurityPolicyStorageError::PathOutsideDatabase);
        }
        Ok(canonical)
    }

    fn validate_staging_directory(&self) -> Result<PathBuf, SecurityPolicyStorageError> {
        let path = self.layout.staging_directory();
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| SecurityPolicyStorageError::Io {
                operation: "inspect security-policy staging directory",
                source,
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SecurityPolicyStorageError::PathOutsideDatabase);
        }
        let canonical =
            fs::canonicalize(&path).map_err(|source| SecurityPolicyStorageError::Io {
                operation: "resolve security-policy staging directory",
                source,
            })?;
        if !canonical.starts_with(self.layout.root()) {
            return Err(SecurityPolicyStorageError::PathOutsideDatabase);
        }
        Ok(canonical)
    }
}

pub(crate) fn security_staging_path(directory: &Path, id: SegmentId) -> PathBuf {
    directory.join(format!("security-segment-{id}.stage"))
}

fn read_and_validate_security_file(
    path: &Path,
    reference: ManifestSegmentReference,
) -> Result<(), SecurityPolicyStorageError> {
    let parent = path
        .parent()
        .ok_or(SecurityPolicyStorageError::PathOutsideDatabase)?;
    read_verified_security_file(path, parent, reference).map(|_| ())
}

fn read_verified_security_file(
    path: &Path,
    expected_directory: &Path,
    reference: ManifestSegmentReference,
) -> Result<(Vec<u8>, SecurityPolicySegment), SecurityPolicyStorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| SecurityPolicyStorageError::Io {
        operation: "inspect staged security-policy segment",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SecurityPolicyStorageError::PathOutsideDatabase);
    }
    let actual = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if actual > MAX_SECURITY_FILE_BYTES {
        return Err(SecurityPolicyStorageError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: MAX_SECURITY_FILE_BYTES,
            actual,
        });
    }
    let canonical = fs::canonicalize(path).map_err(|source| SecurityPolicyStorageError::Io {
        operation: "resolve staged security-policy segment",
        source,
    })?;
    let canonical_parent =
        fs::canonicalize(expected_directory).map_err(|source| SecurityPolicyStorageError::Io {
            operation: "resolve staged security-policy directory",
            source,
        })?;
    if canonical.parent() != Some(canonical_parent.as_path()) {
        return Err(SecurityPolicyStorageError::PathOutsideDatabase);
    }
    let file = File::open(path).map_err(|source| SecurityPolicyStorageError::Io {
        operation: "read staged security-policy segment",
        source,
    })?;
    let opened = file
        .metadata()
        .map_err(|source| SecurityPolicyStorageError::Io {
            operation: "inspect opened staged security-policy segment",
            source,
        })?;
    if !opened.is_file() || opened.len() != metadata.len() {
        return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(actual)
        .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
    file.take(u64::try_from(MAX_SECURITY_FILE_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| SecurityPolicyStorageError::Io {
            operation: "read staged security-policy segment bytes",
            source,
        })?;
    if bytes.len() != actual || bytes.len() > MAX_SECURITY_FILE_BYTES {
        return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
    }
    let decoded = decode_security_segment(reference.id(), &bytes)?;
    if decoded.content_digest() != reference.content_digest()
        || decoded.version().revision() != reference.through_revision()
    {
        return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
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

fn decode_security_segment(
    requested_id: SegmentId,
    bytes: &[u8],
) -> Result<SecurityPolicySegment, SecurityPolicyStorageError> {
    let (content_digest, canonical) = decode_segment_envelope(requested_id, bytes)
        .map_err(SecurityPolicyStorageError::Segment)?;
    let frames = decode_canonical_frames(canonical).map_err(SecurityPolicyStorageError::Segment)?;
    if frames.len() != 1 {
        return Err(SecurityPolicyStorageError::InvalidFrameCount);
    }
    let frame = frames
        .first()
        .copied()
        .ok_or(SecurityPolicyStorageError::InvalidFrameCount)?;
    let (version, policy_record, audit_retention) = decode_security_version(frame)?;
    validate_policy_record_version(&version, policy_record.as_ref())?;
    Ok(SecurityPolicySegment {
        id: requested_id,
        content_digest,
        version,
        policy_record,
        audit_retention,
    })
}

fn encode_security_version(
    version: &SecurityPolicyVersion,
    policy_record: Option<&SecurityPolicyRecord>,
    audit_retention: Option<AuditRetentionPolicy>,
) -> Result<Vec<u8>, SecurityPolicyStorageError> {
    let mut writer = ByteWriter::new();
    writer.put_u16(SECURITY_FORMAT_MAJOR)?;
    writer.put_u16(SECURITY_FORMAT_MINOR)?;
    writer.put_u64(version.revision().value())?;
    writer.put_u64(version.epoch().value())?;
    match audit_retention {
        None => writer.put_u8(0)?,
        Some(policy) => {
            writer.put_u8(1)?;
            writer.put_u64(policy.minimum_retention_millis())?;
            writer.put_u64(u64::try_from(policy.maximum_records()).map_err(|_| {
                SecurityPolicyStorageError::ResourceLimit {
                    resource: DecodeResource::ArrayItems,
                    limit: DecoderLimits::DEFAULT.max_array_items,
                    actual: usize::MAX,
                }
            })?)?;
        }
    }
    match policy_record {
        None => writer.put_u8(0)?,
        Some(record) => {
            writer.put_u8(1)?;
            encode_policy_record(&mut writer, record)?;
        }
    }
    encode_snapshot(&mut writer, version.snapshot())?;
    Ok(writer.finish())
}

fn decode_security_version(
    bytes: &[u8],
) -> Result<
    (
        SecurityPolicyVersion,
        Option<SecurityPolicyRecord>,
        Option<AuditRetentionPolicy>,
    ),
    SecurityPolicyStorageError,
> {
    if bytes.len() > DecoderLimits::DEFAULT.max_frame_bytes {
        return Err(SecurityPolicyStorageError::ResourceLimit {
            resource: DecodeResource::BatchBytes,
            limit: DecoderLimits::DEFAULT.max_frame_bytes,
            actual: bytes.len(),
        });
    }
    let mut reader = ByteReader::new(bytes);
    let major = reader.read_u16()?;
    let minor = reader.read_u16()?;
    if major != SECURITY_FORMAT_MAJOR || minor != SECURITY_FORMAT_MINOR {
        return Err(SecurityPolicyStorageError::UnsupportedVersion { major, minor });
    }
    let revision = Revision::try_from(reader.read_u64()?)
        .map_err(SecurityPolicyStorageError::InvalidRevision)?;
    let epoch = SecurityEpoch::new(reader.read_u64()?);
    let audit_retention = match reader.read_u8()? {
        0 => None,
        1 => {
            let minimum_retention_millis = reader.read_u64()?;
            let maximum_records_u64 = reader.read_u64()?;
            let maximum_records = usize::try_from(maximum_records_u64).map_err(|_| {
                SecurityPolicyStorageError::ResourceLimit {
                    resource: DecodeResource::ArrayItems,
                    limit: DecoderLimits::DEFAULT.max_array_items,
                    actual: usize::MAX,
                }
            })?;
            Some(
                AuditRetentionPolicy::new(minimum_retention_millis, maximum_records)
                    .map_err(SecurityPolicyStorageError::InvalidAuditRetention)?,
            )
        }
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "audit-retention option",
                tag,
            });
        }
    };
    let policy_record = match reader.read_u8()? {
        0 => None,
        1 => Some(decode_policy_record(&mut reader)?),
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "security-policy record option",
                tag,
            });
        }
    };
    let snapshot = decode_snapshot(&mut reader)?;
    reader.finish()?;
    Ok((
        SecurityPolicyVersion::new(revision, epoch, snapshot),
        policy_record,
        audit_retention,
    ))
}

fn validate_policy_record_version(
    version: &SecurityPolicyVersion,
    policy_record: Option<&SecurityPolicyRecord>,
) -> Result<(), SecurityPolicyStorageError> {
    if policy_record.is_some_and(|record| {
        record.recorded_revision() != version.revision()
            || record.security_epoch_after() != version.epoch()
    }) {
        return Err(SecurityPolicyStorageError::PolicyRecordVersionMismatch);
    }
    Ok(())
}

fn encode_policy_record(
    writer: &mut ByteWriter,
    record: &SecurityPolicyRecord,
) -> Result<(), SecurityPolicyStorageError> {
    writer.put_id(record.id())?;
    writer.put_u64(record.recorded_revision().value())?;
    writer.put_id(record.actor())?;
    writer.put_u64(record.security_epoch_after().value())?;
    writer.put_count(record.changes().len())?;
    for change in record.changes() {
        encode_policy_change(writer, change)?;
    }
    Ok(())
}

fn decode_policy_record(
    reader: &mut ByteReader<'_>,
) -> Result<SecurityPolicyRecord, SecurityPolicyStorageError> {
    let id = reader.read_id::<SecurityPolicyRecordId>()?;
    let recorded_revision = Revision::try_from(reader.read_u64()?)
        .map_err(SecurityPolicyStorageError::InvalidRevision)?;
    let actor = reader.read_id::<PrincipalId>()?;
    let security_epoch_after = SecurityEpoch::new(reader.read_u64()?);
    let count = reader.read_count()?;
    if count == 0 {
        return Err(SecurityPolicyStorageError::InvalidPolicyRecord(
            SecurityPolicyRecordError::EmptyChanges,
        ));
    }
    let mut changes = Vec::new();
    reserve(&mut changes, count)?;
    for _ in 0..count {
        changes.push(decode_policy_change(reader)?);
    }
    SecurityPolicyRecord::new(id, recorded_revision, actor, security_epoch_after, changes)
        .map_err(SecurityPolicyStorageError::InvalidPolicyRecord)
}

fn encode_policy_change(
    writer: &mut ByteWriter,
    change: &SecurityPolicyChange,
) -> Result<(), SecurityPolicyStorageError> {
    match change {
        SecurityPolicyChange::PrincipalRegistered { principal_id } => {
            writer.put_u8(0)?;
            writer.put_id(*principal_id)?;
        }
        SecurityPolicyChange::PrincipalStateChanged {
            principal_id,
            state,
        } => {
            writer.put_u8(1)?;
            writer.put_id(*principal_id)?;
            writer.put_u8(principal_state_tag(*state))?;
        }
        SecurityPolicyChange::RoleRegistered { role_id, symbol } => {
            writer.put_u8(2)?;
            writer.put_id(*role_id)?;
            writer.put_string(symbol.as_str())?;
        }
        SecurityPolicyChange::RoleRetired { role_id } => {
            writer.put_u8(3)?;
            writer.put_id(*role_id)?;
        }
        SecurityPolicyChange::RoleAssigned {
            assignment_id,
            principal_id,
            role_id,
            scope,
        } => {
            writer.put_u8(4)?;
            writer.put_id(*assignment_id)?;
            writer.put_id(*principal_id)?;
            writer.put_id(*role_id)?;
            encode_scope(writer, *scope)?;
        }
        SecurityPolicyChange::RoleAssignmentRevoked { assignment_id } => {
            writer.put_u8(5)?;
            writer.put_id(*assignment_id)?;
        }
        SecurityPolicyChange::CapabilityRuleAdded {
            rule_id,
            subject,
            capability,
            effect,
            scope,
        } => {
            writer.put_u8(6)?;
            writer.put_id(*rule_id)?;
            encode_subject(writer, *subject)?;
            writer.put_u8(capability_tag(*capability)?)?;
            writer.put_u8(effect_tag(*effect))?;
            encode_scope(writer, *scope)?;
        }
        SecurityPolicyChange::CapabilityRuleRevoked { rule_id } => {
            writer.put_u8(7)?;
            writer.put_id(*rule_id)?;
        }
    }
    Ok(())
}

fn decode_policy_change(
    reader: &mut ByteReader<'_>,
) -> Result<SecurityPolicyChange, SecurityPolicyStorageError> {
    Ok(match reader.read_u8()? {
        0 => SecurityPolicyChange::PrincipalRegistered {
            principal_id: reader.read_id()?,
        },
        1 => SecurityPolicyChange::PrincipalStateChanged {
            principal_id: reader.read_id()?,
            state: decode_principal_state(reader.read_u8()?)?,
        },
        2 => SecurityPolicyChange::RoleRegistered {
            role_id: reader.read_id()?,
            symbol: Symbol::new(reader.read_string()?)
                .map_err(|_| SecurityPolicyStorageError::InvalidSymbol)?,
        },
        3 => SecurityPolicyChange::RoleRetired {
            role_id: reader.read_id()?,
        },
        4 => SecurityPolicyChange::RoleAssigned {
            assignment_id: reader.read_id::<RoleAssignmentId>()?,
            principal_id: reader.read_id()?,
            role_id: reader.read_id()?,
            scope: decode_scope(reader)?,
        },
        5 => SecurityPolicyChange::RoleAssignmentRevoked {
            assignment_id: reader.read_id()?,
        },
        6 => SecurityPolicyChange::CapabilityRuleAdded {
            rule_id: reader.read_id::<PolicyRuleId>()?,
            subject: decode_subject(reader)?,
            capability: decode_capability(reader.read_u8()?)?,
            effect: decode_effect(reader.read_u8()?)?,
            scope: decode_scope(reader)?,
        },
        7 => SecurityPolicyChange::CapabilityRuleRevoked {
            rule_id: reader.read_id()?,
        },
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "security-policy change",
                tag,
            });
        }
    })
}

fn principal_state_tag(state: PrincipalState) -> u8 {
    match state {
        PrincipalState::Active => 0,
        PrincipalState::Disabled => 1,
        PrincipalState::Retired => 2,
    }
}

fn decode_principal_state(tag: u8) -> Result<PrincipalState, SecurityPolicyStorageError> {
    match tag {
        0 => Ok(PrincipalState::Active),
        1 => Ok(PrincipalState::Disabled),
        2 => Ok(PrincipalState::Retired),
        tag => Err(SecurityPolicyStorageError::InvalidTag {
            kind: "principal-state",
            tag,
        }),
    }
}

fn encode_snapshot(
    writer: &mut ByteWriter,
    snapshot: &SecurityPolicySnapshot,
) -> Result<(), SecurityPolicyStorageError> {
    let mut principals = Vec::new();
    principals
        .try_reserve_exact(snapshot.principals().len())
        .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
    principals.extend(snapshot.principals().iter().copied());
    principals.sort_by_key(|principal| principal.id());
    writer.put_count(principals.len())?;
    for principal in principals {
        writer.put_id(principal.id())?;
        writer.put_u8(principal_state_tag(principal.state()))?;
    }

    let mut roles = Vec::new();
    roles
        .try_reserve_exact(snapshot.roles().len())
        .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
    roles.extend(snapshot.roles().iter());
    roles.sort_by_key(|role| role.id());
    writer.put_count(roles.len())?;
    for role in roles {
        writer.put_id(role.id())?;
        writer.put_string(role.symbol())?;
        writer.put_count(role.bundle().len())?;
        for grant in role.bundle().iter() {
            writer.put_u8(capability_tag(grant.capability())?)?;
            writer.put_u8(effect_tag(grant.effect()))?;
        }
    }

    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(snapshot.assignments().len())
        .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
    assignments.extend(snapshot.assignments().iter().copied());
    assignments.sort_by_key(|assignment| assignment.id());
    writer.put_count(assignments.len())?;
    for assignment in assignments {
        writer.put_id(assignment.id())?;
        writer.put_id(assignment.principal())?;
        writer.put_id(assignment.role())?;
        encode_scope(writer, assignment.scope())?;
    }

    let mut rules = Vec::new();
    rules
        .try_reserve_exact(snapshot.rules().len())
        .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
    rules.extend(snapshot.rules().iter().copied());
    rules.sort_by_key(|rule| rule.id());
    writer.put_count(rules.len())?;
    for rule in rules {
        writer.put_id(rule.id())?;
        encode_subject(writer, rule.subject())?;
        writer.put_u8(capability_tag(rule.grant().capability())?)?;
        writer.put_u8(effect_tag(rule.grant().effect()))?;
        encode_scope(writer, rule.scope())?;
    }
    Ok(())
}

fn decode_snapshot(
    reader: &mut ByteReader<'_>,
) -> Result<SecurityPolicySnapshot, SecurityPolicyStorageError> {
    let principal_count = reader.read_count()?;
    let mut principals = Vec::new();
    reserve(&mut principals, principal_count)?;
    let mut previous_principal = None;
    for _ in 0..principal_count {
        let id = reader.read_id::<PrincipalId>()?;
        ensure_strictly_increasing(&mut previous_principal, id)?;
        let state = decode_principal_state(reader.read_u8()?)?;
        principals.push(Principal::new(id).with_state(state));
    }

    let role_count = reader.read_count()?;
    let mut roles = Vec::new();
    reserve(&mut roles, role_count)?;
    let mut previous_role = None;
    for _ in 0..role_count {
        let id = reader.read_id::<RoleId>()?;
        ensure_strictly_increasing(&mut previous_role, id)?;
        let symbol = reader.read_string()?;
        let grant_count = reader.read_count()?;
        let mut grants = Vec::new();
        reserve(&mut grants, grant_count)?;
        for _ in 0..grant_count {
            let capability = decode_capability(reader.read_u8()?)?;
            let effect = decode_effect(reader.read_u8()?)?;
            grants.push(CapabilityGrant::new(capability, effect));
        }
        let bundle =
            PolicyBundle::from_grants(grants).map_err(SecurityPolicyStorageError::InvalidBundle)?;
        let role = RoleDefinition::new(id, symbol, bundle)
            .map_err(SecurityPolicyStorageError::InvalidRole)?;
        roles.push(role);
    }

    let assignment_count = reader.read_count()?;
    let mut assignments = Vec::new();
    reserve(&mut assignments, assignment_count)?;
    let mut previous_assignment = None;
    for _ in 0..assignment_count {
        let id = reader.read_id::<RoleAssignmentId>()?;
        ensure_strictly_increasing(&mut previous_assignment, id)?;
        let principal = reader.read_id::<PrincipalId>()?;
        let role = reader.read_id::<RoleId>()?;
        let scope = decode_scope(reader)?;
        assignments.push(RoleAssignment::new(id, principal, role, scope));
    }

    let rule_count = reader.read_count()?;
    let mut rules = Vec::new();
    reserve(&mut rules, rule_count)?;
    let mut previous_rule = None;
    for _ in 0..rule_count {
        let id = reader.read_id()?;
        ensure_strictly_increasing(&mut previous_rule, id)?;
        let subject = decode_subject(reader)?;
        let capability = decode_capability(reader.read_u8()?)?;
        let effect = decode_effect(reader.read_u8()?)?;
        let scope = decode_scope(reader)?;
        rules.push(CapabilityRule::new(
            id,
            subject,
            CapabilityGrant::new(capability, effect),
            scope,
        ));
    }
    SecurityPolicySnapshot::new(principals, roles, assignments, rules)
        .map_err(SecurityPolicyStorageError::InvalidPolicy)
}

fn encode_scope(
    writer: &mut ByteWriter,
    scope: PolicyScope,
) -> Result<(), SecurityPolicyStorageError> {
    encode_optional_id(writer, scope.history_space())?;
    encode_optional_id(writer, scope.layer())?;
    match scope.record() {
        None => writer.put_u8(0)?,
        Some(reference) => {
            writer.put_u8(1)?;
            let bytes = encode_record_ref(reference)
                .map_err(SecurityPolicyStorageError::RecordReference)?;
            writer.put_blob(&bytes)?;
        }
    }
    match scope.field() {
        None => writer.put_u8(0)?,
        Some(selector) => {
            writer.put_u8(1)?;
            encode_field_selector(writer, selector)?;
        }
    }
    match scope.relationship() {
        None => writer.put_u8(0)?,
        Some(selector) => {
            writer.put_u8(1)?;
            encode_relationship_selector(writer, selector)?;
        }
    }
    Ok(())
}

fn decode_scope(reader: &mut ByteReader<'_>) -> Result<PolicyScope, SecurityPolicyStorageError> {
    let history_space = decode_optional_id(reader)?;
    let layer = decode_optional_id(reader)?;
    let record = match reader.read_u8()? {
        0 => None,
        1 => {
            let bytes = reader.read_blob(64)?;
            Some(decode_record_ref(bytes).map_err(SecurityPolicyStorageError::RecordReference)?)
        }
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "scope-record option",
                tag,
            });
        }
    };
    let field = match reader.read_u8()? {
        0 => None,
        1 => Some(decode_field_selector(reader)?),
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "scope-field option",
                tag,
            });
        }
    };
    let relationship = match reader.read_u8()? {
        0 => None,
        1 => Some(decode_relationship_selector(reader)?),
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "scope-relationship option",
                tag,
            });
        }
    };
    Ok(PolicyScope::new(
        history_space,
        layer,
        record,
        field,
        relationship,
    ))
}

fn encode_subject(
    writer: &mut ByteWriter,
    subject: PolicySubject,
) -> Result<(), SecurityPolicyStorageError> {
    match subject {
        PolicySubject::Principal(id) => {
            writer.put_u8(0)?;
            writer.put_id(id)?;
        }
        PolicySubject::Role(id) => {
            writer.put_u8(1)?;
            writer.put_id(id)?;
        }
    }
    Ok(())
}

fn decode_subject(
    reader: &mut ByteReader<'_>,
) -> Result<PolicySubject, SecurityPolicyStorageError> {
    match reader.read_u8()? {
        0 => Ok(PolicySubject::Principal(reader.read_id()?)),
        1 => Ok(PolicySubject::Role(reader.read_id()?)),
        tag => Err(SecurityPolicyStorageError::InvalidTag {
            kind: "policy-subject",
            tag,
        }),
    }
}

fn encode_field_selector(
    writer: &mut ByteWriter,
    selector: FieldSelector,
) -> Result<(), SecurityPolicyStorageError> {
    match selector {
        FieldSelector::AssertionSubject => writer.put_u8(0)?,
        FieldSelector::AssertionPredicate => writer.put_u8(1)?,
        FieldSelector::AssertionValue(id) => {
            writer.put_u8(2)?;
            writer.put_id(id)?;
        }
        FieldSelector::AssertionPolarity => writer.put_u8(3)?,
        FieldSelector::AssertionValidity => writer.put_u8(4)?,
        FieldSelector::AssertionPerspective => writer.put_u8(5)?,
        FieldSelector::AssertionEpistemicMode => writer.put_u8(6)?,
        FieldSelector::MaskSelector => writer.put_u8(7)?,
        FieldSelector::MaskValidity => writer.put_u8(8)?,
        FieldSelector::ReplacementBoundarySubject => writer.put_u8(9)?,
        FieldSelector::ReplacementBoundaryPredicate => writer.put_u8(10)?,
        FieldSelector::ReplacementBoundaryValidity => writer.put_u8(11)?,
        FieldSelector::EventKind => writer.put_u8(12)?,
        FieldSelector::EventParticipant(kind, role) => {
            writer.put_u8(13)?;
            writer.put_id(kind)?;
            writer.put_id(role)?;
        }
        FieldSelector::EventAttribute(kind, attribute) => {
            writer.put_u8(14)?;
            writer.put_id(kind)?;
            writer.put_id(attribute)?;
        }
        FieldSelector::EventTime(kind) => {
            writer.put_u8(15)?;
            writer.put_id(kind)?;
        }
        FieldSelector::EventMaskTarget => writer.put_u8(16)?,
        FieldSelector::SourceKind => writer.put_u8(17)?,
        FieldSelector::SourceLocator => writer.put_u8(18)?,
        FieldSelector::SourceContentDigest => writer.put_u8(19)?,
        FieldSelector::SourceMetadata => writer.put_u8(20)?,
    }
    Ok(())
}

fn decode_field_selector(
    reader: &mut ByteReader<'_>,
) -> Result<FieldSelector, SecurityPolicyStorageError> {
    Ok(match reader.read_u8()? {
        0 => FieldSelector::AssertionSubject,
        1 => FieldSelector::AssertionPredicate,
        2 => FieldSelector::AssertionValue(reader.read_id()?),
        3 => FieldSelector::AssertionPolarity,
        4 => FieldSelector::AssertionValidity,
        5 => FieldSelector::AssertionPerspective,
        6 => FieldSelector::AssertionEpistemicMode,
        7 => FieldSelector::MaskSelector,
        8 => FieldSelector::MaskValidity,
        9 => FieldSelector::ReplacementBoundarySubject,
        10 => FieldSelector::ReplacementBoundaryPredicate,
        11 => FieldSelector::ReplacementBoundaryValidity,
        12 => FieldSelector::EventKind,
        13 => FieldSelector::EventParticipant(reader.read_id()?, reader.read_id()?),
        14 => FieldSelector::EventAttribute(reader.read_id()?, reader.read_id()?),
        15 => FieldSelector::EventTime(reader.read_id()?),
        16 => FieldSelector::EventMaskTarget,
        17 => FieldSelector::SourceKind,
        18 => FieldSelector::SourceLocator,
        19 => FieldSelector::SourceContentDigest,
        20 => FieldSelector::SourceMetadata,
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "field-selector",
                tag,
            });
        }
    })
}

fn encode_relationship_selector(
    writer: &mut ByteWriter,
    selector: worlddb_core::RelationshipSelector,
) -> Result<(), SecurityPolicyStorageError> {
    use worlddb_core::RelationshipSelector;
    match selector {
        RelationshipSelector::EventRelation(kind) => {
            writer.put_u8(0)?;
            writer.put_u8(match kind {
                PolicyEventRelationKind::Before => 0,
                PolicyEventRelationKind::SameTime => 1,
                PolicyEventRelationKind::Causes => 2,
            })?;
        }
        RelationshipSelector::Evidence(kind) => {
            writer.put_u8(1)?;
            writer.put_u8(match kind {
                EvidenceRelationship::Supports => 0,
                EvidenceRelationship::Contradicts => 1,
                EvidenceRelationship::Documents => 2,
            })?;
        }
        RelationshipSelector::Provenance(kind) => {
            writer.put_u8(2)?;
            writer.put_u8(match kind {
                ProvenanceRelationship::Corrects => 0,
                ProvenanceRelationship::DerivedFrom => 1,
                ProvenanceRelationship::ResultedFrom => 2,
            })?;
        }
        RelationshipSelector::LifecycleTarget => writer.put_u8(3)?,
    }
    Ok(())
}

fn decode_relationship_selector(
    reader: &mut ByteReader<'_>,
) -> Result<worlddb_core::RelationshipSelector, SecurityPolicyStorageError> {
    use worlddb_core::RelationshipSelector;
    Ok(match reader.read_u8()? {
        0 => RelationshipSelector::EventRelation(match reader.read_u8()? {
            0 => PolicyEventRelationKind::Before,
            1 => PolicyEventRelationKind::SameTime,
            2 => PolicyEventRelationKind::Causes,
            tag => {
                return Err(SecurityPolicyStorageError::InvalidTag {
                    kind: "event-relationship",
                    tag,
                });
            }
        }),
        1 => RelationshipSelector::Evidence(match reader.read_u8()? {
            0 => EvidenceRelationship::Supports,
            1 => EvidenceRelationship::Contradicts,
            2 => EvidenceRelationship::Documents,
            tag => {
                return Err(SecurityPolicyStorageError::InvalidTag {
                    kind: "evidence-relationship",
                    tag,
                });
            }
        }),
        2 => RelationshipSelector::Provenance(match reader.read_u8()? {
            0 => ProvenanceRelationship::Corrects,
            1 => ProvenanceRelationship::DerivedFrom,
            2 => ProvenanceRelationship::ResultedFrom,
            tag => {
                return Err(SecurityPolicyStorageError::InvalidTag {
                    kind: "provenance-relationship",
                    tag,
                });
            }
        }),
        3 => RelationshipSelector::LifecycleTarget,
        tag => {
            return Err(SecurityPolicyStorageError::InvalidTag {
                kind: "relationship-selector",
                tag,
            });
        }
    })
}

fn encode_optional_id<T: DomainId>(
    writer: &mut ByteWriter,
    id: Option<T>,
) -> Result<(), SecurityPolicyStorageError> {
    match id {
        None => writer.put_u8(0),
        Some(id) => {
            writer.put_u8(1)?;
            writer.put_id(id)
        }
    }
}

fn decode_optional_id<T: DomainId>(
    reader: &mut ByteReader<'_>,
) -> Result<Option<T>, SecurityPolicyStorageError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(reader.read_id()?)),
        tag => Err(SecurityPolicyStorageError::InvalidTag {
            kind: "scope identity option",
            tag,
        }),
    }
}

fn capability_tag(capability: Capability) -> Result<u8, SecurityPolicyStorageError> {
    Capability::ALL
        .iter()
        .position(|candidate| *candidate == capability)
        .and_then(|index| u8::try_from(index).ok())
        .ok_or(SecurityPolicyStorageError::InvalidSecurityFrame)
}

fn decode_capability(tag: u8) -> Result<Capability, SecurityPolicyStorageError> {
    Capability::ALL
        .get(usize::from(tag))
        .copied()
        .ok_or(SecurityPolicyStorageError::InvalidTag {
            kind: "capability",
            tag,
        })
}

fn effect_tag(effect: GrantEffect) -> u8 {
    match effect {
        GrantEffect::Allow => 0,
        GrantEffect::Deny => 1,
    }
}

fn decode_effect(tag: u8) -> Result<GrantEffect, SecurityPolicyStorageError> {
    match tag {
        0 => Ok(GrantEffect::Allow),
        1 => Ok(GrantEffect::Deny),
        tag => Err(SecurityPolicyStorageError::InvalidTag {
            kind: "grant-effect",
            tag,
        }),
    }
}

fn reserve<T>(items: &mut Vec<T>, count: usize) -> Result<(), SecurityPolicyStorageError> {
    let estimated_bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or(
        SecurityPolicyStorageError::ResourceLimit {
            resource: DecodeResource::CollectionBytes,
            limit: DecoderLimits::DEFAULT.max_collection_bytes,
            actual: usize::MAX,
        },
    )?;
    if estimated_bytes > DecoderLimits::DEFAULT.max_collection_bytes {
        return Err(SecurityPolicyStorageError::ResourceLimit {
            resource: DecodeResource::CollectionBytes,
            limit: DecoderLimits::DEFAULT.max_collection_bytes,
            actual: estimated_bytes,
        });
    }
    items
        .try_reserve_exact(count)
        .map_err(|_| SecurityPolicyStorageError::AllocationFailed)
}

fn ensure_strictly_increasing<T: Ord + Copy>(
    previous: &mut Option<T>,
    current: T,
) -> Result<(), SecurityPolicyStorageError> {
    if previous.is_some_and(|prior| prior >= current) {
        return Err(SecurityPolicyStorageError::InvalidSecurityFrame);
    }
    *previous = Some(current);
    Ok(())
}

struct ByteWriter {
    bytes: Vec<u8>,
}

impl ByteWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn put_raw(&mut self, bytes: &[u8]) -> Result<(), SecurityPolicyStorageError> {
        let new_len = self.bytes.len().checked_add(bytes.len()).ok_or(
            SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: DecoderLimits::DEFAULT.max_frame_bytes,
                actual: usize::MAX,
            },
        )?;
        if new_len > DecoderLimits::DEFAULT.max_frame_bytes {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: DecoderLimits::DEFAULT.max_frame_bytes,
                actual: new_len,
            });
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn put_u8(&mut self, value: u8) -> Result<(), SecurityPolicyStorageError> {
        self.put_raw(&[value])
    }

    fn put_u16(&mut self, value: u16) -> Result<(), SecurityPolicyStorageError> {
        self.put_raw(&value.to_le_bytes())
    }

    fn put_u32(&mut self, value: u32) -> Result<(), SecurityPolicyStorageError> {
        self.put_raw(&value.to_le_bytes())
    }

    fn put_u64(&mut self, value: u64) -> Result<(), SecurityPolicyStorageError> {
        self.put_raw(&value.to_le_bytes())
    }

    fn put_count(&mut self, count: usize) -> Result<(), SecurityPolicyStorageError> {
        if count > DecoderLimits::DEFAULT.max_array_items {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::ArrayItems,
                limit: DecoderLimits::DEFAULT.max_array_items,
                actual: count,
            });
        }
        self.put_u32(u32::try_from(count).map_err(|_| {
            SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::ArrayItems,
                limit: u32::MAX as usize,
                actual: count,
            }
        })?)
    }

    fn put_id<T: DomainId>(&mut self, id: T) -> Result<(), SecurityPolicyStorageError> {
        self.put_raw(&id.to_bytes())
    }

    fn put_string(&mut self, value: &str) -> Result<(), SecurityPolicyStorageError> {
        if value.len() > DecoderLimits::DEFAULT.max_string_or_bytes {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::StringOrBytes,
                limit: DecoderLimits::DEFAULT.max_string_or_bytes,
                actual: value.len(),
            });
        }
        self.put_u32(u32::try_from(value.len()).map_err(|_| {
            SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::StringOrBytes,
                limit: u32::MAX as usize,
                actual: value.len(),
            }
        })?)?;
        self.put_raw(value.as_bytes())
    }

    fn put_blob(&mut self, value: &[u8]) -> Result<(), SecurityPolicyStorageError> {
        self.put_u32(u32::try_from(value.len()).map_err(|_| {
            SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: u32::MAX as usize,
                actual: value.len(),
            }
        })?)?;
        self.put_raw(value)
    }
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], SecurityPolicyStorageError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(SecurityPolicyStorageError::InvalidSecurityFrame)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(SecurityPolicyStorageError::InvalidSecurityFrame)?;
        self.offset = end;
        Ok(value)
    }

    fn read_u8(&mut self) -> Result<u8, SecurityPolicyStorageError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(SecurityPolicyStorageError::InvalidSecurityFrame)
    }

    fn read_u16(&mut self) -> Result<u16, SecurityPolicyStorageError> {
        let mut value = [0_u8; 2];
        value.copy_from_slice(self.take(2)?);
        Ok(u16::from_le_bytes(value))
    }

    fn read_u32(&mut self) -> Result<u32, SecurityPolicyStorageError> {
        let mut value = [0_u8; 4];
        value.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(value))
    }

    fn read_u64(&mut self) -> Result<u64, SecurityPolicyStorageError> {
        let mut value = [0_u8; 8];
        value.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(value))
    }

    fn read_count(&mut self) -> Result<usize, SecurityPolicyStorageError> {
        let count = usize::try_from(self.read_u32()?)
            .map_err(|_| SecurityPolicyStorageError::InvalidSecurityFrame)?;
        if count > DecoderLimits::DEFAULT.max_array_items {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::ArrayItems,
                limit: DecoderLimits::DEFAULT.max_array_items,
                actual: count,
            });
        }
        Ok(count)
    }

    fn read_id<T: DomainId>(&mut self) -> Result<T, SecurityPolicyStorageError> {
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(self.take(16)?);
        T::try_from_bytes(bytes).map_err(SecurityPolicyStorageError::InvalidIdentity)
    }

    fn read_string(&mut self) -> Result<String, SecurityPolicyStorageError> {
        let length = usize::try_from(self.read_u32()?)
            .map_err(|_| SecurityPolicyStorageError::InvalidSecurityFrame)?;
        if length > DecoderLimits::DEFAULT.max_string_or_bytes {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::StringOrBytes,
                limit: DecoderLimits::DEFAULT.max_string_or_bytes,
                actual: length,
            });
        }
        let value = std::str::from_utf8(self.take(length)?)
            .map_err(|_| SecurityPolicyStorageError::InvalidSymbol)?;
        let mut string = String::new();
        string
            .try_reserve_exact(value.len())
            .map_err(|_| SecurityPolicyStorageError::AllocationFailed)?;
        string.push_str(value);
        Ok(string)
    }

    fn read_blob(&mut self, maximum: usize) -> Result<&'a [u8], SecurityPolicyStorageError> {
        let length = usize::try_from(self.read_u32()?)
            .map_err(|_| SecurityPolicyStorageError::InvalidSecurityFrame)?;
        if length > maximum {
            return Err(SecurityPolicyStorageError::ResourceLimit {
                resource: DecodeResource::BatchBytes,
                limit: maximum,
                actual: length,
            });
        }
        self.take(length)
    }

    fn finish(self) -> Result<(), SecurityPolicyStorageError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(SecurityPolicyStorageError::InvalidSecurityFrame)
        }
    }
}

#[cfg(all(test, windows))]
mod windows_crash_tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::SecurityPolicyHistoryStore;
    use crate::segment::SegmentIoCheckpoint;
    use crate::{DatabaseLayout, RecoveryManager, RecoveryScanner};
    use worlddb_core::{Revision, SecurityEpoch, SecurityPolicySnapshot, SecurityPolicyVersion};

    static NEXT_TEMP_DATABASE: AtomicU64 = AtomicU64::new(0);

    struct TempDatabase(PathBuf);

    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEMP_DATABASE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "worlddb-security-segment-crash-{}-{sequence}",
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

    #[test]
    fn process_crash_at_each_security_segment_stage_write_recovers_genesis() -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M5_22_SECURITY_SEGMENT_CRASH_ROOT";
        const POINT_ENV: &str = "WORLDDB_M5_22_SECURITY_SEGMENT_CRASH_POINT";
        const TEST_NAME: &str = "security_segment::windows_crash_tests::process_crash_at_each_security_segment_stage_write_recovers_genesis";

        if let (Ok(root), Ok(point_name)) = (env::var(ROOT_ENV), env::var(POINT_ENV)) {
            let point = match point_name.as_str() {
                "stage_file_created" => SegmentIoCheckpoint::StageFileCreated,
                "magic_written" => SegmentIoCheckpoint::MagicWritten,
                "identity_written" => SegmentIoCheckpoint::IdentityWritten,
                "digest_written" => SegmentIoCheckpoint::DigestWritten,
                "content_written" => SegmentIoCheckpoint::ContentWritten,
                "file_synced" => SegmentIoCheckpoint::FileSynced,
                "staging_directory_synced" => SegmentIoCheckpoint::StagingDirectorySynced,
                _ => return Err(format!("unknown security-segment checkpoint {point_name}")),
            };
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            RecoveryManager::new(layout.clone())
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            let version = SecurityPolicyVersion::new(
                Revision::GENESIS,
                SecurityEpoch::INITIAL,
                SecurityPolicySnapshot::default(),
            );
            let store = SecurityPolicyHistoryStore::new(layout);
            let _ =
                store.write_version_at_with_checkpoint(&lock, &version, None, None, true, |at| {
                    if at == point {
                        std::process::exit(86);
                    }
                });
            return Err(format!(
                "child did not reach security-segment checkpoint {point_name}"
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
            let status = Command::new(executable)
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(ROOT_ENV, &database.0)
                .env(POINT_ENV, point_name)
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
            let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let report = RecoveryScanner::new(layout.clone())
                .scan(&lock)
                .map_err(|error| error.to_string())?;
            assert_eq!(report.safe_revision(), Revision::GENESIS);
            let recovered = RecoveryManager::new(layout)
                .recover(&lock)
                .map_err(|error| error.to_string())?;
            assert_eq!(recovered.report().safe_revision(), Revision::GENESIS);
            assert!(recovered.report().is_clean());
        }
        Ok(())
    }
}
