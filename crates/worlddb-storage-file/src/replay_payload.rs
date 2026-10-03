//! Canonical storage snapshots embedded in committed WAL operations.

use std::fmt;

use worlddb_core::{DomainId, OperationId, Revision};

use crate::required_audit::{RequiredAuditError, decode_required_audit_payload};
use crate::{
    ContentDigest, ManifestError, ManifestSegmentKind, ManifestSegmentReference, ManifestSnapshot,
    SegmentId,
};

const REPLAY_MAGIC: [u8; 8] = *b"WDBRPL\0\x01";
const MAX_REPLAY_REFERENCES: usize = 262_144;
const REFERENCE_BYTES: usize = 1 + 8 + 16 + 32;
const MAX_REPLAY_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum SnapshotCommitError {
    Wal(super::wal::WalError),
    RequiredAudit(RequiredAuditError),
    Manifest(ManifestError),
    HistorySegment(super::segment::SegmentError),
    SecuritySegment(super::security_segment::SecurityPolicyStorageError),
    InvalidStagedReference,
    PayloadTooLarge,
}

impl fmt::Display for SnapshotCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wal(error) => write!(formatter, "snapshot WAL commit failed: {error}"),
            Self::RequiredAudit(error) => {
                write!(formatter, "snapshot required-audit commit failed: {error}")
            }
            Self::Manifest(error) => write!(formatter, "snapshot manifest is invalid: {error}"),
            Self::HistorySegment(error) => {
                write!(formatter, "history segment validation failed: {error}")
            }
            Self::SecuritySegment(error) => {
                write!(formatter, "security segment validation failed: {error}")
            }
            Self::InvalidStagedReference => formatter
                .write_str("staged segments must be unique members of the committed snapshot"),
            Self::PayloadTooLarge => {
                formatter.write_str("replayable storage snapshot exceeds the WAL payload limit")
            }
        }
    }
}

impl std::error::Error for SnapshotCommitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wal(error) => Some(error),
            Self::RequiredAudit(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::HistorySegment(error) => Some(error),
            Self::SecuritySegment(error) => Some(error),
            Self::InvalidStagedReference | Self::PayloadTooLarge => None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum ReplayPayloadError {
    Invalid,
    Manifest(ManifestError),
    RequiredAudit(RequiredAuditError),
}

pub(crate) struct DecodedReplaySnapshot {
    pub(crate) snapshot: ManifestSnapshot,
    pub(crate) staged: Vec<ManifestSegmentReference>,
}

impl fmt::Display for ReplayPayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => {
                formatter.write_str("replayable storage snapshot payload is malformed")
            }
            Self::Manifest(error) => {
                write!(formatter, "replayable storage snapshot is invalid: {error}")
            }
            Self::RequiredAudit(error) => {
                write!(formatter, "replayable audit envelope is invalid: {error}")
            }
        }
    }
}

impl std::error::Error for ReplayPayloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(error) => Some(error),
            Self::RequiredAudit(error) => Some(error),
            Self::Invalid => None,
        }
    }
}

pub(crate) fn encode_replay_payload(
    snapshot: &ManifestSnapshot,
    staged_segments: &[ManifestSegmentReference],
) -> Result<Vec<u8>, SnapshotCommitError> {
    if snapshot.segments().len() > MAX_REPLAY_REFERENCES
        || staged_segments.len() > snapshot.segments().len()
    {
        return Err(SnapshotCommitError::PayloadTooLarge);
    }
    if staged_segments
        .iter()
        .any(|staged| !snapshot.segments().contains(staged))
    {
        return Err(SnapshotCommitError::InvalidStagedReference);
    }
    let mut canonical_staged = staged_segments.to_vec();
    canonical_staged.sort_by_key(|reference| (reference.kind(), reference.id().to_bytes()));
    if canonical_staged
        .windows(2)
        .any(|pair| match (pair.first(), pair.get(1)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        })
    {
        return Err(SnapshotCommitError::InvalidStagedReference);
    }
    let reference_count = u32::try_from(snapshot.segments().len())
        .map_err(|_| SnapshotCommitError::PayloadTooLarge)?;
    let staged_count =
        u32::try_from(canonical_staged.len()).map_err(|_| SnapshotCommitError::PayloadTooLarge)?;
    let capacity = REPLAY_MAGIC
        .len()
        .checked_add(8)
        .and_then(|value| {
            value.checked_add(snapshot.segments().len().checked_mul(REFERENCE_BYTES)?)
        })
        .and_then(|value| value.checked_add(canonical_staged.len().checked_mul(REFERENCE_BYTES)?))
        .ok_or(SnapshotCommitError::PayloadTooLarge)?;
    if capacity > MAX_REPLAY_PAYLOAD_BYTES {
        return Err(SnapshotCommitError::PayloadTooLarge);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| SnapshotCommitError::PayloadTooLarge)?;
    bytes.extend_from_slice(&REPLAY_MAGIC);
    bytes.extend_from_slice(&reference_count.to_le_bytes());
    bytes.extend_from_slice(&staged_count.to_le_bytes());
    for reference in snapshot.segments() {
        encode_reference(*reference, &mut bytes);
    }
    for reference in &canonical_staged {
        encode_reference(*reference, &mut bytes);
    }
    Ok(bytes)
}

