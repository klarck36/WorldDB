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
    StorageFormatProfile, StorageUpgradeAdminAction, StorageUpgradeBudget, StorageUpgradeError,
    StorageUpgradeManager, StorageUpgradePlan, StorageUpgradeReceipt, StorageUpgradeRestoreTargets,
    StorageUpgradeSafeRestorePoint, StorageUpgradeTransform,
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
