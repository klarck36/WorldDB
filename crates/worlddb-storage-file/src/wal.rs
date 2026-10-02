//! Append-only data-WAL frames, commit markers, and OperationId status.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use worlddb_core::{
    AuditRecord, CHECKSUM_LEN, DecoderLimits, DomainId, FRAME_HEADER_LEN, FrameError, FrameHeader,
    OperationId, Revision, RevisionError, TlvDecoder, TlvEncoder, WireError, decode_frame,
    encode_frame,
};

use crate::layout::DatabaseLayout;
use crate::manifest::{ManifestSegmentKind, ManifestSegmentReference, ManifestSnapshot};
use crate::recovery::{RecoveryCorruptionKind, RecoveryFinding};
use crate::replay_payload::{SnapshotCommitError, encode_replay_payload};
use crate::required_audit::{
    CommittedRequiredAuditRecord, RequiredAuditError, encode_required_audit_payload,
    latest_sequence, records_for_log, validate_commit_binding, validate_sequence_after,
};
use crate::security_segment::SecurityPolicyHistoryStore;
use crate::segment::HistorySegmentStore;
use crate::writer_lock::{WriterLock, WriterLockError};

const WAL_PREPARE_FRAME_KIND: u32 = 0x5744_4250;
const WAL_COMMIT_FRAME_KIND: u32 = 0x5744_4243;
const OPERATION_ID_FIELD_TAG: u32 = 1;
const PAYLOAD_FIELD_TAG: u32 = 2;
const PREPARE_SEGMENT_FIELD_TAG: u32 = 1;
const PREPARE_OFFSET_FIELD_TAG: u32 = 2;
const PREPARE_LENGTH_FIELD_TAG: u32 = 3;
const COMMIT_REVISION_FIELD_TAG: u32 = 4;
const COMMIT_OPERATION_ID_FIELD_TAG: u32 = 5;
const PAYLOAD_HASH_FIELD_TAG: u32 = 6;
const PREVIOUS_COMMIT_HASH_FIELD_TAG: u32 = 7;
const COMMIT_HASH_FIELD_TAG: u32 = 8;
const SEGMENT_PREFIX: &str = "segment-";
const SEGMENT_SUFFIX: &str = ".wal";
const SEGMENT_NUMBER_WIDTH: usize = 20;
const TARGET_SEGMENT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_WAL_FRAME_BYTES: usize = DecoderLimits::DEFAULT.max_frame_bytes;
const COMMIT_HASH_CONTEXT: &str = "worlddb.wal.commit-chain.v1";

type IndeterminateOperationKey = (PathBuf, OperationId);
type IndeterminateOperationMap = BTreeMap<IndeterminateOperationKey, [u8; 32]>;

static INDETERMINATE_COMMIT_IDS: OnceLock<Mutex<IndeterminateOperationMap>> = OnceLock::new();

/// Why a WAL prepare could not be safely appended or read back.
#[derive(Debug)]
pub enum WalError {
    /// The supplied writer lock belongs to another database directory.
    WriterLockMismatch,
    /// Recovery verification has not authorized ordinary database writes.
    RecoveryRequired,
    /// The writer lock could not be obtained for an internal operation.
    WriterLock(WriterLockError),
    /// A filesystem operation failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
    /// One WAL frame failed checksum or wire-format validation.
    Frame(FrameError),
    /// One prepare payload failed strict TLV validation.
    Wire(WireError),
    /// A WAL directory entry does not use the registered segment filename.
    InvalidSegmentName { name: String },
    /// A segment resolves outside the database root or is not a regular file.
    InvalidSegmentPath { sequence: u64 },
    /// Segment files are not a contiguous sequence beginning at one.
    SegmentSequenceGap { expected: u64, actual: u64 },
    /// The next segment number cannot be represented.
    SegmentSequenceOverflow,
    /// A WAL checkpoint requires a fully verified tail with no recovery finding.
    CheckpointRequiresCleanWal,
    /// The requested checkpoint revision is not the current verified head.
    CheckpointRevisionMismatch {
        expected: Revision,
        actual: Revision,
    },
    /// The verified WAL head has no matching commit-marker boundary.
    CheckpointBoundaryMissing { revision: Revision },
    /// A WAL segment exceeds the bounded frame-read policy.
    SegmentTooLarge { limit: u64, actual: u64 },
    /// A complete frame header or body is missing at the end of a segment.
    TornTail { sequence: u64, offset: u64 },
    /// A valid frame uses a kind not yet supported by the prepare reader.
    UnsupportedFrameKind {
        sequence: u64,
        offset: u64,
        kind: u32,
    },
    /// A prepare frame does not contain exactly the OperationId and payload fields.
    MalformedPrepare { sequence: u64, offset: u64 },
    /// The OperationId bytes do not form a valid persistent identity.
    InvalidOperationId(worlddb_core::IdValidationError),
    /// A commit-marker frame does not contain the exact registered fields.
    MalformedCommitMarker { sequence: u64, offset: u64 },
    /// A commit marker references no preceding uncommitted prepare frame.
    CommitMarkerWithoutPrepare { sequence: u64, offset: u64 },
    /// The selected prepare is not the last complete frame in its segment.
    PrepareNotAppendTail,
    /// A commit marker's payload digest differs from its referenced prepare.
    PayloadHashMismatch { sequence: u64, offset: u64 },
    /// A commit marker's chained digest does not match its canonical fields.
    CommitHashMismatch { sequence: u64, offset: u64 },
    /// A commit marker does not extend the prior commit hash.
    CommitChainMismatch { sequence: u64, offset: u64 },
    /// A commit revision is not the next revision after the committed head.
    RevisionSequenceMismatch {
        expected: Revision,
        actual: Revision,
    },
    /// Revision allocation reached its reserved maximum.
    Revision(RevisionError),
    /// A marker append or its second sync may have reached persistent storage.
    UnknownCommitOutcome {
        operation_id: OperationId,
        operation: &'static str,
        source: io::Error,
    },
    /// An OperationId is already bound to different canonical payload bytes.
    IdempotencyMismatch { operation_id: OperationId },
    /// The OperationId already has a committed entry and cannot begin again.
    OperationAlreadyCommitted { operation_id: OperationId },
    /// The OperationId has a synced prepare whose commit outcome is unresolved.
    OperationIndeterminate { operation_id: OperationId },
    /// More than one WAL entry claims the same OperationId.
    DuplicateOperationId { operation_id: OperationId },
    /// An allocation for a bounded decoded payload could not be reserved.
    AllocationFailed,
    /// A payload plus its frame exceeds the configured decoder budget.
    FrameTooLarge { limit: usize, actual: usize },
}

impl fmt::Display for WalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriterLockMismatch => {
                formatter.write_str("writer lock belongs to another database directory")
            }
            Self::RecoveryRequired => {
                formatter.write_str("WAL writes are blocked until recovery verifies a clean state")
            }
            Self::WriterLock(error) => error.fmt(formatter),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Frame(error) => write!(formatter, "invalid WAL frame: {error}"),
            Self::Wire(error) => write!(formatter, "invalid WAL prepare payload: {error}"),
            Self::InvalidSegmentName { name } => {
                write!(formatter, "invalid WAL segment filename {name:?}")
            }
            Self::InvalidSegmentPath { sequence } => {
                write!(
                    formatter,
                    "WAL segment {sequence} is not a regular in-root file"
                )
            }
            Self::SegmentSequenceGap { expected, actual } => write!(
                formatter,
                "WAL segment sequence expected {expected}, found {actual}"
            ),
            Self::SegmentSequenceOverflow => formatter.write_str("WAL segment sequence overflow"),
            Self::CheckpointRequiresCleanWal => {
                formatter.write_str("WAL checkpoint requires a clean committed prefix")
            }
            Self::CheckpointRevisionMismatch { expected, actual } => write!(
                formatter,
                "WAL checkpoint expected revision {}, found head {}",
                expected.value(),
                actual.value()
            ),
            Self::CheckpointBoundaryMissing { revision } => write!(
                formatter,
                "WAL checkpoint has no commit boundary at revision {}",
                revision.value()
            ),
            Self::SegmentTooLarge { limit, actual } => write!(
                formatter,
                "WAL segment exceeds {limit} byte read limit: {actual} bytes"
            ),
            Self::TornTail { sequence, offset } => {
                write!(
                    formatter,
                    "WAL segment {sequence} has a torn tail at {offset}"
                )
            }
            Self::UnsupportedFrameKind {
                sequence,
                offset,
                kind,
            } => write!(
                formatter,
                "unsupported WAL frame kind {kind:#010x} at segment {sequence} offset {offset}"
            ),
            Self::MalformedPrepare { sequence, offset } => write!(
                formatter,
                "malformed WAL prepare at segment {sequence} offset {offset}"
            ),
            Self::InvalidOperationId(error) => {
                write!(formatter, "invalid WAL OperationId: {error}")
            }
            Self::MalformedCommitMarker { sequence, offset } => write!(
                formatter,
                "malformed WAL commit marker at segment {sequence} offset {offset}"
            ),
            Self::CommitMarkerWithoutPrepare { sequence, offset } => write!(
                formatter,
                "WAL commit marker has no matching prepare at segment {sequence} offset {offset}"
            ),
            Self::PrepareNotAppendTail => {
                formatter.write_str("WAL prepare is not the last frame in its segment")
            }
            Self::PayloadHashMismatch { sequence, offset } => write!(
                formatter,
                "WAL payload hash mismatch at segment {sequence} offset {offset}"
            ),
            Self::CommitHashMismatch { sequence, offset } => write!(
                formatter,
                "WAL commit hash mismatch at segment {sequence} offset {offset}"
            ),
            Self::CommitChainMismatch { sequence, offset } => write!(
                formatter,
                "WAL commit hash chain mismatch at segment {sequence} offset {offset}"
            ),
            Self::RevisionSequenceMismatch { expected, actual } => write!(
                formatter,
                "WAL commit revision expected {}, found {}",
                expected.value(),
                actual.value()
            ),
            Self::Revision(error) => write!(formatter, "WAL revision allocation failed: {error}"),
            Self::UnknownCommitOutcome {
                operation_id,
                operation,
                source,
            } => write!(
                formatter,
                "commit outcome for OperationId {operation_id} is unknown after {operation}: {source}"
            ),
            Self::IdempotencyMismatch { operation_id } => write!(
                formatter,
                "OperationId {operation_id} is bound to different payload bytes"
            ),
            Self::OperationAlreadyCommitted { operation_id } => write!(
                formatter,
                "OperationId {operation_id} already has a committed WAL entry"
            ),
            Self::OperationIndeterminate { operation_id } => write!(
                formatter,
                "OperationId {operation_id} has an unresolved WAL prepare"
            ),
            Self::DuplicateOperationId { operation_id } => write!(
                formatter,
                "multiple WAL entries claim OperationId {operation_id}"
            ),
            Self::AllocationFailed => formatter.write_str("could not reserve WAL payload memory"),
            Self::FrameTooLarge { limit, actual } => write!(
                formatter,
                "WAL frame exceeds {limit} byte limit: {actual} bytes"
            ),
        }
    }
}

