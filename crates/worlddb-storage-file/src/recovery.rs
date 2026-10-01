//! Read-only discovery and verification of the committed WAL prefix.

use std::fmt;
use std::io;

use worlddb_core::Revision;
use worlddb_core::storage_internal::SegmentId;

use crate::DatabaseLayout;
use crate::manifest::{Manifest, ManifestError, ManifestSegmentKind, ManifestStore};
use crate::replay_payload::decode_replay_payload;
use crate::security_segment::SecurityPolicyHistoryStore;
use crate::segment::HistorySegmentStore;
use crate::wal::{WalCommitHash, WalCommittedFrame, WalError, WalPrepareLog};
use crate::writer_lock::WriterLock;

/// A structural or integrity fault that prevents a later WAL frame from
/// extending the verified committed prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryCorruptionKind {
    /// A WAL directory entry is not a registered segment filename.
    InvalidSegmentName,
    /// A WAL segment is not a regular file contained by the database root.
    InvalidSegmentPath,
    /// Numbered WAL segments are not contiguous from sequence one.
    SegmentSequenceGap,
    /// A segment exceeds the bounded recovery read size.
    SegmentTooLarge,
    /// A frame's declared length exceeds the registered WAL frame limit.
    FrameTooLarge,
    /// A complete frame failed checksum, version, or framing validation.
    InvalidFrame,
    /// A complete prepare frame has invalid fields or identity bytes.
    MalformedPrepare,
    /// A complete commit marker has invalid fields.
    MalformedCommitMarker,
    /// A commit marker does not bind the immediately preceding prepare.
    CommitMarkerWithoutPrepare,
    /// A commit marker's payload digest does not match the prepare bytes.
    PayloadHashMismatch,
    /// A commit marker's digest does not match its canonical contents.
    CommitHashMismatch,
    /// A commit marker does not continue the prior verified hash.
    CommitChainMismatch,
    /// A commit revision is not the next revision in the verified prefix.
    RevisionSequenceMismatch,
    /// An operation identity is reused in the WAL.
    DuplicateOperationId,
    /// Another frame follows a prepare that has no commit marker.
    UnexpectedFrameAfterUncommittedPrepare,
    /// A segment containing an incomplete frame is followed by another segment.
    TornTailBeforeLaterSegment,
    /// The commit revision cannot advance without reaching its reserved value.
    RevisionOverflow,
}

/// Recovery findings. A torn frame and a complete uncommitted prepare remain
/// separately visible; neither advances `safe_revision`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryFinding {
    /// A frame at the end of a WAL segment is incomplete.
    TornTail {
        /// WAL segment sequence containing the incomplete bytes.
        segment_sequence: u64,
        /// Byte offset where the incomplete frame begins.
        offset: u64,
        /// A complete prepare before the incomplete bytes that has no marker.
        pending_prepare_offset: Option<u64>,
    },
    /// A complete, valid prepare has no matching commit marker.
    UncommittedTail {
        /// WAL segment containing the prepare.
        segment_sequence: u64,
        /// Byte offset of the prepare frame.
        prepare_offset: u64,
    },
    /// A complete frame or the WAL sequence violates the storage contract.
    SafeCorruption {
        /// Segment containing the first observed fault, if known.
        segment_sequence: Option<u64>,
        /// Byte offset of the first observed fault, if known.
        offset: Option<u64>,
        /// Structural or integrity class.
        kind: RecoveryCorruptionKind,
    },
    /// `CURRENT` or its referenced manifest is malformed, missing, or corrupt.
    CurrentManifestCorrupt,
    /// The referenced manifest is newer than the verified WAL prefix.
    ManifestAheadOfSafePrefix {
        /// Revision declared by the manifest.
        manifest_revision: Revision,
        /// Highest fully verified committed revision.
        safe_revision: Revision,
    },
    /// The manifest's commit hash differs from the WAL hash at its revision.
    ManifestCommitHashMismatch {
        /// Revision at which the digest binding failed.
        manifest_revision: Revision,
    },
    /// The current manifest has not yet materialized the latest committed snapshot.
    ManifestBehindCommittedSnapshot {
        /// Revision in the latest committed storage snapshot.
        snapshot_revision: Revision,
        /// Current manifest revision, or `None` when CURRENT is missing.
        manifest_revision: Option<Revision>,
    },
    /// A manifest and the WAL snapshot disagree at the same committed revision.
    ManifestSnapshotMismatch {
        /// Revision where the segment inventories diverge.
        snapshot_revision: Revision,
    },
    /// A committed typed snapshot or Required Audit envelope is malformed or cannot be decoded.
    CommittedReplayPayloadCorrupt {
        /// Revision whose committed replay payload failed validation.
        revision: Revision,
    },
    /// A Required Audit Record sequence is duplicated or regresses in committed WAL order.
    RequiredAuditSequenceInvalid {
        /// Revision whose audit record violates the monotonic sequence contract.
        revision: Revision,
    },
    /// A segment referenced by a current or latest committed snapshot failed validation.
    ReferencedSegmentCorrupt {
        /// Segment namespace.
        kind: ManifestSegmentKind,
        /// Stable immutable segment identity.
        id: SegmentId,
    },
}

