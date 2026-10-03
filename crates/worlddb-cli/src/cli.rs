//! Versioned command-line interface and safe output boundary.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use worlddb_core::api::v1::{CURRENT_PROTOCOL, PublicCode, RequestId};
use worlddb_core::{
    AuditAction, AuditCommitContext, AuditObjectClass, AuditOutcome, AuditPolicyFingerprint,
    AuditRecord, AuditRecordDetails, AuditRecordIdentity, AuditSequence, AuthorizationDecision,
    AuthorizationMode, BreakingMigrationAdminAction, Bytes, Capability, DatabaseId, DomainId,
    EntityId, EntityTypeId, EventAttributeId, EventKindId, EventRoleId, HistorySpaceId, LayerId,
    MigrationAdminDecision, MigrationCategory, MigrationDryRun, MigrationId,
    MigrationItemResolution, MigrationPlan, MigrationRunId, MigrationRunJournalState,
    MigrationStepId, MigrationStepInput, MigrationTransformer, OperationId, PerspectiveId,
    PolicyTarget, PredicateId, PrincipalId, Record, RecordKind, Revision, RevisionBackend,
    SchemaDefinition, SchemaHistoryReferenceModel, SchemaMode, SchemaRevision, SecurityEpoch,
    TimelineId, UpgradePlanId, UpgradeRunId, ValidatedMigrationDecisions, decode_record,
    decode_record_ref,
};
use worlddb_storage_file::{
    BackupAuthenticity, BackupError, BackupProfile, BackupVerification, DatabaseLayout,
    ExactBackupManager, FileStoreGuardedMigrationRun, FormatProbeError, HistorySegmentStore,
    LogicalExport, LogicalExportError, LogicalExportManager, LogicalExportScope,
    LogicalImportDestinationInventory, LogicalImportError, LogicalImportIdMapping,
    LogicalImportIdentity, LogicalImportManager, LogicalImportPlan, Manifest, ManifestSegmentKind,
    ManifestStore, MigrationRestorePointError, RecoveryDisposition, RecoveryError, RecoveryManager,
    RestoreError, RestoreManager, SalvageError, SalvageInventorySource, SalvageManager,
    SalvageSegmentOutcome, SecurityPolicyHistorySnapshot, SecurityPolicyHistoryStore,
    SharingExportError, SharingExportManager, SharingExportScope, StorageDamageClass,
    StorageFileError, StorageUpgradeBudget, StorageUpgradeManager, StorageUpgradeRestoreTargets,
    StorageVerifier, StorageVerifyAction, StorageVerifyError, StorageVerifyReport, WalPrepareLog,
    WriterLock, WriterLockError, verify_audit_complete_backup, verify_exact_backup,
};

use crate::adapter_protocol::{
    AdapterManifest, AdapterProcessHost, AdapterProtocolError, MAX_ADAPTER_MANIFEST_ENCODED_BYTES,
};

const EXIT_SUCCESS: u8 = 0;
const EXIT_INVALID_REQUEST: u8 = 2;
const EXIT_UNSUPPORTED_OPERATION: u8 = 3;
const EXIT_UNAUTHORIZED: u8 = 4;
const EXIT_NOT_FOUND: u8 = 5;
const EXIT_STATE_INVALIDATED: u8 = 6;
const EXIT_CORRUPT_DATA: u8 = 7;
const EXIT_STORAGE_READ: u8 = 8;
const EXIT_CANCELLED: u8 = 9;
const EXIT_BUDGET_EXCEEDED: u8 = 10;
const EXIT_INTERNAL: u8 = 70;

const LOGICAL_ARTIFACT_MAX_BYTES: u64 = 512 * 1024 * 1024;
const LOGICAL_IMPORT_PLAN_MAX_BYTES: u64 = 64 * 1024 * 1024;
const LOGICAL_IMPORT_MAX_INVENTORY_IDENTITIES: usize = 2_000_000;

const HELP_ROOT: &str = "WorldDB CLI\n\nUsage: worlddb-cli [--format human|jsonl] <COMMAND>\n\nCommands:\n  v1 help                    Show version 1 command help\n  v1 version                 Show CLI and protocol versions\n  v1 verify <database>       Read-only storage verification\n  v1 recovery inspect <db>   Read-only recovery and damage report\n  v1 recovery run --apply <db>  Explicit journaled recovery\n  v1 open --read-only <db>   Validate a database without writing\n  v1 salvage <source> --output <new-dir>  Copy verified data to a new fork\n  v1 backup create/verify    Create or verify Exact/AuditComplete backups\n  v1 restore clone           Restore a verified backup as a new database\n  v1 migration               Plan, preview, run, or resume a schema migration\n  v1 export logical|share    Export a declared, authorized scope\n  v1 import plan|prepare     Create or validate an explicit remap plan\n  v1 storage upgrade          Upgrade the storage format with restore proof\n  v1 adapter run             Run an isolated import/export adapter\n  --help                     Show this help\n  --version                  Show version information\n\nThe unversioned `adapter run` command remains available as a compatibility alias.";

const HELP_ADAPTER_RUN: &str = "Usage: worlddb-cli [--format human|jsonl] v1 adapter run --manifest <file> --input <file> --output <file> -- <adapter-executable> [arguments...]\n\nThe manifest binds the operation, deterministic seed, ID mapping, protocol capabilities, and process budgets. The adapter receives only framed stdin/stdout data; the output file is written only after a clean adapter exit.";
const HELP_VERIFY: &str = "Usage: worlddb-cli [--format human|jsonl] v1 verify <database-directory>\n\nRuns read-only storage verification under a shared lock. The report includes safe_revision, disposition, observed damage classes, and safe next actions.";
const HELP_RECOVERY: &str = "Usage: worlddb-cli [--format human|jsonl] v1 recovery inspect <database-directory>\n       worlddb-cli [--format human|jsonl] v1 recovery run --apply <database-directory>\n\n`inspect` is read-only. `run --apply` explicitly enables journaled recovery of eligible WAL tails and committed snapshots.";
const HELP_OPEN_READ_ONLY: &str = "Usage: worlddb-cli [--format human|jsonl] v1 open --read-only <database-directory>\n\nValidates and verifies the database without creating files, changing storage, or enabling writes.";
const HELP_SALVAGE: &str = "Usage: worlddb-cli [--format human|jsonl] v1 salvage <source-directory> --output <new-directory>\n\nCopies verified immutable data to a new marked salvage fork. The source is opened with a shared read-only lock and is never repaired or rewritten.";
const HELP_BACKUP: &str = "Usage: worlddb-cli [--format human|jsonl] v1 backup create <source-directory> --output <new-directory> --profile exact|audit-complete --audit-scope excluded|included\n       worlddb-cli [--format human|jsonl] v1 backup verify <backup-directory> --profile exact|audit-complete --audit-scope excluded|included\n\nThe profile and matching audit scope are mandatory and repeated in every result. Exact excludes audit history. AuditComplete includes the supported raw-read audit prefix. Creation requires the current host-bound ProjectRead and BackupCreate capabilities; AuditComplete also requires AuditRead and AuditExport.";
const HELP_RESTORE: &str = "Usage: worlddb-cli [--format human|jsonl] v1 restore clone <backup-directory> --authorize-with <current-project-directory> --output <new-database-directory> --profile exact|audit-complete --audit-scope excluded|included\n\nRestore checks the current host-bound BackupRestore capability in the authorization project. AuditComplete also requires AuditRead and AuditExport. The authorization project must be the database named by the backup. Same-identity disaster recovery is not supported by the storage contract.";
const HELP_MIGRATION: &str = "Usage: worlddb-cli [--format human|jsonl] v1 migration plan|dry-run|run|resume <database-directory> --plan-file <canonical-MigrationPlan-record> [--run-id <uuid>] [--step <step-uuid> [--operation-id <uuid>] [--record <canonical-record-file>...]] [--omit <record-index>|--replace <record-index> <canonical-record-file>]... [--backup <exact-backup-directory> --restore <new-clone-directory> --confirm-breaking]\n\nPlan and dry-run are read-only. Every supplied record file contains exactly one canonical WorldDB record frame. Step groups must match the plan order. Run and resume require current MigrationExecute permission and stable run/step operation IDs. Breaking requires an exact backup, a real verified restore clone, and the explicit --confirm-breaking flag; resume uses the retained backup and a new restore-clone destination.";
const HELP_STORAGE_UPGRADE: &str = "Usage: worlddb-cli [--format human|jsonl] v1 storage upgrade <database-directory> --backup <new-exact-backup-directory> --restore <new-clone-directory> --confirm\n\nPrepares the supported CURRENT v1 to v2 upgrade, creates an exact backup, verifies a real clone restore, requires current StorageFormatUpgrade, BackupCreate, and BackupRestore permissions, then publishes the format upgrade. The --confirm flag is mandatory.";
const HELP_EXPORT_IMPORT: &str = "Usage: worlddb-cli [--format human|jsonl] v1 export logical|share <database> --output <new-artifact> --from <revision> --through <revision> --history-space <uuid>... --class <RecordKind>...\n       worlddb-cli [--format human|jsonl] v1 import plan <destination> --input <logical-artifact> --output <new-plan> [--map <typed-identity>=<typed-identity>]...\n       worlddb-cli [--format human|jsonl] v1 import prepare <destination> --input <logical-artifact> --plan-file <canonical-plan>\n\nLogical export embeds the full scope and omission manifest. Sharing export filters records by current rights and never reports omission counts. Import plan uses explicit typed remaps such as entity:<uuid>=entity:<uuid> or record:7:<uuid>=record:7:<uuid>; prepare validates the plan against the current destination inventory and DataImport permission. Prepare does not publish records to the database. Valid remap families: history-space, layer, perspective, timeline, entity, entity-type, predicate, event-kind, event-role, event-attribute, and record:<RecordRef wire tag>.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Human,
    JsonLines,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelpScope {
    Root,
    AdapterRun,
    Verify,
    Recovery,
    OpenReadOnly,
    Salvage,
    Backup,
    Restore,
    Migration,
    StorageUpgrade,
    ExportImport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MigrationStatus {
    Planned,
    Previewed,
    Completed,
    Resumed,
}

impl MigrationStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Previewed => "previewed",
            Self::Completed => "completed",
            Self::Resumed => "resumed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MigrationSummary {
    status: MigrationStatus,
    database_id: DatabaseId,
    migration_id: MigrationId,
    category: MigrationCategory,
    fingerprint: [u8; 32],
    source_revision: u64,
    target_revision: u64,
    step_count: usize,
    input_record_count: u64,
    input_bytes: u64,
    estimate_output_bytes: Option<u64>,
    error_count: u64,
    unresolved_count: usize,
    completed_step_count: usize,
    final_revision: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StorageUpgradeSummary {
    plan_id: UpgradePlanId,
    run_id: UpgradeRunId,
    source_revision: u64,
    target_fingerprint: [u8; 32],
    pointer_digest: [u8; 32],
    resumed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExportSummary {
    sharing: bool,
    database_id: Option<DatabaseId>,
    snapshot_revision: Option<u64>,
    from_revision: u64,
    through_revision: u64,
    history_space_count: usize,
    record_kind_count: usize,
    selected_history_spaces: Vec<String>,
    selected_record_kinds: Vec<String>,
    record_count: usize,
    omitted_record_class_count: usize,
    omitted_storage_class_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImportPlanSummary {
    source_database_id: DatabaseId,
    destination_database_id: DatabaseId,
    mapping_count: usize,
    plan_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImportPrepareSummary {
    source_database_id: DatabaseId,
    destination_database_id: DatabaseId,
    stream_fingerprint: [u8; 32],
    mapping_count: usize,
    record_count: usize,
    from_revision: u64,
    through_revision: u64,
    history_space_count: usize,
    record_kind_count: usize,
    omitted_record_class_count: usize,
    omitted_storage_class_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExportKind {
    Logical,
    Sharing,
}

impl ExportKind {
    const fn is_sharing(self) -> bool {
        matches!(self, Self::Sharing)
    }
}

#[derive(Debug)]
struct ExportCommandRequest {
    kind: ExportKind,
    database_path: PathBuf,
    output_path: PathBuf,
    from_revision: Revision,
    through_revision: Revision,
    history_spaces: Vec<HistorySpaceId>,
    record_kinds: Vec<RecordKind>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MigrationCommandAction {
    Plan,
    DryRun,
    Run,
    Resume,
}

#[derive(Debug)]
struct MigrationStepFiles {
    step_id: MigrationStepId,
    operation_id: Option<OperationId>,
    record_files: Vec<PathBuf>,
}

#[derive(Debug)]
struct MigrationResolutionSpec {
    record_index: u64,
    resolution: MigrationItemResolution,
}

#[derive(Debug)]
struct MigrationCommandRequest {
    action: MigrationCommandAction,
    database_path: PathBuf,
    plan_path: PathBuf,
    run_id: Option<MigrationRunId>,
    steps: Vec<MigrationStepFiles>,
    resolutions: Vec<MigrationResolutionSpec>,
    backup_path: Option<PathBuf>,
    restore_path: Option<PathBuf>,
    confirm_breaking: bool,
}

#[derive(Debug)]
struct LoadedMigrationStep {
    step_id: MigrationStepId,
    operation_id: Option<OperationId>,
    records: Vec<Vec<u8>>,
}

struct MigrationAuditContext<'a> {
    plan: &'a MigrationPlan,
    actor: PrincipalId,
    policy: &'a worlddb_core::SecurityPolicySnapshot,
    epoch: SecurityEpoch,
    target: PolicyTarget,
    layout: &'a DatabaseLayout,
    writer_lock: &'a WriterLock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackupAction {
    Created,
    Verified,
}

impl BackupAction {
    const fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Verified => "verified",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackupAuditScope {
    Excluded,
    Included,
}

impl BackupAuditScope {
    const fn label(self) -> &'static str {
        match self {
            Self::Excluded => "Excluded",
            Self::Included => "Included",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthenticityLabel {
    NotClaimed,
    ClaimedButUnverified,
    Verified,
    WrongKey,
    InvalidMac,
}

impl AuthenticityLabel {
    const fn label(self) -> &'static str {
        match self {
            Self::NotClaimed => "NotClaimed",
            Self::ClaimedButUnverified => "ClaimedButUnverified",
            Self::Verified => "Verified",
            Self::WrongKey => "WrongKey",
            Self::InvalidMac => "InvalidMac",
        }
    }

    fn from_authenticity(authenticity: &BackupAuthenticity) -> Self {
        match authenticity {
            BackupAuthenticity::NotClaimed => Self::NotClaimed,
            BackupAuthenticity::ClaimedButUnverified { .. } => Self::ClaimedButUnverified,
            BackupAuthenticity::Verified { .. } => Self::Verified,
            BackupAuthenticity::WrongKey { .. } => Self::WrongKey,
            BackupAuthenticity::InvalidMac { .. } => Self::InvalidMac,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackupSummary {
    database_id: DatabaseId,
    revision: u64,
    item_count: usize,
    audit_safe_sequence: Option<u64>,
    authenticity: AuthenticityLabel,
}

impl BackupSummary {
    fn from_verification(verification: &BackupVerification) -> Self {
        Self {
            database_id: verification.database_id(),
            revision: verification.revision().value(),
            item_count: verification.item_count(),
            audit_safe_sequence: verification
                .audit_safe_sequence()
                .map(|sequence| sequence.value()),
            authenticity: AuthenticityLabel::from_authenticity(verification.authenticity()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RestoreSummary {
    source_database_id: DatabaseId,
    source_revision: u64,
    restored_database_id: DatabaseId,
    restored_revision: u64,
    item_count: usize,
    audit_safe_sequence: Option<u64>,
    authenticity: AuthenticityLabel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CheckKind {
    Verify,
    RecoveryInspect,
    OpenReadOnly,
}

impl CheckKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Verify => "verify",
            Self::RecoveryInspect => "recovery_inspect",
            Self::OpenReadOnly => "open_read_only",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DamageSummary {
    counts: [usize; 6],
    actions: [bool; 6],
}

impl DamageSummary {
    fn from_report(report: &StorageVerifyReport) -> Self {
        let mut summary = Self::default();
        for finding in report.findings() {
            if let Some(count) = summary.counts.get_mut(damage_class_index(finding.class())) {
                *count = (*count).saturating_add(1);
            }
            for action in finding.safe_next_actions() {
                if let Some(selected) = summary.actions.get_mut(verify_action_index(*action)) {
                    *selected = true;
                }
            }
        }
        summary
    }

    fn finding_count(self) -> usize {
        self.counts.iter().copied().sum()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Success {
    Help(HelpScope),
    Version,
    StorageCheck {
        kind: CheckKind,
        safe_revision: u64,
        disposition: RecoveryDisposition,
        damage: DamageSummary,
    },
    RecoveryApplied {
        safe_revision: u64,
        disposition: RecoveryDisposition,
        damage: DamageSummary,
        quarantined_tails: usize,
        replayed_snapshots: usize,
        manifest_published: bool,
    },
    Salvage {
        database_id: DatabaseId,
        safe_revision: u64,
        disposition: RecoveryDisposition,
        inventory_source: SalvageInventorySource,
        copied_segments: usize,
        omitted_segments: usize,
        finding_count: usize,
    },
    Backup {
        action: BackupAction,
        profile: BackupProfile,
        audit_scope: BackupAuditScope,
        summary: BackupSummary,
    },
    Restore {
        profile: BackupProfile,
        audit_scope: BackupAuditScope,
        summary: RestoreSummary,
    },
    Migration(MigrationSummary),
    StorageUpgrade(StorageUpgradeSummary),
    Export(ExportSummary),
    ImportPlan(ImportPlanSummary),
    ImportPrepared(ImportPrepareSummary),
    AdapterRun {
        protocol_major: u16,
        protocol_minor: u16,
        output_bytes: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CliError {
    code: PublicCode,
}

impl CliError {
    const fn new(code: PublicCode) -> Self {
        Self { code }
    }

    const fn invalid_request() -> Self {
        Self::new(PublicCode::INVALID_REQUEST)
    }

    const fn unsupported_operation() -> Self {
        Self::new(PublicCode::UNSUPPORTED_OPERATION)
    }

    fn exit_status(self) -> u8 {
        match self.code.as_str() {
            "InvalidRequest" | "InvalidQuery" => EXIT_INVALID_REQUEST,
            "UnsupportedProtocolVersion"
            | "UnsupportedOperation"
            | "UnsupportedQueryCapability" => EXIT_UNSUPPORTED_OPERATION,
            "Unauthorized" => EXIT_UNAUTHORIZED,
            "NotFound" => EXIT_NOT_FOUND,
            "SnapshotExpired" | "CursorInvalidated" => EXIT_STATE_INVALIDATED,
            "CorruptData" => EXIT_CORRUPT_DATA,
            "StorageRead" => EXIT_STORAGE_READ,
            "Cancelled" => EXIT_CANCELLED,
            "BudgetExceeded" => EXIT_BUDGET_EXCEEDED,
            _ => EXIT_INTERNAL,
        }
    }
}

/// Runs the CLI and returns its documented process status code.
pub fn run<I>(arguments: I) -> u8
where
    I: IntoIterator<Item = OsString>,
{
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    run_with(arguments, &mut stdout, &mut stderr)
}

/// Runs the CLI against caller-provided streams, which also keeps output behavior testable.
pub fn run_with<I, O, E>(arguments: I, stdout: &mut O, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = OsString>,
    O: Write,
    E: Write,
{
    let (format, arguments, global_result) = parse_global_options(arguments);
    let (request_id, request_id_result) = match format {
        OutputFormat::Human => (RequestId::from_bytes([0; 16]), Ok(())),
        OutputFormat::JsonLines => match new_request_id() {
            Ok(request_id) => (request_id, Ok(())),
            Err(()) => (
                RequestId::from_bytes([0; 16]),
                Err(CliError::new(PublicCode::INTERNAL)),
            ),
        },
    };

    let result = match (global_result, request_id_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => parse_command(arguments),
    };

    match result {
        Ok(success) => {
            if write_success(stdout, format, request_id, success).is_ok() {
                EXIT_SUCCESS
            } else {
                let _ = writeln!(stderr, "error[Internal]: Output could not be written.");
                EXIT_INTERNAL
            }
        }
        Err(error) => {
            let status = error.exit_status();
            let write_result = match format {
                OutputFormat::Human => write_human_error(stderr, error),
                OutputFormat::JsonLines => write_json_error(stdout, request_id, error),
            };
            if write_result.is_err() {
                EXIT_INTERNAL
            } else {
                status
            }
        }
    }
}

fn parse_global_options<I>(arguments: I) -> (OutputFormat, VecDeque<OsString>, Result<(), CliError>)
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter().collect::<VecDeque<_>>();
    let mut output_format = OutputFormat::Human;
    let mut format_seen = false;

    loop {
        let Some(argument) = arguments.front() else {
            break;
        };
        let Some(text) = argument.to_str() else {
            break;
        };
        let (inline_value, is_format_option) = match text.strip_prefix("--format=") {
            Some(value) => (Some(String::from(value)), true),
            None if text == "--format" => (None, true),
            None => (None, false),
        };
        if !is_format_option {
            break;
        }
        let _ = arguments.pop_front();
        if format_seen {
            return (output_format, arguments, Err(CliError::invalid_request()));
        }
        format_seen = true;

        let format_value = match inline_value {
            Some(value) => Some(value),
            None => arguments
                .pop_front()
                .and_then(|value| value.into_string().ok()),
        };
        match format_value.as_deref() {
            Some("human") => output_format = OutputFormat::Human,
            Some("jsonl") => output_format = OutputFormat::JsonLines,
            _ => return (output_format, arguments, Err(CliError::invalid_request())),
        }
    }

    (output_format, arguments, Ok(()))
}

fn parse_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(command) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::Root));
    };
    if command == "--help" || command == "-h" || command == "help" {
        if arguments.is_empty() {
            return Ok(Success::Help(HelpScope::Root));
        }
        return parse_help_scope(arguments);
    }
    if command == "--version" || command == "version" {
        return if arguments.is_empty() {
            Ok(Success::Version)
        } else {
            Err(CliError::invalid_request())
        };
    }
    if command == "v1" {
        return parse_v1_command(arguments);
    }
    if command == "adapter" {
        return parse_adapter_command(arguments);
    }

    match command.to_str() {
        Some(version) if is_version_namespace(version) => {
            Err(CliError::new(PublicCode::UNSUPPORTED_PROTOCOL_VERSION))
        }
        Some(_) => Err(CliError::unsupported_operation()),
        None => Err(CliError::invalid_request()),
    }
}

fn parse_help_scope(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(scope) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::Root));
    };
    if scope == "adapter"
        && arguments.pop_front().is_some_and(|run| run == "run")
        && arguments.is_empty()
    {
        return Ok(Success::Help(HelpScope::AdapterRun));
    }
    if scope == "verify" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::Verify));
    }
    if scope == "recovery" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::Recovery));
    }
    if scope == "open" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::OpenReadOnly));
    }
    if scope == "salvage" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::Salvage));
    }
    if scope == "backup" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::Backup));
    }
    if scope == "restore" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::Restore));
    }
    if scope == "migration" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::Migration));
    }
    if scope == "storage-upgrade" && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::StorageUpgrade));
    }
    if matches!(scope.to_str(), Some("export" | "import")) && arguments.is_empty() {
        return Ok(Success::Help(HelpScope::ExportImport));
    }
    Err(CliError::invalid_request())
}

