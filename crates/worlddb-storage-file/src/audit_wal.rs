//! Independent append-only WAL for administrative raw-read page attempts.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use worlddb_core::{
    AdminRawAuditAuthorizer, AdminRawAuditError, AdminRawAuthorizeRequest, AuditAccessPermissions,
    AuditCodecError, AuditOperationId, AuditRetentionPolicy, AuditSequence, AuditSequenceError,
    CHECKSUM_LEN, ClientRequestId, DatabaseId, DecoderLimits, DomainId, FRAME_HEADER_LEN,
    FrameError, FrameHeader, IdGenerationError, PolicyTarget, RawReadAttempt,
    RawReadAttemptIdentity, RawReadAttemptScope, SecurityPolicyView, TlvDecoder, TlvEncoder,
    WireError, decode_frame, decode_raw_read_attempt, encode_frame, encode_raw_read_attempt,
};

use crate::layout::DatabaseLayout;
use crate::manifest::sync_directory;

const AUDIT_LOCK_FILE: &str = "LOCK";
const AUDIT_WAL_FILE: &str = "raw-read.wal";
const PREPARE_FRAME_KIND: u32 = 0x5744_4150;
const COMMIT_FRAME_KIND: u32 = 0x5744_4143;
const RECOVERY_INTENT_FRAME_KIND: u32 = 0x5744_4152;
const RECOVERY_DONE_FRAME_KIND: u32 = 0x5744_4153;
const COMMIT_HASH_CONTEXT: &[u8] = b"worlddb.audit.raw_read.commit.v1";
const ZERO_COMMIT_HASH: [u8; 32] = [0; 32];
const MAX_AUDIT_WAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_AUDIT_FRAME_BYTES: usize = DecoderLimits::DEFAULT.max_frame_bytes;
const FRAME_LENGTH_START: usize = 32;

/// Independent position of the raw-read audit log. It does not contain a WorldDB revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawReadAuditHead {
    sequence: AuditSequence,
    commit_hash: [u8; 32],
}

impl RawReadAuditHead {
    /// Highest fully committed raw-read attempt sequence, zero for an empty log.
    #[must_use]
    pub const fn sequence(self) -> AuditSequence {
        self.sequence
    }

    /// Hash at the highest fully committed audit sequence.
    #[must_use]
    pub const fn commit_hash(&self) -> &[u8; 32] {
        &self.commit_hash
    }
}

/// In-memory, bounded audit prefix captured for an AuditCompleteBackup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawReadAuditSnapshot {
    database_id: Option<DatabaseId>,
    head: RawReadAuditHead,
    bytes: Vec<u8>,
}

impl RawReadAuditSnapshot {
    /// Database identity associated with this audit namespace, when the layout has one.
    #[must_use]
    pub const fn database_id(&self) -> Option<DatabaseId> {
        self.database_id
    }

    /// Independent audit sequence and commit hash at the snapshot boundary.
    #[must_use]
    pub const fn head(&self) -> RawReadAuditHead {
        self.head
    }

    /// Exact, fully verified WAL bytes through `head`.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn into_parts(self) -> (Option<DatabaseId>, RawReadAuditHead, Vec<u8>) {
        (self.database_id, self.head, self.bytes)
    }
}

/// Receipt returned only after the attempt's commit marker has synced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawReadAuditReceipt {
    sequence: AuditSequence,
    audit_operation_id: AuditOperationId,
    commit_hash: [u8; 32],
}

impl RawReadAuditReceipt {
    /// Durable audit sequence assigned to this page attempt.
    #[must_use]
    pub const fn sequence(self) -> AuditSequence {
        self.sequence
    }

    /// Fresh identity for this attempt, distinct across retries.
    #[must_use]
    pub const fn audit_operation_id(self) -> AuditOperationId {
        self.audit_operation_id
    }

    /// Commit-chain hash produced by the synced commit marker.
    #[must_use]
    pub const fn commit_hash(&self) -> &[u8; 32] {
        &self.commit_hash
    }
}

/// One raw-read attempt whose prepare and commit marker both verify.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedRawReadAttempt {
    attempt: RawReadAttempt,
    receipt: RawReadAuditReceipt,
}

/// Outcome of validating and, when safe, quarantining an incomplete audit-WAL tail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawReadAuditRecoveryDisposition {
    /// The audit WAL already ended at a fully committed frame boundary.
    Clean,
    /// An incomplete or uncommitted suffix was quarantined and removed from the active WAL.
    RecoveredIncompleteTail,
}

/// Durable audit recovery result. Its sequence is independent of the WorldDB data revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawReadAuditRecoveryReport {
    disposition: RawReadAuditRecoveryDisposition,
    head: RawReadAuditHead,
    discarded_tail_bytes: u64,
    discarded_tail_digest: [u8; 32],
}

impl RawReadAuditRecoveryReport {
    /// Whether the current audit WAL was already clean or had an incomplete suffix recovered.
    #[must_use]
    pub const fn disposition(self) -> RawReadAuditRecoveryDisposition {
        self.disposition
    }

    /// Highest fully committed attempt after recovery.
    #[must_use]
    pub const fn head(self) -> RawReadAuditHead {
        self.head
    }

    /// Number of suffix bytes preserved in the audit quarantine, zero when clean.
    #[must_use]
    pub const fn discarded_tail_bytes(self) -> u64 {
        self.discarded_tail_bytes
    }

    /// Digest of the quarantined suffix, or zero when the WAL was clean.
    #[must_use]
    pub const fn discarded_tail_digest(&self) -> &[u8; 32] {
        &self.discarded_tail_digest
    }
}

/// Authorization or storage failure while reading or exporting the audit namespace.
#[derive(Debug)]
pub enum RawReadAuditAccessError {
    /// Current policy does not grant AuditRead.
    ReadUnauthorized,
    /// Current policy does not grant AuditExport in addition to AuditRead.
    ExportUnauthorized,
    /// The audit prefix could not be recovered or read safely.
    Audit(RawReadAuditError),
}

impl fmt::Display for RawReadAuditAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadUnauthorized => formatter.write_str("AuditRead is required"),
            Self::ExportUnauthorized => {
                formatter.write_str("AuditRead and AuditExport are required")
            }
            Self::Audit(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RawReadAuditAccessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Audit(error) => Some(error),
            Self::ReadUnauthorized | Self::ExportUnauthorized => None,
        }
    }
}

/// Audit retention change rejected before it can be persisted as a policy version.
#[derive(Debug)]
pub enum RawReadAuditPolicyError {
    /// Current policy does not grant AuditConfigure.
    ConfigureUnauthorized,
    /// A proposed policy would reduce the previously configured minimum retention floor.
    MinimumRetentionReduced {
        /// Previously configured minimum retention duration.
        current_millis: u64,
        /// Proposed minimum retention duration.
        proposed_millis: u64,
    },
    /// The proposed maximum is below the number of currently retained attempts.
    MaximumRecordsBelowCurrentCount {
        /// Existing verified attempt count.
        retained_records: usize,
        /// Proposed maximum record count.
        proposed_maximum: usize,
    },
    /// The current audit prefix could not be recovered or counted.
    Audit(RawReadAuditError),
}

impl fmt::Display for RawReadAuditPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigureUnauthorized => formatter.write_str("AuditConfigure is required"),
            Self::MinimumRetentionReduced {
                current_millis,
                proposed_millis,
            } => write!(
                formatter,
                "audit minimum retention cannot be reduced from {current_millis} ms to {proposed_millis} ms"
            ),
            Self::MaximumRecordsBelowCurrentCount {
                retained_records,
                proposed_maximum,
            } => write!(
                formatter,
                "audit maximum {proposed_maximum} is below the {retained_records} currently retained records"
            ),
            Self::Audit(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RawReadAuditPolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Audit(error) => Some(error),
            Self::ConfigureUnauthorized
            | Self::MinimumRetentionReduced { .. }
            | Self::MaximumRecordsBelowCurrentCount { .. } => None,
        }
    }
}

impl CommittedRawReadAttempt {
    /// Safe request metadata durably recorded before the page was released.
    #[must_use]
    pub const fn attempt(&self) -> &RawReadAttempt {
        &self.attempt
    }

    /// Commit receipt in the independent raw-read audit sequence domain.
    #[must_use]
    pub const fn receipt(&self) -> RawReadAuditReceipt {
        self.receipt
    }
}