impl std::error::Error for WalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WriterLock(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Frame(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::InvalidOperationId(error) => Some(error),
            Self::Revision(error) => Some(error),
            Self::UnknownCommitOutcome { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Location and identity of one synced WAL prepare frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalPrepareReference {
    segment_sequence: u64,
    byte_offset: u64,
    frame_length: u64,
    operation_id: OperationId,
}

impl WalPrepareReference {
    /// Monotonic segment sequence, starting at one.
    #[must_use]
    pub const fn segment_sequence(self) -> u64 {
        self.segment_sequence
    }

    /// Byte offset of the frame within its segment.
    #[must_use]
    pub const fn byte_offset(self) -> u64 {
        self.byte_offset
    }

    /// Exact encoded frame length.
    #[must_use]
    pub const fn frame_length(self) -> u64 {
        self.frame_length
    }

    /// Operation identity included in the prepare frame.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }
}

/// WAL entry read from a segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WalLogEntry {
    /// A valid prepare with no matching commit marker; therefore it is uncommitted.
    PreparedUncommitted(WalPreparedFrame),
    /// A prepare paired with its valid commit marker.
    Committed(WalCommittedFrame),
}

/// Read-back contents of one valid prepare frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalPreparedFrame {
    reference: WalPrepareReference,
    payload: Vec<u8>,
}

impl WalPreparedFrame {
    /// Stable location and operation identity of this frame.
    #[must_use]
    pub const fn reference(&self) -> WalPrepareReference {
        self.reference
    }

    /// Exact payload bytes stored by the caller.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Validated commit-chain digest.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WalCommitHash([u8; 32]);

impl WalCommitHash {
    /// Constructs a hash value from its exact 32-byte representation.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Exact 32-byte BLAKE3 commit-chain digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Current verified revision and commit hash of one WAL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalCommitHead {
    revision: Revision,
    commit_hash: WalCommitHash,
}

/// Exact byte boundaries for one durable, closed WAL prefix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalCheckpoint {
    revision: Revision,
    commit_hash: WalCommitHash,
    segments: Vec<WalCheckpointSegment>,
}

impl WalCheckpoint {
    /// Revision captured by the checkpoint.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Commit-chain digest captured by the checkpoint.
    #[must_use]
    pub const fn commit_hash(&self) -> WalCommitHash {
        self.commit_hash
    }

    /// Ordered WAL segments and exact byte lengths in the closed prefix.
    #[must_use]
    pub fn segments(&self) -> &[WalCheckpointSegment] {
        &self.segments
    }
}

/// One WAL file included through the exact checkpoint byte length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalCheckpointSegment {
    sequence: u64,
    byte_length: u64,
}

impl WalCheckpointSegment {
    /// Monotonic WAL segment sequence.
    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    /// Number of leading bytes covered by the checkpoint.
    #[must_use]
    pub const fn byte_length(self) -> u64 {
        self.byte_length
    }
}

pub(crate) struct WalRecoveryPrefix {
    pub(crate) head: WalCommitHead,
    pub(crate) hashes_by_revision: BTreeMap<Revision, WalCommitHash>,
    pub(crate) committed_frames: Vec<WalCommittedFrame>,
    pub(crate) finding: Option<RecoveryFinding>,
}

impl WalCommitHead {
    /// Latest revision whose commit marker and chain have verified.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Latest verified commit-chain hash; zero at genesis.
    #[must_use]
    pub const fn commit_hash(self) -> WalCommitHash {
        self.commit_hash
    }
}

/// Successful WAL commit-marker publication after the second file sync.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalCommitReceipt {
    reference: WalPrepareReference,
    revision: Revision,
    payload_hash: [u8; 32],
    previous_commit_hash: WalCommitHash,
    commit_hash: WalCommitHash,
    marker_offset: u64,
    marker_length: u64,
}

impl WalCommitReceipt {
    /// Prepare frame bound by this committed marker.
    #[must_use]
    pub const fn reference(self) -> WalPrepareReference {
        self.reference
    }

    /// Revision published by this commit marker.
    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// BLAKE3 digest of the exact prepare payload bytes.
    #[must_use]
    pub const fn payload_hash(self) -> [u8; 32] {
        self.payload_hash
    }

    /// Commit-chain hash immediately before this transaction.
    #[must_use]
    pub const fn previous_commit_hash(self) -> WalCommitHash {
        self.previous_commit_hash
    }

    /// Commit-chain hash produced by this transaction.
    #[must_use]
    pub const fn commit_hash(self) -> WalCommitHash {
        self.commit_hash
    }

    /// Byte offset of the marker frame in its segment.
    #[must_use]
    pub const fn marker_offset(self) -> u64 {
        self.marker_offset
    }

    /// Exact encoded marker length.
    #[must_use]
    pub const fn marker_length(self) -> u64 {
        self.marker_length
    }
}

/// Persistent operation status reconstructed from the complete WAL view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalOperationStatus {
    /// A verified commit marker binds this operation to its original receipt.
    Committed(WalCommitReceipt),
    /// A complete valid WAL scan contains no prepare for this OperationId.
    NotCommitted,
    /// A prepare exists without a verified commit marker.
    Indeterminate,
}

/// Read-back of a prepare and its structurally matching commit marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalCommittedFrame {
    prepare: WalPreparedFrame,
    receipt: WalCommitReceipt,
}

impl WalCommittedFrame {
    /// Validated prepare frame covered by the commit marker.
    #[must_use]
    pub const fn prepare(&self) -> &WalPreparedFrame {
        &self.prepare
    }