fn parse_v1_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(command) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::Root));
    };
    if command == "--help" || command == "-h" || command == "help" {
        if arguments.is_empty() {
            return Ok(Success::Help(HelpScope::Root));
        }
        return parse_help_scope(arguments);
    }
    if command == "--version" || command == "version" {
        return if arguments.is_empty() {
            Ok(Success::Version)
        } else {
            Err(CliError::invalid_request())
        };
    }
    if command == "adapter" {
        return parse_adapter_command(arguments);
    }
    if command == "verify" {
        return parse_verify_command(arguments);
    }
    if command == "recovery" {
        return parse_recovery_command(arguments);
    }
    if command == "open" {
        return parse_open_command(arguments);
    }
    if command == "salvage" {
        return parse_salvage_command(arguments);
    }
    if command == "backup" {
        return parse_backup_command(arguments);
    }
    if command == "restore" {
        return parse_restore_command(arguments);
    }
    if command == "migration" {
        return parse_migration_command(arguments);
    }
    if command == "storage" {
        return parse_storage_command(arguments);
    }
    if command == "export" {
        return parse_export_command(arguments);
    }
    if command == "import" {
        return parse_import_command(arguments);
    }
    Err(CliError::unsupported_operation())
}

fn parse_adapter_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(subcommand) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    if subcommand == "--help" || subcommand == "-h" || subcommand == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::AdapterRun))
        } else {
            Err(CliError::invalid_request())
        };
    }
    if subcommand != "run" {
        return Err(CliError::unsupported_operation());
    }
    if arguments.len() == 1
        && arguments
            .front()
            .is_some_and(|argument| argument == "--help")
    {
        return Ok(Success::Help(HelpScope::AdapterRun));
    }
    run_adapter(arguments)
}

fn parse_verify_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    if arguments.len() == 1
        && arguments
            .front()
            .is_some_and(|argument| argument == "--help")
    {
        return Ok(Success::Help(HelpScope::Verify));
    }
    let Some(database_path) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    if !arguments.is_empty() {
        return Err(CliError::invalid_request());
    }
    run_storage_check(Path::new(&database_path), CheckKind::Verify)
}

fn parse_open_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    if arguments.len() == 1
        && arguments
            .front()
            .is_some_and(|argument| argument == "--help")
    {
        return Ok(Success::Help(HelpScope::OpenReadOnly));
    }
    if arguments.pop_front().as_deref() != Some(OsString::from("--read-only").as_os_str()) {
        return Err(CliError::invalid_request());
    }
    let Some(database_path) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    if !arguments.is_empty() {
        return Err(CliError::invalid_request());
    }
    run_storage_check(Path::new(&database_path), CheckKind::OpenReadOnly)
}

fn parse_recovery_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(subcommand) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::Recovery));
    };
    if subcommand == "--help" || subcommand == "-h" || subcommand == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::Recovery))
        } else {
            Err(CliError::invalid_request())
        };
    }
    if subcommand == "inspect" {
        if arguments.len() == 1
            && arguments
                .front()
                .is_some_and(|argument| argument == "--help")
        {
            return Ok(Success::Help(HelpScope::Recovery));
        }
        let Some(database_path) = arguments.pop_front() else {
            return Err(CliError::invalid_request());
        };
        if !arguments.is_empty() {
            return Err(CliError::invalid_request());
        }
        return run_storage_check(Path::new(&database_path), CheckKind::RecoveryInspect);
    }
    if subcommand == "run" {
        if arguments.len() == 1
            && arguments
                .front()
                .is_some_and(|argument| argument == "--help")
        {
            return Ok(Success::Help(HelpScope::Recovery));
        }
        if arguments.pop_front().as_deref() != Some(OsString::from("--apply").as_os_str()) {
            return Err(CliError::invalid_request());
        }
        let Some(database_path) = arguments.pop_front() else {
            return Err(CliError::invalid_request());
        };
        if !arguments.is_empty() {
            return Err(CliError::invalid_request());
        }
        return run_recovery(Path::new(&database_path));
    }
    Err(CliError::unsupported_operation())
}

fn parse_salvage_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    if arguments.len() == 1
        && arguments
            .front()
            .is_some_and(|argument| argument == "--help")
    {
        return Ok(Success::Help(HelpScope::Salvage));
    }
    let Some(source_path) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    if arguments.pop_front().as_deref() != Some(OsString::from("--output").as_os_str()) {
        return Err(CliError::invalid_request());
    }
    let Some(target_path) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    if !arguments.is_empty() {
        return Err(CliError::invalid_request());
    }
    run_salvage(Path::new(&source_path), Path::new(&target_path))
}

fn parse_backup_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(action) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::Backup));
    };
    if action == "--help" || action == "-h" || action == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::Backup))
        } else {
            Err(CliError::invalid_request())
        };
    }

    match action.to_str() {
        Some("create") => {
            let Some(source_path) = arguments.pop_front() else {
                return Err(CliError::invalid_request());
            };
            if !take_option(&mut arguments, "--output") {
                return Err(CliError::invalid_request());
            }
            let Some(target_path) = arguments.pop_front() else {
                return Err(CliError::invalid_request());
            };
            let (profile, audit_scope) = take_profile_and_scope(&mut arguments)?;
            if !arguments.is_empty() {
                return Err(CliError::invalid_request());
            }
            run_backup_create(
                Path::new(&source_path),
                Path::new(&target_path),
                profile,
                audit_scope,
            )
        }
        Some("verify") => {
            let Some(backup_path) = arguments.pop_front() else {
                return Err(CliError::invalid_request());
            };
            let (profile, audit_scope) = take_profile_and_scope(&mut arguments)?;
            if !arguments.is_empty() {
                return Err(CliError::invalid_request());
            }
            run_backup_verify(Path::new(&backup_path), profile, audit_scope)
        }
        Some(_) => Err(CliError::unsupported_operation()),
        None => Err(CliError::invalid_request()),
    }
}

fn parse_export_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(kind) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::ExportImport));
    };
    if kind == "--help" || kind == "-h" || kind == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::ExportImport))
        } else {
            Err(CliError::invalid_request())
        };
    }
    let kind = match kind.to_str() {
        Some("logical") => ExportKind::Logical,
        Some("share") => ExportKind::Sharing,
        Some(_) => return Err(CliError::unsupported_operation()),
        None => return Err(CliError::invalid_request()),
    };
    let database_path = arguments
        .pop_front()
        .map(PathBuf::from)
        .ok_or_else(CliError::invalid_request)?;
    let mut output_path = None;
    let mut from_revision = None;
    let mut through_revision = None;
    let mut history_spaces = Vec::new();
    let mut record_kinds = Vec::new();
    while let Some(option) = arguments.pop_front() {
        match option.to_str() {
            Some("--output") if output_path.is_none() => {
                output_path = Some(PathBuf::from(
                    arguments
                        .pop_front()
                        .ok_or_else(CliError::invalid_request)?,
                ));
            }
            Some("--from") if from_revision.is_none() => {
                from_revision = Some(parse_cli_id::<Revision>(
                    arguments
                        .pop_front()
                        .ok_or_else(CliError::invalid_request)?,
                )?);
            }
            Some("--through") if through_revision.is_none() => {
                through_revision = Some(parse_cli_id::<Revision>(
                    arguments
                        .pop_front()
                        .ok_or_else(CliError::invalid_request)?,
                )?);
            }
            Some("--history-space") => {
                history_spaces.push(parse_cli_id::<HistorySpaceId>(
                    arguments
                        .pop_front()
                        .ok_or_else(CliError::invalid_request)?,
                )?);
            }
            Some("--class") => {
                let label = pop_cli_text(&mut arguments)?;
                record_kinds.push(parse_record_kind(&label)?);
            }
            _ => return Err(CliError::invalid_request()),
        }
    }
    let request = ExportCommandRequest {
        kind,
        database_path,
        output_path: output_path.ok_or_else(CliError::invalid_request)?,
        from_revision: from_revision.ok_or_else(CliError::invalid_request)?,
        through_revision: through_revision.ok_or_else(CliError::invalid_request)?,
        history_spaces,
        record_kinds,
    };
    run_export_command(request)
}

fn parse_import_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(action) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::ExportImport));
    };
    if action == "--help" || action == "-h" || action == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::ExportImport))
        } else {
            Err(CliError::invalid_request())
        };
    }
    match action.to_str() {
        Some("plan") => {
            let destination_path = arguments
                .pop_front()
                .map(PathBuf::from)
                .ok_or_else(CliError::invalid_request)?;
            let mut input_path = None;
            let mut output_path = None;
            let mut mappings = Vec::new();
            while let Some(option) = arguments.pop_front() {
                match option.to_str() {
                    Some("--input") if input_path.is_none() => {
                        input_path = Some(PathBuf::from(
                            arguments
                                .pop_front()
                                .ok_or_else(CliError::invalid_request)?,
                        ));
                    }
                    Some("--output") if output_path.is_none() => {
                        output_path = Some(PathBuf::from(
                            arguments
                                .pop_front()
                                .ok_or_else(CliError::invalid_request)?,
                        ));
                    }
                    Some("--map") => {
                        mappings.push(parse_import_mapping(&pop_cli_text(&mut arguments)?)?);
                        if mappings.len() > 1_000_000 {
                            return Err(CliError::new(PublicCode::BUDGET_EXCEEDED));
                        }
                    }
                    _ => return Err(CliError::invalid_request()),
                }
            }
            run_import_plan(
                &destination_path,
                &input_path.ok_or_else(CliError::invalid_request)?,
                &output_path.ok_or_else(CliError::invalid_request)?,
                mappings,
            )
        }
        Some("prepare") => {
            let destination_path = arguments
                .pop_front()
                .map(PathBuf::from)
                .ok_or_else(CliError::invalid_request)?;
            if !take_option(&mut arguments, "--input") {
                return Err(CliError::invalid_request());
            }
            let input_path = arguments
                .pop_front()
                .map(PathBuf::from)
                .ok_or_else(CliError::invalid_request)?;
            if !take_option(&mut arguments, "--plan-file") {
                return Err(CliError::invalid_request());
            }
            let plan_path = arguments
                .pop_front()
                .map(PathBuf::from)
                .ok_or_else(CliError::invalid_request)?;
            if !arguments.is_empty() {
                return Err(CliError::invalid_request());
            }
            run_import_prepare(&destination_path, &input_path, &plan_path)
        }
        Some(_) => Err(CliError::unsupported_operation()),
        None => Err(CliError::invalid_request()),
    }
}

fn pop_cli_text(arguments: &mut VecDeque<OsString>) -> Result<String, CliError> {
    arguments
        .pop_front()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(CliError::invalid_request)
}

fn parse_record_kind(value: &str) -> Result<RecordKind, CliError> {
    RecordKind::ALL
        .into_iter()
        .find(|kind| record_kind_label(*kind) == value)
        .ok_or_else(CliError::invalid_request)
}

fn parse_import_mapping(value: &str) -> Result<LogicalImportIdMapping, CliError> {
    let Some((source, target)) = value.split_once('=') else {
        return Err(CliError::invalid_request());
    };
    if target.contains('=') {
        return Err(CliError::invalid_request());
    }
    LogicalImportIdMapping::new(
        parse_import_identity(source)?,
        parse_import_identity(target)?,
    )
    .map_err(|_| CliError::invalid_request())
}