/// Failure to acquire, append, synchronize, or verify the independent raw-read audit WAL.
#[derive(Debug)]
pub enum RawReadAuditError {
    /// Another process currently owns `audit/LOCK`.
    WriterAlreadyHeld,
    /// A lock or WAL path is not a regular in-database filesystem entry.
    InvalidPath,
    /// A filesystem operation failed.
    Io {
        /// Short operation description.
        operation: &'static str,
        /// Operating-system failure.
        source: io::Error,
    },
    /// The active audit WAL changed during validation or exceeds a safe recoverable boundary.
    RecoveryRequired,
    /// A complete frame, prepare/commit pair, or commit-chain value is invalid.
    CorruptWAL,
    /// The audit WAL exceeds the bounded 64 MiB log size supported by this task.
    WALTooLarge { limit: usize, actual: usize },
    /// The independent `AuditSequence` is exhausted.
    SequenceExhausted(AuditSequenceError),
    /// A generated AuditRecordId or AuditOperationId could not be allocated.
    Identity(IdGenerationError),
    /// A canonical raw-read audit frame failed decoding or encoding.
    AuditCodec(AuditCodecError),
    /// A WAL frame checksum or framing rule failed.
    Frame(FrameError),
    /// A WAL TLV payload is malformed or non-canonical.
    Wire(WireError),
    /// The process-local audit writer mutex was poisoned.
    WriterPoisoned,
    /// The WAL payload could not reserve bounded memory.
    AllocationFailed,
}

impl fmt::Display for RawReadAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriterAlreadyHeld => formatter.write_str("raw-read audit writer is already held"),
            Self::InvalidPath => {
                formatter.write_str("raw-read audit path is not a regular in-database entry")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::RecoveryRequired => formatter
                .write_str("raw-read audit WAL changed during validation or needs recovery"),
            Self::CorruptWAL => {
                formatter.write_str("raw-read audit WAL has a corrupt committed prefix")
            }
            Self::WALTooLarge { limit, actual } => write!(
                formatter,
                "raw-read audit WAL exceeds {limit} bytes with {actual}"
            ),
            Self::SequenceExhausted(error) => {
                write!(formatter, "raw-read audit sequence exhausted: {error}")
            }
            Self::Identity(error) => write!(
                formatter,
                "raw-read audit identity generation failed: {error}"
            ),
            Self::AuditCodec(error) => write!(formatter, "raw-read attempt codec failed: {error}"),
            Self::Frame(error) => write!(formatter, "raw-read audit frame failed: {error}"),
            Self::Wire(error) => write!(formatter, "raw-read audit WAL payload failed: {error}"),
            Self::WriterPoisoned => formatter
                .write_str("raw-read audit writer is unavailable after an interrupted append"),
            Self::AllocationFailed => formatter.write_str("raw-read audit WAL allocation failed"),
        }
    }
}

impl std::error::Error for RawReadAuditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::SequenceExhausted(error) => Some(error),
            Self::Identity(error) => Some(error),
            Self::AuditCodec(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::WriterAlreadyHeld
            | Self::InvalidPath
            | Self::RecoveryRequired
            | Self::CorruptWAL
            | Self::WALTooLarge { .. }
            | Self::WriterPoisoned
            | Self::AllocationFailed => None,
        }
    }
}

/// Independent, revision-free append-only raw-read audit WAL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawReadAuditWal {
    layout: DatabaseLayout,
}

impl RawReadAuditWal {
    /// Binds the audit WAL to an opened WorldDB directory layout.
    #[must_use]
    pub const fn new(layout: DatabaseLayout) -> Self {
        Self { layout }
    }

    /// Acquires the separate serialized audit writer and verifies the complete committed prefix.
    ///
    /// An incomplete/uncommitted suffix is quarantined and recovered before the writer becomes
    /// available. Corruption inside the committed prefix remains fail-closed and is never repaired.
    pub fn try_writer(&self) -> Result<RawReadAuditWriter, RawReadAuditError> {
        let lock = AuditWriterLock::try_acquire(&self.layout)?;
        recover_tail_locked(&self.layout)?;
        let scan = scan_wal(&self.layout)?;
        Ok(RawReadAuditWriter {
            layout: self.layout.clone(),
            state: Mutex::new(AuditWriterState {
                _lock: lock,
                head: scan.head,
                poisoned: false,
            }),
        })
    }

    /// Recovers an incomplete tail and reads committed attempts under the independent audit lock.
    fn read_committed_attempts_unchecked(
        &self,
    ) -> Result<(RawReadAuditHead, Vec<CommittedRawReadAttempt>), RawReadAuditError> {
        let _lock = AuditWriterLock::try_acquire(&self.layout)?;
        recover_tail_locked(&self.layout)?;
        let scan = scan_wal(&self.layout)?;
        Ok((scan.head, scan.attempts))
    }

    /// Recovers an incomplete/uncommitted tail under the independent audit lock.
    ///
    /// Complete committed-frame corruption returns an error and leaves the audit WAL byte-for-byte
    /// unchanged. The quarantine, intent, truncation, and completion records are separately synced.
    pub fn recover(&self) -> Result<RawReadAuditRecoveryReport, RawReadAuditError> {
        let _lock = AuditWriterLock::try_acquire(&self.layout)?;
        recover_tail_locked(&self.layout)
    }

    /// Captures a clean audit prefix while excluding another process's audit writer.
    ///
    /// This method does not repair an incomplete tail. Use the live writer's equivalent method
    /// when this process owns the audit writer lock.
    pub fn snapshot_for_backup(
        &self,
        policy: SecurityPolicyView<'_>,
        target: PolicyTarget,
    ) -> Result<RawReadAuditSnapshot, RawReadAuditAccessError> {
        let permissions = AuditAccessPermissions::evaluate(
            policy.current_snapshot(),
            policy.principal_id(),
            target,
        );
        if !permissions.may_read() {
            return Err(RawReadAuditAccessError::ReadUnauthorized);
        }
        if !permissions.may_export() {
            return Err(RawReadAuditAccessError::ExportUnauthorized);
        }
        let _lock =
            AuditWriterLock::try_acquire(&self.layout).map_err(RawReadAuditAccessError::Audit)?;
        let scan = scan_wal(&self.layout).map_err(RawReadAuditAccessError::Audit)?;
        Ok(snapshot_from_scan(&self.layout, scan))
    }

    /// Independently verifies raw audit WAL bytes and returns their committed head.
    pub(crate) fn verify_backup_bytes(bytes: &[u8]) -> Result<RawReadAuditHead, RawReadAuditError> {
        if bytes.len() > MAX_AUDIT_WAL_BYTES {
            return Err(RawReadAuditError::WALTooLarge {
                limit: MAX_AUDIT_WAL_BYTES,
                actual: bytes.len(),
            });
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|_| RawReadAuditError::AllocationFailed)?;
        owned.extend_from_slice(bytes);
        let scan = scan_wal_bytes(owned)?;
        if scan.incomplete_tail || scan.committed_length != bytes.len() {
            return Err(RawReadAuditError::RecoveryRequired);
        }
        Ok(scan.head)
    }

    /// Reads the audit history after current-version policy authorizes AuditRead.
    pub fn read_authorized(
        &self,
        policy: SecurityPolicyView<'_>,
        target: PolicyTarget,
    ) -> Result<(RawReadAuditHead, Vec<CommittedRawReadAttempt>), RawReadAuditAccessError> {
        let permissions = AuditAccessPermissions::evaluate(
            policy.current_snapshot(),
            policy.principal_id(),
            target,
        );
        if !permissions.may_read() {
            return Err(RawReadAuditAccessError::ReadUnauthorized);
        }
        self.read_committed_attempts_unchecked()
            .map_err(RawReadAuditAccessError::Audit)
    }

    /// Exports audit history only when both current AuditRead and AuditExport are granted.
    pub fn export_authorized(
        &self,
        policy: SecurityPolicyView<'_>,
        target: PolicyTarget,
    ) -> Result<(RawReadAuditHead, Vec<CommittedRawReadAttempt>), RawReadAuditAccessError> {
        let permissions = AuditAccessPermissions::evaluate(
            policy.current_snapshot(),
            policy.principal_id(),
            target,
        );
        if !permissions.may_read() {
            return Err(RawReadAuditAccessError::ReadUnauthorized);
        }
        if !permissions.may_export() {
            return Err(RawReadAuditAccessError::ExportUnauthorized);
        }
        self.read_committed_attempts_unchecked()
            .map_err(RawReadAuditAccessError::Audit)
    }

    /// Checks AuditConfigure and validates a proposed retention policy before policy-versioned
    /// persistence. The caller commits the successful change with a Required Audit Record using
    /// the shared-WAL transaction boundary.
    pub fn validate_retention_change(
        &self,
        policy: SecurityPolicyView<'_>,
        current: Option<AuditRetentionPolicy>,
        proposed: AuditRetentionPolicy,
    ) -> Result<(), RawReadAuditPolicyError> {
        let permissions = AuditAccessPermissions::evaluate(
            policy.current_snapshot(),
            policy.principal_id(),
            PolicyTarget::default(),
        );
        if !permissions.may_configure() {
            return Err(RawReadAuditPolicyError::ConfigureUnauthorized);
        }
        if let Some(current) = current {
            if proposed.minimum_retention_millis() < current.minimum_retention_millis() {
                return Err(RawReadAuditPolicyError::MinimumRetentionReduced {
                    current_millis: current.minimum_retention_millis(),
                    proposed_millis: proposed.minimum_retention_millis(),
                });
            }
        }
        let (_, attempts) = self
            .read_committed_attempts_unchecked()
            .map_err(RawReadAuditPolicyError::Audit)?;
        if proposed.maximum_records() < attempts.len() {
            return Err(RawReadAuditPolicyError::MaximumRecordsBelowCurrentCount {
                retained_records: attempts.len(),
                proposed_maximum: proposed.maximum_records(),
            });
        }
        Ok(())
    }
}

