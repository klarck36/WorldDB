#![deny(unsafe_code)]

//! Local filesystem layout and format-probe adapter for WorldDB.

mod audit_wal;
mod backup;
mod compaction;
mod format;
mod guarded_migration;
mod index_generation;
mod index_rebuild;
mod layout;
mod logical_export;
mod logical_import;
mod manifest;
mod migration_commit;
mod migration_run_journal;
mod purge;
mod purge_rewrite;
mod recovery;
mod recovery_journal;
mod recovery_manager;
mod replay_payload;
mod required_audit;
mod salvage;
mod schema_management;
mod security_segment;
mod segment;
mod sharing_export;
mod storage_upgrade;
mod verify;
mod wal;
#[cfg(windows)]
mod windows_publication;
mod writer_lock;

pub use audit_wal::{
    CommittedRawReadAttempt, RawReadAuditAccessError, RawReadAuditError, RawReadAuditHead,
    RawReadAuditPolicyError, RawReadAuditReceipt, RawReadAuditRecoveryDisposition,
    RawReadAuditRecoveryReport, RawReadAuditSnapshot, RawReadAuditWal, RawReadAuditWriter,
};
pub use backup::{
    BackupAuthenticity, BackupError, BackupMacKey, BackupProfile, BackupProgressEvent,
    BackupVerification, ExactBackupManager, MigrationRestorePointError, RestoreError,
    RestoreManager, RestoreReport, verify_audit_complete_backup, verify_exact_backup,
};
pub use compaction::{
    CompactionError, CompactionManager, CompactionOutcome, ReclamationOutcome, SegmentPin,
    SegmentPinKind,
};
pub use format::{
    FORMAT_FILE_BYTES, FORMAT_FILE_KIND, FormatCapabilities, FormatProbeError, probe_format,
};
pub use guarded_migration::{
    FileStoreGuardedMigrationExecutionError, FileStoreGuardedMigrationJournalFailure,
    FileStoreGuardedMigrationOpenError, FileStoreGuardedMigrationRun,
};
pub use index_generation::{
    INDEX_GENERATION_FRAME_KIND, IndexFileDecision, IndexGeneration, IndexGenerationError,
    decode_index_generation, encode_index_generation, select_index_generation,
};
pub use index_rebuild::{
    IndexGenerationInventoryEntry, IndexGenerationStore, IndexRebuildError, IndexRebuildLimits,
    IndexRebuildManager, IndexRebuildReceipt, IndexRebuildSource, IndexStorageInventory,
    PinnedIndexSnapshot, StoredIndexGeneration,
};
pub use layout::{DatabaseLayout, StorageFileError};
pub use logical_export::{
    LogicalExport, LogicalExportClassSummary, LogicalExportEntry, LogicalExportError,
    LogicalExportHistorySpace, LogicalExportInclusion, LogicalExportManager, LogicalExportManifest,
    LogicalExportScope, LogicalExportStorageClass,
};
pub use logical_import::{
    LogicalImport, LogicalImportDestinationInventory, LogicalImportError, LogicalImportIdMapping,
    LogicalImportIdentity, LogicalImportManager, LogicalImportPlan,
};
pub use manifest::{
    Manifest, ManifestError, ManifestReceipt, ManifestSegmentKind, ManifestSegmentReference,
    ManifestSnapshot, ManifestStore,
};
pub use migration_commit::{
    FileMigrationBackendError, FileMigrationCommitBackend, FileMigrationHistoryRead,
};
pub use migration_run_journal::{MigrationRunJournalFileStore, MigrationRunJournalFileStoreError};
pub use purge::{
    PurgeApproval, PurgeCascadePlan, PurgeError, PurgeExternalArtifact, PurgeExternalArtifactKind,
    PurgeIndexGeneration, PurgePlan, PurgePlanManager, PurgeRecordId, PurgeSidecarInventory,
};
pub use purge_rewrite::{
    PurgeIdMapping, PurgeReport, PurgeRewriteError, PurgeRewriteManager, PurgeRewriteReceipt,
    PurgeRewriteRequest,
};
pub use recovery::{
    CurrentManifestState, RecoveryCorruptionKind, RecoveryDisposition, RecoveryFinding,
    RecoveryReport, RecoveryScanError, RecoveryScanner,
};
pub use recovery_journal::JournalError;
pub use recovery_manager::{RecoveryError, RecoveryManager, RecoveryOutcome};
pub use replay_payload::SnapshotCommitError;
pub use required_audit::{CommittedRequiredAuditRecord, RequiredAuditError};
pub use salvage::{
    SalvageError, SalvageInventorySource, SalvageManager, SalvageReport, SalvageSegmentOutcome,
    SalvageSegmentResult, SalvageSegmentSource, SalvageUnit,
};
pub use schema_management::{
    AssertionCorrectionReceipt, EntityCatalogPublicationReceipt, EntityManagementError,
    EventCorrectionReceipt, EventSnapshotRef, FactExplorerRequest, FactGraphExecution,
    FactHistoryRecord, FactManagementError, FactOperationStatus, FactPublicationReceipt,
    FactQueryExecution, FactQueryHistoryRow, FactQueryOperation, FactQueryRequest, FactQueryResult,
    FactResolutionPreviewRequest, FactSnapshot, FactTokenSearchPage, FactTokenSearchRequest,
    FactTokenSearchSession, FileEntityManager, FileFactManager, FileHistorySpaceTransferManager,
    FileProjectMetadataManager, FileSchemaManager, FileSecurityPolicyManager,
    HistorySpaceTransferError, LayerManagementError, MetadataPublicationReceipt,
    PolicyManagementError, PolicyPublicationReceipt, ReplacementBoundaryDraft,
    SchemaManagementError, SchemaPublicationReceipt, SourceDraft, SourceSupersessionDraft,
    TransferEventRelationItem, TransferPublicationReceipt, TransferSourceItem,
};
pub use security_segment::{
    SecurityPolicyHistorySnapshot, SecurityPolicyHistoryStore, SecurityPolicySegment,
    SecurityPolicySegmentReceipt, SecurityPolicyStorageError,
};
pub use segment::{
    ContentDigest, HistorySegment, HistorySegmentReceipt, HistorySegmentStore, SegmentError,
};
pub use sharing_export::{
    SharingExport, SharingExportError, SharingExportManager, SharingExportScope,
};
pub use storage_upgrade::{
    CurrentPointerFormat, StorageFormatProfile, StorageUpgradeAdminAction, StorageUpgradeBudget,
    StorageUpgradeError, StorageUpgradeManager, StorageUpgradePlan, StorageUpgradeReceipt,
    StorageUpgradeRestoreTargets, StorageUpgradeSafeRestorePoint, StorageUpgradeTransform,
};
pub use verify::{
    StorageDamageClass, StorageVerifier, StorageVerifyAction, StorageVerifyError,
    StorageVerifyFinding, StorageVerifyInventory, StorageVerifyIssue, StorageVerifyReport,
};
pub use wal::{
    WalCheckpoint, WalCheckpointSegment, WalCommitHash, WalCommitHead, WalCommitReceipt,
    WalCommittedFrame, WalError, WalLogEntry, WalOperationStatus, WalPrepareLog,
    WalPrepareReference, WalPreparedFrame,
};
pub use worlddb_core::storage_internal::SegmentId;
pub use writer_lock::{WriterLock, WriterLockError};

