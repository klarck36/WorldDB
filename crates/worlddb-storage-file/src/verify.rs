//! Independent, read-only verification across storage and typed projections.

use std::collections::BTreeMap;
use std::fmt;

use worlddb_core::{
    Record, Revision, SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode,
    SecurityPolicyHistory,
};

use crate::replay_payload::decode_replay_payload;
use crate::wal::WalOperationStatus;
use crate::{
    CurrentManifestState, DatabaseLayout, ManifestSegmentKind, ManifestSegmentReference,
    RecoveryCorruptionKind, RecoveryDisposition, RecoveryFinding, RecoveryReport,
    RecoveryScanError, RecoveryScanner, SecurityPolicyHistoryStore, SegmentId, WalPrepareLog,
    WriterLock,
};

/// Observable class of damage; it describes the detected pattern, not a claim
/// about the physical cause of the bytes changing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageDamageClass {
    /// A checksum, digest, or immutable content binding no longer verifies.
    Bitflip,
    /// A frame or commit tail ends before its complete encoded boundary.
    Truncation,
    /// Valid frames or segment sequences appear in an invalid order.
    Reorder,
    /// The same durable frame identity appears more than once.
    DuplicateFrame,
    /// Checksums pass, but a typed payload or cross-record invariant fails.
    SemanticInvalidity,
    /// A different storage fault was found whose byte pattern is not specific.
    Other,
}

/// Non-destructive next steps offered by a verify report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageVerifyAction {
    /// Preserve the source database byte-for-byte for diagnosis.
    PreserveOriginal,
    /// Keep the database read-only until an operator selects a recovery path.
    KeepReadOnly,
    /// Run the journaled recovery path for a verified incomplete WAL tail.
    RunJournaledTailRecovery,
    /// Run journaled recovery to publish a verified committed snapshot.
    RunJournaledRecovery,
    /// Restore into a separate destination from a separately verified backup.
    RestoreVerifiedBackupToNewDestination,
    /// Salvage into a new database while retaining a loss report.
    SalvageIntoNewDatabase,
}

/// Verifier issue with the original scanner evidence retained when available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageVerifyIssue {
    /// Existing read-only scanner evidence.
    Recovery(RecoveryFinding),
    /// The independently reconstructed OperationId index disagrees with the WAL scan.
    OperationIndexMismatch,
    /// Typed schema records fail cross-record historical validation.
    SchemaHistoryInvalid { reason: String },
    /// Typed capability snapshots fail cross-revision historical validation.
    CapabilityHistoryInvalid { reason: String },
    /// A segment changed or failed typed readback during the independent pass.
    SegmentReadbackFailed {
        kind: ManifestSegmentKind,
        id: SegmentId,
        reason: String,
    },
}

/// One verifier finding and safe follow-up actions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageVerifyFinding {
    class: StorageDamageClass,
    issue: StorageVerifyIssue,
    safe_next_actions: Vec<StorageVerifyAction>,
}

impl StorageVerifyFinding {
    /// Observable damage class.
    #[must_use]
    pub const fn class(&self) -> StorageDamageClass {
        self.class
    }

    /// Detailed scanner or independent-pass evidence.
    #[must_use]
    pub const fn issue(&self) -> &StorageVerifyIssue {
        &self.issue
    }

    /// Actions that retain the original and avoid unsafe in-place repair.
    #[must_use]
    pub fn safe_next_actions(&self) -> &[StorageVerifyAction] {
        &self.safe_next_actions
    }
}

/// Counts from the verified WAL and latest complete manifest inventory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StorageVerifyInventory {
    manifest_segments: Option<usize>,
    history_segments: usize,
    security_policy_segments: usize,
    committed_wal_frames: usize,
    operation_id_entries: Option<usize>,
    schema_definitions: usize,
    schema_publication_revisions: usize,
    capability_versions: usize,
    capability_rules: usize,
}

impl StorageVerifyInventory {
    /// Segment count in the latest complete inventory visible to the verifier.
    #[must_use]
    pub const fn manifest_segments(self) -> Option<usize> {
        self.manifest_segments
    }

    /// Validated immutable history-segment references.
    #[must_use]
    pub const fn history_segments(self) -> usize {
        self.history_segments
    }

    /// Validated immutable security-policy segment references.
    #[must_use]
    pub const fn security_policy_segments(self) -> usize {
        self.security_policy_segments
    }