    /// Commit marker fields and their location.
    #[must_use]
    pub const fn receipt(&self) -> WalCommitReceipt {
        self.receipt
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WalCommitMarker {
    prepare_reference: WalPrepareReference,
    operation_id: OperationId,
    revision: Revision,
    payload_hash: [u8; 32],
    previous_commit_hash: WalCommitHash,
    commit_hash: WalCommitHash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RawWalFrame {
    Prepare(WalPreparedFrame),
    Commit {
        marker: WalCommitMarker,
        byte_offset: u64,
        frame_length: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndexedWalOperation {
    payload_fingerprint: [u8; 32],
    status: IndexedWalOperationStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IndexedWalOperationStatus {
    Committed(WalCommitReceipt),
    Indeterminate,
}

/// Append-only WAL access for the database layout's numbered segment files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalPrepareLog {
    layout: DatabaseLayout,
    root: PathBuf,
    directory: PathBuf,
}

impl WalPrepareLog {
    /// Binds the prepare log to a validated database layout.
    #[must_use]
    pub fn new(layout: &DatabaseLayout) -> Self {
        Self {
            layout: layout.clone(),
            root: layout.root().to_path_buf(),
            directory: layout.wal_directory(),
        }
    }

    pub(crate) fn scan_recovery_prefix(
        &self,
        lock: &WriterLock,
    ) -> Result<WalRecoveryPrefix, WalError> {
        self.require_lock(lock)?;
        let mut head = WalCommitHead {
            revision: Revision::GENESIS,
            commit_hash: WalCommitHash([0; 32]),
        };
        let mut hashes_by_revision = BTreeMap::from([(Revision::GENESIS, head.commit_hash)]);
        let mut committed_frames = Vec::new();
        let mut finding = None;
        let segments = match self.list_segments_unchecked() {
            Ok(segments) => segments,
            Err(WalError::InvalidSegmentName { .. }) => {
                finding = Some(RecoveryFinding::SafeCorruption {
                    segment_sequence: None,
                    offset: None,
                    kind: RecoveryCorruptionKind::InvalidSegmentName,
                });
                return Ok(WalRecoveryPrefix {
                    head,
                    hashes_by_revision,
                    committed_frames,
                    finding,
                });
            }
            Err(WalError::InvalidSegmentPath { sequence }) => {
                finding = Some(RecoveryFinding::SafeCorruption {
                    segment_sequence: Some(sequence),
                    offset: None,
                    kind: RecoveryCorruptionKind::InvalidSegmentPath,
                });
                return Ok(WalRecoveryPrefix {
                    head,
                    hashes_by_revision,
                    committed_frames,
                    finding,
                });
            }
            Err(error) => return Err(error),
        };

        let mut seen_operations = BTreeSet::new();
        for (index, (sequence, path)) in segments.iter().enumerate() {
            let expected = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or(WalError::SegmentSequenceOverflow)?;
            if *sequence != expected {
                finding = Some(RecoveryFinding::SafeCorruption {
                    segment_sequence: Some(*sequence),
                    offset: Some(0),
                    kind: RecoveryCorruptionKind::SegmentSequenceGap,
                });
                break;
            }

            let bytes = match read_segment_bytes(*sequence, path) {
                Ok(bytes) => bytes,
                Err(WalError::SegmentTooLarge { .. }) => {
                    finding = Some(RecoveryFinding::SafeCorruption {
                        segment_sequence: Some(*sequence),
                        offset: Some(0),
                        kind: RecoveryCorruptionKind::SegmentTooLarge,
                    });
                    break;
                }
                Err(error) => return Err(error),
            };
            let (frames, tail) = decode_raw_segment_prefix(*sequence, &bytes)?;
            let mut pending: Option<WalPreparedFrame> = None;
            for frame in frames {
                match frame {
                    RawWalFrame::Prepare(prepared) => {
                        if pending.is_some() {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(prepared.reference.byte_offset),
                                kind:
                                    RecoveryCorruptionKind::UnexpectedFrameAfterUncommittedPrepare,
                            });
                            break;
                        }
                        if !seen_operations.insert(prepared.reference.operation_id) {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(prepared.reference.byte_offset),
                                kind: RecoveryCorruptionKind::DuplicateOperationId,
                            });
                            break;
                        }
                        pending = Some(prepared);
                    }
                    RawWalFrame::Commit {
                        marker,
                        byte_offset,
                        frame_length,
                    } => {
                        let Some(prepared) = pending.take() else {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(byte_offset),
                                kind: RecoveryCorruptionKind::CommitMarkerWithoutPrepare,
                            });
                            break;
                        };
                        if marker.prepare_reference.segment_sequence != *sequence
                            || marker.prepare_reference.byte_offset >= byte_offset
                            || prepared.reference != marker.prepare_reference
                            || prepared.reference.operation_id != marker.operation_id
                            || prepared
                                .reference
                                .byte_offset
                                .checked_add(prepared.reference.frame_length)
                                != Some(byte_offset)
                        {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(byte_offset),
                                kind: RecoveryCorruptionKind::CommitMarkerWithoutPrepare,
                            });
                            break;
                        }
                        if *blake3::hash(&prepared.payload).as_bytes() != marker.payload_hash {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(byte_offset),
                                kind: RecoveryCorruptionKind::PayloadHashMismatch,
                            });
                            break;
                        }
                        if marker.previous_commit_hash != head.commit_hash {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(byte_offset),
                                kind: RecoveryCorruptionKind::CommitChainMismatch,
                            });
                            break;
                        }
                        let expected_revision = match head.revision.next_commit() {
                            Ok(revision) => revision,
                            Err(_) => {
                                finding = Some(RecoveryFinding::SafeCorruption {
                                    segment_sequence: Some(*sequence),
                                    offset: Some(byte_offset),
                                    kind: RecoveryCorruptionKind::RevisionOverflow,
                                });
                                break;
                            }
                        };
                        if marker.revision != expected_revision {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(byte_offset),
                                kind: RecoveryCorruptionKind::RevisionSequenceMismatch,
                            });
                            break;
                        }
                        let expected_hash = calculate_commit_hash(
                            marker.prepare_reference,
                            marker.revision,
                            marker.operation_id,
                            marker.payload_hash,
                            marker.previous_commit_hash,
                        );
                        if marker.commit_hash.0 != expected_hash {
                            finding = Some(RecoveryFinding::SafeCorruption {
                                segment_sequence: Some(*sequence),
                                offset: Some(byte_offset),
                                kind: RecoveryCorruptionKind::CommitHashMismatch,
                            });
                            break;
                        }
                        head = WalCommitHead {
                            revision: marker.revision,
                            commit_hash: marker.commit_hash,
                        };
                        committed_frames.push(WalCommittedFrame {
                            prepare: prepared,
                            receipt: WalCommitReceipt {
                                reference: marker.prepare_reference,
                                revision: marker.revision,
                                payload_hash: marker.payload_hash,
                                previous_commit_hash: marker.previous_commit_hash,
                                commit_hash: marker.commit_hash,
                                marker_offset: byte_offset,
                                marker_length: frame_length,
                            },
                        });
                        hashes_by_revision.insert(head.revision, head.commit_hash);
                    }
                }
                if finding.is_some() {
                    break;
                }
            }
            if finding.is_some() {
                break;
            }

            if let Some(RawSegmentTail::Corrupt { offset, kind }) = tail {
                finding = Some(RecoveryFinding::SafeCorruption {
                    segment_sequence: Some(*sequence),
                    offset: Some(offset),
                    kind,
                });
                break;
            }

            if let Some(RawSegmentTail::Torn { offset }) = tail {
                if index + 1 < segments.len() {
                    finding = Some(RecoveryFinding::SafeCorruption {
                        segment_sequence: Some(*sequence),
                        offset: Some(offset),
                        kind: RecoveryCorruptionKind::TornTailBeforeLaterSegment,
                    });
                } else {
                    finding = Some(RecoveryFinding::TornTail {
                        segment_sequence: *sequence,
                        offset,
                        pending_prepare_offset: pending
                            .as_ref()
                            .map(|prepared| prepared.reference.byte_offset),
                    });
                }
                break;
            }
            if let Some(prepared) = pending {
                if index + 1 < segments.len() {
                    finding = Some(RecoveryFinding::SafeCorruption {
                        segment_sequence: Some(*sequence),
                        offset: Some(prepared.reference.byte_offset),
                        kind: RecoveryCorruptionKind::UnexpectedFrameAfterUncommittedPrepare,
                    });
                } else {
                    finding = Some(RecoveryFinding::UncommittedTail {
                        segment_sequence: *sequence,
                        prepare_offset: prepared.reference.byte_offset,
                    });
                }
                break;
            }
        }

        Ok(WalRecoveryPrefix {
            head,
            hashes_by_revision,
            committed_frames,
            finding,
        })
    }

    /// Appends one OperationId-bound payload frame and syncs its segment file.
    ///
    /// The writer lock must belong to this database and remain held through
    /// the later commit-marker operation. Reusing an OperationId is rejected;
    /// use [`Self::commit_operation`] to replay an existing committed receipt.
    pub fn append_prepare(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        payload: &[u8],
    ) -> Result<WalPrepareReference, WalError> {
        self.append_prepare_with_sync(lock, operation_id, payload, File::sync_all)
    }

    /// Reads and validates all prepare and commit frames in one numbered segment.
    ///
    /// A prepare paired with a structurally valid marker is returned as
    /// committed; chain-wide revision and predecessor validation is provided
    /// by [`Self::commit_head`].
    pub fn read_segment(
        &self,
        lock: &WriterLock,
        sequence: u64,
    ) -> Result<Vec<WalLogEntry>, WalError> {
        self.require_lock(lock)?;
        if sequence == 0 {
            return Err(WalError::InvalidSegmentName {
                name: segment_file_name(sequence),
            });
        }
        let path = self.segment_path(sequence);
        self.validate_segment_path(sequence, &path)?;
        let bytes = read_segment_bytes(sequence, &path)?;
        decode_segment(sequence, &bytes)
    }

    /// Verifies the complete WAL commit chain and returns its current head.
    pub fn commit_head(&self, lock: &WriterLock) -> Result<WalCommitHead, WalError> {
        self.require_lock(lock)?;
        self.scan_commit_head()
    }

    /// Verifies and seals a committed WAL prefix before a hot snapshot copy.
    ///
    /// The caller holds the writer lock. The active segment is synced and a
    /// fresh empty segment is published when necessary, so later writes append
    /// beyond the returned immutable prefix. Only the prefix through
    /// `expected_revision` is returned, even when an earlier checkpoint left
    /// an empty active segment after it.
    pub fn checkpoint(
        &self,
        lock: &WriterLock,
        expected_revision: Revision,
    ) -> Result<WalCheckpoint, WalError> {
        self.require_lock(lock)?;
        if !lock.require_write_access() {
            return Err(WalError::RecoveryRequired);
        }
        let prefix = self.scan_recovery_prefix(lock)?;
        if prefix.finding.is_some() {
            return Err(WalError::CheckpointRequiresCleanWal);
        }
        if prefix.head.revision != expected_revision {
            return Err(WalError::CheckpointRevisionMismatch {
                expected: expected_revision,
                actual: prefix.head.revision,
            });
        }

        let checkpoint_boundary = if expected_revision == Revision::GENESIS {
            None
        } else {
            let committed = prefix
                .committed_frames
                .iter()
                .find(|frame| frame.receipt().revision() == expected_revision)
                .ok_or(WalError::CheckpointBoundaryMissing {
                    revision: expected_revision,
                })?;
            let receipt = committed.receipt();
            let byte_length = receipt
                .marker_offset()
                .checked_add(receipt.marker_length())
                .ok_or(WalError::CheckpointBoundaryMissing {
                    revision: expected_revision,
                })?;
            Some((receipt.reference().segment_sequence(), byte_length))
        };

        let all_segments = self.list_segments()?;
        for (_, path) in &all_segments {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|source| WalError::Io {
                    operation: "open WAL segment for checkpoint sync",
                    source,
                })?;
            file.sync_all().map_err(|source| WalError::Io {
                operation: "sync WAL checkpoint segment",
                source,
            })?;
        }

        if let Some((last_sequence, last_path)) = all_segments.last() {
            let length = fs::metadata(last_path)
                .map_err(|source| WalError::Io {
                    operation: "inspect active WAL checkpoint segment",
                    source,
                })?
                .len();
            if length > 0 {
                let next_sequence = last_sequence
                    .checked_add(1)
                    .ok_or(WalError::SegmentSequenceOverflow)?;
                let next_path = self.segment_path(next_sequence);
                let next_file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&next_path)
                    .map_err(|source| WalError::Io {
                        operation: "publish next WAL segment at checkpoint",
                        source,
                    })?;
                next_file.sync_all().map_err(|source| WalError::Io {
                    operation: "sync next WAL segment at checkpoint",
                    source,
                })?;
                drop(next_file);
                crate::manifest::sync_directory(&self.directory).map_err(|source| {
                    WalError::Io {
                        operation: "sync WAL directory at checkpoint",
                        source,
                    }
                })?;
            }
        }

        let mut segments = Vec::new();
        if let Some((boundary_sequence, boundary_length)) = checkpoint_boundary {
            let segment_count =
                usize::try_from(boundary_sequence).map_err(|_| WalError::AllocationFailed)?;
            segments
                .try_reserve_exact(segment_count)
                .map_err(|_| WalError::AllocationFailed)?;
            for (sequence, path) in all_segments {
                if sequence > boundary_sequence {
                    break;
                }
                let actual_length = fs::metadata(path)
                    .map_err(|source| WalError::Io {
                        operation: "inspect closed WAL checkpoint segment",
                        source,
                    })?
                    .len();
                let byte_length = if sequence == boundary_sequence {
                    if actual_length != boundary_length {
                        return Err(WalError::CheckpointBoundaryMissing {
                            revision: expected_revision,
                        });
                    }
                    boundary_length
                } else {
                    actual_length
                };
                segments.push(WalCheckpointSegment {
                    sequence,
                    byte_length,
                });
            }
            if segments.last().map(|segment| segment.sequence) != Some(boundary_sequence) {
                return Err(WalError::CheckpointBoundaryMissing {
                    revision: expected_revision,
                });
            }
        }

        Ok(WalCheckpoint {
            revision: expected_revision,
            commit_hash: prefix.head.commit_hash,
            segments,
        })
    }

    /// Verifies the complete WAL commit chain and returns the hash at exactly
    /// `revision`, if that revision is already committed.
    ///
    /// The full chain is scanned even when the requested revision is older
    /// than the head. A manifest publisher can therefore bind a lagging
    /// snapshot to its real commit hash without treating an unverified prefix
    /// as authoritative.
    pub fn verified_head_at(
        &self,
        lock: &WriterLock,
        revision: Revision,
    ) -> Result<Option<WalCommitHead>, WalError> {
        self.require_lock(lock)?;
        let (head, requested_head) = self.scan_commit_chain(Some(revision))?;
        Ok(if revision > head.revision {
            None
        } else {
            requested_head
        })
    }

    /// Reconstructs one OperationId's durable status from the complete WAL.
    ///
    /// `NotCommitted` is returned only after every segment and commit-chain
    /// entry validates. A synced prepare without a verified marker is
    /// `Indeterminate` until recovery or reconciliation resolves it.
    pub fn operation_status(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
    ) -> Result<WalOperationStatus, WalError> {
        self.require_lock(lock)?;
        if self
            .indeterminate_operation_fingerprint(operation_id)
            .is_some()
        {
            return Ok(WalOperationStatus::Indeterminate);
        }
        let index = self.scan_operation_index(lock)?;
        Ok(match index.get(&operation_id).map(|entry| entry.status) {
            Some(IndexedWalOperationStatus::Committed(receipt)) => {
                WalOperationStatus::Committed(receipt)
            }
            Some(IndexedWalOperationStatus::Indeterminate) => WalOperationStatus::Indeterminate,
            None => WalOperationStatus::NotCommitted,
        })
    }

    /// Commits a new OperationId or replays its original committed receipt.
    ///
    /// A matching committed payload returns the same receipt without appending
    /// another prepare. Reuse with different bytes fails as
    /// `IdempotencyMismatch`; an unresolved prepare remains indeterminate and
    /// cannot be executed again. `canonical_payload` is the exact request byte
    /// sequence whose BLAKE3 fingerprint is bound to the OperationId. The
    /// database writer lock is mandatory.
    pub fn commit_operation(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        canonical_payload: &[u8],
    ) -> Result<WalCommitReceipt, WalError> {
        self.require_lock(lock)?;
        if !lock.require_write_access() {
            return Err(WalError::RecoveryRequired);
        }
        let payload_fingerprint = *blake3::hash(canonical_payload).as_bytes();
        if let Some(uncertain_fingerprint) = self.indeterminate_operation_fingerprint(operation_id)
        {
            if uncertain_fingerprint != payload_fingerprint {
                return Err(WalError::IdempotencyMismatch { operation_id });
            }
            return Err(WalError::OperationIndeterminate { operation_id });
        }
        let index = self.scan_operation_index(lock)?;
        if let Some(entry) = index.get(&operation_id) {
            if entry.payload_fingerprint != payload_fingerprint {
                return Err(WalError::IdempotencyMismatch { operation_id });
            }
            return match entry.status {
                IndexedWalOperationStatus::Committed(receipt) => Ok(receipt),
                IndexedWalOperationStatus::Indeterminate => {
                    Err(WalError::OperationIndeterminate { operation_id })
                }
            };
        }
        self.commit_new_operation(lock, operation_id, canonical_payload)
    }

    /// Commits a full manifest snapshot as a replayable WAL payload.
    ///
    /// `staged_segments` must be immutable files produced by the corresponding
    /// history or security-policy staging store. They are published only after
    /// this method's WAL commitpoint is verified. Referenced segments omitted
    /// from `staged_segments` must already exist at their immutable final paths.
    pub fn commit_manifest_snapshot(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        segments: Vec<ManifestSegmentReference>,
        staged_segments: &[ManifestSegmentReference],
    ) -> Result<WalCommitReceipt, SnapshotCommitError> {
        let payload =
            self.encode_manifest_snapshot_payload(lock, operation_id, segments, staged_segments)?;
        self.commit_operation(lock, operation_id, &payload)
            .map_err(SnapshotCommitError::Wal)
    }

    /// Commits a manifest snapshot and its Required Audit Record in the same WAL prepare and
    /// commit marker. The audit context must name the revision assigned to this OperationId.
    pub fn commit_audited_manifest_snapshot(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        segments: Vec<ManifestSegmentReference>,
        staged_segments: &[ManifestSegmentReference],
        audit_record: &AuditRecord,
    ) -> Result<WalCommitReceipt, SnapshotCommitError> {
        let payload =
            self.encode_manifest_snapshot_payload(lock, operation_id, segments, staged_segments)?;
        self.commit_required_audit(lock, operation_id, &payload, audit_record)
            .map_err(SnapshotCommitError::RequiredAudit)
    }

    /// Commits a non-empty canonical domain-action payload and Required Audit Record as one
    /// prepare/commit-marker durability unit.
    pub fn commit_required_audit(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        canonical_action_payload: &[u8],
        audit_record: &AuditRecord,
    ) -> Result<WalCommitReceipt, RequiredAuditError> {
        self.require_lock(lock).map_err(RequiredAuditError::Wal)?;
        if !lock.require_write_access() {
            return Err(RequiredAuditError::Wal(WalError::RecoveryRequired));
        }
        let status = self
            .operation_status(lock, operation_id)
            .map_err(RequiredAuditError::Wal)?;
        let target_revision = match status {
            WalOperationStatus::Committed(receipt) => receipt.revision,
            WalOperationStatus::NotCommitted | WalOperationStatus::Indeterminate => self
                .commit_head(lock)
                .and_then(|head| head.revision.next_commit().map_err(WalError::Revision))
                .map_err(RequiredAuditError::Wal)?,
        };
        validate_commit_binding(audit_record, target_revision, operation_id)?;
        if matches!(status, WalOperationStatus::NotCommitted) {
            let committed = records_for_log(self, lock)?;
            validate_sequence_after(audit_record, latest_sequence(&committed))?;
        }
        let payload = encode_required_audit_payload(canonical_action_payload, audit_record)?;
        self.commit_operation(lock, operation_id, &payload)
            .map_err(RequiredAuditError::Wal)
    }

    /// Reads every Required Audit Record whose operation and commit marker verify.
    /// Records are returned in committed revision order; an incomplete or malformed WAL is
    /// rejected instead of returning a partial audit view.
    pub fn committed_required_audit_records(
        &self,
        lock: &WriterLock,
    ) -> Result<Vec<CommittedRequiredAuditRecord>, RequiredAuditError> {
        let records = records_for_log(self, lock)?;
        Ok(records)
    }

    fn encode_manifest_snapshot_payload(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        segments: Vec<ManifestSegmentReference>,
        staged_segments: &[ManifestSegmentReference],
    ) -> Result<Vec<u8>, SnapshotCommitError> {
        self.require_lock(lock).map_err(SnapshotCommitError::Wal)?;
        if !lock.require_write_access() {
            return Err(SnapshotCommitError::Wal(WalError::RecoveryRequired));
        }
        let status = self
            .operation_status(lock, operation_id)
            .map_err(SnapshotCommitError::Wal)?;
        let target_revision = match status {
            WalOperationStatus::Committed(receipt) => receipt.revision,
            WalOperationStatus::NotCommitted | WalOperationStatus::Indeterminate => self
                .commit_head(lock)
                .and_then(|head| head.revision.next_commit().map_err(WalError::Revision))
                .map_err(SnapshotCommitError::Wal)?,
        };
        let snapshot = ManifestSnapshot::new(target_revision, segments)
            .map_err(SnapshotCommitError::Manifest)?;
        let payload = encode_replay_payload(&snapshot, staged_segments)?;

        // The original committed operation is the idempotency authority. Recovery may already
        // have moved its staged files to their immutable final paths, so retries compare the
        // original canonical bytes without requiring staging paths to remain present.
        if matches!(status, WalOperationStatus::Committed(_)) {
            return Ok(payload);
        }

        let history = HistorySegmentStore::new(self.layout.clone());
        let security = SecurityPolicyHistoryStore::new(self.layout.clone());
        for reference in snapshot.segments() {
            if staged_segments.contains(reference) {
                match reference.kind() {
                    ManifestSegmentKind::History => history
                        .validate_staged_reference(*reference)
                        .map_err(SnapshotCommitError::HistorySegment)?,
                    ManifestSegmentKind::SecurityPolicy => security
                        .validate_staged_reference(*reference)
                        .map_err(SnapshotCommitError::SecuritySegment)?,
                }
            } else {
                match reference.kind() {
                    ManifestSegmentKind::History => history
                        .validate_manifest_reference(*reference)
                        .map_err(SnapshotCommitError::HistorySegment)?,
                    ManifestSegmentKind::SecurityPolicy => security
                        .validate_manifest_reference(*reference)
                        .map_err(SnapshotCommitError::SecuritySegment)?,
                }
            }
        }
        Ok(payload)
    }

    fn commit_new_operation(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        canonical_payload: &[u8],
    ) -> Result<WalCommitReceipt, WalError> {
        let reference = self.append_prepare(lock, operation_id, canonical_payload)?;
        self.commit_prepared(lock, reference)
    }

    /// Appends the commit marker for the segment's last prepare and syncs it.
    ///
    /// The returned receipt exists only after the second WAL sync succeeds.
    /// If marker append or sync fails, the result is an unknown commit outcome
    /// and the caller must reconcile this OperationId before retrying.
    pub fn commit_prepared(
        &self,
        lock: &WriterLock,
        reference: WalPrepareReference,
    ) -> Result<WalCommitReceipt, WalError> {
        self.commit_prepared_with_sync(lock, reference, File::sync_all)
    }

    fn append_prepare_with_sync(
        &self,
        lock: &WriterLock,
        operation_id: OperationId,
        payload: &[u8],
        sync: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<WalPrepareReference, WalError> {
        self.require_lock(lock)?;
        if !lock.require_write_access() {
            return Err(WalError::RecoveryRequired);
        }
        let payload_fingerprint = *blake3::hash(payload).as_bytes();
        if let Some(uncertain_fingerprint) = self.indeterminate_operation_fingerprint(operation_id)
        {
            if uncertain_fingerprint != payload_fingerprint {
                return Err(WalError::IdempotencyMismatch { operation_id });
            }
            return Err(WalError::OperationIndeterminate { operation_id });
        }
        let operation_index = self.scan_operation_index(lock)?;
        if let Some(existing) = operation_index.get(&operation_id) {
            if existing.payload_fingerprint != payload_fingerprint {
                return Err(WalError::IdempotencyMismatch { operation_id });
            }
            return Err(match existing.status {
                IndexedWalOperationStatus::Committed(_) => {
                    WalError::OperationAlreadyCommitted { operation_id }
                }
                IndexedWalOperationStatus::Indeterminate => {
                    WalError::OperationIndeterminate { operation_id }
                }
            });
        }
        let frame = encode_prepare(operation_id, payload)?;
        let frame_length = u64::try_from(frame.len()).map_err(|_| WalError::FrameTooLarge {
            limit: MAX_WAL_FRAME_BYTES,
            actual: usize::MAX,
        })?;
        let segments = self.list_segments()?;

        let (sequence, path, create_new) = match segments.last() {
            Some((last_sequence, last_path)) => {
                let metadata = fs::metadata(last_path).map_err(|source| WalError::Io {
                    operation: "inspect current WAL segment",
                    source,
                })?;
                let current_length = metadata.len();
                let would_exceed_target = current_length
                    .checked_add(frame_length)
                    .is_none_or(|length| length > TARGET_SEGMENT_BYTES);
                if current_length > 0
                    && (frame_length > TARGET_SEGMENT_BYTES || would_exceed_target)
                {
                    let sequence = last_sequence
                        .checked_add(1)
                        .ok_or(WalError::SegmentSequenceOverflow)?;
                    (sequence, self.segment_path(sequence), true)
                } else {
                    (*last_sequence, last_path.clone(), false)
                }
            }
            None => (1, self.segment_path(1), true),
        };

        let mut options = OpenOptions::new();
        options.append(true).read(true);
        if create_new {
            options.create_new(true);
        }
        let mut file = options.open(&path).map_err(|source| WalError::Io {
            operation: "open WAL segment for append",
            source,
        })?;
        self.validate_segment_path(sequence, &path)?;
        let current_length = file
            .metadata()
            .map_err(|source| WalError::Io {
                operation: "inspect open WAL segment",
                source,
            })?
            .len();
        let byte_offset = current_length;
        if current_length
            .checked_add(frame_length)
            .is_none_or(|length| length > u64::try_from(MAX_WAL_FRAME_BYTES).unwrap_or(u64::MAX))
        {
            return Err(WalError::SegmentTooLarge {
                limit: u64::try_from(MAX_WAL_FRAME_BYTES).unwrap_or(u64::MAX),
                actual: current_length.saturating_add(frame_length),
            });
        }
        append_frame_and_sync(&mut file, &frame, sync)?;
        drop(file);

        Ok(WalPrepareReference {
            segment_sequence: sequence,
            byte_offset,
            frame_length,
            operation_id,
        })
    }

    fn commit_prepared_with_sync(
        &self,
        lock: &WriterLock,
        reference: WalPrepareReference,
        sync: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<WalCommitReceipt, WalError> {
        self.require_lock(lock)?;
        if !lock.require_write_access() {
            return Err(WalError::RecoveryRequired);
        }
        if self
            .indeterminate_operation_fingerprint(reference.operation_id)
            .is_some()
        {
            return Err(WalError::OperationIndeterminate {
                operation_id: reference.operation_id,
            });
        }
        let head = self.scan_commit_head()?;
        let entries = self.read_segment(lock, reference.segment_sequence)?;
        let prepared = entries.iter().find_map(|entry| match entry {
            WalLogEntry::PreparedUncommitted(frame) if frame.reference == reference => {
                Some(frame.clone())
            }
            _ => None,
        });
        let prepared = prepared.ok_or(WalError::PrepareNotAppendTail)?;
        let path = self.segment_path(reference.segment_sequence);
        self.validate_segment_path(reference.segment_sequence, &path)?;
        let expected_marker_offset = reference
            .byte_offset
            .checked_add(reference.frame_length)
            .ok_or(WalError::PrepareNotAppendTail)?;
        let next_revision = head.revision.next_commit().map_err(WalError::Revision)?;
        let payload_hash = *blake3::hash(&prepared.payload).as_bytes();
        let commit_hash = calculate_commit_hash(
            reference,
            next_revision,
            reference.operation_id,
            payload_hash,
            head.commit_hash,
        );
        let marker = WalCommitMarker {
            prepare_reference: reference,
            operation_id: reference.operation_id,
            revision: next_revision,
            payload_hash,
            previous_commit_hash: head.commit_hash,
            commit_hash: WalCommitHash(commit_hash),
        };
        let marker_frame = encode_commit_marker(&marker)?;
        let marker_length =
            u64::try_from(marker_frame.len()).map_err(|_| WalError::FrameTooLarge {
                limit: MAX_WAL_FRAME_BYTES,
                actual: usize::MAX,
            })?;

        let mut file = OpenOptions::new()
            .append(true)
            .read(true)
            .open(&path)
            .map_err(|source| WalError::Io {
                operation: "open WAL segment for commit marker",
                source,
            })?;
        self.validate_segment_path(reference.segment_sequence, &path)?;
        let marker_offset = file
            .metadata()
            .map_err(|source| WalError::Io {
                operation: "inspect WAL segment before commit marker",
                source,
            })?
            .len();
        if marker_offset != expected_marker_offset {
            return Err(WalError::PrepareNotAppendTail);
        }
        if let Err(error) =
            append_commit_marker_and_sync(&mut file, &marker_frame, reference.operation_id, sync)
        {
            if matches!(&error, WalError::UnknownCommitOutcome { .. }) {
                self.mark_operation_indeterminate_in_process(reference.operation_id, payload_hash);
            }
            return Err(error);
        }
        drop(file);

        Ok(WalCommitReceipt {
            reference,
            revision: next_revision,
            payload_hash,
            previous_commit_hash: head.commit_hash,
            commit_hash: WalCommitHash(commit_hash),
            marker_offset,
            marker_length,
        })
    }

    fn scan_commit_head(&self) -> Result<WalCommitHead, WalError> {
        self.scan_commit_chain(None).map(|(head, _)| head)
    }

    fn scan_commit_chain(
        &self,
        requested_revision: Option<Revision>,
    ) -> Result<(WalCommitHead, Option<WalCommitHead>), WalError> {
        let mut head = WalCommitHead {
            revision: Revision::GENESIS,
            commit_hash: WalCommitHash([0; 32]),
        };
        let mut requested_head = requested_revision
            .filter(|revision| *revision == Revision::GENESIS)
            .map(|_| head);
        for (sequence, path) in self.list_segments()? {
            let bytes = read_segment_bytes(sequence, &path)?;
            let frames = decode_raw_segment(sequence, &bytes)?;
            let mut pending = None;
            for frame in frames {
                match frame {
                    RawWalFrame::Prepare(prepared) => {
                        let reference = prepared.reference;
                        pending = Some((
                            reference,
                            reference.operation_id,
                            *blake3::hash(&prepared.payload).as_bytes(),
                        ));
                    }
                    RawWalFrame::Commit {
                        marker,
                        byte_offset,
                        ..
                    } => {
                        if marker.prepare_reference.segment_sequence != sequence
                            || marker.prepare_reference.byte_offset >= byte_offset
                        {
                            return Err(WalError::CommitMarkerWithoutPrepare {
                                sequence,
                                offset: byte_offset,
                            });
                        }
                        let Some((prepare_reference, operation_id, payload_hash)) = pending.take()
                        else {
                            return Err(WalError::CommitMarkerWithoutPrepare {
                                sequence,
                                offset: byte_offset,
                            });
                        };
                        if prepare_reference != marker.prepare_reference
                            || operation_id != marker.operation_id
                            || prepare_reference
                                .byte_offset
                                .checked_add(prepare_reference.frame_length)
                                != Some(byte_offset)
                        {
                            return Err(WalError::CommitMarkerWithoutPrepare {
                                sequence,
                                offset: byte_offset,
                            });
                        }
                        if payload_hash != marker.payload_hash {
                            return Err(WalError::PayloadHashMismatch {
                                sequence,
                                offset: byte_offset,
                            });
                        }
                        if marker.previous_commit_hash != head.commit_hash {
                            return Err(WalError::CommitChainMismatch {
                                sequence,
                                offset: byte_offset,
                            });
                        }
                        let expected_revision =
                            head.revision.next_commit().map_err(WalError::Revision)?;
                        if marker.revision != expected_revision {
                            return Err(WalError::RevisionSequenceMismatch {
                                expected: expected_revision,
                                actual: marker.revision,
                            });
                        }
                        let expected_hash = calculate_commit_hash(
                            marker.prepare_reference,
                            marker.revision,
                            marker.operation_id,
                            marker.payload_hash,
                            marker.previous_commit_hash,
                        );
                        if marker.commit_hash.0 != expected_hash {
                            return Err(WalError::CommitHashMismatch {
                                sequence,
                                offset: byte_offset,
                            });
                        }
                        head = WalCommitHead {
                            revision: marker.revision,
                            commit_hash: marker.commit_hash,
                        };
                        if requested_revision == Some(head.revision) {
                            requested_head = Some(head);
                        }
                    }
                }
            }
        }
        Ok((head, requested_head))
    }

    fn scan_operation_index(
        &self,
        lock: &WriterLock,
    ) -> Result<BTreeMap<OperationId, IndexedWalOperation>, WalError> {
        self.require_lock(lock)?;
        self.scan_commit_head()?;
        let mut operations = BTreeMap::new();
        for (sequence, _) in self.list_segments()? {
            for entry in self.read_segment(lock, sequence)? {
                let (operation_id, payload_fingerprint, status) = match entry {
                    WalLogEntry::PreparedUncommitted(prepare) => (
                        prepare.reference.operation_id,
                        *blake3::hash(&prepare.payload).as_bytes(),
                        IndexedWalOperationStatus::Indeterminate,
                    ),
                    WalLogEntry::Committed(committed) => (
                        committed.prepare.reference.operation_id,
                        *blake3::hash(&committed.prepare.payload).as_bytes(),
                        IndexedWalOperationStatus::Committed(committed.receipt),
                    ),
                };
                if operations.contains_key(&operation_id) {
                    return Err(WalError::DuplicateOperationId { operation_id });
                }
                operations.insert(
                    operation_id,
                    IndexedWalOperation {
                        payload_fingerprint,
                        status,
                    },
                );
            }
        }
        Ok(operations)
    }

    pub(crate) fn verified_operation_statuses(
        &self,
        lock: &WriterLock,
    ) -> Result<BTreeMap<OperationId, WalOperationStatus>, WalError> {
        self.scan_operation_index(lock).map(|operations| {
            operations
                .into_iter()
                .map(|(operation_id, operation)| {
                    let status = match operation.status {
                        IndexedWalOperationStatus::Committed(receipt) => {
                            WalOperationStatus::Committed(receipt)
                        }
                        IndexedWalOperationStatus::Indeterminate => {
                            WalOperationStatus::Indeterminate
                        }
                    };
                    (operation_id, status)
                })
                .collect()
        })
    }

    fn require_lock(&self, lock: &WriterLock) -> Result<(), WalError> {
        if lock.belongs_to_database_root(&self.root) {
            Ok(())
        } else {
            Err(WalError::WriterLockMismatch)
        }
    }

    fn indeterminate_operation_fingerprint(&self, operation_id: OperationId) -> Option<[u8; 32]> {
        indeterminate_commit_ids()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&(self.root.clone(), operation_id))
            .copied()
    }

    fn mark_operation_indeterminate_in_process(
        &self,
        operation_id: OperationId,
        payload_fingerprint: [u8; 32],
    ) {
        indeterminate_commit_ids()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((self.root.clone(), operation_id), payload_fingerprint);
    }

    fn list_segments(&self) -> Result<Vec<(u64, PathBuf)>, WalError> {
        let segments = self.list_segments_unchecked()?;
        let mut expected = 1_u64;
        for (index, (sequence, _)) in segments.iter().enumerate() {
            if *sequence != expected {
                return Err(WalError::SegmentSequenceGap {
                    expected,
                    actual: *sequence,
                });
            }
            if index + 1 < segments.len() {
                expected = expected
                    .checked_add(1)
                    .ok_or(WalError::SegmentSequenceOverflow)?;
            }
        }
        Ok(segments)
    }

    fn list_segments_unchecked(&self) -> Result<Vec<(u64, PathBuf)>, WalError> {
        let entries = fs::read_dir(&self.directory).map_err(|source| WalError::Io {
            operation: "list WAL segments",
            source,
        })?;
        let mut segments = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| WalError::Io {
                operation: "read WAL directory entry",
                source,
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let sequence = parse_segment_sequence(&name)?;
            let path = entry.path();
            self.validate_segment_path(sequence, &path)?;
            segments.push((sequence, path));
        }
        segments.sort_by_key(|(sequence, _)| *sequence);
        Ok(segments)
    }

    fn validate_segment_path(&self, sequence: u64, path: &Path) -> Result<(), WalError> {
        let metadata = fs::symlink_metadata(path).map_err(|source| WalError::Io {
            operation: "inspect WAL segment path",
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(WalError::InvalidSegmentPath { sequence });
        }
        let canonical = fs::canonicalize(path).map_err(|source| WalError::Io {
            operation: "resolve WAL segment path",
            source,
        })?;
        let canonical_directory =
            fs::canonicalize(&self.directory).map_err(|source| WalError::Io {
                operation: "resolve WAL segment directory",
                source,
            })?;
        if canonical.parent() != Some(canonical_directory.as_path())
            || !canonical.starts_with(&self.root)
        {
            return Err(WalError::InvalidSegmentPath { sequence });
        }
        Ok(())
    }

    fn segment_path(&self, sequence: u64) -> PathBuf {
        self.directory.join(segment_file_name(sequence))
    }
}

fn encode_prepare(operation_id: OperationId, payload: &[u8]) -> Result<Vec<u8>, WalError> {
    let mut encoder = TlvEncoder::new();
    encoder
        .push(OPERATION_ID_FIELD_TAG, &operation_id.to_bytes())
        .map_err(WalError::Wire)?;
    encoder
        .push(PAYLOAD_FIELD_TAG, payload)
        .map_err(WalError::Wire)?;
    let payload = encoder.finish();
    let frame = encode_frame(FrameHeader::new(WAL_PREPARE_FRAME_KIND), &payload)
        .map_err(WalError::Frame)?;
    if frame.len() > MAX_WAL_FRAME_BYTES {
        return Err(WalError::FrameTooLarge {
            limit: MAX_WAL_FRAME_BYTES,
            actual: frame.len(),
        });
    }
    Ok(frame)
}

pub(crate) fn segment_file_name(sequence: u64) -> String {
    format!("{SEGMENT_PREFIX}{sequence:0SEGMENT_NUMBER_WIDTH$}{SEGMENT_SUFFIX}")
}

fn parse_segment_sequence(name: &str) -> Result<u64, WalError> {
    let numeric = name
        .strip_prefix(SEGMENT_PREFIX)
        .and_then(|value| value.strip_suffix(SEGMENT_SUFFIX))
        .ok_or_else(|| WalError::InvalidSegmentName {
            name: name.to_owned(),
        })?;
    if numeric.len() != SEGMENT_NUMBER_WIDTH || !numeric.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(WalError::InvalidSegmentName {
            name: name.to_owned(),
        });
    }
    let sequence = numeric
        .parse::<u64>()
        .map_err(|_| WalError::InvalidSegmentName {
            name: name.to_owned(),
        })?;
    if sequence == 0 {
        return Err(WalError::InvalidSegmentName {
            name: name.to_owned(),
        });
    }
    Ok(sequence)
}