/// Serialized writer used by the administrative raw-read release boundary.
pub struct RawReadAuditWriter {
    layout: DatabaseLayout,
    state: Mutex<AuditWriterState>,
}

struct AuditWriterState {
    // Keeping this handle alive holds the independent OS lock while requests are authorized.
    _lock: AuditWriterLock,
    head: RawReadAuditHead,
    poisoned: bool,
}

impl RawReadAuditWriter {
    /// Highest committed independent audit sequence held by this writer.
    pub fn head(&self) -> Result<RawReadAuditHead, RawReadAuditError> {
        let state = self.lock_state()?;
        if state.poisoned {
            return Err(RawReadAuditError::WriterPoisoned);
        }
        Ok(state.head)
    }

    /// Captures the current committed audit prefix while serializing against this writer's appends.
    pub fn snapshot_for_backup(
        &self,
        policy: SecurityPolicyView<'_>,
        target: PolicyTarget,
    ) -> Result<RawReadAuditSnapshot, RawReadAuditAccessError> {
        let permissions = AuditAccessPermissions::evaluate(
            policy.current_snapshot(),
            policy.principal_id(),
            target,
        );
        if !permissions.may_read() {
            return Err(RawReadAuditAccessError::ReadUnauthorized);
        }
        if !permissions.may_export() {
            return Err(RawReadAuditAccessError::ExportUnauthorized);
        }
        let state = self.lock_state().map_err(RawReadAuditAccessError::Audit)?;
        if state.poisoned {
            return Err(RawReadAuditAccessError::Audit(
                RawReadAuditError::WriterPoisoned,
            ));
        }
        let scan = scan_wal(&self.layout).map_err(RawReadAuditAccessError::Audit)?;
        if scan.head != state.head {
            return Err(RawReadAuditAccessError::Audit(
                RawReadAuditError::RecoveryRequired,
            ));
        }
        Ok(snapshot_from_scan(&self.layout, scan))
    }

    /// Appends one attempt and returns only after both prepare and commit marker syncs succeed.
    pub fn append_attempt(
        &self,
        scope: RawReadAttemptScope,
        client_request_id: ClientRequestId,
    ) -> Result<RawReadAuditReceipt, RawReadAuditError> {
        let mut state = self.lock_state()?;
        if state.poisoned {
            return Err(RawReadAuditError::WriterPoisoned);
        }
        let sequence = state
            .head
            .sequence
            .next()
            .map_err(RawReadAuditError::SequenceExhausted)?;
        let record_id = worlddb_core::storage_internal::generate_raw_read_audit_record_id()
            .map_err(RawReadAuditError::Identity)?;
        let audit_operation_id =
            worlddb_core::storage_internal::generate_raw_read_audit_operation_id()
                .map_err(RawReadAuditError::Identity)?;
        let attempt = RawReadAttempt::new(
            RawReadAttemptIdentity {
                record_id,
                sequence,
                audit_operation_id,
                client_request_id,
            },
            scope,
        );
        match append_attempt(&self.layout, &attempt, state.head) {
            Ok(receipt) => {
                state.head = RawReadAuditHead {
                    sequence,
                    commit_hash: *receipt.commit_hash(),
                };
                Ok(receipt)
            }
            Err(error) => {
                // An append or sync error has an unknown persistent outcome. This writer must
                // stop immediately; a fresh writer will scan and either continue or fail closed.
                state.poisoned = true;
                Err(error)
            }
        }
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, AuditWriterState>, RawReadAuditError> {
        self.state
            .lock()
            .map_err(|_| RawReadAuditError::WriterPoisoned)
    }
}

impl AdminRawAuditAuthorizer for RawReadAuditWriter {
    fn authorize(&self, request: &AdminRawAuthorizeRequest) -> Result<(), AdminRawAuditError> {
        self.append_attempt(
            RawReadAttemptScope {
                principal_id: request.principal_id(),
                scope_fingerprint: request.scope_fingerprint().clone(),
                snapshot_id: request.snapshot_id(),
                // Bind the currently effective SecurityEpoch used for the release decision.
                security_epoch: request.current_security_epoch(),
                page_ordinal: request.page_ordinal(),
            },
            request.client_request_id(),
        )
        .map(|_| ())
        .map_err(|_| AdminRawAuditError)
    }
}

struct AuditWriterLock {
    _file: File,
}

impl AuditWriterLock {
    fn try_acquire(layout: &DatabaseLayout) -> Result<Self, RawReadAuditError> {
        let audit_root = canonical_regular_directory(&layout.audit_directory())?;
        let wal_directory = canonical_regular_directory(&layout.audit_wal_directory())?;
        if !wal_directory.starts_with(&audit_root) {
            return Err(RawReadAuditError::InvalidPath);
        }
        let path = layout.audit_directory().join(AUDIT_LOCK_FILE);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RawReadAuditError::InvalidPath);
            }
        }
        let file = open_lock_file(&path).map_err(|source| RawReadAuditError::Io {
            operation: "open raw-read audit lock",
            source,
        })?;
        let canonical_path = fs::canonicalize(&path).map_err(|source| RawReadAuditError::Io {
            operation: "resolve raw-read audit lock",
            source,
        })?;
        if canonical_path.parent() != Some(audit_root.as_path()) {
            return Err(RawReadAuditError::InvalidPath);
        }
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => Ok(Self { _file: file }),
            Err(fs4::TryLockError::WouldBlock) => Err(RawReadAuditError::WriterAlreadyHeld),
            Err(fs4::TryLockError::Error(source)) => Err(RawReadAuditError::Io {
                operation: "acquire raw-read audit lock",
                source,
            }),
        }
    }
}

struct AuditScan {
    head: RawReadAuditHead,
    attempts: Vec<CommittedRawReadAttempt>,
    file_bytes: Vec<u8>,
    committed_length: usize,
    incomplete_tail: bool,
}

struct PendingAttempt {
    sequence: AuditSequence,
    audit_operation_id: AuditOperationId,
    payload_hash: [u8; 32],
    attempt: RawReadAttempt,
}

struct DecodedCommit {
    sequence: AuditSequence,
    audit_operation_id: AuditOperationId,
    payload_hash: [u8; 32],
    previous_hash: [u8; 32],
    commit_hash: [u8; 32],
}

fn scan_wal(layout: &DatabaseLayout) -> Result<AuditScan, RawReadAuditError> {
    let scan = scan_wal_allow_incomplete_tail(layout)?;
    if scan.incomplete_tail {
        return Err(RawReadAuditError::RecoveryRequired);
    }
    Ok(scan)
}

fn scan_wal_allow_incomplete_tail(layout: &DatabaseLayout) -> Result<AuditScan, RawReadAuditError> {
    let directory = canonical_regular_directory(&layout.audit_wal_directory())?;
    let path = directory.join(AUDIT_WAL_FILE);
    let bytes = read_wal_file(&path, &directory)?.unwrap_or_default();
    scan_wal_bytes(bytes)
}