fn parse_import_identity(value: &str) -> Result<LogicalImportIdentity, CliError> {
    let mut parts = value.split(':');
    let family = parts.next().ok_or_else(CliError::invalid_request)?;
    let first = parts.next().ok_or_else(CliError::invalid_request)?;
    match (family, parts.next()) {
        ("history-space", None) => Ok(LogicalImportIdentity::HistorySpace(parse_cli_id::<
            HistorySpaceId,
        >(
            OsString::from(first),
        )?)),
        ("layer", None) => Ok(LogicalImportIdentity::Layer(parse_cli_id::<LayerId>(
            OsString::from(first),
        )?)),
        ("perspective", None) => Ok(LogicalImportIdentity::Perspective(parse_cli_id::<
            PerspectiveId,
        >(
            OsString::from(first)
        )?)),
        ("timeline", None) => Ok(LogicalImportIdentity::Timeline(parse_cli_id::<TimelineId>(
            OsString::from(first),
        )?)),
        ("entity", None) => Ok(LogicalImportIdentity::Entity(parse_cli_id::<EntityId>(
            OsString::from(first),
        )?)),
        ("entity-type", None) => Ok(LogicalImportIdentity::EntityType(parse_cli_id::<
            EntityTypeId,
        >(
            OsString::from(first)
        )?)),
        ("predicate", None) => Ok(LogicalImportIdentity::Predicate(
            parse_cli_id::<PredicateId>(OsString::from(first))?,
        )),
        ("event-kind", None) => Ok(LogicalImportIdentity::EventKind(
            parse_cli_id::<EventKindId>(OsString::from(first))?,
        )),
        ("event-role", None) => Ok(LogicalImportIdentity::EventRole(
            parse_cli_id::<EventRoleId>(OsString::from(first))?,
        )),
        ("event-attribute", None) => Ok(LogicalImportIdentity::EventAttribute(parse_cli_id::<
            EventAttributeId,
        >(
            OsString::from(first),
        )?)),
        ("record", Some(identity)) if parts.next().is_none() => {
            let tag = first
                .parse::<u16>()
                .ok()
                .and_then(|tag| u8::try_from(tag).ok())
                .filter(|tag| *tag > 0)
                .ok_or_else(CliError::invalid_request)?;
            let id = parse_cli_id::<DatabaseId>(OsString::from(identity))?;
            let mut bytes = Vec::with_capacity(17);
            bytes.push(tag);
            bytes.extend_from_slice(&id.to_bytes());
            decode_record_ref(&bytes)
                .map(LogicalImportIdentity::Record)
                .map_err(|_| CliError::invalid_request())
        }
        _ => Err(CliError::invalid_request()),
    }
}

fn parse_restore_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(mode) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::Restore));
    };
    if mode == "--help" || mode == "-h" || mode == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::Restore))
        } else {
            Err(CliError::invalid_request())
        };
    }
    if mode != "clone" {
        return Err(CliError::unsupported_operation());
    }
    let Some(backup_path) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    if !take_option(&mut arguments, "--authorize-with") {
        return Err(CliError::invalid_request());
    }
    let Some(authorization_project) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    if !take_option(&mut arguments, "--output") {
        return Err(CliError::invalid_request());
    }
    let Some(destination) = arguments.pop_front() else {
        return Err(CliError::invalid_request());
    };
    let (profile, audit_scope) = take_profile_and_scope(&mut arguments)?;
    if !arguments.is_empty() {
        return Err(CliError::invalid_request());
    }
    run_restore_clone(
        Path::new(&backup_path),
        Path::new(&authorization_project),
        Path::new(&destination),
        profile,
        audit_scope,
    )
}

fn parse_migration_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(action) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::Migration));
    };
    if action == "--help" || action == "-h" || action == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::Migration))
        } else {
            Err(CliError::invalid_request())
        };
    }
    let action = match action.to_str() {
        Some("plan") => MigrationCommandAction::Plan,
        Some("dry-run") => MigrationCommandAction::DryRun,
        Some("run") => MigrationCommandAction::Run,
        Some("resume") => MigrationCommandAction::Resume,
        Some(_) => return Err(CliError::unsupported_operation()),
        None => return Err(CliError::invalid_request()),
    };
    if arguments.len() == 1
        && arguments
            .front()
            .is_some_and(|argument| argument == "--help")
    {
        return Ok(Success::Help(HelpScope::Migration));
    }

    let database_path = arguments
        .pop_front()
        .map(PathBuf::from)
        .ok_or_else(CliError::invalid_request)?;
    if !take_option(&mut arguments, "--plan-file") {
        return Err(CliError::invalid_request());
    }
    let plan_path = arguments
        .pop_front()
        .map(PathBuf::from)
        .ok_or_else(CliError::invalid_request)?;
    let mut request = MigrationCommandRequest {
        action,
        database_path,
        plan_path,
        run_id: None,
        steps: Vec::new(),
        resolutions: Vec::new(),
        backup_path: None,
        restore_path: None,
        confirm_breaking: false,
    };

    while let Some(argument) = arguments.pop_front() {
        match argument.to_str() {
            Some("--run-id")
                if matches!(
                    action,
                    MigrationCommandAction::Run | MigrationCommandAction::Resume
                ) =>
            {
                let value = arguments
                    .pop_front()
                    .ok_or_else(CliError::invalid_request)?;
                if request.run_id.replace(parse_cli_id(value)?).is_some() {
                    return Err(CliError::invalid_request());
                }
            }
            Some("--step") if action != MigrationCommandAction::Plan => {
                let step_id = parse_cli_id(
                    arguments
                        .pop_front()
                        .ok_or_else(CliError::invalid_request)?,
                )?;
                let operation_id = if matches!(
                    action,
                    MigrationCommandAction::Run | MigrationCommandAction::Resume
                ) {
                    if !take_option(&mut arguments, "--operation-id") {
                        return Err(CliError::invalid_request());
                    }
                    Some(parse_cli_id(
                        arguments
                            .pop_front()
                            .ok_or_else(CliError::invalid_request)?,
                    )?)
                } else {
                    None
                };
                let mut record_files = Vec::new();
                while arguments
                    .front()
                    .is_some_and(|argument| argument == "--record")
                {
                    let _ = arguments.pop_front();
                    record_files.push(
                        arguments
                            .pop_front()
                            .map(PathBuf::from)
                            .ok_or_else(CliError::invalid_request)?,
                    );
                }
                request.steps.push(MigrationStepFiles {
                    step_id,
                    operation_id,
                    record_files,
                });
            }
            Some("--omit")
                if matches!(
                    action,
                    MigrationCommandAction::Run | MigrationCommandAction::Resume
                ) =>
            {
                let value = arguments
                    .pop_front()
                    .and_then(|value| value.into_string().ok())
                    .ok_or_else(CliError::invalid_request)?;
                let record_index = parse_canonical_u64(&value)?;
                request.resolutions.push(MigrationResolutionSpec {
                    record_index,
                    resolution: MigrationItemResolution::OmitRecord,
                });
            }
            Some("--replace")
                if matches!(
                    action,
                    MigrationCommandAction::Run | MigrationCommandAction::Resume
                ) =>
            {
                let value = arguments
                    .pop_front()
                    .and_then(|value| value.into_string().ok())
                    .ok_or_else(CliError::invalid_request)?;
                let record_index = parse_canonical_u64(&value)?;
                let frame_path = arguments
                    .pop_front()
                    .map(PathBuf::from)
                    .ok_or_else(CliError::invalid_request)?;
                let bytes = read_bounded_file(&frame_path, 16 * 1024 * 1024)?;
                request.resolutions.push(MigrationResolutionSpec {
                    record_index,
                    resolution: MigrationItemResolution::ReplaceRecord(bytes),
                });
            }
            Some("--backup")
                if matches!(
                    action,
                    MigrationCommandAction::Run | MigrationCommandAction::Resume
                ) =>
            {
                let path = arguments
                    .pop_front()
                    .map(PathBuf::from)
                    .ok_or_else(CliError::invalid_request)?;
                if request.backup_path.replace(path).is_some() {
                    return Err(CliError::invalid_request());
                }
            }
            Some("--restore")
                if matches!(
                    action,
                    MigrationCommandAction::Run | MigrationCommandAction::Resume
                ) =>
            {
                let path = arguments
                    .pop_front()
                    .map(PathBuf::from)
                    .ok_or_else(CliError::invalid_request)?;
                if request.restore_path.replace(path).is_some() {
                    return Err(CliError::invalid_request());
                }
            }
            Some("--confirm-breaking")
                if matches!(
                    action,
                    MigrationCommandAction::Run | MigrationCommandAction::Resume
                ) =>
            {
                if request.confirm_breaking {
                    return Err(CliError::invalid_request());
                }
                request.confirm_breaking = true;
            }
            _ => return Err(CliError::invalid_request()),
        }
    }

    if matches!(
        action,
        MigrationCommandAction::Run | MigrationCommandAction::Resume
    ) && request.run_id.is_none()
    {
        return Err(CliError::invalid_request());
    }
    if matches!(
        action,
        MigrationCommandAction::Plan | MigrationCommandAction::DryRun
    ) && (!request.resolutions.is_empty()
        || request.backup_path.is_some()
        || request.restore_path.is_some()
        || request.confirm_breaking)
    {
        return Err(CliError::invalid_request());
    }

    run_migration_command(request)
}

fn parse_storage_command(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let Some(action) = arguments.pop_front() else {
        return Ok(Success::Help(HelpScope::StorageUpgrade));
    };
    if action == "--help" || action == "-h" || action == "help" {
        return if arguments.is_empty() {
            Ok(Success::Help(HelpScope::StorageUpgrade))
        } else {
            Err(CliError::invalid_request())
        };
    }
    if action != "upgrade" {
        return Err(CliError::unsupported_operation());
    }
    if arguments.len() == 1
        && arguments
            .front()
            .is_some_and(|argument| argument == "--help")
    {
        return Ok(Success::Help(HelpScope::StorageUpgrade));
    }
    if arguments.len() == 1
        && arguments
            .front()
            .is_some_and(|argument| argument == "--help")
    {
        return Ok(Success::Help(HelpScope::StorageUpgrade));
    }
    let database_path = arguments
        .pop_front()
        .map(PathBuf::from)
        .ok_or_else(CliError::invalid_request)?;
    if !take_option(&mut arguments, "--backup") {
        return Err(CliError::invalid_request());
    }
    let backup_path = arguments
        .pop_front()
        .map(PathBuf::from)
        .ok_or_else(CliError::invalid_request)?;
    if !take_option(&mut arguments, "--restore") {
        return Err(CliError::invalid_request());
    }
    let restore_path = arguments
        .pop_front()
        .map(PathBuf::from)
        .ok_or_else(CliError::invalid_request)?;
    if arguments.pop_front().as_deref() != Some(OsString::from("--confirm").as_os_str())
        || !arguments.is_empty()
    {
        return Err(CliError::invalid_request());
    }
    run_storage_upgrade(&database_path, &backup_path, &restore_path)
}

fn parse_cli_id<T: FromStr>(value: OsString) -> Result<T, CliError> {
    value
        .into_string()
        .map_err(|_| CliError::invalid_request())?
        .parse()
        .map_err(|_| CliError::invalid_request())
}

fn parse_canonical_u64(value: &str) -> Result<u64, CliError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| CliError::invalid_request())?;
    if parsed.to_string() != value {
        return Err(CliError::invalid_request());
    }
    Ok(parsed)
}