fn indeterminate_commit_ids() -> &'static Mutex<IndeterminateOperationMap> {
    INDETERMINATE_COMMIT_IDS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn read_segment_bytes(_sequence: u64, path: &Path) -> Result<Vec<u8>, WalError> {
    let limit = u64::try_from(MAX_WAL_FRAME_BYTES).unwrap_or(u64::MAX);
    let file = File::open(path).map_err(|source| WalError::Io {
        operation: "open WAL segment for readback",
        source,
    })?;
    let file_length = file
        .metadata()
        .map_err(|source| WalError::Io {
            operation: "inspect WAL segment for readback",
            source,
        })?
        .len();
    if file_length > limit {
        return Err(WalError::SegmentTooLarge {
            limit,
            actual: file_length,
        });
    }
    let capacity = usize::try_from(file_length).map_err(|_| WalError::SegmentTooLarge {
        limit,
        actual: file_length,
    })?;
    let read_limit = limit.checked_add(1).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| WalError::AllocationFailed)?;
    let mut file = file;
    Read::by_ref(&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|source| WalError::Io {
            operation: "read WAL segment for readback",
            source,
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(WalError::SegmentTooLarge {
            limit,
            actual: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        });
    }
    Ok(bytes)
}

fn decode_segment(sequence: u64, bytes: &[u8]) -> Result<Vec<WalLogEntry>, WalError> {
    let raw_frames = decode_raw_segment(sequence, bytes)?;
    let mut entries = Vec::<Option<WalLogEntry>>::new();
    let mut pending = None;
    for raw in raw_frames {
        match raw {
            RawWalFrame::Prepare(prepared) => {
                let entry_index = entries.len();
                entries
                    .try_reserve(1)
                    .map_err(|_| WalError::AllocationFailed)?;
                entries.push(Some(WalLogEntry::PreparedUncommitted(prepared)));
                pending = Some(entry_index);
            }
            RawWalFrame::Commit {
                marker,
                byte_offset,
                frame_length,
            } => {
                let Some(entry_index) = pending.take() else {
                    return Err(WalError::CommitMarkerWithoutPrepare {
                        sequence,
                        offset: byte_offset,
                    });
                };
                if marker.prepare_reference.segment_sequence != sequence
                    || marker.prepare_reference.byte_offset >= byte_offset
                {
                    return Err(WalError::CommitMarkerWithoutPrepare {
                        sequence,
                        offset: byte_offset,
                    });
                }
                let entry_slot =
                    entries
                        .get_mut(entry_index)
                        .ok_or(WalError::CommitMarkerWithoutPrepare {
                            sequence,
                            offset: byte_offset,
                        })?;
                let Some(WalLogEntry::PreparedUncommitted(prepared)) = entry_slot.as_ref() else {
                    return Err(WalError::CommitMarkerWithoutPrepare {
                        sequence,
                        offset: byte_offset,
                    });
                };
                if prepared.reference != marker.prepare_reference
                    || prepared.reference.operation_id != marker.operation_id
                    || prepared
                        .reference
                        .byte_offset
                        .checked_add(prepared.reference.frame_length)
                        != Some(byte_offset)
                {
                    return Err(WalError::CommitMarkerWithoutPrepare {
                        sequence,
                        offset: byte_offset,
                    });
                }
                let actual_payload_hash = *blake3::hash(&prepared.payload).as_bytes();
                if actual_payload_hash != marker.payload_hash {
                    return Err(WalError::PayloadHashMismatch {
                        sequence,
                        offset: byte_offset,
                    });
                }
                let expected_hash = calculate_commit_hash(
                    marker.prepare_reference,
                    marker.revision,
                    marker.operation_id,
                    marker.payload_hash,
                    marker.previous_commit_hash,
                );
                if expected_hash != marker.commit_hash.0 {
                    return Err(WalError::CommitHashMismatch {
                        sequence,
                        offset: byte_offset,
                    });
                }
                let marker_receipt = WalCommitReceipt {
                    reference: marker.prepare_reference,
                    revision: marker.revision,
                    payload_hash: marker.payload_hash,
                    previous_commit_hash: marker.previous_commit_hash,
                    commit_hash: marker.commit_hash,
                    marker_offset: byte_offset,
                    marker_length: frame_length,
                };
                *entry_slot = Some(WalLogEntry::Committed(WalCommittedFrame {
                    prepare: prepared.clone(),
                    receipt: marker_receipt,
                }));
            }
        }
    }
    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(entries.len())
        .map_err(|_| WalError::AllocationFailed)?;
    for entry in entries.into_iter().flatten() {
        decoded.push(entry);
    }
    Ok(decoded)
}