fn scan_wal_bytes(bytes: Vec<u8>) -> Result<AuditScan, RawReadAuditError> {
    if bytes.len() > MAX_AUDIT_WAL_BYTES {
        return Err(RawReadAuditError::WALTooLarge {
            limit: MAX_AUDIT_WAL_BYTES,
            actual: bytes.len(),
        });
    }
    if bytes.is_empty() {
        return Ok(AuditScan {
            head: RawReadAuditHead {
                sequence: AuditSequence::new(0),
                commit_hash: ZERO_COMMIT_HASH,
            },
            attempts: Vec::new(),
            file_bytes: Vec::new(),
            committed_length: 0,
            incomplete_tail: false,
        });
    }
    let mut attempts = Vec::new();
    let mut operation_ids = BTreeSet::new();
    let mut record_ids = BTreeSet::new();
    let mut head = RawReadAuditHead {
        sequence: AuditSequence::new(0),
        commit_hash: ZERO_COMMIT_HASH,
    };
    let mut pending: Option<PendingAttempt> = None;
    let mut pending_start: Option<usize> = None;
    let mut committed_length = 0_usize;
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let (frame_bytes, frame_end) = match next_frame(&bytes, offset) {
            Ok(Some(frame)) => frame,
            Ok(None) => return Err(RawReadAuditError::CorruptWAL),
            Err(RawReadAuditError::RecoveryRequired) => {
                return Ok(AuditScan {
                    head,
                    attempts,
                    file_bytes: bytes,
                    committed_length: pending_start.unwrap_or(committed_length),
                    incomplete_tail: true,
                });
            }
            Err(error) => return Err(error),
        };
        let frame = decode_frame(frame_bytes).map_err(RawReadAuditError::Frame)?;
        match frame.header().kind() {
            PREPARE_FRAME_KIND => {
                if pending.is_some() {
                    return Err(RawReadAuditError::CorruptWAL);
                }
                let (sequence, audit_operation_id, raw_attempt) = decode_prepare(frame.payload())?;
                let expected_sequence = head
                    .sequence
                    .next()
                    .map_err(RawReadAuditError::SequenceExhausted)?;
                if sequence != expected_sequence || operation_ids.contains(&audit_operation_id) {
                    return Err(RawReadAuditError::CorruptWAL);
                }
                pending_start = Some(offset);
                let attempt =
                    decode_raw_read_attempt(raw_attempt).map_err(RawReadAuditError::AuditCodec)?;
                if attempt.sequence() != sequence
                    || attempt.audit_operation_id() != audit_operation_id
                    || record_ids.contains(&attempt.record_id())
                {
                    return Err(RawReadAuditError::CorruptWAL);
                }
                let payload_hash = *blake3::hash(raw_attempt).as_bytes();
                pending = Some(PendingAttempt {
                    sequence,
                    audit_operation_id,
                    payload_hash,
                    attempt,
                });
            }
            COMMIT_FRAME_KIND => {
                let Some(prepared) = pending.take() else {
                    return Err(RawReadAuditError::CorruptWAL);
                };
                let commit = decode_commit(frame.payload())?;
                if commit.sequence != prepared.sequence
                    || commit.audit_operation_id != prepared.audit_operation_id
                    || commit.payload_hash != prepared.payload_hash
                    || commit.previous_hash != head.commit_hash
                    || commit.commit_hash
                        != compute_commit_hash(
                            commit.previous_hash,
                            commit.sequence,
                            commit.audit_operation_id,
                            commit.payload_hash,
                        )
                {
                    return Err(RawReadAuditError::CorruptWAL);
                }
                operation_ids.insert(commit.audit_operation_id);
                record_ids.insert(prepared.attempt.record_id());
                head = RawReadAuditHead {
                    sequence: commit.sequence,
                    commit_hash: commit.commit_hash,
                };
                committed_length = frame_end;
                pending_start = None;
                attempts
                    .try_reserve(1)
                    .map_err(|_| RawReadAuditError::AllocationFailed)?;
                attempts.push(CommittedRawReadAttempt {
                    attempt: prepared.attempt,
                    receipt: RawReadAuditReceipt {
                        sequence: commit.sequence,
                        audit_operation_id: commit.audit_operation_id,
                        commit_hash: commit.commit_hash,
                    },
                });
            }
            _ => return Err(RawReadAuditError::CorruptWAL),
        }
        offset = frame_end;
    }
    if pending.is_some() {
        return Ok(AuditScan {
            head,
            attempts,
            file_bytes: bytes,
            committed_length: pending_start.unwrap_or(committed_length),
            incomplete_tail: true,
        });
    }
    Ok(AuditScan {
        head,
        attempts,
        file_bytes: bytes,
        committed_length,
        incomplete_tail: false,
    })
}

#[cfg(test)]
pub(crate) fn fuzz_audit_wal(layout: &DatabaseLayout, bytes: &[u8]) -> bool {
    let path = layout.audit_wal_directory().join(AUDIT_WAL_FILE);
    if fs::write(&path, bytes).is_err() {
        return false;
    }
    let path_cleanup = AuditFuzzWalFile(path);
    let scan_path = scan_wal(layout).is_ok() || scan_wal_allow_incomplete_tail(layout).is_ok();
    let scan_bytes = scan_wal_bytes(bytes.to_vec()).is_ok();
    let repair = decode_repair_intent(bytes).is_ok();
    let prepare = decode_prepare(bytes).is_ok();
    let commit = decode_commit(bytes).is_ok();
    let field = || {
        let mut decoder = TlvDecoder::new(bytes);
        decode_u64_field(&mut decoder, 1).is_ok()
    };
    let id_field = || {
        let mut decoder = TlvDecoder::new(bytes);
        decode_id_field::<AuditOperationId>(&mut decoder, 1).is_ok()
    };
    let fixed_field = || {
        let mut decoder = TlvDecoder::new(bytes);
        decode_fixed_field::<32>(&mut decoder, 1).is_ok()
    };
    let bytes_field = || {
        let mut decoder = TlvDecoder::new(bytes);
        decode_bytes_field(&mut decoder, 1).is_ok()
    };
    drop(path_cleanup);
    scan_path
        || scan_bytes
        || repair
        || prepare
        || commit
        || field()
        || id_field()
        || fixed_field()
        || bytes_field()
}

#[cfg(test)]
struct AuditFuzzWalFile(PathBuf);

#[cfg(test)]
impl Drop for AuditFuzzWalFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn snapshot_from_scan(layout: &DatabaseLayout, scan: AuditScan) -> RawReadAuditSnapshot {
    RawReadAuditSnapshot {
        database_id: layout.database_id(),
        head: scan.head,
        bytes: scan.file_bytes,
    }
}

fn recover_tail_locked(
    layout: &DatabaseLayout,
) -> Result<RawReadAuditRecoveryReport, RawReadAuditError> {
    recover_tail_locked_with_checkpoint(layout, |_| {})
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuditRecoveryCheckpoint {
    QuarantineCopyPublished,
    IntentPublished,
    TailTruncated,
    RepairCompleted,
}

fn recover_tail_locked_with_checkpoint(
    layout: &DatabaseLayout,
    mut checkpoint: impl FnMut(AuditRecoveryCheckpoint),
) -> Result<RawReadAuditRecoveryReport, RawReadAuditError> {
    complete_pending_repairs(layout, &mut checkpoint)?;
    let scan = scan_wal_allow_incomplete_tail(layout)?;
    if !scan.incomplete_tail {
        return Ok(RawReadAuditRecoveryReport {
            disposition: RawReadAuditRecoveryDisposition::Clean,
            head: scan.head,
            discarded_tail_bytes: 0,
            discarded_tail_digest: ZERO_COMMIT_HASH,
        });
    }

    let safe_length = scan.committed_length;
    let tail = scan
        .file_bytes
        .get(safe_length..)
        .ok_or(RawReadAuditError::CorruptWAL)?;
    if tail.is_empty() {
        return Err(RawReadAuditError::CorruptWAL);
    }
    let safe_length_u64 = u64::try_from(safe_length).map_err(|_| RawReadAuditError::CorruptWAL)?;
    let original_length =
        u64::try_from(scan.file_bytes.len()).map_err(|_| RawReadAuditError::CorruptWAL)?;
    let tail_digest = *blake3::hash(tail).as_bytes();
    let prefix_bytes = scan
        .file_bytes
        .get(..safe_length)
        .ok_or(RawReadAuditError::CorruptWAL)?;
    let prefix_digest = *blake3::hash(prefix_bytes).as_bytes();
    let repair = AuditTailRepair {
        safe_length: safe_length_u64,
        original_length,
        prefix_digest,
        tail_digest,
    };
    let quarantine_directory = ensure_audit_quarantine_directory(layout)?;
    let paths = repair_paths(&quarantine_directory, repair);

    // Keep a verifiable copy first, then persist the intent, then shorten the active WAL.
    // A crash before the intent leaves the active WAL intact; a crash after it is resumed below.
    persist_quarantined_tail(&paths, tail)?;
    checkpoint(AuditRecoveryCheckpoint::QuarantineCopyPublished);
    let intent_bytes = encode_repair_intent(repair)?;
    persist_immutable_marker(
        &paths.intent,
        &paths.intent_stage,
        &intent_bytes,
        &quarantine_directory,
    )?;
    checkpoint(AuditRecoveryCheckpoint::IntentPublished);
    truncate_wal_to(layout, safe_length_u64, original_length)?;
    checkpoint(AuditRecoveryCheckpoint::TailTruncated);
    let done_bytes = encode_repair_done(&intent_bytes, prefix_digest)?;
    persist_immutable_marker(
        &paths.done,
        &paths.done_stage,
        &done_bytes,
        &quarantine_directory,
    )?;
    checkpoint(AuditRecoveryCheckpoint::RepairCompleted);

    let verified = scan_wal(layout)?;
    if verified.head != scan.head || verified.attempts.len() != scan.attempts.len() {
        return Err(RawReadAuditError::CorruptWAL);
    }
    Ok(RawReadAuditRecoveryReport {
        disposition: RawReadAuditRecoveryDisposition::RecoveredIncompleteTail,
        head: verified.head,
        discarded_tail_bytes: u64::try_from(tail.len())
            .map_err(|_| RawReadAuditError::CorruptWAL)?,
        discarded_tail_digest: tail_digest,
    })
}

#[derive(Clone, Copy)]
struct AuditTailRepair {
    safe_length: u64,
    original_length: u64,
    prefix_digest: [u8; 32],
    tail_digest: [u8; 32],
}

struct AuditTailRepairPaths {
    quarantine: PathBuf,
    quarantine_stage: PathBuf,
    intent: PathBuf,
    intent_stage: PathBuf,
    done: PathBuf,
    done_stage: PathBuf,
}

fn ensure_audit_quarantine_directory(
    layout: &DatabaseLayout,
) -> Result<PathBuf, RawReadAuditError> {
    let wal_directory = canonical_regular_directory(&layout.audit_wal_directory())?;
    let path = wal_directory.join("quarantine");
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|source| RawReadAuditError::Io {
                operation: "create raw-read audit quarantine directory",
                source,
            })?;
            sync_directory(&wal_directory).map_err(|source| RawReadAuditError::Io {
                operation: "sync audit WAL directory after quarantine creation",
                source,
            })?;
        }
        Err(source) => {
            return Err(RawReadAuditError::Io {
                operation: "inspect raw-read audit quarantine directory",
                source,
            });
        }
    }
    let canonical = canonical_regular_directory(&path)?;
    if !canonical.starts_with(&wal_directory) {
        return Err(RawReadAuditError::InvalidPath);
    }
    Ok(canonical)
}

