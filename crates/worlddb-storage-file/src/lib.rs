#![deny(unsafe_code)]

//! Local filesystem layout and format-probe adapter for WorldDB.

mod audit_wal;
mod compaction;
mod format;
mod index_generation;
mod index_rebuild;
mod layout;
mod manifest;
mod recovery;
mod recovery_journal;
mod recovery_manager;
mod replay_payload;
mod required_audit;
mod salvage;
mod security_segment;
mod segment;
mod verify;
mod wal;
#[cfg(windows)]
mod windows_publication;
mod writer_lock;

pub use audit_wal::{
    CommittedRawReadAttempt, RawReadAuditAccessError, RawReadAuditError, RawReadAuditHead,
    RawReadAuditPolicyError, RawReadAuditReceipt, RawReadAuditRecoveryDisposition,
    RawReadAuditRecoveryReport, RawReadAuditWal, RawReadAuditWriter,
};
pub use compaction::{
    CompactionError, CompactionManager, CompactionOutcome, ReclamationOutcome, SegmentPin,
    SegmentPinKind,
};
pub use format::{
    FORMAT_FILE_BYTES, FORMAT_FILE_KIND, FormatCapabilities, FormatProbeError, probe_format,
};
pub use index_generation::{
    INDEX_GENERATION_FRAME_KIND, IndexFileDecision, IndexGeneration, IndexGenerationError,
    decode_index_generation, encode_index_generation, select_index_generation,
};
pub use index_rebuild::{
    IndexGenerationStore, IndexRebuildError, IndexRebuildLimits, IndexRebuildManager,
    IndexRebuildReceipt, IndexRebuildSource, PinnedIndexSnapshot, StoredIndexGeneration,
};
pub use layout::{DatabaseLayout, StorageFileError};
pub use manifest::{
    Manifest, ManifestError, ManifestReceipt, ManifestSegmentKind, ManifestSegmentReference,
    ManifestSnapshot, ManifestStore,
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
pub use verify::{
    StorageDamageClass, StorageVerifier, StorageVerifyAction, StorageVerifyError,
    StorageVerifyFinding, StorageVerifyInventory, StorageVerifyIssue, StorageVerifyReport,
};
pub use wal::{
    WalCommitHash, WalCommitHead, WalCommitReceipt, WalCommittedFrame, WalError, WalLogEntry,
    WalOperationStatus, WalPrepareLog, WalPrepareReference, WalPreparedFrame,
};
pub use worlddb_core::storage_internal::SegmentId;
pub use writer_lock::{WriterLock, WriterLockError};