fn decode_raw_segment(sequence: u64, bytes: &[u8]) -> Result<Vec<RawWalFrame>, WalError> {
    let mut frames = Vec::new();
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let header_end = offset
            .checked_add(FRAME_HEADER_LEN)
            .ok_or(WalError::TornTail {
                sequence,
                offset: u64::try_from(offset).unwrap_or(u64::MAX),
            })?;
        let length_start = offset.checked_add(32).ok_or(WalError::TornTail {
            sequence,
            offset: u64::try_from(offset).unwrap_or(u64::MAX),
        })?;
        let length_end = length_start.checked_add(8).ok_or(WalError::TornTail {
            sequence,
            offset: u64::try_from(offset).unwrap_or(u64::MAX),
        })?;
        let Some(length_bytes) = bytes.get(length_start..length_end) else {
            return Err(WalError::TornTail {
                sequence,
                offset: u64::try_from(offset).unwrap_or(u64::MAX),
            });
        };
        if bytes.get(offset..header_end).is_none() {
            return Err(WalError::TornTail {
                sequence,
                offset: u64::try_from(offset).unwrap_or(u64::MAX),
            });
        }
        let payload_length =
            u64::from_le_bytes(length_bytes.try_into().map_err(|_| WalError::TornTail {
                sequence,
                offset: u64::try_from(offset).unwrap_or(u64::MAX),
            })?);
        let payload_length =
            usize::try_from(payload_length).map_err(|_| WalError::FrameTooLarge {
                limit: MAX_WAL_FRAME_BYTES,
                actual: usize::MAX,
            })?;
        let frame_length = FRAME_HEADER_LEN
            .checked_add(payload_length)
            .and_then(|length| length.checked_add(CHECKSUM_LEN))
            .ok_or(WalError::FrameTooLarge {
                limit: MAX_WAL_FRAME_BYTES,
                actual: usize::MAX,
            })?;
        if frame_length > MAX_WAL_FRAME_BYTES {
            return Err(WalError::FrameTooLarge {
                limit: MAX_WAL_FRAME_BYTES,
                actual: frame_length,
            });
        }
        let frame_end = offset.checked_add(frame_length).ok_or(WalError::TornTail {
            sequence,
            offset: u64::try_from(offset).unwrap_or(u64::MAX),
        })?;
        let Some(frame_bytes) = bytes.get(offset..frame_end) else {
            return Err(WalError::TornTail {
                sequence,
                offset: u64::try_from(offset).unwrap_or(u64::MAX),
            });
        };
        let offset_u64 = u64::try_from(offset).unwrap_or(u64::MAX);
        let frame = decode_frame(frame_bytes).map_err(WalError::Frame)?;
        let frame_length_u64 =
            u64::try_from(frame_length).map_err(|_| WalError::FrameTooLarge {
                limit: MAX_WAL_FRAME_BYTES,
                actual: frame_length,
            })?;
        let raw = match frame.header().kind() {
            WAL_PREPARE_FRAME_KIND => {
                let (operation_id, payload) =
                    decode_prepare_payload(sequence, offset_u64, frame.payload())?;
                RawWalFrame::Prepare(WalPreparedFrame {
                    reference: WalPrepareReference {
                        segment_sequence: sequence,
                        byte_offset: offset_u64,
                        frame_length: frame_length_u64,
                        operation_id,
                    },
                    payload,
                })
            }
            WAL_COMMIT_FRAME_KIND => RawWalFrame::Commit {
                marker: decode_commit_payload(sequence, offset_u64, frame.payload())?,
                byte_offset: offset_u64,
                frame_length: frame_length_u64,
            },
            kind => {
                return Err(WalError::UnsupportedFrameKind {
                    sequence,
                    offset: offset_u64,
                    kind,
                });
            }
        };
        frames
            .try_reserve(1)
            .map_err(|_| WalError::AllocationFailed)?;
        frames.push(raw);
        offset = frame_end;
    }
    Ok(frames)
}

