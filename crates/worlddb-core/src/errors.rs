//! Concrete error domains and stable public codes for WorldDB boundaries.
//!
//! Error variants model failures only. In particular, an optimistic commit
//! conflict is a normal [`CommitOutcome`] and is never a [`CommitError`].

use std::fmt;

use crate::ids::{JobId, OperationId, Revision};
use crate::wire::WireError;

/// Failure while opening a database.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum OpenError {
    /// The requested database does not exist.
    NotFound,
    /// Another process owns the required lock.
    Locked,
    /// The file format is not supported by this engine version.
    UnsupportedFormat,
    /// An explicit schema migration is required.
    NeedsMigration,
    /// Recovery must run before the database can be opened normally.
    RecoveryRequired,
    /// The process lacks permission to open the database.
    PermissionDenied,
    /// The database is structurally corrupt.
    Corrupt,
    /// An input/output operation failed.
    Io,
}

/// Structural or domain validation failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum ValidationError {
    /// The submitted schema violates its declared contract.
    SchemaViolation,
    /// A value has an invalid kind or representation.
    InvalidValue,
    /// A referenced entity or record is invalid or unavailable.
    InvalidReference,
    /// A temporal range is invalid.
    InvalidTemporalRange,
    /// The requested operation is not supported in this context.
    UnsupportedOperation,
    /// A configured or hard resource budget was exceeded.
    BudgetExceeded,
}

/// Query construction or execution failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum QueryError {
    /// The query is structurally or semantically invalid.
    InvalidQuery,
    /// The query was cancelled before completion.
    Cancelled,
    /// A configured or hard resource budget was exceeded.
    BudgetExceeded,
    /// The pinned snapshot is no longer available.
    SnapshotExpired,
    /// The caller is not authorized to perform the query.
    Unauthorized,
    /// The storage layer failed while reading query data.
    StorageRead,
    /// Query data failed integrity or structural validation.
    CorruptData,
}

/// Conflict-free result of one commit attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    /// The commit reached its durable publication point.
    Committed(CommitReceipt),
    /// Optimistic concurrency found retry-relevant dependencies changed.
    Conflict(ConflictReport),
}

/// Receipt for a durably committed operation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommitReceipt {
    operation_id: OperationId,
    revision: Revision,
    warnings: Vec<crate::schema_write_validation::DeprecatedSchemaWriteWarning>,
}

impl CommitReceipt {
    /// Creates a receipt for a published logical operation.
    #[must_use]
    pub const fn new(operation_id: OperationId, revision: Revision) -> Self {
        Self {
            operation_id,
            revision,
            warnings: Vec::new(),
        }
    }

    /// Creates a receipt carrying the typed warnings accepted by validation.
    #[must_use]
    pub fn with_warnings(
        operation_id: OperationId,
        revision: Revision,
        warnings: Vec<crate::schema_write_validation::DeprecatedSchemaWriteWarning>,
    ) -> Self {
        Self {
            operation_id,
            revision,
            warnings,
        }
    }

    /// Returns the logical operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Returns the revision assigned to the commit.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Typed validation warnings associated with this successful commit.
    #[must_use]
    pub fn warnings(&self) -> &[crate::schema_write_validation::DeprecatedSchemaWriteWarning] {
        &self.warnings
    }
}

/// Security-filterable classes of optimistic concurrency dependency conflicts.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ConflictFact {
    /// A record read by the transaction changed after its base snapshot.
    ReadDependencyChanged,
    /// A record targeted by a write changed after its base snapshot.
    WriteTargetChanged,
    /// The database revision advanced beyond the transaction's accepted base.
    BaseRevisionAdvanced,
}

/// Conflict facts that may be exposed after security filtering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictReport {
    facts: Vec<ConflictFact>,
}

impl ConflictReport {
    /// Creates a conflict report from retry-relevant facts.
    #[must_use]
    pub const fn new(facts: Vec<ConflictFact>) -> Self {
        Self { facts }
    }

    /// Returns the conflict facts selected for this caller.
    #[must_use]
    pub fn facts(&self) -> &[ConflictFact] {
        &self.facts
    }

    /// Returns only conflict facts the caller is authorized to disclose.
    #[must_use]
    pub fn filtered(&self, mut may_disclose: impl FnMut(ConflictFact) -> bool) -> Self {
        Self::new(
            self.facts
                .iter()
                .copied()
                .filter(|fact| may_disclose(*fact))
                .collect(),
        )
    }
}

/// Failure while committing a transaction.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum CommitError {
    /// Transaction validation failed before publication.
    Validation(ValidationError),
    /// Commit authorization failed.
    Authorization(SecurityError),
    /// Storage failed while processing the commit.
    Storage(StorageError),
    /// The request may have committed; resolve it using this operation ID.
    UnknownCommitOutcome { operation_id: OperationId },
    /// The database is open read-only.
    DatabaseReadOnly,
    /// The database is shutting down and does not accept new commits.
    ShuttingDown,
}