fn run_migration_command(request: MigrationCommandRequest) -> Result<Success, CliError> {
    let plan = load_migration_plan(&request.plan_path)?;
    if request.action == MigrationCommandAction::Plan && !request.steps.is_empty() {
        return Err(CliError::invalid_request());
    }
    let execution = matches!(
        request.action,
        MigrationCommandAction::Run | MigrationCommandAction::Resume
    );
    let loaded_steps = if request.action == MigrationCommandAction::Plan {
        Vec::new()
    } else {
        load_migration_step_records(&plan, &request.steps, execution)?
    };

    let principal = current_host_principal()?;
    let project = open_current_policy_project(&request.database_path)?;
    let layout = project.layout.clone();
    let source_revision = project.manifest.revision();
    let database_id = layout
        .database_id()
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    let actual_schema_fingerprint = schema_fingerprint_from_manifest(&layout, &project.manifest)?;
    let policy_history = project.policy_history.policy().clone();
    drop(project);

    let policy = policy_history
        .select(AuthorizationMode::Now, principal, source_revision)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    let target = PolicyTarget::default();
    match request.action {
        MigrationCommandAction::Plan | MigrationCommandAction::DryRun => {
            authorize_cli_capability(policy, Capability::MigrationPlan, target)?;
            validate_migration_source(&plan, source_revision, actual_schema_fingerprint)?;
        }
        MigrationCommandAction::Run => {
            validate_migration_source(&plan, source_revision, actual_schema_fingerprint)?;
            authorize_cli_capability(policy, Capability::MigrationExecute, target)?;
        }
        MigrationCommandAction::Resume => {
            authorize_cli_capability(policy, Capability::MigrationExecute, target)?;
        }
    }

    if request.action == MigrationCommandAction::Plan {
        return Ok(Success::Migration(MigrationSummary {
            status: MigrationStatus::Planned,
            database_id,
            migration_id: plan.migration_id(),
            category: plan.category(),
            fingerprint: *plan.fingerprint().as_bytes(),
            source_revision: plan
                .source_schema_precondition()
                .revision()
                .revision()
                .value(),
            target_revision: plan.target_schema().revision().revision().value(),
            step_count: plan.steps().len(),
            input_record_count: 0,
            input_bytes: 0,
            estimate_output_bytes: None,
            error_count: 0,
            unresolved_count: 0,
            completed_step_count: 0,
            final_revision: None,
        }));
    }

    let mut step_metadata = Vec::new();
    step_metadata
        .try_reserve_exact(loaded_steps.len())
        .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
    let mut flat_records = Vec::new();
    for step in loaded_steps {
        step_metadata.push((step.step_id, step.operation_id, step.records.len()));
        flat_records.extend(step.records);
    }
    let step_count = step_metadata.len();
    let source_schema_revision = plan.source_schema_precondition().revision();
    let report = MigrationDryRun::run(
        &plan,
        source_schema_revision,
        *plan.source_schema_precondition().fingerprint(),
        &flat_records,
    );

    if request.action == MigrationCommandAction::DryRun {
        return Ok(Success::Migration(migration_summary_from_report(
            MigrationStatus::Previewed,
            database_id,
            &plan,
            &report,
            0,
            None,
        )));
    }

    let mut decisions = Vec::new();
    for specification in request.resolutions {
        let item = report
            .unresolved_items()
            .iter()
            .copied()
            .find(|item| item.record_index() == specification.record_index)
            .ok_or_else(CliError::invalid_request)?;
        decisions.push(
            MigrationAdminDecision::for_item(&report, item, specification.resolution)
                .map_err(|_| CliError::invalid_request())?,
        );
    }
    let validated_decisions = ValidatedMigrationDecisions::validate(
        &plan,
        &report,
        decisions,
        policy.current_snapshot(),
        principal,
        target,
    )
    .map_err(|error| match error {
        worlddb_core::MigrationDecisionError::Unauthorized => {
            CliError::new(PublicCode::UNAUTHORIZED)
        }
        _ => CliError::new(PublicCode::CORRUPT_DATA),
    })?;

    let run_id = request.run_id.ok_or_else(CliError::invalid_request)?;
    if request.action == MigrationCommandAction::Resume {
        validate_resumable_migration_journal(&layout, run_id)?;
    }

    let safe_restore_point = if plan.category() == MigrationCategory::Breaking {
        if !request.confirm_breaking {
            return Err(CliError::invalid_request());
        }
        let backup = request
            .backup_path
            .as_deref()
            .ok_or_else(CliError::invalid_request)?;
        let restore = request
            .restore_path
            .as_deref()
            .ok_or_else(CliError::invalid_request)?;
        reject_restore_target_within_project(backup, layout.root())?;
        reject_restore_target_within_project(restore, layout.root())?;
        reject_overlapping_targets(backup, restore)?;
        let manager = ExactBackupManager::new(layout.clone());
        let proof = if request.action == MigrationCommandAction::Resume {
            manager.create_migration_safe_restore_point_from_backup(
                &plan, backup, restore, None, policy, target,
            )
        } else {
            manager
                .create_migration_safe_restore_point(&plan, backup, restore, None, policy, target)
        }
        .map_err(map_migration_restore_point_error)?;
        Some(proof)
    } else {
        if request.confirm_breaking
            || request.backup_path.is_some()
            || request.restore_path.is_some()
        {
            return Err(CliError::invalid_request());
        }
        None
    };

    let root = layout.root().to_path_buf();
    let layout = DatabaseLayout::open(root).map_err(map_storage_file_error)?;
    let writer_lock = layout.try_writer_lock().map_err(map_writer_lock_error)?;
    let verification = StorageVerifier::new(layout.clone())
        .verify(&writer_lock)
        .map_err(map_storage_verify_error)?;
    if !verification.is_clean() {
        return Err(CliError::new(PublicCode::STORAGE_READ));
    }
    let mut run = FileStoreGuardedMigrationRun::open(layout.clone(), &writer_lock)
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    let journal = run
        .load_journal_status(run_id)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    match (request.action, journal.as_ref().map(|entry| entry.state())) {
        (MigrationCommandAction::Run, Some(_))
        | (MigrationCommandAction::Resume, Some(MigrationRunJournalState::Completed)) => {
            return Err(CliError::invalid_request());
        }
        (MigrationCommandAction::Resume, None) => {
            return Err(CliError::new(PublicCode::NOT_FOUND));
        }
        _ => {}
    }

    let manifest = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    let locked_policy_history = load_policy_history(&layout, &manifest)?;
    let locked_policy = locked_policy_history
        .policy()
        .select(AuthorizationMode::Now, principal, manifest.revision())
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    authorize_cli_capability(locked_policy, Capability::MigrationExecute, target)?;

    let execution_source_fingerprint = if request.action == MigrationCommandAction::Resume {
        *plan.source_schema_precondition().fingerprint()
    } else {
        let actual = schema_fingerprint_from_run(&run, run.latest_published())?;
        validate_migration_source(&plan, run.latest_published(), actual)?;
        actual
    };
    let locked_database_id = layout
        .database_id()
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    if locked_database_id != database_id {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    let transformer = MigrationTransformer::for_version(plan.transformer_version())
        .map_err(|_| CliError::unsupported_operation())?;
    let mut inputs = Vec::new();
    inputs
        .try_reserve_exact(step_count)
        .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
    let mut flat_records = flat_records.into_iter();
    for (step_id, operation_id, record_count) in step_metadata {
        let operation_id = operation_id.ok_or_else(CliError::invalid_request)?;
        let records = flat_records.by_ref().take(record_count).collect();
        inputs.push(MigrationStepInput::new(step_id, operation_id, records));
    }

    let restore_ref = safe_restore_point.as_ref();
    let admin_action = if let Some(restore_point) = restore_ref {
        Some(
            BreakingMigrationAdminAction::confirm(
                &plan,
                locked_database_id,
                &execution_source_fingerprint,
                principal,
                locked_policy.current_snapshot(),
                target,
                restore_point,
            )
            .map_err(|_| CliError::new(PublicCode::UNAUTHORIZED))?,
        )
    } else {
        None
    };
    let required_audit_records = migration_audit_records(
        &inputs,
        MigrationAuditContext {
            plan: &plan,
            actor: principal,
            policy: locked_policy.current_snapshot(),
            epoch: locked_policy.current_epoch(),
            target,
            layout: &layout,
            writer_lock: &writer_lock,
        },
    )?;

    let result = run
        .execute(
            &plan,
            run_id,
            database_id,
            execution_source_fingerprint,
            transformer,
            inputs,
            validated_decisions,
            principal,
            locked_policy.current_snapshot(),
            target,
            locked_policy.current_epoch(),
            restore_ref,
            admin_action.as_ref(),
            required_audit_records,
            |backend, base_revision, entries, target_schema| {
                let actual =
                    schema_fingerprint_after_migration_step(backend, base_revision, entries)?;
                if actual == *target_schema.schema().fingerprint() {
                    Ok(())
                } else {
                    Err(CliError::new(PublicCode::CORRUPT_DATA))
                }
            },
        )
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;

    Ok(Success::Migration(migration_summary_from_report(
        if request.action == MigrationCommandAction::Resume {
            MigrationStatus::Resumed
        } else {
            MigrationStatus::Completed
        },
        locked_database_id,
        &plan,
        &report,
        result.completed_steps().len(),
        Some(result.final_revision().value()),
    )))
}

fn validate_resumable_migration_journal(
    layout: &DatabaseLayout,
    run_id: MigrationRunId,
) -> Result<(), CliError> {
    let writer_lock = layout.try_writer_lock().map_err(map_writer_lock_error)?;
    let verification = StorageVerifier::new(layout.clone())
        .verify(&writer_lock)
        .map_err(map_storage_verify_error)?;
    if !verification.is_clean() {
        return Err(CliError::new(PublicCode::STORAGE_READ));
    }
    let run = FileStoreGuardedMigrationRun::open(layout.clone(), &writer_lock)
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    match run
        .load_journal_status(run_id)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?
    {
        Some(snapshot) if snapshot.state() == MigrationRunJournalState::Running => Ok(()),
        Some(_) => Err(CliError::invalid_request()),
        None => Err(CliError::new(PublicCode::NOT_FOUND)),
    }
}

fn load_migration_plan(path: &Path) -> Result<MigrationPlan, CliError> {
    const MAX_MIGRATION_PLAN_BYTES: u64 = 16 * 1024 * 1024;
    let bytes = read_bounded_file(path, MAX_MIGRATION_PLAN_BYTES)?;
    let record = decode_record(&bytes)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?
        .into_record();
    match record {
        Record::MigrationPlan(plan) => Ok(plan),
        _ => Err(CliError::invalid_request()),
    }
}

fn load_migration_step_records(
    plan: &MigrationPlan,
    step_files: &[MigrationStepFiles],
    execution: bool,
) -> Result<Vec<LoadedMigrationStep>, CliError> {
    if step_files.len() != plan.steps().len() || plan.step_targets().is_none() {
        return Err(CliError::invalid_request());
    }
    let mut total_bytes = 0_u64;
    let mut total_records = 0_u64;
    let mut loaded = Vec::new();
    loaded
        .try_reserve_exact(step_files.len())
        .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
    for (planned_step, supplied) in plan.steps().iter().zip(step_files) {
        if *planned_step != supplied.step_id || supplied.operation_id.is_some() != execution {
            return Err(CliError::invalid_request());
        }
        let mut records = Vec::new();
        records
            .try_reserve_exact(supplied.record_files.len())
            .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
        for path in &supplied.record_files {
            let remaining = plan
                .budget()
                .max_memory_bytes()
                .checked_sub(total_bytes)
                .ok_or_else(|| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
            let bytes = read_bounded_file(path, remaining)?;
            total_bytes = total_bytes
                .checked_add(
                    u64::try_from(bytes.len())
                        .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?,
                )
                .ok_or_else(|| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
            total_records = total_records
                .checked_add(1)
                .ok_or_else(|| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
            if total_records > plan.budget().max_work_units() {
                return Err(CliError::new(PublicCode::BUDGET_EXCEEDED));
            }
            records.push(bytes);
        }
        loaded.push(LoadedMigrationStep {
            step_id: supplied.step_id,
            operation_id: supplied.operation_id,
            records,
        });
    }
    Ok(loaded)
}

fn validate_migration_source(
    plan: &MigrationPlan,
    revision: Revision,
    schema_fingerprint: [u8; 32],
) -> Result<(), CliError> {
    let actual_revision = SchemaRevision::from_published_revision(revision);
    plan.validate_source_schema(actual_revision, schema_fingerprint)
        .map_err(|_| CliError::new(PublicCode::CURSOR_INVALIDATED))
}

fn migration_summary_from_report(
    status: MigrationStatus,
    database_id: DatabaseId,
    plan: &MigrationPlan,
    report: &worlddb_core::MigrationDryRunReport,
    completed_step_count: usize,
    final_revision: Option<u64>,
) -> MigrationSummary {
    MigrationSummary {
        status,
        database_id,
        migration_id: plan.migration_id(),
        category: plan.category(),
        fingerprint: *plan.fingerprint().as_bytes(),
        source_revision: report.source_revision().revision().value(),
        target_revision: plan.target_schema().revision().revision().value(),
        step_count: plan.steps().len(),
        input_record_count: report.source_record_count().unwrap_or(0),
        input_bytes: report.source_bytes().unwrap_or(0),
        estimate_output_bytes: report.output().map(|output| output.encoded_bytes()),
        error_count: report.error_count(),
        unresolved_count: report.unresolved_items().len(),
        completed_step_count,
        final_revision,
    }
}

fn load_policy_history(
    layout: &DatabaseLayout,
    manifest: &Manifest,
) -> Result<SecurityPolicyHistorySnapshot, CliError> {
    let segment_ids = manifest
        .segments()
        .iter()
        .filter(|segment| segment.kind() == ManifestSegmentKind::SecurityPolicy)
        .map(|segment| segment.id())
        .collect::<Vec<_>>();
    if segment_ids.is_empty() {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    SecurityPolicyHistoryStore::new(layout.clone())
        .load_history(manifest.revision(), &segment_ids)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))
}

fn schema_fingerprint_from_manifest(
    layout: &DatabaseLayout,
    manifest: &Manifest,
) -> Result<[u8; 32], CliError> {
    let store = HistorySegmentStore::new(layout.clone());
    let mut accumulator = SchemaHistoryAccumulator::new();
    for reference in manifest
        .segments()
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::History)
    {
        let segment = store
            .read_segment(reference.id())
            .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
        if segment.content_digest() != reference.content_digest() {
            return Err(CliError::new(PublicCode::CORRUPT_DATA));
        }
        for decoded in segment.records() {
            accumulator.push(decoded.record())?;
        }
    }
    accumulator.fingerprint(manifest.revision())
}

fn schema_fingerprint_from_run(
    run: &FileStoreGuardedMigrationRun<'_>,
    revision: Revision,
) -> Result<[u8; 32], CliError> {
    let reader = run
        .read_at(revision)
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    let mut accumulator = SchemaHistoryAccumulator::new();
    for (_, record) in reader {
        accumulator.push(record)?;
    }
    accumulator.fingerprint(revision)
}

fn schema_fingerprint_after_migration_step(
    run: &FileStoreGuardedMigrationRun<'_>,
    base_revision: Revision,
    staged_records: &[Record],
) -> Result<[u8; 32], CliError> {
    let reader = run
        .read_at(base_revision)
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    let mut accumulator = SchemaHistoryAccumulator::new();
    for (_, record) in reader {
        accumulator.push(record)?;
    }
    for record in staged_records {
        accumulator.push(record)?;
    }
    let target_revision = base_revision
        .next_commit()
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    accumulator.fingerprint(target_revision)
}

#[derive(Default)]
struct SchemaHistoryAccumulator {
    genesis: Vec<SchemaDefinition>,
    batches: BTreeMap<Revision, Vec<SchemaDefinition>>,
}

impl SchemaHistoryAccumulator {
    fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, record: &Record) -> Result<(), CliError> {
        let definition = match record {
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
            _ => None,
        };
        if let Some((revision, definition)) = definition {
            if revision == Revision::GENESIS {
                self.genesis
                    .try_reserve(1)
                    .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
                self.genesis.push(definition);
            } else {
                let batch = self.batches.entry(revision).or_default();
                batch
                    .try_reserve(1)
                    .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
                batch.push(definition);
            }
        }
        Ok(())
    }

    fn fingerprint(self, _head: Revision) -> Result<[u8; 32], CliError> {
        let mut model = if self.genesis.is_empty() {
            SchemaHistoryReferenceModel::new()
        } else {
            SchemaHistoryReferenceModel::with_genesis(self.genesis)
                .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?
        };
        for (revision, definitions) in self.batches {
            model
                .publish(revision, definitions)
                .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
        }
        model
            .schema_at(SchemaMode::Current, _head)
            .map(|snapshot| snapshot.fingerprint())
            .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))
    }
}