fn decode_raw_segment_prefix(
    sequence: u64,
    bytes: &[u8],
) -> Result<(Vec<RawWalFrame>, Option<RawSegmentTail>), WalError> {
    let mut frames = Vec::new();
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let offset_u64 = u64::try_from(offset).unwrap_or(u64::MAX);
        let Some(length_start) = offset.checked_add(32) else {
            return Ok((
                frames,
                Some(RawSegmentTail::Corrupt {
                    offset: offset_u64,
                    kind: RecoveryCorruptionKind::FrameTooLarge,
                }),
            ));
        };
        let Some(length_end) = length_start.checked_add(8) else {
            return Ok((
                frames,
                Some(RawSegmentTail::Corrupt {
                    offset: offset_u64,
                    kind: RecoveryCorruptionKind::FrameTooLarge,
                }),
            ));
        };
        let Some(length_bytes) = bytes.get(length_start..length_end) else {
            return Ok((frames, Some(RawSegmentTail::Torn { offset: offset_u64 })));
        };
        if bytes
            .get(offset..offset.saturating_add(FRAME_HEADER_LEN))
            .is_none()
        {
            return Ok((frames, Some(RawSegmentTail::Torn { offset: offset_u64 })));
        }
        let payload_length =
            u64::from_le_bytes(length_bytes.try_into().map_err(|_| WalError::TornTail {
                sequence,
                offset: offset_u64,
            })?);
        let payload_length = match usize::try_from(payload_length) {
            Ok(length) => length,
            Err(_) => {
                return Ok((
                    frames,
                    Some(RawSegmentTail::Corrupt {
                        offset: offset_u64,
                        kind: RecoveryCorruptionKind::FrameTooLarge,
                    }),
                ));
            }
        };
        let Some(frame_length) = FRAME_HEADER_LEN
            .checked_add(payload_length)
            .and_then(|length| length.checked_add(CHECKSUM_LEN))
        else {
            return Ok((
                frames,
                Some(RawSegmentTail::Corrupt {
                    offset: offset_u64,
                    kind: RecoveryCorruptionKind::FrameTooLarge,
                }),
            ));
        };
        if frame_length > MAX_WAL_FRAME_BYTES {
            return Ok((
                frames,
                Some(RawSegmentTail::Corrupt {
                    offset: offset_u64,
                    kind: RecoveryCorruptionKind::FrameTooLarge,
                }),
            ));
        }
        let Some(frame_end) = offset.checked_add(frame_length) else {
            return Ok((
                frames,
                Some(RawSegmentTail::Corrupt {
                    offset: offset_u64,
                    kind: RecoveryCorruptionKind::FrameTooLarge,
                }),
            ));
        };
        if frame_end > bytes.len() {
            return Ok((frames, Some(RawSegmentTail::Torn { offset: offset_u64 })));
        }
        let Some(frame_bytes) = bytes.get(offset..frame_end) else {
            return Ok((frames, Some(RawSegmentTail::Torn { offset: offset_u64 })));
        };
        let decoded = match decode_raw_segment(sequence, frame_bytes) {
            Ok(decoded) => decoded,
            Err(WalError::AllocationFailed) => return Err(WalError::AllocationFailed),
            Err(error) => {
                return Ok((
                    frames,
                    Some(RawSegmentTail::Corrupt {
                        offset: offset_u64,
                        kind: recovery_corruption_kind(&error),
                    }),
                ));
            }
        };
        if decoded.len() != 1 {
            return Ok((
                frames,
                Some(RawSegmentTail::Corrupt {
                    offset: offset_u64,
                    kind: RecoveryCorruptionKind::InvalidFrame,
                }),
            ));
        }
        let Some(mut raw) = decoded.into_iter().next() else {
            return Ok((
                frames,
                Some(RawSegmentTail::Corrupt {
                    offset: offset_u64,
                    kind: RecoveryCorruptionKind::InvalidFrame,
                }),
            ));
        };
        match &mut raw {
            RawWalFrame::Prepare(prepared) => {
                prepared.reference.byte_offset = offset_u64;
            }
            RawWalFrame::Commit { byte_offset, .. } => *byte_offset = offset_u64,
        }
        frames
            .try_reserve(1)
            .map_err(|_| WalError::AllocationFailed)?;
        frames.push(raw);
        offset = frame_end;
    }
    Ok((frames, None))
}