/// Storage operation whose failure was observed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageOperation {
    /// Append a WAL frame.
    WalAppend,
    /// Synchronize the WAL.
    WalSync,
    /// Write a data segment.
    SegmentWrite,
    /// Synchronize a data segment.
    SegmentSync,
    /// Write a manifest.
    ManifestWrite,
    /// Publish a manifest.
    ManifestPublish,
    /// Synchronize the containing directory.
    DirectorySync,
    /// Acquire or inspect a database lock.
    Lock,
    /// Read persisted data.
    Read,
    /// Verify persisted data.
    Verify,
}

/// Observable class of a storage failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageFailureClass {
    /// The requested object was absent.
    NotFound,
    /// Access was denied by the operating system or backend.
    PermissionDenied,
    /// Another process holds an incompatible lock.
    Locked,
    /// The operation failed for another I/O reason.
    Io,
    /// Persisted bytes or metadata failed integrity validation.
    Corrupt,
    /// The backend could not satisfy a durability guarantee.
    Durability,
}

/// Typed storage failure; detailed causes are attached at the mapping boundary.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum StorageError {
    /// Requested persistent data was absent.
    NotFound { operation: StorageOperation },
    /// The process was not allowed to perform the operation.
    PermissionDenied { operation: StorageOperation },
    /// An incompatible process lock prevented the operation.
    Locked { operation: StorageOperation },
    /// The operation failed for another I/O reason.
    Io { operation: StorageOperation },
    /// Persisted bytes or metadata were corrupt.
    Corrupt { operation: StorageOperation },
    /// The operation could not establish its required durability guarantee.
    Durability { operation: StorageOperation },
}

impl StorageError {
    /// Creates a storage error without erasing its operation or failure class.
    #[must_use]
    pub const fn new(operation: StorageOperation, class: StorageFailureClass) -> Self {
        match class {
            StorageFailureClass::NotFound => Self::NotFound { operation },
            StorageFailureClass::PermissionDenied => Self::PermissionDenied { operation },
            StorageFailureClass::Locked => Self::Locked { operation },
            StorageFailureClass::Io => Self::Io { operation },
            StorageFailureClass::Corrupt => Self::Corrupt { operation },
            StorageFailureClass::Durability => Self::Durability { operation },
        }
    }

    /// Returns the operation that failed.
    #[must_use]
    pub const fn operation(self) -> StorageOperation {
        match self {
            Self::NotFound { operation }
            | Self::PermissionDenied { operation }
            | Self::Locked { operation }
            | Self::Io { operation }
            | Self::Corrupt { operation }
            | Self::Durability { operation } => operation,
        }
    }

    /// Returns the observed failure class.
    #[must_use]
    pub const fn class(self) -> StorageFailureClass {
        match self {
            Self::NotFound { .. } => StorageFailureClass::NotFound,
            Self::PermissionDenied { .. } => StorageFailureClass::PermissionDenied,
            Self::Locked { .. } => StorageFailureClass::Locked,
            Self::Io { .. } => StorageFailureClass::Io,
            Self::Corrupt { .. } => StorageFailureClass::Corrupt,
            Self::Durability { .. } => StorageFailureClass::Durability,
        }
    }

    /// Returns the stable public identifier for a storage failure.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::NotFound { .. } => "WDB-STORAGE-NOT-FOUND",
            Self::PermissionDenied { .. } => "WDB-STORAGE-PERMISSION-DENIED",
            Self::Locked { .. } => "WDB-STORAGE-LOCKED",
            Self::Io { .. } => "WDB-STORAGE-IO",
            Self::Corrupt { .. } => "WDB-STORAGE-CORRUPT",
            Self::Durability { .. } => "WDB-STORAGE-DURABILITY",
        }
    }
}

/// Recovery failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum RecoveryError {
    /// Recovery found an incomplete or gapped history.
    HistoryGap,
    /// Recovery found committed data that failed integrity checks.
    CorruptCommittedData,
    /// Recovery could not safely determine a commit outcome.
    IndeterminateCommit,
    /// The recovery operation could not complete because of storage failure.
    StorageFailure,
    /// Recovery requires an explicit administrative decision.
    DecisionRequired,
}

/// Schema or storage-format migration failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MigrationError {
    /// The migration plan is invalid or internally inconsistent.
    InvalidPlan,
    /// The database format or migration step is unsupported.
    UnsupportedFormat,
    /// A precondition or schema constraint failed.
    ConstraintViolation,
    /// An unresolved item needs an explicit decision.
    DecisionRequired,
    /// The migration could not proceed from its recorded checkpoint.
    ResumeMismatch,
    /// The migration operation failed in storage.
    StorageFailure,
}