fn migration_audit_records(
    inputs: &[MigrationStepInput],
    context: MigrationAuditContext<'_>,
) -> Result<Vec<AuditRecord>, CliError> {
    let MigrationAuditContext {
        plan,
        actor,
        policy,
        epoch,
        target,
        layout,
        writer_lock,
    } = context;
    let committed = WalPrepareLog::new(layout)
        .committed_required_audit_records(writer_lock)
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    let operation_ids = inputs
        .iter()
        .map(MigrationStepInput::operation_id)
        .collect::<std::collections::BTreeSet<_>>();
    if operation_ids.len() != inputs.len() {
        return Err(CliError::invalid_request());
    }
    let mut existing = BTreeMap::new();
    let mut highest_sequence = 0_u64;
    for entry in committed {
        highest_sequence = highest_sequence.max(entry.record().sequence().value());
        if operation_ids.contains(&entry.operation_id())
            && existing
                .insert(entry.operation_id(), entry.record().clone())
                .is_some()
        {
            return Err(CliError::new(PublicCode::CORRUPT_DATA));
        }
    }

    let rights = policy.effective_capability_fingerprint(actor, target);
    let policy_fingerprint = AuditPolicyFingerprint::new(Bytes::new(rights.to_vec()))
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    let targets = plan
        .step_targets()
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    if inputs.len() != targets.len() {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(inputs.len())
        .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
    for (input, step_target) in inputs.iter().zip(targets) {
        if let Some(record) = existing.remove(&input.operation_id()) {
            output.push(record);
            continue;
        }
        highest_sequence = highest_sequence
            .checked_add(1)
            .ok_or_else(|| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
        let record_id = worlddb_core::storage_internal::generate_migration_audit_record_id()
            .map_err(|_| CliError::new(PublicCode::INTERNAL))?;
        let audit_operation_id =
            worlddb_core::storage_internal::generate_migration_audit_operation_id()
                .map_err(|_| CliError::new(PublicCode::INTERNAL))?;
        output.push(AuditRecord::new(
            AuditRecordIdentity {
                record_id,
                sequence: AuditSequence::new(highest_sequence),
                audit_operation_id,
            },
            AuditRecordDetails {
                actor,
                action: AuditAction::Migration,
                object_class: AuditObjectClass::Migration,
                outcome: AuditOutcome::Succeeded,
                commit_context: AuditCommitContext::Committed {
                    revision: step_target.schema().revision().revision(),
                    operation_id: input.operation_id(),
                },
                security_epoch: epoch,
                policy_fingerprint: policy_fingerprint.clone(),
            },
        ));
    }
    Ok(output)
}

fn run_storage_upgrade(
    database_path: &Path,
    backup_path: &Path,
    restore_path: &Path,
) -> Result<Success, CliError> {
    let principal = current_host_principal()?;
    let project = open_current_policy_project(database_path)?;
    let layout = project.layout.clone();
    let policy_revision = project.manifest.revision();
    let policy_history = project.policy_history.policy().clone();
    drop(project);
    let policy = policy_history
        .select(AuthorizationMode::Now, principal, policy_revision)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    let target = PolicyTarget::default();
    authorize_cli_capability(policy, Capability::StorageFormatUpgrade, target)?;
    authorize_cli_capability(policy, Capability::BackupCreate, target)?;
    authorize_cli_capability(policy, Capability::BackupRestore, target)?;
    reject_restore_target_within_project(backup_path, layout.root())?;
    reject_restore_target_within_project(restore_path, layout.root())?;
    reject_overlapping_targets(backup_path, restore_path)?;

    let manager = StorageUpgradeManager::new();
    let plan = manager
        .prepare(&layout, StorageUpgradeBudget::current_pointer_v1_to_v2())
        .map_err(map_storage_upgrade_error)?;
    let restore_point = manager
        .create_safe_restore_point(
            &layout,
            &plan,
            StorageUpgradeRestoreTargets::new(backup_path, restore_path),
            None,
            policy,
            target,
        )
        .map_err(map_storage_upgrade_error)?;
    let action = manager
        .confirm(&plan, &restore_point, policy, target)
        .map_err(map_storage_upgrade_error)?;
    let receipt = manager
        .execute(&layout, &plan, &restore_point, &action, policy, target)
        .map_err(map_storage_upgrade_error)?;
    Ok(Success::StorageUpgrade(StorageUpgradeSummary {
        plan_id: receipt.plan_id(),
        run_id: receipt.run_id(),
        source_revision: receipt.source_revision().value(),
        target_fingerprint: *receipt.target_profile_fingerprint(),
        pointer_digest: *receipt.current_pointer_digest(),
        resumed: receipt.resumed(),
    }))
}

fn reject_overlapping_targets(first: &Path, second: &Path) -> Result<(), CliError> {
    let first = canonical_target_candidate(first)?;
    let second = canonical_target_candidate(second)?;
    if first.starts_with(&second) || second.starts_with(&first) {
        return Err(CliError::invalid_request());
    }
    Ok(())
}

fn canonical_target_candidate(path: &Path) -> Result<PathBuf, CliError> {
    let file_name = path.file_name().ok_or_else(CliError::invalid_request)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    Ok(parent.join(file_name))
}

fn map_migration_restore_point_error(error: MigrationRestorePointError) -> CliError {
    match error {
        MigrationRestorePointError::AuthorizationDenied { .. } => {
            CliError::new(PublicCode::UNAUTHORIZED)
        }
        MigrationRestorePointError::Backup(error) => map_backup_error(error),
        MigrationRestorePointError::Restore(error) => map_restore_error(error),
        MigrationRestorePointError::PlanNotBreaking
        | MigrationRestorePointError::SourceBindingMismatch
        | MigrationRestorePointError::RestoreBindingMismatch
        | MigrationRestorePointError::RestoredLayoutMismatch
        | MigrationRestorePointError::Proof(_) => CliError::new(PublicCode::CORRUPT_DATA),
        MigrationRestorePointError::Io { .. } | MigrationRestorePointError::StorageFile(_) => {
            CliError::new(PublicCode::STORAGE_READ)
        }
        MigrationRestorePointError::AuditFingerprint(_) => CliError::new(PublicCode::CORRUPT_DATA),
    }
}

fn map_storage_upgrade_error(error: worlddb_storage_file::StorageUpgradeError) -> CliError {
    match error {
        worlddb_storage_file::StorageUpgradeError::AuthorizationDenied { .. }
        | worlddb_storage_file::StorageUpgradeError::AuthorizationChanged
        | worlddb_storage_file::StorageUpgradeError::ActionBindingMismatch => {
            CliError::new(PublicCode::UNAUTHORIZED)
        }
        worlddb_storage_file::StorageUpgradeError::Backup(error) => map_backup_error(error),
        worlddb_storage_file::StorageUpgradeError::Restore(error) => map_restore_error(error),
        worlddb_storage_file::StorageUpgradeError::WriterLock(error) => {
            map_writer_lock_error(error)
        }
        worlddb_storage_file::StorageUpgradeError::StorageFile(error) => {
            map_storage_file_error(error)
        }
        worlddb_storage_file::StorageUpgradeError::StorageVerify(error) => {
            map_storage_verify_error(error)
        }
        worlddb_storage_file::StorageUpgradeError::Io { .. }
        | worlddb_storage_file::StorageUpgradeError::Manifest(_)
        | worlddb_storage_file::StorageUpgradeError::Wal(_) => {
            CliError::new(PublicCode::STORAGE_READ)
        }
        worlddb_storage_file::StorageUpgradeError::UnsupportedSourceProfile => {
            CliError::unsupported_operation()
        }
        worlddb_storage_file::StorageUpgradeError::SourceBindingMismatch => {
            CliError::new(PublicCode::CURSOR_INVALIDATED)
        }
        worlddb_storage_file::StorageUpgradeError::SourceNotClean
        | worlddb_storage_file::StorageUpgradeError::RestoreBindingMismatch
        | worlddb_storage_file::StorageUpgradeError::InvalidJournal => {
            CliError::new(PublicCode::CORRUPT_DATA)
        }
        worlddb_storage_file::StorageUpgradeError::InvalidBudget
        | worlddb_storage_file::StorageUpgradeError::JournalMissing
        | worlddb_storage_file::StorageUpgradeError::BudgetExceeded => {
            CliError::new(PublicCode::BUDGET_EXCEEDED)
        }
        worlddb_storage_file::StorageUpgradeError::IdGeneration(_) => {
            CliError::new(PublicCode::INTERNAL)
        }
        worlddb_storage_file::StorageUpgradeError::InjectedFailure { .. } => {
            CliError::new(PublicCode::CANCELLED)
        }
    }
}

fn take_option(arguments: &mut VecDeque<OsString>, option: &str) -> bool {
    arguments.pop_front().as_deref() == Some(OsString::from(option).as_os_str())
}

fn take_profile_and_scope(
    arguments: &mut VecDeque<OsString>,
) -> Result<(BackupProfile, BackupAuditScope), CliError> {
    if !take_option(arguments, "--profile") {
        return Err(CliError::invalid_request());
    }
    let profile = arguments
        .pop_front()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(CliError::invalid_request)?;
    if !take_option(arguments, "--audit-scope") {
        return Err(CliError::invalid_request());
    }
    let scope = arguments
        .pop_front()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(CliError::invalid_request)?;

    match (profile.as_str(), scope.as_str()) {
        ("exact", "excluded") => Ok((BackupProfile::ExactDatabase, BackupAuditScope::Excluded)),
        ("audit-complete", "included") => {
            Ok((BackupProfile::AuditComplete, BackupAuditScope::Included))
        }
        _ => Err(CliError::invalid_request()),
    }
}

fn run_backup_create(
    source: &Path,
    target: &Path,
    profile: BackupProfile,
    audit_scope: BackupAuditScope,
) -> Result<Success, CliError> {
    let principal = current_host_principal()?;
    let source = canonical_project_root(source)?;
    let layout = DatabaseLayout::open(&source).map_err(map_storage_file_error)?;
    let manager = ExactBackupManager::new(layout);
    let verification = match profile {
        BackupProfile::ExactDatabase => {
            manager.create_exact_backup_authorized(target, None, principal, PolicyTarget::default())
        }
        BackupProfile::AuditComplete => manager.create_audit_complete_backup_authorized(
            target,
            None,
            principal,
            PolicyTarget::default(),
        ),
    }
    .map_err(map_backup_error)?;
    if verification.profile() != profile {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    Ok(Success::Backup {
        action: BackupAction::Created,
        profile,
        audit_scope,
        summary: BackupSummary::from_verification(&verification),
    })
}

fn current_host_principal() -> Result<PrincipalId, CliError> {
    let identity = worlddb_process_adapter::current_process_identity_bytes()
        .map_err(|_| CliError::new(PublicCode::UNAUTHORIZED))?;
    worlddb_core::derive_host_account_principal(&identity)
        .map_err(|_| CliError::new(PublicCode::UNAUTHORIZED))
}

fn canonical_project_root(path: &Path) -> Result<PathBuf, CliError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    fs::canonicalize(path).map_err(|_| CliError::new(PublicCode::STORAGE_READ))
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn run_backup_verify(
    backup: &Path,
    profile: BackupProfile,
    audit_scope: BackupAuditScope,
) -> Result<Success, CliError> {
    let verification = match profile {
        BackupProfile::ExactDatabase => verify_exact_backup(backup, None),
        BackupProfile::AuditComplete => verify_audit_complete_backup(backup, None),
    }
    .map_err(map_backup_error)?;
    if verification.profile() != profile {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    Ok(Success::Backup {
        action: BackupAction::Verified,
        profile,
        audit_scope,
        summary: BackupSummary::from_verification(&verification),
    })
}

fn run_restore_clone(
    backup: &Path,
    authorization_project: &Path,
    destination: &Path,
    profile: BackupProfile,
    audit_scope: BackupAuditScope,
) -> Result<Success, CliError> {
    let principal = current_host_principal()?;
    let authorization_project = open_current_policy_project(authorization_project)?;
    let policy_view = authorization_project
        .policy_history
        .policy()
        .select(
            AuthorizationMode::Now,
            principal,
            authorization_project.manifest.revision(),
        )
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    let policy_target = PolicyTarget::default();
    for capability in [Capability::ProjectRead, Capability::BackupRestore] {
        authorize_cli_capability(policy_view, capability, policy_target)?;
    }
    if profile == BackupProfile::AuditComplete {
        for capability in [Capability::AuditRead, Capability::AuditExport] {
            authorize_cli_capability(policy_view, capability, policy_target)?;
        }
    }
    reject_restore_target_within_project(destination, authorization_project.layout.root())?;

    let verification = match profile {
        BackupProfile::ExactDatabase => verify_exact_backup(backup, None),
        BackupProfile::AuditComplete => verify_audit_complete_backup(backup, None),
    }
    .map_err(map_backup_error)?;
    if verification.profile() != profile {
        return Err(CliError::invalid_request());
    }
    let authorization_database_id = authorization_project
        .layout
        .database_id()
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    if verification.database_id() != authorization_database_id {
        return Err(CliError::new(PublicCode::UNAUTHORIZED));
    }

    let fingerprint = AuditPolicyFingerprint::new(Bytes::new(
        policy_view
            .current_snapshot()
            .effective_capability_fingerprint(principal, policy_target)
            .to_vec(),
    ))
    .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    let restore = RestoreManager::new()
        .restore_clone(
            backup,
            destination,
            None,
            policy_view,
            policy_target,
            fingerprint,
        )
        .map_err(map_restore_error)?;
    if restore.profile() != profile
        || restore.source_database_id() != verification.database_id()
        || restore.source_revision() != verification.revision()
        || restore.audit_safe_sequence() != verification.audit_safe_sequence()
    {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    Ok(Success::Restore {
        profile,
        audit_scope,
        summary: RestoreSummary {
            source_database_id: restore.source_database_id(),
            source_revision: restore.source_revision().value(),
            restored_database_id: restore.restored_database_id(),
            restored_revision: restore.restored_revision().value(),
            item_count: verification.item_count(),
            audit_safe_sequence: restore
                .audit_safe_sequence()
                .map(|sequence| sequence.value()),
            authenticity: AuthenticityLabel::from_authenticity(restore.source_authenticity()),
        },
    })
}

fn run_export_command(request: ExportCommandRequest) -> Result<Success, CliError> {
    let logical_scope = LogicalExportScope::new(
        request.from_revision,
        request.through_revision,
        request.history_spaces.clone(),
        request.record_kinds.clone(),
    )
    .map_err(map_logical_export_error)?;
    let principal = current_host_principal()?;
    let project = open_current_policy_project(&request.database_path)?;
    let layout = project.layout.clone();
    let policy_revision = project.manifest.revision();
    let database_id = layout
        .database_id()
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    let policy_history = project.policy_history.policy().clone();
    drop(project);
    reject_restore_target_within_project(&request.output_path, layout.root())?;
    ensure_new_output_target(&request.output_path)?;
    let policy = policy_history
        .select(AuthorizationMode::Now, principal, policy_revision)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;

    if request.kind.is_sharing() {
        let scope = SharingExportScope::new(
            request.from_revision,
            request.through_revision,
            request.history_spaces,
            request.record_kinds,
        )
        .map_err(map_sharing_export_error)?;
        let export = SharingExportManager::new(layout)
            .export(scope.clone(), policy)
            .map_err(map_sharing_export_error)?;
        let bytes = export.encode().map_err(map_sharing_export_error)?;
        write_new_output(&request.output_path, &bytes)?;
        return Ok(Success::Export(ExportSummary {
            sharing: true,
            database_id: None,
            snapshot_revision: None,
            from_revision: export.from_revision().value(),
            through_revision: export.through_revision().value(),
            history_space_count: export.history_spaces().len(),
            record_kind_count: scope.record_kinds().len(),
            selected_history_spaces: scope
                .history_spaces()
                .iter()
                .map(|id| id.to_canonical_string())
                .collect(),
            selected_record_kinds: scope
                .record_kinds()
                .iter()
                .map(|kind| record_kind_label(*kind).to_owned())
                .collect(),
            record_count: export.records().len(),
            omitted_record_class_count: 0,
            omitted_storage_class_count: 0,
        }));
    }

    let export = LogicalExportManager::new(layout)
        .export(logical_scope, policy)
        .map_err(map_logical_export_error)?;
    let bytes = export.encode().map_err(map_logical_export_error)?;
    let manifest = export.manifest();
    let summary = ExportSummary {
        sharing: false,
        database_id: Some(manifest.database_id()),
        snapshot_revision: Some(manifest.snapshot_revision().value()),
        from_revision: manifest.from_revision().value(),
        through_revision: manifest.through_revision().value(),
        history_space_count: manifest.selected_history_spaces().len(),
        record_kind_count: manifest.selected_record_kinds().len(),
        selected_history_spaces: manifest
            .selected_history_spaces()
            .iter()
            .map(|id| id.to_canonical_string())
            .collect(),
        selected_record_kinds: manifest
            .selected_record_kinds()
            .iter()
            .map(|kind| record_kind_label(*kind).to_owned())
            .collect(),
        record_count: export.records().len(),
        omitted_record_class_count: manifest
            .classes()
            .iter()
            .filter(|entry| !entry.selected())
            .count(),
        omitted_storage_class_count: manifest.omitted_storage_classes().len(),
    };
    if manifest.database_id() != database_id {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    write_new_output(&request.output_path, &bytes)?;
    Ok(Success::Export(summary))
}

fn run_import_plan(
    destination_path: &Path,
    input_path: &Path,
    output_path: &Path,
    mappings: Vec<LogicalImportIdMapping>,
) -> Result<Success, CliError> {
    let principal = current_host_principal()?;
    let project = open_current_policy_project(destination_path)?;
    let policy = project
        .policy_history
        .policy()
        .select(
            AuthorizationMode::Now,
            principal,
            project.manifest.revision(),
        )
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    authorize_cli_capability(policy, Capability::ProjectRead, PolicyTarget::default())?;
    authorize_cli_capability(policy, Capability::DataImport, PolicyTarget::default())?;
    let destination_database_id = project
        .layout
        .database_id()
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    reject_restore_target_within_project(output_path, project.layout.root())?;
    ensure_new_output_target(output_path)?;
    drop(project);

    let source_artifact = read_bounded_file(input_path, LOGICAL_ARTIFACT_MAX_BYTES)?;
    let plan = LogicalImportPlan::new(&source_artifact, destination_database_id, mappings)
        .map_err(map_logical_import_error)?;
    let mapping_count = plan.mappings().len();
    let source_database_id = plan.source_database_id();
    let plan_bytes = plan.encode().map_err(map_logical_import_error)?;
    let plan_digest = *blake3::hash(&plan_bytes).as_bytes();
    write_new_output(output_path, &plan_bytes)?;
    Ok(Success::ImportPlan(ImportPlanSummary {
        source_database_id,
        destination_database_id,
        mapping_count,
        plan_digest,
    }))
}

fn run_import_prepare(
    destination_path: &Path,
    input_path: &Path,
    plan_path: &Path,
) -> Result<Success, CliError> {
    let principal = current_host_principal()?;
    let project = open_current_policy_project(destination_path)?;
    let destination_database_id = project
        .layout
        .database_id()
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    let policy = project
        .policy_history
        .policy()
        .select(
            AuthorizationMode::Now,
            principal,
            project.manifest.revision(),
        )
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    authorize_cli_capability(policy, Capability::ProjectRead, PolicyTarget::default())?;
    authorize_cli_capability(policy, Capability::DataImport, PolicyTarget::default())?;

    let source_artifact = read_bounded_file(input_path, LOGICAL_ARTIFACT_MAX_BYTES)?;
    let plan_bytes = read_bounded_file(plan_path, LOGICAL_IMPORT_PLAN_MAX_BYTES)?;
    let _source = LogicalExport::decode(&source_artifact)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    let _plan = LogicalImportPlan::decode(&plan_bytes).map_err(map_logical_import_error)?;
    let store = HistorySegmentStore::new(project.layout.clone());
    let mut identities = std::collections::BTreeSet::new();
    for reference in project
        .manifest
        .segments()
        .iter()
        .filter(|reference| reference.kind() == ManifestSegmentKind::History)
    {
        let segment = store
            .read_segment(reference.id())
            .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
        if segment.content_digest() != reference.content_digest() {
            return Err(CliError::new(PublicCode::CORRUPT_DATA));
        }
        for decoded in segment.records() {
            identities.extend(LogicalImportIdentity::defined_by_record(decoded.record()));
            if identities.len() > LOGICAL_IMPORT_MAX_INVENTORY_IDENTITIES {
                return Err(CliError::new(PublicCode::BUDGET_EXCEEDED));
            }
        }
    }
    let inventory = LogicalImportDestinationInventory::new(destination_database_id, identities)
        .map_err(map_logical_import_error)?;
    let prepared = LogicalImportManager::prepare(&source_artifact, &plan_bytes, &inventory)
        .map_err(map_logical_import_error)?;
    let manifest = prepared.source().manifest();
    let summary = ImportPrepareSummary {
        source_database_id: manifest.database_id(),
        destination_database_id,
        stream_fingerprint: prepared.stream_fingerprint(),
        mapping_count: prepared.plan().mappings().len(),
        record_count: prepared.records().len(),
        from_revision: manifest.from_revision().value(),
        through_revision: manifest.through_revision().value(),
        history_space_count: manifest.selected_history_spaces().len(),
        record_kind_count: manifest.selected_record_kinds().len(),
        omitted_record_class_count: manifest
            .classes()
            .iter()
            .filter(|entry| !entry.selected())
            .count(),
        omitted_storage_class_count: manifest.omitted_storage_classes().len(),
    };
    drop(project);
    Ok(Success::ImportPrepared(summary))
}

struct CurrentPolicyProject {
    layout: DatabaseLayout,
    _lock: WriterLock,
    manifest: Manifest,
    policy_history: SecurityPolicyHistorySnapshot,
}

fn open_current_policy_project(path: &Path) -> Result<CurrentPolicyProject, CliError> {
    let root = canonical_project_root(path)?;
    let layout = DatabaseLayout::open(root).map_err(map_storage_file_error)?;
    let lock = layout.try_read_only_lock().map_err(map_writer_lock_error)?;
    let report = StorageVerifier::new(layout.clone())
        .verify(&lock)
        .map_err(map_storage_verify_error)?;
    if !report.is_clean() {
        return Err(CliError::new(PublicCode::STORAGE_READ));
    }
    let manifest = ManifestStore::new(layout.clone())
        .read_current()
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?
        .ok_or_else(|| CliError::new(PublicCode::CORRUPT_DATA))?;
    let security_segments = manifest
        .segments()
        .iter()
        .filter(|segment| segment.kind() == ManifestSegmentKind::SecurityPolicy)
        .map(|segment| segment.id())
        .collect::<Vec<_>>();
    if security_segments.is_empty() {
        return Err(CliError::new(PublicCode::CORRUPT_DATA));
    }
    let policy_history = SecurityPolicyHistoryStore::new(layout.clone())
        .load_history(manifest.revision(), &security_segments)
        .map_err(|_| CliError::new(PublicCode::CORRUPT_DATA))?;
    Ok(CurrentPolicyProject {
        layout,
        _lock: lock,
        manifest,
        policy_history,
    })
}

fn authorize_cli_capability(
    policy: worlddb_core::SecurityPolicyView<'_>,
    capability: Capability,
    target: PolicyTarget,
) -> Result<(), CliError> {
    if policy
        .current_snapshot()
        .authorize(policy.principal_id(), capability, target)
        == AuthorizationDecision::Allow
    {
        Ok(())
    } else {
        Err(CliError::new(PublicCode::UNAUTHORIZED))
    }
}

fn reject_restore_target_within_project(
    destination: &Path,
    project_root: &Path,
) -> Result<(), CliError> {
    let file_name = destination
        .file_name()
        .ok_or_else(CliError::invalid_request)?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent =
        fs::canonicalize(parent).map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    let candidate = canonical_parent.join(file_name);
    if candidate.starts_with(project_root) || project_root.starts_with(&candidate) {
        return Err(CliError::invalid_request());
    }
    Ok(())
}

fn run_storage_check(path: &Path, kind: CheckKind) -> Result<Success, CliError> {
    let layout = DatabaseLayout::open(path).map_err(map_storage_file_error)?;
    let lock = layout.try_read_only_lock().map_err(map_writer_lock_error)?;
    let report = StorageVerifier::new(layout)
        .verify(&lock)
        .map_err(map_storage_verify_error)?;
    Ok(Success::StorageCheck {
        kind,
        safe_revision: report.safe_revision().value(),
        disposition: report.disposition(),
        damage: DamageSummary::from_report(&report),
    })
}

fn run_recovery(path: &Path) -> Result<Success, CliError> {
    let layout = DatabaseLayout::open(path).map_err(map_storage_file_error)?;
    let lock = layout.try_writer_lock().map_err(map_writer_lock_error)?;
    let outcome = RecoveryManager::new(layout.clone())
        .recover(&lock)
        .map_err(map_recovery_error)?;
    let report = StorageVerifier::new(layout)
        .verify(&lock)
        .map_err(map_storage_verify_error)?;
    Ok(Success::RecoveryApplied {
        safe_revision: report.safe_revision().value(),
        disposition: report.disposition(),
        damage: DamageSummary::from_report(&report),
        quarantined_tails: outcome.quarantined_tails(),
        replayed_snapshots: outcome.replayed_snapshots(),
        manifest_published: outcome.manifest_receipt().is_some(),
    })
}

fn run_salvage(source: &Path, target: &Path) -> Result<Success, CliError> {
    let layout = DatabaseLayout::open(source).map_err(map_storage_file_error)?;
    let lock = layout.try_read_only_lock().map_err(map_writer_lock_error)?;
    let report = SalvageManager::new(layout)
        .salvage(&lock, target)
        .map_err(map_salvage_error)?;
    let mut copied_segments = 0;
    let mut omitted_segments = 0;
    for segment in report.segments() {
        match segment.outcome() {
            SalvageSegmentOutcome::Copied { .. } => copied_segments += 1,
            SalvageSegmentOutcome::Omitted { .. } => omitted_segments += 1,
        }
    }
    Ok(Success::Salvage {
        database_id: report.database_id(),
        safe_revision: report.safe_revision().value(),
        disposition: report.disposition(),
        inventory_source: report.inventory_source(),
        copied_segments,
        omitted_segments,
        finding_count: report.findings().len(),
    })
}

fn is_version_namespace(value: &str) -> bool {
    value.strip_prefix('v').is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn run_adapter(mut arguments: VecDeque<OsString>) -> Result<Success, CliError> {
    let mut manifest_path = None;
    let mut input_path = None;
    let mut output_path = None;
    let mut adapter_program = None;
    let mut adapter_arguments = Vec::new();

    while let Some(argument) = arguments.pop_front() {
        if argument == "--" {
            adapter_program = arguments.pop_front();
            adapter_arguments = arguments.into_iter().collect();
            break;
        }
        let Some(flag) = argument.to_str() else {
            return Err(CliError::invalid_request());
        };
        let value = arguments
            .pop_front()
            .ok_or_else(CliError::invalid_request)?;
        let slot = match flag {
            "--manifest" => &mut manifest_path,
            "--input" => &mut input_path,
            "--output" => &mut output_path,
            _ => return Err(CliError::invalid_request()),
        };
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(CliError::invalid_request());
        }
    }

    let manifest_path = manifest_path.ok_or_else(CliError::invalid_request)?;
    let input_path = input_path.ok_or_else(CliError::invalid_request)?;
    let output_path = output_path.ok_or_else(CliError::invalid_request)?;
    let adapter_program = adapter_program.ok_or_else(CliError::invalid_request)?;

    let manifest_limit = u64::try_from(MAX_ADAPTER_MANIFEST_ENCODED_BYTES)
        .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
    let manifest_bytes = read_bounded_file(&manifest_path, manifest_limit)?;
    let manifest = AdapterManifest::decode(&manifest_bytes).map_err(map_adapter_error)?;
    let input = read_bounded_file(&input_path, manifest.budget().max_input_bytes())?;

    let mut command = Command::new(adapter_program);
    command.args(adapter_arguments);
    let output = AdapterProcessHost
        .run(command, &manifest, input)
        .map_err(map_adapter_error)?;
    write_output(&output_path, output.bytes())?;
    Ok(Success::AdapterRun {
        protocol_major: output.negotiated_protocol().major(),
        protocol_minor: output.negotiated_protocol().minor(),
        output_bytes: output.bytes().len(),
    })
}

fn read_bounded_file(path: &Path, byte_limit: u64) -> Result<Vec<u8>, CliError> {
    let file = File::open(path).map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    let metadata = file
        .metadata()
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    if metadata.len() > byte_limit {
        return Err(CliError::new(PublicCode::BUDGET_EXCEEDED));
    }
    let initial_capacity =
        usize::try_from(metadata.len()).map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(initial_capacity)
        .map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?;
    file.take(byte_limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| CliError::new(PublicCode::STORAGE_READ))?;
    if u64::try_from(bytes.len()).map_err(|_| CliError::new(PublicCode::BUDGET_EXCEEDED))?
        > byte_limit
    {
        return Err(CliError::new(PublicCode::BUDGET_EXCEEDED));
    }
    Ok(bytes)
}

fn write_output(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let mut file = File::create(path).map_err(|_| CliError::new(PublicCode::INTERNAL))?;
    file.write_all(bytes)
        .map_err(|_| CliError::new(PublicCode::INTERNAL))
}

fn write_new_output(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                CliError::invalid_request()
            } else {
                CliError::new(PublicCode::INTERNAL)
            }
        })?;
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(CliError::new(PublicCode::INTERNAL));
    }
    Ok(())
}

fn ensure_new_output_target(path: &Path) -> Result<(), CliError> {
    let candidate = canonical_target_candidate(path)?;
    match fs::symlink_metadata(candidate) {
        Ok(_) => Err(CliError::invalid_request()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CliError::new(PublicCode::STORAGE_READ)),
    }
}

fn map_logical_export_error(error: LogicalExportError) -> CliError {
    match error {
        LogicalExportError::AuthorizationDenied
        | LogicalExportError::ProjectAuthorizationDenied => CliError::new(PublicCode::UNAUTHORIZED),
        LogicalExportError::InvalidScope
        | LogicalExportError::DuplicateHistorySpace
        | LogicalExportError::DuplicateRecordKind => CliError::invalid_request(),
        LogicalExportError::UnknownHistorySpace => CliError::new(PublicCode::NOT_FOUND),
        LogicalExportError::UnsupportedRecordKind => CliError::unsupported_operation(),
        LogicalExportError::ResourceLimit | LogicalExportError::AllocationFailed => {
            CliError::new(PublicCode::BUDGET_EXCEEDED)
        }
        LogicalExportError::InvalidEncoding
        | LogicalExportError::DigestMismatch
        | LogicalExportError::NonCanonicalEncoding
        | LogicalExportError::RevisionlessRecord
        | LogicalExportError::MissingDependency
        | LogicalExportError::DatabaseIdentityMissing
        | LogicalExportError::SnapshotMismatch => CliError::new(PublicCode::CORRUPT_DATA),
        _ => CliError::new(PublicCode::STORAGE_READ),
    }
}

fn map_sharing_export_error(error: SharingExportError) -> CliError {
    match error {
        SharingExportError::AuthorizationDenied => CliError::new(PublicCode::UNAUTHORIZED),
        SharingExportError::ResourceLimit | SharingExportError::AllocationFailed => {
            CliError::new(PublicCode::BUDGET_EXCEEDED)
        }
        SharingExportError::LogicalExport(error) => map_logical_export_error(error),
        SharingExportError::ImplicitHistorySpaceDependency => CliError::invalid_request(),
        SharingExportError::DuplicateRecordIdentity
        | SharingExportError::AuditReceiptMissing
        | SharingExportError::SnapshotMismatch
        | SharingExportError::InvalidEncoding
        | SharingExportError::DigestMismatch
        | SharingExportError::NonCanonicalEncoding => CliError::new(PublicCode::CORRUPT_DATA),
        _ => CliError::new(PublicCode::STORAGE_READ),
    }
}

fn map_logical_import_error(error: LogicalImportError) -> CliError {
    match error {
        LogicalImportError::Export(error) => map_logical_export_error(error),
        LogicalImportError::ResourceLimit | LogicalImportError::AllocationFailed => {
            CliError::new(PublicCode::BUDGET_EXCEEDED)
        }
        LogicalImportError::InvalidPlanEncoding
        | LogicalImportError::PlanDigestMismatch
        | LogicalImportError::NonCanonicalPlan
        | LogicalImportError::SourceArtifactMismatch
        | LogicalImportError::DuplicateImportedRecordIdentity(_)
        | LogicalImportError::Record(_) => CliError::new(PublicCode::CORRUPT_DATA),
        _ => CliError::invalid_request(),
    }
}

fn map_adapter_error(error: AdapterProtocolError) -> CliError {
    let code = match error {
        AdapterProtocolError::InvalidManifest
        | AdapterProtocolError::InvalidHandshake
        | AdapterProtocolError::InvalidCapability
        | AdapterProtocolError::DuplicateCapability
        | AdapterProtocolError::InvalidFrame
        | AdapterProtocolError::NonCanonicalEncoding => PublicCode::INVALID_REQUEST,
        AdapterProtocolError::FrameDigestMismatch
        | AdapterProtocolError::TrailingResponseBytes
        | AdapterProtocolError::InvalidOutput => PublicCode::CORRUPT_DATA,
        AdapterProtocolError::ProtocolMismatch
        | AdapterProtocolError::RequiredCapabilityMissing(_) => PublicCode::UNSUPPORTED_OPERATION,
        AdapterProtocolError::InputTooLarge
        | AdapterProtocolError::OutputTooLarge
        | AdapterProtocolError::Timeout
        | AdapterProtocolError::AllocationFailed
        | AdapterProtocolError::ResourceLimit => PublicCode::BUDGET_EXCEEDED,
        AdapterProtocolError::AdapterCrashed(_)
        | AdapterProtocolError::ProcessIo(_)
        | AdapterProtocolError::ProcessPipeUnavailable
        | AdapterProtocolError::SupervisorFailed => PublicCode::INTERNAL,
    };
    CliError::new(code)
}

fn map_storage_file_error(error: StorageFileError) -> CliError {
    match error {
        StorageFileError::DatabaseAlreadyExists => CliError::invalid_request(),
        StorageFileError::Identity(_) | StorageFileError::StagingNameExhausted => {
            CliError::new(PublicCode::INTERNAL)
        }
        StorageFileError::InvalidDatabaseId(_)
        | StorageFileError::InvalidLayoutEntry { .. }
        | StorageFileError::PathEscapesDatabaseRoot { .. } => {
            CliError::new(PublicCode::CORRUPT_DATA)
        }
        StorageFileError::Io { .. } => CliError::new(PublicCode::STORAGE_READ),
        StorageFileError::Format(FormatProbeError::UnsupportedRequiredCapabilities { .. }) => {
            CliError::unsupported_operation()
        }
        StorageFileError::Format(_) => CliError::new(PublicCode::CORRUPT_DATA),
        StorageFileError::WriterLock(error) => map_writer_lock_error(error),
        StorageFileError::RecoveryRequired => CliError::unsupported_operation(),
    }
}

fn map_writer_lock_error(error: WriterLockError) -> CliError {
    match error {
        WriterLockError::AlreadyHeld
        | WriterLockError::LockFileMissing
        | WriterLockError::Io(_) => CliError::new(PublicCode::STORAGE_READ),
    }
}

fn map_storage_verify_error(error: StorageVerifyError) -> CliError {
    match error {
        StorageVerifyError::Recovery(_) | StorageVerifyError::Wal(_) => {
            CliError::new(PublicCode::STORAGE_READ)
        }
        StorageVerifyError::HistorySegment(_) | StorageVerifyError::SecuritySegment(_) => {
            CliError::new(PublicCode::CORRUPT_DATA)
        }
    }
}

fn map_recovery_error(error: RecoveryError) -> CliError {
    match error {
        RecoveryError::UnsafeFindings(_)
        | RecoveryError::InvalidReplayPayload
        | RecoveryError::ReplaySegmentMissing
        | RecoveryError::ReplayRevisionInvalid
        | RecoveryError::TailJournalMismatch
        | RecoveryError::TailSourceInvalid => CliError::new(PublicCode::CORRUPT_DATA),
        RecoveryError::ForeignWriterLock | RecoveryError::WriteAccessDenied => {
            CliError::unsupported_operation()
        }
        RecoveryError::Interrupted => CliError::new(PublicCode::CANCELLED),
        RecoveryError::Scan(_)
        | RecoveryError::Journal(_)
        | RecoveryError::Wal(_)
        | RecoveryError::Manifest(_)
        | RecoveryError::HistorySegment(_)
        | RecoveryError::SecuritySegment(_)
        | RecoveryError::Io { .. } => CliError::new(PublicCode::STORAGE_READ),
    }
}

fn map_salvage_error(error: SalvageError) -> CliError {
    match error {
        SalvageError::ForeignWriterLock => CliError::unsupported_operation(),
        SalvageError::Scan(_) => CliError::new(PublicCode::STORAGE_READ),
        SalvageError::Identity(_) => CliError::new(PublicCode::INTERNAL),
        SalvageError::InvalidTarget | SalvageError::TargetExists => CliError::invalid_request(),
        SalvageError::Io { .. } => CliError::new(PublicCode::STORAGE_READ),
    }
}

fn map_backup_error(error: BackupError) -> CliError {
    match error {
        BackupError::Io { .. }
        | BackupError::StorageFile(_)
        | BackupError::Wal(_)
        | BackupError::Manifest(_)
        | BackupError::Compaction(_)
        | BackupError::Audit(_)
        | BackupError::StorageVerify(_) => CliError::new(PublicCode::STORAGE_READ),
        BackupError::WriterLock(error) => map_writer_lock_error(error),
        BackupError::AuditAccess(_) => CliError::new(PublicCode::UNAUTHORIZED),
        BackupError::AuthorizationDenied { .. } => CliError::new(PublicCode::UNAUTHORIZED),
        BackupError::SecurityPolicy(_) | BackupError::PolicyUnavailable => {
            CliError::new(PublicCode::CORRUPT_DATA)
        }
        BackupError::AuditSnapshotConflict => CliError::new(PublicCode::INTERNAL),
        BackupError::DatabaseIdentityMissing
        | BackupError::SourceSnapshotNotClean
        | BackupError::SourceSnapshotMismatch
        | BackupError::IncompleteTarget
        | BackupError::InvalidBackupManifest
        | BackupError::IntegrityMismatch
        | BackupError::InventoryMismatch
        | BackupError::TargetSnapshotMismatch
        | BackupError::TargetNotClean(_)
        | BackupError::InvalidItemPath
        | BackupError::ProfileMismatch => CliError::new(PublicCode::CORRUPT_DATA),
        BackupError::InvalidTarget
        | BackupError::TargetAlreadyExists
        | BackupError::TargetParentMissing
        | BackupError::InvalidKeyId => CliError::invalid_request(),
        BackupError::AuditSegmentsUnsupported => CliError::unsupported_operation(),
        BackupError::ResourceLimit | BackupError::AllocationFailed => {
            CliError::new(PublicCode::BUDGET_EXCEEDED)
        }
    }
}

fn map_restore_error(error: RestoreError) -> CliError {
    match error {
        RestoreError::AuthorizationDenied { .. } => CliError::new(PublicCode::UNAUTHORIZED),
        RestoreError::Backup(error) => map_backup_error(error),
        RestoreError::TargetAlreadyExists => CliError::invalid_request(),
        RestoreError::CopyInventoryMismatch
        | RestoreError::SourceSnapshotMismatch
        | RestoreError::AuditLineageMismatch => CliError::new(PublicCode::CORRUPT_DATA),
        _ => CliError::new(PublicCode::STORAGE_READ),
    }
}

fn backup_profile_label(profile: BackupProfile) -> &'static str {
    match profile {
        BackupProfile::ExactDatabase => "ExactDatabase",
        BackupProfile::AuditComplete => "AuditComplete",
    }
}

fn migration_category_label(category: MigrationCategory) -> &'static str {
    match category {
        MigrationCategory::MetadataOnly => "MetadataOnly",
        MigrationCategory::Additive => "Additive",
        MigrationCategory::CompatibleConstraintChange => "CompatibleConstraintChange",
        MigrationCategory::Restrictive => "Restrictive",
        MigrationCategory::Breaking => "Breaking",
    }
}