fn repair_paths(directory: &Path, repair: AuditTailRepair) -> AuditTailRepairPaths {
    let stem = format!(
        "tail-{:016x}-{}",
        repair.safe_length,
        hex_digest(&repair.tail_digest)
    );
    AuditTailRepairPaths {
        quarantine: directory.join(format!("{stem}.bin")),
        quarantine_stage: directory.join(format!("{stem}.stage")),
        intent: directory.join(format!("{stem}.intent")),
        intent_stage: directory.join(format!("{stem}.intent.stage")),
        done: directory.join(format!("{stem}.done")),
        done_stage: directory.join(format!("{stem}.done.stage")),
    }
}

fn persist_quarantined_tail(
    paths: &AuditTailRepairPaths,
    tail: &[u8],
) -> Result<(), RawReadAuditError> {
    if let Some(existing) = read_quarantine_file(&paths.quarantine)? {
        if existing != tail {
            return Err(RawReadAuditError::CorruptWAL);
        }
        return Ok(());
    }
    remove_regular_stage_if_present(&paths.quarantine_stage)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&paths.quarantine_stage)
        .map_err(|source| RawReadAuditError::Io {
            operation: "create staged raw-read audit quarantine copy",
            source,
        })?;
    file.write_all(tail)
        .map_err(|source| RawReadAuditError::Io {
            operation: "write staged raw-read audit quarantine copy",
            source,
        })?;
    file.sync_all().map_err(|source| RawReadAuditError::Io {
        operation: "sync staged raw-read audit quarantine copy",
        source,
    })?;
    fs::rename(&paths.quarantine_stage, &paths.quarantine).map_err(|source| {
        RawReadAuditError::Io {
            operation: "publish raw-read audit quarantine copy",
            source,
        }
    })?;
    let directory = paths
        .quarantine
        .parent()
        .ok_or(RawReadAuditError::InvalidPath)?;
    sync_directory(directory).map_err(|source| RawReadAuditError::Io {
        operation: "sync raw-read audit quarantine directory",
        source,
    })?;
    Ok(())
}

fn complete_pending_repairs(
    layout: &DatabaseLayout,
    checkpoint: &mut impl FnMut(AuditRecoveryCheckpoint),
) -> Result<(), RawReadAuditError> {
    let wal_directory = canonical_regular_directory(&layout.audit_wal_directory())?;
    let quarantine_directory = wal_directory.join("quarantine");
    match fs::symlink_metadata(&quarantine_directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(RawReadAuditError::Io {
                operation: "inspect audit quarantine directory during recovery",
                source,
            });
        }
        Ok(_) => {}
    }
    let quarantine_directory = canonical_regular_directory(&quarantine_directory)?;
    if !quarantine_directory.starts_with(&wal_directory) {
        return Err(RawReadAuditError::InvalidPath);
    }
    let entries = fs::read_dir(&quarantine_directory).map_err(|source| RawReadAuditError::Io {
        operation: "list raw-read audit recovery records",
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| RawReadAuditError::Io {
            operation: "read raw-read audit recovery entry",
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("intent") {
            continue;
        }
        let intent_bytes = read_small_regular_file(&path)?;
        let repair = decode_repair_intent(&intent_bytes)?;
        let paths = repair_paths(&quarantine_directory, repair);
        if paths.intent != path {
            return Err(RawReadAuditError::CorruptWAL);
        }
        if let Some(done) = read_quarantine_file(&paths.done)? {
            if done != encode_repair_done(&intent_bytes, repair.prefix_digest)? {
                return Err(RawReadAuditError::CorruptWAL);
            }
            validate_quarantine_tail(&paths.quarantine, repair)?;
            validate_active_prefix(&wal_directory, repair)?;
            continue;
        }

        let wal_path = wal_directory.join(AUDIT_WAL_FILE);
        let active_bytes =
            read_wal_file(&wal_path, &wal_directory)?.ok_or(RawReadAuditError::CorruptWAL)?;
        let safe_length =
            usize::try_from(repair.safe_length).map_err(|_| RawReadAuditError::CorruptWAL)?;
        let original_length =
            usize::try_from(repair.original_length).map_err(|_| RawReadAuditError::CorruptWAL)?;
        if active_bytes.len() == original_length {
            if digest_slice(active_bytes.get(..safe_length))? != repair.prefix_digest
                || digest_slice(active_bytes.get(safe_length..))? != repair.tail_digest
            {
                return Err(RawReadAuditError::CorruptWAL);
            }
            let tail = active_bytes
                .get(safe_length..)
                .ok_or(RawReadAuditError::CorruptWAL)?;
            persist_quarantined_tail(&paths, tail)?;
            checkpoint(AuditRecoveryCheckpoint::QuarantineCopyPublished);
            truncate_wal_to(layout, repair.safe_length, repair.original_length)?;
            checkpoint(AuditRecoveryCheckpoint::TailTruncated);
        } else if active_bytes.len() == safe_length {
            if digest_slice(active_bytes.get(..safe_length))? != repair.prefix_digest {
                return Err(RawReadAuditError::CorruptWAL);
            }
            validate_quarantine_tail(&paths.quarantine, repair)?;
        } else {
            return Err(RawReadAuditError::CorruptWAL);
        }
        persist_immutable_marker(
            &paths.done,
            &paths.done_stage,
            &encode_repair_done(&intent_bytes, repair.prefix_digest)?,
            &quarantine_directory,
        )?;
        checkpoint(AuditRecoveryCheckpoint::RepairCompleted);
    }
    Ok(())
}

fn validate_active_prefix(
    wal_directory: &Path,
    repair: AuditTailRepair,
) -> Result<(), RawReadAuditError> {
    let wal_path = wal_directory.join(AUDIT_WAL_FILE);
    let active_bytes =
        read_wal_file(&wal_path, wal_directory)?.ok_or(RawReadAuditError::CorruptWAL)?;
    let safe_length =
        usize::try_from(repair.safe_length).map_err(|_| RawReadAuditError::CorruptWAL)?;
    if active_bytes.len() < safe_length
        || digest_slice(active_bytes.get(..safe_length))? != repair.prefix_digest
    {
        return Err(RawReadAuditError::CorruptWAL);
    }
    Ok(())
}

fn validate_quarantine_tail(path: &Path, repair: AuditTailRepair) -> Result<(), RawReadAuditError> {
    let tail = read_quarantine_file(path)?.ok_or(RawReadAuditError::CorruptWAL)?;
    if u64::try_from(tail.len()).map_err(|_| RawReadAuditError::CorruptWAL)?
        != repair.original_length.saturating_sub(repair.safe_length)
        || *blake3::hash(&tail).as_bytes() != repair.tail_digest
    {
        return Err(RawReadAuditError::CorruptWAL);
    }
    Ok(())
}

fn truncate_wal_to(
    layout: &DatabaseLayout,
    safe_length: u64,
    expected_current_length: u64,
) -> Result<(), RawReadAuditError> {
    let directory = canonical_regular_directory(&layout.audit_wal_directory())?;
    let path = directory.join(AUDIT_WAL_FILE);
    let metadata = fs::symlink_metadata(&path).map_err(|source| RawReadAuditError::Io {
        operation: "inspect raw-read audit WAL before journaled recovery",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RawReadAuditError::InvalidPath);
    }
    if metadata.len() != expected_current_length {
        return Err(RawReadAuditError::CorruptWAL);
    }
    let canonical_path = fs::canonicalize(&path).map_err(|source| RawReadAuditError::Io {
        operation: "resolve raw-read audit WAL before journaled recovery",
        source,
    })?;
    if canonical_path.parent() != Some(directory.as_path()) {
        return Err(RawReadAuditError::InvalidPath);
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|source| RawReadAuditError::Io {
            operation: "open raw-read audit WAL for journaled recovery",
            source,
        })?;
    let opened_metadata = file.metadata().map_err(|source| RawReadAuditError::Io {
        operation: "inspect opened raw-read audit WAL for journaled recovery",
        source,
    })?;
    if !opened_metadata.is_file() || opened_metadata.len() != expected_current_length {
        return Err(RawReadAuditError::CorruptWAL);
    }
    file.set_len(safe_length)
        .and_then(|()| file.sync_all())
        .map_err(|source| RawReadAuditError::Io {
            operation: "truncate and sync raw-read audit WAL to verified commit prefix",
            source,
        })
}

fn encode_repair_intent(repair: AuditTailRepair) -> Result<Vec<u8>, RawReadAuditError> {
    let mut encoder = TlvEncoder::new();
    encoder
        .push(1, &repair.safe_length.to_le_bytes())
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(2, &repair.original_length.to_le_bytes())
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(3, &repair.prefix_digest)
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(4, &repair.tail_digest)
        .map_err(RawReadAuditError::Wire)?;
    encode_frame(
        FrameHeader::new(RECOVERY_INTENT_FRAME_KIND),
        &encoder.finish(),
    )
    .map_err(RawReadAuditError::Frame)
}

fn decode_repair_intent(bytes: &[u8]) -> Result<AuditTailRepair, RawReadAuditError> {
    let frame = decode_frame(bytes).map_err(RawReadAuditError::Frame)?;
    if frame.header().kind() != RECOVERY_INTENT_FRAME_KIND {
        return Err(RawReadAuditError::CorruptWAL);
    }
    let mut decoder = TlvDecoder::new(frame.payload());
    let safe_length = decode_u64_field(&mut decoder, 1)?.value();
    let original_length = decode_u64_field(&mut decoder, 2)?.value();
    let prefix_digest = decode_fixed_field::<32>(&mut decoder, 3)?;
    let tail_digest = decode_fixed_field::<32>(&mut decoder, 4)?;
    ensure_no_more_fields(&mut decoder)?;
    let repair = AuditTailRepair {
        safe_length,
        original_length,
        prefix_digest,
        tail_digest,
    };
    if safe_length >= original_length || encode_repair_intent(repair)? != bytes {
        return Err(RawReadAuditError::CorruptWAL);
    }
    Ok(repair)
}

fn encode_repair_done(
    intent_bytes: &[u8],
    prefix_digest: [u8; 32],
) -> Result<Vec<u8>, RawReadAuditError> {
    let mut encoder = TlvEncoder::new();
    encoder
        .push(1, blake3::hash(intent_bytes).as_bytes())
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(2, &prefix_digest)
        .map_err(RawReadAuditError::Wire)?;
    encode_frame(
        FrameHeader::new(RECOVERY_DONE_FRAME_KIND),
        &encoder.finish(),
    )
    .map_err(RawReadAuditError::Frame)
}

fn persist_immutable_marker(
    target: &Path,
    stage: &Path,
    expected: &[u8],
    directory: &Path,
) -> Result<(), RawReadAuditError> {
    if let Some(actual) = read_quarantine_file(target)? {
        if actual == expected {
            return Ok(());
        }
        return Err(RawReadAuditError::CorruptWAL);
    }
    remove_regular_stage_if_present(stage)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(stage)
        .map_err(|source| RawReadAuditError::Io {
            operation: "create staged raw-read audit recovery marker",
            source,
        })?;
    file.write_all(expected)
        .and_then(|()| file.sync_all())
        .map_err(|source| RawReadAuditError::Io {
            operation: "write and sync staged raw-read audit recovery marker",
            source,
        })?;
    fs::rename(stage, target).map_err(|source| RawReadAuditError::Io {
        operation: "publish raw-read audit recovery marker",
        source,
    })?;
    sync_directory(directory).map_err(|source| RawReadAuditError::Io {
        operation: "sync raw-read audit recovery directory",
        source,
    })
}

fn remove_regular_stage_if_present(path: &Path) -> Result<(), RawReadAuditError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RawReadAuditError::InvalidPath);
            }
            fs::remove_file(path).map_err(|source| RawReadAuditError::Io {
                operation: "remove incomplete staged raw-read audit recovery file",
                source,
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RawReadAuditError::Io {
            operation: "inspect staged raw-read audit recovery file",
            source,
        }),
    }
}