/// Readiness classification returned by the safe recovery verifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDisposition {
    /// WAL, manifest, and referenced immutable segments verify; ordinary writes may proceed.
    Clean,
    /// Only a recoverable WAL tail is present; ordinary writes wait for recovery.
    RecoveryRequired,
    /// Safe-area corruption or an inconsistent committed snapshot requires read-only handling.
    QuarantinedReadOnly,
}

/// Result of discovering the `CURRENT` pointer and its referenced generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CurrentManifestState {
    /// No current manifest is present; this is expected for a new database.
    Missing,
    /// The pointer digest and manifest structure verified.
    Loaded(Manifest),
    /// The pointer or its referenced generation failed validation.
    Corrupt,
}

/// Read-only recovery scan result. The safe revision is always a fully
/// verified commit prefix, even when findings require administrative recovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryReport {
    safe_revision: Revision,
    safe_commit_hash: WalCommitHash,
    current_manifest: CurrentManifestState,
    findings: Vec<RecoveryFinding>,
}

impl RecoveryReport {
    /// Highest revision whose complete commit marker and full hash chain verify.
    #[must_use]
    pub const fn safe_revision(&self) -> Revision {
        self.safe_revision
    }

    /// Commit-chain digest at `safe_revision`.
    #[must_use]
    pub const fn safe_commit_hash(&self) -> WalCommitHash {
        self.safe_commit_hash
    }

    /// Discovered manifest state from `CURRENT`.
    #[must_use]
    pub const fn current_manifest(&self) -> &CurrentManifestState {
        &self.current_manifest
    }

    /// All observed tail, corruption, and manifest consistency findings.
    #[must_use]
    pub fn findings(&self) -> &[RecoveryFinding] {
        &self.findings
    }

    /// Whether the WAL and the discovered current manifest form a clean view.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.disposition() == RecoveryDisposition::Clean
    }

    /// Whether ordinary writes are safe, recovery is required, or the database must stay read-only.
    #[must_use]
    pub fn disposition(&self) -> RecoveryDisposition {
        if self.findings.is_empty() {
            RecoveryDisposition::Clean
        } else if self.findings.iter().any(|finding| {
            !matches!(
                finding,
                RecoveryFinding::TornTail { .. }
                    | RecoveryFinding::UncommittedTail { .. }
                    | RecoveryFinding::ManifestBehindCommittedSnapshot { .. }
            )
        }) {
            RecoveryDisposition::QuarantinedReadOnly
        } else {
            RecoveryDisposition::RecoveryRequired
        }
    }
}

impl fmt::Display for RecoveryReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.disposition() {
            RecoveryDisposition::Clean => "CLEAN",
            RecoveryDisposition::RecoveryRequired => "RECOVERY_REQUIRED",
            RecoveryDisposition::QuarantinedReadOnly => "QUARANTINED_READ_ONLY",
        };
        write!(
            formatter,
            "{label}: safe_revision={}, findings={}",
            self.safe_revision.value(),
            self.findings.len()
        )?;
        for finding in &self.findings {
            write!(formatter, "\n- {finding:?}")?;
        }
        Ok(())
    }
}