#[derive(Clone, Copy)]
enum RawSegmentTail {
    Torn {
        offset: u64,
    },
    Corrupt {
        offset: u64,
        kind: RecoveryCorruptionKind,
    },
}

fn recovery_corruption_kind(error: &WalError) -> RecoveryCorruptionKind {
    match error {
        WalError::MalformedPrepare { .. } | WalError::InvalidOperationId(_) => {
            RecoveryCorruptionKind::MalformedPrepare
        }
        WalError::MalformedCommitMarker { .. } => RecoveryCorruptionKind::MalformedCommitMarker,
        WalError::CommitMarkerWithoutPrepare { .. } => {
            RecoveryCorruptionKind::CommitMarkerWithoutPrepare
        }
        WalError::PayloadHashMismatch { .. } => RecoveryCorruptionKind::PayloadHashMismatch,
        WalError::CommitHashMismatch { .. } => RecoveryCorruptionKind::CommitHashMismatch,
        WalError::CommitChainMismatch { .. } => RecoveryCorruptionKind::CommitChainMismatch,
        WalError::RevisionSequenceMismatch { .. } => {
            RecoveryCorruptionKind::RevisionSequenceMismatch
        }
        WalError::DuplicateOperationId { .. } => RecoveryCorruptionKind::DuplicateOperationId,
        WalError::SegmentTooLarge { .. } => RecoveryCorruptionKind::SegmentTooLarge,
        WalError::FrameTooLarge { .. } => RecoveryCorruptionKind::FrameTooLarge,
        _ => RecoveryCorruptionKind::InvalidFrame,
    }
}

fn decode_prepare_payload(
    sequence: u64,
    offset: u64,
    bytes: &[u8],
) -> Result<(OperationId, Vec<u8>), WalError> {
    let mut decoder = TlvDecoder::new(bytes);
    let operation = decoder
        .next_field()
        .map_err(WalError::Wire)?
        .ok_or(WalError::MalformedPrepare { sequence, offset })?;
    let payload = decoder
        .next_field()
        .map_err(WalError::Wire)?
        .ok_or(WalError::MalformedPrepare { sequence, offset })?;
    if operation.tag() != OPERATION_ID_FIELD_TAG
        || payload.tag() != PAYLOAD_FIELD_TAG
        || decoder.next_field().map_err(WalError::Wire)?.is_some()
    {
        return Err(WalError::MalformedPrepare { sequence, offset });
    }
    let operation_bytes = <[u8; 16]>::try_from(operation.value())
        .map_err(|_| WalError::MalformedPrepare { sequence, offset })?;
    let operation_id =
        OperationId::try_from_bytes(operation_bytes).map_err(WalError::InvalidOperationId)?;
    let mut owned_payload = Vec::new();
    owned_payload
        .try_reserve_exact(payload.value().len())
        .map_err(|_| WalError::AllocationFailed)?;
    owned_payload.extend_from_slice(payload.value());
    Ok((operation_id, owned_payload))
}

fn decode_commit_payload(
    sequence: u64,
    offset: u64,
    bytes: &[u8],
) -> Result<WalCommitMarker, WalError> {
    let mut decoder = TlvDecoder::new(bytes);
    let segment_field = next_commit_field(&mut decoder, sequence, offset)?;
    let prepare_offset_field = next_commit_field(&mut decoder, sequence, offset)?;
    let prepare_length_field = next_commit_field(&mut decoder, sequence, offset)?;
    let revision_field = next_commit_field(&mut decoder, sequence, offset)?;
    let operation_field = next_commit_field(&mut decoder, sequence, offset)?;
    let payload_hash_field = next_commit_field(&mut decoder, sequence, offset)?;
    let previous_hash_field = next_commit_field(&mut decoder, sequence, offset)?;
    let commit_hash_field = next_commit_field(&mut decoder, sequence, offset)?;
    if segment_field.tag() != PREPARE_SEGMENT_FIELD_TAG
        || prepare_offset_field.tag() != PREPARE_OFFSET_FIELD_TAG
        || prepare_length_field.tag() != PREPARE_LENGTH_FIELD_TAG
        || revision_field.tag() != COMMIT_REVISION_FIELD_TAG
        || operation_field.tag() != COMMIT_OPERATION_ID_FIELD_TAG
        || payload_hash_field.tag() != PAYLOAD_HASH_FIELD_TAG
        || previous_hash_field.tag() != PREVIOUS_COMMIT_HASH_FIELD_TAG
        || commit_hash_field.tag() != COMMIT_HASH_FIELD_TAG
        || decoder.next_field().map_err(WalError::Wire)?.is_some()
    {
        return Err(WalError::MalformedCommitMarker { sequence, offset });
    }
    let prepare_segment = decode_u64_field(segment_field.value(), sequence, offset)?;
    let prepare_byte_offset = decode_u64_field(prepare_offset_field.value(), sequence, offset)?;
    let prepare_frame_length = decode_u64_field(prepare_length_field.value(), sequence, offset)?;
    let revision_number = decode_u64_field(revision_field.value(), sequence, offset)?;
    if prepare_segment == 0 || prepare_frame_length == 0 {
        return Err(WalError::MalformedCommitMarker { sequence, offset });
    }
    let operation_bytes = <[u8; 16]>::try_from(operation_field.value())
        .map_err(|_| WalError::MalformedCommitMarker { sequence, offset })?;
    let operation_id =
        OperationId::try_from_bytes(operation_bytes).map_err(WalError::InvalidOperationId)?;
    let payload_hash = decode_hash_field(payload_hash_field.value(), sequence, offset)?;
    let previous_commit_hash = WalCommitHash(decode_hash_field(
        previous_hash_field.value(),
        sequence,
        offset,
    )?);
    let commit_hash = WalCommitHash(decode_hash_field(
        commit_hash_field.value(),
        sequence,
        offset,
    )?);
    let revision = Revision::new(revision_number).map_err(WalError::Revision)?;
    Ok(WalCommitMarker {
        prepare_reference: WalPrepareReference {
            segment_sequence: prepare_segment,
            byte_offset: prepare_byte_offset,
            frame_length: prepare_frame_length,
            operation_id,
        },
        operation_id,
        revision,
        payload_hash,
        previous_commit_hash,
        commit_hash,
    })
}

fn next_commit_field<'a>(
    decoder: &mut TlvDecoder<'a>,
    sequence: u64,
    offset: u64,
) -> Result<worlddb_core::TlvField<'a>, WalError> {
    decoder
        .next_field()
        .map_err(WalError::Wire)?
        .ok_or(WalError::MalformedCommitMarker { sequence, offset })
}

fn decode_u64_field(bytes: &[u8], sequence: u64, offset: u64) -> Result<u64, WalError> {
    let value = <[u8; 8]>::try_from(bytes)
        .map_err(|_| WalError::MalformedCommitMarker { sequence, offset })?;
    Ok(u64::from_le_bytes(value))
}

fn decode_hash_field(bytes: &[u8], sequence: u64, offset: u64) -> Result<[u8; 32], WalError> {
    <[u8; 32]>::try_from(bytes).map_err(|_| WalError::MalformedCommitMarker { sequence, offset })
}

fn encode_commit_marker(marker: &WalCommitMarker) -> Result<Vec<u8>, WalError> {
    let mut encoder = TlvEncoder::new();
    encoder
        .push(
            PREPARE_SEGMENT_FIELD_TAG,
            &marker.prepare_reference.segment_sequence.to_le_bytes(),
        )
        .map_err(WalError::Wire)?;
    encoder
        .push(
            PREPARE_OFFSET_FIELD_TAG,
            &marker.prepare_reference.byte_offset.to_le_bytes(),
        )
        .map_err(WalError::Wire)?;
    encoder
        .push(
            PREPARE_LENGTH_FIELD_TAG,
            &marker.prepare_reference.frame_length.to_le_bytes(),
        )
        .map_err(WalError::Wire)?;
    encoder
        .push(
            COMMIT_REVISION_FIELD_TAG,
            &marker.revision.value().to_le_bytes(),
        )
        .map_err(WalError::Wire)?;
    encoder
        .push(
            COMMIT_OPERATION_ID_FIELD_TAG,
            &marker.operation_id.to_bytes(),
        )
        .map_err(WalError::Wire)?;
    encoder
        .push(PAYLOAD_HASH_FIELD_TAG, &marker.payload_hash)
        .map_err(WalError::Wire)?;
    encoder
        .push(
            PREVIOUS_COMMIT_HASH_FIELD_TAG,
            marker.previous_commit_hash.as_bytes(),
        )
        .map_err(WalError::Wire)?;
    encoder
        .push(COMMIT_HASH_FIELD_TAG, marker.commit_hash.as_bytes())
        .map_err(WalError::Wire)?;
    let payload = encoder.finish();
    let frame =
        encode_frame(FrameHeader::new(WAL_COMMIT_FRAME_KIND), &payload).map_err(WalError::Frame)?;
    if frame.len() > MAX_WAL_FRAME_BYTES {
        return Err(WalError::FrameTooLarge {
            limit: MAX_WAL_FRAME_BYTES,
            actual: frame.len(),
        });
    }
    Ok(frame)
}