fn read_quarantine_file(path: &Path) -> Result<Option<Vec<u8>>, RawReadAuditError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(RawReadAuditError::Io {
            operation: "inspect raw-read audit recovery file",
            source,
        }),
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RawReadAuditError::InvalidPath);
            }
            let bytes = read_small_regular_file(path)?;
            if bytes.len() > MAX_AUDIT_WAL_BYTES {
                return Err(RawReadAuditError::WALTooLarge {
                    limit: MAX_AUDIT_WAL_BYTES,
                    actual: bytes.len(),
                });
            }
            Ok(Some(bytes))
        }
    }
}

fn read_small_regular_file(path: &Path) -> Result<Vec<u8>, RawReadAuditError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| RawReadAuditError::Io {
        operation: "inspect raw-read audit recovery file",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RawReadAuditError::InvalidPath);
    }
    let limit = u64::try_from(MAX_AUDIT_WAL_BYTES).unwrap_or(u64::MAX);
    if metadata.len() > limit {
        return Err(RawReadAuditError::WALTooLarge {
            limit: MAX_AUDIT_WAL_BYTES,
            actual: usize::try_from(metadata.len()).unwrap_or(usize::MAX),
        });
    }
    let file = File::open(path).map_err(|source| RawReadAuditError::Io {
        operation: "open raw-read audit recovery file",
        source,
    })?;
    let opened = file.metadata().map_err(|source| RawReadAuditError::Io {
        operation: "inspect opened raw-read audit recovery file",
        source,
    })?;
    if !opened.is_file() || opened.len() != metadata.len() {
        return Err(RawReadAuditError::InvalidPath);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(usize::try_from(metadata.len()).unwrap_or(usize::MAX))
        .map_err(|_| RawReadAuditError::AllocationFailed)?;
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| RawReadAuditError::Io {
            operation: "read raw-read audit recovery file",
            source,
        })?;
    if u64::try_from(bytes.len()).map_err(|_| RawReadAuditError::CorruptWAL)? != metadata.len() {
        return Err(RawReadAuditError::CorruptWAL);
    }
    Ok(bytes)
}

fn digest_slice(bytes: Option<&[u8]>) -> Result<[u8; 32], RawReadAuditError> {
    bytes
        .map(|bytes| *blake3::hash(bytes).as_bytes())
        .ok_or(RawReadAuditError::CorruptWAL)
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;

        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn append_attempt(
    layout: &DatabaseLayout,
    attempt: &RawReadAttempt,
    previous: RawReadAuditHead,
) -> Result<RawReadAuditReceipt, RawReadAuditError> {
    let raw_attempt = encode_raw_read_attempt(attempt).map_err(RawReadAuditError::AuditCodec)?;
    let prepare = encode_prepare(
        attempt.sequence(),
        attempt.audit_operation_id(),
        &raw_attempt,
    )?;
    let payload_hash = *blake3::hash(&raw_attempt).as_bytes();
    let commit_hash = compute_commit_hash(
        previous.commit_hash,
        attempt.sequence(),
        attempt.audit_operation_id(),
        payload_hash,
    );
    let commit = encode_commit(
        attempt.sequence(),
        attempt.audit_operation_id(),
        payload_hash,
        previous.commit_hash,
        commit_hash,
    )?;
    let total_length =
        prepare
            .len()
            .checked_add(commit.len())
            .ok_or(RawReadAuditError::WALTooLarge {
                limit: MAX_AUDIT_WAL_BYTES,
                actual: usize::MAX,
            })?;
    if total_length > MAX_AUDIT_WAL_BYTES {
        return Err(RawReadAuditError::WALTooLarge {
            limit: MAX_AUDIT_WAL_BYTES,
            actual: total_length,
        });
    }
    let directory = canonical_regular_directory(&layout.audit_wal_directory())?;
    let path = directory.join(AUDIT_WAL_FILE);
    let existed = match fs::symlink_metadata(&path) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(RawReadAuditError::Io {
                operation: "inspect raw-read audit WAL before append",
                source,
            });
        }
    };
    let mut file = open_wal_for_append(&path, &directory)?;
    let current_length = usize::try_from(
        file.metadata()
            .map_err(|source| RawReadAuditError::Io {
                operation: "inspect raw-read audit WAL",
                source,
            })?
            .len(),
    )
    .unwrap_or(usize::MAX);
    if current_length
        .checked_add(total_length)
        .is_none_or(|length| length > MAX_AUDIT_WAL_BYTES)
    {
        return Err(RawReadAuditError::WALTooLarge {
            limit: MAX_AUDIT_WAL_BYTES,
            actual: current_length.saturating_add(total_length),
        });
    }
    if !existed {
        sync_directory(&directory).map_err(|source| RawReadAuditError::Io {
            operation: "sync raw-read audit WAL directory after file creation",
            source,
        })?;
    }
    file.write_all(&prepare)
        .map_err(|source| RawReadAuditError::Io {
            operation: "append raw-read audit prepare",
            source,
        })?;
    file.sync_all().map_err(|source| RawReadAuditError::Io {
        operation: "sync raw-read audit prepare",
        source,
    })?;
    file.write_all(&commit)
        .map_err(|source| RawReadAuditError::Io {
            operation: "append raw-read audit commit marker",
            source,
        })?;
    file.sync_all().map_err(|source| RawReadAuditError::Io {
        operation: "sync raw-read audit commit marker",
        source,
    })?;
    Ok(RawReadAuditReceipt {
        sequence: attempt.sequence(),
        audit_operation_id: attempt.audit_operation_id(),
        commit_hash,
    })
}