#[cfg(test)]
#[path = "../../../tools/fuzz/rust_campaign.rs"]
mod fuzz_campaign_support;

#[cfg(test)]
static STORAGE_FUZZ_FIXTURE_ROOT: std::sync::OnceLock<Result<std::path::PathBuf, String>> =
    std::sync::OnceLock::new();

#[cfg(test)]
static NEXT_RECOVERY_FUZZ_DATABASE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
struct RecoveryFuzzDatabase(std::path::PathBuf);

#[cfg(test)]
impl Drop for RecoveryFuzzDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
struct RestoreFuzzFiles(Vec<(std::path::PathBuf, Vec<u8>)>);

#[cfg(test)]
impl Drop for RestoreFuzzFiles {
    fn drop(&mut self) {
        for (path, bytes) in &self.0 {
            let _ = std::fs::write(path, bytes);
        }
    }
}

#[cfg(test)]
struct RestoreManifestDirectory {
    directory: std::path::PathBuf,
    current_path: std::path::PathBuf,
    current_bytes: Vec<u8>,
    original_names: std::collections::HashSet<std::ffi::OsString>,
}

#[cfg(test)]
impl Drop for RestoreManifestDirectory {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.directory) {
            for entry in entries.flatten() {
                if self.original_names.contains(&entry.file_name()) {
                    continue;
                }
                if let Ok(metadata) = std::fs::symlink_metadata(entry.path()) {
                    if metadata.is_file() && !metadata.file_type().is_symlink() {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        let _ = std::fs::write(&self.current_path, &self.current_bytes);
    }
}

#[cfg(test)]
fn fuzz_campaign_probe(target: &str, bytes: &[u8]) -> Result<bool, String> {
    use worlddb_core::DecoderLimits;

    let accepted = match target {
        "storage_format_probe" => format::probe_format(bytes).is_ok(),
        "storage_current_manifest" => fuzz_current_manifest(bytes)?,
        "storage_manifest_name" => fuzz_manifest_name(bytes)?,
        "storage_history_segment" => fuzz_history_segment(bytes)?,
        "storage_security_segment" => fuzz_security_segment(bytes)?,
        "storage_wal_segment" => wal::fuzz_wal_segment(bytes),
        "storage_backup_manifest" => backup::fuzz_backup_manifest(bytes),
        "storage_index_generation" => {
            decode_index_generation(bytes, &DecoderLimits::DEFAULT).is_ok()
        }
        "storage_index_pointer" => index_rebuild::fuzz_index_pointer(bytes),
        "storage_index_generation_name" => index_rebuild::fuzz_index_generation_name(bytes),
        "storage_compaction_pin_manifest" => compaction::fuzz_compaction_pin_manifest(bytes),
        "storage_migration_run_journal" => migration_run_journal::fuzz_migration_run_journal(bytes),
        "storage_guarded_migration_journal" => {
            guarded_migration::fuzz_guarded_migration_journal(bytes)
        }
        "storage_storage_upgrade_journal" => storage_upgrade::fuzz_storage_upgrade_journal(bytes),
        "storage_logical_export" => LogicalExport::decode(bytes).is_ok(),
        "storage_logical_import_plan" => LogicalImportPlan::decode(bytes).is_ok(),
        "storage_import_prepare" => fuzz_import_prepare(bytes),
        "storage_sharing_export" => SharingExport::decode(bytes).is_ok(),
        "storage_wal_recovery_prefix" => fuzz_recovery_database(bytes, |layout, lock| {
            RecoveryScanner::new(layout.clone()).scan(lock).is_ok()
        })?,
        "storage_wal_payloads" => wal::fuzz_wal_payloads(bytes),
        "storage_audit_wal" => fuzz_recovery_database(bytes, |layout, _lock| {
            audit_wal::fuzz_audit_wal(layout, bytes)
        })?,
        "storage_recovery_journal" => fuzz_recovery_database(bytes, |layout, lock| {
            recovery_journal::fuzz_recovery_journal(layout, lock, bytes)
        })?,
        "storage_recovery_pipeline" => fuzz_recovery_database(bytes, |layout, lock| {
            RecoveryManager::new(layout.clone()).recover(lock).is_ok()
        })?,
        "storage_verify_pipeline" => fuzz_recovery_database(bytes, |layout, lock| {
            StorageVerifier::new(layout.clone()).verify(lock).is_ok()
        })?,
        "storage_salvage_scanner" => fuzz_salvage_scanner(bytes)?,
        "storage_required_audit_payload" => required_audit::fuzz_required_audit_payload(bytes),
        "storage_replay_payload" => replay_payload::fuzz_replay_payload(bytes),
        _ => return Err(format!("unknown storage fuzz target: {target}")),
    };
    Ok(accepted)
}

#[cfg(test)]
fn fuzz_recovery_database(
    bytes: &[u8],
    probe: impl FnOnce(&DatabaseLayout, &WriterLock) -> bool,
) -> Result<bool, String> {
    use std::sync::atomic::Ordering;

    let sequence = NEXT_RECOVERY_FUZZ_DATABASE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "worlddb-storage-recovery-fuzz-{}-{sequence}",
        std::process::id()
    ));
    let _cleanup = RecoveryFuzzDatabase(root.clone());
    let layout = DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
    std::fs::write(
        layout
            .wal_directory()
            .join("segment-00000000000000000001.wal"),
        bytes,
    )
    .map_err(|error| error.to_string())?;
    let lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    Ok(probe(&layout, &lock))
}