    /// Fully committed frames in the verified WAL prefix.
    #[must_use]
    pub const fn committed_wal_frames(self) -> usize {
        self.committed_wal_frames
    }

    /// Entry count when the independent OperationId index pass completed.
    #[must_use]
    pub const fn operation_id_entries(self) -> Option<usize> {
        self.operation_id_entries
    }

    /// Typed schema definitions independently materialized.
    #[must_use]
    pub const fn schema_definitions(self) -> usize {
        self.schema_definitions
    }

    /// Distinct schema publication revisions independently validated.
    #[must_use]
    pub const fn schema_publication_revisions(self) -> usize {
        self.schema_publication_revisions
    }

    /// Typed capability-policy revisions independently validated.
    #[must_use]
    pub const fn capability_versions(self) -> usize {
        self.capability_versions
    }

    /// Capability rules across the validated policy history.
    #[must_use]
    pub const fn capability_rules(self) -> usize {
        self.capability_rules
    }
}

/// Result of the independent read-only storage verification pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageVerifyReport {
    safe_revision: Revision,
    disposition: RecoveryDisposition,
    inventory: StorageVerifyInventory,
    findings: Vec<StorageVerifyFinding>,
}

impl StorageVerifyReport {
    /// Highest WAL revision whose complete commit marker and hash chain verify.
    #[must_use]
    pub const fn safe_revision(&self) -> Revision {
        self.safe_revision
    }

    /// Effective write/read-only classification after both verification passes.
    #[must_use]
    pub const fn disposition(&self) -> RecoveryDisposition {
        self.disposition
    }

    /// Counts and verification coverage for the independent storage views.
    #[must_use]
    pub const fn inventory(&self) -> StorageVerifyInventory {
        self.inventory
    }

    /// Corruption findings with explicit safe next steps.
    #[must_use]
    pub fn findings(&self) -> &[StorageVerifyFinding] {
        &self.findings
    }

    /// Whether every verified layer was clean.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty() && self.disposition == RecoveryDisposition::Clean
    }
}

impl fmt::Display for StorageVerifyReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.disposition {
            RecoveryDisposition::Clean => "CLEAN",
            RecoveryDisposition::RecoveryRequired => "RECOVERY_REQUIRED",
            RecoveryDisposition::QuarantinedReadOnly => "QUARANTINED_READ_ONLY",
        };
        write!(
            formatter,
            "{label}: safe_revision={}, findings={}, wal_frames={}, operation_ids={:?}, segments={:?}",
            self.safe_revision.value(),
            self.findings.len(),
            self.inventory.committed_wal_frames,
            self.inventory.operation_id_entries,
            self.inventory.manifest_segments
        )?;
        for finding in &self.findings {
            write!(
                formatter,
                "\n- {:?}: {:?}; safe_next_actions={:?}",
                finding.class, finding.issue, finding.safe_next_actions
            )?;
        }
        Ok(())
    }
}

/// Why the independent verify pass could not read one of its required views.
#[derive(Debug)]
pub enum StorageVerifyError {
    /// The read-only WAL/manifest scan could not complete at the filesystem layer.
    Recovery(RecoveryScanError),
    /// The independent WAL index reconstruction failed operationally.
    Wal(crate::WalError),
    /// A referenced history segment could not be decoded during typed verification.
    HistorySegment(crate::SegmentError),
    /// A referenced security segment could not be decoded during typed verification.
    SecuritySegment(crate::SecurityPolicyStorageError),
}

impl fmt::Display for StorageVerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => write!(formatter, "storage verify scan failed: {error}"),
            Self::Wal(error) => write!(formatter, "storage verify WAL index failed: {error}"),
            Self::HistorySegment(error) => {
                write!(formatter, "storage verify history read failed: {error}")
            }
            Self::SecuritySegment(error) => {
                write!(formatter, "storage verify security read failed: {error}")
            }
        }
    }
}

impl std::error::Error for StorageVerifyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Recovery(error) => Some(error),
            Self::Wal(error) => Some(error),
            Self::HistorySegment(error) => Some(error),
            Self::SecuritySegment(error) => Some(error),
        }
    }
}

/// Read-only verifier for the WAL, manifest, segment references, and typed histories.
#[derive(Clone, Debug)]
pub struct StorageVerifier {
    layout: DatabaseLayout,
}