fn fingerprint_hex(fingerprint: &[u8; 32]) -> String {
    fingerprint
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(&mut output, "{byte:02x}");
            output
        })
}

fn damage_class_index(class: StorageDamageClass) -> usize {
    match class {
        StorageDamageClass::Bitflip => 0,
        StorageDamageClass::Truncation => 1,
        StorageDamageClass::Reorder => 2,
        StorageDamageClass::DuplicateFrame => 3,
        StorageDamageClass::SemanticInvalidity => 4,
        StorageDamageClass::Other => 5,
    }
}

fn damage_class_label(index: usize) -> &'static str {
    match index {
        0 => "Bitflip",
        1 => "Truncation",
        2 => "Reorder",
        3 => "DuplicateFrame",
        4 => "SemanticInvalidity",
        _ => "Other",
    }
}

fn verify_action_index(action: StorageVerifyAction) -> usize {
    match action {
        StorageVerifyAction::PreserveOriginal => 0,
        StorageVerifyAction::KeepReadOnly => 1,
        StorageVerifyAction::RunJournaledTailRecovery => 2,
        StorageVerifyAction::RunJournaledRecovery => 3,
        StorageVerifyAction::RestoreVerifiedBackupToNewDestination => 4,
        StorageVerifyAction::SalvageIntoNewDatabase => 5,
    }
}

