//! Explicit, host-controlled database recovery operations and safe UI reports.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use worlddb_storage_file::{
    DatabaseLayout, RecoveryCorruptionKind, RecoveryDisposition, RecoveryFinding, RecoveryManager,
    SalvageInventorySource, SalvageManager, SalvageSegmentOutcome, StorageDamageClass,
    StorageVerifier, StorageVerifyAction, StorageVerifyIssue, StorageVerifyReport,
};

use crate::EngineError;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryReportView {
    pub safe_revision: String,
    pub disposition: String,
    pub read_only: bool,
    pub current_manifest: String,
    pub inventory: RecoveryInventoryView,
    pub findings: Vec<RecoveryFindingView>,
    pub next_actions: Vec<String>,
    pub can_run_journaled_recovery: bool,
    pub can_restore_verified_backup: bool,
    pub can_salvage: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryInventoryView {
    pub wal_commit_frames: String,
    pub manifest_segments: Option<String>,
    pub history_segments: String,
    pub security_policy_segments: String,
    pub schema_definitions: String,
    pub capability_versions: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryFindingView {
    pub code: String,
    pub damage_class: String,
    pub summary: String,
    pub next_actions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryApplyView {
    pub quarantined_tails: String,
    pub replayed_snapshots: String,
    pub manifest_published: bool,
    pub report: RecoveryReportView,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoverySalvageView {
    pub new_database_id: String,
    pub safe_revision: String,
    pub disposition: String,
    pub inventory_source: String,
    pub copied_segments: String,
    pub omitted_segments: String,
    pub finding_count: String,
    pub uncertainty_count: String,
    pub archive_name: String,
}

/// Recovery-only access to a database selected by a trusted native host dialog.
/// Inspect and salvage use a pre-existing shared lock and do not rewrite source data.
pub struct RecoveryHost {
    layout: DatabaseLayout,
}

impl RecoveryHost {
    pub fn open(database_root: &Path) -> Result<Self, EngineError> {
        let layout = DatabaseLayout::open(database_root)
            .map_err(|_| EngineError::Recovery("invalid_project"))?;
        Ok(Self { layout })
    }

    /// Reads the verified prefix and typed inventories without modifying project files.
    pub fn inspect(&self) -> Result<RecoveryReportView, EngineError> {
        let lock = self
            .layout
            .try_read_only_lock()
            .map_err(|_| EngineError::Recovery("read_only_lock_unavailable"))?;
        let report = StorageVerifier::new(self.layout.clone())
            .verify(&lock)
            .map_err(|_| EngineError::Recovery("verification_unavailable"))?;
        Ok(recovery_report_view(&report))
    }

    /// Applies the storage layer's journaled recovery path only when explicitly requested.
    pub fn run_journaled_recovery(&self) -> Result<RecoveryApplyView, EngineError> {
        let lock = self
            .layout
            .try_writer_lock()
            .map_err(|_| EngineError::Recovery("writer_lock_unavailable"))?;
        let before = StorageVerifier::new(self.layout.clone())
            .verify(&lock)
            .map_err(|_| EngineError::Recovery("verification_unavailable"))?;
        if !recovery_report_view(&before).can_run_journaled_recovery {
            return Err(EngineError::Recovery("journaled_recovery_not_available"));
        }
        let outcome = RecoveryManager::new(self.layout.clone())
            .recover(&lock)
            .map_err(|_| EngineError::Recovery("journaled_recovery_rejected"))?;
        let report = StorageVerifier::new(self.layout.clone())
            .verify(&lock)
            .map_err(|_| EngineError::Recovery("post_recovery_verification_unavailable"))?;
        Ok(RecoveryApplyView {
            quarantined_tails: outcome.quarantined_tails().to_string(),
            replayed_snapshots: outcome.replayed_snapshots().to_string(),
            manifest_published: outcome.manifest_receipt().is_some(),
            report: recovery_report_view(&report),
        })
    }

    /// Copies verified segments into a new marked fork. The source is held read-only.
    pub fn salvage_to(&self, destination: &Path) -> Result<RecoverySalvageView, EngineError> {
        let lock = self
            .layout
            .try_read_only_lock()
            .map_err(|_| EngineError::Recovery("read_only_lock_unavailable"))?;
        let report = SalvageManager::new(self.layout.clone())
            .salvage(&lock, destination)
            .map_err(|_| EngineError::Recovery("salvage_rejected"))?;
        let mut copied_segments = 0_usize;
        let mut omitted_segments = 0_usize;
        for segment in report.segments() {
            match segment.outcome() {
                SalvageSegmentOutcome::Copied { .. } => copied_segments += 1,
                SalvageSegmentOutcome::Omitted { .. } => omitted_segments += 1,
            }
        }
        let archive_name = report
            .archive_path()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Salvage-Archiv".to_owned());
        Ok(RecoverySalvageView {
            new_database_id: report.database_id().to_string(),
            safe_revision: report.safe_revision().value().to_string(),
            disposition: disposition_code(report.disposition()).to_owned(),
            inventory_source: inventory_source_code(report.inventory_source()).to_owned(),
            copied_segments: copied_segments.to_string(),
            omitted_segments: omitted_segments.to_string(),
            finding_count: report.findings().len().to_string(),
            uncertainty_count: report.uncertainties().len().to_string(),
            archive_name,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.layout.root()
    }
}

pub fn inspect_recovery(database_root: &Path) -> Result<RecoveryReportView, EngineError> {
    RecoveryHost::open(database_root)?.inspect()
}

pub fn run_journaled_recovery(database_root: &Path) -> Result<RecoveryApplyView, EngineError> {
    RecoveryHost::open(database_root)?.run_journaled_recovery()
}

pub fn salvage_recovery(
    database_root: &Path,
    destination: &Path,
) -> Result<RecoverySalvageView, EngineError> {
    RecoveryHost::open(database_root)?.salvage_to(destination)
}

fn recovery_report_view(report: &StorageVerifyReport) -> RecoveryReportView {
    let mut actions = BTreeSet::new();
    let findings = report
        .findings()
        .iter()
        .map(|finding| {
            let (code, summary) = issue_summary(finding.issue());
            let next_actions = finding
                .safe_next_actions()
                .iter()
                .map(|action| action_code(*action).to_owned())
                .collect::<Vec<_>>();
            actions.extend(next_actions.iter().cloned());
            RecoveryFindingView {
                code: code.to_owned(),
                damage_class: damage_class_code(finding.class()).to_owned(),
                summary: summary.to_owned(),
                next_actions,
            }
        })
        .collect::<Vec<_>>();
    let disposition = disposition_code(report.disposition()).to_owned();
    let can_run_journaled_recovery = report.disposition() == RecoveryDisposition::RecoveryRequired
        && (actions.contains("run_journaled_recovery")
            || actions.contains("run_journaled_tail_recovery"));
    let can_restore_verified_backup =
        actions.contains("restore_verified_backup_to_new_destination");
    let can_salvage = actions.contains("salvage_into_new_database");
    let inventory = report.inventory();
    RecoveryReportView {
        safe_revision: report.safe_revision().value().to_string(),
        read_only: report.disposition() != RecoveryDisposition::Clean,
        disposition,
        current_manifest: match report.findings().iter().find_map(|finding| {
            if let StorageVerifyIssue::Recovery(RecoveryFinding::CurrentManifestCorrupt) =
                finding.issue()
            {
                Some("corrupt")
            } else {
                None
            }
        }) {
            Some(value) => value.to_owned(),
            None => "verified_or_not_applicable".to_owned(),
        },
        inventory: RecoveryInventoryView {
            wal_commit_frames: inventory.committed_wal_frames().to_string(),
            manifest_segments: inventory.manifest_segments().map(|value| value.to_string()),
            history_segments: inventory.history_segments().to_string(),
            security_policy_segments: inventory.security_policy_segments().to_string(),
            schema_definitions: inventory.schema_definitions().to_string(),
            capability_versions: inventory.capability_versions().to_string(),
        },
        findings,
        next_actions: actions.into_iter().collect(),
        can_run_journaled_recovery,
        can_restore_verified_backup,
        can_salvage,
    }
}

fn issue_summary(issue: &StorageVerifyIssue) -> (&'static str, &'static str) {
    match issue {
        StorageVerifyIssue::Recovery(finding) => recovery_finding_summary(finding),
        StorageVerifyIssue::OperationIndexMismatch => (
            "operation_index_mismatch",
            "Der OperationId-Index stimmt nicht mit dem vollständig geprüften WAL überein.",
        ),
        StorageVerifyIssue::SchemaHistoryInvalid { .. } => (
            "schema_history_invalid",
            "Die Schemahistorie konnte nicht vollständig verifiziert werden.",
        ),
        StorageVerifyIssue::CapabilityHistoryInvalid { .. } => (
            "capability_history_invalid",
            "Die Berechtigungshistorie konnte nicht vollständig verifiziert werden.",
        ),
        StorageVerifyIssue::SegmentReadbackFailed { .. } => (
            "segment_readback_failed",
            "Ein referenziertes Datensegment konnte beim zweiten Prüflauf nicht verifiziert werden.",
        ),
    }
}

fn recovery_finding_summary(finding: &RecoveryFinding) -> (&'static str, &'static str) {
    match finding {
        RecoveryFinding::TornTail { .. } => (
            "torn_tail",
            "Ein WAL-Endstück ist unvollständig und zählt nicht zur sicheren Revision.",
        ),
        RecoveryFinding::UncommittedTail { .. } => (
            "uncommitted_tail",
            "Ein vollständiger Prepare-Eintrag besitzt keinen Commitmarker.",
        ),
        RecoveryFinding::SafeCorruption { kind, .. } => (
            corruption_code(*kind),
            "Im sicheren WAL-Präfix wurde eine Integritätsverletzung gefunden.",
        ),
        RecoveryFinding::CurrentManifestCorrupt => (
            "current_manifest_corrupt",
            "CURRENT oder das referenzierte Manifest ist nicht verifizierbar.",
        ),
        RecoveryFinding::ManifestAheadOfSafePrefix { .. } => (
            "manifest_ahead_of_safe_prefix",
            "Das Manifest beansprucht eine Revision oberhalb des verifizierten WAL-Präfixes.",
        ),
        RecoveryFinding::ManifestCommitHashMismatch { .. } => (
            "manifest_commit_hash_mismatch",
            "Der Manifest-Commitbezug stimmt nicht mit der WAL-Hashkette überein.",
        ),
        RecoveryFinding::ManifestBehindCommittedSnapshot { .. } => (
            "manifest_behind_committed_snapshot",
            "Das Manifest liegt hinter einem vollständig committed Snapshot.",
        ),
        RecoveryFinding::ManifestSnapshotMismatch { .. } => (
            "manifest_snapshot_mismatch",
            "Manifest und committed Snapshot nennen unterschiedliche Segmentinventare.",
        ),
        RecoveryFinding::CommittedReplayPayloadCorrupt { .. } => (
            "committed_replay_payload_corrupt",
            "Ein committed Replay-Payload ist nicht vollständig verifizierbar.",
        ),
        RecoveryFinding::RequiredAuditSequenceInvalid { .. } => (
            "required_audit_sequence_invalid",
            "Eine Required-Audit-Sequenz ist doppelt oder nicht fortlaufend.",
        ),
        RecoveryFinding::ReferencedSegmentCorrupt { .. } => (
            "referenced_segment_corrupt",
            "Ein vom committed Inventar referenziertes Segment ist beschädigt.",
        ),
    }
}

fn action_code(action: StorageVerifyAction) -> &'static str {
    match action {
        StorageVerifyAction::PreserveOriginal => "preserve_original",
        StorageVerifyAction::KeepReadOnly => "keep_read_only",
        StorageVerifyAction::RunJournaledTailRecovery => "run_journaled_tail_recovery",
        StorageVerifyAction::RunJournaledRecovery => "run_journaled_recovery",
        StorageVerifyAction::RestoreVerifiedBackupToNewDestination => {
            "restore_verified_backup_to_new_destination"
        }
        StorageVerifyAction::SalvageIntoNewDatabase => "salvage_into_new_database",
    }
}

fn damage_class_code(class: StorageDamageClass) -> &'static str {
    match class {
        StorageDamageClass::Bitflip => "bitflip",
        StorageDamageClass::Truncation => "truncation",
        StorageDamageClass::Reorder => "reorder",
        StorageDamageClass::DuplicateFrame => "duplicate_frame",
        StorageDamageClass::SemanticInvalidity => "semantic_invalidity",
        StorageDamageClass::Other => "other",
    }
}

fn disposition_code(disposition: RecoveryDisposition) -> &'static str {
    match disposition {
        RecoveryDisposition::Clean => "clean",
        RecoveryDisposition::RecoveryRequired => "recovery_required",
        RecoveryDisposition::QuarantinedReadOnly => "quarantined_read_only",
    }
}

fn inventory_source_code(source: SalvageInventorySource) -> &'static str {
    match source {
        SalvageInventorySource::CommittedWalSnapshot { .. } => "committed_wal_snapshot",
        SalvageInventorySource::CurrentManifest { .. } => "current_manifest",
        SalvageInventorySource::NoVerifiedInventory => "no_verified_inventory",
    }
}

fn corruption_code(kind: RecoveryCorruptionKind) -> &'static str {
    match kind {
        RecoveryCorruptionKind::InvalidSegmentName => "invalid_segment_name",
        RecoveryCorruptionKind::InvalidSegmentPath => "invalid_segment_path",
        RecoveryCorruptionKind::SegmentSequenceGap => "segment_sequence_gap",
        RecoveryCorruptionKind::SegmentTooLarge => "segment_too_large",
        RecoveryCorruptionKind::FrameTooLarge => "frame_too_large",
        RecoveryCorruptionKind::InvalidFrame => "invalid_frame",
        RecoveryCorruptionKind::MalformedPrepare => "malformed_prepare",
        RecoveryCorruptionKind::MalformedCommitMarker => "malformed_commit_marker",
        RecoveryCorruptionKind::CommitMarkerWithoutPrepare => "commit_marker_without_prepare",
        RecoveryCorruptionKind::PayloadHashMismatch => "payload_hash_mismatch",
        RecoveryCorruptionKind::CommitHashMismatch => "commit_hash_mismatch",
        RecoveryCorruptionKind::CommitChainMismatch => "commit_chain_mismatch",
        RecoveryCorruptionKind::RevisionSequenceMismatch => "revision_sequence_mismatch",
        RecoveryCorruptionKind::DuplicateOperationId => "duplicate_operation_id",
        RecoveryCorruptionKind::UnexpectedFrameAfterUncommittedPrepare => {
            "unexpected_frame_after_uncommitted_prepare"
        }
        RecoveryCorruptionKind::TornTailBeforeLaterSegment => "torn_tail_before_later_segment",
        RecoveryCorruptionKind::RevisionOverflow => "revision_overflow",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{DomainId, OperationId};
    use worlddb_storage_file::{DatabaseLayout, WalPrepareLog};

    use super::RecoveryHost;
    use crate::EngineHost;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempTree(PathBuf);

    impl TempTree {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("worlddb-ode-recovery-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).expect("temporary recovery directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, current: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(current).expect("enumerate recovery source") {
                let entry = entry.expect("read recovery directory entry");
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("recovery path stays under root")
                    .to_owned();
                let metadata = fs::symlink_metadata(&path).expect("read entry metadata");
                if metadata.is_dir() {
                    visit(root, &path, output);
                } else {
                    output.insert(relative, fs::read(path).expect("read recovery source file"));
                }
            }
        }

        let mut output = BTreeMap::new();
        visit(root, root, &mut output);
        output
    }

    fn create_clean_database(root: &Path) {
        drop(EngineHost::open(root).expect("create storage fixture"));
    }

    fn operation_id(tail: u8) -> OperationId {
        OperationId::try_from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x7c, 0xde, 0x8f, 0x01, 0x23, 0x45, 0x67, 0x89,
            0xab, tail,
        ])
        .expect("valid operation identity")
    }

    #[test]
    fn inspect_and_clean_recovery_report_without_rewriting_source_files() {
        let temporary = TempTree::new();
        let source = temporary.path().join("source.worlddb");
        create_clean_database(&source);
        let before = snapshot(&source);

        let recovery = RecoveryHost::open(&source).expect("open recovery view");
        let report = recovery.inspect().expect("inspect without writing");
        assert_eq!(report.disposition, "clean");
        assert!(!report.read_only);
        assert!(report.findings.is_empty());
        assert_eq!(snapshot(&source), before);

        assert!(recovery.run_journaled_recovery().is_err());
        assert_eq!(snapshot(&source), before);
    }

    #[test]
    fn explicit_journaled_recovery_quarantines_only_an_eligible_uncommitted_tail() {
        let temporary = TempTree::new();
        let source = temporary.path().join("source.worlddb");
        create_clean_database(&source);
        let layout = DatabaseLayout::open(&source).expect("open storage fixture");
        let lock = layout.try_writer_lock().expect("acquire writer lock");
        WalPrepareLog::new(&layout)
            .append_prepare(&lock, operation_id(1), b"durable uncommitted prepare")
            .expect("append an uncommitted prepare");
        drop(lock);

        let recovery = RecoveryHost::open(&source).expect("open recovery view");
        let preview = recovery.inspect().expect("inspect damaged tail read-only");
        assert_eq!(preview.disposition, "recovery_required");
        assert!(preview.read_only);
        assert!(preview.can_run_journaled_recovery);

        let applied = recovery
            .run_journaled_recovery()
            .expect("explicitly quarantine eligible tail");
        assert_eq!(applied.quarantined_tails, "1");
        assert_eq!(applied.report.disposition, "clean");
        assert!(!applied.report.read_only);
    }

    #[test]
    fn salvage_copies_to_a_new_archive_and_leaves_source_byte_identical() {
        let temporary = TempTree::new();
        let source = temporary.path().join("source.worlddb");
        let target = temporary.path().join("fork.salvage");
        create_clean_database(&source);
        let before = snapshot(&source);

        let result = RecoveryHost::open(&source)
            .expect("open recovery view")
            .salvage_to(&target)
            .expect("create marked salvage archive");

        assert_eq!(result.disposition, "clean");
        assert_eq!(result.copied_segments, "0");
        assert_eq!(result.omitted_segments, "0");
        assert_eq!(result.archive_name, "fork.salvage");
        assert!(target.join("SALVAGE").is_file());
        assert!(target.join("SALVAGE_REPORT.tsv").is_file());
        assert_eq!(snapshot(&source), before);
    }
}