/// Backup or restore failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum BackupError {
    /// The backup inventory or manifest is invalid.
    InvalidManifest,
    /// A backup item did not match its recorded digest.
    IntegrityMismatch,
    /// The backup is incompatible with this engine version.
    IncompatibleFormat,
    /// Required authentication material is invalid or unavailable.
    AuthenticationFailure,
    /// The pinned source snapshot changed or could not be completed.
    SnapshotUnavailable,
    /// Backup or restore storage failed.
    StorageFailure,
}

/// Logical export failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum ExportError {
    /// The requested export scope is invalid.
    InvalidScope,
    /// The requested export format is unsupported.
    UnsupportedFormat,
    /// The caller cannot export the requested data.
    Unauthorized,
    /// A selected record or schema dependency is unavailable.
    MissingDependency,
    /// The export could not complete because of storage or stream failure.
    Io,
}

/// Security boundary failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum SecurityError {
    /// The requested action is not authorized.
    AccessDenied,
    /// A required capability is absent.
    CapabilityMissing,
    /// The caller's security epoch became stale.
    EpochChanged,
    /// The caller identity or policy context is invalid.
    InvalidPrincipal,
}

/// Background job failure.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum JobError {
    /// The requested job does not exist or is not visible.
    NotFound,
    /// The job was cancelled before completion.
    Cancelled,
    /// The job exceeded a configured resource budget.
    BudgetExceeded,
    /// The job cannot resume from the supplied checkpoint.
    ResumeMismatch,
    /// A job dependency failed.
    DependencyFailed,
}

/// Retry instructions safe to expose at a public boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RetryHint {
    /// Repeating the request is not currently recommended.
    DoNotRetry,
    /// The logical operation can be repeated without changing its identity.
    RetrySameOperation,
    /// Query status by OperationId before deciding whether to retry.
    ResolveOperationStatus,
}

/// Observable severity fact, without an application response decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Severity {
    /// The request ended without a fault requiring intervention.
    Informational,
    /// The request was denied or needs caller attention.
    Warning,
    /// The operation failed.
    Error,
    /// Integrity or commit state may be uncertain.
    Critical,
}

/// Retryability fact about the failed operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Retryability {
    /// Repeating without changing state is not expected to help.
    Never,
    /// A transient condition may clear; the same logical operation is safe.
    SameOperationMayBeRetried,
    /// Resolve the outcome by OperationId before any retry.
    ResolveOperationStatus,
}

/// Integrity effect known at the layer that detected the error.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntegrityImpact {
    /// No integrity impact was observed.
    None,
    /// Integrity could not be determined.
    Unknown,
    /// Persisted integrity checks failed.
    CorruptionDetected,
}

/// Purely observed error facts; it contains no suggested action or recovery policy.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ErrorFacts {
    severity: Severity,
    retryability: Retryability,
    integrity_impact: IntegrityImpact,
}

impl ErrorFacts {
    /// Creates a value containing only the three observed error facts.
    #[must_use]
    pub const fn new(
        severity: Severity,
        retryability: Retryability,
        integrity_impact: IntegrityImpact,
    ) -> Self {
        Self {
            severity,
            retryability,
            integrity_impact,
        }
    }

    /// Returns observed severity.
    #[must_use]
    pub const fn severity(self) -> Severity {
        self.severity
    }

    /// Returns observed retryability.
    #[must_use]
    pub const fn retryability(self) -> Retryability {
        self.retryability
    }

    /// Returns the observed integrity effect.
    #[must_use]
    pub const fn integrity_impact(self) -> IntegrityImpact {
        self.integrity_impact
    }
}

/// Public error fields suitable for a log entry or a versioned IPC envelope.
/// This DTO intentionally has no source, cause, path, query, or value field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PublicErrorDto {
    code: &'static str,
    message_key: &'static str,
    retry_hint: RetryHint,
    operation_id: Option<OperationId>,
    job_id: Option<JobId>,
}

impl PublicErrorDto {
    fn new(
        code: &'static str,
        retry_hint: RetryHint,
        operation_id: Option<OperationId>,
        job_id: Option<JobId>,
    ) -> Self {
        Self {
            code,
            message_key: code,
            retry_hint,
            operation_id,
            job_id,
        }
    }

    /// Returns the stable public code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }

    /// Returns the localization key associated with the public code.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        self.message_key
    }

    /// Returns the conservative public retry instruction.
    #[must_use]
    pub const fn retry_hint(self) -> RetryHint {
        self.retry_hint
    }

    /// Returns the safe logical operation identity when status resolution is required.
    #[must_use]
    pub const fn operation_id(self) -> Option<OperationId> {
        self.operation_id
    }

    /// Returns a safe background job identity when the error belongs to a job.
    #[must_use]
    pub const fn job_id(self) -> Option<JobId> {
        self.job_id
    }
}

impl fmt::Display for PublicErrorDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

/// Error types that can be projected to the stable public error envelope.
pub trait PublicErrorCode {
    /// Returns the stable public identifier.
    fn public_code(&self) -> &'static str;

    /// Returns explicitly reviewed, pure facts for this error variant.
    fn facts(&self) -> ErrorFacts;

    /// Returns a safe operation identity for resolving unknown commit outcomes.
    fn operation_id(&self) -> Option<OperationId> {
        None
    }
}

/// Creates a cause-free public DTO from a concrete domain error.
#[must_use]
pub fn to_public_error<E: PublicErrorCode>(error: &E) -> PublicErrorDto {
    let facts = error.facts();
    let retry_hint = match facts.retryability() {
        Retryability::Never => RetryHint::DoNotRetry,
        Retryability::SameOperationMayBeRetried => RetryHint::RetrySameOperation,
        Retryability::ResolveOperationStatus => RetryHint::ResolveOperationStatus,
    };
    PublicErrorDto::new(error.public_code(), retry_hint, error.operation_id(), None)
}

/// Projects a job error and its safe JobId into the public error envelope.
#[must_use]
pub fn to_public_job_error(error: &JobError, job_id: JobId) -> PublicErrorDto {
    let facts = error.facts();
    let retry_hint = match facts.retryability() {
        Retryability::Never => RetryHint::DoNotRetry,
        Retryability::SameOperationMayBeRetried => RetryHint::RetrySameOperation,
        Retryability::ResolveOperationStatus => RetryHint::ResolveOperationStatus,
    };
    PublicErrorDto::new(error.public_code(), retry_hint, None, Some(job_id))
}

impl PublicErrorCode for OpenError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::NotFound | Self::NeedsMigration => ErrorFacts::new(
                Severity::Informational,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::Locked | Self::PermissionDenied => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::UnsupportedFormat => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::RecoveryRequired => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
            Self::Corrupt => ErrorFacts::new(
                Severity::Critical,
                Retryability::Never,
                IntegrityImpact::CorruptionDetected,
            ),
            Self::Io => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
}
impl PublicErrorCode for ValidationError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::SchemaViolation
            | Self::InvalidValue
            | Self::InvalidReference
            | Self::InvalidTemporalRange
            | Self::UnsupportedOperation => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::BudgetExceeded => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
        }
    }
}
impl PublicErrorCode for QueryError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::InvalidQuery => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::Cancelled => ErrorFacts::new(
                Severity::Informational,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::BudgetExceeded | Self::SnapshotExpired | Self::Unauthorized => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::StorageRead => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
            Self::CorruptData => ErrorFacts::new(
                Severity::Critical,
                Retryability::Never,
                IntegrityImpact::CorruptionDetected,
            ),
        }
    }
}
impl PublicErrorCode for CommitError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::UnknownCommitOutcome { .. } => ErrorFacts::new(
                Severity::Critical,
                Retryability::ResolveOperationStatus,
                IntegrityImpact::Unknown,
            ),
            Self::Validation(_) => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::Authorization(_) | Self::DatabaseReadOnly | Self::ShuttingDown => {
                ErrorFacts::new(
                    Severity::Warning,
                    Retryability::Never,
                    IntegrityImpact::None,
                )
            }
            Self::Storage(_) => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
    fn operation_id(&self) -> Option<OperationId> {
        match self {
            Self::UnknownCommitOutcome { operation_id } => Some(*operation_id),
            Self::Validation(_)
            | Self::Authorization(_)
            | Self::Storage(_)
            | Self::DatabaseReadOnly
            | Self::ShuttingDown => None,
        }
    }
}
impl PublicErrorCode for StorageError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::NotFound { .. } | Self::PermissionDenied { .. } | Self::Locked { .. } => {
                ErrorFacts::new(
                    Severity::Warning,
                    Retryability::Never,
                    IntegrityImpact::None,
                )
            }
            Self::Io { .. } => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
            Self::Corrupt { .. } => ErrorFacts::new(
                Severity::Critical,
                Retryability::Never,
                IntegrityImpact::CorruptionDetected,
            ),
            Self::Durability { .. } => ErrorFacts::new(
                Severity::Critical,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
}
impl PublicErrorCode for RecoveryError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::HistoryGap | Self::CorruptCommittedData => ErrorFacts::new(
                Severity::Critical,
                Retryability::Never,
                IntegrityImpact::CorruptionDetected,
            ),
            Self::IndeterminateCommit => ErrorFacts::new(
                Severity::Critical,
                Retryability::ResolveOperationStatus,
                IntegrityImpact::Unknown,
            ),
            Self::StorageFailure => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
            Self::DecisionRequired => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
}
impl PublicErrorCode for MigrationError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::InvalidPlan | Self::UnsupportedFormat | Self::ConstraintViolation => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::DecisionRequired => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::ResumeMismatch => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
            Self::StorageFailure => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
}
impl PublicErrorCode for BackupError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::InvalidManifest => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
            Self::IntegrityMismatch => ErrorFacts::new(
                Severity::Critical,
                Retryability::Never,
                IntegrityImpact::CorruptionDetected,
            ),
            Self::IncompatibleFormat => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::AuthenticationFailure => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::SnapshotUnavailable | Self::StorageFailure => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
}
impl PublicErrorCode for ExportError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::InvalidScope | Self::UnsupportedFormat => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::Unauthorized => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::MissingDependency | Self::Io => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
}
impl PublicErrorCode for SecurityError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::AccessDenied | Self::CapabilityMissing | Self::EpochChanged => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::InvalidPrincipal => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
        }
    }
}
impl PublicErrorCode for WireError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::Varint { .. }
            | Self::Integer(_)
            | Self::Decimal(_)
            | Self::Symbol(_)
            | Self::Identity(_)
            | Self::Truncated { .. }
            | Self::LengthOverflow { .. }
            | Self::LengthMismatch { .. }
            | Self::TrailingBytes { .. }
            | Self::ValueTagOverflow { .. }
            | Self::UnknownValueTag { .. }
            | Self::FieldTagOverflow { .. }
            | Self::FieldNotIncreasing { .. }
            | Self::InvalidBoolean { .. }
            | Self::InvalidUtf8 { .. }
            | Self::UnknownExternalCode { .. } => {
                ErrorFacts::new(Severity::Error, Retryability::Never, IntegrityImpact::None)
            }
            Self::ResourceLimitExceeded { .. } | Self::AllocationFailed { .. } => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
        }
    }
}
impl PublicErrorCode for JobError {
    fn public_code(&self) -> &'static str {
        (*self).public_code()
    }
    fn facts(&self) -> ErrorFacts {
        match self {
            Self::NotFound => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::Cancelled => ErrorFacts::new(
                Severity::Informational,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::BudgetExceeded => ErrorFacts::new(
                Severity::Warning,
                Retryability::Never,
                IntegrityImpact::None,
            ),
            Self::ResumeMismatch | Self::DependencyFailed => ErrorFacts::new(
                Severity::Error,
                Retryability::Never,
                IntegrityImpact::Unknown,
            ),
        }
    }
}