/// Why a read-only recovery scan could not complete.
#[derive(Debug)]
pub enum RecoveryScanError {
    /// WAL discovery or reading failed for an operational reason.
    Wal(WalError),
    /// Reading the current pointer or manifest failed at the filesystem layer.
    ManifestIo {
        /// Filesystem operation that failed.
        operation: &'static str,
        /// Operating-system error.
        source: io::Error,
    },
}

impl fmt::Display for RecoveryScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wal(error) => write!(formatter, "WAL recovery scan failed: {error}"),
            Self::ManifestIo { operation, source } => {
                write!(formatter, "{operation}: {source}")
            }
        }
    }
}

impl std::error::Error for RecoveryScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wal(error) => Some(error),
            Self::ManifestIo { source, .. } => Some(source),
        }
    }
}

/// Read-only scanner for `CURRENT`, immutable manifests, and WAL segments.
#[derive(Clone, Debug)]
pub struct RecoveryScanner {
    layout: DatabaseLayout,
}

impl RecoveryScanner {
    /// Binds the scanner to an opened, validated database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Verifies the complete committed prefix while the caller holds this
    /// database's writer lock. The scan never changes database files and
    /// restricts the lock to recovery/read-only use when findings are present.
    pub fn scan(&self, writer_lock: &WriterLock) -> Result<RecoveryReport, RecoveryScanError> {
        match self.scan_with_frames(writer_lock) {
            Ok((report, _)) => {
                writer_lock.note_recovery_disposition(report.disposition());
                Ok(report)
            }
            Err(error) => {
                writer_lock.recovery_scan_failed();
                Err(error)
            }
        }
    }