fn calculate_commit_hash(
    prepare_reference: WalPrepareReference,
    revision: Revision,
    operation_id: OperationId,
    payload_hash: [u8; 32],
    previous_commit_hash: WalCommitHash,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(COMMIT_HASH_CONTEXT);
    hasher.update(&prepare_reference.segment_sequence.to_le_bytes());
    hasher.update(&prepare_reference.byte_offset.to_le_bytes());
    hasher.update(&prepare_reference.frame_length.to_le_bytes());
    hasher.update(&revision.value().to_le_bytes());
    hasher.update(&operation_id.to_bytes());
    hasher.update(&payload_hash);
    hasher.update(previous_commit_hash.as_bytes());
    *hasher.finalize().as_bytes()
}

fn append_frame_and_sync(
    file: &mut File,
    bytes: &[u8],
    sync: impl FnOnce(&File) -> io::Result<()>,
) -> Result<(), WalError> {
    file.write_all(bytes).map_err(|source| WalError::Io {
        operation: "append WAL prepare frame",
        source,
    })?;
    sync(file).map_err(|source| WalError::Io {
        operation: "sync WAL prepare frame",
        source,
    })
}

fn append_commit_marker_and_sync(
    file: &mut File,
    bytes: &[u8],
    operation_id: OperationId,
    sync: impl FnOnce(&File) -> io::Result<()>,
) -> Result<(), WalError> {
    file.write_all(bytes)
        .map_err(|source| WalError::UnknownCommitOutcome {
            operation_id,
            operation: "append commit marker",
            source,
        })?;
    sync(file).map_err(|source| WalError::UnknownCommitOutcome {
        operation_id,
        operation: "second WAL sync",
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{DomainId, OperationId, Revision};

    use super::{RawWalFrame, decode_raw_segment, encode_commit_marker, segment_file_name};
    use crate::{
        DatabaseLayout, RecoveryCorruptionKind, RecoveryFinding, RecoveryManager, RecoveryScanner,
        WalCommitHash, WalError, WalLogEntry, WalOperationStatus, WalPrepareLog,
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDatabase(PathBuf);

    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("worlddb-m5-03-{}-{sequence}", std::process::id()));
            DatabaseLayout::create(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn operation_id() -> Result<OperationId, worlddb_core::IdValidationError> {
        OperationId::try_from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89,
            0xab, 0xcd,
        ])
    }

    fn operation_id_with_tail(tail: u8) -> Result<OperationId, worlddb_core::IdValidationError> {
        OperationId::try_from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89,
            0xab, tail,
        ])
    }

    #[test]
    fn sync_fault_is_reported_and_the_read_back_prepare_stays_uncommitted() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let log = WalPrepareLog::new(&layout);
        let operation_id = operation_id().map_err(|error| error.to_string())?;

        let result = log.append_prepare_with_sync(
            &lock,
            operation_id,
            b"payload-before-commit-marker",
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "injected WAL sync failure",
                ))
            },
        );
        assert!(matches!(
            result,
            Err(WalError::Io {
                operation: "sync WAL prepare frame",
                ..
            })
        ));

        let entries = log
            .read_segment(&lock, 1)
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            entries.as_slice(),
            [WalLogEntry::PreparedUncommitted(entry)]
                if entry.reference().operation_id() == operation_id
                    && entry.payload() == b"payload-before-commit-marker"
        ));
        Ok(())
    }

    #[test]
    fn commit_sync_fault_returns_unknown_outcome_without_a_receipt() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let log = WalPrepareLog::new(&layout);
        let operation_id = operation_id().map_err(|error| error.to_string())?;
        let reference = log
            .append_prepare(&lock, operation_id, b"payload-before-commitpoint")
            .map_err(|error| error.to_string())?;

        let result = log.commit_prepared_with_sync(&lock, reference, |file| {
            let length = file.metadata()?.len();
            if length <= reference.byte_offset() + reference.frame_length() {
                return Err(io::Error::other(
                    "commit marker was not appended before sync",
                ));
            }
            Err(io::Error::other("injected second WAL sync failure"))
        });

        assert!(matches!(
            result,
            Err(WalError::UnknownCommitOutcome {
                operation_id: failed_operation,
                operation: "second WAL sync",
                ..
            }) if failed_operation == operation_id
        ));
        let reopened_layout =
            DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let reopened_log = WalPrepareLog::new(&reopened_layout);
        let entries = reopened_log
            .read_segment(&lock, 1)
            .map_err(|error| error.to_string())?;
        assert!(matches!(entries.as_slice(), [WalLogEntry::Committed(_)]));
        assert_eq!(
            reopened_log
                .operation_status(&lock, operation_id)
                .map_err(|error| error.to_string())?,
            WalOperationStatus::Indeterminate
        );
        assert!(matches!(
            reopened_log.commit_operation(&lock, operation_id, b"payload-before-commitpoint"),
            Err(WalError::OperationIndeterminate {
                operation_id: pending_id,
            }) if pending_id == operation_id
        ));
        assert!(matches!(
            reopened_log.commit_operation(&lock, operation_id, b"different payload"),
            Err(WalError::IdempotencyMismatch {
                operation_id: mismatch_id,
            }) if mismatch_id == operation_id
        ));
        Ok(())
    }

    #[test]
    fn process_crash_at_wal_prepare_and_commit_sync_recovers_one_prefix() -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M5_22_WAL_CRASH_ROOT";
        const POINT_ENV: &str = "WORLDDB_M5_22_WAL_CRASH_POINT";
        const TEST_NAME: &str =
            "wal::tests::process_crash_at_wal_prepare_and_commit_sync_recovers_one_prefix";

        if let (Ok(root), Ok(point)) = (env::var(ROOT_ENV), env::var(POINT_ENV)) {
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let lock = layout
                .try_writer_lock()
                .map_err(|error| error.to_string())?;
            let log = WalPrepareLog::new(&layout);
            let operation_id = operation_id_with_tail(0xd1).map_err(|error| error.to_string())?;
            if point == "after_prepare_sync" {
                let _ = log.append_prepare_with_sync(
                    &lock,
                    operation_id,
                    b"m5-22-uncommitted-prepare",
                    |file| {
                        file.sync_all()?;
                        std::process::exit(86);
                    },
                );
            } else if point == "after_commit_sync" {
                let payload = log
                    .encode_manifest_snapshot_payload(&lock, operation_id, vec![], &[])
                    .map_err(|error| error.to_string())?;
                let reference = log
                    .append_prepare(&lock, operation_id, &payload)
                    .map_err(|error| error.to_string())?;
                let _ = log.commit_prepared_with_sync(&lock, reference, |file| {
                    file.sync_all()?;
                    std::process::exit(86);
                });
            }
            return Err(format!("child did not reach WAL crash point {point}"));
        }

        for (point, expected_revision) in [
            ("after_prepare_sync", Revision::GENESIS),
            (
                "after_commit_sync",
                Revision::try_from(1).map_err(|error| error.to_string())?,
            ),
        ] {
            let database = TempDatabase::create()?;
            let executable = env::current_exe().map_err(|error| error.to_string())?;
            let status = Command::new(executable)
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(ROOT_ENV, &database.0)
                .env(POINT_ENV, point)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|error| error.to_string())?;
            if status.code() != Some(86) {
                return Err(format!(
                    "child for {point} exited with {:?}, expected crash code 86",
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
            assert_eq!(report.safe_revision(), expected_revision);
            let recovered = RecoveryManager::new(layout.clone())
                .recover(&lock)
                .map_err(|error| format!("restart after {point}: {error}"))?;
            assert!(recovered.report().is_clean());
            assert_eq!(recovered.report().safe_revision(), expected_revision);
            if point == "after_commit_sync" {
                assert_eq!(
                    crate::ManifestStore::new(layout)
                        .read_current()
                        .map_err(|error| error.to_string())?
                        .map(|manifest| manifest.revision()),
                    Some(expected_revision)
                );
            }
        }
        Ok(())
    }

    #[test]
    fn recovery_scan_stops_at_a_checksum_valid_commit_chain_mismatch() -> Result<(), String> {
        let database = TempDatabase::create()?;
        let layout = DatabaseLayout::open(&database.0).map_err(|error| error.to_string())?;
        let lock = layout
            .try_writer_lock()
            .map_err(|error| error.to_string())?;
        let log = WalPrepareLog::new(&layout);
        let first = log
            .commit_operation(
                &lock,
                operation_id().map_err(|error| error.to_string())?,
                b"first commit",
            )
            .map_err(|error| error.to_string())?;
        let second = log
            .commit_operation(
                &lock,
                operation_id_with_tail(0xce).map_err(|error| error.to_string())?,
                b"second commit",
            )
            .map_err(|error| error.to_string())?;
        let path = layout
            .wal_directory()
            .join(segment_file_name(second.reference().segment_sequence()));
        let mut bytes = fs::read(&path).map_err(|error| error.to_string())?;
        let mut marker = decode_raw_segment(second.reference().segment_sequence(), &bytes)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find_map(|frame| match frame {
                RawWalFrame::Commit {
                    marker,
                    byte_offset,
                    ..
                } if byte_offset == second.marker_offset() => Some(marker),
                _ => None,
            })
            .ok_or_else(|| "second commit marker was not found".to_owned())?;
        marker.previous_commit_hash = WalCommitHash::from_bytes([0x55; 32]);
        let replacement = encode_commit_marker(&marker).map_err(|error| error.to_string())?;
        if u64::try_from(replacement.len()).map_err(|error| error.to_string())?
            != second.marker_length()
        {
            return Err("rewritten commit marker changed its registered frame length".to_owned());
        }
        let start = usize::try_from(second.marker_offset()).map_err(|error| error.to_string())?;
        let end = start
            .checked_add(replacement.len())
            .ok_or_else(|| "commit marker byte range overflowed".to_owned())?;
        let target = bytes
            .get_mut(start..end)
            .ok_or_else(|| "commit marker byte range is outside the WAL segment".to_owned())?;
        target.copy_from_slice(&replacement);
        fs::write(&path, bytes).map_err(|error| error.to_string())?;

        let report = RecoveryScanner::new(layout)
            .scan(&lock)
            .map_err(|error| error.to_string())?;
        assert_eq!(report.safe_revision(), first.revision());
        assert!(
            report
                .findings()
                .contains(&RecoveryFinding::SafeCorruption {
                    segment_sequence: Some(second.reference().segment_sequence()),
                    offset: Some(second.marker_offset()),
                    kind: RecoveryCorruptionKind::CommitChainMismatch,
                })
        );
        Ok(())
    }
}