#[cfg(test)]
fn fuzz_salvage_scanner(bytes: &[u8]) -> Result<bool, String> {
    let root = fuzz_storage_fixture_root()?;
    let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
    salvage::fuzz_salvage_scanner(&layout, bytes)
}

#[cfg(test)]
fn fuzz_import_prepare(bytes: &[u8]) -> bool {
    use worlddb_core::{DatabaseId, DomainId};

    let mut destination_bytes = [0_u8; 16];
    destination_bytes[6] = 0x70;
    destination_bytes[8] = 0x80;
    destination_bytes[15] = 0xfe;
    let Ok(destination_id) = DatabaseId::try_from_bytes(destination_bytes) else {
        return false;
    };
    let Ok(plan) = LogicalImportPlan::new(bytes, destination_id, Vec::new()) else {
        return false;
    };
    let Ok(plan_bytes) = plan.encode() else {
        return false;
    };
    let Ok(destination) = LogicalImportDestinationInventory::new(destination_id, []) else {
        return false;
    };
    LogicalImportManager::prepare(bytes, &plan_bytes, &destination).is_ok()
}

#[cfg(test)]
fn fuzz_storage_fixture_root() -> Result<std::path::PathBuf, String> {
    use std::fs;
    use std::path::{Path, PathBuf};

    fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path)?;
            if metadata.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "fuzz fixture unexpectedly contains a symlink",
                ));
            }
            if metadata.is_dir() {
                copy_tree(&source_path, &destination_path)?;
            } else if metadata.is_file() {
                fs::copy(source_path, destination_path)?;
            }
        }
        Ok(())
    }

    STORAGE_FUZZ_FIXTURE_ROOT
        .get_or_init(|| {
            let source =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m7-16h/storage");
            let mut workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            loop {
                if workspace.join("policy/fuzz-targets.tsv").is_file() {
                    break;
                }
                if !workspace.pop() {
                    return Err("could not locate the WorldDB workspace root".to_owned());
                }
            }
            let report_path = std::env::var_os("WORLDDB_FUZZ_REPORT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("worlddb-fuzzer-report.json"));
            let report_path = if report_path.is_absolute() {
                report_path
            } else {
                workspace.join(report_path)
            };
            let report_directory = report_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir);
            fs::create_dir_all(&report_directory).map_err(|error| error.to_string())?;
            let destination =
                report_directory.join(format!("storage-fuzz-fixture-{}", std::process::id()));
            if destination.exists() {
                fs::remove_dir_all(&destination).map_err(|error| error.to_string())?;
            }
            if let Err(error) = copy_tree(&source, &destination) {
                let _ = fs::remove_dir_all(&destination);
                return Err(error.to_string());
            }
            Ok(destination)
        })
        .as_ref()
        .map_err(Clone::clone)
        .cloned()
}