/// Internal error with a concrete cause and an independently safe public view.
#[derive(Clone, Copy)]
pub struct InternalError<E> {
    public: PublicErrorDto,
    cause: E,
}

impl<E> InternalError<E> {
    /// Associates a concrete internal cause with its safe public projection.
    #[must_use]
    pub const fn new(public: PublicErrorDto, cause: E) -> Self {
        Self { public, cause }
    }

    /// Returns the safe public view without exposing the internal cause.
    #[must_use]
    pub const fn public(&self) -> PublicErrorDto {
        self.public
    }
}

impl<E> fmt::Display for InternalError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.public, formatter)
    }
}

impl<E> fmt::Debug for InternalError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InternalError")
            .field("code", &self.public.code)
            .field("retry_hint", &self.public.retry_hint)
            .field("operation_id", &self.public.operation_id)
            .finish()
    }
}

impl<E: std::error::Error + 'static> std::error::Error for InternalError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// Failure found while resolving a resource name at a security boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResourceLookupFailure {
    /// No matching resource exists.
    NotFound,
    /// The caller cannot access the matching resource.
    Unauthorized,
}

/// Maps lookup failures while optionally hiding whether the resource exists.
#[must_use]
pub fn map_resource_lookup_error(
    failure: ResourceLookupFailure,
    conceal_existence: bool,
) -> PublicErrorDto {
    if conceal_existence {
        return PublicErrorDto::new(
            "WDB-SECURITY-RESOURCE-UNAVAILABLE",
            RetryHint::DoNotRetry,
            None,
            None,
        );
    }
    match failure {
        ResourceLookupFailure::NotFound => {
            PublicErrorDto::new("WDB-SECURITY-NOT-FOUND", RetryHint::DoNotRetry, None, None)
        }
        ResourceLookupFailure::Unauthorized => to_public_error(&SecurityError::AccessDenied),
    }
}