fn verify_action_label(index: usize) -> &'static str {
    match index {
        0 => "PreserveOriginal",
        1 => "KeepReadOnly",
        2 => "RunJournaledTailRecovery",
        3 => "RunJournaledRecovery",
        4 => "RestoreVerifiedBackupToNewDestination",
        _ => "SalvageIntoNewDatabase",
    }
}

const fn disposition_label(disposition: RecoveryDisposition) -> &'static str {
    match disposition {
        RecoveryDisposition::Clean => "Clean",
        RecoveryDisposition::RecoveryRequired => "RecoveryRequired",
        RecoveryDisposition::QuarantinedReadOnly => "QuarantinedReadOnly",
    }
}

const fn salvage_inventory_label(source: SalvageInventorySource) -> &'static str {
    match source {
        SalvageInventorySource::CommittedWalSnapshot { .. } => "CommittedWalSnapshot",
        SalvageInventorySource::CurrentManifest { .. } => "CurrentManifest",
        SalvageInventorySource::NoVerifiedInventory => "NoVerifiedInventory",
    }
}

fn format_damage_counts(summary: DamageSummary) -> String {
    let mut output = String::from("{");
    for (index, count) in summary.counts.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        let _ = write!(output, "{}={count}", damage_class_label(index));
    }
    output.push('}');
    output
}

fn format_safe_actions(summary: DamageSummary) -> String {
    let mut output = String::new();
    for (index, selected) in summary.actions.iter().enumerate() {
        if *selected {
            if !output.is_empty() {
                output.push(',');
            }
            output.push_str(verify_action_label(index));
        }
    }
    if output.is_empty() {
        String::from("none")
    } else {
        output
    }
}

fn new_request_id() -> Result<RequestId, ()> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| ())?;
    let Some(version) = bytes.get_mut(6) else {
        return Err(());
    };
    *version = (*version & 0x0f) | 0x40;
    let Some(variant) = bytes.get_mut(8) else {
        return Err(());
    };
    *variant = (*variant & 0x3f) | 0x80;
    Ok(RequestId::from_bytes(bytes))
}

fn format_request_id(request_id: RequestId) -> String {
    request_id.as_bytes().into_iter().enumerate().fold(
        String::with_capacity(36),
        |mut output, (index, byte)| {
            if matches!(index, 4 | 6 | 8 | 10) {
                output.push('-');
            }
            let _ = write!(&mut output, "{byte:02x}");
            output
        },
    )
}

fn write_success<W: Write>(
    writer: &mut W,
    format: OutputFormat,
    request_id: RequestId,
    success: Success,
) -> io::Result<()> {
    match format {
        OutputFormat::Human => match success {
            Success::Help(HelpScope::Root) => writeln!(writer, "{HELP_ROOT}"),
            Success::Help(HelpScope::AdapterRun) => writeln!(writer, "{HELP_ADAPTER_RUN}"),
            Success::Help(HelpScope::Verify) => writeln!(writer, "{HELP_VERIFY}"),
            Success::Help(HelpScope::Recovery) => writeln!(writer, "{HELP_RECOVERY}"),
            Success::Help(HelpScope::OpenReadOnly) => writeln!(writer, "{HELP_OPEN_READ_ONLY}"),
            Success::Help(HelpScope::Salvage) => writeln!(writer, "{HELP_SALVAGE}"),
            Success::Help(HelpScope::Backup) => writeln!(writer, "{HELP_BACKUP}"),
            Success::Help(HelpScope::Restore) => writeln!(writer, "{HELP_RESTORE}"),
            Success::Help(HelpScope::Migration) => writeln!(writer, "{HELP_MIGRATION}"),
            Success::Help(HelpScope::StorageUpgrade) => {
                writeln!(writer, "{HELP_STORAGE_UPGRADE}")
            }
            Success::Help(HelpScope::ExportImport) => writeln!(writer, "{HELP_EXPORT_IMPORT}"),
            Success::Version => writeln!(
                writer,
                "worlddb-cli {} (CLI protocol {}.{})",
                env!("CARGO_PKG_VERSION"),
                CURRENT_PROTOCOL.major(),
                CURRENT_PROTOCOL.minor()
            ),
            Success::AdapterRun {
                protocol_major,
                protocol_minor,
                output_bytes,
            } => writeln!(
                writer,
                "adapter completed: protocol {protocol_major}.{protocol_minor}, {output_bytes} output bytes"
            ),
            Success::StorageCheck {
                kind,
                safe_revision,
                disposition,
                damage,
            } => writeln!(
                writer,
                "{}: disposition={}, safe_revision={}, findings={}, damage_classes={}, safe_actions={}, source_modified=false",
                kind.label(),
                disposition_label(disposition),
                safe_revision,
                damage.finding_count(),
                format_damage_counts(damage),
                format_safe_actions(damage)
            ),
            Success::RecoveryApplied {
                safe_revision,
                disposition,
                damage,
                quarantined_tails,
                replayed_snapshots,
                manifest_published,
            } => writeln!(
                writer,
                "recovery: disposition={}, safe_revision={}, findings={}, damage_classes={}, quarantined_tails={}, replayed_snapshots={}, manifest_published={}, source_modified={}",
                disposition_label(disposition),
                safe_revision,
                damage.finding_count(),
                format_damage_counts(damage),
                quarantined_tails,
                replayed_snapshots,
                manifest_published,
                quarantined_tails > 0 || replayed_snapshots > 0 || manifest_published
            ),
            Success::Salvage {
                database_id,
                safe_revision,
                disposition,
                inventory_source,
                copied_segments,
                omitted_segments,
                finding_count,
            } => writeln!(
                writer,
                "salvage: new_database_id={}, inventory_source={}, disposition={}, safe_revision={}, copied_segments={}, omitted_segments={}, findings={}, source_modified=false",
                database_id.to_canonical_string(),
                salvage_inventory_label(inventory_source),
                disposition_label(disposition),
                safe_revision,
                copied_segments,
                omitted_segments,
                finding_count
            ),
            Success::Backup {
                action,
                profile,
                audit_scope,
                summary,
            } => writeln!(
                writer,
                "backup {}: profile={}, audit_scope={}, database_id={}, revision={}, item_count={}, audit_safe_sequence={}, authenticity={}, target_verified=true, source_modified=false",
                action.label(),
                backup_profile_label(profile),
                audit_scope.label(),
                summary.database_id.to_canonical_string(),
                summary.revision,
                summary.item_count,
                summary
                    .audit_safe_sequence
                    .map_or_else(|| String::from("none"), |sequence| sequence.to_string()),
                summary.authenticity.label()
            ),
            Success::Restore {
                profile,
                audit_scope,
                summary,
            } => writeln!(
                writer,
                "restore clone: profile={}, audit_scope={}, source_database_id={}, source_revision={}, restored_database_id={}, restored_revision={}, item_count={}, audit_safe_sequence={}, authenticity={}, target_verified=true, source_modified=false",
                backup_profile_label(profile),
                audit_scope.label(),
                summary.source_database_id.to_canonical_string(),
                summary.source_revision,
                summary.restored_database_id.to_canonical_string(),
                summary.restored_revision,
                summary.item_count,
                summary
                    .audit_safe_sequence
                    .map_or_else(|| String::from("none"), |sequence| sequence.to_string()),
                summary.authenticity.label()
            ),
            Success::Migration(summary) => writeln!(
                writer,
                "migration {}: database_id={}, migration_id={}, category={}, plan_fingerprint={}, source_revision={}, target_revision={}, steps={}, input_records={}, input_bytes={}, estimate_output_bytes={}, errors={}, unresolved={}, completed_steps={}, final_revision={}",
                summary.status.label(),
                summary.database_id.to_canonical_string(),
                summary.migration_id.to_canonical_string(),
                migration_category_label(summary.category),
                fingerprint_hex(&summary.fingerprint),
                summary.source_revision,
                summary.target_revision,
                summary.step_count,
                summary.input_record_count,
                summary.input_bytes,
                summary
                    .estimate_output_bytes
                    .map_or_else(|| String::from("none"), |bytes| bytes.to_string()),
                summary.error_count,
                summary.unresolved_count,
                summary.completed_step_count,
                summary
                    .final_revision
                    .map_or_else(|| String::from("none"), |revision| revision.to_string())
            ),
            Success::StorageUpgrade(summary) => writeln!(
                writer,
                "storage upgrade completed: plan_id={}, run_id={}, source_revision={}, target_profile_fingerprint={}, current_pointer_digest={}, resumed={}",
                summary.plan_id.to_canonical_string(),
                summary.run_id.to_canonical_string(),
                summary.source_revision,
                fingerprint_hex(&summary.target_fingerprint),
                fingerprint_hex(&summary.pointer_digest),
                summary.resumed
            ),
            Success::Export(summary) => write_human_export(writer, summary),
            Success::ImportPlan(summary) => writeln!(
                writer,
                "import plan created: source_database_id={}, destination_database_id={}, remappings={}, plan_digest={}",
                summary.source_database_id.to_canonical_string(),
                summary.destination_database_id.to_canonical_string(),
                summary.mapping_count,
                fingerprint_hex(&summary.plan_digest)
            ),
            Success::ImportPrepared(summary) => writeln!(
                writer,
                "import prepared: source_database_id={}, destination_database_id={}, records={}, remappings={}, stream_fingerprint={}, from_revision={}, through_revision={}, history_spaces={}, record_classes={}, omitted_record_classes={}, omitted_storage_classes={}, writes_database=false",
                summary.source_database_id.to_canonical_string(),
                summary.destination_database_id.to_canonical_string(),
                summary.record_count,
                summary.mapping_count,
                fingerprint_hex(&summary.stream_fingerprint),
                summary.from_revision,
                summary.through_revision,
                summary.history_space_count,
                summary.record_kind_count,
                summary.omitted_record_class_count,
                summary.omitted_storage_class_count
            ),
        },
        OutputFormat::JsonLines => write_json_success(writer, request_id, success),
    }
}

fn write_human_export<W: Write>(writer: &mut W, summary: ExportSummary) -> io::Result<()> {
    let selected_history_spaces = summary.selected_history_spaces.join(",");
    let selected_record_kinds = summary.selected_record_kinds.join(",");
    if summary.sharing {
        writeln!(
            writer,
            "sharing export completed: from_revision={}, through_revision={}, history_spaces={}, selected_history_spaces=[{}], record_classes={}, selected_record_classes=[{}], included_records={}, omission_counts=withheld, source_audit_committed=true",
            summary.from_revision,
            summary.through_revision,
            summary.history_space_count,
            selected_history_spaces,
            summary.record_kind_count,
            selected_record_kinds,
            summary.record_count
        )
    } else {
        let database_id = summary
            .database_id
            .map_or_else(|| String::from("unknown"), DatabaseId::to_canonical_string);
        let snapshot_revision = summary
            .snapshot_revision
            .map_or_else(|| String::from("unknown"), |value| value.to_string());
        writeln!(
            writer,
            "logical export completed: database_id={database_id}, snapshot_revision={snapshot_revision}, from_revision={}, through_revision={}, history_spaces={}, selected_history_spaces=[{}], record_classes={}, selected_record_classes=[{}], records={}, omitted_record_classes={}, omitted_storage_classes={}",
            summary.from_revision,
            summary.through_revision,
            summary.history_space_count,
            selected_history_spaces,
            summary.record_kind_count,
            selected_record_kinds,
            summary.record_count,
            summary.omitted_record_class_count,
            summary.omitted_storage_class_count
        )
    }
}

fn record_kind_label(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::HistorySpaceDefinition => "HistorySpaceDefinition",
        RecordKind::Entity => "Entity",
        RecordKind::EntityRetirement => "EntityRetirement",
        RecordKind::PerspectiveDefinitionRevision => "PerspectiveDefinitionRevision",
        RecordKind::PerspectiveRetirement => "PerspectiveRetirement",
        RecordKind::LayerDefinition => "LayerDefinition",
        RecordKind::LayerSchemaSnapshot => "LayerSchemaSnapshot",
        RecordKind::EntityTypeDefinition => "EntityTypeDefinition",
        RecordKind::PredicateDefinition => "PredicateDefinition",
        RecordKind::EventKindDefinition => "EventKindDefinition",
        RecordKind::MigrationPlan => "MigrationPlan",
        RecordKind::MigrationRun => "MigrationRun",
        RecordKind::MigrationStepCommitIdentity => "MigrationStepCommitIdentity",
        RecordKind::Assertion => "Assertion",
        RecordKind::AssertionValidityClosure => "AssertionValidityClosure",
        RecordKind::AssertionRetraction => "AssertionRetraction",
        RecordKind::Mask => "Mask",
        RecordKind::MaskValidityClosure => "MaskValidityClosure",
        RecordKind::MaskRetraction => "MaskRetraction",
        RecordKind::ReplacementBoundary => "ReplacementBoundary",
        RecordKind::ReplacementBoundaryValidityClosure => "ReplacementBoundaryValidityClosure",
        RecordKind::ReplacementBoundaryRetraction => "ReplacementBoundaryRetraction",
        RecordKind::ArchiveTransition => "ArchiveTransition",
        RecordKind::Event => "Event",
        RecordKind::EventMask => "EventMask",
        RecordKind::EventSpanClosure => "EventSpanClosure",
        RecordKind::EventRetraction => "EventRetraction",
        RecordKind::EventMaskRetraction => "EventMaskRetraction",
        RecordKind::EventRelation => "EventRelation",
        RecordKind::EventRelationRetraction => "EventRelationRetraction",
        RecordKind::Source => "Source",
        RecordKind::Evidence => "Evidence",
        RecordKind::Provenance => "Provenance",
        RecordKind::EvidenceRetraction => "EvidenceRetraction",
        RecordKind::ProvenanceRetraction => "ProvenanceRetraction",
        RecordKind::TransferLineage => "TransferLineage",
    }
}