#[cfg(test)]
fn cleanup_fuzz_storage_fixture() -> Result<(), String> {
    let Some(Ok(root)) = STORAGE_FUZZ_FIXTURE_ROOT.get() else {
        return Ok(());
    };
    std::fs::remove_dir_all(root).map_err(|error| error.to_string())
}

#[cfg(test)]
fn fuzz_current_manifest(bytes: &[u8]) -> Result<bool, String> {
    use std::fs;

    let root = fuzz_storage_fixture_root()?;
    let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
    let current_path = layout.current_file();
    let manifest_path = layout
        .manifests_directory()
        .join("manifest-00000000000000000002.wdbm");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m7-16h/storage");
    let original_current = fs::read(source.join("CURRENT")).map_err(|error| error.to_string())?;
    let original_manifest = fs::read(
        source
            .join("manifests")
            .join("manifest-00000000000000000002.wdbm"),
    )
    .map_err(|error| error.to_string())?;
    let _restore = RestoreFuzzFiles(vec![
        (current_path.clone(), original_current),
        (manifest_path.clone(), original_manifest),
    ]);
    let is_manifest = bytes.starts_with(b"WDBMAN\0\x01");
    let target_path = if is_manifest {
        &manifest_path
    } else {
        &current_path
    };
    fs::write(target_path, bytes).map_err(|error| error.to_string())?;
    let accepted = ManifestStore::new(layout)
        .read_current()
        .is_ok_and(|manifest| manifest.is_some());
    Ok(accepted)
}