impl StorageVerifier {
    /// Binds the verifier to an opened, validated database layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Verifies storage without changing database files.
    ///
    /// The caller holds the database writer lock so a concurrent WorldDB writer
    /// cannot publish a different inventory during the read-only passes.
    pub fn verify(
        &self,
        writer_lock: &WriterLock,
    ) -> Result<StorageVerifyReport, StorageVerifyError> {
        let scanner = RecoveryScanner::new(self.layout.clone());
        let (recovery, committed_frames) = match scanner.scan_with_frames(writer_lock) {
            Ok(result) => result,
            Err(error) => {
                writer_lock.recovery_scan_failed();
                return Err(StorageVerifyError::Recovery(error));
            }
        };
        writer_lock.note_recovery_disposition(recovery.disposition());

        let mut findings = recovery
            .findings()
            .iter()
            .cloned()
            .map(finding_from_recovery)
            .collect::<Vec<_>>();
        let mut inventory = StorageVerifyInventory {
            committed_wal_frames: committed_frames.len(),
            ..StorageVerifyInventory::default()
        };

        if recovery.disposition() == RecoveryDisposition::Clean {
            let statuses =
                match WalPrepareLog::new(&self.layout).verified_operation_statuses(writer_lock) {
                    Ok(statuses) => statuses,
                    Err(error) => {
                        writer_lock.recovery_scan_failed();
                        return Err(StorageVerifyError::Wal(error));
                    }
                };
            let index_matches = statuses.len() == committed_frames.len()
                && committed_frames.iter().all(|frame| {
                    statuses.get(&frame.prepare().reference().operation_id())
                        == Some(&WalOperationStatus::Committed(frame.receipt()))
                });
            if index_matches {
                inventory.operation_id_entries = Some(statuses.len());
            } else {
                findings.push(finding(
                    StorageDamageClass::SemanticInvalidity,
                    StorageVerifyIssue::OperationIndexMismatch,
                ));
            }
        }

        let inventory_view = latest_inventory(&recovery, &committed_frames);
        if let Some((inventory_revision, references)) = inventory_view {
            inventory.manifest_segments = Some(references.len());
            inventory.history_segments = references
                .iter()
                .filter(|reference| reference.kind() == ManifestSegmentKind::History)
                .count();
            inventory.security_policy_segments = references
                .iter()
                .filter(|reference| reference.kind() == ManifestSegmentKind::SecurityPolicy)
                .count();
            if let Err(error) = self.verify_typed_histories(
                inventory_revision,
                &references,
                &recovery,
                &mut inventory,
                &mut findings,
            ) {
                writer_lock.note_recovery_disposition(RecoveryDisposition::QuarantinedReadOnly);
                return Err(error);
            }
        }

        let has_quarantining_finding = findings.iter().any(|item| {
            item.class != StorageDamageClass::Truncation
                && !matches!(
                    item.issue,
                    StorageVerifyIssue::Recovery(
                        RecoveryFinding::ManifestBehindCommittedSnapshot { .. }
                    )
                )
        });
        let disposition = if has_quarantining_finding {
            RecoveryDisposition::QuarantinedReadOnly
        } else {
            recovery.disposition()
        };
        if disposition == RecoveryDisposition::QuarantinedReadOnly {
            writer_lock.note_recovery_disposition(disposition);
        }

        Ok(StorageVerifyReport {
            safe_revision: recovery.safe_revision(),
            disposition,
            inventory,
            findings,
        })
    }

