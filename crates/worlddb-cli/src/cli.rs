//! Versioned command-line interface and safe output boundary.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use worlddb_core::api::v1::{CURRENT_PROTOCOL, PublicCode, RequestId};
use worlddb_core::{
    AuditPolicyFingerprint, AuthorizationDecision, AuthorizationMode, Bytes, Capability,
    DatabaseId, DomainId, PolicyTarget, PrincipalId,
};
use worlddb_storage_file::{
    BackupAuthenticity, BackupError, BackupProfile, BackupVerification, DatabaseLayout,
    ExactBackupManager, FormatProbeError, Manifest, ManifestSegmentKind, ManifestStore,
    RecoveryDisposition, RecoveryError, RecoveryManager, RestoreError, RestoreManager,
    SalvageError, SalvageInventorySource, SalvageManager, SalvageSegmentOutcome,
    SecurityPolicyHistorySnapshot, SecurityPolicyHistoryStore, StorageDamageClass,
    StorageFileError, StorageVerifier, StorageVerifyAction, StorageVerifyError,
    StorageVerifyReport, WriterLock, WriterLockError, verify_audit_complete_backup,
    verify_exact_backup,
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

const HELP_ROOT: &str = "WorldDB CLI\n\nUsage: worlddb-cli [--format human|jsonl] <COMMAND>\n\nCommands:\n  v1 help                    Show version 1 command help\n  v1 version                 Show CLI and protocol versions\n  v1 verify <database>       Read-only storage verification\n  v1 recovery inspect <db>   Read-only recovery and damage report\n  v1 recovery run --apply <db>  Explicit journaled recovery\n  v1 open --read-only <db>   Validate a database without writing\n  v1 salvage <source> --output <new-dir>  Copy verified data to a new fork\n  v1 backup create/verify    Create or verify Exact/AuditComplete backups\n  v1 restore clone           Restore a verified backup as a new database\n  v1 adapter run             Run an isolated import/export adapter\n  --help                     Show this help\n  --version                  Show version information\n\nThe unversioned `adapter run` command remains available as a compatibility alias.";

const HELP_ADAPTER_RUN: &str = "Usage: worlddb-cli [--format human|jsonl] v1 adapter run --manifest <file> --input <file> --output <file> -- <adapter-executable> [arguments...]\n\nThe manifest binds the operation, deterministic seed, ID mapping, protocol capabilities, and process budgets. The adapter receives only framed stdin/stdout data; the output file is written only after a clean adapter exit.";
const HELP_VERIFY: &str = "Usage: worlddb-cli [--format human|jsonl] v1 verify <database-directory>\n\nRuns read-only storage verification under a shared lock. The report includes safe_revision, disposition, observed damage classes, and safe next actions.";
const HELP_RECOVERY: &str = "Usage: worlddb-cli [--format human|jsonl] v1 recovery inspect <database-directory>\n       worlddb-cli [--format human|jsonl] v1 recovery run --apply <database-directory>\n\n`inspect` is read-only. `run --apply` explicitly enables journaled recovery of eligible WAL tails and committed snapshots.";
const HELP_OPEN_READ_ONLY: &str = "Usage: worlddb-cli [--format human|jsonl] v1 open --read-only <database-directory>\n\nValidates and verifies the database without creating files, changing storage, or enabling writes.";
const HELP_SALVAGE: &str = "Usage: worlddb-cli [--format human|jsonl] v1 salvage <source-directory> --output <new-directory>\n\nCopies verified immutable data to a new marked salvage fork. The source is opened with a shared read-only lock and is never repaired or rewritten.";
const HELP_BACKUP: &str = "Usage: worlddb-cli [--format human|jsonl] v1 backup create <source-directory> --output <new-directory> --profile exact|audit-complete --audit-scope excluded|included\n       worlddb-cli [--format human|jsonl] v1 backup verify <backup-directory> --profile exact|audit-complete --audit-scope excluded|included\n\nThe profile and matching audit scope are mandatory and repeated in every result. Exact excludes audit history. AuditComplete includes the supported raw-read audit prefix. Creation requires the current host-bound ProjectRead and BackupCreate capabilities; AuditComplete also requires AuditRead and AuditExport.";
const HELP_RESTORE: &str = "Usage: worlddb-cli [--format human|jsonl] v1 restore clone <backup-directory> --authorize-with <current-project-directory> --output <new-database-directory> --profile exact|audit-complete --audit-scope excluded|included\n\nRestore checks the current host-bound BackupRestore capability in the authorization project. AuditComplete also requires AuditRead and AuditExport. The authorization project must be the database named by the backup. Same-identity disaster recovery is not supported by the storage contract.";

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
        },
        OutputFormat::JsonLines => write_json_success(writer, request_id, success),
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
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"root\",\"usage\":\"worlddb-cli [--format human|jsonl] <COMMAND>\",\"commands\":[\"v1 verify\",\"v1 recovery inspect\",\"v1 recovery run --apply\",\"v1 open --read-only\",\"v1 salvage\",\"v1 backup\",\"v1 restore\",\"v1 adapter run\",\"help\",\"--version\"]}}}}}}"
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
        EXIT_UNAUTHORIZED, EXIT_UNSUPPORTED_OPERATION,
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
}