/// Recovery action selected by the higher application boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RecoveryAction {
    /// No recovery action is indicated.
    None,
    /// Run read-only verification before exposing the database.
    VerifyReadOnly,
    /// Run the database recovery procedure.
    RunRecovery,
    /// Resolve the operation status before retrying.
    ResolveOperationStatus,
    /// Require an explicit administrative decision.
    RequestAdministrator,
}

/// Applies an exhaustive, boundary-owned action policy to a recovery error.
#[must_use]
pub const fn recovery_action(error: RecoveryError, authorized: bool) -> RecoveryAction {
    if !authorized {
        return RecoveryAction::RequestAdministrator;
    }
    match error {
        RecoveryError::HistoryGap | RecoveryError::CorruptCommittedData => {
            RecoveryAction::VerifyReadOnly
        }
        RecoveryError::IndeterminateCommit => RecoveryAction::ResolveOperationStatus,
        RecoveryError::StorageFailure => RecoveryAction::RunRecovery,
        RecoveryError::DecisionRequired => RecoveryAction::RequestAdministrator,
    }
}

impl OpenError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::NotFound => "WDB-OPEN-NOT-FOUND",
            Self::Locked => "WDB-OPEN-LOCKED",
            Self::UnsupportedFormat => "WDB-OPEN-UNSUPPORTED-FORMAT",
            Self::NeedsMigration => "WDB-OPEN-NEEDS-MIGRATION",
            Self::RecoveryRequired => "WDB-OPEN-RECOVERY-REQUIRED",
            Self::PermissionDenied => "WDB-OPEN-PERMISSION-DENIED",
            Self::Corrupt => "WDB-OPEN-CORRUPT",
            Self::Io => "WDB-OPEN-IO",
        }
    }
}

impl ValidationError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::SchemaViolation => "WDB-VALIDATION-SCHEMA-VIOLATION",
            Self::InvalidValue => "WDB-VALIDATION-INVALID-VALUE",
            Self::InvalidReference => "WDB-VALIDATION-INVALID-REFERENCE",
            Self::InvalidTemporalRange => "WDB-VALIDATION-INVALID-TEMPORAL-RANGE",
            Self::UnsupportedOperation => "WDB-VALIDATION-UNSUPPORTED-OPERATION",
            Self::BudgetExceeded => "WDB-VALIDATION-BUDGET-EXCEEDED",
        }
    }
}

impl QueryError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::InvalidQuery => "WDB-QUERY-INVALID",
            Self::Cancelled => "WDB-QUERY-CANCELLED",
            Self::BudgetExceeded => "WDB-QUERY-BUDGET-EXCEEDED",
            Self::SnapshotExpired => "WDB-QUERY-SNAPSHOT-EXPIRED",
            Self::Unauthorized => "WDB-QUERY-UNAUTHORIZED",
            Self::StorageRead => "WDB-QUERY-STORAGE-READ",
            Self::CorruptData => "WDB-QUERY-CORRUPT-DATA",
        }
    }
}

impl CommitError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::Validation(_) => "WDB-COMMIT-VALIDATION",
            Self::Authorization(_) => "WDB-COMMIT-AUTHORIZATION",
            Self::Storage(_) => "WDB-COMMIT-STORAGE",
            Self::UnknownCommitOutcome { .. } => "WDB-COMMIT-UNKNOWN",
            Self::DatabaseReadOnly => "WDB-COMMIT-READ-ONLY",
            Self::ShuttingDown => "WDB-COMMIT-SHUTTING-DOWN",
        }
    }
}
impl RecoveryError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::HistoryGap => "WDB-RECOVERY-HISTORY-GAP",
            Self::CorruptCommittedData => "WDB-RECOVERY-CORRUPT-COMMITTED-DATA",
            Self::IndeterminateCommit => "WDB-RECOVERY-INDETERMINATE-COMMIT",
            Self::StorageFailure => "WDB-RECOVERY-STORAGE",
            Self::DecisionRequired => "WDB-RECOVERY-DECISION-REQUIRED",
        }
    }
}

impl MigrationError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::InvalidPlan => "WDB-MIGRATION-INVALID-PLAN",
            Self::UnsupportedFormat => "WDB-MIGRATION-UNSUPPORTED-FORMAT",
            Self::ConstraintViolation => "WDB-MIGRATION-CONSTRAINT-VIOLATION",
            Self::DecisionRequired => "WDB-MIGRATION-DECISION-REQUIRED",
            Self::ResumeMismatch => "WDB-MIGRATION-RESUME-MISMATCH",
            Self::StorageFailure => "WDB-MIGRATION-STORAGE",
        }
    }
}