pub(crate) fn decode_replay_payload(
    payload: &[u8],
    revision: Revision,
    operation_id: OperationId,
) -> Result<Option<DecodedReplaySnapshot>, ReplayPayloadError> {
    let audited = decode_required_audit_payload(payload, revision, operation_id)
        .map_err(ReplayPayloadError::RequiredAudit)?;
    let payload = match audited {
        Some(decoded) => decoded.replay_payload.unwrap_or(decoded.action_payload),
        None => payload,
    };
    if payload.get(..REPLAY_MAGIC.len()) != Some(&REPLAY_MAGIC) {
        return Ok(None);
    }
    if payload.len() > MAX_REPLAY_PAYLOAD_BYTES || payload.len() < REPLAY_MAGIC.len() + 8 {
        return Err(ReplayPayloadError::Invalid);
    }
    let mut reader = Reader::new(payload, REPLAY_MAGIC.len());
    let reference_count =
        usize::try_from(reader.u32()?).map_err(|_| ReplayPayloadError::Invalid)?;
    let staged_count = usize::try_from(reader.u32()?).map_err(|_| ReplayPayloadError::Invalid)?;
    if reference_count > MAX_REPLAY_REFERENCES || staged_count > reference_count {
        return Err(ReplayPayloadError::Invalid);
    }
    let expected_len = REPLAY_MAGIC
        .len()
        .checked_add(8)
        .and_then(|value| value.checked_add(reference_count.checked_mul(REFERENCE_BYTES)?))
        .and_then(|value| value.checked_add(staged_count.checked_mul(REFERENCE_BYTES)?))
        .ok_or(ReplayPayloadError::Invalid)?;
    if payload.len() != expected_len {
        return Err(ReplayPayloadError::Invalid);
    }
    let mut references = Vec::new();
    references
        .try_reserve_exact(reference_count)
        .map_err(|_| ReplayPayloadError::Invalid)?;
    for _ in 0..reference_count {
        references.push(reader.reference()?);
    }
    let mut staged = Vec::new();
    staged
        .try_reserve_exact(staged_count)
        .map_err(|_| ReplayPayloadError::Invalid)?;
    for _ in 0..staged_count {
        staged.push(reader.reference()?);
    }
    reader.finish()?;
    if staged
        .iter()
        .any(|reference| !references.contains(reference))
    {
        return Err(ReplayPayloadError::Invalid);
    }
    let mut canonical_staged = staged.clone();
    canonical_staged.sort_by_key(segment_sort_key);
    if canonical_staged != staged {
        return Err(ReplayPayloadError::Invalid);
    }
    let snapshot = ManifestSnapshot::new(revision, references.clone())
        .map_err(ReplayPayloadError::Manifest)?;
    if snapshot.segments() != references.as_slice() {
        return Err(ReplayPayloadError::Invalid);
    }
    Ok(Some(DecodedReplaySnapshot { snapshot, staged }))
}

fn segment_sort_key(reference: &ManifestSegmentReference) -> (ManifestSegmentKind, [u8; 16]) {
    (reference.kind(), reference.id().to_bytes())
}

fn encode_reference(reference: ManifestSegmentReference, output: &mut Vec<u8>) {
    output.push(match reference.kind() {
        ManifestSegmentKind::History => 1,
        ManifestSegmentKind::SecurityPolicy => 2,
    });
    output.extend_from_slice(&reference.through_revision().value().to_le_bytes());
    output.extend_from_slice(reference.id().as_bytes());
    output.extend_from_slice(reference.content_digest().as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8], offset: usize) -> Self {
        Self { bytes, offset }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ReplayPayloadError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ReplayPayloadError::Invalid)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ReplayPayloadError::Invalid)?;
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, ReplayPayloadError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ReplayPayloadError::Invalid)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, ReplayPayloadError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| ReplayPayloadError::Invalid)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn reference(&mut self) -> Result<ManifestSegmentReference, ReplayPayloadError> {
        let kind = match self.take(1)?.first().copied() {
            Some(1) => ManifestSegmentKind::History,
            Some(2) => ManifestSegmentKind::SecurityPolicy,
            _ => return Err(ReplayPayloadError::Invalid),
        };
        let through_revision =
            Revision::try_from(self.u64()?).map_err(|_| ReplayPayloadError::Invalid)?;
        let id_bytes: [u8; 16] = self
            .take(16)?
            .try_into()
            .map_err(|_| ReplayPayloadError::Invalid)?;
        let id = SegmentId::try_from_bytes(id_bytes).map_err(|_| ReplayPayloadError::Invalid)?;
        let digest = ContentDigest::from_bytes(
            self.take(32)?
                .try_into()
                .map_err(|_| ReplayPayloadError::Invalid)?,
        );
        Ok(ManifestSegmentReference::new(
            kind,
            id,
            digest,
            through_revision,
        ))
    }

    fn finish(self) -> Result<(), ReplayPayloadError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ReplayPayloadError::Invalid)
        }
    }
}
