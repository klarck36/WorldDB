use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use worlddb_cli::adapter_protocol::{
    AdapterManifest, AdapterProcessHost, MAX_ADAPTER_MANIFEST_ENCODED_BYTES,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("WorldDB CLI: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let Some(action) = arguments.next() else {
        print_help();
        return Ok(());
    };
    if action == "--help" || action == "-h" || action == "help" {
        print_help();
        return Ok(());
    }
    if action != "adapter" {
        return Err(String::from("expected the `adapter` command"));
    }
    let Some(subcommand) = arguments.next() else {
        return Err(String::from("expected `adapter run`"));
    };
    if subcommand == "--help" || subcommand == "-h" {
        print_adapter_help();
        return Ok(());
    }
    if subcommand != "run" {
        return Err(String::from("expected `adapter run`"));
    }
    run_adapter(arguments.collect())
}

fn run_adapter(arguments: Vec<OsString>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let mut manifest_path = None;
    let mut input_path = None;
    let mut output_path = None;
    let mut adapter_program = None;
    let mut adapter_arguments = Vec::new();

    while let Some(argument) = arguments.next() {
        if argument == "--" {
            adapter_program = arguments.next();
            adapter_arguments = arguments.collect();
            break;
        }
        let Some(flag) = argument.to_str() else {
            return Err(String::from("adapter options must be valid UTF-8"));
        };
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} requires a path"))?;
        let slot = match flag {
            "--manifest" => &mut manifest_path,
            "--input" => &mut input_path,
            "--output" => &mut output_path,
            _ => return Err(format!("unknown adapter option {flag}")),
        };
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(format!("{flag} may be specified only once"));
        }
    }

    let manifest_path = manifest_path.ok_or_else(|| String::from("--manifest is required"))?;
    let input_path = input_path.ok_or_else(|| String::from("--input is required"))?;
    let output_path = output_path.ok_or_else(|| String::from("--output is required"))?;
    let adapter_program =
        adapter_program.ok_or_else(|| String::from("adapter executable is required after `--`"))?;

    let manifest_bytes = read_bounded_file(
        &manifest_path,
        u64::try_from(MAX_ADAPTER_MANIFEST_ENCODED_BYTES)
            .map_err(|_| String::from("manifest size limit is unsupported on this platform"))?,
    )?;
    let manifest = AdapterManifest::decode(&manifest_bytes)
        .map_err(|error| format!("invalid adapter manifest: {error}"))?;
    let input = read_bounded_file(&input_path, manifest.budget().max_input_bytes())?;

    let mut command = Command::new(adapter_program);
    command.args(adapter_arguments);
    let output = AdapterProcessHost
        .run(command, &manifest, input)
        .map_err(|error| error.to_string())?;
    write_output(&output_path, output.bytes())?;
    println!(
        "adapter completed: process {}, protocol {}.{}, {} output bytes",
        output.adapter_process_id(),
        output.negotiated_protocol().major(),
        output.negotiated_protocol().minor(),
        output.bytes().len()
    );
    Ok(())
}

fn read_bounded_file(path: &Path, byte_limit: u64) -> Result<Vec<u8>, String> {
    let file =
        File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.len() > byte_limit {
        return Err(format!("{} exceeds its byte limit", path.display()));
    }
    let initial_capacity = usize::try_from(metadata.len())
        .map_err(|_| format!("{} is too large for this process", path.display()))?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(initial_capacity).map_err(|_| {
        format!(
            "cannot allocate a bounded read buffer for {}",
            path.display()
        )
    })?;
    file.take(byte_limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if u64::try_from(bytes.len()).map_err(|_| String::from("file length overflow"))? > byte_limit {
        return Err(format!("{} grew beyond its byte limit", path.display()));
    }
    Ok(bytes)
}

fn write_output(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file =
        File::create(path).map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn print_help() {
    println!(
        "WorldDB CLI\n\nCommands:\n  adapter run   Run a bounded import/export adapter process\n  --help        Show this help"
    );
}

fn print_adapter_help() {
    println!(
        "Usage: worlddb-cli adapter run --manifest <file> --input <file> --output <file> -- <adapter-executable> [arguments...]\n\nThe manifest binds the operation, deterministic seed, ID mapping, protocol capabilities, and process budgets. The adapter receives only framed stdin/stdout data; the output file is written only after a clean adapter exit."
    );
}