impl BackupError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::InvalidManifest => "WDB-BACKUP-INVALID-MANIFEST",
            Self::IntegrityMismatch => "WDB-BACKUP-INTEGRITY-MISMATCH",
            Self::IncompatibleFormat => "WDB-BACKUP-INCOMPATIBLE-FORMAT",
            Self::AuthenticationFailure => "WDB-BACKUP-AUTHENTICATION",
            Self::SnapshotUnavailable => "WDB-BACKUP-SNAPSHOT-UNAVAILABLE",
            Self::StorageFailure => "WDB-BACKUP-STORAGE",
        }
    }
}

impl ExportError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::InvalidScope => "WDB-EXPORT-INVALID-SCOPE",
            Self::UnsupportedFormat => "WDB-EXPORT-UNSUPPORTED-FORMAT",
            Self::Unauthorized => "WDB-EXPORT-UNAUTHORIZED",
            Self::MissingDependency => "WDB-EXPORT-MISSING-DEPENDENCY",
            Self::Io => "WDB-EXPORT-IO",
        }
    }
}

impl SecurityError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::AccessDenied => "WDB-SECURITY-ACCESS-DENIED",
            Self::CapabilityMissing => "WDB-SECURITY-CAPABILITY-MISSING",
            Self::EpochChanged => "WDB-SECURITY-EPOCH-CHANGED",
            Self::InvalidPrincipal => "WDB-SECURITY-INVALID-PRINCIPAL",
        }
    }
}