#[cfg(test)]
fn fuzz_manifest_name(bytes: &[u8]) -> Result<bool, String> {
    use std::fs;

    let root = fuzz_storage_fixture_root()?;
    let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
    let store = ManifestStore::new(layout.clone());
    let current = store
        .read_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "manifest-name fuzz fixture has no current generation".to_owned())?;
    let current_path = layout.current_file();
    let original_current = fs::read(&current_path).map_err(|error| error.to_string())?;
    let candidate = std::str::from_utf8(bytes)
        .ok()
        .map(str::trim)
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 240
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("manifest-fuzz-{}.wdbm", blake3::hash(bytes).to_hex()));
    let candidate_path = layout.manifests_directory().join(&candidate);
    let original_names = fs::read_dir(layout.manifests_directory())
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    let _restore = RestoreManifestDirectory {
        directory: layout.manifests_directory(),
        current_path: current_path.clone(),
        current_bytes: original_current,
        original_names,
    };
    if !candidate_path.exists() {
        fs::write(&candidate_path, []).map_err(|error| error.to_string())?;
    }
    let writer_lock = layout
        .try_writer_lock()
        .map_err(|error| error.to_string())?;
    let wal = WalPrepareLog::new(&layout);
    let snapshot = ManifestSnapshot::new(current.revision(), current.segments().to_vec())
        .map_err(|error| error.to_string())?;
    let result = store.publish(&writer_lock, &wal, snapshot).is_ok();
    drop(writer_lock);
    Ok(result)
}

#[cfg(test)]
fn fuzz_history_segment(bytes: &[u8]) -> Result<bool, String> {
    use std::fs;
    use worlddb_core::DomainId;

    let root = fuzz_storage_fixture_root()?;
    let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
    let id = SegmentId::try_from_bytes([
        0x5c, 0x25, 0xd7, 0xe9, 0xf4, 0x47, 0x4f, 0x4d, 0xb9, 0x44, 0x47, 0xda, 0x34, 0x50, 0xae,
        0xd8,
    ])
    .map_err(|error| error.to_string())?;
    let path = layout
        .segments_directory()
        .join(format!("segment-{}.wdbseg", id.to_canonical_string()));
    let original = fs::read(&path).map_err(|error| error.to_string())?;
    let _restore = RestoreFuzzFiles(vec![(path.clone(), original)]);
    fs::write(path, bytes).map_err(|error| error.to_string())?;
    Ok(HistorySegmentStore::new(layout).read_segment(id).is_ok())
}