    fn verify_typed_histories(
        &self,
        inventory_revision: Revision,
        references: &[ManifestSegmentReference],
        recovery: &RecoveryReport,
        inventory: &mut StorageVerifyInventory,
        findings: &mut Vec<StorageVerifyFinding>,
    ) -> Result<(), StorageVerifyError> {
        let history = crate::HistorySegmentStore::new(self.layout.clone());
        let security = SecurityPolicyHistoryStore::new(self.layout.clone());
        let mut schema_batches = BTreeMap::<Revision, Vec<SchemaDefinition>>::new();
        let mut policy_versions = BTreeMap::new();

        for reference in references {
            if recovery.findings().iter().any(|item| {
                matches!(
                    item,
                    RecoveryFinding::ReferencedSegmentCorrupt { kind, id }
                        if *kind == reference.kind() && *id == reference.id()
                )
            }) {
                continue;
            }
            match reference.kind() {
                ManifestSegmentKind::History => {
                    let segment = history
                        .read_verified_reference(*reference, true)
                        .map_err(StorageVerifyError::HistorySegment)?;
                    if segment.content_digest() != reference.content_digest() {
                        findings.push(finding(
                            StorageDamageClass::Bitflip,
                            StorageVerifyIssue::SegmentReadbackFailed {
                                kind: reference.kind(),
                                id: reference.id(),
                                reason: "content digest changed during typed verification".into(),
                            },
                        ));
                        continue;
                    }
                    for decoded in segment.records() {
                        if let Some((revision, definition)) = schema_definition(decoded.record()) {
                            schema_batches.entry(revision).or_default().push(definition);
                            inventory.schema_definitions += 1;
                        }
                    }
                }
                ManifestSegmentKind::SecurityPolicy => {
                    let segment = security
                        .read_verified_reference(*reference, true)
                        .map_err(StorageVerifyError::SecuritySegment)?;
                    if segment.content_digest() != reference.content_digest()
                        || segment.version().revision() != reference.through_revision()
                    {
                        findings.push(finding(
                            StorageDamageClass::Bitflip,
                            StorageVerifyIssue::SegmentReadbackFailed {
                                kind: reference.kind(),
                                id: reference.id(),
                                reason: "security snapshot identity or digest changed during verification".into(),
                            },
                        ));
                        continue;
                    }
                    let revision = segment.version().revision();
                    if policy_versions
                        .insert(revision, segment.version().clone())
                        .is_some()
                    {
                        findings.push(finding(
                            StorageDamageClass::DuplicateFrame,
                            StorageVerifyIssue::CapabilityHistoryInvalid {
                                reason: format!(
                                    "multiple capability snapshots claim revision {revision}"
                                ),
                            },
                        ));
                    }
                    inventory.capability_rules += segment.version().snapshot().rules().len();
                }
            }
        }

        if !schema_batches.is_empty() {
            let genesis_definitions = schema_batches
                .remove(&Revision::GENESIS)
                .unwrap_or_default();
            let mut model = match SchemaHistoryReferenceModel::with_genesis(genesis_definitions) {
                Ok(model) => model,
                Err(error) => {
                    findings.push(finding(
                        StorageDamageClass::SemanticInvalidity,
                        StorageVerifyIssue::SchemaHistoryInvalid {
                            reason: error.to_string(),
                        },
                    ));
                    return Ok(());
                }
            };
            for (revision, definitions) in schema_batches {
                if let Err(error) = model.publish(revision, definitions) {
                    findings.push(finding(
                        StorageDamageClass::SemanticInvalidity,
                        StorageVerifyIssue::SchemaHistoryInvalid {
                            reason: error.to_string(),
                        },
                    ));
                    break;
                }
                inventory.schema_publication_revisions += 1;
            }
            if let Err(error) = model.schema_at(SchemaMode::Current, inventory_revision) {
                findings.push(finding(
                    StorageDamageClass::SemanticInvalidity,
                    StorageVerifyIssue::SchemaHistoryInvalid {
                        reason: error.to_string(),
                    },
                ));
            }
        }

        if !policy_versions.is_empty() {
            let through_revision = policy_versions
                .last_key_value()
                .map(|(revision, _)| *revision)
                .unwrap_or(Revision::GENESIS);
            let versions = policy_versions.into_values().collect::<Vec<_>>();
            match SecurityPolicyHistory::new(through_revision, versions) {
                Ok(history) => inventory.capability_versions = history.versions().len(),
                Err(error) => findings.push(finding(
                    StorageDamageClass::SemanticInvalidity,
                    StorageVerifyIssue::CapabilityHistoryInvalid {
                        reason: error.to_string(),
                    },
                )),
            }
        }
        Ok(())
    }
}