fn encode_prepare(
    sequence: AuditSequence,
    audit_operation_id: AuditOperationId,
    raw_attempt: &[u8],
) -> Result<Vec<u8>, RawReadAuditError> {
    let mut encoder = TlvEncoder::new();
    encoder
        .push(1, &sequence.value().to_le_bytes())
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(2, audit_operation_id.as_bytes())
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(3, raw_attempt)
        .map_err(RawReadAuditError::Wire)?;
    encode_frame(FrameHeader::new(PREPARE_FRAME_KIND), &encoder.finish())
        .map_err(RawReadAuditError::Frame)
}

fn encode_commit(
    sequence: AuditSequence,
    audit_operation_id: AuditOperationId,
    payload_hash: [u8; 32],
    previous_hash: [u8; 32],
    commit_hash: [u8; 32],
) -> Result<Vec<u8>, RawReadAuditError> {
    let mut encoder = TlvEncoder::new();
    encoder
        .push(1, &sequence.value().to_le_bytes())
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(2, audit_operation_id.as_bytes())
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(3, &payload_hash)
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(4, &previous_hash)
        .map_err(RawReadAuditError::Wire)?;
    encoder
        .push(5, &commit_hash)
        .map_err(RawReadAuditError::Wire)?;
    encode_frame(FrameHeader::new(COMMIT_FRAME_KIND), &encoder.finish())
        .map_err(RawReadAuditError::Frame)
}

fn decode_prepare(
    payload: &[u8],
) -> Result<(AuditSequence, AuditOperationId, &[u8]), RawReadAuditError> {
    let mut decoder = TlvDecoder::new(payload);
    let sequence = decode_u64_field(&mut decoder, 1)?;
    let operation_id = decode_id_field::<AuditOperationId>(&mut decoder, 2)?;
    let attempt = decode_bytes_field(&mut decoder, 3)?;
    ensure_no_more_fields(&mut decoder)?;
    Ok((sequence, operation_id, attempt))
}

fn decode_commit(payload: &[u8]) -> Result<DecodedCommit, RawReadAuditError> {
    let mut decoder = TlvDecoder::new(payload);
    let sequence = decode_u64_field(&mut decoder, 1)?;
    let operation_id = decode_id_field::<AuditOperationId>(&mut decoder, 2)?;
    let payload_hash = decode_fixed_field::<32>(&mut decoder, 3)?;
    let previous_hash = decode_fixed_field::<32>(&mut decoder, 4)?;
    let commit_hash = decode_fixed_field::<32>(&mut decoder, 5)?;
    ensure_no_more_fields(&mut decoder)?;
    Ok(DecodedCommit {
        sequence,
        audit_operation_id: operation_id,
        payload_hash,
        previous_hash,
        commit_hash,
    })
}

fn decode_u64_field(
    decoder: &mut TlvDecoder<'_>,
    expected_tag: u32,
) -> Result<AuditSequence, RawReadAuditError> {
    let value = next_field(decoder, expected_tag)?;
    let bytes: [u8; 8] = value
        .try_into()
        .map_err(|_| RawReadAuditError::CorruptWAL)?;
    Ok(AuditSequence::new(u64::from_le_bytes(bytes)))
}

fn decode_id_field<T: DomainId>(
    decoder: &mut TlvDecoder<'_>,
    expected_tag: u32,
) -> Result<T, RawReadAuditError> {
    let bytes: [u8; 16] = next_field(decoder, expected_tag)?
        .try_into()
        .map_err(|_| RawReadAuditError::CorruptWAL)?;
    T::try_from_bytes(bytes).map_err(|_| RawReadAuditError::CorruptWAL)
}

fn decode_fixed_field<const N: usize>(
    decoder: &mut TlvDecoder<'_>,
    expected_tag: u32,
) -> Result<[u8; N], RawReadAuditError> {
    next_field(decoder, expected_tag)?
        .try_into()
        .map_err(|_| RawReadAuditError::CorruptWAL)
}

fn decode_bytes_field<'a>(
    decoder: &mut TlvDecoder<'a>,
    expected_tag: u32,
) -> Result<&'a [u8], RawReadAuditError> {
    next_field(decoder, expected_tag)
}

fn next_field<'a>(
    decoder: &mut TlvDecoder<'a>,
    expected_tag: u32,
) -> Result<&'a [u8], RawReadAuditError> {
    let field = decoder
        .next_field()
        .map_err(RawReadAuditError::Wire)?
        .ok_or(RawReadAuditError::CorruptWAL)?;
    if field.tag() != expected_tag {
        return Err(RawReadAuditError::CorruptWAL);
    }
    Ok(field.value())
}

fn ensure_no_more_fields(decoder: &mut TlvDecoder<'_>) -> Result<(), RawReadAuditError> {
    if decoder
        .next_field()
        .map_err(RawReadAuditError::Wire)?
        .is_some()
    {
        Err(RawReadAuditError::CorruptWAL)
    } else {
        Ok(())
    }
}

fn compute_commit_hash(
    previous_hash: [u8; 32],
    sequence: AuditSequence,
    operation_id: AuditOperationId,
    payload_hash: [u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(COMMIT_HASH_CONTEXT);
    hasher.update(&previous_hash);
    hasher.update(&sequence.value().to_le_bytes());
    hasher.update(operation_id.as_bytes());
    hasher.update(&payload_hash);
    *hasher.finalize().as_bytes()
}

fn next_frame(bytes: &[u8], offset: usize) -> Result<Option<(&[u8], usize)>, RawReadAuditError> {
    if offset == bytes.len() {
        return Ok(None);
    }
    let length_start = offset
        .checked_add(FRAME_LENGTH_START)
        .ok_or(RawReadAuditError::CorruptWAL)?;
    let length_end = offset
        .checked_add(FRAME_LENGTH_START + 8)
        .ok_or(RawReadAuditError::CorruptWAL)?;
    let Some(length_bytes) = bytes.get(length_start..length_end) else {
        return Err(RawReadAuditError::RecoveryRequired);
    };
    let payload_length = usize::try_from(u64::from_le_bytes(
        length_bytes
            .try_into()
            .map_err(|_| RawReadAuditError::CorruptWAL)?,
    ))
    .map_err(|_| RawReadAuditError::CorruptWAL)?;
    let actual_frame_length = FRAME_HEADER_LEN
        .checked_add(payload_length)
        .and_then(|length| length.checked_add(CHECKSUM_LEN))
        .ok_or(RawReadAuditError::CorruptWAL)?;
    if actual_frame_length > MAX_AUDIT_FRAME_BYTES {
        return Err(RawReadAuditError::CorruptWAL);
    }
    let frame_end = offset
        .checked_add(actual_frame_length)
        .ok_or(RawReadAuditError::CorruptWAL)?;
    let Some(frame_bytes) = bytes.get(offset..frame_end) else {
        return Err(RawReadAuditError::RecoveryRequired);
    };
    Ok(Some((frame_bytes, frame_end)))
}

fn read_wal_file(path: &Path, directory: &Path) -> Result<Option<Vec<u8>>, RawReadAuditError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(RawReadAuditError::Io {
                operation: "inspect raw-read audit WAL",
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RawReadAuditError::InvalidPath);
    }
    let canonical_path = fs::canonicalize(path).map_err(|source| RawReadAuditError::Io {
        operation: "resolve raw-read audit WAL",
        source,
    })?;
    if canonical_path.parent() != Some(directory) {
        return Err(RawReadAuditError::InvalidPath);
    }
    let file_size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if file_size > MAX_AUDIT_WAL_BYTES {
        return Err(RawReadAuditError::WALTooLarge {
            limit: MAX_AUDIT_WAL_BYTES,
            actual: file_size,
        });
    }
    let file = File::open(path).map_err(|source| RawReadAuditError::Io {
        operation: "open raw-read audit WAL",
        source,
    })?;
    let opened_metadata = file.metadata().map_err(|source| RawReadAuditError::Io {
        operation: "inspect opened raw-read audit WAL",
        source,
    })?;
    if !opened_metadata.is_file() || opened_metadata.len() != metadata.len() {
        return Err(RawReadAuditError::InvalidPath);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(file_size)
        .map_err(|_| RawReadAuditError::AllocationFailed)?;
    file.take(u64::try_from(MAX_AUDIT_WAL_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| RawReadAuditError::Io {
            operation: "read raw-read audit WAL",
            source,
        })?;
    if bytes.len() != file_size || bytes.len() > MAX_AUDIT_WAL_BYTES {
        return Err(RawReadAuditError::RecoveryRequired);
    }
    Ok(Some(bytes))
}

fn open_wal_for_append(path: &Path, directory: &Path) -> Result<File, RawReadAuditError> {
    let mut options = OpenOptions::new();
    options.append(true).read(true).write(true);
    let file = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RawReadAuditError::InvalidPath);
            }
            options.open(path).map_err(|source| RawReadAuditError::Io {
                operation: "open raw-read audit WAL for append",
                source,
            })?
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => options
            .create_new(true)
            .open(path)
            .map_err(|source| RawReadAuditError::Io {
                operation: "create raw-read audit WAL",
                source,
            })?,
        Err(source) => {
            return Err(RawReadAuditError::Io {
                operation: "inspect raw-read audit WAL before append",
                source,
            });
        }
    };
    let canonical_path = fs::canonicalize(path).map_err(|source| RawReadAuditError::Io {
        operation: "resolve opened raw-read audit WAL",
        source,
    })?;
    if canonical_path.parent() != Some(directory) {
        return Err(RawReadAuditError::InvalidPath);
    }
    let metadata = file.metadata().map_err(|source| RawReadAuditError::Io {
        operation: "inspect opened raw-read audit WAL for append",
        source,
    })?;
    if !metadata.is_file() {
        return Err(RawReadAuditError::InvalidPath);
    }
    Ok(file)
}