#[cfg(test)]
fn fuzz_security_segment(bytes: &[u8]) -> Result<bool, String> {
    use std::fs;
    use worlddb_core::DomainId;

    let root = fuzz_storage_fixture_root()?;
    let layout = DatabaseLayout::open(&root).map_err(|error| error.to_string())?;
    let id = SegmentId::try_from_bytes([
        0x5c, 0xb2, 0xf9, 0xa6, 0xb0, 0x5b, 0x44, 0xce, 0x9e, 0x56, 0x17, 0xb4, 0x53, 0x26, 0x22,
        0x37,
    ])
    .map_err(|error| error.to_string())?;
    let path = layout
        .security_segments_directory()
        .join(format!("segment-{}.wdbseg", id.to_canonical_string()));
    let original = fs::read(&path).map_err(|error| error.to_string())?;
    let _restore = RestoreFuzzFiles(vec![(path.clone(), original)]);
    fs::write(path, bytes).map_err(|error| error.to_string())?;
    Ok(SecurityPolicyHistoryStore::new(layout)
        .read_version(id)
        .is_ok())
}

#[cfg(test)]
#[test]
#[ignore = "24-hour fuzz campaign; run through tools/fuzz/run-target.ps1"]
fn storage_fuzz_campaign() -> Result<(), String> {
    let campaign = fuzz_campaign_support::run_campaign(
        &[
            "storage_format_probe",
            "storage_current_manifest",
            "storage_manifest_name",
            "storage_history_segment",
            "storage_security_segment",
            "storage_wal_segment",
            "storage_backup_manifest",
            "storage_index_generation",
            "storage_index_pointer",
            "storage_index_generation_name",
            "storage_compaction_pin_manifest",
            "storage_migration_run_journal",
            "storage_guarded_migration_journal",
            "storage_storage_upgrade_journal",
            "storage_wal_recovery_prefix",
            "storage_wal_payloads",
            "storage_audit_wal",
            "storage_recovery_journal",
            "storage_recovery_pipeline",
            "storage_verify_pipeline",
            "storage_salvage_scanner",
            "storage_required_audit_payload",
            "storage_replay_payload",
            "storage_logical_export",
            "storage_logical_import_plan",
            "storage_import_prepare",
            "storage_sharing_export",
        ],
        fuzz_campaign_probe,
    );
    let cleanup = cleanup_fuzz_storage_fixture();
    campaign.map_err(|error| format!("storage fuzz campaign failed: {error}"))?;
    cleanup.map_err(|error| format!("storage fuzz fixture cleanup failed: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod fuzz_campaign_tests {
    use super::fuzz_campaign_probe;

    const TARGETS: &[&str] = &[
        "storage_format_probe",
        "storage_current_manifest",
        "storage_manifest_name",
        "storage_history_segment",
        "storage_security_segment",
        "storage_wal_segment",
        "storage_backup_manifest",
        "storage_index_generation",
        "storage_index_pointer",
        "storage_index_generation_name",
        "storage_compaction_pin_manifest",
        "storage_migration_run_journal",
        "storage_guarded_migration_journal",
        "storage_storage_upgrade_journal",
        "storage_wal_recovery_prefix",
        "storage_wal_payloads",
        "storage_audit_wal",
        "storage_recovery_journal",
        "storage_recovery_pipeline",
        "storage_verify_pipeline",
        "storage_salvage_scanner",
        "storage_required_audit_payload",
        "storage_replay_payload",
        "storage_logical_export",
        "storage_logical_import_plan",
        "storage_import_prepare",
        "storage_sharing_export",
    ];

    #[test]
    fn fuzz_campaign_dispatch_rejects_unregistered_storage_targets() {
        assert!(fuzz_campaign_probe("unknown", b"seed").is_err());
        for target in TARGETS {
            assert!(fuzz_campaign_probe(target, b"seed").is_ok());
        }
        assert!(super::cleanup_fuzz_storage_fixture().is_ok());
    }
}
