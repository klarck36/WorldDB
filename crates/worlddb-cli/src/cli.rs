//! Versioned command-line interface and safe output boundary.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use worlddb_core::api::v1::{CURRENT_PROTOCOL, PublicCode, RequestId};

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

const HELP_ROOT: &str = "WorldDB CLI\n\nUsage: worlddb-cli [--format human|jsonl] <COMMAND>\n\nCommands:\n  v1 help                 Show version 1 command help\n  v1 version              Show CLI and protocol versions\n  v1 adapter run          Run an isolated import/export adapter\n  --help                  Show this help\n  --version               Show version information\n\nThe unversioned `adapter run` command remains available as a compatibility alias.";

const HELP_ADAPTER_RUN: &str = "Usage: worlddb-cli [--format human|jsonl] v1 adapter run --manifest <file> --input <file> --output <file> -- <adapter-executable> [arguments...]\n\nThe manifest binds the operation, deterministic seed, ID mapping, protocol capabilities, and process budgets. The adapter receives only framed stdin/stdout data; the output file is written only after a clean adapter exit.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Human,
    JsonLines,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelpScope {
    Root,
    AdapterRun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Success {
    Help(HelpScope),
    Version,
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
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"root\",\"usage\":\"worlddb-cli [--format human|jsonl] <COMMAND>\",\"commands\":[\"v1\",\"v1 version\",\"v1 adapter run\",\"help\",\"--version\"]}}}}}}"
        ),
        Success::Help(HelpScope::AdapterRun) => writeln!(
            writer,
            "{{\"cli_protocol\":{{\"major\":1,\"minor\":0}},\"request_id\":\"{request_id}\",\"outcome\":{{\"type\":\"help\",\"data\":{{\"scope\":\"adapter_run\",\"usage\":\"worlddb-cli [--format human|jsonl] v1 adapter run --manifest <file> --input <file> --output <file> -- <adapter-executable> [arguments...]\",\"required_options\":[\"--manifest\",\"--input\",\"--output\"],\"separator\":\"--\"}}}}}}"
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
    }
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