impl JobError {
    /// Returns the stable public identifier for this error category.
    #[must_use]
    pub const fn public_code(self) -> &'static str {
        match self {
            Self::NotFound => "WDB-JOB-NOT-FOUND",
            Self::Cancelled => "WDB-JOB-CANCELLED",
            Self::BudgetExceeded => "WDB-JOB-BUDGET-EXCEEDED",
            Self::ResumeMismatch => "WDB-JOB-RESUME-MISMATCH",
            Self::DependencyFailed => "WDB-JOB-DEPENDENCY-FAILED",
        }
    }
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for OpenError {}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for ValidationError {}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for QueryError {}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for CommitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Authorization(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::UnknownCommitOutcome { .. } | Self::DatabaseReadOnly | Self::ShuttingDown => None,
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for StorageError {}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for RecoveryError {}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for MigrationError {}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for BackupError {}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for ExportError {}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for SecurityError {}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl fmt::Debug for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).public_code())
    }
}
impl std::error::Error for JobError {}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::{
        CommitError, CommitOutcome, ConflictFact, ConflictReport, IntegrityImpact, InternalError,
        JobError, OpenError, PublicErrorCode, RecoveryAction, RecoveryError, ResourceLookupFailure,
        RetryHint, Retryability, Severity, StorageError, StorageFailureClass, StorageOperation,
        map_resource_lookup_error, recovery_action, to_public_error, to_public_job_error,
    };
    use crate::ids::{DomainId, JobId, OperationId};

    fn operation_id() -> Option<OperationId> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = 1;
        OperationId::try_from_bytes(bytes).ok()
    }

    fn job_id() -> Option<JobId> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = 2;
        JobId::try_from_bytes(bytes).ok()
    }

    #[test]
    fn commit_conflict_and_unknown_outcome_are_distinct_protocol_types() {
        let conflict = CommitOutcome::Conflict(ConflictReport::new(vec![
            ConflictFact::ReadDependencyChanged,
        ]));
        assert!(matches!(conflict, CommitOutcome::Conflict(_)));
        assert_eq!(
            conflict,
            CommitOutcome::Conflict(ConflictReport::new(vec![
                ConflictFact::ReadDependencyChanged,
            ]))
        );
        if let Some(operation_id) = operation_id() {
            let unknown = CommitError::UnknownCommitOutcome { operation_id };
            assert_eq!(unknown.public_code(), "WDB-COMMIT-UNKNOWN");
            assert_ne!(unknown.public_code(), "WDB-COMMIT-CONFLICT");
        }
    }

    #[test]
    fn conflict_report_filter_keeps_only_authorized_facts() {
        let report = ConflictReport::new(vec![
            ConflictFact::ReadDependencyChanged,
            ConflictFact::WriteTargetChanged,
            ConflictFact::BaseRevisionAdvanced,
        ]);
        let visible = report.filtered(|fact| fact != ConflictFact::WriteTargetChanged);
        assert_eq!(
            visible.facts(),
            &[
                ConflictFact::ReadDependencyChanged,
                ConflictFact::BaseRevisionAdvanced,
            ]
        );
    }

    #[test]
    fn error_codes_are_stable_and_domain_specific() {
        assert_eq!(OpenError::NotFound.public_code(), "WDB-OPEN-NOT-FOUND");
        assert_ne!(
            OpenError::NotFound.public_code(),
            super::QueryError::InvalidQuery.public_code()
        );
        let storage = StorageError::new(StorageOperation::WalSync, StorageFailureClass::Durability);
        assert_eq!(storage.operation(), StorageOperation::WalSync);
        assert_eq!(storage.class(), StorageFailureClass::Durability);
        assert_eq!(storage.public_code(), "WDB-STORAGE-DURABILITY");
        let wire = crate::WireError::UnknownExternalCode { code: 9001 };
        assert_eq!(wire.public_code(), "WDB-WIRE-UNKNOWN-EXTERNAL-CODE");
        assert_eq!(wire.to_string(), wire.public_code());
        assert_eq!(format!("{wire:?}"), wire.public_code());
        assert_eq!(wire.to_string(), format!("{wire:?}"));
        assert_eq!(wire.to_string(), wire.to_string());
        if let Some(job_id) = job_id() {
            let public_job = to_public_job_error(&JobError::BudgetExceeded, job_id);
            assert_eq!(public_job.code(), "WDB-JOB-BUDGET-EXCEEDED");
            assert_eq!(public_job.job_id(), Some(job_id));
            assert_eq!(public_job.operation_id(), None);
        }
    }

    #[test]
    fn public_error_formatting_is_deterministic_and_has_no_side_effects() {
        let error = OpenError::Corrupt;
        let display_first = error.to_string();
        let debug_first = format!("{error:?}");
        assert_eq!(display_first, "WDB-OPEN-CORRUPT");
        assert_eq!(debug_first, display_first);
        assert_eq!(error.to_string(), display_first);
        assert_eq!(format!("{error:?}"), debug_first);

        let dto = to_public_error(&error);
        assert_eq!(dto, to_public_error(&error));
        assert_eq!(format!("{dto}"), display_first);
    }

    #[derive(Debug)]
    struct CanaryCause(&'static str);

    impl fmt::Display for CanaryCause {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl std::error::Error for CanaryCause {}

    #[test]
    fn public_error_views_hide_secret_causes_and_preserve_typed_sources() {
        const CANARY: &str = "WORLDDB-SECRET-CANARY-7F3A";
        let Some(operation_id) = operation_id() else {
            return;
        };
        let commit_error = CommitError::UnknownCommitOutcome { operation_id };
        assert_eq!(commit_error.to_string(), "WDB-COMMIT-UNKNOWN");
        assert_eq!(format!("{commit_error:?}"), "WDB-COMMIT-UNKNOWN");

        let public = to_public_error(&commit_error);
        assert_eq!(public.code(), "WDB-COMMIT-UNKNOWN");
        assert_eq!(public.retry_hint(), RetryHint::ResolveOperationStatus);
        assert_eq!(public.operation_id(), Some(operation_id));
        let facts = commit_error.facts();
        assert_eq!(facts.severity(), Severity::Critical);
        assert_eq!(facts.retryability(), Retryability::ResolveOperationStatus);
        assert_eq!(facts.integrity_impact(), IntegrityImpact::Unknown);
        assert!(!format!("{public}").contains(CANARY));
        assert!(!format!("{public:?}").contains(CANARY));

        let internal = InternalError::new(public, CanaryCause(CANARY));
        assert!(!format!("{internal}").contains(CANARY));
        assert!(!format!("{internal:?}").contains(CANARY));
        let source = std::error::Error::source(&internal);
        assert_eq!(source.map(ToString::to_string).as_deref(), Some(CANARY));
    }

    #[test]
    fn security_and_recovery_boundary_mappings_are_explicit() {
        let absent = map_resource_lookup_error(ResourceLookupFailure::NotFound, true);
        let denied = map_resource_lookup_error(ResourceLookupFailure::Unauthorized, true);
        assert_eq!(absent, denied);
        assert_eq!(absent.code(), "WDB-SECURITY-RESOURCE-UNAVAILABLE");
        assert_eq!(
            map_resource_lookup_error(ResourceLookupFailure::NotFound, false).code(),
            "WDB-SECURITY-NOT-FOUND"
        );
        assert_eq!(
            recovery_action(RecoveryError::HistoryGap, true),
            RecoveryAction::VerifyReadOnly
        );
        assert_eq!(
            recovery_action(RecoveryError::IndeterminateCommit, true),
            RecoveryAction::ResolveOperationStatus
        );
        assert_eq!(
            recovery_action(RecoveryError::DecisionRequired, false),
            RecoveryAction::RequestAdministrator
        );
        let corrupt_storage =
            StorageError::new(StorageOperation::Verify, StorageFailureClass::Corrupt);
        let facts = corrupt_storage.facts();
        assert_eq!(facts.severity(), Severity::Critical);
        assert_eq!(facts.retryability(), Retryability::Never);
        assert_eq!(
            facts.integrity_impact(),
            IntegrityImpact::CorruptionDetected
        );
    }
}