    pub(crate) fn scan_with_frames(
        &self,
        writer_lock: &WriterLock,
    ) -> Result<(RecoveryReport, Vec<WalCommittedFrame>), RecoveryScanError> {
        let wal = WalPrepareLog::new(&self.layout);
        let verified = wal
            .scan_recovery_prefix(writer_lock)
            .map_err(RecoveryScanError::Wal)?;
        let mut findings = verified.finding.into_iter().collect::<Vec<_>>();

        let manifest_store = ManifestStore::new(self.layout.clone());
        let current_manifest = match manifest_store.read_current() {
            Ok(Some(manifest)) => CurrentManifestState::Loaded(manifest),
            Ok(None) => CurrentManifestState::Missing,
            Err(ManifestError::Io { operation, source }) => {
                return Err(RecoveryScanError::ManifestIo { operation, source });
            }
            Err(_) => {
                findings.push(RecoveryFinding::CurrentManifestCorrupt);
                CurrentManifestState::Corrupt
            }
        };

        if let CurrentManifestState::Loaded(manifest) = &current_manifest {
            if manifest.revision() > verified.head.revision() {
                findings.push(RecoveryFinding::ManifestAheadOfSafePrefix {
                    manifest_revision: manifest.revision(),
                    safe_revision: verified.head.revision(),
                });
            } else if verified.hashes_by_revision.get(&manifest.revision())
                != Some(&manifest.commit_hash())
            {
                findings.push(RecoveryFinding::ManifestCommitHashMismatch {
                    manifest_revision: manifest.revision(),
                });
            }
            let history = HistorySegmentStore::new(self.layout.clone());
            let security = SecurityPolicyHistoryStore::new(self.layout.clone());
            for reference in manifest.segments() {
                if !validate_final_reference(&history, &security, *reference) {
                    findings.push(RecoveryFinding::ReferencedSegmentCorrupt {
                        kind: reference.kind(),
                        id: reference.id(),
                    });
                }
            }
        }

        let mut latest_snapshot = None;
        let mut previous_audit_sequence = None;
        for frame in &verified.committed_frames {
            let receipt = frame.receipt();
            let operation_id = frame.prepare().reference().operation_id();
            match decode_replay_payload(frame.prepare().payload(), receipt.revision(), operation_id)
            {
                Ok(Some(decoded)) => {
                    latest_snapshot = Some((decoded.snapshot, decoded.staged));
                }
                Ok(None) => {}
                Err(_) => findings.push(RecoveryFinding::CommittedReplayPayloadCorrupt {
                    revision: receipt.revision(),
                }),
            }
            match crate::required_audit::decode_required_audit_payload(
                frame.prepare().payload(),
                receipt.revision(),
                operation_id,
            ) {
                Ok(Some(decoded)) => {
                    if let Some(previous) = previous_audit_sequence {
                        if decoded.record.sequence() <= previous {
                            findings.push(RecoveryFinding::RequiredAuditSequenceInvalid {
                                revision: receipt.revision(),
                            });
                        } else {
                            previous_audit_sequence = Some(decoded.record.sequence());
                        }
                    } else {
                        previous_audit_sequence = Some(decoded.record.sequence());
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    if !findings.contains(&RecoveryFinding::CommittedReplayPayloadCorrupt {
                        revision: receipt.revision(),
                    }) {
                        findings.push(RecoveryFinding::CommittedReplayPayloadCorrupt {
                            revision: receipt.revision(),
                        });
                    }
                }
            }
        }
        if let Some((snapshot, staged)) = latest_snapshot {
            let history = HistorySegmentStore::new(self.layout.clone());
            let security = SecurityPolicyHistoryStore::new(self.layout.clone());
            for reference in snapshot.segments() {
                let valid = if staged.contains(reference)
                    && !validate_final_reference(&history, &security, *reference)
                {
                    validate_staged_reference(&history, &security, *reference)
                } else {
                    validate_final_reference(&history, &security, *reference)
                };
                if !valid {
                    findings.push(RecoveryFinding::ReferencedSegmentCorrupt {
                        kind: reference.kind(),
                        id: reference.id(),
                    });
                }
            }
            match &current_manifest {
                CurrentManifestState::Missing if snapshot.revision() > Revision::GENESIS => {
                    findings.push(RecoveryFinding::ManifestBehindCommittedSnapshot {
                        snapshot_revision: snapshot.revision(),
                        manifest_revision: None,
                    });
                }
                CurrentManifestState::Loaded(manifest)
                    if manifest.revision() < snapshot.revision() =>
                {
                    findings.push(RecoveryFinding::ManifestBehindCommittedSnapshot {
                        snapshot_revision: snapshot.revision(),
                        manifest_revision: Some(manifest.revision()),
                    });
                }
                CurrentManifestState::Loaded(manifest)
                    if manifest.revision() == snapshot.revision()
                        && manifest.segments() != snapshot.segments() =>
                {
                    findings.push(RecoveryFinding::ManifestSnapshotMismatch {
                        snapshot_revision: snapshot.revision(),
                    });
                }
                CurrentManifestState::Missing
                | CurrentManifestState::Corrupt
                | CurrentManifestState::Loaded(_) => {}
            }
        }

        Ok((
            RecoveryReport {
                safe_revision: verified.head.revision(),
                safe_commit_hash: verified.head.commit_hash(),
                current_manifest,
                findings,
            },
            verified.committed_frames,
        ))
    }
}

fn validate_final_reference(
    history: &HistorySegmentStore,
    security: &SecurityPolicyHistoryStore,
    reference: crate::manifest::ManifestSegmentReference,
) -> bool {
    match reference.kind() {
        ManifestSegmentKind::History => history.validate_manifest_reference(reference).is_ok(),
        ManifestSegmentKind::SecurityPolicy => {
            security.validate_manifest_reference(reference).is_ok()
        }
    }
}

fn validate_staged_reference(
    history: &HistorySegmentStore,
    security: &SecurityPolicyHistoryStore,
    reference: crate::manifest::ManifestSegmentReference,
) -> bool {
    match reference.kind() {
        ManifestSegmentKind::History => history.validate_staged_reference(reference).is_ok(),
        ManifestSegmentKind::SecurityPolicy => {
            security.validate_staged_reference(reference).is_ok()
        }
    }
}