fn write_json_success<W: Write>(
    writer: &mut W,
    request_id: RequestId,
    success: Success,
) -> io::Result<()> {
    let request_id = format_request_id(request_id);
    match success {
        Success::Help(HelpScope::Root) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"root\",\"usage\":\"worlddb-cli [--format human|jsonl] <COMMAND>\",\"commands\":[\"v1 verify\",\"v1 recovery inspect\",\"v1 recovery run --apply\",\"v1 open --read-only\",\"v1 salvage\",\"v1 backup\",\"v1 restore\",\"v1 migration\",\"v1 export logical|share\",\"v1 import plan|prepare\",\"v1 storage upgrade\",\"v1 adapter run\",\"help\",\"--version\"]}}}}}}"
        ),
        Success::Help(HelpScope::AdapterRun) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"adapter_run\",\"usage\":\"worlddb-cli [--format human|jsonl] v1 adapter run --manifest <file> --input <file> --output <file> -- <adapter-executable> [arguments...]\",\"required_options\":[\"--manifest\",\"--input\",\"--output\"],\"separator\":\"--\"}}}}}}"
        ),
        Success::Help(HelpScope::Verify) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"verify\",\"usage\":\"worlddb-cli [--format human|jsonl] v1 verify <database-directory>\"}}}}}}"
        ),
        Success::Help(HelpScope::Recovery) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"recovery\",\"inspect_usage\":\"v1 recovery inspect <database-directory>\",\"apply_usage\":\"v1 recovery run --apply <database-directory>\"}}}}}}"
        ),
        Success::Help(HelpScope::OpenReadOnly) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"open_read_only\",\"usage\":\"v1 open --read-only <database-directory>\"}}}}}}"
        ),
        Success::Help(HelpScope::Salvage) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"salvage\",\"usage\":\"v1 salvage <source-directory> --output <new-directory>\"}}}}}}"
        ),
        Success::Help(HelpScope::Backup) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"backup\",\"create_usage\":\"v1 backup create <source-directory> --output <new-directory> --profile exact|audit-complete --audit-scope excluded|included\",\"verify_usage\":\"v1 backup verify <backup-directory> --profile exact|audit-complete --audit-scope excluded|included\"}}}}}}"
        ),
        Success::Help(HelpScope::Restore) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"restore\",\"clone_usage\":\"v1 restore clone <backup-directory> --authorize-with <current-project-directory> --output <new-database-directory> --profile exact|audit-complete --audit-scope excluded|included\",\"same_identity_disaster_recovery\":false}}}}}}"
        ),
        Success::Help(HelpScope::Migration) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"migration\",\"usage\":\"v1 migration plan|dry-run|run|resume <database> --plan-file <record> [step options]\",\"breaking_requires\":[\"--backup\",\"--restore\",\"--confirm-breaking\"],\"paths_in_results\":false}}}}}}"
        ),
        Success::Help(HelpScope::StorageUpgrade) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"storage_upgrade\",\"usage\":\"v1 storage upgrade <database> --backup <new-directory> --restore <new-directory> --confirm\",\"confirmation_required\":true}}}}}}"
        ),
        Success::Help(HelpScope::ExportImport) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"export_import\",\"usage\":\"v1 export logical|share; v1 import plan|prepare\",\"paths_in_results\":false,\"prepare_publishes_records\":false}}}}}}"
        ),
        Success::Version => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"version\",\"data\":{{\"cli_version\":\"{}\",\"protocol\":{{\"major\":{},\"minor\":{}}}}}}}}}",
            env!("CARGO_PKG_VERSION"),
            CURRENT_PROTOCOL.major(),
            CURRENT_PROTOCOL.minor()
        ),
        Success::AdapterRun {
            protocol_major,
            protocol_minor,
            output_bytes,
        } => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"adapter_run\",\"data\":{{\"status\":\"completed\",\"adapter_protocol\":{{\"major\":{protocol_major},\"minor\":{protocol_minor}}},\"output_bytes\":\"{output_bytes}\"}}}}}}"
        ),
        Success::StorageCheck {
            kind,
            safe_revision,
            disposition,
            damage,
        } => {
            write!(
                writer,
                "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"{}\",\"data\":{{\"status\":\"completed\",\"safe_revision\":\"{safe_revision}\",\"disposition\":\"{}\",\"finding_count\":\"{}\",\"source_modified\":false,\"damage_classes\":{{",
                kind.label(),
                disposition_label(disposition),
                damage.finding_count()
            )?;
            write_json_damage_counts(writer, damage)?;
            writer.write_all(b"},\"safe_actions\":[")?;
            write_json_safe_actions(writer, damage)?;
            writer.write_all(b"]}}}\n")
        }
        Success::RecoveryApplied {
            safe_revision,
            disposition,
            damage,
            quarantined_tails,
            replayed_snapshots,
            manifest_published,
        } => {
            let source_modified =
                quarantined_tails > 0 || replayed_snapshots > 0 || manifest_published;
            write!(
                writer,
                "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"recovery\",\"data\":{{\"status\":\"applied\",\"safe_revision\":\"{safe_revision}\",\"disposition\":\"{}\",\"finding_count\":\"{}\",\"source_modified\":{source_modified},\"quarantined_tails\":\"{quarantined_tails}\",\"replayed_snapshots\":\"{replayed_snapshots}\",\"manifest_published\":{manifest_published},\"damage_classes\":{{",
                disposition_label(disposition),
                damage.finding_count()
            )?;
            write_json_damage_counts(writer, damage)?;
            writer.write_all(b"},\"safe_actions\":[")?;
            write_json_safe_actions(writer, damage)?;
            writer.write_all(b"]}}}\n")
        }
        Success::Salvage {
            database_id,
            safe_revision,
            disposition,
            inventory_source,
            copied_segments,
            omitted_segments,
            finding_count,
        } => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"salvage\",\"data\":{{\"status\":\"completed\",\"new_database_id\":\"{}\",\"inventory_source\":\"{}\",\"safe_revision\":\"{safe_revision}\",\"disposition\":\"{}\",\"copied_segments\":\"{copied_segments}\",\"omitted_segments\":\"{omitted_segments}\",\"finding_count\":\"{finding_count}\",\"archive_created\":true,\"source_modified\":false}}}}}}",
            database_id.to_canonical_string(),
            salvage_inventory_label(inventory_source),
            disposition_label(disposition)
        ),
        Success::Backup {
            action,
            profile,
            audit_scope,
            summary,
        } => {
            write!(
                writer,
                "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"backup\",\"data\":{{\"status\":\"{}\",\"profile\":\"{}\",\"audit_scope\":\"{}\",\"database_id\":\"{}\",\"revision\":\"{}\",\"item_count\":\"{}\",\"audit_safe_sequence\":",
                action.label(),
                backup_profile_label(profile),
                audit_scope.label(),
                summary.database_id.to_canonical_string(),
                summary.revision,
                summary.item_count
            )?;
            if let Some(sequence) = summary.audit_safe_sequence {
                write!(writer, "\"{sequence}\"")?;
            } else {
                writer.write_all(b"null")?;
            }
            writeln!(
                writer,
                ",\"authenticity\":\"{}\",\"target_verified\":true,\"source_modified\":false}}}}}}",
                summary.authenticity.label()
            )
        }
        Success::Restore {
            profile,
            audit_scope,
            summary,
        } => {
            write!(
                writer,
                "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"restore_clone\",\"data\":{{\"status\":\"restored\",\"profile\":\"{}\",\"audit_scope\":\"{}\",\"source_database_id\":\"{}\",\"source_revision\":\"{}\",\"restored_database_id\":\"{}\",\"restored_revision\":\"{}\",\"item_count\":\"{}\",\"audit_safe_sequence\":",
                backup_profile_label(profile),
                audit_scope.label(),
                summary.source_database_id.to_canonical_string(),
                summary.source_revision,
                summary.restored_database_id.to_canonical_string(),
                summary.restored_revision,
                summary.item_count
            )?;
            if let Some(sequence) = summary.audit_safe_sequence {
                write!(writer, "\"{sequence}\"")?;
            } else {
                writer.write_all(b"null")?;
            }
            writeln!(
                writer,
                ",\"authenticity\":\"{}\",\"target_verified\":true,\"source_modified\":false}}}}}}",
                summary.authenticity.label()
            )
        }
        Success::Migration(summary) => {
            write!(
                writer,
                "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"migration\",\"data\":{{\"status\":\"{}\",\"database_id\":\"{}\",\"migration_id\":\"{}\",\"category\":\"{}\",\"plan_fingerprint\":\"{}\",\"source_revision\":\"{}\",\"target_revision\":\"{}\",\"step_count\":\"{}\",\"input_record_count\":\"{}\",\"input_bytes\":\"{}\",\"estimate_output_bytes\":",
                summary.status.label(),
                summary.database_id.to_canonical_string(),
                summary.migration_id.to_canonical_string(),
                migration_category_label(summary.category),
                fingerprint_hex(&summary.fingerprint),
                summary.source_revision,
                summary.target_revision,
                summary.step_count,
                summary.input_record_count,
                summary.input_bytes
            )?;
            if let Some(bytes) = summary.estimate_output_bytes {
                write!(writer, "\"{bytes}\"")?;
            } else {
                writer.write_all(b"null")?;
            }
            write!(
                writer,
                ",\"error_count\":\"{}\",\"unresolved_count\":\"{}\",\"completed_step_count\":\"{}\",\"final_revision\":",
                summary.error_count, summary.unresolved_count, summary.completed_step_count
            )?;
            if let Some(revision) = summary.final_revision {
                write!(writer, "\"{revision}\"")?;
            } else {
                writer.write_all(b"null")?;
            }
            writer.write_all(b"}}}\n")
        }
        Success::StorageUpgrade(summary) => {
            write!(
                writer,
                "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"storage_upgrade\",\"data\":{{\"status\":\"completed\",\"plan_id\":\"{}\",\"run_id\":\"{}\",\"source_revision\":\"{}\",\"target_profile_fingerprint\":\"{}\",\"current_pointer_digest\":\"{}\",\"resumed\":{}",
                summary.plan_id.to_canonical_string(),
                summary.run_id.to_canonical_string(),
                summary.source_revision,
                fingerprint_hex(&summary.target_fingerprint),
                fingerprint_hex(&summary.pointer_digest),
                summary.resumed
            )?;
            writer.write_all(b"}}}\n")
        }
        Success::Export(summary) => {
            if summary.sharing {
                write!(
                    writer,
                    "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"sharing_export\",\"data\":{{\"status\":\"completed\",\"scope\":{{\"from_revision\":\"{}\",\"through_revision\":\"{}\",\"history_space_count\":\"{}\",\"history_spaces\":",
                    summary.from_revision, summary.through_revision, summary.history_space_count
                )?;
                write_json_string_array(writer, &summary.selected_history_spaces)?;
                write!(
                    writer,
                    ",\"record_class_count\":\"{}\",\"record_classes\":",
                    summary.record_kind_count
                )?;
                write_json_string_array(writer, &summary.selected_record_kinds)?;
                writeln!(
                    writer,
                    "}},\"included_record_count\":\"{}\",\"omission_counts_disclosed\":false,\"source_audit_committed\":true,\"source_modified\":true}}}}}}",
                    summary.record_count
                )
            } else {
                let database_id = summary
                    .database_id
                    .map_or_else(|| String::from("unknown"), DatabaseId::to_canonical_string);
                let snapshot_revision = summary.snapshot_revision.unwrap_or_default();
                write!(
                    writer,
                    "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"logical_export\",\"data\":{{\"status\":\"completed\",\"database_id\":\"{database_id}\",\"snapshot_revision\":\"{snapshot_revision}\",\"scope\":{{\"from_revision\":\"{}\",\"through_revision\":\"{}\",\"history_space_count\":\"{}\",\"history_spaces\":",
                    summary.from_revision, summary.through_revision, summary.history_space_count
                )?;
                write_json_string_array(writer, &summary.selected_history_spaces)?;
                write!(
                    writer,
                    ",\"record_class_count\":\"{}\",\"record_classes\":",
                    summary.record_kind_count
                )?;
                write_json_string_array(writer, &summary.selected_record_kinds)?;
                writeln!(
                    writer,
                    "}},\"record_count\":\"{}\",\"omission_manifest\":{{\"complete\":true,\"record_classes_omitted\":\"{}\",\"storage_classes_omitted\":\"{}\"}},\"source_modified\":false}}}}}}",
                    summary.record_count,
                    summary.omitted_record_class_count,
                    summary.omitted_storage_class_count
                )
            }
        }
        Success::ImportPlan(summary) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"import_plan\",\"data\":{{\"status\":\"created\",\"source_database_id\":\"{}\",\"destination_database_id\":\"{}\",\"mapping_count\":\"{}\",\"plan_digest\":\"{}\",\"writes_database\":false}}}}}}",
            summary.source_database_id.to_canonical_string(),
            summary.destination_database_id.to_canonical_string(),
            summary.mapping_count,
            fingerprint_hex(&summary.plan_digest)
        ),
        Success::ImportPrepared(summary) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"import_prepare\",\"data\":{{\"status\":\"prepared\",\"source_database_id\":\"{}\",\"destination_database_id\":\"{}\",\"stream_fingerprint\":\"{}\",\"mapping_count\":\"{}\",\"record_count\":\"{}\",\"scope\":{{\"from_revision\":\"{}\",\"through_revision\":\"{}\",\"history_space_count\":\"{}\",\"record_class_count\":\"{}\"}},\"omission_manifest\":{{\"complete\":true,\"record_classes_omitted\":\"{}\",\"storage_classes_omitted\":\"{}\"}},\"writes_database\":false}}}}}}",
            summary.source_database_id.to_canonical_string(),
            summary.destination_database_id.to_canonical_string(),
            fingerprint_hex(&summary.stream_fingerprint),
            summary.mapping_count,
            summary.record_count,
            summary.from_revision,
            summary.through_revision,
            summary.history_space_count,
            summary.record_kind_count,
            summary.omitted_record_class_count,
            summary.omitted_storage_class_count
        ),
    }
}

fn write_json_damage_counts<W: Write>(writer: &mut W, damage: DamageSummary) -> io::Result<()> {
    for (index, count) in damage.counts.iter().enumerate() {
        if index > 0 {
            writer.write_all(b",")?;
        }
        write!(writer, "\"{}\":\"{}\"", damage_class_label(index), count)?;
    }
    Ok(())
}

fn write_json_string_array<W: Write>(writer: &mut W, values: &[String]) -> io::Result<()> {
    writer.write_all(b"[")?;
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            writer.write_all(b",")?;
        }
        writer.write_all(b"\"")?;
        for character in value.chars() {
            match character {
                '"' => writer.write_all(b"\\\"")?,
                '\\' => writer.write_all(b"\\\\")?,
                '\u{08}' => writer.write_all(b"\\b")?,
                '\u{0c}' => writer.write_all(b"\\f")?,
                '\n' => writer.write_all(b"\\n")?,
                '\r' => writer.write_all(b"\\r")?,
                '\t' => writer.write_all(b"\\t")?,
                control if control <= '\u{1f}' => write!(writer, "\\u{:04x}", u32::from(control))?,
                _ => write!(writer, "{character}")?,
            }
        }
        writer.write_all(b"\"")?;
    }
    writer.write_all(b"]")
}

fn write_json_safe_actions<W: Write>(writer: &mut W, damage: DamageSummary) -> io::Result<()> {
    let mut first = true;
    for (index, selected) in damage.actions.iter().enumerate() {
        if *selected {
            if !first {
                writer.write_all(b",")?;
            }
            first = false;
            write!(writer, "\"{}\"", verify_action_label(index))?;
        }
    }
    Ok(())
}

fn write_human_error<W: Write>(writer: &mut W, error: CliError) -> io::Result<()> {
    writeln!(
        writer,
        "error[{}]: {}",
        error.code.as_str(),
        safe_message(error.code)
    )
}

fn write_json_error<W: Write>(
    writer: &mut W,
    request_id: RequestId,
    error: CliError,
) -> io::Result<()> {
    let request_id = format_request_id(request_id);
    let code = error.code.as_str();
    writeln!(
        writer,
        "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"error\",\"data\":{{\"code\":\"{code}\",\"retryable\":false,\"message_key\":\"{code}\"}}}}}}"
    )
}

fn safe_message(code: PublicCode) -> &'static str {
    match code.as_str() {
        "InvalidRequest" => "The request is invalid. Consult the command help.",
        "UnsupportedProtocolVersion" => "The requested protocol version is not supported.",
        "UnsupportedOperation" => "The requested operation is not supported.",
        "InvalidQuery" => "The query is invalid.",
        "UnsupportedQueryCapability" => "The requested query capability is not supported.",
        "Unauthorized" => "The operation is not authorized.",
        "NotFound" => "The requested resource was not found.",
        "SnapshotExpired" => "The selected snapshot is no longer available.",
        "CursorInvalidated" => "The query cursor is no longer valid.",
        "Cancelled" => "The operation was cancelled.",
        "BudgetExceeded" => "The operation exceeded a configured resource limit.",
        "StorageRead" => "Required input could not be read.",
        "CorruptData" => "Input or stored data failed integrity validation.",
        _ => "The operation could not be completed.",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CliError, EXIT_BUDGET_EXCEEDED, EXIT_CANCELLED, EXIT_CORRUPT_DATA, EXIT_INTERNAL,
        EXIT_INVALID_REQUEST, EXIT_NOT_FOUND, EXIT_STATE_INVALIDATED, EXIT_STORAGE_READ,
        EXIT_UNAUTHORIZED, EXIT_UNSUPPORTED_OPERATION, LogicalImportIdentity,
    };
    use worlddb_core::api::v1::PublicCode;

    #[test]
    fn every_public_code_has_a_stable_process_exit_status() {
        let cases = [
            (PublicCode::INVALID_REQUEST, EXIT_INVALID_REQUEST),
            (
                PublicCode::UNSUPPORTED_PROTOCOL_VERSION,
                EXIT_UNSUPPORTED_OPERATION,
            ),
            (
                PublicCode::UNSUPPORTED_QUERY_CAPABILITY,
                EXIT_UNSUPPORTED_OPERATION,
            ),
            (PublicCode::INVALID_QUERY, EXIT_INVALID_REQUEST),
            (PublicCode::UNAUTHORIZED, EXIT_UNAUTHORIZED),
            (PublicCode::NOT_FOUND, EXIT_NOT_FOUND),
            (PublicCode::SNAPSHOT_EXPIRED, EXIT_STATE_INVALIDATED),
            (PublicCode::CURSOR_INVALIDATED, EXIT_STATE_INVALIDATED),
            (PublicCode::CANCELLED, EXIT_CANCELLED),
            (PublicCode::BUDGET_EXCEEDED, EXIT_BUDGET_EXCEEDED),
            (PublicCode::STORAGE_READ, EXIT_STORAGE_READ),
            (PublicCode::CORRUPT_DATA, EXIT_CORRUPT_DATA),
            (
                PublicCode::UNSUPPORTED_OPERATION,
                EXIT_UNSUPPORTED_OPERATION,
            ),
            (PublicCode::INTERNAL, EXIT_INTERNAL),
        ];

        for (code, expected) in cases {
            assert_eq!(CliError::new(code).exit_status(), expected);
        }
    }

    #[test]
    fn request_id_formatter_produces_canonical_uuid_text() {
        let id = super::format_request_id(super::RequestId::from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x4c, 0xde, 0x8f, 0x10, 0x32, 0x54, 0x76, 0x98,
            0xba, 0xdc,
        ]));

        assert_eq!(id, "01234567-89ab-4cde-8f10-32547698badc");
    }

    #[test]
    fn machine_adapter_success_serializes_only_safe_result_fields() {
        let mut output = Vec::new();
        let result = super::write_success(
            &mut output,
            super::OutputFormat::JsonLines,
            super::RequestId::from_bytes([0x11; 16]),
            super::Success::AdapterRun {
                protocol_major: 1,
                protocol_minor: 0,
                output_bytes: 42,
            },
        );

        assert!(result.is_ok());
        let text = String::from_utf8_lossy(&output);
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("\"type\":\"adapter_run\""));
        assert!(text.contains("\"output_bytes\":\"42\""));
        assert!(!text.contains("path"));
        assert!(!text.contains("process_id"));
    }

    #[test]
    fn import_mapping_parser_accepts_typed_record_refs_and_rejects_cross_family_maps() {
        let mapping = super::parse_import_mapping(
            "record:1:01234567-89ab-7cde-8f01-23456789abcd=record:1:01234567-89ab-7cde-8f01-23456789abce",
        );
        assert!(mapping.is_ok());
        let Some(mapping) = mapping.ok() else {
            return;
        };
        assert!(matches!(
            mapping.source(),
            LogicalImportIdentity::Record(reference) if reference.wire_tag().value() == 1
        ));
        assert!(matches!(
            mapping.target(),
            LogicalImportIdentity::Record(reference) if reference.wire_tag().value() == 1
        ));

        assert!(super::parse_import_mapping(
            "record:1:01234567-89ab-7cde-8f01-23456789abcd=record:2:01234567-89ab-7cde-8f01-23456789abce"
        )
        .is_err());
        assert!(super::parse_import_mapping(
            "entity:01234567-89ab-7cde-8f01-23456789abcd=layer:01234567-89ab-7cde-8f01-23456789abce"
        )
        .is_err());
    }
}