fn latest_inventory(
    recovery: &RecoveryReport,
    committed_frames: &[crate::WalCommittedFrame],
) -> Option<(Revision, Vec<ManifestSegmentReference>)> {
    for frame in committed_frames.iter().rev() {
        match decode_replay_payload(
            frame.prepare().payload(),
            frame.receipt().revision(),
            frame.prepare().reference().operation_id(),
        ) {
            Ok(Some(decoded)) => {
                return Some((
                    decoded.snapshot.revision(),
                    decoded.snapshot.segments().to_vec(),
                ));
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }
    match recovery.current_manifest() {
        CurrentManifestState::Loaded(manifest) => {
            Some((manifest.revision(), manifest.segments().to_vec()))
        }
        CurrentManifestState::Missing | CurrentManifestState::Corrupt => None,
    }
}

fn schema_definition(record: &Record) -> Option<(Revision, SchemaDefinition)> {
    match record {
        Record::LayerDefinition(value) => Some((
            value.created_revision().revision(),
            SchemaDefinition::Layer(value.clone()),
        )),
        Record::LayerSchemaSnapshot(value) => Some((
            value.revision().revision(),
            SchemaDefinition::LayerSnapshot(value.clone()),
        )),
        Record::EntityTypeDefinition(value) => Some((
            value.created_revision(),
            SchemaDefinition::EntityType(value.clone()),
        )),
        Record::PredicateDefinition(value) => Some((
            value.created_revision(),
            SchemaDefinition::Predicate(value.clone()),
        )),
        Record::EventKindDefinition(value) => Some((
            value.created_revision(),
            SchemaDefinition::EventKind(value.clone()),
        )),
        Record::TimelineDefinition(value) => Some((
            value.created_revision(),
            SchemaDefinition::Timeline(value.clone()),
        )),
        Record::TimeUnitDefinition(value) => Some((
            value.created_revision(),
            SchemaDefinition::TimeUnit(value.clone()),
        )),
        _ => None,
    }
}

fn finding_from_recovery(item: RecoveryFinding) -> StorageVerifyFinding {
    let needs_manifest_recovery = matches!(
        &item,
        RecoveryFinding::ManifestBehindCommittedSnapshot { .. }
    );
    let class = match &item {
        RecoveryFinding::TornTail { .. } | RecoveryFinding::UncommittedTail { .. } => {
            StorageDamageClass::Truncation
        }
        RecoveryFinding::SafeCorruption { kind, .. } => match kind {
            RecoveryCorruptionKind::DuplicateOperationId => StorageDamageClass::DuplicateFrame,
            RecoveryCorruptionKind::SegmentSequenceGap
            | RecoveryCorruptionKind::TornTailBeforeLaterSegment
            | RecoveryCorruptionKind::CommitMarkerWithoutPrepare
            | RecoveryCorruptionKind::UnexpectedFrameAfterUncommittedPrepare => {
                StorageDamageClass::Reorder
            }
            RecoveryCorruptionKind::CommitChainMismatch
            | RecoveryCorruptionKind::RevisionSequenceMismatch => {
                StorageDamageClass::SemanticInvalidity
            }
            RecoveryCorruptionKind::InvalidFrame
            | RecoveryCorruptionKind::PayloadHashMismatch
            | RecoveryCorruptionKind::CommitHashMismatch => StorageDamageClass::Bitflip,
            _ => StorageDamageClass::Other,
        },
        RecoveryFinding::CurrentManifestCorrupt => StorageDamageClass::Bitflip,
        RecoveryFinding::ManifestAheadOfSafePrefix { .. }
        | RecoveryFinding::ManifestCommitHashMismatch { .. }
        | RecoveryFinding::ManifestSnapshotMismatch { .. }
        | RecoveryFinding::CommittedReplayPayloadCorrupt { .. }
        | RecoveryFinding::RequiredAuditSequenceInvalid { .. } => {
            StorageDamageClass::SemanticInvalidity
        }
        RecoveryFinding::ManifestBehindCommittedSnapshot { .. } => StorageDamageClass::Other,
        RecoveryFinding::ReferencedSegmentCorrupt { .. } => StorageDamageClass::Bitflip,
    };
    let mut result = finding(class, StorageVerifyIssue::Recovery(item));
    if needs_manifest_recovery {
        result
            .safe_next_actions
            .insert(2, StorageVerifyAction::RunJournaledRecovery);
    }
    result
}

fn finding(class: StorageDamageClass, issue: StorageVerifyIssue) -> StorageVerifyFinding {
    let safe_next_actions = match class {
        StorageDamageClass::Truncation => vec![
            StorageVerifyAction::PreserveOriginal,
            StorageVerifyAction::KeepReadOnly,
            StorageVerifyAction::RunJournaledTailRecovery,
        ],
        StorageDamageClass::Bitflip
        | StorageDamageClass::Reorder
        | StorageDamageClass::DuplicateFrame
        | StorageDamageClass::SemanticInvalidity
        | StorageDamageClass::Other => vec![
            StorageVerifyAction::PreserveOriginal,
            StorageVerifyAction::KeepReadOnly,
            StorageVerifyAction::RestoreVerifiedBackupToNewDestination,
            StorageVerifyAction::SalvageIntoNewDatabase,
        ],
    };
    StorageVerifyFinding {
        class,
        issue,
        safe_next_actions,
    }
}