fn canonical_regular_directory(path: &Path) -> Result<PathBuf, RawReadAuditError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| RawReadAuditError::Io {
        operation: "inspect raw-read audit directory",
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RawReadAuditError::InvalidPath);
    }
    let canonical = fs::canonicalize(path).map_err(|source| RawReadAuditError::Io {
        operation: "resolve raw-read audit directory",
        source,
    })?;
    if canonical != path {
        return Err(RawReadAuditError::InvalidPath);
    }
    Ok(canonical)
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    options.open(path)
}

#[cfg(all(test, windows))]
mod tests {
    use std::env;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::{
        AuditScopeFingerprint, AuditSequence, Bytes, ClientRequestId, DomainId, PageOrdinal,
        PrincipalId, RawReadAttempt, RawReadAttemptIdentity, RawReadAttemptScope, SecurityEpoch,
        SnapshotId,
    };

    use super::{
        AuditRecoveryCheckpoint, AuditWriterLock, RawReadAuditWal, encode_prepare,
        recover_tail_locked_with_checkpoint, scan_wal, scan_wal_allow_incomplete_tail,
    };
    use crate::DatabaseLayout;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDatabase(PathBuf);

    impl TempDatabase {
        fn create() -> Result<Self, String> {
            let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!(
                "worlddb-m5-22-audit-{}-{sequence}",
                std::process::id()
            ));
            DatabaseLayout::create(&root).map_err(|error| error.to_string())?;
            Ok(Self(root))
        }

        fn layout(&self) -> Result<DatabaseLayout, String> {
            DatabaseLayout::open(&self.0).map_err(|error| error.to_string())
        }
    }

    impl Drop for TempDatabase {
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

    fn uncommitted_prepare() -> Result<Vec<u8>, String> {
        let audit_operation_id = id::<worlddb_core::AuditOperationId>(52)?;
        let attempt = RawReadAttempt::new(
            RawReadAttemptIdentity {
                record_id: id::<worlddb_core::AuditRecordId>(51)?,
                sequence: AuditSequence::new(2),
                audit_operation_id,
                client_request_id: id::<ClientRequestId>(53)?,
            },
            RawReadAttemptScope {
                principal_id: id::<PrincipalId>(4)?,
                scope_fingerprint: AuditScopeFingerprint::new(Bytes::new(vec![0x19, 0x55]))
                    .map_err(|error| error.to_string())?,
                snapshot_id: id::<SnapshotId>(2)?,
                security_epoch: SecurityEpoch::INITIAL,
                page_ordinal: PageOrdinal::new(9),
            },
        );
        let payload =
            worlddb_core::encode_raw_read_attempt(&attempt).map_err(|error| error.to_string())?;
        encode_prepare(AuditSequence::new(2), audit_operation_id, &payload)
            .map_err(|error| error.to_string())
    }

    #[cfg(windows)]
    #[test]
    fn process_crash_can_interrupt_each_audit_tail_recovery_checkpoint() -> Result<(), String> {
        const ROOT_ENV: &str = "WORLDDB_M5_22_AUDIT_CRASH_ROOT";
        const POINT_ENV: &str = "WORLDDB_M5_22_AUDIT_CRASH_POINT";
        const TEST_NAME: &str =
            "audit_wal::tests::process_crash_can_interrupt_each_audit_tail_recovery_checkpoint";

        if let (Ok(root), Ok(point_name)) = (env::var(ROOT_ENV), env::var(POINT_ENV)) {
            let point = match point_name.as_str() {
                "quarantine_copy_published" => AuditRecoveryCheckpoint::QuarantineCopyPublished,
                "intent_published" => AuditRecoveryCheckpoint::IntentPublished,
                "tail_truncated" => AuditRecoveryCheckpoint::TailTruncated,
                "repair_completed" => AuditRecoveryCheckpoint::RepairCompleted,
                _ => return Err(format!("unknown audit recovery checkpoint {point_name}")),
            };
            let layout = DatabaseLayout::open(root).map_err(|error| error.to_string())?;
            let _lock = AuditWriterLock::try_acquire(&layout).map_err(|error| error.to_string())?;
            let _ = recover_tail_locked_with_checkpoint(&layout, |checkpoint| {
                if checkpoint == point {
                    std::process::exit(86);
                }
            });
            return Err(format!("child did not reach audit checkpoint {point_name}"));
        }

        let database = TempDatabase::create()?;
        let layout = database.layout()?;
        let audit = RawReadAuditWal::new(layout.clone());
        let writer = audit.try_writer().map_err(|error| error.to_string())?;
        writer
            .append_attempt(
                RawReadAttemptScope {
                    principal_id: id::<PrincipalId>(4)?,
                    scope_fingerprint: AuditScopeFingerprint::new(Bytes::new(vec![0x19, 0x55]))
                        .map_err(|error| error.to_string())?,
                    snapshot_id: id::<SnapshotId>(2)?,
                    security_epoch: SecurityEpoch::INITIAL,
                    page_ordinal: PageOrdinal::new(0),
                },
                id::<ClientRequestId>(50)?,
            )
            .map_err(|error| error.to_string())?;
        drop(writer);
        let tail = uncommitted_prepare()?;
        let wal_path = layout.audit_wal_directory().join("raw-read.wal");
        let mut file = OpenOptions::new()
            .append(true)
            .open(&wal_path)
            .map_err(|error| error.to_string())?;
        file.write_all(&tail).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);

        for (checkpoint, point_name) in [
            (
                AuditRecoveryCheckpoint::QuarantineCopyPublished,
                "quarantine_copy_published",
            ),
            (AuditRecoveryCheckpoint::IntentPublished, "intent_published"),
            (AuditRecoveryCheckpoint::TailTruncated, "tail_truncated"),
            (AuditRecoveryCheckpoint::RepairCompleted, "repair_completed"),
        ] {
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
                    "child for {checkpoint:?} exited with {:?}, expected crash code 86",
                    status.code()
                ));
            }

            let reopened_layout = database.layout()?;
            let lock = AuditWriterLock::try_acquire(&reopened_layout)
                .map_err(|error| error.to_string())?;
            let scan = scan_wal_allow_incomplete_tail(&reopened_layout)
                .map_err(|error| error.to_string())?;
            assert_eq!(scan.head.sequence().value(), 1);
            drop(lock);
        }

        let reopened_layout = database.layout()?;
        let recovery = RawReadAuditWal::new(reopened_layout.clone())
            .recover()
            .map_err(|error| error.to_string())?;
        assert_eq!(recovery.head().sequence().value(), 1);
        assert_eq!(
            scan_wal(&reopened_layout)
                .map_err(|error| error.to_string())?
                .attempts
                .len(),
            1
        );
        Ok(())
    }
}
